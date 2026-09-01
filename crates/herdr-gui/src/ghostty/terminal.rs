//! [INPUT]: Depends on the crate::ghostty module-root re-export surface (`use super::*`) and
//! std memory/synchronization primitives.
//! [OUTPUT]: Exposes (within the ghostty module tree) construction/scrollback caps, VT writes
//! (including 7-bit/C1 OSC-8, OSC 10-12 dynamic-color probes, BEL scan wiring,
//! `set_dynamic_colors` model dynamic defaults, and `take_pending_color_queries` consumption
//! of child color queries), alt-screen detection, BEL counting, viewport scroll/resize/Drop
//! (plus the test-only scrollbar probe), and the extraction-plan state (`last_frame_plan`)
//! behind `take_last_frame_plan`.
//! [POS]: The terminal slice of the ghostty module — GhosttyTerminal's owner (struct
//! definition + lifecycle); its method surface coexists with the encode/selection/frame
//! slices.

use super::*;

pub(super) const COLOR_DIRTY_FOREGROUND: u8 = 1 << 0;
pub(super) const COLOR_DIRTY_BACKGROUND: u8 = 1 << 1;
pub(super) const COLOR_DIRTY_CURSOR: u8 = 1 << 2;

/// OSC-8 control sequences in ordinary TUI output are small; keeping a bounded exact copy
/// lets repeated repaints compare the link semantic stream directly, avoiding per-cell
/// grid-ref FFI calls on every frame.
const OSC8_PAYLOAD_LIMIT: usize = 16 * 1024;
const OSC8_SEQUENCE_LIMIT: usize = 1024;
const OSC8_FINGERPRINT_OFFSET: u64 = 14_695_981_039_346_656_037;
const OSC8_FINGERPRINT_PRIME: u64 = 1_099_511_628_211;

#[derive(Default)]
struct OscColorProbe {
    state: u8,
    command: u16,
    digits: u8,
    /// Payload started with `?`: this sequence is a dynamic-color query, not a set.
    payload_query: bool,
}

impl OscColorProbe {
    /// Scan raw VT bytes. Returns `(color_dirty, color_queries)`: sets of the dynamic
    /// colors (OSC 10/11/12 and their resets) change the model's resolved colors;
    /// queries (`OSC 10;?` / `OSC 11;?`) only ask the emulator to report them. The
    /// state machine persists across `write` chunks so sequences split between reads
    /// are still recognized.
    fn scan(&mut self, bytes: &[u8]) -> (u8, u8) {
        let mut dirty = 0_u8;
        let mut queries = 0_u8;
        for &byte in bytes {
            match self.state {
                0 if byte == 0x1b => self.state = 1,
                0 => {}
                1 if byte == b']' => {
                    self.state = 2;
                    self.command = 0;
                    self.digits = 0;
                    self.payload_query = false;
                }
                1 if byte == 0x1b => {}
                1 => self.state = 0,
                2 if byte.is_ascii_digit() && self.digits < 3 => {
                    self.command = self
                        .command
                        .saturating_mul(10)
                        .saturating_add(u16::from(byte - b'0'));
                    self.digits += 1;
                }
                2 if byte == b';' => self.state = 3,
                2 => {
                    // No payload: the sequence ends at BEL or ST and still carries the
                    // dynamic-color command (e.g. `OSC 110` resets).
                    if byte == 0x07 {
                        dirty |= color_dirty_for_osc_command(self.command);
                        self.state = 0;
                    } else if byte == 0x1b {
                        dirty |= color_dirty_for_osc_command(self.command);
                        self.state = 1;
                    } else {
                        self.state = 0;
                    }
                }
                3 => {
                    // First payload byte decides set vs query.
                    self.payload_query = byte == b'?';
                    if byte == 0x07 {
                        self.finalize(&mut dirty, &mut queries);
                    } else if byte == 0x1b {
                        self.finalize(&mut dirty, &mut queries);
                        self.state = 1;
                    } else {
                        self.state = 4;
                    }
                }
                4 if byte == 0x07 => self.finalize(&mut dirty, &mut queries),
                4 if byte == 0x1b => self.state = 5,
                4 => {}
                5 => {
                    // ST terminator (`ESC \`) or any other byte: the payload ended.
                    self.finalize(&mut dirty, &mut queries);
                }
                _ => self.state = 0,
            }
        }
        (dirty, queries)
    }

    fn finalize(&mut self, dirty: &mut u8, queries: &mut u8) {
        if self.payload_query {
            match self.command {
                10 => *queries |= COLOR_QUERY_FOREGROUND,
                11 => *queries |= COLOR_QUERY_BACKGROUND,
                _ => {}
            }
        } else {
            *dirty |= color_dirty_for_osc_command(self.command);
        }
        self.state = 0;
        self.payload_query = false;
    }
}

fn color_dirty_for_osc_command(command: u16) -> u8 {
    match command {
        10 | 110 => COLOR_DIRTY_FOREGROUND,
        11 | 111 => COLOR_DIRTY_BACKGROUND,
        12 | 112 => COLOR_DIRTY_CURSOR,
        _ => 0,
    }
}

/// Independent key encoder state. `setopt_from_terminal` copies the terminal keyboard modes
/// into this object when VT state changes; input handling can then encode repeated named keys
/// without waiting on the much larger terminal/frame mutex.
pub struct GhosttyKeyEncoderState {
    pub(super) encoder: GhosttyKeyEncoderHandle,
    pub(super) event: GhosttyKeyEventHandle,
}

impl GhosttyKeyEncoderState {
    fn new() -> Result<Self, String> {
        let mut encoder = ptr::null_mut();
        let result = unsafe { ghostty_key_encoder_new(ptr::null(), &mut encoder) };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_key_encoder_new failed: {result}"));
        }
        let mut event = ptr::null_mut();
        let result = unsafe { ghostty_key_event_new(ptr::null(), &mut event) };
        if result != GHOSTTY_SUCCESS {
            unsafe { ghostty_key_encoder_free(encoder) };
            return Err(format!("ghostty_key_event_new failed: {result}"));
        }
        Ok(Self { encoder, event })
    }

    pub(super) fn sync_from_terminal(&mut self, terminal: GhosttyTerminalHandle) {
        unsafe { ghostty_key_encoder_setopt_from_terminal(self.encoder, terminal) };
    }
}

impl Drop for GhosttyKeyEncoderState {
    fn drop(&mut self) {
        unsafe {
            ghostty_key_event_free(self.event);
            ghostty_key_encoder_free(self.encoder);
        }
    }
}

unsafe impl Send for GhosttyKeyEncoderState {}

pub struct GhosttyTerminal {
    pub(super) api: Arc<GhosttyApi>,
    pub(super) terminal: GhosttyTerminalHandle,
    pub(super) render_state: GhosttyRenderStateHandle,
    pub(super) row_iterator: GhosttyRowIteratorHandle,
    pub(super) row_cells: GhosttyRowCellsHandle,
    pub(super) mouse_encoder: GhosttyMouseEncoderHandle,
    pub(super) mouse_event: GhosttyMouseEventHandle,
    pub(super) key_encoder: Arc<Mutex<GhosttyKeyEncoderState>>,
    pub(super) hyperlinks_seen: bool,
    pub(super) osc8_probe_state: u8,
    /// OSC-8 sequences written since the last frame extraction. Bounded exact semantic-stream
    /// comparison means identical repaint control sequences no longer mark it dirty; URI or
    /// parameter changes still force a hyperlink refresh.
    pub(super) osc8_dirty: bool,
    osc8_payload: Vec<u8>,
    osc8_payload_overflow: bool,
    osc8_stream_overflow: bool,
    osc8_sequences_since_frame: Vec<Vec<u8>>,
    osc8_last_sequences: Option<Vec<Vec<u8>>>,
    /// Cross-chunk probe for OSC 10/11/12 (and 110/111/112 resets). It only marks which
    /// dynamic-color channel changed; color values are still read entirely from
    /// libghostty-vt's authoritative state.
    osc_color_probe: OscColorProbe,
    pub(super) color_dirty: u8,
    /// Unanswered OSC 10/11 color-query bits (`COLOR_QUERY_*`) sent by the child process;
    /// cleared on read.
    pending_color_queries: u8,
    pub(super) resolved_default_foreground: Option<u32>,
    pub(super) resolved_default_background: Option<u32>,
    pub(super) resolved_cursor_color: Option<u32>,
    /// BEL scan state (persists across write chunks): 0x07 inside an OSC/DCS/APC/PM/SOS
    /// string state is a terminator and does not ring; 0x07 in Ground/CSI states executes
    /// with VT500 semantics (rings).
    pub(super) bell_scan: VtBellScan,
    /// BELs observed in live input since the last take; history seed replay resets it via
    /// discard.
    pub(super) pending_bells: u64,
    /// Per-row, per-cell RAW bitfield signature from the last extraction (identity basis of
    /// the frame-level fast path). Invariant: the signature always describes the row content
    /// of "the previous frame extracted by this side"; any extraction path that bypasses
    /// frame_reusing (frame()) resets/rebuilds it.
    pub(super) row_signatures: Vec<Vec<u64>>,
    /// Row-level change plan of the most recent extraction (B16): what `take_last_frame_plan`
    /// hands to the presentation layer so it can skip deep grid comparisons. Reset at the
    /// start of every extraction and only promoted beyond `Unknown` on success.
    pub(super) last_frame_plan: TerminalFramePlan,
    /// Regression observability: rows reused wholesale by the signature fast path
    /// (test-build-only instrumentation).
    #[cfg(test)]
    pub(super) reused_rows: usize,
}

impl Drop for GhosttyTerminal {
    fn drop(&mut self) {
        unsafe {
            ghostty_mouse_event_free(self.mouse_event);
            ghostty_mouse_encoder_free(self.mouse_encoder);
            (self.api.row_cells_free)(self.row_cells);
            (self.api.row_iterator_free)(self.row_iterator);
            (self.api.render_state_free)(self.render_state);
            (self.api.terminal_free)(self.terminal);
        }
    }
}

unsafe impl Send for GhosttyTerminal {}

impl GhosttyTerminal {
    #[cfg(test)]
    pub(crate) fn new(api: Arc<GhosttyApi>, cols: u16, rows: u16) -> Result<Self, String> {
        Self::new_with_scrollback(api, cols, rows, LOCAL_SCROLLBACK_LINES as usize)
    }

    pub(crate) fn new_with_scrollback(
        api: Arc<GhosttyApi>,
        cols: u16,
        rows: u16,
        max_scrollback: usize,
    ) -> Result<Self, String> {
        let mut terminal = ptr::null_mut();
        let result = unsafe {
            (api.terminal_new)(
                ptr::null(),
                &mut terminal,
                GhosttyTerminalOptions {
                    cols,
                    rows,
                    max_scrollback: max_scrollback.max(rows as usize),
                },
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty_terminal_new failed: {result}"));
        }

        let mut render_state = ptr::null_mut();
        let result = unsafe { (api.render_state_new)(ptr::null(), &mut render_state) };
        if result != GHOSTTY_SUCCESS {
            unsafe { (api.terminal_free)(terminal) };
            return Err(format!("ghostty_render_state_new failed: {result}"));
        }

        let mut row_iterator = ptr::null_mut();
        let result = unsafe { (api.row_iterator_new)(ptr::null(), &mut row_iterator) };
        if result != GHOSTTY_SUCCESS {
            unsafe {
                (api.render_state_free)(render_state);
                (api.terminal_free)(terminal);
            }
            return Err(format!(
                "ghostty_render_state_row_iterator_new failed: {result}"
            ));
        }

        let mut row_cells = ptr::null_mut();
        let result = unsafe { (api.row_cells_new)(ptr::null(), &mut row_cells) };
        if result != GHOSTTY_SUCCESS {
            unsafe {
                (api.row_iterator_free)(row_iterator);
                (api.render_state_free)(render_state);
                (api.terminal_free)(terminal);
            }
            return Err(format!(
                "ghostty_render_state_row_cells_new failed: {result}"
            ));
        }

        let mut mouse_encoder = ptr::null_mut();
        let result = unsafe { ghostty_mouse_encoder_new(ptr::null(), &mut mouse_encoder) };
        if result != GHOSTTY_SUCCESS {
            unsafe {
                (api.row_cells_free)(row_cells);
                (api.row_iterator_free)(row_iterator);
                (api.render_state_free)(render_state);
                (api.terminal_free)(terminal);
            }
            return Err(format!("ghostty_mouse_encoder_new failed: {result}"));
        }

        let mut mouse_event = ptr::null_mut();
        let result = unsafe { ghostty_mouse_event_new(ptr::null(), &mut mouse_event) };
        if result != GHOSTTY_SUCCESS {
            unsafe {
                ghostty_mouse_encoder_free(mouse_encoder);
                (api.row_cells_free)(row_cells);
                (api.row_iterator_free)(row_iterator);
                (api.render_state_free)(render_state);
                (api.terminal_free)(terminal);
            }
            return Err(format!("ghostty_mouse_event_new failed: {result}"));
        }

        let mut key_encoder = match GhosttyKeyEncoderState::new() {
            Ok(encoder) => encoder,
            Err(error) => {
                unsafe {
                    ghostty_mouse_event_free(mouse_event);
                    ghostty_mouse_encoder_free(mouse_encoder);
                    (api.row_cells_free)(row_cells);
                    (api.row_iterator_free)(row_iterator);
                    (api.render_state_free)(render_state);
                    (api.terminal_free)(terminal);
                }
                return Err(error);
            }
        };
        key_encoder.sync_from_terminal(terminal);
        let key_encoder = Arc::new(Mutex::new(key_encoder));

        Ok(Self {
            api,
            terminal,
            render_state,
            row_iterator,
            row_cells,
            mouse_encoder,
            mouse_event,
            key_encoder,
            hyperlinks_seen: false,
            osc8_probe_state: 0,
            osc8_dirty: true,
            osc8_payload: Vec::new(),
            osc8_payload_overflow: false,
            osc8_stream_overflow: false,
            osc8_sequences_since_frame: Vec::new(),
            osc8_last_sequences: None,
            osc_color_probe: OscColorProbe::default(),
            color_dirty: COLOR_DIRTY_FOREGROUND | COLOR_DIRTY_BACKGROUND | COLOR_DIRTY_CURSOR,
            pending_color_queries: 0,
            resolved_default_foreground: None,
            resolved_default_background: None,
            resolved_cursor_color: None,
            bell_scan: VtBellScan::default(),
            pending_bells: 0,
            row_signatures: Vec::new(),
            last_frame_plan: TerminalFramePlan::Unknown,
            #[cfg(test)]
            reused_rows: 0,
        })
    }

    fn observe_osc8_marker(&mut self, bytes: &[u8]) {
        // Recognize OSC 8's 7-bit ESC/BEL/ST and C1 OSC/ST framing byte by byte, saving the
        // params+URI payload (never logged). Parse state persists across write chunks and
        // stays accurate even when a single PTY read is split apart.
        for &byte in bytes {
            match self.osc8_probe_state {
                0 if byte == 0x1b => self.osc8_probe_state = 1,
                0 if byte == 0x9d => self.osc8_probe_state = 6,
                0 => {}
                1 if byte == b']' => self.osc8_probe_state = 2,
                1 if byte == 0x1b => self.osc8_probe_state = 1,
                1 => self.osc8_probe_state = 0,
                2 if byte == b'8' => self.osc8_probe_state = 3,
                2 if byte == 0x1b => self.osc8_probe_state = 1,
                2 => self.osc8_probe_state = 0,
                3 if byte == b';' => {
                    self.hyperlinks_seen = true;
                    self.osc8_payload.clear();
                    self.osc8_payload_overflow = false;
                    self.osc8_probe_state = 4;
                }
                3 if byte == 0x1b => self.osc8_probe_state = 1,
                3 => self.osc8_probe_state = 0,
                4 if byte == 0x07 => self.finish_osc8_sequence(),
                4 if byte == 0x9c => self.finish_osc8_sequence(),
                4 if byte == 0x1b => self.osc8_probe_state = 5,
                4 => self.push_osc8_payload_byte(byte),
                5 if byte == b'\\' => self.finish_osc8_sequence(),
                5 if byte == 0x1b => {
                    // The previous ESC belongs to the payload; keep the newest ESC and wait
                    // for later bytes to confirm the ST prefix.
                    self.push_osc8_payload_byte(0x1b);
                    self.osc8_probe_state = 5;
                }
                5 => {
                    // An ESC that is not part of ST belongs to the payload; unless the
                    // current byte is a BEL terminator, keep scanning the current byte as
                    // payload.
                    self.push_osc8_payload_byte(0x1b);
                    if byte == 0x07 {
                        self.finish_osc8_sequence();
                    } else {
                        self.push_osc8_payload_byte(byte);
                        self.osc8_probe_state = 4;
                    }
                }
                6 if byte == b'8' => self.osc8_probe_state = 3,
                6 if byte == 0x9d => self.osc8_probe_state = 6,
                6 if byte == 0x1b => self.osc8_probe_state = 1,
                6 => self.osc8_probe_state = 0,
                _ => self.osc8_probe_state = 0,
            }
        }
        self.update_osc8_dirty();
    }

    fn push_osc8_payload_byte(&mut self, byte: u8) {
        if self.osc8_payload.len() < OSC8_PAYLOAD_LIMIT {
            self.osc8_payload.push(byte);
        } else {
            self.osc8_payload_overflow = true;
        }
    }

    fn finish_osc8_sequence(&mut self) {
        if self.osc8_payload_overflow {
            // An oversized payload cannot be compared safely; conservatively force full
            // extraction until the frame baseline resets.
            self.osc8_stream_overflow = true;
        } else if self.osc8_sequences_since_frame.len() >= OSC8_SEQUENCE_LIMIT {
            self.osc8_stream_overflow = true;
        } else {
            // An OSC-8 close (payload `;`) must also enter the semantic stream: it changes
            // the hyperlink state of subsequent cells, so old spans cannot be reused even
            // when the repainted text/RAW cells are identical.
            self.osc8_sequences_since_frame
                .push(std::mem::take(&mut self.osc8_payload));
        }
        self.osc8_payload.clear();
        self.osc8_probe_state = 0;
    }

    fn update_osc8_dirty(&mut self) {
        self.osc8_dirty = self.osc8_probe_state >= 3
            || self.osc8_stream_overflow
            || (!self.osc8_sequences_since_frame.is_empty()
                && self.osc8_last_sequences.as_ref() != Some(&self.osc8_sequences_since_frame));
    }

    /// Commit the observed OSC-8 stream as the current frame baseline. Unfinished or
    /// oversized sequences stay dirty, keeping the conservative full-extraction path until
    /// the state finishes parsing.
    pub(super) fn finish_osc8_frame(&mut self) {
        if self.osc8_probe_state >= 3 {
            self.osc8_dirty = true;
            return;
        }
        if self.osc8_stream_overflow {
            self.osc8_last_sequences = None;
        } else if !self.osc8_sequences_since_frame.is_empty() {
            if crate::terminal_trace::enabled() {
                let (fingerprint, bytes) =
                    Self::osc8_sequences_fingerprint(&self.osc8_sequences_since_frame);
                crate::terminal_trace::event(format_args!(
                    "stage=ghostty.osc8 stream_count={} stream_bytes={} stream_fingerprint={fingerprint:016x} baseline_equal={}",
                    self.osc8_sequences_since_frame.len(),
                    bytes,
                    self.osc8_last_sequences
                        .as_ref()
                        .is_some_and(|baseline| baseline == &self.osc8_sequences_since_frame),
                ));
            }
            self.osc8_last_sequences = Some(std::mem::take(&mut self.osc8_sequences_since_frame));
        }
        self.osc8_payload.clear();
        self.osc8_payload_overflow = false;
        self.osc8_stream_overflow = false;
        self.osc8_dirty = false;
    }

    fn osc8_sequences_fingerprint(sequences: &[Vec<u8>]) -> (u64, usize) {
        let mut fingerprint = OSC8_FINGERPRINT_OFFSET;
        let mut bytes = 0_usize;
        for sequence in sequences {
            for &byte in sequence {
                fingerprint ^= u64::from(byte);
                fingerprint = fingerprint.wrapping_mul(OSC8_FINGERPRINT_PRIME);
                bytes = bytes.saturating_add(1);
            }
            fingerprint ^= 0xff;
            fingerprint = fingerprint.wrapping_mul(OSC8_FINGERPRINT_PRIME);
        }
        (fingerprint, bytes)
    }

    pub fn key_encoder_handle(&self) -> Arc<Mutex<GhosttyKeyEncoderState>> {
        self.key_encoder.clone()
    }

    pub fn write(&mut self, bytes: &[u8]) {
        self.observe_osc8_marker(bytes);
        let (color_dirty, color_queries) = self.osc_color_probe.scan(bytes);
        self.color_dirty |= color_dirty;
        self.pending_color_queries |= color_queries;
        self.pending_bells += self.bell_scan.scan(bytes);
        unsafe {
            (self.api.terminal_vt_write)(self.terminal, bytes.as_ptr(), bytes.len());
        }
        if let Ok(mut encoder) = self.key_encoder.lock() {
            encoder.sync_from_terminal(self.terminal);
        }
    }

    /// Seed the model's dynamic default foreground/background (OSC 10/11). Shardlane
    /// hosts the terminal emulator for the Herdr TUI child, so the emulator-side
    /// dynamic colors are ours to maintain; values derive from the active Herdr theme.
    pub fn set_dynamic_colors(&mut self, foreground: u32, background: u32) {
        let bytes = dynamic_color_set_bytes(foreground, background);
        self.write(&bytes);
    }

    /// Consume the OSC 10/11 query bits observed since the last take; read-and-clear.
    pub fn take_pending_color_queries(&mut self) -> u8 {
        std::mem::take(&mut self.pending_color_queries)
    }

    /// Whether the terminal is on the alternate screen (less/vim/man-style TUIs): true if
    /// any of the three DECSET entries is set; a failed query conservatively returns false.
    pub fn is_alternate_screen(&mut self) -> bool {
        [
            GHOSTTY_MODE_ALT_SCREEN,
            GHOSTTY_MODE_ALT_SCREEN_NO_CURSOR_CLEAR,
            GHOSTTY_MODE_ALT_SCREEN_LEGACY,
        ]
        .iter()
        .any(|&mode| {
            let mut enabled = false;
            let result = unsafe { ghostty_terminal_mode_get(self.terminal, mode, &mut enabled) };
            result == GHOSTTY_SUCCESS && enabled
        })
    }

    /// Number of BELs (0x07) observed in the live VT stream since the last call; cleared on
    /// read.
    pub fn take_pending_bells(&mut self) -> u64 {
        std::mem::take(&mut self.pending_bells)
    }

    /// Vendored-ABI regression contract (scroll-projection tests only).
    #[cfg(test)]
    pub fn scroll_to_row(&mut self, row: u64) -> Result<(), String> {
        // The vendored lib predates the absolute-row viewport tag, but exposes the same
        // authoritative scrollbar offset. Convert the desired absolute row into the stable
        // delta operation so dragging round-trips correctly across the locked ABI.
        let current = self.scrollbar()?.offset;
        let delta = i128::from(row) - i128::from(current);
        let delta = delta.clamp(isize::MIN as i128, isize::MAX as i128) as isize;
        self.scroll(delta);
        Ok(())
    }

    /// Viewport scroll primitive. Test-only regression entry point: production scroll
    /// paths (the local-scrollback chain) were deleted with the TUI-only convergence
    /// (audit B02); the vendored ABI binding stays pinned for the scroll tests.
    #[cfg(test)]
    pub fn scroll(&mut self, rows: isize) {
        unsafe {
            (self.api.terminal_scroll_viewport)(
                self.terminal,
                GhosttyScrollViewport {
                    tag: GHOSTTY_SCROLL_VIEWPORT_DELTA,
                    value: GhosttyScrollViewportValue {
                        delta: rows,
                        padding: 0,
                    },
                },
            );
        }
    }

    /// Scrollbar geometry probe. Test-only regression observable (scroll-projection
    /// contract); production frames no longer carry a scrollbar (audit B07 — the only
    /// consumer was the deleted local-scrollback chain).
    #[cfg(test)]
    pub fn scrollbar(&self) -> Result<TerminalScrollbar, String> {
        let mut scrollbar = GhosttyTerminalScrollbar::default();
        let result = unsafe {
            (self.api.terminal_get)(
                self.terminal,
                GHOSTTY_TERMINAL_DATA_SCROLLBAR,
                (&mut scrollbar as *mut GhosttyTerminalScrollbar).cast(),
            )
        };
        if result != GHOSTTY_SUCCESS {
            return Err(format!("ghostty terminal scrollbar failed: {result}"));
        }
        Ok(TerminalScrollbar {
            total: scrollbar.total,
            offset: scrollbar.offset,
            len: scrollbar.len,
        })
    }

    pub fn resize(
        &mut self,
        cols: u16,
        rows: u16,
        pixel_width: u16,
        pixel_height: u16,
    ) -> Result<(), String> {
        let result = unsafe {
            (self.api.terminal_resize)(
                self.terminal,
                cols,
                rows,
                pixel_width.max(1).into(),
                pixel_height.max(1).into(),
            )
        };
        if result == GHOSTTY_SUCCESS {
            Ok(())
        } else {
            Err(format!("ghostty_terminal_resize failed: {result}"))
        }
    }
}
