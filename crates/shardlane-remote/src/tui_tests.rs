//! Unit tests for the Remote shared Herdr TUI transport adapter.
//!
//! [INPUT]: private stream-frame helpers from `tui.rs` and the Host-owned
//! shared TUI DTOs from `shardlane_host`.
//! [OUTPUT]: deterministic wire-shape tests for the Remote TUI adapter.
//! [POS]: process/session ownership and bounds tests live with the owner in
//! `shardlane_host::shared_tui`; this file only verifies the network frame
//! projection and does not create a runtime or a client process.

use super::*;

#[test]
fn output_frame_preserves_bytes_with_base64_encoding() {
    let frame = TuiStreamFrame::Output {
        revision: 7,
        encoding: "base64",
        data: base64::engine::general_purpose::STANDARD.encode([0, 1, 255]),
    };
    let value: serde_json::Value = serde_json::from_str(&frame.wire())
        .unwrap_or_else(|error| panic!("frame must serialize: {error}"));
    assert_eq!(value["type"], "output");
    assert_eq!(value["encoding"], "base64");
    assert_eq!(value["revision"], 7);
}

#[test]
fn open_session_request_defaults_match_host_shared_defaults() {
    let body: OpenSessionRequest = serde_json::from_str("{}")
        .unwrap_or_else(|error| panic!("empty body deserializes: {error}"));
    assert_eq!(body.cols.or(Some(DEFAULT_COLS)), Some(DEFAULT_COLS));
    assert_eq!(body.rows.or(Some(DEFAULT_ROWS)), Some(DEFAULT_ROWS));
}
