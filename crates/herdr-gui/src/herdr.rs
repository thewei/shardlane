//! Compatibility shim: the Herdr integration layer moved into the shardlane-host crate
//! (the remote API reuses the same runtime boundary).
//!
//! [INPUT]: Depends on all public items of shardlane_host::herdr
//! [OUTPUT]: Re-exports every public symbol of the former herdr module (zero GUI changes)
//! [POS]: Historical path stabilizer for herdr-gui; new code should use shardlane_host::herdr directly

pub use shardlane_host::herdr::*;
