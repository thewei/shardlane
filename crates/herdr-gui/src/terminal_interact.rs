//! Terminal interaction layer: pixel→cell hit testing, cell/word/line selection state machines,
//! Ghostty-encoded mouse/wheel reporting, wheel row conversion, and interaction geometry.
//!
//! [INPUT]: Depends on `super` (main.rs)'s ShardlaneApp selection/terminal state fields and session handles,
//!          `terminal_view`'s TerminalSelection/TERMINAL_SELECTION_BG and the frame's palette-derived selection color,
//!          and `terminal_stream`'s ManagedTerminal (semantic selection/mouse encoding Ghostty ABI)
//! [OUTPUT]: Exposes selection interaction methods (begin/drag/sync), the mouse/wheel handler family (the
//!           first child-row wheel event immediately takes surface ownership),
//!           try_report_terminal_mouse/wheel, try_translate_alt_screen_wheel, and the
//!           SelectionGeometry/PendingTerminalCopy/TerminalMouseReport interaction types
//! [POS]: The complete home for main.rs's terminal interaction domain; rendering/polling remain in main.rs

use super::*;

impl ShardlaneApp {
    pub(super) fn pixel_to_cell_for_target(&self, x: f64, y: f64) -> (u16, u16) {
        let frame = self.terminal_frame_for_target();
        if frame.lines.is_empty() {
            return (0, 0);
        }
        let cell_height = self.terminal_cell_height();
        let cell_width = self.terminal_cell_width();
        let row =
            ((y / cell_height).floor().max(0.0) as usize).min(frame.lines.len().saturating_sub(1));
        let line = &frame.lines[row];
        let max_col = line.cells.len().saturating_sub(1);
        let col = ((x / cell_width).floor().max(0.0) as usize).min(max_col);
        (
            col.min(u16::MAX as usize) as u16,
            row.min(u16::MAX as usize) as u16,
        )
    }

    pub(super) fn selection_cell_at_position(
        &self,
        geometry: SelectionGeometry,
        position: crepuscularity_gpui::Point<crepuscularity_gpui::Pixels>,
    ) -> (u16, u16) {
        self.pixel_to_cell_for_target(
            (position.x.to_f64() - geometry.origin_x).max(0.0),
            (position.y.to_f64() - geometry.origin_y).max(0.0),
        )
    }

    pub(super) fn begin_terminal_selection(
        &mut self,
        target: String,
        geometry: SelectionGeometry,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        let cell = self.selection_cell_at_position(geometry, event.position);
        let previous = self.selections.get(&target).copied();
        self.selection_target = Some(target.clone());
        self.selection_geometry = Some(geometry);

        if event.modifiers.shift {
            let start = previous.map(|selection| selection.0).unwrap_or(cell);
            self.selection_anchor_cell = Some(start);
            self.selection_mode = TerminalSelectionMode::Cell;
            self.selections.insert(target, (start, cell));
            self.selection_dragged = true;
            self.selecting = true;
        } else if event.click_count >= 3 {
            self.selection_anchor_cell = Some(cell);
            self.selection_mode = TerminalSelectionMode::Line;
            if let Some(selection) = self.line_selection_at(&target, cell) {
                self.selections.insert(target.clone(), selection);
            } else {
                self.selections.remove(&target);
            }
            self.selection_dragged = true;
            self.selecting = true;
        } else if event.click_count == 2 {
            self.selection_anchor_cell = Some(cell);
            self.selection_mode = TerminalSelectionMode::Word;
            if let Some(selection) = self.word_selection_at(&target, cell) {
                self.selections.insert(target.clone(), selection);
            } else {
                self.selections.remove(&target);
            }
            self.selection_dragged = true;
            self.selecting = true;
        } else {
            self.selection_anchor_cell = Some(cell);
            self.selection_mode = TerminalSelectionMode::Cell;
            self.selections.insert(target, (cell, cell));
            self.selection_dragged = false;
            self.selecting = true;
        }

        self.sync_terminal_selection(cx);
        cx.notify();
    }

    fn word_selection_at(&self, target: &str, cell: (u16, u16)) -> Option<TerminalSelection> {
        self.semantic_selection_at(target, cell, false)
    }

    fn line_selection_at(&self, target: &str, cell: (u16, u16)) -> Option<TerminalSelection> {
        self.semantic_selection_at(target, cell, true)
    }

    fn semantic_selection_at(
        &self,
        target: &str,
        cell: (u16, u16),
        line: bool,
    ) -> Option<TerminalSelection> {
        let tui_projection = target == herdr_tui::TUI_TARGET;
        let raw_cell = if tui_projection {
            self.tui_visible_to_raw_cell(cell)
        } else {
            cell
        };
        let fallback = self.selections.get(target).copied();
        self.selection_at_terminal(target, fallback, |managed, fallback| {
            let selection = if line {
                managed.select_line_at(raw_cell)
            } else {
                managed.select_word_at(raw_cell)
            };
            let selection = selection.ok().flatten()?;
            if tui_projection {
                self.tui_raw_to_visible_selection(selection).or(fallback)
            } else {
                Some(selection)
            }
        })
    }

    /// Shared probe behind the word/line click and drag selection paths (audit B06):
    /// resolves `target`'s managed terminal and runs `select` under it. On renderer lock
    /// contention both paths keep the target's previous selection instead of destroying
    /// it — the word-click path used to degrade to a 1-cell selection here.
    fn selection_at_terminal(
        &self,
        target: &str,
        fallback: Option<TerminalSelection>,
        select: impl FnOnce(
            &mut ManagedTerminal,
            Option<TerminalSelection>,
        ) -> Option<TerminalSelection>,
    ) -> Option<TerminalSelection> {
        let managed = if self.terminal_target.as_deref() == Some(target) {
            self.terminal.as_ref()
        } else if target == crate::right_panel::lazygit::LAZYGIT_TARGET {
            self.lazygit_session.terminal.as_ref()
        } else {
            None
        };
        let Some(managed) = managed else {
            return fallback;
        };
        match managed.try_lock() {
            Ok(mut managed) => select(&mut managed, fallback),
            Err(_) => fallback,
        }
    }

    pub(super) fn drag_selection_at(
        &self,
        target: &str,
        anchor: (u16, u16),
        current: (u16, u16),
        mode: TerminalSelectionMode,
    ) -> Option<TerminalSelection> {
        if mode == TerminalSelectionMode::Cell {
            return Some((anchor, current));
        }
        let fallback = self.selections.get(target).copied();
        let tui_projection = target == herdr_tui::TUI_TARGET;
        let raw_anchor = if tui_projection {
            self.tui_visible_to_raw_cell(anchor)
        } else {
            anchor
        };
        let raw_current = if tui_projection {
            self.tui_visible_to_raw_cell(current)
        } else {
            current
        };
        self.selection_at_terminal(target, fallback, |managed, fallback| {
            let selection = match mode {
                TerminalSelectionMode::Word => managed.select_word_drag(raw_anchor, raw_current),
                TerminalSelectionMode::Line => managed.select_line_drag(raw_anchor, raw_current),
                TerminalSelectionMode::Cell => unreachable!(),
            };
            let Some(selection) = selection.ok().flatten() else {
                return fallback;
            };
            if tui_projection {
                self.tui_raw_to_visible_selection(selection).or(fallback)
            } else {
                Some(selection)
            }
        })
    }

    pub(super) fn sync_terminal_selection(&self, cx: &mut Context<Self>) {
        let main_selection = self
            .terminal_target
            .as_deref()
            .and_then(|target| self.selections.get(target).copied());
        let selection_bg = terminal_selection_background(&self.terminal_frame);
        self.terminal_pane.update(cx, |pane, cx| {
            pane.set_selection(main_selection, selection_bg, cx)
        });
        if self.is_lazygit_surface_active() {
            let selection = self
                .selections
                .get(crate::right_panel::lazygit::LAZYGIT_TARGET)
                .copied();
            let selection_bg = terminal_selection_background(self.lazygit_frame());
            self.lazygit_session.pane.update(cx, |pane, cx| {
                pane.set_selection(selection, selection_bg, cx)
            });
        }
    }
}

/// Mouse selection must use the same palette-derived highlight as the current terminal frame.
/// Falling back to the historical dark blue is only valid before a palette has been applied;
/// forcing it on every drag made dark text nearly disappear in light appearance.
pub(crate) fn terminal_selection_background(frame: &TerminalFrame) -> u32 {
    frame.selection_color.unwrap_or(TERMINAL_SELECTION_BG)
}

// ------------------------------------------------------------------
// Mouse/wheel encoding and interaction geometry (relocated from main.rs)
// ------------------------------------------------------------------

/// Screen-space origin of the terminal content area's top-left (including padding/inset conversion), for pixel→cell conversion.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SelectionGeometry {
    pub(crate) origin_x: f64,
    pub(crate) origin_y: f64,
}

/// Pending copy request for copy-on-select: prefers Ghostty semantic selection text, falling back
/// to local extraction. `generation` stamps the terminal token the selection was made against;
/// the poll flush discards the request when the host restarted in between (audit B20).
#[derive(Clone, Debug)]
pub(crate) struct PendingTerminalCopy {
    pub(crate) selection: TerminalSelection,
    pub(crate) fallback_text: String,
    pub(crate) generation: u64,
}

/// One mouse event awaiting encoded reporting (Ghostty mouse encoding input).
pub(crate) struct TerminalMouseReport<'a> {
    pub(crate) action: TerminalMouseAction,
    pub(crate) button: Option<TerminalMouseButton>,
    pub(crate) modifiers: &'a crepuscularity_gpui::Modifiers,
    pub(crate) position: crepuscularity_gpui::Point<Pixels>,
    pub(crate) selection_geometry: SelectionGeometry,
    pub(crate) any_button_pressed: bool,
}

impl ShardlaneApp {
    pub(super) fn scroll_rows_for_event(
        residual_px: &mut f64,
        event: &ScrollWheelEvent,
        cell_height: f64,
    ) -> isize {
        let line_height = px(cell_height.max(1.0) as f32);
        let mut dy = event.delta.pixel_delta(line_height).y.to_f64();
        if event.delta.precise() {
            // GPUI forwards AppKit's precise scrollingDeltaY unchanged. Ghostty's macOS
            // SurfaceView applies a 2x multiplier before handing that same precise delta to
            // its terminal core; mirror that native default so hosted Herdr TUI trackpad
            // distance matches Ghostty rather than feeling artificially damped.
            dy *= 2.0;
        }
        consume_terminal_scroll_rows(
            residual_px,
            dy,
            cell_height,
            matches!(event.touch_phase, TouchPhase::Ended),
        )
    }

    pub(super) fn terminal_mouse_button(button: &MouseButton) -> Option<TerminalMouseButton> {
        match button {
            MouseButton::Left => Some(TerminalMouseButton::Left),
            MouseButton::Right => Some(TerminalMouseButton::Right),
            MouseButton::Middle => Some(TerminalMouseButton::Middle),
            MouseButton::Navigate(_) => None,
        }
    }

    fn terminal_mouse_modifiers(modifiers: &crepuscularity_gpui::Modifiers) -> TerminalModifiers {
        TerminalModifiers {
            shift: modifiers.shift,
            control: modifiers.control,
            alt: modifiers.alt,
            platform: modifiers.platform,
        }
    }

    /// Shared target→size resolution, chrome-offset compensation, and SGR geometry
    /// construction behind the mouse and wheel report paths (audit B05): a coordinate
    /// compensation change can only land in one place now. Returns the encoded geometry
    /// plus the chrome-compensated event position in cell-pixel space.
    fn terminal_mouse_geometry_for(
        &self,
        target: &str,
        origin_x: f64,
        origin_y: f64,
        position_x: f64,
        position_y: f64,
    ) -> Option<(TerminalMouseGeometry, (f64, f64))> {
        let mut size = if self.terminal_target.as_deref() == Some(target) {
            self.terminal_size
        } else if target == crate::right_panel::lazygit::LAZYGIT_TARGET {
            self.lazygit_size()
        } else {
            None
        }?;
        let cell_width = self.terminal_cell_width();
        let cell_height = self.terminal_cell_height();
        let mut x = (position_x - origin_x).max(0.0);
        let mut y = (position_y - origin_y).max(0.0);
        if target == herdr_tui::TUI_TARGET {
            x += cell_width * f64::from(self.tui_chrome_projection.left);
            y += cell_height * f64::from(self.tui_chrome_projection.top);
            size = self.tui_raw_terminal_size(size);
        }
        let geometry = TerminalMouseGeometry {
            screen_width: u32::from(size.2.max(1)),
            screen_height: u32::from(size.3.max(1)),
            cell_width: cell_width.round().max(1.0) as u32,
            cell_height: cell_height.round().max(1.0) as u32,
        };
        Some((geometry, (x, y)))
    }

    pub(super) fn try_report_terminal_mouse(
        &mut self,
        target: &str,
        report: TerminalMouseReport<'_>,
        cx: &mut Context<Self>,
    ) -> bool {
        // Shift is the conventional terminal escape hatch for local selection/scrollback.
        if report.modifiers.shift {
            return false;
        }

        let managed = self.managed_terminal_for_target(target);
        let Some(managed) = managed else {
            return false;
        };
        let Some((geometry, (x, y))) = self.terminal_mouse_geometry_for(
            target,
            report.selection_geometry.origin_x,
            report.selection_geometry.origin_y,
            report.position.x.to_f64(),
            report.position.y.to_f64(),
        ) else {
            return false;
        };
        // Hosted TUI input is lossless: renderer contention must not drop mouse events.
        let trace = crate::terminal_trace::enabled();
        let lock_started = trace.then(Instant::now);
        let mut terminal = match managed.lock() {
            Ok(terminal) => terminal,
            Err(_) => return false,
        };
        let lock_wait_us = lock_started
            .map(crate::terminal_trace::elapsed_us)
            .unwrap_or(0);
        let encode_started = trace.then(Instant::now);
        let encoded = terminal.encode_mouse(
            report.action,
            report.button,
            Self::terminal_mouse_modifiers(report.modifiers),
            (x as f32, y as f32),
            geometry,
            report.any_button_pressed,
        );
        let encode_us = encode_started
            .map(crate::terminal_trace::elapsed_us)
            .unwrap_or(0);
        drop(terminal);
        let Ok(encoded) = encoded else {
            return false;
        };
        if encoded.is_empty() {
            return false;
        }
        if trace
            && (report.action != TerminalMouseAction::Motion
                || lock_wait_us >= 500
                || encode_us >= 500)
        {
            crate::terminal_trace::event(format_args!(
                "stage=ui.mouse_encode action={:?} button={:?} bytes={} lock_wait_us={lock_wait_us} encode_us={encode_us}",
                report.action,
                report.button,
                encoded.len(),
            ));
        }
        self.queue_terminal_raw_bytes_traced(target.to_string(), encoded, "mouse", cx);
        true
    }

    pub(super) fn try_report_terminal_wheel(
        &mut self,
        target: &str,
        steps: isize,
        event: &ScrollWheelEvent,
        selection_geometry: SelectionGeometry,
        cx: &mut Context<Self>,
    ) -> bool {
        if steps == 0 || event.modifiers.shift {
            return false;
        }
        let managed = self.managed_terminal_for_target(target);
        let Some(managed) = managed else {
            return false;
        };
        let Some((mouse_geometry, (x, y))) = self.terminal_mouse_geometry_for(
            target,
            selection_geometry.origin_x,
            selection_geometry.origin_y,
            event.position.x.to_f64(),
            event.position.y.to_f64(),
        ) else {
            return false;
        };
        let button = if steps > 0 {
            TerminalMouseButton::WheelUp
        } else {
            TerminalMouseButton::WheelDown
        };
        // Precision wheel events are user input; never drop them because the renderer is
        // momentarily extracting a frame from the same terminal model.
        let trace = crate::terminal_trace::enabled();
        let lock_started = trace.then(Instant::now);
        let mut terminal = match managed.lock() {
            Ok(terminal) => terminal,
            Err(_) => return false,
        };
        let lock_wait_us = lock_started
            .map(crate::terminal_trace::elapsed_us)
            .unwrap_or(0);
        let encode_started = trace.then(Instant::now);
        let mut bytes = Vec::new();
        // No per-event line cap: a cap drops motion during fast scrolling (a 1375-line request
        // emitted only ~1248 lines in practice), accumulating rubber-band lag. An SGR press is
        // ~16 bytes per line, no strain on the PTY; the residual guard keeps the total within
        // the rows the gesture actually crossed.
        for _ in 0..steps.unsigned_abs() {
            let press = match terminal.encode_mouse(
                TerminalMouseAction::Press,
                Some(button),
                Self::terminal_mouse_modifiers(&event.modifiers),
                (x as f32, y as f32),
                mouse_geometry,
                false,
            ) {
                Ok(encoded) if !encoded.is_empty() => encoded,
                _ if bytes.is_empty() => return false,
                _ => break,
            };
            // Xterm wheel input is a button-press event; terminal emulators do not emit a
            // matching release for every notch. Avoid doubling packets and generating a
            // synthetic generic mouse-release event in Herdr/crossterm.
            bytes.extend_from_slice(&press);
        }
        let encode_us = encode_started
            .map(crate::terminal_trace::elapsed_us)
            .unwrap_or(0);
        drop(terminal);
        if bytes.is_empty() {
            return false;
        }
        if trace {
            crate::terminal_trace::event(format_args!(
                "stage=ui.scroll_encode steps={steps} bytes={} lock_wait_us={lock_wait_us} encode_us={encode_us}",
                bytes.len(),
            ));
        }
        self.queue_terminal_raw_bytes_traced(target.to_string(), bytes, "scroll", cx);
        if scroll_debug_enabled() {
            lag_log(format_args!(
                "scroll_debug wheel t={} steps={steps}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0),
            ));
        }
        true
    }

    /// Terminal BEL feedback (bell observed on the live VT stream):
    /// - Window active: the user is watching the terminal; the bell is in-band info, don't disturb.
    /// - Window unfocused: request dock attention (AppKit coalesces on its own), and throttle system
    ///   notifications to 2s per target to avoid a notification storm during dense Agent output.
    pub(super) fn handle_terminal_bells(&mut self, target: &str, bells: u64) {
        if bells == 0 || self.window_active {
            return;
        }
        notifications::request_dock_attention();
        let now = Instant::now();
        let throttled = self
            .terminal_bell_last_notify
            .get(target)
            .is_some_and(|at| now.duration_since(*at) < Duration::from_secs(2));
        if !throttled {
            self.terminal_bell_last_notify
                .insert(target.to_string(), now);
            let matched =
                self.state.panes.iter().find(|pane| {
                    pane.terminal_id.as_deref() == Some(target) || pane.pane_id == target
                });
            let pane_title = matched
                .and_then(|pane| pane.title.clone().or_else(|| pane.terminal_title.clone()))
                .unwrap_or_else(|| "Terminal".to_string());
            let body = format!("Bell: {pane_title}");
            match matched.map(|pane| pane.pane_id.clone()) {
                // P2-2: BEL notifications also carry a pane_id; clicking jumps to the corresponding pane.
                Some(pane_id) => notifications::show_with_pane("Shardlane", &body, &pane_id),
                None => notifications::show("Shardlane", &body),
            }
        }
    }

    /// The alternate screen (mouse-reporting-less TUIs like less/vim/man) has no scrollback to scroll:
    /// wheel input is translated to Up/Down key input per the iTerm2/Ghostty convention, using the same
    /// ordered input queue as the keyboard. Returns false when not applicable (not alt screen / no
    /// controller), yielding back to local scrolling.
    ///
    /// Alt-screen (less/vim-like) wheel translation: when the host TUI hasn't enabled mouse reporting,
    /// convert wheel rows into ordered ArrowUp/Down bytes via the local Ghostty key encoder (max 6 steps).
    /// After TUI-only convergence, the primary target and the visible Lazygit auxiliary share the same
    /// reporting boundary; there is no per-pane branch, and Shift is the local escape-key convention.
    pub(super) fn try_translate_alt_screen_wheel(
        &mut self,
        target: &str,
        rows: isize,
        cx: &mut Context<Self>,
    ) -> bool {
        if rows == 0 {
            return false;
        }
        if self.terminal_target.as_deref() != Some(target) {
            return false;
        }
        let Some(managed) = self.terminal.clone() else {
            return false;
        };
        let trace = crate::terminal_trace::enabled();
        let lock_started = trace.then(Instant::now);
        let Ok(mut terminal) = managed.lock() else {
            return false;
        };
        let lock_wait_us = lock_started
            .map(crate::terminal_trace::elapsed_us)
            .unwrap_or(0);
        if !terminal.is_alternate_screen() {
            return false;
        }
        let key = if rows > 0 {
            crate::ghostty::TerminalKey::ArrowUp
        } else {
            crate::ghostty::TerminalKey::ArrowDown
        };
        let steps = rows.unsigned_abs().min(6);
        let encode_started = trace.then(Instant::now);
        let mut bytes = Vec::new();
        for _ in 0..steps {
            match terminal.encode_key(key, TerminalModifiers::default(), 0) {
                Ok(encoded) => bytes.extend_from_slice(&encoded),
                Err(_) => return false,
            }
        }
        let encode_us = encode_started
            .map(crate::terminal_trace::elapsed_us)
            .unwrap_or(0);
        drop(terminal);
        if bytes.is_empty() {
            return false;
        }
        if trace {
            crate::terminal_trace::event(format_args!(
                "stage=ui.alt_scroll_encode steps={steps} bytes={} lock_wait_us={lock_wait_us} encode_us={encode_us}",
                bytes.len(),
            ));
        }
        self.queue_terminal_raw_bytes_traced(target.to_string(), bytes, "alt_scroll", cx);
        true
    }

    pub(super) fn handle_terminal_mouse_move_target(
        &mut self,
        target: &str,
        geometry: SelectionGeometry,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        let button = event
            .pressed_button
            .as_ref()
            .and_then(Self::terminal_mouse_button);
        if self.try_report_terminal_mouse(
            target,
            TerminalMouseReport {
                action: TerminalMouseAction::Motion,
                button,
                modifiers: &event.modifiers,
                position: event.position,
                selection_geometry: geometry,
                any_button_pressed: event.pressed_button.is_some(),
            },
            cx,
        ) {
            cx.stop_propagation();
            return;
        }
        if self.selecting && event.dragging() && self.selection_target.as_deref() == Some(target) {
            let Some(anchor) = self.selection_anchor_cell else {
                return;
            };
            let current = self.selection_cell_at_position(geometry, event.position);
            if let Some(selection) =
                self.drag_selection_at(target, anchor, current, self.selection_mode)
            {
                if selection != (anchor, anchor) {
                    self.selection_dragged = true;
                }
                self.selections.insert(target.to_string(), selection);
                self.sync_terminal_selection(cx);
                cx.notify();
                cx.stop_propagation();
            }
        }
    }

    pub(super) fn handle_terminal_mouse_up_target(
        &mut self,
        target: &str,
        geometry: SelectionGeometry,
        event: &MouseUpEvent,
        cx: &mut Context<Self>,
    ) {
        let Some(button) = Self::terminal_mouse_button(&event.button) else {
            return;
        };
        if self.try_report_terminal_mouse(
            target,
            TerminalMouseReport {
                action: TerminalMouseAction::Release,
                button: Some(button),
                modifiers: &event.modifiers,
                position: event.position,
                selection_geometry: geometry,
                any_button_pressed: false,
            },
            cx,
        ) {
            cx.stop_propagation();
            return;
        }
        if event.button == MouseButton::Left
            && self.selecting
            && self.selection_target.as_deref() == Some(target)
        {
            if !self.selection_dragged {
                self.selections.remove(target);
            }
            self.selecting = false;
            self.selection_geometry = None;
            self.selection_anchor_cell = None;
            self.selection_mode = TerminalSelectionMode::Cell;
            self.selection_dragged = false;
            self.sync_terminal_selection(cx);
            cx.notify();
            cx.stop_propagation();
        }
    }

    pub(super) fn full_terminal_selection_geometry(&self) -> SelectionGeometry {
        let (origin_x, origin_y) = self.terminal_canvas_origin();
        let padding = f64::from(self.terminal_content_padding());
        SelectionGeometry {
            origin_x: origin_x + padding,
            origin_y: origin_y + padding,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crepuscularity_gpui::ScrollDelta;

    #[test]
    fn macos_precise_scroll_matches_ghostty_two_x_distance() {
        let precise = ScrollWheelEvent {
            delta: ScrollDelta::Pixels(point(px(0.0), px(5.0))),
            touch_phase: TouchPhase::Moved,
            ..Default::default()
        };
        let mut precise_residual = 0.0;
        assert_eq!(
            ShardlaneApp::scroll_rows_for_event(&mut precise_residual, &precise, 10.0),
            1
        );
        assert!(precise_residual.abs() < f64::EPSILON);

        let discrete = ScrollWheelEvent {
            delta: ScrollDelta::Lines(point(0.0, 0.5)),
            touch_phase: TouchPhase::Moved,
            ..Default::default()
        };
        let mut discrete_residual = 0.0;
        assert_eq!(
            ShardlaneApp::scroll_rows_for_event(&mut discrete_residual, &discrete, 10.0),
            0
        );
        assert!((discrete_residual - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn scroll_rows_for_event_covers_slow_fast_and_gesture_end_paths() {
        let slow = ScrollWheelEvent {
            delta: ScrollDelta::Pixels(point(px(0.0), px(3.0))),
            touch_phase: TouchPhase::Moved,
            ..Default::default()
        };
        let next_slow = ScrollWheelEvent {
            delta: ScrollDelta::Pixels(point(px(0.0), px(2.0))),
            touch_phase: TouchPhase::Moved,
            ..Default::default()
        };
        let mut residual = 0.0;
        assert_eq!(
            ShardlaneApp::scroll_rows_for_event(&mut residual, &slow, 10.0),
            0
        );
        assert!((residual - 6.0).abs() < f64::EPSILON);
        assert_eq!(
            ShardlaneApp::scroll_rows_for_event(&mut residual, &next_slow, 10.0),
            1
        );
        assert!(residual.abs() < f64::EPSILON);

        let fast = ScrollWheelEvent {
            delta: ScrollDelta::Lines(point(0.0, 9.0)),
            touch_phase: TouchPhase::Moved,
            ..Default::default()
        };
        assert_eq!(
            ShardlaneApp::scroll_rows_for_event(&mut residual, &fast, 10.0),
            9,
            "large wheel deltas must retain their full distance"
        );

        let ended = ScrollWheelEvent {
            delta: ScrollDelta::Pixels(point(px(0.0), px(1.0))),
            touch_phase: TouchPhase::Ended,
            ..Default::default()
        };
        assert_eq!(
            ShardlaneApp::scroll_rows_for_event(&mut residual, &ended, 10.0),
            0
        );
        assert!(residual.abs() < f64::EPSILON);
    }
}
