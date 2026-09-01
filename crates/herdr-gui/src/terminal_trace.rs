//! Terminal interaction latency diagnostics: uses a unified monotonic clock and packet
//! sequence numbers to tie together GPUI input → PTY →
//! Herdr TUI echo → VT drain → frame projection → TerminalPane render.
//!
//! [INPUT]: The `SHARDLANE_TERMINAL_TRACE=1` environment switch and the shardlane-host bounded async lag logger
//! [OUTPUT]: `enabled` / `now_us` / `next_packet_id` / `event`; disabled by default and never logs on the hot path
//! [POS]: Observability seam for the hosted Herdr TUI; records only lengths, phases, durations, and key classes — never user text content

use shardlane_host::diagnostics::lag_log;
use std::fmt::Arguments;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static ENABLED: OnceLock<bool> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();
static PACKET_SEQ: AtomicU64 = AtomicU64::new(1);
static READ_SEQ: AtomicU64 = AtomicU64::new(1);
static LAST_FRAME_INPUT_ID: AtomicU64 = AtomicU64::new(0);
static LAST_FRAME_READ_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("SHARDLANE_TERMINAL_TRACE").is_some())
}

pub(crate) fn now_us() -> u64 {
    let micros = START.get_or_init(Instant::now).elapsed().as_micros();
    u64::try_from(micros).unwrap_or(u64::MAX)
}

pub(crate) fn next_packet_id() -> u64 {
    if enabled() {
        PACKET_SEQ.fetch_add(1, Ordering::Relaxed)
    } else {
        0
    }
}

pub(crate) fn next_read_id() -> u64 {
    if enabled() {
        READ_SEQ.fetch_add(1, Ordering::Relaxed)
    } else {
        0
    }
}

pub(crate) fn elapsed_us(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}

pub(crate) fn set_last_frame_source(input_id: u64, read_id: u64) {
    if enabled() {
        LAST_FRAME_INPUT_ID.store(input_id, Ordering::Relaxed);
        LAST_FRAME_READ_ID.store(read_id, Ordering::Relaxed);
    }
}

pub(crate) fn last_frame_source() -> (u64, u64) {
    (
        LAST_FRAME_INPUT_ID.load(Ordering::Relaxed),
        LAST_FRAME_READ_ID.load(Ordering::Relaxed),
    )
}

pub(crate) fn event(args: Arguments<'_>) {
    if enabled() {
        lag_log(format_args!("terminal.trace t_us={} {args}", now_us()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monotonic_trace_clock_never_moves_backwards() {
        let first = now_us();
        let second = now_us();
        assert!(second >= first);
    }
}
