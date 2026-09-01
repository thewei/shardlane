//! [INPUT]: Depends on the ShardlaneApp type and imports from the crate root (super), `herdr_tui`
//!           (ordinary `herdr` spawn/system config inheritance/navigation/chrome projection), `ManagedTerminal::host_process`
//!           (PTY transport), `poll_managed_terminal` (frame polling), and the
//!           resolved Shortcut Registry (unified key swallowing via shell_input::handle_keystroke).
//! [OUTPUT]: Exposes ShardlaneApp's Herdr TUI host lifecycle:
//!           `ensure_tui_surface`/`attach_herdr_tui`/
//!           `dispatch_tui_focus`/`tui_agent_terminal_for`/`exit_tui_surface`/
//!           `restart_tui_surface`/`handle_tui_keyboard` (the observer-driven keyboard boundary)/
//!           `handle_tui_scroll_wheel`/`handle_tui_mouse_down`/
//!           `restore_tui_grid_on_activation` (activation = the shared-grid width-ownership
//!           signal; navigation probes are crop-only)/
//!           `tui_key_encodes_to_pty`/`text_is_terminal_control_payload`/
//!           `tui_native_context_menu_owns_button` (pure classification functions), and
//!           `TUI_RESPAWN_COOLDOWN`.
//! [POS]: The `crates/herdr-gui` shell primary TUI host responsibility domain (spawn/restart/exit/
//!           shared-focus navigation/terminal-native input boundary of the singleton PTY child),
//!           mechanically split out of main.rs; rendering belongs to shell_render.rs's tui_surface_view
//!           and policy to herdr_tui.rs.
//!           Keyboard dispatch must go through the app-global keystroke observer (GPUI 0.2.2 key
//!           events only bubble along the root track_focus focus path; deep listeners inside the
//!           content area never receive key down; wheel/mouse go through hit-test and are unaffected).
//!           Printable text is still owned by AppKit NSTextInputClient/GPUI InputHandler (IME composition
//!           comes first); on the InputHandler side, text_is_terminal_control_payload drops "\n"/"\t"
//!           control payloads to prevent double sends.
use super::*;

/// Respawn cooldown after the host child process exits or spawn fails: prevents infinite loops; navigation/restart can punch through.
const TUI_RESPAWN_COOLDOWN: Duration = Duration::from_secs(3);

/// Shift+wheel is intentionally left to the parent surface (horizontal scroll
/// and native container behavior). It must not enter the terminal's vertical
/// residual accumulator or steal propagation.
fn terminal_wheel_uses_vertical_scroll(event: &ScrollWheelEvent) -> bool {
    !event.modifiers.shift
}

/// Whether a keystroke must be encoded into the hosted PTY by the Ghostty key encoder.
///
/// Ownership split (verified against gpui 0.2.2 `parse_keystroke`): classification MUST be
/// by key name + modifiers, never by `key_char`, because GPUI fills `key_char` for Enter
/// (`"\n"`) and Tab (`"\t") as well — treating those as native text leaves them encoded by
/// nobody and the key dies. Named terminal keys and any ctrl/alt/cmd/function chord belong
/// to the Ghostty encoder; printable text (incl. shifted/space) stays on AppKit's
/// NSTextInputClient path, which is also what lets pinyin/kana compose before any PTY byte.
pub(crate) fn tui_key_encodes_to_pty(key: &Keystroke) -> bool {
    if key.modifiers.control
        || key.modifiers.alt
        || key.modifiers.platform
        || key.modifiers.function
    {
        return true;
    }
    // Unmodified (shift allowed): every terminal command key has a multi-character GPUI
    // name (`enter`, `tab`, `backspace`, `escape`, arrows, f-keys...), while printable text
    // always arrives as a single-character name or `space`. So: encode everything except
    // single-char/space names.
    !(key.key == "space" || key.key.chars().count() == 1)
}

/// Keys GPUI reports with a control `key_char` even though the Ghostty encoder already sent
/// them (`enter` → "\n", `tab` → "\t"). The InputHandler must not re-insert those or named
/// keys double-fire. IME commit payloads are natural language and never control-only.
pub(crate) fn text_is_terminal_control_payload(text: &str) -> bool {
    !text.is_empty() && text.chars().all(char::is_control)
}

/// Plain right-click belongs to Shardlane's native terminal context menu. It must never be
/// encoded into the hosted Herdr TUI, otherwise Herdr opens its own TUI menu underneath the
/// native GPUI menu and a right-button drag can emit an orphan SGR motion sequence.
pub(crate) fn tui_native_context_menu_owns_button(button: &MouseButton) -> bool {
    matches!(button, MouseButton::Right)
}

impl ShardlaneApp {
    /// Translate Shardlane's visible Pane grid into the larger raw Herdr TUI grid that
    /// also contains hidden navigation chrome. `terminal_size` remains the visible SSOT;
    /// only the hosted PTY/Ghostty transport receives this compensated size.
    ///
    /// The chrome allowance comes from the stored chrome projection: Herdr's navigation
    /// chrome is fixed-width (sidebar, tab bar) and the Pane absorbs the rest, so those
    /// margins are invariant under the imposition. Deliberately NOT derived from a fresh
    /// `pane.layout` area here — an area fetched asynchronously can race the adopted grid
    /// it must be measured against and produce garbage margins.
    pub(super) fn tui_raw_terminal_size(&self, visible: TerminalSize) -> TerminalSize {
        let (cols, rows) = self.tui_chrome_projection.outer_grid(visible.0, visible.1);
        let extra_cols = u32::from(cols.saturating_sub(visible.0));
        let extra_rows = u32::from(rows.saturating_sub(visible.1));
        let pixel_width = u32::from(visible.2)
            .saturating_add((self.terminal_cell_width() * f64::from(extra_cols)).round() as u32)
            .min(u32::from(u16::MAX)) as u16;
        let pixel_height = u32::from(visible.3)
            .saturating_add((self.terminal_cell_height() * f64::from(extra_rows)).round() as u32)
            .min(u32::from(u16::MAX)) as u16;
        (cols, rows, pixel_width, pixel_height)
    }

    pub(super) fn tui_visible_to_raw_cell(&self, cell: (u16, u16)) -> (u16, u16) {
        self.tui_chrome_projection.visible_to_raw_cell(cell)
    }

    pub(super) fn tui_raw_to_visible_selection(
        &self,
        selection: TerminalSelection,
    ) -> Option<TerminalSelection> {
        self.tui_chrome_projection
            .raw_to_visible_selection(selection)
    }

    /// Apply Herdr's authoritative Pane-area geometry to the hosted presentation. The
    /// server/runtime still renders the complete TUI; Shardlane only changes the visible
    /// projection and compensates the raw PTY size so the Pane keeps the full client grid.
    ///
    /// `reimpose=false` is the shared-geometry adoption path (another viewer resized the
    /// global session): update only the crop — re-imposing our compensated grid here
    /// would fight the viewer that owns the new global geometry (last writer wins).
    pub(super) fn apply_tui_chrome_area(
        &mut self,
        area: LayoutRect,
        reimpose: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.terminal_target.as_deref() != Some(herdr_tui::TUI_TARGET) {
            return false;
        }
        let Some(visible) = self.terminal_surface_size.or(self.terminal_size) else {
            return false;
        };
        // Chrome margins are always measured in the grid the viewer model actually
        // holds; they are invariant under the desktop's own imposition, so both the
        // crop-only (adoption/navigation) and reimpose paths share this computation.
        let (adopted_cols, adopted_rows) = self.tui_adopted_grid.unwrap_or((visible.0, visible.1));
        let Some(next) =
            herdr_tui::TuiChromeProjection::from_layout(adopted_cols, adopted_rows, area)
        else {
            return false;
        };
        let projection_changed = next != self.tui_chrome_projection;
        if projection_changed {
            lag_log(format_args!(
                "tui.chrome projection left={} top={} right={} bottom={} grid={}x{} pane={}x{} reimpose={}",
                next.left, next.top, next.right, next.bottom, adopted_cols, adopted_rows,
                area.width, area.height, reimpose,
            ));
            self.tui_chrome_projection = next;
        }
        if !reimpose {
            if !projection_changed {
                return false;
            }
            // Re-project the already-adopted raw grid under the new crop. No extraction
            // happened, so there is no row plan; the Arc clone keeps the raw compare at ptr_eq.
            let raw_frame = self.terminal_raw_frame.clone();
            self.set_terminal_frame(raw_frame, None, cx);
            return true;
        }
        // Reimpose always forces the raw resize, even when the crop is unchanged:
        // the shared session may hold any grid after Remote/Mobile resizes, and a
        // same-size TIOCSWINSZ is a cheap no-op for the child.
        self.reimpose_tui_grid(visible, None, cx);
        true
    }

    /// Force the shared Herdr TUI session back to this desktop's compensated grid even
    /// when the visible grid is unchanged: after a Remote/Mobile resize the session may
    /// hold any grid, and a same-size TIOCSWINSZ is a cheap no-op for the child. Shared
    /// by both re-imposition paths (audit B13): invalidates the cached visible size so
    /// the forced resize cannot early-return, then triggers it — `Some(pane_id)` keeps
    /// the activation path's fresh `pane.layout` probe (chrome margins are measured
    /// against the grid the model holds at probe time, never a synchronous resize with a
    /// possibly-stale area), `None` resizes to `visible` immediately.
    fn reimpose_tui_grid(
        &mut self,
        visible: TerminalSize,
        probe_pane_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.terminal_size = None;
        if let Some(pane_id) = probe_pane_id {
            self.schedule_tui_chrome_probe(Some(pane_id), true, cx);
        } else {
            self.resize_main_terminal_to_size(visible, cx);
        }
    }

    /// Short-lived geometry probe used after host attach/focus. Layout updates remain the
    /// normal event-driven source; this is only a startup/focus race fallback and never polls.
    /// `reimpose` forwards to `apply_tui_chrome_area`.
    pub(super) fn schedule_tui_chrome_probe(
        &mut self,
        pane_id: Option<String>,
        reimpose: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(pane_id) = pane_id.or_else(|| self.state.focused_pane_id.clone()) else {
            return;
        };
        let Some(client) = self.client.clone() else {
            return;
        };
        let token = self.terminal_token;
        self._tui_projection_task = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
            let result = cx
                .background_executor()
                .spawn(async move { client.pane_layout(&pane_id) })
                .await;
            let Ok(layout) = result else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                if view.terminal_token == token {
                    view.apply_tui_chrome_area(layout.area, reimpose, cx);
                }
            });
        });
    }

    /// Whether the respawn cooldown is active.
    fn tui_respawn_blocked(&self) -> bool {
        self.tui_respawn_blocked_until
            .is_some_and(|until| Instant::now() < until)
    }

    /// Ensure the TUI host surface exists (spawn when absent and not cooling down). Idempotent:
    /// no repeat spawn while a host exists or attach is in flight. `force` punches through the
    /// cooldown (user navigation / explicit restart).
    pub(super) fn ensure_tui_surface(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        force: bool,
    ) {
        // Unbound window (⌘N on the picker) or binding still bootstrapping: no
        // TUI surface exists to attach to yet.
        if self.binding.is_none() {
            return;
        }
        if self.terminal.is_some() {
            return;
        }
        if !force && self.tui_respawn_blocked() {
            return;
        }
        self.attach_herdr_tui(window, cx, false);
    }

    /// TUI host attach (post-A01): no longer spawns a private child process; instead it attaches to
    /// the Host-owned shared Herdr TUI session (one child + one PTY per Host process;
    /// Remote/Mobile viewers attach to the same session). Locally we keep a viewer-specific Ghostty
    /// model/viewport that consumes the shared bytes; input/resize go through the shared session's
    /// serial seam. `restart` uses manager.restart() (a genuinely new-generation process); otherwise
    /// open() reuses it idempotently.
    ///
    /// Entering the TUI synchronously releases all Embedded per-pane controllers (the resource goal
    /// of this mode); Shardlane never takes over a controller.
    pub(super) fn attach_herdr_tui(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        restart: bool,
    ) {
        // Unbound window: nothing to host (the Project picker is showing).
        if self.binding.is_none() {
            return;
        }
        // Single surface: clean up multi-pane controller state (the TUI draws all panes itself).
        // Hosted Herdr TUI is already a complete terminal UI. Do not overlay Shardlane's
        // Embedded scrollback scrollbar on top of it; that produced the dotted/double
        // scrollbar reported in manual testing.
        if self.terminal_target.as_deref() == Some(herdr_tui::TUI_TARGET) {
            self.sync_terminal_geometry(window, cx);
            return;
        }
        // in-flight guard: no repeat spawn while attach is starting (multiple navigations during
        // startup would fire attach repeatedly, otherwise a dual-PTY race).
        if self.terminal_attach_target.is_some() {
            return;
        }
        if self.tui_respawn_blocked() {
            return;
        }
        if let Some(client) = self.client.as_ref() {
            if !herdr_tui::protocol_supported(client) {
                // The explicit compatibility gate for TUI-only: an old protocol does not fall back to
                // Embedded; give an actionable upgrade hint (plan §19 Protocol too old).
                let detected = client.protocol().unwrap_or(0);
                self.tui_host.set_failed(format!(
                    "Herdr needs an update — connected protocol {detected}, Shardlane requires protocol 20+"
                ));
                self.tui_respawn_blocked_until = Some(Instant::now() + TUI_RESPAWN_COOLDOWN);
                cx.notify();
                return;
            }
        }
        let size = self
            .terminal_surface_size
            .unwrap_or_else(|| self.terminal_size(window));
        let raw_size = self.tui_raw_terminal_size(size);
        // Shardlane is the hosted terminal emulator: resolve the theme-derived dynamic
        // colors on the UI thread (window appearance aware) and hold the pane fill so
        // bootstrap paint never flashes the vendored dark fallback.
        let hosted_colors = self.hosted_terminal_colors_for_window(window);
        self.hosted_terminal_colors = Some(hosted_colors);
        self.terminal_pane.update(cx, |pane, cx| {
            pane.set_placeholder_background(hosted_colors.1, cx);
        });
        self.reset_terminal_scroll_state();
        self.terminal_token = self.terminal_token.wrapping_add(1);
        let token = self.terminal_token;
        self.terminal = None;
        self.terminal_key_encoder = None;
        self.terminal_input = None;
        self.terminal_target = None;
        self.terminal_attach_target = Some(herdr_tui::TUI_TARGET.to_string());
        self.terminal_size = Some(size);
        self.tui_adopted_grid = None;
        self.tui_host.begin_restart();
        self.set_terminal_frame(Arc::new(TerminalFrame::default()), None, cx);

        let shared_tui = self.tui_manager.clone();
        // Socket resolution: B1 SSH bridge first (remote machines), then the
        // bound session's own socket (herdr's `default` session = base socket).
        let socket_override = self.binding.as_ref().and_then(|binding| {
            binding.socket_override.clone().or_else(|| {
                Some(shardlane_host::herdr::session_socket_path_for(
                    binding.session_name(),
                ))
            })
        });
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    // A01: attach the Host-owned shared Herdr TUI session. This
                    // spawn call creates the one child only when no running
                    // session exists; Remote viewers attach the same object.
                    let socket_override = socket_override.as_deref();
                    let session = if restart {
                        shared_tui.restart(raw_size.0, raw_size.1, socket_override)
                    } else {
                        shared_tui.open(raw_size.0, raw_size.1, socket_override)
                    }
                    .map_err(|error| format!("open shared Herdr TUI: {error}"))?;
                    let mut managed = ManagedTerminal::attach_shared(
                        &session,
                        raw_size.0,
                        raw_size.1,
                        crate::ghostty::LOCAL_SCROLLBACK_LINES as usize,
                    )?;
                    // Adopt this viewer's geometry as the authoritative shared
                    // size (SIGWINCH repaint) and force a full repaint for the
                    // fresh local model even when the size already matched.
                    session
                        .resize(raw_size.0, raw_size.1)
                        .map_err(|error| format!("shared TUI resize: {error}"))?;
                    session
                        .force_redraw()
                        .map_err(|error| format!("shared TUI redraw: {error}"))?;
                    // Seed the emulator's dynamic defaults before the first frame
                    // extraction; the child's own OSC queries are answered by the
                    // poll loop once its output starts flowing.
                    managed.set_dynamic_colors(hosted_colors.0, hosted_colors.1);
                    // Startup readiness check (audit TUI blueprint): child process alive + stream read
                    // loop alive + surface initialized successfully, rather than declaring Running the
                    // moment spawn returns.
                    let wake = managed.wake_receiver();
                    let input = managed.input_handle();
                    let key_encoder = managed.key_encoder_handle();
                    let poll_wake = managed.wake_sender();
                    let frame = managed.frame_reusing(None).unwrap_or_default();
                    Ok::<_, String>((managed, wake, input, key_encoder, poll_wake, frame))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.terminal_token != token {
                    return;
                }
                view.terminal_attach_target = None;
                match result {
                    Ok((managed, wake, input, key_encoder, poll_wake, frame)) => {
                        view.terminal = Some(Arc::new(Mutex::new(managed)));
                        view.terminal_key_encoder = Some(key_encoder);
                        view.terminal_input = Some(input);
                        view.terminal_target = Some(herdr_tui::TUI_TARGET.to_string());
                        view.terminal_size = Some(size);
                        // The viewer model was created at the compensated attach grid and
                        // the session was resized to it below; record it as the adopted
                        // grid so crop-only probes compute margins against reality.
                        view.tui_adopted_grid = Some((raw_size.0, raw_size.1));
                        view.tui_host.set_running();
                        view.tui_respawn_blocked_until = None;
                        // B20: pended copy-on-select can wake this poll loop directly.
                        view.terminal_poll_wake = Some(poll_wake);
                        // Attach bootstrap frame: no row plan (signatures were just seeded).
                        view.set_terminal_frame(Arc::new(frame), None, cx);
                        view.sync_terminal_application_focus(cx);
                        view.schedule_tui_chrome_probe(
                            view.state.focused_pane_id.clone(),
                            true,
                            cx,
                        );
                        poll_managed_terminal(token, herdr_tui::TUI_TARGET.to_string(), wake, cx);
                    }
                    Err(err) => {
                        // Shared teardown reset list (audit B04): also clears the retained
                        // surface size/frame — the attach-failure arm is exactly the site
                        // that used to forget them.
                        view.reset_hosted_tui_state(true, cx);
                        view.tui_respawn_blocked_until =
                            Some(Instant::now() + TUI_RESPAWN_COOLDOWN);
                        view.tui_host.set_failed(err);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// TUI-mode navigation dispatch: drive the focus chain so the host TUI follows sidebar navigation.
    /// The last click wins (the previous task handle is dropped, cancelling it).
    pub(super) fn dispatch_tui_focus(
        &mut self,
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: Option<String>,
        agent_terminal_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let projection_pane_id = pane_id.clone();
        self._tui_focus_task = cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    herdr_tui::execute_focus_plan(
                        &client,
                        workspace_id.as_deref(),
                        tab_id.as_deref(),
                        pane_id.as_deref(),
                        agent_terminal_id.as_deref(),
                    )
                })
                .await;
            match outcome {
                Ok(()) => {
                    let _ = this.update(cx, |view, cx| {
                        // Tab/Pane navigation is view switching, not client activation:
                        // refresh only the crop, never re-impose the shared grid width.
                        view.schedule_tui_chrome_probe(projection_pane_id, false, cx);
                    });
                }
                Err(error) => {
                    lag_log(format_args!("tui.focus chain: {error}"));
                    // B22: a failed navigation chain used to be lag-log-only; surface a
                    // non-blocking toast so the user knows the click had no effect.
                    let window_handle =
                        this.update(cx, |view, _| view.window_handle).ok().flatten();
                    if let Some(window_handle) = window_handle {
                        let _ = cx.update_window(window_handle, |_, window, cx| {
                            window.push_notification(
                                format!("Herdr focus navigation failed: {error}"),
                                cx,
                            );
                        });
                    }
                }
            }
        });
    }

    /// Whether the shared session grid currently differs from this window's target grid:
    /// the header "Restore Width" fallback button is only offered when a restore is due.
    /// A lost adopted-grid record also counts as needing a restore — that is exactly the
    /// desync the button exists to rescue.
    pub(super) fn tui_width_restore_needed(&self) -> bool {
        if self.terminal_target.as_deref() != Some(herdr_tui::TUI_TARGET) {
            return false;
        }
        let Some(visible) = self.terminal_surface_size.or(self.terminal_size) else {
            return false;
        };
        let (cols, rows, _, _) = self.tui_raw_terminal_size(visible);
        self.tui_adopted_grid != Some((cols, rows))
    }

    /// Window activation and the header "Restore Width" button are the width-ownership
    /// signals for the shared Herdr TUI session: whichever client activates restores its
    /// own grid ("last activated client wins"), so returning to Shardlane after a Remote
    /// viewer resized the session re-imposes the desktop's compensated grid. Mere
    /// navigation never calls this.
    ///
    /// The re-imposition deliberately runs through a fresh `pane.layout` probe instead of
    /// a synchronous resize: chrome margins measured against the grid the model holds at
    /// probe time are invariant under the imposition, while a synchronous impose with a
    /// possibly-stale area can land on the un-compensated grid and get stuck there.
    pub(super) fn restore_tui_grid_on_activation(&mut self, cx: &mut Context<Self>) {
        if self.terminal_target.as_deref() != Some(herdr_tui::TUI_TARGET) {
            return;
        }
        let Some(visible) = self.terminal_surface_size.or(self.terminal_size) else {
            return;
        };
        lag_log(format_args!(
            "tui.activation restore requested adopted={:?}",
            self.tui_adopted_grid,
        ));
        self.reimpose_tui_grid(visible, self.state.focused_pane_id.clone(), cx);
    }

    /// When a pane hosts an Agent, return the direct agent.focus target (terminal_id).
    pub(super) fn tui_agent_terminal_for(&self, pane_id: Option<&str>) -> Option<String> {
        let pane_id = pane_id?;
        self.state
            .agents
            .iter()
            .find(|agent| agent.pane_id.as_deref() == Some(pane_id))
            .map(|agent| agent.terminal_id.clone())
    }

    /// User-triggered Herdr Settings change: edit the real Herdr config, validate it with the
    /// installed Herdr CLI, reload the running server, then restart the thin hosted client.
    /// Shardlane owns no second Herdr config; Settings is only a visual editor for the user's
    /// normal `config.toml` (or `HERDR_CONFIG_PATH` override).
    pub(super) fn apply_herdr_user_config_update_and_restart(
        &mut self,
        update: herdr_tui::HerdrUserConfigUpdate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let theme_change = update.is_theme_change();
        self.detach_hosted_tui(cx);
        let client = self.client.clone();
        let window_handle = window.window_handle();
        self._tui_config_apply_task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let snapshot = herdr_tui::update_herdr_user_config(update)?;
                    if let Some(client) = client {
                        client
                            .reload_config()
                            .map_err(|error| format!("reload Herdr config: {error}"))?;
                    }
                    Ok::<_, String>(snapshot)
                })
                .await;
            if theme_change && result.is_ok() {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
            }
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |view, cx| match result {
                    Ok(snapshot) => {
                        view.herdr_user_config = snapshot;
                        view.sync_app_theme_from_herdr(cx);
                        view.apply_hosted_terminal_theme_background(window, cx);
                        view.save_config();
                        view.restart_tui_surface(window, cx);
                    }
                    Err(error) => {
                        lag_log(format_args!("herdr.config apply failed: {error}"));
                        window
                            .push_notification(format!("Herdr config update failed: {error}"), cx);
                    }
                });
            });
        });
    }

    /// Drop the hosted `herdr` child and clear any retained terminal presentation
    /// before Herdr config reload/restart work begins.
    pub(super) fn detach_hosted_tui(&mut self, cx: &mut Context<Self>) {
        self.terminal_token = self.terminal_token.wrapping_add(1);
        self.terminal_attach_target = None;
        self.reset_hosted_tui_state(true, cx);
    }

    /// Real restart (audit TUI-03): the old generation is invalidated (token increments → all old
    /// poll/attach tasks self-terminate) → manager.restart() swaps the shared child process's
    /// generation (all viewers change generation together) → new generation Starting → spawn. The
    /// lifecycle event sequence Running(g1) → Starting(g2) → Running(g2) is carried by the
    /// token/tui_host state.
    pub(super) fn restart_tui_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        lag_log(format_args!(
            "tui.restart: tearing down previous generation"
        ));
        self.detach_hosted_tui(cx);
        // Punch through the cooldown: the user explicitly restarted.
        self.tui_respawn_blocked_until = None;
        self.attach_herdr_tui(window, cx, true);
    }

    /// Hosted-terminal keyboard boundary, driven from the app-global keystroke observer.
    ///
    /// Why the observer and not an element-scoped `.on_key_down`: GPUI 0.2.2 dispatches key
    /// events only along the focus path (from the `track_focus` node upward). Shardlane tracks
    /// focus at the shell root, so key listeners deeper in the content area never fire — while
    /// mouse/wheel listeners are hit-test based and keep working. That mismatch is exactly what
    /// broke hosted-TUI keyboard input in the previous iteration.
    ///
    /// Ownership inside this boundary: printable text stays with AppKit's NSTextInputClient /
    /// GPUI InputHandler (`replace_text_in_range`), which is required for CJK/IME composition —
    /// pinyin/kana keystrokes must reach macOS's input context before any bytes hit the PTY.
    /// Named keys and modifier chords that survive action bindings are encoded by Ghostty here
    /// and written straight to the singleton primary hosted PTY. Returns true when bytes were queued.
    pub(super) fn handle_tui_keyboard(&mut self, key: &Keystroke, cx: &mut Context<Self>) -> bool {
        // Registry key swallowing / input surfaces / overlay occlusion are handled uniformly in
        // handle_keystroke (Shortcut SSOT); this boundary only validates the host target and
        // classifies Ghostty keys.
        if self.terminal_target.as_deref() != Some(herdr_tui::TUI_TARGET) {
            return false;
        }
        // While the Chat presentation owns keyboard focus, the hidden host TUI must not receive
        // ordinary input (plan §12.2 isolation boundary; acceptance testing observed ASCII leaking
        // into the PTY in the Chat state).
        if crate::chat::model::chat_surface_blocks_tui_input(self.chat.model.mode) {
            return false;
        }
        if !tui_key_encodes_to_pty(key) {
            // Printable text stays with AppKit NSTextInputClient (IME-first ordering).
            return false;
        }

        let target = herdr_tui::TUI_TARGET.to_string();
        self.prepare_terminal_for_input(&target, cx);
        let Some(bytes) = self.encode_terminal_key_for(&target, key) else {
            lag_log(format_args!(
                "tui.keyboard unmapped key {} dropped",
                key.key
            ));
            return false;
        };
        if bytes.is_empty() {
            return false;
        }
        self.queue_terminal_raw_bytes_traced(target, bytes, "key", cx);
        true
    }

    /// Trackpad/wheel input for the hosted TUI: convert the platform scroll delta into whole
    /// terminal rows with a persistent pixel residual (same primitive as Embedded panes), then
    /// emit one SGR wheel press per row. The residual keeps sub-line motion alive with no
    /// threshold dead zone, while avoiding one wheel packet per high-frequency trackpad event —
    /// that flood forced the remote TUI into per-event full repaints and made slow scrolling
    /// feel chunky. Scrollbar drag / momentum-end flush semantics are owned by
    /// `consume_terminal_scroll_rows`.
    pub(super) fn handle_tui_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_target.as_deref() != Some(herdr_tui::TUI_TARGET) {
            return;
        }
        let trace_started = crate::terminal_trace::enabled().then(Instant::now);
        let cell_height = self.terminal_cell_height();
        let line_height = px(cell_height.max(1.0) as f32);
        let raw_delta_y = event.delta.pixel_delta(line_height).y.to_f64();
        let residual_before = self.terminal_scroll_residual_px;
        if !terminal_wheel_uses_vertical_scroll(event) {
            // A previously accumulated vertical fraction belongs to the old
            // gesture; carrying it across an unhandled Shift+wheel event would
            // turn the next ordinary wheel event into a phantom row.
            self.terminal_scroll_residual_px = 0.0;
            if let Some(started) = trace_started {
                crate::terminal_trace::event(format_args!(
                    "stage=ui.scroll route=shift_unhandled precise={} phase={:?} raw_delta_y={raw_delta_y:.3} steps=0 residual_before={residual_before:.3} residual_after=0.0 elapsed_us={}",
                    event.delta.precise(),
                    event.touch_phase,
                    crate::terminal_trace::elapsed_us(started),
                ));
            }
            return;
        }
        // A wheel gesture owns the terminal surface from its first non-zero
        // pixel, even before a complete row is crossed. Clear an existing local
        // selection immediately (Ghostty does the same) and prevent a parent
        // scroll container from stealing a sub-row event. This runs once per
        // event (audit B14): the previous branch-inside/after pair double-ran
        // selection clearing and activity marking at trackpad frequency.
        if raw_delta_y.abs() > f64::EPSILON {
            self.prepare_terminal_for_scroll_input(herdr_tui::TUI_TARGET, cx);
            cx.stop_propagation();
        }
        let steps =
            Self::scroll_rows_for_event(&mut self.terminal_scroll_residual_px, event, cell_height);
        let residual_after = self.terminal_scroll_residual_px;
        if steps == 0 {
            cx.stop_propagation();
            if let Some(started) = trace_started {
                crate::terminal_trace::event(format_args!(
                    "stage=ui.scroll route=residual precise={} phase={:?} raw_delta_y={raw_delta_y:.3} steps=0 residual_before={residual_before:.3} residual_after={residual_after:.3} elapsed_us={}",
                    event.delta.precise(),
                    event.touch_phase,
                    crate::terminal_trace::elapsed_us(started),
                ));
            }
            return;
        }
        // Mouse reporting enabled → SGR wheel; not enabled and on the alt screen (less/vim-like) →
        // the local key encoder translates to ArrowUp/Down (ordered queue); the host TUI has no
        // local scrollback branch to fall back to.
        if self.try_report_terminal_wheel(
            herdr_tui::TUI_TARGET,
            steps,
            event,
            self.full_terminal_selection_geometry(),
            cx,
        ) {
            cx.stop_propagation();
            if let Some(started) = trace_started {
                crate::terminal_trace::event(format_args!(
                    "stage=ui.scroll route=mouse_report precise={} phase={:?} raw_delta_y={raw_delta_y:.3} steps={steps} residual_before={residual_before:.3} residual_after={residual_after:.3} elapsed_us={}",
                    event.delta.precise(),
                    event.touch_phase,
                    crate::terminal_trace::elapsed_us(started),
                ));
            }
            return;
        }
        let translated = self.try_translate_alt_screen_wheel(herdr_tui::TUI_TARGET, steps, cx);
        if translated {
            cx.stop_propagation();
        }
        if let Some(started) = trace_started {
            crate::terminal_trace::event(format_args!(
                "stage=ui.scroll route={} precise={} phase={:?} raw_delta_y={raw_delta_y:.3} steps={steps} residual_before={residual_before:.3} residual_after={residual_after:.3} elapsed_us={}",
                if translated { "alt_scroll" } else { "unhandled" },
                event.delta.precise(),
                event.touch_phase,
                crate::terminal_trace::elapsed_us(started),
            ));
        }
    }

    /// TUI surface mouse down: the host TUI's own UI (sidebar/Tab/Agent list) depends on mouse
    /// events — first try mouse reporting (Ghostty encoding → PTY); when mouse reporting is not
    /// enabled, fall back to local selection/middle-click paste. Does not call focus_pane_id
    /// (TUI_TARGET is not a pane).
    pub(super) fn handle_tui_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if tui_native_context_menu_owns_button(&event.button) {
            return;
        }
        if let Some(button) = Self::terminal_mouse_button(&event.button) {
            if self.try_report_terminal_mouse(
                herdr_tui::TUI_TARGET,
                terminal_interact::TerminalMouseReport {
                    action: TerminalMouseAction::Press,
                    button: Some(button),
                    modifiers: &event.modifiers,
                    position: event.position,
                    selection_geometry: self.full_terminal_selection_geometry(),
                    any_button_pressed: true,
                },
                cx,
            ) {
                cx.stop_propagation();
                return;
            }
        }
        match event.button {
            MouseButton::Left => {
                let geometry = self.full_terminal_selection_geometry();
                self.begin_terminal_selection(
                    herdr_tui::TUI_TARGET.to_string(),
                    geometry,
                    event,
                    window,
                    cx,
                );
            }
            MouseButton::Middle => self.paste(&Paste, window, cx),
            MouseButton::Right | MouseButton::Navigate(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_key(key: &str, key_char: Option<&str>) -> Keystroke {
        Keystroke {
            key: key.to_string(),
            key_char: key_char.map(str::to_string),
            modifiers: Default::default(),
        }
    }

    fn modified_key(key: &str, shift: bool, control: bool, alt: bool, platform: bool) -> Keystroke {
        Keystroke {
            key: key.to_string(),
            key_char: None,
            modifiers: crepuscularity_gpui::Modifiers {
                control,
                alt,
                shift,
                platform,
                function: false,
            },
        }
    }

    #[test]
    fn tui_printable_text_stays_native_named_keys_encode_for_pty() {
        // Printable text (incl. shifted and space) belongs to AppKit NSTextInputClient —
        // required for pinyin/kana IME to compose before any PTY byte.
        assert!(!tui_key_encodes_to_pty(&plain_key("n", Some("n"))));
        assert!(!tui_key_encodes_to_pty(&plain_key("N", Some("N"))));
        assert!(!tui_key_encodes_to_pty(&plain_key(".", Some("."))));
        assert!(!tui_key_encodes_to_pty(&plain_key("space", None)));

        // GPUI fills key_char for enter/tab ("\n"/"\t"); classification must be by name so
        // the Ghostty encoder owns them. (Regression: key_char-based split killed Enter/Tab.)
        assert!(tui_key_encodes_to_pty(&plain_key("enter", Some("\n"))));
        assert!(tui_key_encodes_to_pty(&plain_key("tab", Some("\t"))));
        assert!(tui_key_encodes_to_pty(&plain_key("backspace", None)));
        assert!(tui_key_encodes_to_pty(&plain_key("escape", None)));
        for arrow in ["up", "down", "left", "right", "pageup", "home", "f5"] {
            assert!(tui_key_encodes_to_pty(&plain_key(arrow, None)), "{arrow}");
        }

        // Any ctrl/alt/cmd/function chord encodes (GUI swallow list checked separately).
        let mut control = plain_key("c", None);
        control.modifiers.control = true;
        assert!(tui_key_encodes_to_pty(&control));
        assert!(tui_key_encodes_to_pty(&modified_key(
            "b", false, false, true, false
        )));
        let mut cmd_v = plain_key("v", None);
        cmd_v.modifiers.platform = true;
        assert!(tui_key_encodes_to_pty(&cmd_v));
        // shift alone stays printable text.
        assert!(!tui_key_encodes_to_pty(&modified_key(
            "a", true, false, false, false
        )));
    }

    #[test]
    fn tui_right_click_is_reserved_for_native_context_menu() {
        assert!(tui_native_context_menu_owns_button(&MouseButton::Right));
        assert!(!tui_native_context_menu_owns_button(&MouseButton::Left));
        assert!(!tui_native_context_menu_owns_button(&MouseButton::Middle));
    }

    #[test]
    fn shift_wheel_stays_with_parent_and_cannot_accumulate_terminal_rows() {
        let shift = ScrollWheelEvent {
            modifiers: crepuscularity_gpui::Modifiers {
                shift: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let ordinary = ScrollWheelEvent::default();
        assert!(!terminal_wheel_uses_vertical_scroll(&shift));
        assert!(terminal_wheel_uses_vertical_scroll(&ordinary));
    }

    #[test]
    fn terminal_input_handler_never_replays_control_payloads() {
        assert!(text_is_terminal_control_payload("\n"));
        assert!(text_is_terminal_control_payload("\t"));
        assert!(text_is_terminal_control_payload("\u{7f}"));
        assert!(!text_is_terminal_control_payload(""));
        assert!(!text_is_terminal_control_payload("n"));
        assert!(!text_is_terminal_control_payload("你好"));
        assert!(!text_is_terminal_control_payload("a\n"));
    }

    #[test]
    fn tui_precision_scroll_accumulates_rows_without_threshold_dead_zone() {
        // consume_terminal_scroll_rows semantics shared with Embedded panes:
        // sub-cell pixel deltas persist as residual until a whole row accumulates.
        let mut residual = 0.0_f64;
        let cell_height = 20.0_f64;
        let consume = |residual: &mut f64, dy_px: f64| {
            super::consume_terminal_scroll_rows(residual, dy_px, cell_height, false)
        };
        assert_eq!(consume(&mut residual, -6.0), 0);
        assert_eq!(consume(&mut residual, -6.0), 0);
        assert_eq!(consume(&mut residual, -10.0), -1);
        assert!((residual + 2.0).abs() < 1e-9);
        // No dead zone: continued slow scrolling keeps emitting rows without ever resetting.
        assert_eq!(consume(&mut residual, -30.0), -1);
    }
}
