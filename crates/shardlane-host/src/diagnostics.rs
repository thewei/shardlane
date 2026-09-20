//! Diagnostic log: a single per-process writer appends to
//! `/tmp/shardlane-lag.log` (via a drop-friendly sync_channel), rotating to
//! `.1` when the file passes its size cap so daily runs can never grow it
//! without bound. `SHARDLANE_LAG_LOG_PATH` selects a separate file for
//! isolated native smoke runs.
//!
//! [INPUT]: depends on std only (no GPUI/external crates)
//! [OUTPUT]: exposes pub lag_log (in-process sequence/timestamp/pid prefix;
//! shardlane-remote diagnostics go through the same channel) and pub op_log;
//! both rotate at their size caps (lag 10 MiB, op 512 KiB, one `.1`
//! generation, main file always holds the newest lines);
//! `SHARDLANE_LAG_LOG_PATH` / `SHARDLANE_UI_LOG_PATH` select the log files
//! for isolated smokes
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

/// 诊断日志单文件上限（2026-09-20）：超过即轮转到 `.1`（覆盖上一代），
/// 磁盘占用上界为 2×上限，主文件始终承载最新日志。
const LAG_LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;

fn lag_log_path() -> PathBuf {
    LAG_LOG_PATH
        .get_or_init(|| {
            std::env::var_os("SHARDLANE_LAG_LOG_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/tmp/shardlane-lag.log"))
        })
        .clone()
}

/// Append-only 日志写入器：内存跟踪已写字节数，超过 `max_bytes` 即把当前
/// 文件 rename 到 `<path>.1`（同卷原子替换上一代）并重新打开。历史遗留的
/// 超大文件无需人工干预——首次写入即触发轮转自愈。打开失败时静默丢行，
/// 与既有行为一致。
struct RotatingAppender {
    path: PathBuf,
    max_bytes: u64,
    file: Option<std::fs::File>,
    written: u64,
}

impl RotatingAppender {
    fn open(path: PathBuf, max_bytes: u64) -> Self {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok();
        let written = file
            .as_ref()
            .and_then(|file| file.metadata().ok())
            .map(|meta| meta.len())
            .unwrap_or(0);
        Self {
            path,
            max_bytes,
            file,
            written,
        }
    }

    fn write_line(&mut self, line: &str) {
        if self.written >= self.max_bytes {
            self.rotate();
        }
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if writeln!(file, "{line}").is_ok() {
            self.written += u64::try_from(line.len()).unwrap_or(u64::MAX) + 1;
        }
    }

    fn rotate(&mut self) {
        self.file = None; // drop 关闭当前句柄,再原子改名
        let mut rotated = self.path.clone().into_os_string();
        rotated.push(".1");
        let _ = std::fs::rename(&self.path, rotated);
        self.file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok();
        self.written = 0;
    }
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
        let (sender, receiver) = sync_channel::<String>(2_048);
        std::thread::spawn(move || {
            let mut writer = RotatingAppender::open(lag_log_path(), LAG_LOG_MAX_BYTES);
            for line in receiver {
                writer.write_line(&line);
            }
        });
        sender
    });
    let _ = sender.try_send(line);
}

static OP_SEQ: AtomicU64 = AtomicU64::new(0);
static OP_LOGGER: OnceLock<SyncSender<String>> = OnceLock::new();
static OP_LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// 操作日志单文件上限：首次打开超限即轮转到 .1（保留一代）。
const OP_LOG_MAX_BYTES: u64 = 512 * 1024;

fn op_log_path() -> PathBuf {
    OP_LOG_PATH
        .get_or_init(|| {
            std::env::var_os("SHARDLANE_UI_LOG_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::var_os("HOME")
                        .map(|home| {
                            PathBuf::from(home)
                                .join(".shardlane")
                                .join("logs")
                                .join("ui.log")
                        })
                        .unwrap_or_else(|| PathBuf::from("/tmp/shardlane-ui.log"))
                })
        })
        .clone()
}

/// 用户操作与关键节点日志（2026-09-19）：Chat 入口判定、钩子事件、
/// journal 写入、钩子安装动作等，供远程协作方读取分析。级别标记
/// INFO/WARN/ERROR；后台线程落盘，调用方零阻塞。
pub fn op_log(level: &str, args: std::fmt::Arguments<'_>) {
    let seq = OP_SEQ.fetch_add(1, Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0);
    let pid = std::process::id();
    let line = format!("[{ts:.3} pid={pid} #{seq}] {level}: {args}");
    let sender = OP_LOGGER.get_or_init(|| {
        let (sender, receiver) = sync_channel::<String>(2_048);
        std::thread::spawn(move || {
            // 与 lag_log 同一写入器:轮转检查在写入循环内,单次长运行同样不会超限。
            let mut writer = RotatingAppender::open(op_log_path(), OP_LOG_MAX_BYTES);
            for line in receiver {
                writer.write_line(&line);
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    struct TempLogDir(PathBuf);

    impl TempLogDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "shardlane-diagnostics-{}-{tag}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> PathBuf {
            self.0.join("lag.log")
        }
    }

    impl Drop for TempLogDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn rotating_appender_keeps_latest_lines_and_single_generation() {
        let dir = TempLogDir::new("rotate");
        let mut writer = RotatingAppender::open(dir.path(), 100);
        for i in 0..20 {
            writer.write_line(&format!("line-{i:02} ################"));
        }

        // 主文件承载最新日志,且不超上限(允许单行溢出)。
        let latest = std::fs::read_to_string(dir.path()).unwrap();
        assert!(
            std::fs::metadata(dir.path()).unwrap().len() <= 100 + 32,
            "main file must stay near the cap"
        );
        assert!(latest.contains("line-19"));
        assert!(!latest.contains("line-00"));

        // 恰好保留一代 .1,内含更早的内容;永不产生 .2/.3。
        let rotated = std::fs::read_to_string(dir.0.join("lag.log.1")).unwrap();
        // 单代轮转下 .1 是倒数第二批(12..15),更早的行已被覆盖丢弃——
        // 这正是"只保留最新"的设计行为。
        assert!(rotated.contains("line-12"));
        assert!(rotated.contains("line-14"));
        assert!(!rotated.contains("line-16"));
        assert!(latest.contains("line-16"));
        assert!(!dir.0.join("lag.log.2").exists());
    }

    #[test]
    fn rotating_appender_heals_oversized_leftover_on_first_write() {
        let dir = TempLogDir::new("heal");
        // 模拟旧版无限追加遗留的超大文件。
        std::fs::write(dir.path(), "x".repeat(300)).unwrap();

        let mut writer = RotatingAppender::open(dir.path(), 100);
        writer.write_line("fresh");

        // 首写即把遗留文件挪到 .1,主文件从空开始只含新行。
        let rotated = std::fs::read_to_string(dir.0.join("lag.log.1")).unwrap();
        assert_eq!(rotated, "x".repeat(300));
        assert_eq!(std::fs::read_to_string(dir.path()).unwrap(), "fresh\n");
    }
}
