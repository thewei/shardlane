//! Diagnostic log: a single per-process writer appends to
//! `/tmp/shardlane-lag.log` (via a drop-friendly sync_channel).
//! `SHARDLANE_LAG_LOG_PATH` selects a separate file for isolated native
//! smoke runs.
//!
//! [INPUT]: depends on std only (no GPUI/external crates)
//! [OUTPUT]: exposes pub lag_log (in-process sequence/timestamp/pid prefix;
//! shardlane-remote diagnostics go through the same channel);
//!           `SHARDLANE_LAG_LOG_PATH` selects the log file for isolated smokes
//! [POS]: the observability layer of shardlane-host, consumed by herdr.rs
//! (RPC/event path timing); remote API counting goes through this channel too

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static LAG_SEQ: AtomicU64 = AtomicU64::new(0);
static LAG_LOGGER: OnceLock<SyncSender<String>> = OnceLock::new();
static LAG_LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

fn lag_log_path() -> PathBuf {
    LAG_LOG_PATH
        .get_or_init(|| {
            std::env::var_os("SHARDLANE_LAG_LOG_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/tmp/shardlane-lag.log"))
        })
        .clone()
}

pub fn lag_log(args: std::fmt::Arguments<'_>) {
    let seq = LAG_SEQ.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0);
    let pid = std::process::id();
    let line = format!("[{ts:.3} pid={pid} #{seq}] {args}");
    let sender = LAG_LOGGER.get_or_init(|| {
        let (sender, receiver) = sync_channel(2_048);
        std::thread::spawn(move || {
            let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(lag_log_path())
            else {
                return;
            };
            for line in receiver {
                let _ = writeln!(file, "{line}");
            }
        });
        sender
    });
    let _ = sender.try_send(line);
}

/// Opt-in verbose RPC diagnostics (same `SHARDLANE_TERMINAL_TRACE` gate as the
/// shared TUI transport). Off by default because full RPC params carry user
/// prompt text, terminal text, and argv, which must never land in the
/// world-readable lag log unasked.
pub fn rpc_params_trace_enabled() -> bool {
    static PARAMS_TRACE_ENABLED: OnceLock<bool> = OnceLock::new();
    *PARAMS_TRACE_ENABLED.get_or_init(|| std::env::var_os("SHARDLANE_TERMINAL_TRACE").is_some())
}
