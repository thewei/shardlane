//! [INPUT]: Depends on the ShardlaneApp type from the crate root (super) and existing types/imports (use super::*); no independent external dependencies.
//! [OUTPUT]: Exposes ShardlaneApp's terminal input pipeline: IME/key encoding/paste/clipboard/input queue and dispatch (inherent impl shard),
//! plus the terminal_keystroke_blocked guard predicate (keys don't enter the terminal while an input surface is focused).
//! [POS]: The `crates/herdr-gui` shell input responsibility domain, mechanically split out of main.rs; together with sibling shell_* modules it forms ShardlaneApp's method surface.
use super::*;

/// Terminal key dispatch guard (the observe_keystrokes path): when GUI key swallowing applies /
/// the terminal surface is occluded by an overlay / an input surface (steering composer, ⌘F find)
/// is focused, the key belongs to that input surface and must not be encoded into the terminal.
/// GPUI still dispatches keys to observe_keystrokes after the Input's KeyBinding actions (see
/// input.rs's should_swallow_gui_keystroke_with_config comment); this predicate is the sole interception point.
pub(crate) fn terminal_keystroke_blocked(
    gui_swallow: bool,
    surface_blocked: bool,
    input_surface_focused: bool,
) -> bool {
    gui_swallow || surface_blocked || input_surface_focused
}

/// Observer decision result (the testable seam of audit P1-1/P1-2): registry chords and input-surface
/// keys are consumed in place; Script hotkeys get exclusive dispatch; only the Terminal route is encoded into the host PTY.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShellKeyRoute<S> {
    Script(S),
    InputSurface,
    Terminal,
}

/// Pure decision: Script hotkeys take priority over the input-surface guard (review R2 P2 semantics —
/// a focused composer doesn't shadow script keys like F2-F12); only when both are empty does the host
/// TUI get the key. Chords hit by the registry return earlier in the caller and never reach this function.
pub(crate) fn route_shell_keystroke<S>(
    script_hit: Option<S>,
    input_surface_focused: bool,
) -> ShellKeyRoute<S> {
    if let Some(script_id) = script_hit {
        return ShellKeyRoute::Script(script_id);
    }
    if input_surface_focused {
        return ShellKeyRoute::InputSurface;
    }
    ShellKeyRoute::Terminal
}

impl ShardlaneApp {
    pub(super) fn clear_ime_state(&mut self) {
        self.ime_target = None;
        self.ime_marked_text.clear();
        self.ime_selected_range = None;
    }

    /// Whether the in-conversation find (⌘F) input holds focus (keys belong to the find box, not the PTY).
    pub(crate) fn conversation_find_focused(&self) -> bool {
        (self.history.open && self.history_find_focused())
            || (self.chat.model.mode == crate::chat::WorkSurfaceMode::Chat
                && self.chat.find_focused)
    }

    /// ⌘F routing: History open → find within History; otherwise Chat presentation mode → Chat.
    pub(crate) fn toggle_conversation_find(
        &mut self,
        _: &FindInConversation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.history.open {
            self.toggle_history_find(window, cx);
        } else if self.chat.model.mode == crate::chat::WorkSurfaceMode::Chat
            && self.chat.model.binding.is_some()
        {
            self.toggle_chat_find(window, cx);
        }
    }

    /// Find bar previous/next match (routed by the current surface).
    pub(crate) fn conversation_find_step(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        step: isize,
    ) {
        if self.history.open {
            self.history_find_step(cx, step);
        } else {
            self.chat_find_step(window, cx, step);
        }
    }

    /// Close the in-conversation find and return focus.
    pub(crate) fn conversation_find_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.open {
            self.history_find_close(cx);
        } else {
            self.chat_find_close(cx);
        }
        window.focus(&self.focus_handle);
        self.sync_terminal_application_focus(cx);
        cx.notify();
    }

    /// Find input focus tracking (find.rs on_focus/on_blur callbacks).
    pub(crate) fn set_conversation_find_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        if self.history.open {
            self.history_set_find_focused(focused);
        } else {
            self.chat.find_focused = focused;
        }
        self.sync_terminal_application_focus(cx);
        cx.notify();
    }

    pub(super) fn copy(&mut self, _: &Copy, window: &mut Window, cx: &mut Context<Self>) {
        // notate 08-29 round four: Markdown selection first — text drag-selected in Chat/History
        // bodies is copied with ⌘C (spans joined in document order); only with no selection does it
        // fall through to terminal selection copy.
        let mut selected = if self.chat.model.mode == crate::chat::WorkSurfaceMode::Chat {
            self.chat_transcript_selection_text()
        } else {
            None
        };
        if selected.is_none() && self.history.open {
            selected = self.history_transcript_selection_text();
        }
        if let Some(text) = selected.filter(|text| !text.trim().is_empty()) {
            cx.write_to_clipboard(crepuscularity_gpui::ClipboardItem::new_string(text));
            window.push_notification("Copied selection", cx);
            return;
        }
        if self.terminal_surface_blocked() {
            return;
        }
        let target = self
            .selection_target
            .as_ref()
            .filter(|target| self.selections.contains_key(*target))
            .cloned()
            .or_else(|| self.focused_terminal_target());
        // A27/B22: terminal copies get the same transient feedback as the markdown
        // paths above (one feedback model). When no terminal surface is focused at
        // all (input fields etc.), stay silent.
        if let Some(target) = target {
            if self.copy_selection_to_clipboard(&target, cx) {
                window.push_notification("Copied selection", cx);
            } else {
                window.push_notification("No terminal selection to copy", cx);
            }
        }
    }

    pub(super) fn copy_selection_to_clipboard(
        &mut self,
        target: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let trace = crate::terminal_trace::enabled();
        let trace_started = trace.then(Instant::now);
        let Some(selection) = self.selections.get(target).copied() else {
            return false;
        };
        let fallback_started = trace.then(Instant::now);
        let fallback_text = self.extract_selection_text(selection.0, selection.1);
        let fallback_us = fallback_started
            .map(crate::terminal_trace::elapsed_us)
            .unwrap_or(0);
        let semantic_selection = if target == herdr_tui::TUI_TARGET {
            self.tui_chrome_projection
                .visible_to_raw_selection(selection)
        } else {
            selection
        };
        let request = PendingTerminalCopy {
            selection: semantic_selection,
            fallback_text: fallback_text.clone(),
            generation: self.terminal_token,
        };

        if self.terminal_target.as_deref() == Some(target) && self.terminal.is_some() {
            self.pending_copy_selection = Some(request);
            // B20: wake the poll loop so a backoff idling cannot delay the copy by up to
            // one idle interval; the flush validates the generation stamp against the
            // current terminal token.
            if let Some(wake) = self.terminal_poll_wake.as_ref() {
                let _ = wake.try_send(());
            }
            if trace {
                crate::terminal_trace::event(format_args!(
                    "stage=ui.copy route=semantic_pending fallback_bytes={} fallback_us={fallback_us} total_us={}",
                    fallback_text.len(),
                    trace_started
                        .map(crate::terminal_trace::elapsed_us)
                        .unwrap_or(0),
                ));
            }
            return true;
        }
        if fallback_text.is_empty() {
            return false;
        }
        let bytes = fallback_text.len();
        cx.write_to_clipboard(crepuscularity_gpui::ClipboardItem::new_string(
            fallback_text,
        ));
        if trace {
            crate::terminal_trace::event(format_args!(
                "stage=ui.copy route=fallback bytes={bytes} fallback_us={fallback_us} total_us={}",
                trace_started
                    .map(crate::terminal_trace::elapsed_us)
                    .unwrap_or(0),
            ));
        }
        true
    }

    pub(super) fn select_all(
        &mut self,
        _: &SelectAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_surface_blocked() {
            return;
        }
        let Some(target) = self.focused_terminal_target() else {
            return;
        };
        let frame = self.terminal_frame_for_target();
        if frame.lines.is_empty() {
            return;
        }
        let last_row = frame.lines.len().saturating_sub(1).min(u16::MAX as usize) as u16;
        let last_col = frame
            .lines
            .last()
            .map(|line| line.cells.len().saturating_sub(1).min(u16::MAX as usize) as u16)
            .unwrap_or(0);
        self.selections
            .insert(target.clone(), ((0, 0), (last_col, last_row)));
        self.selection_target = Some(target);
        self.selection_dragged = true;
        self.selecting = false;
        self.sync_terminal_selection(cx);
        cx.notify();
    }

    pub(super) fn focused_terminal_target(&self) -> Option<String> {
        // A visible Lazygit surface temporarily owns input while the Herdr TUI remains
        // the primary runtime terminal.
        if self.is_lazygit_surface_active() && self.lazygit_session.terminal.is_some() {
            return Some(crate::right_panel::lazygit::LAZYGIT_TARGET.to_string());
        }
        (self.terminal_target.as_deref() == Some(herdr_tui::TUI_TARGET))
            .then(|| herdr_tui::TUI_TARGET.to_string())
    }

    pub(super) fn focused_has_selection(&self) -> bool {
        self.focused_terminal_target()
            .is_some_and(|target| self.selections.contains_key(&target))
    }

    pub(super) fn extract_selection_text(&self, start: (u16, u16), end: (u16, u16)) -> String {
        let frame = self.terminal_frame_for_target();
        let (start, end) = normalize_cell_selection(start, end);
        let mut result = String::new();
        for (row_idx, line) in frame.lines.iter().enumerate() {
            let row = row_idx as u16;
            if row < start.1 || row > end.1 {
                continue;
            }
            let first_col = if row == start.1 { start.0 as usize } else { 0 };
            let end_col = if row == end.1 {
                end.0 as usize
            } else {
                line.cells.len().saturating_sub(1)
            };
            if first_col < line.cells.len() {
                for cell in line
                    .cells
                    .iter()
                    .take(end_col.saturating_add(1))
                    .skip(first_col)
                {
                    result.push_str(cell);
                }
            }
            if row < end.1 {
                while result.ends_with(' ') {
                    result.pop();
                }
                result.push('\n');
            }
        }
        result.trim_end_matches(' ').to_string()
    }

    pub(super) fn paste(&mut self, _: &Paste, _window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_surface_blocked() {
            return;
        }
        let trace = crate::terminal_trace::enabled();
        let trace_started = trace.then(Instant::now);
        let clipboard_started = trace.then(Instant::now);
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let Some(text) = item.text() else {
            return;
        };
        let clipboard_us = clipboard_started
            .map(crate::terminal_trace::elapsed_us)
            .unwrap_or(0);
        let input_bytes = text.len();
        let Some(target) = self.focused_terminal_target() else {
            return;
        };
        // Contract (audit A33): `focused_terminal_target` can only return the hosted
        // targets (TUI / Lazygit) — no further re-validation is needed here.
        self.prepare_terminal_for_input(&target, cx);
        self.queue_terminal_paste(target.clone(), text, cx);
        if trace {
            crate::terminal_trace::event(format_args!(
                "stage=ui.paste bytes={input_bytes} clipboard_us={clipboard_us} total_us={}",
                trace_started
                    .map(crate::terminal_trace::elapsed_us)
                    .unwrap_or(0),
            ));
        }
    }

    pub(super) fn handle_keystroke(
        &mut self,
        key: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let trace_started = crate::terminal_trace::enabled().then(Instant::now);
        let trace_route = |route: &str| {
            if let Some(started) = trace_started {
                crate::terminal_trace::event(format_args!(
                    "stage=ui.key_route route={route} modified={} elapsed_us={}",
                    key.modifiers.control
                        || key.modifiers.alt
                        || key.modifiers.platform
                        || key.modifiers.function,
                    crate::terminal_trace::elapsed_us(started),
                ));
            }
        };
        // UX: recording state has highest priority — capture the next keystroke as the new chord (Esc cancels). The
        // action side is already suppressed by the full NoAction mask installed when recording began, so no double fire (see shortcuts_view).
        if let Some(id) = self.shortcut_recording {
            self.finish_shortcut_recording(key, id, window, cx);
            trace_route("shortcut_recording");
            return;
        }
        // Shortcut SSOT (audit 2026-08-28 P1-1): the observer has a single priority chain —
        // focused native control → resolved Shortcut Registry → Script hotkeys →
        // hosted Herdr TUI. A registry-hit chord has already had its action executed by GPUI
        // action dispatch; this code's only job is to keep that chord out of the host PTY (avoiding
        // action + terminal bytes double fire). There is no second hardcoded swallow table anymore;
        // chords dropped by override / disabled no longer hit the registry and pass through to the
        // host terminal per the existing P1-4 semantics.
        let gui_swallow =
            crate::input::should_swallow_gui_keystroke_with_config(key, &self.config.shortcuts);
        let surface_blocked = self.terminal_surface_blocked();
        if terminal_keystroke_blocked(gui_swallow, surface_blocked, false) {
            if let Some(started) = trace_started {
                crate::terminal_trace::event(format_args!(
                    "stage=ui.key_route route=blocked gui_swallow={gui_swallow} surface_blocked={surface_blocked} blocked_flags=[settings={} help={} history={} new_agent={} hist_search={} search={} about={} rename={} script_dialog={} nav_loading={}] modified={} elapsed_us={}",
                    self.show_settings,
                    self.show_help,
                    self.history.open,
                    self.new_agent_open,
                    self.history.search_open,
                    self.search_open,
                    self.about_open,
                    self.rename_open,
                    self.script_dialog_open,
                    self.navigation_loading,
                    key.modifiers.control
                        || key.modifiers.alt
                        || key.modifiers.platform
                        || key.modifiers.function,
                    crate::terminal_trace::elapsed_us(started),
                ));
            }
            return;
        }
        // Script hotkeys take priority over the input-surface guard (review R2 P2 semantics preserved); only when
        // both are empty does the host TUI get the key — neither Script nor input-surface routes encode any bytes to the PTY.
        match route_shell_keystroke(
            self.script_id_for_keybinding(key),
            self.steering_input_focused()
                || self.chat_prompt_focused()
                || self.history_prompt_focused()
                || self.conversation_find_focused(),
        ) {
            ShellKeyRoute::InputSurface => trace_route("input_surface"),
            ShellKeyRoute::Script(script_id) => {
                self.run_script_id(script_id, window, cx);
                trace_route("script");
            }
            ShellKeyRoute::Terminal => {
                if self.is_lazygit_surface_active() {
                    let encoded = self.handle_lazygit_keyboard(key, cx);
                    trace_route(if encoded {
                        "lazygit_encoded"
                    } else {
                        "lazygit_native_text"
                    });
                    return;
                }
                // Host TUI keyboard boundary: printable text stays with AppKit NSTextInputClient /
                // GPUI InputHandler (IME composition first); named keys and modifier chords are
                // Ghostty-encoded into the primary Herdr PTY; the visible Lazygit is handled by the
                // auxiliary route above.
                // Focus self-healing (acceptance-tested 2026-08-28): after a secondary page closes,
                // window focus may linger on a removed input (the TUI surface's mouse-down recovery
                // doesn't always fire) and character keys are then all lost. Any key heading to the
                // host TUI first confirms focus belongs to the root handle; from the next keystroke
                // the InputHandler resumes receiving input.
                let repaired_focus = window
                    .focused(cx)
                    .is_none_or(|handle| handle != self.focus_handle);
                if repaired_focus {
                    window.focus(&self.focus_handle);
                }
                let encoded = self.handle_tui_keyboard(key, cx);
                if let Some(started) = trace_started {
                    crate::terminal_trace::event(format_args!(
                        "stage=ui.key_route route={} repaired_focus={repaired_focus} elapsed_us={}",
                        if encoded {
                            "terminal_encoded"
                        } else {
                            "terminal_native_text"
                        },
                        crate::terminal_trace::elapsed_us(started),
                    ));
                }
            }
        }
    }

    /// Encode one keystroke with the host terminal's current Ghostty mode (sharing the same ordered
    /// byte channel as paste/mouse/focus).
    pub(super) fn encode_terminal_key_for(&self, target: &str, key: &Keystroke) -> Option<Vec<u8>> {
        let (terminal_key, unshifted) = ghostty_terminal_key(key)?;
        let key_encoder = if self.terminal_target.as_deref() == Some(target) {
            self.terminal_key_encoder.clone()
        } else if target == crate::right_panel::lazygit::LAZYGIT_TARGET {
            self.lazygit_key_encoder()
        } else {
            None
        }?;
        // The encoder is a tiny mode snapshot synchronized after every VT write. Named/modifier
        // key repeat therefore never waits on the terminal/frame mutex while a redraw is being
        // extracted; only concurrent key encoding contends on this dedicated encoder lock.
        let trace = crate::terminal_trace::enabled();
        let lock_started = trace.then(Instant::now);
        let mut key_encoder = key_encoder.lock().ok()?;
        let lock_wait_us = lock_started
            .map(crate::terminal_trace::elapsed_us)
            .unwrap_or(0);
        let encode_started = trace.then(Instant::now);
        let mut encoded = key_encoder
            .encode_key(
                terminal_key,
                TerminalModifiers {
                    // GPUI's macOS parser folds shifted number/punctuation keys
                    // into the key name (`Cmd+Shift+7` → `&`) and clears the
                    // Shift bit. Restore that consumed modifier before handing
                    // the physical key to Ghostty's encoder.
                    shift: key.modifiers.shift
                        || crate::input::terminal_key_uses_implicit_shift(key),
                    control: key.modifiers.control,
                    alt: key.modifiers.alt,
                    platform: key.modifiers.platform,
                },
                None,
                unshifted,
            )
            .ok()?;
        if encoded.is_empty() {
            if let Some(byte) = crate::input::legacy_alt_fallback_byte(key, unshifted) {
                // libghostty-vt deliberately leaves the macOS Option-as-Alt
                // policy to its host in legacy mode.  The hosted PTY has no
                // second host policy, so preserve the conventional ESC prefix
                // here and keep Alt+printable input lossless.
                encoded.extend_from_slice(&[0x1b, byte]);
            }
        }
        if trace {
            let key_class = if key.modifiers.control
                || key.modifiers.alt
                || key.modifiers.platform
                || key.modifiers.function
            {
                "modified"
            } else {
                "named"
            };
            crate::terminal_trace::event(format_args!(
                "stage=ui.key_encode class={key_class} bytes={} lock_wait_us={lock_wait_us} encode_us={}",
                encoded.len(),
                encode_started
                    .map(crate::terminal_trace::elapsed_us)
                    .unwrap_or(0),
            ));
        }
        Some(encoded)
    }

    fn mark_terminal_input_activity(&mut self, target: &str, cx: &mut Context<Self>) {
        self.last_terminal_input_at = Some(Instant::now());
        if self.selections.remove(target).is_some() {
            if self.selection_target.as_deref() == Some(target) {
                self.selection_target = None;
            }
            if self.terminal_target.as_deref() == Some(target) {
                self.terminal_pane
                    .update(cx, |pane, cx| pane.set_selection(None, 0, cx));
            } else if target == crate::right_panel::lazygit::LAZYGIT_TARGET {
                self.lazygit_session
                    .pane
                    .update(cx, |pane, cx| pane.set_selection(None, 0, cx));
            }
        }
    }

    pub(super) fn prepare_terminal_for_input(&mut self, target: &str, cx: &mut Context<Self>) {
        self.mark_terminal_input_activity(target, cx);
        // Keyboard/text input ends any in-progress wheel gesture. A later wheel event starts a
        // fresh pixel residual instead of inheriting stale sub-row distance across input modes.
        if self.terminal_target.as_deref() == Some(target) {
            self.terminal_scroll_residual_px = 0.0;
        } else if target == crate::right_panel::lazygit::LAZYGIT_TARGET {
            self.lazygit_session.scroll_residual_px = 0.0;
        }
    }

    pub(super) fn prepare_terminal_for_scroll_input(
        &mut self,
        target: &str,
        cx: &mut Context<Self>,
    ) {
        // Wheel/trackpad events must preserve the fractional pixel residual accumulated by
        // `scroll_rows_for_event`; clearing it here quantizes every event independently.
        self.mark_terminal_input_activity(target, cx);
    }

    pub(super) fn queue_terminal_text(
        &mut self,
        pane_id: String,
        target: String,
        text: String,
        cx: &mut Context<Self>,
    ) {
        self.queue_terminal_text_traced(pane_id, target, text, "text", cx);
    }

    pub(super) fn queue_terminal_text_traced(
        &mut self,
        pane_id: String,
        target: String,
        text: String,
        trace_kind: &'static str,
        cx: &mut Context<Self>,
    ) {
        if text.is_empty() {
            return;
        }
        if target == herdr_tui::TUI_TARGET || target == crate::right_panel::lazygit::LAZYGIT_TARGET
        {
            if let Some(control) = self.terminal_control_for_target(&target) {
                if let Err(error) = control.send_text_traced(&text, trace_kind) {
                    lag_log(format_args!("tui text input rejected: {error}"));
                }
                return;
            }
        }
        coalesce_terminal_input(
            &mut self.input_queue,
            TerminalInputCommand::Text {
                pane_id,
                target,
                text,
            },
        );
        self.dispatch_terminal_input(cx);
    }

    pub(super) fn terminal_control_for_target(&self, target: &str) -> Option<TerminalControlInput> {
        if self.terminal_target.as_deref() == Some(target) {
            self.terminal_input.clone()
        } else if target == crate::right_panel::lazygit::LAZYGIT_TARGET {
            self.lazygit_control()
        } else {
            None
        }
    }

    pub(super) fn managed_terminal_for_target(
        &self,
        target: &str,
    ) -> Option<Arc<Mutex<ManagedTerminal>>> {
        if self.terminal_target.as_deref() == Some(target) {
            self.terminal.clone()
        } else if target == crate::right_panel::lazygit::LAZYGIT_TARGET {
            self.lazygit_terminal()
        } else {
            None
        }
    }

    /// Hosted-target focus report (TUI / Lazygit only). The legacy RPC branch keeps
    /// working with the target standing in for the retired per-pane id (audit B15).
    pub(super) fn queue_terminal_focus(
        &mut self,
        target: &str,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(terminal) = self.managed_terminal_for_target(target) else {
            return false;
        };
        if target == herdr_tui::TUI_TARGET || target == crate::right_panel::lazygit::LAZYGIT_TARGET
        {
            let trace = crate::terminal_trace::enabled();
            let lock_started = trace.then(Instant::now);
            let mut terminal = match terminal.lock() {
                Ok(terminal) => terminal,
                Err(error) => {
                    lag_log(format_args!("tui focus encode rejected: {error}"));
                    return false;
                }
            };
            let lock_wait_us = lock_started
                .map(crate::terminal_trace::elapsed_us)
                .unwrap_or(0);
            let encode_started = trace.then(Instant::now);
            let encoded = match terminal.encode_focus(focused) {
                Ok(encoded) => encoded,
                Err(error) => {
                    lag_log(format_args!("tui focus encode rejected: {error}"));
                    return false;
                }
            };
            let encode_us = encode_started
                .map(crate::terminal_trace::elapsed_us)
                .unwrap_or(0);
            drop(terminal);
            if trace {
                crate::terminal_trace::event(format_args!(
                    "stage=ui.focus_encode focused={focused} bytes={} lock_wait_us={lock_wait_us} encode_us={encode_us}",
                    encoded.len(),
                ));
            }
            if encoded.is_empty() {
                return true;
            }
            let Some(control) = self.terminal_control_for_target(target) else {
                return false;
            };
            if let Err(error) = control.send_bytes_traced(&encoded, "focus") {
                lag_log(format_args!("tui focus input rejected: {error}"));
                return false;
            }
            return true;
        }
        self.input_queue.push_back(TerminalInputCommand::Focus {
            pane_id: target.to_string(),
            target: target.to_string(),
            terminal,
            focused,
        });
        self.dispatch_terminal_input(cx);
        true
    }

    /// Application-level terminal focus reporting: inject Ghostty focus escapes into the PTY only
    /// when the window's active state flips (active→In, inactive→Out). In-app navigation (switching
    /// Tab/Pane) does not inject — a focus escape is PTY input and would make the Agent TUI visibly
    /// repaint on the Herdr side, violating the navigation-decoupling principle.
    /// Cost: a previously viewed pane may hold a stale Focus-In until window deactivation corrects it.
    /// Application-level terminal focus reporting (the F1/F18-corrected state machine):
    /// - Inject Ghostty focus escapes only on window-active flips (active→In, inactive→Out);
    /// - In-app navigation doesn't inject (a focus escape is PTY input and would make the Agent TUI repaint on Herdr);
    /// - The reported state is valid only while the corresponding controller is alive; it is voided when
    ///   the controller is torn down, and the sync following the next attach sends In (F1);
    /// - With no pane to report, stay in the unreported state without poisoning it (F18); later syncs retry naturally.
    pub(super) fn sync_terminal_application_focus(&mut self, cx: &mut Context<Self>) {
        // While the steering composer holds focus the terminal input surface is inactive: don't report
        // FocusIn (honest ?1004 semantics), but frame projection continues (surface_blocked doesn't
        // include steering, avoiding a frozen picture).
        let terminal_surface_active = self.window_active
            && !self.terminal_surface_blocked()
            && !self.steering_input_focused();
        // Lifecycle self-check: the reported target controller was torn down → void the report; re-send after attach.
        if let Some((_, target)) = self.reported_terminal_focus.clone() {
            if !self.terminal_focus_report_is_live(&target) {
                self.reported_terminal_focus = None;
            }
        }
        let action = terminal_focus_report_action(
            terminal_surface_active,
            self.reported_terminal_focus.as_ref(),
        );
        match action {
            TerminalFocusReportAction::FocusIn => {
                // TUI-only: primary/auxiliary reporting targets are both decided by their own host state (stay unreported while not running).
                let focus_target = if self.terminal_target.as_deref() == Some(herdr_tui::TUI_TARGET)
                {
                    Some(herdr_tui::TUI_TARGET.to_string())
                } else {
                    None
                };
                if let Some(target) = focus_target {
                    // Queueing fails while the controller isn't ready: stay unreported and let later syncs retry.
                    if self.queue_terminal_focus(&target, true, cx) {
                        self.reported_terminal_focus = Some((target.clone(), target));
                    }
                }
                // No pane/host: stay unreported; later syncs retry naturally.
            }
            TerminalFocusReportAction::FocusOut => {
                if let Some((_, target)) = self.reported_terminal_focus.take() {
                    let _ = self.queue_terminal_focus(&target, false, cx);
                }
            }
            TerminalFocusReportAction::None => {}
        }
        // Local projection recovery keeps the original semantics: refresh the frame when active with no reported focus (purely local, no Herdr side effects).
        if terminal_surface_active && self.reported_terminal_focus.is_none() {
            self.resume_terminal_projection(cx);
        }
    }

    /// Whether the reported target is still the current host terminal.
    pub(super) fn terminal_focus_report_is_live(&self, target: &str) -> bool {
        self.terminal_target.as_deref() == Some(target)
    }

    pub(super) fn resume_terminal_projection(&mut self, cx: &mut Context<Self>) {
        self.terminal_pending_frame = true;
        let _ = self.maybe_refresh_terminal_frame(cx, true);
    }

    /// Hosted-target paste (TUI / Lazygit only). The legacy RPC branch keeps working with
    /// the target standing in for the retired per-pane id (audit B15).
    pub(super) fn queue_terminal_paste(
        &mut self,
        target: String,
        text: String,
        cx: &mut Context<Self>,
    ) {
        if text.is_empty() {
            return;
        }
        if target == herdr_tui::TUI_TARGET || target == crate::right_panel::lazygit::LAZYGIT_TARGET
        {
            let Some(terminal) = self.managed_terminal_for_target(&target) else {
                return;
            };
            let trace = crate::terminal_trace::enabled();
            let lock_started = trace.then(Instant::now);
            let mut terminal = match terminal.lock() {
                Ok(terminal) => terminal,
                Err(error) => {
                    lag_log(format_args!("tui paste encode rejected: {error}"));
                    return;
                }
            };
            let lock_wait_us = lock_started
                .map(crate::terminal_trace::elapsed_us)
                .unwrap_or(0);
            let encode_started = trace.then(Instant::now);
            let encoded = match terminal.encode_paste(&text) {
                Ok(encoded) => encoded,
                Err(error) => {
                    lag_log(format_args!("tui paste encode rejected: {error}"));
                    return;
                }
            };
            let encode_us = encode_started
                .map(crate::terminal_trace::elapsed_us)
                .unwrap_or(0);
            drop(terminal);
            if trace {
                crate::terminal_trace::event(format_args!(
                    "stage=ui.paste_encode input_bytes={} encoded_bytes={} lock_wait_us={lock_wait_us} encode_us={encode_us}",
                    text.len(),
                    encoded.len(),
                ));
            }
            if let Some(control) = self.terminal_control_for_target(&target) {
                if let Err(error) = control.send_bytes_traced(&encoded, "paste") {
                    lag_log(format_args!("tui paste input rejected: {error}"));
                }
            }
            return;
        }
        self.input_queue.push_back(TerminalInputCommand::Paste {
            pane_id: target.clone(),
            target,
            text,
        });
        self.dispatch_terminal_input(cx);
    }

    /// Hosted-target raw byte enqueue (TUI / Lazygit only; keys/mouse/wheel/alt-scroll).
    /// The legacy RPC fallback keeps working with the target standing in for the retired
    /// per-pane id (audit B15).
    pub(super) fn queue_terminal_raw_bytes_traced(
        &mut self,
        target: String,
        bytes: Vec<u8>,
        trace_kind: &'static str,
        cx: &mut Context<Self>,
    ) {
        if bytes.is_empty() {
            return;
        }
        // Hosted TUI is a local PTY, not a Herdr controller RPC. Its PtyHandle owns a
        // dedicated ordered writer thread, so enqueue bytes there directly instead of
        // wrapping every key/mouse packet in a background task + root-state callback.
        // This is especially important for precision trackpads, where one gesture can
        // produce many wheel packets in a few milliseconds.
        if target == herdr_tui::TUI_TARGET || target == crate::right_panel::lazygit::LAZYGIT_TARGET
        {
            if let Some(control) = self.terminal_control_for_target(&target) {
                if let Err(error) = control.send_bytes_traced(&bytes, trace_kind) {
                    lag_log(format_args!("tui input rejected: {error}"));
                }
                return;
            }
        }
        self.input_queue.push_back(TerminalInputCommand::RawBytes {
            pane_id: target.clone(),
            target,
            bytes,
        });
        self.dispatch_terminal_input(cx);
    }

    pub(super) fn queue_terminal_key(
        &mut self,
        pane_id: String,
        key: String,
        cx: &mut Context<Self>,
    ) {
        coalesce_terminal_input(
            &mut self.input_queue,
            TerminalInputCommand::Keys {
                pane_id,
                keys: vec![key],
            },
        );
        self.dispatch_terminal_input(cx);
    }

    pub(super) fn dispatch_terminal_input(&mut self, cx: &mut Context<Self>) {
        if self.input_in_flight {
            return;
        }
        let Some(command) = self.input_queue.pop_front() else {
            return;
        };
        let Some(client) = self.client.clone() else {
            self.input_queue.clear();
            return;
        };
        let control = match &command {
            TerminalInputCommand::Text { target, .. }
            | TerminalInputCommand::Paste { target, .. }
            | TerminalInputCommand::Focus { target, .. }
            | TerminalInputCommand::RawBytes { target, .. } => {
                self.terminal_control_for_target(target)
            }
            TerminalInputCommand::Keys { .. } => None,
        };
        let paste_terminal = match &command {
            TerminalInputCommand::Paste { target, .. } => self.managed_terminal_for_target(target),
            _ => None,
        };
        self.input_in_flight = true;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    match command {
                        TerminalInputCommand::Text {
                            pane_id,
                            target: _,
                            text,
                        } => match control {
                            Some(control) => control
                                .send_text(&text)
                                .map_err(TerminalInputFailure::input),
                            None => client
                                .send_text(&pane_id, &text)
                                .map_err(classify_input_failure),
                        },
                        TerminalInputCommand::Paste {
                            pane_id,
                            target: _,
                            text,
                        } => {
                            let encoded = if let Some(terminal) = paste_terminal {
                                terminal
                                    .lock()
                                    .map_err(TerminalInputFailure::input)?
                                    .encode_paste(&text)?
                            } else {
                                text.into_bytes()
                            };
                            match control {
                                Some(control) => control
                                    .send_bytes(&encoded)
                                    .map_err(TerminalInputFailure::input),
                                None => {
                                    let encoded = String::from_utf8(encoded)
                                        .map_err(TerminalInputFailure::input)?;
                                    client
                                        .send_text(&pane_id, &encoded)
                                        .map_err(classify_input_failure)
                                }
                            }
                        }
                        TerminalInputCommand::Focus {
                            pane_id,
                            target: _,
                            terminal,
                            focused,
                        } => {
                            let encoded = terminal
                                .lock()
                                .map_err(TerminalInputFailure::input)?
                                .encode_focus(focused)?;
                            if encoded.is_empty() {
                                return Ok(());
                            }
                            match control {
                                Some(control) => control
                                    .send_bytes(&encoded)
                                    .map_err(TerminalInputFailure::input),
                                None => {
                                    let encoded = String::from_utf8(encoded)
                                        .map_err(TerminalInputFailure::input)?;
                                    client
                                        .send_text(&pane_id, &encoded)
                                        .map_err(classify_input_failure)
                                }
                            }
                        }
                        TerminalInputCommand::RawBytes {
                            pane_id,
                            target: _,
                            bytes,
                        } => match control {
                            Some(control) => control
                                .send_bytes(&bytes)
                                .map_err(TerminalInputFailure::input),
                            None => {
                                let text = String::from_utf8(bytes)
                                    .map_err(TerminalInputFailure::input)?;
                                client
                                    .send_text(&pane_id, &text)
                                    .map_err(classify_input_failure)
                            }
                        },
                        TerminalInputCommand::Keys { pane_id, keys } => client
                            .send_keys(&pane_id, &keys)
                            .map_err(classify_input_failure),
                    }
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.input_in_flight = false;
                if let Err(failure) = result {
                    match failure {
                        TerminalInputFailure::Connection(reason) => {
                            view.status = ConnectionStatus::Offline(reason);
                        }
                        TerminalInputFailure::Input(reason) => {
                            lag_log(format_args!("terminal input rejected: {reason}"));
                        }
                    }
                }
                view.dispatch_terminal_input(cx);
            });
        })
        .detach();
    }
}
