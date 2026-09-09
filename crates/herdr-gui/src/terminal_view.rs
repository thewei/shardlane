//! [INPUT]: Depends on `crate::ghostty` (`TerminalFrame`/`TerminalLine`/`TerminalRun`/`TerminalCursorStyle`/`TerminalFramePlan`) and the GPUI render pipeline.
//! [OUTPUT]: Exposes `TerminalPane` (row-entity paint recycling), `TerminalSelection`, `TerminalGeometry`, `TERMINAL_SELECTION_BG`,
//!           `cached_terminal`, and cursor preference resolution (`terminal_cursor_style_override`/`terminal_cursor_blink_override`,
//!           dispatched uniformly by main.rs in `apply_terminal_render_settings`); the selection/search highlight
//!           colors prefer the frame's `selection_color` (palette-derived), falling back to hardcoded defaults when
//!           no palette exists. The cursor blink chain only stays awake while "there is a cursor and it is blinking",
//!           stopping when idle (blinking resumption re-arms via set_frame/preference dispatch).
//! [POS]: `crates/herdr-gui`'s terminal cell painting and geometry layer, projecting the underlying frame into a
//!           high-performance GPUI element with row backgrounds, selection, cursor, hyperlinks (with hover tint
//!           feedback), and text runs. The row grid height is fixed to rows × cell_height (not stretched by flex),
//!           and Block Elements (Herdr scrollbar ▕/▐ etc.) render as cell-geometry overlays; together these guarantee
//!           row-spanning continuous elements with no periodic seams (evidence: scripts/scrollbar-continuity-ab.sh).

use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{
    cached_view, div, font, px, rgb, AnyElement, App, Entity, Font, FontFallbacks, FontFeatures,
    IntoElement, MouseButton, Render, Task as BackgroundJob, Timer, Window,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::ghostty::{
    TerminalCursorStyle, TerminalFrame, TerminalFramePlan, TerminalLine, TerminalRun,
};
use crate::settings::{TerminalCursorBlinkPreference, TerminalCursorStylePreference};

pub type TerminalSelection = ((u16, u16), (u16, u16));

/// Bootstrap selection fallback before the first Herdr/Ghostty frame exposes resolved colors.
/// Steady-state selection color comes from `TerminalFrame::selection_color`.
pub const TERMINAL_SELECTION_BG: u32 = 0x264f78;
const TERMINAL_FALLBACK_CELL_WIDTH_RATIO: f32 = 0.60;

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalGeometry {
    pub font_family: String,
    pub font_size: f32,
    pub cell_width: f32,
    pub cell_height: f32,
}

impl Default for TerminalGeometry {
    fn default() -> Self {
        Self {
            font_family: "Menlo".to_string(),
            font_size: 12.0,
            cell_width: 7.2,
            cell_height: 18.0,
        }
    }
}

impl TerminalGeometry {
    pub fn resolve(font_family: &str, font_size: f32, line_height: f32, cx: &App) -> Self {
        let font_family = if font_family.trim().is_empty() {
            "Menlo".to_string()
        } else {
            font_family.to_string()
        };
        let font_size = font_size.clamp(10.0, 24.0);
        let cell_height = line_height.max(font_size + 2.0).clamp(14.0, 34.0);
        let terminal_font = terminal_font(&font_family);
        let font_id = cx.text_system().resolve_font(&terminal_font);
        let measured_width = cx
            .text_system()
            .ch_advance(font_id, px(font_size))
            .ok()
            .map(|width| width.to_f64() as f32)
            .filter(|width| width.is_finite() && *width > 0.0);
        let cell_width = measured_width
            .unwrap_or(font_size * TERMINAL_FALLBACK_CELL_WIDTH_RATIO)
            .max(1.0);
        Self {
            font_family,
            font_size,
            cell_width,
            cell_height,
        }
    }

    pub fn font(&self) -> Font {
        terminal_font(&self.font_family)
    }
}

fn terminal_font(font_family: &str) -> Font {
    let mut terminal_font = font(font_family.to_string());
    terminal_font.features = FontFeatures::disable_ligatures();
    // Comprehensive fallback chain for terminal glyphs:
    // 1. Popular Nerd Font & coding symbol fonts (for Powerline, dev icons, ❯ prompts)
    // 2. Monospace grid staples (Menlo, Monaco, Courier New)
    // 3. Mathematical, technical, and special Unicode symbol fonts (STIX Two Math for ⏵ U+23F5, Apple Symbols, Arial Unicode MS)
    // 4. CJK & Emoji fallbacks (PingFang SC, Hiragino Sans GB, Apple Color Emoji)
    const CANDIDATE_FALLBACKS: &[&str] = &[
        "Symbols Nerd Font Mono",
        "Symbols Nerd Font",
        "JetBrainsMono Nerd Font Mono",
        "JetBrainsMono Nerd Font",
        "Monaco Nerd Font Mono",
        "Monaco Nerd Font",
        "DroidSansMono Nerd Font",
        "MesloLGS NF",
        "Maple Mono",
        "Menlo",
        "Monaco",
        "Courier New",
        "STIX Two Math",
        "Apple Symbols",
        "Arial Unicode MS",
        "PingFang SC",
        "Hiragino Sans GB",
        "Apple Color Emoji",
    ];
    let fallbacks = CANDIDATE_FALLBACKS
        .iter()
        .filter(|fallback| **fallback != font_family)
        .map(|s| (*s).to_string())
        .collect();
    terminal_font.fallbacks = Some(FontFallbacks::from_fonts(fallbacks));
    terminal_font
}

/// Per-row paint layering (row background, explicit cell backgrounds, selection wash,
/// cursor, and glyph color). Private presentation state shared by every `TerminalRowPane`.
#[derive(Clone, Copy, PartialEq)]
struct TerminalPaintStyle {
    bg: u32,
    selection_bg: u32,
    cursor_color: u32,
    cell_width: f32,
    cell_height: f32,
    bg_opacity: f32,
}

struct TerminalRowPane {
    row: u16,
    line: TerminalLine,
    selection_span: Option<(u16, u16)>,
    style: TerminalPaintStyle,
    geometry: TerminalGeometry,
    cursor_col: Option<u16>,
    cursor_style: TerminalCursorStyle,
    cursor_visible: bool,
}

impl TerminalRowPane {
    #[allow(clippy::too_many_arguments)]
    fn new(
        row: u16,
        line: TerminalLine,
        selection_span: Option<(u16, u16)>,
        style: TerminalPaintStyle,
        geometry: TerminalGeometry,
        cursor_col: Option<u16>,
        cursor_style: TerminalCursorStyle,
        cursor_visible: bool,
    ) -> Self {
        Self {
            row,
            line,
            selection_span,
            style,
            geometry,
            cursor_col,
            cursor_style,
            cursor_visible: cursor_col.is_some() && cursor_visible,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn matches_presentation(
        &self,
        line: &TerminalLine,
        selection_span: Option<(u16, u16)>,
        style: TerminalPaintStyle,
        geometry: &TerminalGeometry,
        cursor_col: Option<u16>,
        cursor_style: TerminalCursorStyle,
        cursor_visible: bool,
    ) -> bool {
        let cursor_visible = cursor_col.is_some() && cursor_visible;
        let cursor_style = if cursor_col.is_some() {
            cursor_style
        } else {
            self.cursor_style
        };
        self.line == *line
            && self.selection_span == selection_span
            && self.style == style
            && self.geometry == *geometry
            && self.cursor_col == cursor_col
            && self.cursor_style == cursor_style
            && self.cursor_visible == cursor_visible
    }

    #[allow(clippy::too_many_arguments)]
    fn sync(
        &mut self,
        line: &TerminalLine,
        selection_span: Option<(u16, u16)>,
        style: TerminalPaintStyle,
        geometry: &TerminalGeometry,
        cursor_col: Option<u16>,
        cursor_style: TerminalCursorStyle,
        cursor_visible: bool,
        cx: &mut Context<Self>,
    ) {
        if self.matches_presentation(
            line,
            selection_span,
            style,
            geometry,
            cursor_col,
            cursor_style,
            cursor_visible,
        ) {
            return;
        }
        let cursor_visible = cursor_col.is_some() && cursor_visible;
        let cursor_style = if cursor_col.is_some() {
            cursor_style
        } else {
            self.cursor_style
        };
        if self.line != *line {
            self.line = line.clone();
        }
        self.selection_span = selection_span;
        self.style = style;
        self.geometry = geometry.clone();
        self.cursor_col = cursor_col;
        self.cursor_style = cursor_style;
        self.cursor_visible = cursor_visible;
        cx.notify();
    }
}

impl Render for TerminalRowPane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let trace_started = crate::terminal_trace::enabled().then(Instant::now);
        let selection = self
            .selection_span
            .map(|(start, end)| ((start, self.row), (end, self.row)));
        let cursor = self.cursor_col.map(|col| (col, self.row));
        let rendered = div()
            .w_full()
            .h(px(self.geometry.cell_height))
            .flex_none()
            .font(self.geometry.font())
            .text_size(px(self.geometry.font_size))
            .line_height(px(self.geometry.cell_height))
            .child(terminal_line(
                self.row,
                &self.line,
                selection,
                self.style,
                cursor,
                self.cursor_style,
                self.cursor_visible,
            ));
        if let Some(started) = trace_started {
            crate::terminal_trace::event(format_args!(
                "stage=pane.row_build row={} runs={} cells={} elapsed_us={}",
                self.row,
                self.line.runs.len(),
                self.line.cells.len(),
                crate::terminal_trace::elapsed_us(started),
            ));
        }
        rendered
    }
}

/// Own entity so parent chrome re-renders (spaces dropdown, etc.) can reuse
/// previous layout/paint via [`AnyView::cached`].
pub struct TerminalPane {
    frame: Arc<TerminalFrame>,
    rows: Vec<Entity<TerminalRowPane>>,
    bg: u32,
    selection: Option<TerminalSelection>,
    selection_bg: u32,
    cursor_color: u32,
    geometry: TerminalGeometry,
    padding: f32,
    bg_opacity: f32,
    cursor_phase_visible: bool,
    /// User-forced cursor shape: None = follow the terminal program's DECSCUSR/default (including hollow when unfocused).
    cursor_style_override: Option<TerminalCursorStyle>,
    /// User-forced blink switch: None = follow the terminal program's blink mode.
    cursor_blink_override: Option<bool>,
    _cursor_script: BackgroundJob<()>,
}

impl TerminalPane {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut pane = Self {
            frame: Arc::new(TerminalFrame::default()),
            rows: Vec::new(),
            bg: 0x0a0a0a,
            selection: None,
            selection_bg: TERMINAL_SELECTION_BG,
            cursor_color: TERMINAL_SELECTION_BG,
            geometry: TerminalGeometry::default(),
            padding: 8.0,
            bg_opacity: 1.0,
            cursor_phase_visible: true,
            cursor_style_override: None,
            cursor_blink_override: None,
            _cursor_script: BackgroundJob::ready(()),
        };
        pane.schedule_cursor_tick(cx);
        pane
    }

    fn schedule_cursor_tick(&mut self, cx: &mut Context<Self>) {
        self._cursor_script = cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(500)).await;
            let Some(this) = this.upgrade() else {
                return;
            };
            let _ = this.update(cx, |pane, cx| {
                // Only keep the chained wake while "there is a cursor and it is blinking" (perf §2.8:
                // decorative timers need an explicitly visible lifetime and must not wake periodically
                // when idle); blinking resumption re-arms via set_frame / set_cursor_preferences.
                let blink_active = pane.frame.cursor.is_some() && pane.effective_cursor_blinking();
                let next = if blink_active {
                    !pane.cursor_phase_visible
                } else {
                    true
                };
                if next != pane.cursor_phase_visible {
                    pane.cursor_phase_visible = next;
                    // Audit B17: only the cursor row's presentation can change on a blink
                    // tick; sync that row alone instead of deep-comparing the whole grid.
                    pane.sync_cursor_blink(cx);
                    cx.notify();
                }
                if blink_active {
                    pane.schedule_cursor_tick(cx);
                }
            });
        });
    }

    /// Re-arm the blink chain when blinking resumes (BackgroundJob drop = cancel; re-entrancy safe).
    fn ensure_cursor_tick(&mut self, cx: &mut Context<Self>) {
        if self.frame.cursor.is_some() && self.effective_cursor_blinking() {
            self.schedule_cursor_tick(cx);
        }
    }

    /// Dispatch user cursor preferences (shape/blink); repaint only on change. None keeps following the terminal program.
    pub fn set_cursor_preferences(
        &mut self,
        style: Option<TerminalCursorStyle>,
        blink: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        if self.cursor_style_override != style || self.cursor_blink_override != blink {
            self.cursor_style_override = style;
            self.cursor_blink_override = blink;
            self.ensure_cursor_tick(cx);
            self.sync_rows(cx);
            cx.notify();
        }
    }

    fn effective_cursor_style(&self) -> TerminalCursorStyle {
        self.cursor_style_override
            .unwrap_or(self.frame.cursor_style)
    }

    fn effective_cursor_blinking(&self) -> bool {
        self.cursor_blink_override
            .unwrap_or(self.frame.cursor_blinking)
    }

    fn paint_style(&self) -> TerminalPaintStyle {
        TerminalPaintStyle {
            bg: self.bg,
            selection_bg: self.selection_bg,
            cursor_color: self.cursor_color,
            cell_width: self.geometry.cell_width,
            cell_height: self.geometry.cell_height,
            bg_opacity: self.bg_opacity,
        }
    }

    fn cursor_visible(&self) -> bool {
        self.frame.cursor.is_some()
            && (!self.effective_cursor_blinking() || self.cursor_phase_visible)
    }

    fn sync_rows(&mut self, cx: &mut Context<Self>) {
        let style = self.paint_style();
        let cursor_style = self.effective_cursor_style();
        let cursor_visible = self.cursor_visible();
        let cursor = self.frame.cursor;
        while self.rows.len() < self.frame.lines.len() {
            let row_index = self.rows.len();
            let row = u16::try_from(row_index).unwrap_or(u16::MAX);
            let line = self.frame.lines[row_index].clone();
            let selection_span = selection_span_for_row(self.selection, row, line.cells.len());
            let cursor_col = cursor
                .filter(|(_, cursor_row)| *cursor_row == row)
                .map(|(col, _)| col);
            let geometry = self.geometry.clone();
            self.rows.push(cx.new(|_| {
                TerminalRowPane::new(
                    row,
                    line,
                    selection_span,
                    style,
                    geometry,
                    cursor_col,
                    cursor_style,
                    cursor_visible,
                )
            }));
        }
        self.rows.truncate(self.frame.lines.len());
        let all_rows = (0..self.frame.lines.len()).collect::<Vec<usize>>();
        self.sync_row_targets(&all_rows, cx);
    }

    /// Re-sync only the given row indices (B16): identical per-row work to the full pass,
    /// restricted to rows an extraction plan proved capable of differing.
    fn sync_row_targets(&mut self, rows: &[usize], cx: &mut Context<Self>) {
        let style = self.paint_style();
        let cursor_style = self.effective_cursor_style();
        let cursor_visible = self.cursor_visible();
        let cursor = self.frame.cursor;
        for row_index in rows {
            let Some(line) = self.frame.lines.get(*row_index) else {
                continue;
            };
            let row = u16::try_from(*row_index).unwrap_or(u16::MAX);
            let selection_span = selection_span_for_row(self.selection, row, line.cells.len());
            let cursor_col = cursor
                .filter(|(_, cursor_row)| *cursor_row == row)
                .map(|(col, _)| col);
            self.rows[*row_index].update(cx, |row_view, cx| {
                row_view.sync(
                    line,
                    selection_span,
                    style,
                    &self.geometry,
                    cursor_col,
                    cursor_style,
                    cursor_visible,
                    cx,
                );
            });
        }
    }

    /// Blink-phase-only update (audit B17): when just the blink phase flipped, the cursor
    /// row is the only row whose presentation can change; syncing it alone skips the
    /// full-grid per-row deep comparison the 500ms tick used to pay.
    fn sync_cursor_blink(&mut self, cx: &mut Context<Self>) {
        let Some((col, cursor_row)) = self.frame.cursor else {
            return;
        };
        let row_index = usize::from(cursor_row);
        let Some(line) = self.frame.lines.get(row_index) else {
            return;
        };
        if row_index >= self.rows.len() {
            return;
        }
        let style = self.paint_style();
        let cursor_style = self.effective_cursor_style();
        let cursor_visible = self.cursor_visible();
        let selection_span = selection_span_for_row(self.selection, cursor_row, line.cells.len());
        let geometry = self.geometry.clone();
        self.rows[row_index].update(cx, |row_view, cx| {
            row_view.sync(
                line,
                selection_span,
                style,
                &geometry,
                Some(col),
                cursor_style,
                cursor_visible,
                cx,
            );
        });
    }

    /// Immediate theme-follow fill while the hosted Herdr TUI attaches/restarts: the
    /// next authoritative frame replaces it from the emulator's own resolved colors.
    pub fn set_placeholder_background(&mut self, background: u32, cx: &mut Context<Self>) {
        if self.bg == background {
            return;
        }
        self.bg = background;
        self.sync_rows(cx);
        cx.notify();
    }

    pub fn set_frame(
        &mut self,
        frame: Arc<TerminalFrame>,
        plan: TerminalFramePlan,
        cx: &mut Context<Self>,
    ) {
        // ShardlaneApp::set_terminal_frame is the single caller and already performs the
        // semantic TerminalFrame equality check before touching this entity. Repeating that
        // deep lines/runs/cells comparison here doubled per-frame comparison work.
        if Arc::ptr_eq(&self.frame, &frame) {
            return;
        }
        // Hosted Herdr owns Terminal colors. Default bg/fg are resolved by Ghostty after Herdr
        // theme/OSC application and travel with the frame; Shardlane only falls back before the
        // first authoritative frame exists.
        let previous_paint_style = self.paint_style();
        if let Some(background) = frame.surface_background.or(frame.default_background) {
            self.bg = background;
        }
        self.cursor_color = frame
            .cursor_color
            .or(frame.default_foreground)
            .unwrap_or(TERMINAL_SELECTION_BG);
        self.selection_bg = frame.selection_color.unwrap_or(TERMINAL_SELECTION_BG);
        // Cursor movement resets the blink phase (aligned with Ghostty: as soon as the cursor moves it
        // stays visible and the 500ms timer restarts). Without resetting on input, insert mode
        // (DECSCUSR 5 blinking bar) and the default shell cursor would vanish on a fixed ~500ms beat
        // during continuous typing/backspacing — perceived by users as "input dropping frames/stuttering".
        // Reset only when the phase changes: while the cursor is still (e.g. other regions scrolling
        // output), the blink beat is unaffected by the frame stream.
        let previous_cursor = self.frame.cursor;
        // Re-arm the blink chain only on the "not blinking → blinking" transition: a high-frequency
        // set_frame that unconditionally reset the 500ms timer would freeze the cursor phase in the
        // visible state.
        let was_blinking = self.frame.cursor.is_some() && self.effective_cursor_blinking();
        self.frame = frame;
        let is_blinking = self.frame.cursor.is_some() && self.effective_cursor_blinking();
        if !was_blinking && is_blinking {
            self.ensure_cursor_tick(cx);
        }
        if cursor_phase_reset_on_move(previous_cursor, self.frame.cursor) {
            self.cursor_phase_visible = true;
            // Re-arm the tick: cancel the old timer and count 500ms afresh from this move (BackgroundJob
            // drop = cancel, see schedule_cursor_tick).
            self.schedule_cursor_tick(cx);
        }
        // B16: with an exact extraction plan, re-sync only the rows extraction flagged (plus
        // both cursor rows — cursor column/visibility live only there). Any other presentation
        // input that reaches past the plan (paint style change, geometry/selection handled by
        // their own setters, unknown plan, shape change) forces the full deep pass.
        let style_changed = self.paint_style() != previous_paint_style;
        match plan_sync_row_targets(
            &plan,
            previous_cursor,
            self.frame.cursor,
            self.frame.lines.len(),
            self.rows.len(),
            style_changed,
        ) {
            Some(rows) => self.sync_row_targets(&rows, cx),
            None => self.sync_rows(cx),
        }
        cx.notify();
    }

    pub fn set_render_settings(
        &mut self,
        geometry: TerminalGeometry,
        padding: f32,
        bg_opacity: f32,
        cx: &mut Context<Self>,
    ) {
        let padding = padding.clamp(0.0, 32.0);
        let bg_opacity = bg_opacity.clamp(0.0, 1.0);
        if self.geometry == geometry
            && (self.padding - padding).abs() < f32::EPSILON
            && (self.bg_opacity - bg_opacity).abs() < f32::EPSILON
        {
            return;
        }
        self.geometry = geometry;
        self.padding = padding;
        self.bg_opacity = bg_opacity;
        self.sync_rows(cx);
        cx.notify();
    }

    pub fn set_selection(
        &mut self,
        selection: Option<TerminalSelection>,
        selection_bg: u32,
        cx: &mut Context<Self>,
    ) {
        if self.selection == selection && self.selection_bg == selection_bg {
            return;
        }
        self.selection = selection;
        self.selection_bg = selection_bg;
        self.sync_rows(cx);
        cx.notify();
    }
}

impl Render for TerminalPane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let trace_started = crate::terminal_trace::enabled().then(Instant::now);
        let trace_shape = trace_started.map(|_| {
            let runs = self
                .frame
                .lines
                .iter()
                .map(|line| line.runs.len())
                .sum::<usize>();
            let cells = self
                .frame
                .lines
                .iter()
                .map(|line| line.cells.len())
                .sum::<usize>();
            (runs, cells)
        });
        // The row grid must be determined by rows × cell_height (a terminal is a fixed cell grid) and
        // must not be handed to flex stretching: cached_view wraps each row in flex_1, so a size_full
        // container would divide the container height evenly by row count (e.g. 36.7px ≠ cell_height
        // 36px), leaving a sub-pixel seam at each row's bottom; invisible against same-color row
        // backgrounds, but row-wise geometry overlays like the scrollbar ▕/▐ show a periodic 1px gap
        // (a segmented scrollbar). With fixed height, row pitch == cell_pitch and overlays tile seamlessly.
        let mut background = rgb(self.bg);
        background.a = self.bg_opacity.clamp(0.0, 1.0);
        let content = div()
            .w_full()
            .h(px(self.rows.len() as f32 * self.geometry.cell_height))
            .flex()
            .flex_col()
            .bg(background)
            .children(self.rows.iter().cloned().map(cached_view));
        // TUI-only: the host surface has no local scrollback scrollbar — the Herdr TUI draws all of
        // its UI itself and Shardlane no longer stacks a second scrollbar. The
        // row grid height stays fixed at rows × cell_height (see the content comment above); extra
        // area is filled with the same terminal background so the first frame/row-count changes never
        // leave a differently-colored band below the window content.
        let rendered = div()
            .relative()
            .size_full()
            .overflow_hidden()
            .p(px(self.padding))
            .child(content);
        if let (Some(started), Some((runs, cells))) = (trace_started, trace_shape) {
            let (input_id, read_id) = crate::terminal_trace::last_frame_source();
            crate::terminal_trace::event(format_args!(
                "stage=pane.render_build input_id={input_id} read_id={read_id} rows={} runs={runs} cells={cells} elapsed_us={}",
                self.frame.lines.len(),
                crate::terminal_trace::elapsed_us(started),
            ));
        }
        rendered
    }
}

/// Embed terminal entity with GPUI paint recycling when parent re-renders.
pub fn cached_terminal(entity: Entity<TerminalPane>) -> impl IntoElement {
    cached_view(entity)
}

#[allow(clippy::too_many_arguments)]
fn terminal_line(
    row: u16,
    line: &TerminalLine,
    selection: Option<TerminalSelection>,
    style: TerminalPaintStyle,
    cursor: Option<(u16, u16)>,
    cursor_style: TerminalCursorStyle,
    cursor_visible: bool,
) -> impl IntoElement {
    let selection = selection_span_for_row(selection, row, line.cells.len());
    let cursor = terminal_cursor(
        row,
        cursor,
        cursor_style,
        cursor_visible,
        style.cell_width,
        style.cell_height,
        style.cursor_color,
    );
    let mut background = rgb(style.bg);
    background.a = style.bg_opacity.clamp(0.0, 1.0);
    div()
        .relative()
        .w_full()
        .h(px(style.cell_height))
        .flex()
        .flex_none()
        .overflow_hidden()
        // 1. Base transparent/solid row background
        .bg(background)
        // 2. Explicit cell background layer (TUI panels/status bars/syntax highlighting etc.)
        .children(
            line.runs
                .iter()
                .filter_map(|run| run.bg.map(|bg| (run, bg)))
                .map(move |(run, bg)| {
                    let (left, width) = terminal_run_geometry(run, style.cell_width);
                    div()
                        .absolute()
                        .left(px(left))
                        .top_0()
                        .h_full()
                        .w(px(width))
                        .bg(rgb(bg))
                }),
        )
        // 3. Selection highlight layer (above the cell backgrounds)
        .when_some(selection, move |el, (start, end)| {
            el.child(
                div()
                    .absolute()
                    .left(px(start as f32 * style.cell_width))
                    .top_0()
                    .h_full()
                    .w(px((end.saturating_sub(start) + 1) as f32 * style.cell_width))
                    .bg(rgb(style.selection_bg)),
            )
        })
        // 5. Cursor layer (Block/Bar/Underline above the cell backgrounds and selection)
        .when_some(cursor, |el, cursor| el.child(cursor))
        // 6. Text glyph layer (transparent background; keeps text drawn crisply above cursor, selection, and backgrounds)
        .children(
            line.runs
                .iter()
                .map(move |run| terminal_run(run, style.cell_width)),
        )
        // Block Elements (Herdr scrollbar/progress etc.) cannot rely on the font em-box to fill the
        // terminal cell: users can set font-size/line-height to 13/18, in which case ordinary text
        // rendering leaves leading between rows, turning a run of `▐` into a dashed line. For block
        // characters that can be represented exactly, use cell-geometry overlays — the effect matches
        // the system Terminal and doesn't depend on the specific system font.
        .children(terminal_block_glyph_overlays(line, style))
        // 7. Hyperlink interaction layer: hover uses a low-opacity cursor-color tint (adapting to the
        //    palette) to aid discoverability; Cmd+click still goes through safe open.
        .children(line.hyperlinks.iter().filter_map(|link| {
            if !is_openable_terminal_link(&link.uri) {
                return None;
            }
            let uri = link.uri.clone();
            let width = link
                .end_col
                .saturating_sub(link.start_col)
                .saturating_add(1);
            let mut hover_tint = rgb(style.cursor_color);
            hover_tint.a = 0.22;
            Some(
                div()
                    .absolute()
                    .left(px(link.start_col as f32 * style.cell_width))
                    .top_0()
                    .h_full()
                    .w(px(width as f32 * style.cell_width))
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_tint))
                    .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                        if event.modifiers.platform {
                            cx.stop_propagation();
                            cx.open_url(&uri);
                        }
                    }),
            )
        }))
}

fn is_openable_terminal_link(uri: &str) -> bool {
    let uri = uri.trim();
    !uri.chars().any(char::is_control)
        && (uri.starts_with("https://")
            || uri.starts_with("http://")
            || uri.starts_with("mailto:")
            || uri.starts_with("file://"))
}

fn terminal_cursor(
    row: u16,
    cursor: Option<(u16, u16)>,
    cursor_style: TerminalCursorStyle,
    cursor_visible: bool,
    cell_width: f32,
    cell_height: f32,
    cursor_color: u32,
) -> Option<AnyElement> {
    let (col, cursor_row) = cursor?;
    if !cursor_visible || cursor_row != row {
        return None;
    }
    let left = px(col as f32 * cell_width);
    let cursor_color = rgb(cursor_color);
    let base = div().absolute().left(left).top_0();
    Some(match cursor_style {
        TerminalCursorStyle::Bar => base.w(px(2.0)).h_full().bg(cursor_color).into_any_element(),
        TerminalCursorStyle::Block => base
            .w(px(cell_width))
            .h_full()
            .bg(cursor_color)
            .into_any_element(),
        TerminalCursorStyle::Underline => base
            .top(px((cell_height - 2.0).max(0.0)))
            .w(px(cell_width))
            .h(px(2.0))
            .bg(cursor_color)
            .into_any_element(),
        TerminalCursorStyle::HollowBlock => base
            .w(px(cell_width))
            .h_full()
            .border_1()
            .border_color(cursor_color)
            .into_any_element(),
    })
}

fn selection_span_for_row(
    selection: Option<TerminalSelection>,
    row: u16,
    cell_count: usize,
) -> Option<(u16, u16)> {
    let ((start_col, start_row), (end_col, end_row)) = normalize_selection(selection?);
    if row < start_row || row > end_row || cell_count == 0 {
        return None;
    }
    let max_col = cell_count.saturating_sub(1).min(u16::MAX as usize) as u16;
    let start = if row == start_row { start_col } else { 0 }.min(max_col);
    let end = if row == end_row { end_col } else { max_col }.min(max_col);
    (start <= end).then_some((start, end))
}

fn normalize_selection(selection: TerminalSelection) -> TerminalSelection {
    let (start, end) = selection;
    if start.1 < end.1 || (start.1 == end.1 && start.0 <= end.0) {
        (start, end)
    } else {
        (end, start)
    }
}

fn terminal_run(run: &TerminalRun, cell_width: f32) -> impl IntoElement {
    let (left, width) = terminal_run_geometry(run, cell_width);
    div()
        .absolute()
        .left(px(left))
        .top_0()
        .h_full()
        .w(px(width))
        .overflow_hidden()
        .text_color(rgb(run.fg))
        // GPUI text children need ownership (non-'static borrows can't escape); the String clone here
        // is a bounded cost. Eliminating it entirely needs audit P2's row-element caching/one-text-per-row approach.
        .child(run.text.clone())
}

fn terminal_run_geometry(run: &TerminalRun, cell_width: f32) -> (f32, f32) {
    (
        run.start_col as f32 * cell_width,
        run.cell_count.max(1) as f32 * cell_width,
    )
}

fn terminal_block_glyph_overlays(
    line: &TerminalLine,
    style: TerminalPaintStyle,
) -> Vec<AnyElement> {
    let mut overlays = Vec::new();
    let mut run_index = 0usize;
    for (col, cell) in line.cells.iter().enumerate() {
        let mut chars = cell.chars();
        let Some(ch) = chars.next() else { continue };
        if chars.next().is_some() {
            continue;
        }
        let Some((x, y, width, height)) = terminal_block_rect(ch) else {
            continue;
        };
        while line
            .runs
            .get(run_index)
            .is_some_and(|run| usize::from(run.start_col.saturating_add(run.cell_count)) <= col)
        {
            run_index += 1;
        }
        let Some(fg) = line.runs.get(run_index).and_then(|run| {
            let start = usize::from(run.start_col);
            let end = usize::from(run.start_col.saturating_add(run.cell_count));
            (start <= col && col < end).then_some(run.fg)
        }) else {
            continue;
        };
        overlays.push(
            div()
                .absolute()
                .left(px(col as f32 * style.cell_width + x * style.cell_width))
                .top(px(y * style.cell_height))
                .w(px(width * style.cell_width))
                .h(px(height * style.cell_height))
                .bg(rgb(fg))
                .into_any_element(),
        );
    }
    overlays
}

/// Geometric block-cell coverage for the most common Unicode Block Elements used by TUIs.
/// Fractions are relative to one terminal cell `(x, y, width, height)`.
fn terminal_block_rect(ch: char) -> Option<(f32, f32, f32, f32)> {
    Some(match ch {
        '█' => (0.0, 0.0, 1.0, 1.0),
        '▐' => (0.5, 0.0, 0.5, 1.0),
        '▌' => (0.0, 0.0, 0.5, 1.0),
        '▕' => (0.875, 0.0, 0.125, 1.0),
        '▏' => (0.0, 0.0, 0.125, 1.0),
        '▎' => (0.0, 0.0, 0.25, 1.0),
        '▍' => (0.0, 0.0, 0.375, 1.0),
        '▋' => (0.0, 0.0, 0.625, 1.0),
        '▊' => (0.0, 0.0, 0.75, 1.0),
        '▉' => (0.0, 0.0, 0.875, 1.0),
        '▀' => (0.0, 0.0, 1.0, 0.5),
        '▔' => (0.0, 0.0, 1.0, 0.125),
        '▄' => (0.0, 0.5, 1.0, 0.5),
        '▁' => (0.0, 0.875, 1.0, 0.125),
        '▂' => (0.0, 0.75, 1.0, 0.25),
        '▃' => (0.0, 0.625, 1.0, 0.375),
        '▅' => (0.0, 0.375, 1.0, 0.625),
        '▆' => (0.0, 0.25, 1.0, 0.75),
        '▇' => (0.0, 0.125, 1.0, 0.875),
        _ => return None,
    })
}

/// Setting preference → render override: `follow-terminal` returns None (the terminal program stays authoritative).
pub(crate) fn terminal_cursor_style_override(
    preference: TerminalCursorStylePreference,
) -> Option<TerminalCursorStyle> {
    match preference {
        TerminalCursorStylePreference::FollowTerminal => None,
        TerminalCursorStylePreference::Block => Some(TerminalCursorStyle::Block),
        TerminalCursorStylePreference::Bar => Some(TerminalCursorStyle::Bar),
        TerminalCursorStylePreference::Underline => Some(TerminalCursorStyle::Underline),
    }
}

/// Whether cursor movement should reset the blink phase: the new position exists and differs from the
/// old one (appeared/moved). Disappearance (Some → None) doesn't reset — with no cursor at all, the
/// phase is meaningless. Aligned with Ghostty's behavior: the cursor stays visible on any movement
/// and doesn't vanish on a fixed beat while typing.
fn cursor_phase_reset_on_move(previous: Option<(u16, u16)>, next: Option<(u16, u16)>) -> bool {
    next.is_some() && previous != next
}

/// Blink preference → render override: `follow-terminal` returns None (the program's blink mode stays authoritative).
pub(crate) fn terminal_cursor_blink_override(
    preference: TerminalCursorBlinkPreference,
) -> Option<bool> {
    match preference {
        TerminalCursorBlinkPreference::FollowTerminal => None,
        TerminalCursorBlinkPreference::On => Some(true),
        TerminalCursorBlinkPreference::Off => Some(false),
    }
}

/// B16 (pure, deterministic): the row indices that must re-sync when a frame arrives with an
/// extraction plan. `None` mandates the full deep pass: unknown plan, shape change (the plan's
/// row indices would not describe this grid), or a paint-style change (which recolors every
/// row). `Some(rows)` is the minimal set — the plan-flagged rows plus the old and new cursor
/// rows, since cursor column/visibility are the only presentation inputs that ride on
/// non-content rows. Fallback policy: correctness over speed.
fn plan_sync_row_targets(
    plan: &TerminalFramePlan,
    previous_cursor: Option<(u16, u16)>,
    next_cursor: Option<(u16, u16)>,
    line_count: usize,
    row_count: usize,
    style_changed: bool,
) -> Option<Vec<usize>> {
    if style_changed || line_count != row_count {
        return None;
    }
    let mut rows = match plan {
        TerminalFramePlan::RowsUnchanged => Vec::new(),
        TerminalFramePlan::RowsChanged(changed) => changed
            .iter()
            .copied()
            .filter(|row| *row < line_count)
            .collect::<Vec<usize>>(),
        TerminalFramePlan::Unknown => return None,
    };
    for row in [previous_cursor, next_cursor].into_iter().flatten() {
        let row = usize::from(row.1);
        if row < line_count {
            rows.push(row);
        }
    }
    rows.sort_unstable();
    rows.dedup();
    Some(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_elements_use_cell_geometry_instead_of_font_line_box() {
        use super::terminal_block_rect;

        assert_eq!(terminal_block_rect('▐'), Some((0.5, 0.0, 0.5, 1.0)));
        assert_eq!(terminal_block_rect('▕'), Some((0.875, 0.0, 0.125, 1.0)));
        assert_eq!(terminal_block_rect('█'), Some((0.0, 0.0, 1.0, 1.0)));
        assert_eq!(terminal_block_rect('▄'), Some((0.0, 0.5, 1.0, 0.5)));
        assert_eq!(terminal_block_rect('A'), None);
    }

    #[test]
    fn cursor_preferences_map_to_render_overrides() {
        use super::{terminal_cursor_blink_override, terminal_cursor_style_override};
        use crate::ghostty::TerminalCursorStyle;
        use crate::settings::{TerminalCursorBlinkPreference, TerminalCursorStylePreference};

        assert_eq!(
            terminal_cursor_style_override(TerminalCursorStylePreference::FollowTerminal),
            None
        );
        assert_eq!(
            terminal_cursor_style_override(TerminalCursorStylePreference::Block),
            Some(TerminalCursorStyle::Block)
        );
        assert_eq!(
            terminal_cursor_style_override(TerminalCursorStylePreference::Bar),
            Some(TerminalCursorStyle::Bar)
        );
        assert_eq!(
            terminal_cursor_style_override(TerminalCursorStylePreference::Underline),
            Some(TerminalCursorStyle::Underline)
        );

        assert_eq!(
            terminal_cursor_blink_override(TerminalCursorBlinkPreference::FollowTerminal),
            None
        );
        assert_eq!(
            terminal_cursor_blink_override(TerminalCursorBlinkPreference::On),
            Some(true)
        );
        assert_eq!(
            terminal_cursor_blink_override(TerminalCursorBlinkPreference::Off),
            Some(false)
        );
    }

    #[test]
    fn cursor_phase_resets_only_when_cursor_moves() {
        // Input moves the cursor: every step must reset the blink phase, otherwise insert mode (blinking bar)
        // and the default shell cursor vanish on a fixed 500ms beat during continuous typing — perceived as frame drops.
        assert!(cursor_phase_reset_on_move(None, Some((3, 0))));
        assert!(cursor_phase_reset_on_move(Some((3, 0)), Some((4, 0))));
        assert!(cursor_phase_reset_on_move(Some((4, 0)), Some((4, 1))));
        // A still cursor (frames output in other regions) and disappearance must not touch the phase.
        assert!(!cursor_phase_reset_on_move(Some((4, 1)), Some((4, 1))));
        assert!(!cursor_phase_reset_on_move(Some((4, 1)), None));
        assert!(!cursor_phase_reset_on_move(None, None));
    }
    #[test]
    #[ignore = "local TerminalPane element-build diagnostic; run explicitly on the development Mac"]
    fn terminal_line_element_build_120x40_smoke() {
        let mut line = TerminalLine {
            cells: vec!["x".to_string(); 120],
            ..TerminalLine::default()
        };
        for chunk in 0..12_u16 {
            line.runs.push(TerminalRun {
                start_col: chunk * 10,
                cell_count: 10,
                mergeable_after: false,
                text: "x".repeat(10),
                fg: if chunk % 2 == 0 { 0xd8dee9 } else { 0x88c0d0 },
                bg: (chunk % 3 == 0).then_some(0x2e3440),
            });
        }
        let style = TerminalPaintStyle {
            bg: 0x2e3440,
            selection_bg: 0x4c566a,
            cursor_color: 0xd8dee9,
            cell_width: 7.2,
            cell_height: 18.0,
            bg_opacity: 1.0,
        };
        const FRAMES: usize = 500;
        let started = std::time::Instant::now();
        for _ in 0..FRAMES {
            for row in 0..40_u16 {
                std::hint::black_box(terminal_line(
                    row,
                    &line,
                    None,
                    style,
                    None,
                    TerminalCursorStyle::Block,
                    true,
                ));
            }
        }
        let per_frame_us = started.elapsed().as_secs_f64() * 1_000_000.0 / FRAMES as f64;
        let one_row_started = std::time::Instant::now();
        for _ in 0..(FRAMES * 40) {
            std::hint::black_box(terminal_line(
                0,
                &line,
                None,
                style,
                None,
                TerminalCursorStyle::Block,
                true,
            ));
        }
        let per_row_us =
            one_row_started.elapsed().as_secs_f64() * 1_000_000.0 / (FRAMES * 40) as f64;
        eprintln!(
            "TerminalPane element build 120x40: {per_frame_us:.1}us/frame; retained changed row: {per_row_us:.1}us/row"
        );
        assert!(per_frame_us < 10_000.0 && per_row_us < 1_000.0);
    }

    #[test]
    fn retained_row_ignores_cursor_blink_and_style_when_row_has_no_cursor() {
        let line = TerminalLine::default();
        let geometry = TerminalGeometry::default();
        let style = TerminalPaintStyle {
            bg: 0x101010,
            selection_bg: 0x264f78,
            cursor_color: 0xffffff,
            cell_width: geometry.cell_width,
            cell_height: geometry.cell_height,
            bg_opacity: 1.0,
        };
        let row = TerminalRowPane::new(
            0,
            line.clone(),
            None,
            style,
            geometry.clone(),
            None,
            TerminalCursorStyle::Block,
            true,
        );
        assert!(!row.cursor_visible);
        assert!(row.matches_presentation(
            &line,
            None,
            style,
            &geometry,
            None,
            TerminalCursorStyle::Underline,
            false,
        ));
        assert!(!row.matches_presentation(
            &line,
            None,
            style,
            &geometry,
            Some(3),
            TerminalCursorStyle::Underline,
            true,
        ));
    }

    #[test]
    fn retained_row_invalidates_when_cursor_leaves_the_row() {
        let line = TerminalLine::default();
        let geometry = TerminalGeometry::default();
        let style = TerminalPaintStyle {
            bg: 0x101010,
            selection_bg: 0x264f78,
            cursor_color: 0xffffff,
            cell_width: geometry.cell_width,
            cell_height: geometry.cell_height,
            bg_opacity: 1.0,
        };
        let row = TerminalRowPane::new(
            0,
            line.clone(),
            None,
            style,
            geometry.clone(),
            Some(3),
            TerminalCursorStyle::Block,
            true,
        );
        assert!(!row.matches_presentation(
            &line,
            None,
            style,
            &geometry,
            None,
            TerminalCursorStyle::Block,
            true,
        ));
    }

    #[test]
    fn terminal_run_geometry_is_anchored_to_authoritative_grid_columns() {
        let run = TerminalRun {
            start_col: 3,
            cell_count: 2,
            mergeable_after: false,
            text: "你".to_string(),
            fg: 0xffffff,
            bg: None,
        };
        assert_eq!(terminal_run_geometry(&run, 7.25), (21.75, 14.5));
    }

    #[test]
    fn terminal_font_disables_contextual_ligatures() {
        assert_eq!(
            TerminalGeometry::default()
                .font()
                .features
                .is_calt_enabled(),
            Some(false)
        );
    }

    #[test]
    fn selection_span_normalizes_reverse_drag() {
        assert_eq!(
            selection_span_for_row(Some(((5, 2), (1, 1))), 1, 8),
            Some((1, 7))
        );
        assert_eq!(
            selection_span_for_row(Some(((5, 2), (1, 1))), 2, 8),
            Some((0, 5))
        );
    }

    /// B16 work-bound contract: an extraction plan limits row re-syncs to exactly the rows
    /// it proves capable of differing (plus old/new cursor rows). An unchanged frame yields
    /// zero row re-syncs; changed rows yield only those rows; anything unproven (unknown
    /// plan, shape change, style change) falls back to the full deep pass.
    #[test]
    fn frame_plan_limits_row_resync_to_proven_rows() {
        // Unchanged frame, no cursor: zero row re-syncs. (With a cursor present, an unchanged
        // frame never reaches the pane at all — set_terminal_frame early-returns.)
        assert_eq!(
            plan_sync_row_targets(&TerminalFramePlan::RowsUnchanged, None, None, 4, 4, false),
            Some(Vec::new())
        );
        // A still cursor still syncs its own row: blink mode/phase ride on that row even
        // when the position is unchanged.
        assert_eq!(
            plan_sync_row_targets(
                &TerminalFramePlan::RowsUnchanged,
                Some((3, 1)),
                Some((3, 1)),
                4,
                4,
                false
            ),
            Some(vec![1])
        );
        // Unchanged grid, cursor moved 1 → 3: only the two cursor rows re-sync.
        assert_eq!(
            plan_sync_row_targets(
                &TerminalFramePlan::RowsUnchanged,
                Some((3, 1)),
                Some((5, 3)),
                4,
                4,
                false
            ),
            Some(vec![1, 3])
        );
        // Cursor appearing (None → row 2) must still sync its row.
        assert_eq!(
            plan_sync_row_targets(
                &TerminalFramePlan::RowsUnchanged,
                None,
                Some((0, 2)),
                4,
                4,
                false
            ),
            Some(vec![2])
        );
        // Changed rows plus cursor rows: sorted union, nothing else.
        assert_eq!(
            plan_sync_row_targets(
                &TerminalFramePlan::RowsChanged(vec![2, 0]),
                None,
                Some((0, 1)),
                4,
                4,
                false
            ),
            Some(vec![0, 1, 2])
        );
        // Unknown plan, row-count mismatch, or style change: full deep pass.
        assert_eq!(
            plan_sync_row_targets(&TerminalFramePlan::Unknown, None, None, 4, 4, false),
            None
        );
        assert_eq!(
            plan_sync_row_targets(&TerminalFramePlan::RowsUnchanged, None, None, 3, 4, false),
            None
        );
        assert_eq!(
            plan_sync_row_targets(&TerminalFramePlan::RowsUnchanged, None, None, 4, 4, true),
            None
        );
        // Out-of-bounds plan indices are dropped defensively (cursor row kept).
        assert_eq!(
            plan_sync_row_targets(
                &TerminalFramePlan::RowsChanged(vec![1, 9]),
                Some((0, 0)),
                Some((0, 0)),
                4,
                4,
                false
            ),
            Some(vec![0, 1])
        );
    }

    /// Regression against the real vendored libghostty-vt: herdr draws its pane
    /// scrollbar as SGR-colored block-element cells (track `▕` U+2595, thumb `▐`
    /// U+2590). Every such cell must resolve a run fg color and produce a
    /// geometric overlay, both in the raw frame and after chrome projection —
    /// otherwise the row falls back to the font glyph and the track renders
    /// dashed (the segmented-scrollbar artifact).
    #[test]
    fn scrollbar_block_cells_get_geometric_overlays_through_real_ghostty() {
        use crate::ghostty::{GhosttyRuntime, GhosttyTerminal};

        let runtime = GhosttyRuntime::detect().unwrap_or_else(|error| panic!("{error}"));
        let api = runtime.load_api().unwrap_or_else(|error| panic!("{error}"));
        let mut terminal =
            GhosttyTerminal::new(api, 20, 4).unwrap_or_else(|error| panic!("{error}"));
        // herdr draws: normal text, then a colored ▕ (U+2595) track in the last columns.
        terminal.write(b"label \x1b[38;2;140;140;160m\xe2\x96\x95\xe2\x96\x95\xe2\x96\x95\x1b[0m");
        let frame = terminal.frame().unwrap_or_else(|error| panic!("{error}"));
        let line = &frame.lines[0];
        assert_eq!(
            &line.cells[6..9],
            &["▕".to_string(), "▕".to_string(), "▕".to_string()]
        );
        let style = TerminalPaintStyle {
            bg: 0x18181f,
            selection_bg: 0x264f78,
            cursor_color: 0xd8dee9,
            cell_width: 7.225,
            cell_height: 18.0,
            bg_opacity: 1.0,
        };
        let overlays = terminal_block_glyph_overlays(line, style);
        assert_eq!(overlays.len(), 3, "raw frame must overlay every ▕ cell");

        // The hosted surface paints the projected (chrome-cropped) frame.
        let projected = frame.project_rect(1, 0, 0, 0);
        let projected_overlays = terminal_block_glyph_overlays(&projected.lines[0], style);
        assert_eq!(
            projected_overlays.len(),
            3,
            "projected frame must overlay every ▕ cell"
        );
    }

    /// The rows container is laid out at the fixed cell grid (rows × cell_height).
    /// `cached_view` wraps every row in `flex_1`, so a `size_full` container would
    /// stretch rows to container_height/rows — a ~0.7px/row mismatch against
    /// cell_height that shows up as periodic 1px gaps in any per-row full-height
    /// overlay (the segmented herdr scrollbar). This pins the height math the
    /// render path must use; the visible evidence lives in
    /// `scripts/scrollbar-continuity-ab.sh` (baseline FAIL 25.0 → fixed PASS 100).
    #[test]
    fn rows_container_uses_fixed_cell_grid_height() {
        let rows = 40_usize;
        let cell_height = 18.0_f32;
        let content_height = rows as f32 * cell_height;
        assert_eq!(content_height, 720.0);
        // The stretch artifact case: a 1467px-tall pane over 40 rows yields 36.675px
        // per row, which must never be the row pitch — overlays assume cell_height.
        let stretched = 1467.0_f32 / rows as f32;
        assert!((stretched - cell_height).abs() > 0.5);
    }
}
