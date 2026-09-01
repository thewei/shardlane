//! [INPUT]: Depends on the crate::ghostty module-root re-export surface (`use super::*`) and
//! std memory/synchronization primitives.
//! [OUTPUT]: Exposes (within the ghostty module tree) grid-ref/hyperlink hit testing, viewport
//! coordinate conversion, word/line/range selection, and selection text export.
//! [POS]: The selection slice of the ghostty module — GhosttyTerminal's selection, consumed by
//! terminal interaction (word/line/drag selection).

use super::*;

impl GhosttyTerminal {
    fn grid_ref_at(&self, point: (u16, u16)) -> Result<GhosttyGridRef, String> {
        let mut grid_ref = GhosttyGridRef::default();
        let result = unsafe {
            ghostty_terminal_grid_ref(
                self.terminal,
                GhosttyPoint::viewport(point.0, point.1),
                &mut grid_ref,
            )
        };
        if result == GHOSTTY_SUCCESS {
            Ok(grid_ref)
        } else {
            Err(format!("ghostty_terminal_grid_ref failed: {result}"))
        }
    }

    pub(super) fn hyperlink_uri_at(&self, point: (u16, u16)) -> Result<Option<String>, String> {
        let grid_ref = self.grid_ref_at(point)?;
        let mut required = 0_usize;
        let result =
            unsafe { ghostty_grid_ref_hyperlink_uri(&grid_ref, ptr::null_mut(), 0, &mut required) };
        if result == GHOSTTY_SUCCESS && required == 0 {
            return Ok(None);
        }
        if result != GHOSTTY_OUT_OF_SPACE && result != GHOSTTY_SUCCESS {
            return Err(format!(
                "ghostty_grid_ref_hyperlink_uri size failed: {result}"
            ));
        }
        if required == 0 {
            return Ok(None);
        }
        let mut output = vec![0_u8; required];
        let result = unsafe {
            ghostty_grid_ref_hyperlink_uri(
                &grid_ref,
                output.as_mut_ptr(),
                output.len(),
                &mut required,
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_grid_ref_hyperlink_uri failed: {result}"));
        }
        output.truncate(required);
        let uri = String::from_utf8(output)
            .map_err(|err| format!("ghostty hyperlink URI returned invalid UTF-8: {err}"))?;
        Ok((!uri.is_empty()).then_some(uri))
    }

    fn viewport_point_from_grid_ref(
        &self,
        grid_ref: &GhosttyGridRef,
    ) -> Result<Option<(u16, u16)>, String> {
        let mut point = GhosttyPointCoordinate { x: 0, y: 0 };
        let result = unsafe {
            ghostty_terminal_point_from_grid_ref(
                self.terminal,
                grid_ref,
                GHOSTTY_POINT_TAG_VIEWPORT,
                &mut point,
            )
        };
        match result {
            GHOSTTY_SUCCESS => Ok(Some((point.x, point.y.min(u32::from(u16::MAX)) as u16))),
            GHOSTTY_NO_VALUE => Ok(None),
            other => Err(format!(
                "ghostty_terminal_point_from_grid_ref failed: {other}"
            )),
        }
    }

    fn viewport_selection(
        &self,
        selection: &GhosttySelection,
    ) -> Result<Option<TerminalGridSelection>, String> {
        let Some(start) = self.viewport_point_from_grid_ref(&selection.start)? else {
            return Ok(None);
        };
        let Some(end) = self.viewport_point_from_grid_ref(&selection.end)? else {
            return Ok(None);
        };
        Ok(Some((start, end)))
    }

    pub fn select_word_at(
        &self,
        point: (u16, u16),
    ) -> Result<Option<TerminalGridSelection>, String> {
        let grid_ref = self.grid_ref_at(point)?;
        let options = GhosttyTerminalSelectWordOptions::defaults(grid_ref);
        let mut selection = GhosttySelection {
            size: std::mem::size_of::<GhosttySelection>(),
            start: GhosttyGridRef::default(),
            end: GhosttyGridRef::default(),
            rectangle: false,
        };
        let result =
            unsafe { ghostty_terminal_select_word(self.terminal, &options, &mut selection) };
        match result {
            GHOSTTY_SUCCESS => self.viewport_selection(&selection),
            GHOSTTY_NO_VALUE => Ok(None),
            other => Err(format!("ghostty_terminal_select_word failed: {other}")),
        }
    }

    fn select_word_between_refs(
        &self,
        start: GhosttyGridRef,
        end: GhosttyGridRef,
    ) -> Result<Option<TerminalGridSelection>, String> {
        let options = GhosttyTerminalSelectWordBetweenOptions::defaults(start, end);
        let mut selection = GhosttySelection {
            size: std::mem::size_of::<GhosttySelection>(),
            start: GhosttyGridRef::default(),
            end: GhosttyGridRef::default(),
            rectangle: false,
        };
        let result = unsafe {
            ghostty_terminal_select_word_between(self.terminal, &options, &mut selection)
        };
        match result {
            GHOSTTY_SUCCESS => self.viewport_selection(&selection),
            GHOSTTY_NO_VALUE => Ok(None),
            other => Err(format!(
                "ghostty_terminal_select_word_between failed: {other}"
            )),
        }
    }

    pub fn select_word_drag(
        &self,
        anchor: (u16, u16),
        current: (u16, u16),
    ) -> Result<Option<TerminalGridSelection>, String> {
        let anchor_ref = self.grid_ref_at(anchor)?;
        let current_ref = self.grid_ref_at(current)?;
        let from_anchor = self.select_word_between_refs(anchor_ref, current_ref)?;
        let from_current = self.select_word_between_refs(current_ref, anchor_ref)?;
        let Some(first) = from_anchor.or(from_current) else {
            return Ok(None);
        };
        let second = from_current.unwrap_or(first);
        let mut points = [first.0, first.1, second.0, second.1];
        points.sort_by_key(|point| (point.1, point.0));
        Ok(Some((points[0], points[3])))
    }

    pub fn select_line_drag(
        &self,
        anchor: (u16, u16),
        current: (u16, u16),
    ) -> Result<Option<TerminalGridSelection>, String> {
        let first = self.select_line_at(anchor)?;
        let second = self.select_line_at(current)?;
        let Some(first) = first.or(second) else {
            return Ok(None);
        };
        let second = second.unwrap_or(first);
        let mut points = [first.0, first.1, second.0, second.1];
        points.sort_by_key(|point| (point.1, point.0));
        Ok(Some((points[0], points[3])))
    }

    pub fn select_line_at(
        &self,
        point: (u16, u16),
    ) -> Result<Option<TerminalGridSelection>, String> {
        let grid_ref = self.grid_ref_at(point)?;
        let options = GhosttyTerminalSelectLineOptions::defaults(grid_ref);
        let mut selection = GhosttySelection {
            size: std::mem::size_of::<GhosttySelection>(),
            start: GhosttyGridRef::default(),
            end: GhosttyGridRef::default(),
            rectangle: false,
        };
        let result =
            unsafe { ghostty_terminal_select_line(self.terminal, &options, &mut selection) };
        match result {
            GHOSTTY_SUCCESS => self.viewport_selection(&selection),
            GHOSTTY_NO_VALUE => Ok(None),
            other => Err(format!("ghostty_terminal_select_line failed: {other}")),
        }
    }

    pub fn selection_text(&mut self, start: (u16, u16), end: (u16, u16)) -> Result<String, String> {
        let (start, end) = if start.1 < end.1 || (start.1 == end.1 && start.0 <= end.0) {
            (start, end)
        } else {
            (end, start)
        };
        let selection = GhosttySelection {
            size: std::mem::size_of::<GhosttySelection>(),
            start: self.grid_ref_at(start)?,
            end: self.grid_ref_at(end)?,
            rectangle: false,
        };
        let options = GhosttyFormatterTerminalOptions::plain_selection(&selection);
        let mut formatter = ptr::null_mut();
        let result = unsafe {
            ghostty_formatter_terminal_new(ptr::null(), &mut formatter, self.terminal, options)
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_formatter_terminal_new failed: {result}"));
        }

        let formatted = (|| {
            let mut required = 0_usize;
            let result = unsafe {
                ghostty_formatter_format_buf(formatter, ptr::null_mut(), 0, &mut required)
            };
            if result != GHOSTTY_OUT_OF_SPACE && result != GHOSTTY_SUCCESS {
                return Err(format!(
                    "ghostty_formatter_format_buf size failed: {result}"
                ));
            }
            if required == 0 {
                return Ok(String::new());
            }
            let mut output = vec![0_u8; required];
            let result = unsafe {
                ghostty_formatter_format_buf(
                    formatter,
                    output.as_mut_ptr(),
                    output.len(),
                    &mut required,
                )
            };
            if result != GHOSTTY_SUCCESS {
                return Err(format!("ghostty_formatter_format_buf failed: {result}"));
            }
            output.truncate(required);
            String::from_utf8(output)
                .map_err(|err| format!("ghostty formatter returned invalid UTF-8: {err}"))
        })();
        unsafe { ghostty_formatter_free(formatter) };
        formatted
    }
}
