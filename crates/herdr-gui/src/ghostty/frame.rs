//! [INPUT]: Depends on the crate::ghostty module-root re-export surface (`use super::*`) and
//! std memory/synchronization primitives.
//! [OUTPUT]: Exposes (within the ghostty module tree) render-state current colors,
//! frame/frame_reusing line/cursor projection (including the RAW bitfield signature and the
//! OSC-8 semantic-stream fast path, surfaced to callers as the TerminalFramePlan row-level
//! plan via `take_last_frame_plan`), render state, and cell-level reads.
//! [POS]: The frame slice of the ghostty module — GhosttyTerminal's frame extraction, consumed
//! by terminal_view's per-frame rendering.

use super::terminal::{COLOR_DIRTY_BACKGROUND, COLOR_DIRTY_CURSOR, COLOR_DIRTY_FOREGROUND};
use super::*;

enum SignatureFramePlan {
    Reuse(TerminalFrame),
    Partial(Vec<bool>),
    Full,
}

impl GhosttyTerminal {
    pub(super) fn default_fg(&self) -> Result<Option<u32>, String> {
        let mut color = GhosttyColorRgb::default();
        let result = unsafe {
            (self.api.terminal_get)(
                self.terminal,
                GHOSTTY_TERMINAL_DATA_COLOR_FOREGROUND_DEFAULT,
                (&mut color as *mut GhosttyColorRgb).cast(),
            )
        };
        if result == GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_terminal_get default fg failed: {result}"));
        }
        Ok(Some(rgb_u32(color.r, color.g, color.b)))
    }

    pub(super) fn default_bg(&self) -> Result<Option<u32>, String> {
        let mut color = GhosttyColorRgb::default();
        let result = unsafe {
            (self.api.terminal_get)(
                self.terminal,
                GHOSTTY_TERMINAL_DATA_COLOR_BACKGROUND_DEFAULT,
                (&mut color as *mut GhosttyColorRgb).cast(),
            )
        };
        if result == GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_terminal_get default bg failed: {result}"));
        }
        Ok(Some(rgb_u32(color.r, color.g, color.b)))
    }

    fn render_colors(&self) -> Result<GhosttyRenderStateColors, String> {
        let mut colors = GhosttyRenderStateColors::default();
        let result =
            unsafe { (self.api.render_state_colors_get)(self.render_state, &mut colors as *mut _) };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_render_state_colors_get failed: {result}"));
        }
        Ok(colors)
    }

    /// Resolve Ghostty's incremental color snapshot into persistent presentation state. The
    /// render-state ABI zeroes unchanged color fields on later updates, so a channel is accepted
    /// only on first initialization or when the VT stream actually carried OSC 10/11/12 (or the
    /// matching reset command). This preserves a previously resolved color across ordinary frame
    /// reuse while still allowing black (`#000000`) to be a legitimate *new* color.
    fn resolved_colors(&mut self) -> Result<(u32, Option<u32>, u32), String> {
        let foreground_dirty = self.resolved_default_foreground.is_none()
            || self.color_dirty & COLOR_DIRTY_FOREGROUND != 0;
        let background_dirty = self.resolved_default_background.is_none()
            || self.color_dirty & COLOR_DIRTY_BACKGROUND != 0;
        let cursor_dirty =
            self.resolved_cursor_color.is_none() || self.color_dirty & COLOR_DIRTY_CURSOR != 0;

        if foreground_dirty || background_dirty || cursor_dirty {
            let colors = self.render_colors()?;
            if foreground_dirty {
                let render_foreground = rgb_u32(
                    colors.foreground.r,
                    colors.foreground.g,
                    colors.foreground.b,
                );
                self.resolved_default_foreground = self.default_fg()?.or(Some(render_foreground));
            }
            if background_dirty {
                let render_background = rgb_u32(
                    colors.background.r,
                    colors.background.g,
                    colors.background.b,
                );
                self.resolved_default_background = self.default_bg()?.or(Some(render_background));
            }
            if cursor_dirty {
                self.resolved_cursor_color = colors
                    .cursor_has_value
                    .then(|| rgb_u32(colors.cursor.r, colors.cursor.g, colors.cursor.b))
                    .or(self.resolved_default_foreground);
            }
        }

        self.color_dirty = 0;
        let foreground = self.resolved_default_foreground.ok_or_else(|| {
            "ghostty resolved foreground unavailable after render-state update".to_string()
        })?;
        let background = self.resolved_default_background;
        let cursor = self.resolved_cursor_color.unwrap_or(foreground);
        Ok((foreground, background, cursor))
    }

    pub fn frame(&mut self) -> Result<TerminalFrame, String> {
        self.frame_reusing(None)
    }

    /// Row-level change plan of the most recent successful extraction (B16): `RowsUnchanged`
    /// or `RowsChanged` lets the caller skip deep grid comparisons (exact — RAW bitfields are
    /// the cell's complete storage); `Unknown` mandates the full deep path. Takes the value;
    /// the next extraction starts from a fresh `Unknown`.
    pub fn take_last_frame_plan(&mut self) -> TerminalFramePlan {
        std::mem::take(&mut self.last_frame_plan)
    }

    /// Extract the current frame. Two reuse tiers:
    /// - Frame-level signature fast path: when the OSC-8 semantic stream is unchanged and
    ///   every cell's RAW bitfield matches the previous extraction exactly, clone the previous
    ///   frame's row projections wholesale and refresh only scalars such as cursor/blink/
    ///   scrollbar;
    /// - Row-level hyperlink reuse: when the fast path misses, rows whose (cells, runs) are
    ///   exactly equal reuse their hyperlink spans as-is, skipping per-cell link FFI probing.
    ///
    /// Link state can only change via OSC-8 sequences (VT semantics), and the RAW bitfield is
    /// the cell's complete storage (character/style/color/link bits), so both reuse tiers are
    /// exact rather than heuristic caching.
    pub fn frame_reusing(
        &mut self,
        previous: Option<&TerminalFrame>,
    ) -> Result<TerminalFrame, String> {
        let trace = crate::terminal_trace::enabled();
        let total_started = trace.then(std::time::Instant::now);
        let mut signature_us = 0_u64;
        let mut changed_rows = None;
        let mut path = "extract";
        // Conservative default: every extraction starts with no row knowledge; only the
        // signature-proven paths below promote the plan.
        self.last_frame_plan = TerminalFramePlan::Unknown;
        if self.osc8_dirty {
            // The link table may have changed: the signature is invalid, so re-extract fully
            // (this pass does not rebuild the signature; it is recollected on the next
            // non-dirty extraction, preserving the signature ⟷ previous invariant).
            self.row_signatures.clear();
        } else if let Some(previous) = previous {
            let signature_started = trace.then(std::time::Instant::now);
            match self.signature_frame_plan(previous)? {
                SignatureFramePlan::Reuse(frame) => {
                    self.last_frame_plan = TerminalFramePlan::RowsUnchanged;
                    if trace {
                        signature_us = signature_started
                            .map(crate::terminal_trace::elapsed_us)
                            .unwrap_or(0);
                        crate::terminal_trace::event(format_args!(
                            "stage=ghostty.frame path=reuse signature_us={signature_us} extract_us=0 changed_rows=0 osc8_dirty={} hyperlinks_seen={} signature_rows={} total_us={}",
                            self.osc8_dirty,
                            self.hyperlinks_seen,
                            self.row_signatures.len(),
                            total_started
                                .map(crate::terminal_trace::elapsed_us)
                                .unwrap_or(0),
                        ));
                    }
                    return Ok(frame);
                }
                SignatureFramePlan::Partial(rows) => {
                    path = "partial";
                    changed_rows = Some(rows);
                }
                SignatureFramePlan::Full => {}
            }
            signature_us = signature_started
                .map(crate::terminal_trace::elapsed_us)
                .unwrap_or(0);
        } else {
            // bootstrap (frame(): the authoritative frame after initial extraction, scroll,
            // or resize): collect signatures so the next frame_reusing can hit the fast path.
            self.refresh_row_signatures();
        }
        let changed_count = changed_rows
            .as_ref()
            .map(|rows| rows.iter().filter(|changed| **changed).count())
            .unwrap_or(0);
        let extract_started = trace.then(std::time::Instant::now);
        let frame = self.extract_frame(previous, changed_rows.as_deref())?;
        // Promote the extraction plan for the presentation layer: the partial path proved
        // exactly which rows differ from the previous extraction; every other path stays on
        // the conservative Unknown set at the top of this function.
        if let Some(rows) = changed_rows {
            self.last_frame_plan = TerminalFramePlan::RowsChanged(
                rows.iter()
                    .enumerate()
                    .filter_map(|(row, changed)| changed.then_some(row))
                    .collect(),
            );
        }
        // An OSC-8 semantic change deliberately clears the cached signatures before the
        // conservative full extract. Re-seed them immediately from the same post-FFI model so
        // the next ordinary repaint does not pay a second full viewport extraction just to
        // rebuild the cache. The signature pass is the cheap RAW-only path; hyperlink spans have
        // already been freshly probed by `extract_frame`.
        if self.row_signatures.len() != frame.lines.len() {
            let signature_refresh_started = trace.then(std::time::Instant::now);
            self.refresh_row_signatures();
            if trace {
                crate::terminal_trace::event(format_args!(
                    "stage=ghostty.frame.signature_refresh rows={} elapsed_us={}",
                    self.row_signatures.len(),
                    signature_refresh_started
                        .map(crate::terminal_trace::elapsed_us)
                        .unwrap_or(0),
                ));
            }
        }
        if trace {
            crate::terminal_trace::event(format_args!(
                "stage=ghostty.frame path={path} signature_us={signature_us} extract_us={} changed_rows={changed_count} osc8_dirty={} hyperlinks_seen={} signature_rows={} total_us={}",
                extract_started
                    .map(crate::terminal_trace::elapsed_us)
                    .unwrap_or(0),
                self.osc8_dirty,
                self.hyperlinks_seen,
                self.row_signatures.len(),
                total_started
                    .map(crate::terminal_trace::elapsed_us)
                    .unwrap_or(0),
            ));
        }
        Ok(frame)
    }

    /// render_state → terminal refresh + row iterator (shared by the extraction and
    /// signature paths). Idempotent: repeated calls only refresh the snapshot and iterator,
    /// never mutating the terminal itself.
    fn begin_row_iteration(&mut self) -> Result<(), String> {
        let result = unsafe { (self.api.render_state_update)(self.render_state, self.terminal) };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_render_state_update failed: {result}"));
        }
        let result = unsafe {
            (self.api.render_state_get)(
                self.render_state,
                RENDER_STATE_DATA_ROW_ITERATOR,
                (&mut self.row_iterator as *mut GhosttyRowIteratorHandle).cast(),
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!(
                "ghostty_render_state_get row iterator failed: {result}"
            ));
        }
        Ok(())
    }

    /// Read the RAW bitfield signature of every cell in the frame in a single pass. Returns
    /// None when any read is unsupported by the vendored ABI (safely degrading to full
    /// extraction) without producing diagnostic noise.
    fn read_row_signatures_safely(&mut self) -> Option<Vec<Vec<u64>>> {
        self.begin_row_iteration().ok()?;
        self.read_row_signatures_from_current_iterator()
    }

    fn read_row_signatures_from_current_iterator(&mut self) -> Option<Vec<Vec<u64>>> {
        let mut signatures = Vec::new();
        while unsafe { (self.api.row_iterator_next)(self.row_iterator) } {
            let result = unsafe {
                (self.api.row_get)(
                    self.row_iterator,
                    RENDER_STATE_ROW_DATA_CELLS,
                    (&mut self.row_cells as *mut GhosttyRowCellsHandle).cast(),
                )
            };
            if result != GHOSTTY_SUCCESS {
                return None;
            }
            let mut row = Vec::new();
            while unsafe { (self.api.row_cells_next)(self.row_cells) } {
                let mut raw: GhosttyCell = 0;
                let result = unsafe {
                    (self.api.row_cells_get)(
                        self.row_cells,
                        ROW_CELLS_DATA_RAW,
                        (&mut raw as *mut GhosttyCell).cast(),
                    )
                };
                if result != GHOSTTY_SUCCESS {
                    return None;
                }
                row.push(raw);
            }
            signatures.push(row);
        }
        Some(signatures)
    }

    fn refresh_row_signatures(&mut self) {
        if let Some(signatures) = self.read_row_signatures_safely() {
            self.row_signatures = signatures;
        } else {
            self.row_signatures.clear();
        }
    }

    /// Build an exact extraction plan from per-cell RAW signatures. When only a subset of rows
    /// changed, the second render-state pass can reuse the previous `TerminalLine` for every
    /// unchanged row and call the expensive text/color FFI only for changed rows. Global default
    /// color changes and shape changes stay on the conservative full-extract path.
    fn signature_frame_plan(
        &mut self,
        previous: &TerminalFrame,
    ) -> Result<SignatureFramePlan, String> {
        // `render_state_colors_get` must be read before the row iterator is exhausted. The full
        // extraction path already follows update → colors → rows; keep the signature path
        // identical so OSC 10/11 default-color changes are not lost.
        self.begin_row_iteration()?;
        let (default_foreground, default_background, cursor_color) = self.resolved_colors()?;
        let Some(fresh) = self.read_row_signatures_from_current_iterator() else {
            self.row_signatures.clear();
            return Ok(SignatureFramePlan::Full);
        };
        let shape_matches =
            fresh.len() == previous.lines.len() && fresh.len() == self.row_signatures.len();
        let colors_match = previous.default_foreground == Some(default_foreground)
            && previous.default_background == default_background;
        if !shape_matches || !colors_match {
            if crate::terminal_trace::enabled() {
                crate::terminal_trace::event(format_args!(
                    "stage=ghostty.frame.plan path=full reason={} shape_matches={} colors_match={} fresh_rows={} previous_rows={} signature_rows={}",
                    if !shape_matches { "shape" } else { "colors" },
                    shape_matches,
                    colors_match,
                    fresh.len(),
                    previous.lines.len(),
                    self.row_signatures.len(),
                ));
            }
            self.row_signatures = fresh;
            return Ok(SignatureFramePlan::Full);
        }

        let changed_rows = fresh
            .iter()
            .zip(self.row_signatures.iter())
            .map(|(current, stored)| current != stored)
            .collect::<Vec<_>>();
        self.row_signatures = fresh;
        if changed_rows.iter().any(|changed| *changed) {
            if crate::terminal_trace::enabled() {
                crate::terminal_trace::event(format_args!(
                    "stage=ghostty.frame.plan path=partial changed_rows={}",
                    changed_rows.iter().filter(|changed| **changed).count(),
                ));
            }
            return Ok(SignatureFramePlan::Partial(changed_rows));
        }

        #[cfg(test)]
        {
            self.reused_rows += previous.lines.len();
        }
        let mut frame = previous.clone();
        frame.default_foreground = Some(default_foreground);
        frame.default_background = default_background;
        frame.cursor = self.cursor()?;
        frame.cursor_style = self.cursor_style()?;
        frame.cursor_blinking = self.render_bool(RENDER_STATE_DATA_CURSOR_BLINKING)?;
        frame.cursor_color = Some(cursor_color);
        frame.selection_color = frame
            .surface_background
            .or(default_background)
            .map(|background| default_selection_color(default_foreground, background));
        self.finish_osc8_frame();
        Ok(SignatureFramePlan::Reuse(frame))
    }

    fn extract_frame(
        &mut self,
        previous: Option<&TerminalFrame>,
        changed_rows: Option<&[bool]>,
    ) -> Result<TerminalFrame, String> {
        self.begin_row_iteration()?;
        let cursor = self.cursor()?;
        let cursor_style = self.cursor_style()?;
        let cursor_blinking = self.render_bool(RENDER_STATE_DATA_CURSOR_BLINKING)?;
        let (default_fg, default_bg, cursor_color) = self.resolved_colors()?;
        let reuse_links = self.hyperlinks_seen && !self.osc8_dirty;
        let mut lines = Vec::new();
        let mut y = 0_u16;
        while unsafe { (self.api.row_iterator_next)(self.row_iterator) } {
            let row_index = usize::from(y);
            let row_unchanged = changed_rows
                .and_then(|rows| rows.get(row_index))
                .is_some_and(|changed| !*changed);
            if row_unchanged {
                if let Some(previous_row) = previous.and_then(|frame| frame.lines.get(row_index)) {
                    lines.push(previous_row.clone());
                    #[cfg(test)]
                    {
                        self.reused_rows += 1;
                    }
                    y = y.saturating_add(1);
                    continue;
                }
            }
            let result = unsafe {
                (self.api.row_get)(
                    self.row_iterator,
                    RENDER_STATE_ROW_DATA_CELLS,
                    (&mut self.row_cells as *mut GhosttyRowCellsHandle).cast(),
                )
            };
            if result != GHOSTTY_SUCCESS {
                return Err(format!(
                    "ghostty_render_state_row_get cells failed: {result}"
                ));
            }
            let mut line = TerminalLine::default();
            let mut x = 0_u16;
            while unsafe { (self.api.row_cells_next)(self.row_cells) } {
                let text = self.cell_text()?;
                let col = x;
                line.cells.push(text.clone());
                x = x.saturating_add(1);
                if text.is_empty() {
                    extend_run_span(&mut line.runs, col);
                    continue;
                }
                let fg = self
                    .cell_color(ROW_CELLS_DATA_FG_COLOR)?
                    .unwrap_or(default_fg);
                let bg = terminal_bg(self.cell_color(ROW_CELLS_DATA_BG_COLOR)?, default_bg);
                push_run(&mut line.runs, col, text, fg, bg);
            }
            if self.hyperlinks_seen {
                let previous_row = previous.and_then(|frame| frame.lines.get(y as usize));
                let reusable = reuse_links
                    && previous_row
                        .is_some_and(|row| row.cells == line.cells && row.runs == line.runs);
                if reusable {
                    if let Some(row) = previous_row {
                        line.hyperlinks = row.hyperlinks.clone();
                    }
                } else {
                    // Rebuild spans cell by cell; empty cells participate too, so links
                    // spanning whitespace stay intact — byte-for-byte identical to the
                    // pre-reuse extraction behavior.
                    for col in 0..line.cells.len() as u16 {
                        if let Some(uri) = self.hyperlink_uri_at((col, y))? {
                            match line.hyperlinks.last_mut() {
                                Some(link)
                                    if link.uri == uri && link.end_col.saturating_add(1) == col =>
                                {
                                    link.end_col = col;
                                }
                                _ => line.hyperlinks.push(TerminalHyperlink {
                                    start_col: col,
                                    end_col: col,
                                    uri,
                                }),
                            }
                        }
                    }
                }
            }
            lines.push(line);
            y = y.saturating_add(1);
        }
        self.finish_osc8_frame();
        let surface_background = dominant_surface_background(&lines)
            .or_else(|| previous.and_then(|frame| frame.surface_background));
        Ok(TerminalFrame {
            lines,
            default_foreground: Some(default_fg),
            default_background: default_bg,
            surface_background,
            cursor,
            cursor_style,
            cursor_blinking,
            cursor_color: Some(cursor_color),
            selection_color: surface_background
                .or(default_bg)
                .map(|background| default_selection_color(default_fg, background)),
        })
    }

    fn cursor_style(&self) -> Result<TerminalCursorStyle, String> {
        match self.render_i32(RENDER_STATE_DATA_CURSOR_VISUAL_STYLE)? {
            0 => Ok(TerminalCursorStyle::Bar),
            1 => Ok(TerminalCursorStyle::Block),
            2 => Ok(TerminalCursorStyle::Underline),
            3 => Ok(TerminalCursorStyle::HollowBlock),
            value => Err(format!("unknown ghostty cursor visual style: {value}")),
        }
    }

    fn cursor(&self) -> Result<Option<(u16, u16)>, String> {
        if !self.render_bool(RENDER_STATE_DATA_CURSOR_VISIBLE)?
            || !self.render_bool(RENDER_STATE_DATA_CURSOR_VIEWPORT_HAS_VALUE)?
        {
            return Ok(None);
        }
        Ok(Some((
            self.render_u16(RENDER_STATE_DATA_CURSOR_VIEWPORT_X)?,
            self.render_u16(RENDER_STATE_DATA_CURSOR_VIEWPORT_Y)?,
        )))
    }

    fn render_bool(&self, data: u32) -> Result<bool, String> {
        let mut out = false;
        let result = unsafe {
            (self.api.render_state_get)(self.render_state, data, (&mut out as *mut bool).cast())
        };
        if result == GHOSTTY_SUCCESS {
            Ok(out)
        } else {
            Err(format!("ghostty render bool failed: {result}"))
        }
    }

    fn render_i32(&self, data: u32) -> Result<i32, String> {
        let mut out = 0_i32;
        let result = unsafe {
            (self.api.render_state_get)(self.render_state, data, (&mut out as *mut i32).cast())
        };
        if result == GHOSTTY_SUCCESS {
            Ok(out)
        } else {
            Err(format!("ghostty render i32 failed: {result}"))
        }
    }

    fn render_u16(&self, data: u32) -> Result<u16, String> {
        let mut out = 0_u16;
        let result = unsafe {
            (self.api.render_state_get)(self.render_state, data, (&mut out as *mut u16).cast())
        };
        if result == GHOSTTY_SUCCESS {
            Ok(out)
        } else {
            Err(format!("ghostty render u16 failed: {result}"))
        }
    }

    fn cell_text(&self) -> Result<String, String> {
        let mut len = 0_u32;
        let result = unsafe {
            (self.api.row_cells_get)(
                self.row_cells,
                ROW_CELLS_DATA_GRAPHEMES_LEN,
                (&mut len as *mut u32).cast(),
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty row cell grapheme len failed: {result}"));
        }
        if len > 0 {
            let mut codepoints = vec![0_u32; len as usize];
            let result = unsafe {
                (self.api.row_cells_get)(
                    self.row_cells,
                    ROW_CELLS_DATA_GRAPHEMES_BUF,
                    codepoints.as_mut_ptr().cast(),
                )
            };
            if result != GHOSTTY_SUCCESS {
                return Err(format!("ghostty row cell grapheme buffer failed: {result}"));
            }
            return Ok(codepoints
                .into_iter()
                .map(|codepoint| char::from_u32(codepoint).unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect());
        }

        // A zero grapheme length means either a normal empty cell or a wide-character spacer.
        // Query the raw-cell width tag only for this case so normal text avoids extra FFI calls.
        let mut raw = GhosttyCell::default();
        let result = unsafe {
            (self.api.row_cells_get)(
                self.row_cells,
                ROW_CELLS_DATA_RAW,
                (&mut raw as *mut GhosttyCell).cast(),
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty row cell raw failed: {result}"));
        }
        let mut wide = 0_u32;
        let result =
            unsafe { (self.api.cell_get)(raw, CELL_DATA_WIDE, (&mut wide as *mut u32).cast()) };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty cell wide failed: {result}"));
        }
        if wide == CELL_WIDE_SPACER_TAIL || wide == CELL_WIDE_SPACER_HEAD {
            Ok(String::new())
        } else {
            Ok(" ".to_string())
        }
    }

    fn cell_color(&self, data: u32) -> Result<Option<u32>, String> {
        let mut color = GhosttyColorRgb::default();
        let result = unsafe {
            (self.api.row_cells_get)(
                self.row_cells,
                data,
                (&mut color as *mut GhosttyColorRgb).cast(),
            )
        };
        match result {
            GHOSTTY_SUCCESS => Ok(Some(rgb_u32(color.r, color.g, color.b))),
            GHOSTTY_INVALID_VALUE => Ok(None),
            other => Err(format!("ghostty row cell color failed: {other}")),
        }
    }
}
