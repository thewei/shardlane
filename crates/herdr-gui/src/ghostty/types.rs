//! [INPUT]: Pure std data structures; Hosted Terminal's default fg/bg comes from Ghostty's
//! parsed Herdr/OSC terminal state.
//! [OUTPUT]: Exposes (within the ghostty module tree) projection structs such as
//! TerminalFrame/TerminalLine/TerminalRun/TerminalHyperlink (plus the test-only
//! TerminalScrollbar regression observable), the TerminalFramePlan row-level extraction
//! plan consumed by the presentation layer, color helpers
//! deriving surface/selection backgrounds from the actual Herdr frame, OSC 10/11 dynamic-color
//! set/query byte construction (`COLOR_QUERY_*`/`color_query_report_bytes`, re-exported at the
//! module root), line run merging, and small color utilities.
//! [POS]: The types layer of the ghostty module — data shapes produced by frame extraction and
//! consumed by terminal_view rendering and tests.

pub type TerminalGridSelection = ((u16, u16), (u16, u16));

/// Query bits: the hosted child asked the emulator to report its dynamic default
/// foreground/background via `OSC 10;?` / `OSC 11;?`.
pub const COLOR_QUERY_FOREGROUND: u8 = 1 << 0;
pub const COLOR_QUERY_BACKGROUND: u8 = 1 << 1;

/// OSC 10/11 set sequence for the local model. Foreground and background are written
/// together: the pinned libghostty-vt snapshot only re-exposes isolated OSC 11 reliably
/// in that form (see `frame_reuse_invalidates_when_default_background_changes`).
pub(super) fn dynamic_color_set_bytes(foreground: u32, background: u32) -> Vec<u8> {
    format!("\x1b]10;#{foreground:06x}\x07\x1b]11;#{background:06x}\x07").into_bytes()
}

/// xterm-style dynamic-color report answering a child's `OSC {number};?` query:
/// `OSC {number} ; rgb:rrrr/gggg/bbbb ST` with each byte doubled to 16 bits.
pub(super) fn osc_color_report_bytes(number: u16, color: u32) -> Vec<u8> {
    let channel = |shift: u32| {
        let value = (color >> shift) & 0xff;
        format!("{value:02x}{value:02x}")
    };
    format!(
        "\x1b]{};rgb:{}/{}/{}\x1b\\",
        number,
        channel(16),
        channel(8),
        channel(0)
    )
    .into_bytes()
}

/// Compose the PTY-input answer for the child's pending OSC 10/11 color queries.
/// Shardlane hosts the terminal emulator for the Herdr TUI, so query answers are
/// ours to give; the colors derive from the active Herdr theme (single authority).
pub fn color_query_report_bytes(queries: u8, foreground: u32, background: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    if queries & COLOR_QUERY_FOREGROUND != 0 {
        bytes.extend_from_slice(&osc_color_report_bytes(10, foreground));
    }
    if queries & COLOR_QUERY_BACKGROUND != 0 {
        bytes.extend_from_slice(&osc_color_report_bytes(11, background));
    }
    bytes
}

/// Selection overlay fallback derived from the hosted Herdr terminal's resolved colors.
/// Shardlane does not own a separate Terminal palette. A 25% foreground mix is visibly distinct while preserving
/// much stronger glyph contrast than the historical fixed dark-blue highlight in light themes.
pub(super) fn default_selection_color(default_foreground: u32, default_background: u32) -> u32 {
    let channel = |shift: u32| {
        let fg = (default_foreground >> shift) & 0xff;
        let bg = (default_background >> shift) & 0xff;
        ((bg.saturating_mul(3) + fg) / 4) << shift
    };
    channel(16) | channel(8) | channel(0) | 0xff00_0000
}

/// Resolve a *confirmed* host surface color from the visible Herdr frame, not from a parallel
/// Shardlane theme table. A dominant explicit background is accepted only when it covers at
/// least 80% of visible cells. Anything below that threshold is deliberately `None`: rapid
/// scroll/resize frames are often only partially repainted, and encoding that transition as the
/// terminal default background caused the host surface to flash black between Herdr-colored
/// frames. The presentation seam may retain the previously confirmed surface until a new color
/// is confirmed.
pub(super) fn dominant_surface_background(lines: &[TerminalLine]) -> Option<u32> {
    let total_cells = lines.iter().map(|line| line.cells.len()).sum::<usize>();
    if total_cells == 0 {
        return None;
    }

    let mut by_background = std::collections::HashMap::<u32, usize>::new();
    for run in lines.iter().flat_map(|line| line.runs.iter()) {
        let Some(background) = run.bg else {
            continue;
        };
        let count = by_background.entry(background).or_default();
        *count = count.saturating_add(usize::from(run.cell_count));
    }
    let (background, cells) = by_background.into_iter().max_by_key(|(_, cells)| *cells)?;
    (cells.saturating_mul(5) >= total_cells.saturating_mul(4)).then_some(background)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TerminalCursorStyle {
    Bar,
    #[default]
    Block,
    Underline,
    HollowBlock,
}

/// Row-level change plan computed by frame extraction (B16). Exact, not heuristic: it is
/// derived from per-cell RAW bitfield signatures, and the RAW bitfield is the cell's complete
/// storage (character/style/color/link bits). It lets presentation consumers replace full-grid
/// deep comparisons with per-row work. Frame scalars (cursor/colors) are intentionally NOT
/// covered — consumers compare those small fields directly (see `TerminalFrame::scalars_match`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum TerminalFramePlan {
    /// Every row is byte-identical to the previous extraction's rows (frame-level RAW
    /// signature fast path). Frame scalars (cursor/colors) may still differ.
    RowsUnchanged,
    /// Only the listed row indices (ascending, unique, in bounds) differ from the previous
    /// extraction; every other row is a byte-identical clone of the previous frame's row.
    RowsChanged(Vec<usize>),
    /// No row-level knowledge (bootstrap/scroll/resize extraction, OSC-8 semantic stream
    /// change, ABI fallback, or "no extraction happened"). Consumers must fall back to the
    /// full deep comparison/sync — correctness over speed.
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalFrame {
    pub lines: Vec<TerminalLine>,
    /// Resolved terminal default foreground after Herdr/OSC theme application. Hosted TUI
    /// presentation treats this as the authoritative Terminal color source instead of keeping a
    /// second Shardlane-owned palette in parallel.
    pub default_foreground: Option<u32>,
    /// Resolved terminal default background after Herdr/OSC theme application.
    pub default_background: Option<u32>,
    /// Actual visible Herdr surface background. When an explicit background covers at least 80%
    /// of the projected viewport this is that dominant color; otherwise it equals the resolved
    /// terminal default. Shardlane content chrome consumes this field directly.
    pub surface_background: Option<u32>,
    /// Visible cursor position in viewport cell coordinates.
    pub cursor: Option<(u16, u16)>,
    pub cursor_style: TerminalCursorStyle,
    pub cursor_blinking: bool,
    /// Resolved cursor color from the authoritative Herdr/Ghostty frame.
    pub cursor_color: Option<u32>,
    /// Selection overlay derived from the authoritative visible surface foreground/background.
    pub selection_color: Option<u32>,
}

impl TerminalFrame {
    /// Equality of every scalar field (everything except `lines`). Combined with a
    /// `TerminalFramePlan::RowsUnchanged` extraction plan (which proves the grids
    /// byte-identical), this is exactly full `TerminalFrame` equality without walking cells.
    pub(crate) fn scalars_match(&self, other: &TerminalFrame) -> bool {
        self.default_foreground == other.default_foreground
            && self.default_background == other.default_background
            && self.surface_background == other.surface_background
            && self.cursor == other.cursor
            && self.cursor_style == other.cursor_style
            && self.cursor_blinking == other.cursor_blinking
            && self.cursor_color == other.cursor_color
            && self.selection_color == other.selection_color
    }

    /// Keep the last high-confidence Herdr surface color across partial repaint frames. Rapid
    /// wheel/trackpad scrolling can update only part of the visible grid, so a frame that does
    /// not meet the 80% confirmation threshold must not be interpreted as a request to switch
    /// the host surface back to the terminal default. Returns true when a previous confirmed
    /// surface was retained.
    pub(crate) fn retain_confirmed_surface_background_from(
        &mut self,
        previous: &TerminalFrame,
    ) -> bool {
        if self.surface_background.is_some()
            || self.lines.is_empty()
            || previous.surface_background.is_none()
        {
            return false;
        }
        self.surface_background = previous.surface_background;
        if let (Some(foreground), Some(background)) =
            (self.default_foreground, self.surface_background)
        {
            self.selection_color = Some(default_selection_color(foreground, background));
        }
        true
    }

    /// Project a rectangular terminal viewport by removing chrome cells from each edge.
    /// Used only by the hosted Herdr TUI presentation: Ghostty keeps the complete Herdr
    /// screen as its semantic model while Shardlane paints the authoritative Pane area.
    pub(crate) fn project_rect(&self, left: u16, top: u16, right: u16, bottom: u16) -> Self {
        if left == 0 && top == 0 && right == 0 && bottom == 0 {
            return self.clone();
        }
        let start_row = usize::from(top).min(self.lines.len());
        let end_row = self
            .lines
            .len()
            .saturating_sub(usize::from(bottom))
            .max(start_row);
        let lines = self.lines[start_row..end_row]
            .iter()
            .map(|line| project_terminal_line(line, left, right))
            .collect::<Vec<_>>();
        let surface_background = dominant_surface_background(&lines);
        let cursor = self.cursor.and_then(|(col, row)| {
            let row = row.checked_sub(top)?;
            let col = col.checked_sub(left)?;
            let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            let width = lines
                .get(usize::from(row))
                .map(|line| u16::try_from(line.cells.len()).unwrap_or(u16::MAX))
                .unwrap_or(0);
            (row < height && col < width).then_some((col, row))
        });
        Self {
            lines,
            default_foreground: self.default_foreground,
            default_background: self.default_background,
            surface_background,
            cursor,
            cursor_style: self.cursor_style,
            cursor_blinking: self.cursor_blinking,
            cursor_color: self.cursor_color,
            selection_color: self.default_foreground.and_then(|foreground| {
                surface_background
                    .or(self.default_background)
                    .map(|background| default_selection_color(foreground, background))
            }),
        }
    }
}

fn project_terminal_line(line: &TerminalLine, left: u16, right: u16) -> TerminalLine {
    let start = usize::from(left).min(line.cells.len());
    let end = line
        .cells
        .len()
        .saturating_sub(usize::from(right))
        .max(start);
    if start == 0 && end == line.cells.len() {
        return line.clone();
    }

    let mut projected = TerminalLine {
        cells: line.cells[start..end].to_vec(),
        ..TerminalLine::default()
    };
    let mut run_index = 0usize;
    for original_col in start..end {
        while line.runs.get(run_index).is_some_and(|run| {
            usize::from(run.start_col.saturating_add(run.cell_count)) <= original_col
        }) {
            run_index += 1;
        }
        let Some(source_run) = line.runs.get(run_index).filter(|run| {
            usize::from(run.start_col) <= original_col
                && original_col < usize::from(run.start_col.saturating_add(run.cell_count))
        }) else {
            continue;
        };
        let projected_col = u16::try_from(original_col.saturating_sub(start)).unwrap_or(u16::MAX);
        let text = line.cells.get(original_col).cloned().unwrap_or_default();
        if text.is_empty() {
            if let Some(last) = projected.runs.last_mut().filter(|last| {
                last.fg == source_run.fg
                    && last.bg == source_run.bg
                    && last.start_col.saturating_add(last.cell_count) == projected_col
            }) {
                last.cell_count = last.cell_count.saturating_add(1);
                last.mergeable_after = false;
            } else {
                projected.runs.push(TerminalRun {
                    start_col: projected_col,
                    cell_count: 1,
                    mergeable_after: false,
                    text: String::new(),
                    fg: source_run.fg,
                    bg: source_run.bg,
                });
            }
        } else {
            push_run(
                &mut projected.runs,
                projected_col,
                text,
                source_run.fg,
                source_run.bg,
            );
        }
    }

    let crop_start = u16::try_from(start).unwrap_or(u16::MAX);
    let crop_last = u16::try_from(end.saturating_sub(1)).unwrap_or(u16::MAX);
    projected.hyperlinks = line
        .hyperlinks
        .iter()
        .filter_map(|link| {
            let start_col = link.start_col.max(crop_start);
            let end_col = link.end_col.min(crop_last);
            (start_col <= end_col).then(|| TerminalHyperlink {
                start_col: start_col.saturating_sub(crop_start),
                end_col: end_col.saturating_sub(crop_start),
                uri: link.uri.clone(),
            })
        })
        .collect();
    projected
}

/// Incremental terminal projection produced from Ghostty render state changes.
///
/// This is intentionally introduced before changing the renderer. The current
/// frame extraction remains the compatibility path while callers can migrate to
/// applying row-level updates without coupling UI state to Ghostty internals.
#[cfg(test)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalDelta {
    pub changed_rows: Vec<TerminalRowDelta>,
    pub line_count_changed: bool,
    pub cursor_changed: bool,
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalRowDelta {
    pub row: u16,
    pub line: TerminalLine,
}

#[cfg(test)]
impl TerminalDelta {
    pub fn from_full_frame(frame: &TerminalFrame, previous: Option<&TerminalFrame>) -> Self {
        let changed_rows = match previous {
            Some(previous) => frame
                .lines
                .iter()
                .enumerate()
                .filter_map(|(row, line)| {
                    (previous.lines.get(row) != Some(line)).then_some(TerminalRowDelta {
                        row: row as u16,
                        line: line.clone(),
                    })
                })
                .collect(),
            None => frame
                .lines
                .iter()
                .cloned()
                .enumerate()
                .map(|(row, line)| TerminalRowDelta {
                    row: row as u16,
                    line,
                })
                .collect(),
        };

        Self {
            changed_rows,
            line_count_changed: previous
                .is_none_or(|previous| previous.lines.len() != frame.lines.len()),
            cursor_changed: previous.is_none_or(|previous| previous.cursor != frame.cursor),
        }
    }
}

/// Scrollbar geometry probe, kept only as a vendored-ABI regression observable for the
/// scroll-projection tests; it is no longer part of `TerminalFrame` (the per-frame FFI
/// probe died with the deleted local-scrollback chain, audit B07).
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TerminalScrollbar {
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalHyperlink {
    pub start_col: u16,
    pub end_col: u16,
    pub uri: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TerminalLine {
    pub runs: Vec<TerminalRun>,
    /// Terminal-cell text indexed by authoritative grid column.
    /// Wide-cell spacer columns are represented by an empty string.
    pub cells: Vec<String>,
    /// OSC 8 hyperlink spans in authoritative grid columns.
    pub hyperlinks: Vec<TerminalHyperlink>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalRun {
    /// Authoritative terminal-grid column where this styled run begins.
    pub start_col: u16,
    /// Number of terminal grid cells occupied by the run, including wide-cell spacers.
    pub cell_count: u16,
    /// Wide-cell spacer columns terminate text-layout coalescing so the next glyph is
    /// positioned from its authoritative grid column instead of a fallback-font advance.
    pub(crate) mergeable_after: bool,
    pub text: String,
    pub fg: u32,
    pub bg: Option<u32>,
}

pub const LOCAL_SCROLLBACK_LINES: u32 = 16_384;

pub(super) fn push_run(
    runs: &mut Vec<TerminalRun>,
    col: u16,
    text: String,
    fg: u32,
    bg: Option<u32>,
) {
    if let Some(last) = runs.last_mut() {
        let next_col = last.start_col.saturating_add(last.cell_count);
        if last.mergeable_after && last.fg == fg && last.bg == bg && next_col == col {
            last.text.push_str(&text);
            last.cell_count = last.cell_count.saturating_add(1);
            return;
        }
    }
    runs.push(TerminalRun {
        start_col: col,
        cell_count: 1,
        mergeable_after: true,
        text,
        fg,
        bg,
    });
}

pub(super) fn extend_run_span(runs: &mut [TerminalRun], col: u16) {
    let Some(last) = runs.last_mut() else {
        return;
    };
    if last.start_col.saturating_add(last.cell_count) == col {
        last.cell_count = last.cell_count.saturating_add(1);
        last.mergeable_after = false;
    }
}

pub(super) fn rgb_u32(r: u8, g: u8, b: u8) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

pub(super) fn terminal_bg(color: Option<u32>, default_bg: Option<u32>) -> Option<u32> {
    let color = color?;
    if default_bg == Some(color) {
        None
    } else {
        Some(color)
    }
}
