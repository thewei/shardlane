//! [INPUT]: Depends on the vendored libghostty-vt statically linked C ABI (the `ghostty_*` symbol set, including the key/mouse encoders) and std memory/synchronization primitives; the implementation is split across the ghostty/ submodules.
//! [OUTPUT]: Exposes `GhosttyRuntime`, `GhosttyTerminal`, `TerminalFrame{default_foreground,default_background,cursor_color,selection_color}`, `TerminalFramePlan` (exact per-frame row-level change plan, B16), `TerminalLine`, `TerminalRun`, `TerminalCursorStyle`, `TerminalKey`, `TerminalModifiers`, `COLOR_QUERY_*`/`color_query_report_bytes` (host terminal emulator OSC 10/11 dynamic color semantics), and the `encode_key`/`encode_mouse` encoding entry points; `is_alternate_screen`/`take_pending_bells` provide the low-level decisions for scroll-wheel translation and BEL feedback. (The scrollbar probe is test-only: frames carry no scrollbar since the local-scrollback chain was deleted, audit B07.)
//! [POS]: The low-level VT emulation semantics and frame projection authority for `crates/herdr-gui` (module root: mod declarations + re-exports; ABI in ghostty/ffi; lifetime/encode/selection/frame extraction/BEL as shards). It feeds faithful guess-free character/color data to `terminal_stream.rs` (transport and lifecycle) and `terminal_view.rs` (GPUI rendering); the terminal encoding authority for keys/modifiers also lives at this layer.

#[cfg(test)]
mod alt_screen_and_bell;
mod bell;
mod encode;
mod ffi;
mod frame;
mod input;
#[cfg(test)]
mod scrollback_projection;
mod selection;
mod terminal;
#[cfg(test)]
mod tests;
mod types;

use std::{
    ffi::c_void,
    ptr,
    sync::{Arc, Mutex},
};

// Private names shared across shards (resolved via submodule `use super::*`; visibility unchanged, reachable only within the ghostty tree)
use bell::*;
use ffi::*;
use types::*;

// Former public surface (visibility unchanged; formerly private items such as push_run are carried only by the private glob, not widened)
pub(crate) use ffi::GhosttyApi;
pub use ffi::GhosttyRuntime;
pub use input::{
    TerminalKey, TerminalModifiers, TerminalMouseAction, TerminalMouseButton, TerminalMouseGeometry,
};
pub use terminal::{GhosttyKeyEncoderState, GhosttyTerminal};
#[cfg(test)]
pub use types::TerminalDelta;
pub use types::{
    color_query_report_bytes, TerminalCursorStyle, TerminalFrame, TerminalFramePlan,
    TerminalGridSelection, TerminalHyperlink, TerminalLine, TerminalRun, COLOR_QUERY_BACKGROUND,
    COLOR_QUERY_FOREGROUND, LOCAL_SCROLLBACK_LINES,
};
