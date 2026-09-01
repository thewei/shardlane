//! [INPUT]: the install implementation submodules for the provider bridges.
//! [OUTPUT]: aggregate re-exports; currently only `command_code` (Batch C).
//! Append Kiro/Gemini (Batch B) and DSH (Batch D) here once they land.
//! [POS]: the install asset directory for the provider-native bridges; each
//! submodule owns only its own Provider's managed install, while the shared
//! runtime protocol contract (the pane.report-agent family) is carried by
//! the Mod/plugin assets.

pub mod command_code;
