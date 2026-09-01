//! [INPUT]: Depends on the crate::ghostty module-root re-export surface (`use super::*`) and
//! std memory/synchronization primitives.
//! [OUTPUT]: Exposes (within the ghostty module tree) VtBellPhase/VtBellScan (state survives
//! across write chunks), with xterm-aligned semantics.
//! [POS]: The bell scanner of the ghostty module — consumed by terminal.rs's write wiring;
//! alt_screen_and_bell tests lock the semantics.

/// Minimal VT control-sequence state machine that only answers one question: does this byte
/// constitute a BEL ring? Semantics align with xterm: a C0 BEL executes in Ground/Esc/CSI
/// states (rings); in the OSC string state a BEL is a terminator (no ring); DCS/SOS/PM/APC
/// string states ignore BEL and end only on ST (ESC \).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum VtBellPhase {
    #[default]
    Ground,
    Esc,
    Csi,
    Osc,
    /// DCS/SOS/PM/APC string state.
    Str,
    /// ESC inside a string state: wait for `\\` to confirm ST, otherwise treat as the first
    /// byte of a new sequence.
    StrEsc,
}

/// BEL scanner whose state persists across write chunks; the caller (GhosttyTerminal) owns
/// the state and the counters.
#[derive(Clone, Debug, Default)]
pub(super) struct VtBellScan {
    phase: VtBellPhase,
}

impl VtBellScan {
    /// Scan a span of bytes and return how many of the BELs in it ring.
    pub(super) fn scan(&mut self, bytes: &[u8]) -> u64 {
        let mut bells = 0_u64;
        for &byte in bytes {
            self.phase = match (self.phase, byte) {
                (VtBellPhase::Ground, 0x1b) => VtBellPhase::Esc,
                (VtBellPhase::Ground, 0x07) => {
                    bells += 1;
                    VtBellPhase::Ground
                }
                (VtBellPhase::Ground, _) => VtBellPhase::Ground,
                (VtBellPhase::Esc, b'[') => VtBellPhase::Csi,
                (VtBellPhase::Esc, b']') => VtBellPhase::Osc,
                (VtBellPhase::Esc, b'P' | b'X' | b'^' | b'_') => VtBellPhase::Str,
                (VtBellPhase::Esc, 0x1b) => VtBellPhase::Esc,
                (VtBellPhase::Esc, 0x07) => {
                    bells += 1;
                    VtBellPhase::Ground
                }
                (VtBellPhase::Esc, _) => VtBellPhase::Ground,
                (VtBellPhase::Csi, 0x40..=0x7e) => VtBellPhase::Ground,
                (VtBellPhase::Csi, 0x1b) => VtBellPhase::Esc,
                (VtBellPhase::Csi, 0x07) => {
                    bells += 1;
                    VtBellPhase::Csi
                }
                (VtBellPhase::Csi, _) => VtBellPhase::Csi,
                (VtBellPhase::Osc, 0x07) => VtBellPhase::Ground,
                (VtBellPhase::Osc, 0x1b) => VtBellPhase::StrEsc,
                (VtBellPhase::Osc, _) => VtBellPhase::Osc,
                (VtBellPhase::Str, 0x1b) => VtBellPhase::StrEsc,
                (VtBellPhase::Str, _) => VtBellPhase::Str,
                (VtBellPhase::StrEsc, b'\\') => VtBellPhase::Ground,
                (VtBellPhase::StrEsc, b'[') => VtBellPhase::Csi,
                (VtBellPhase::StrEsc, b']') => VtBellPhase::Osc,
                (VtBellPhase::StrEsc, b'P' | b'X' | b'^' | b'_') => VtBellPhase::Str,
                (VtBellPhase::StrEsc, 0x1b) => VtBellPhase::StrEsc,
                (VtBellPhase::StrEsc, _) => VtBellPhase::Ground,
            };
        }
        bells
    }
}
