// SPDX-License-Identifier: MIT
// Upstream MIT-derived watch strategy; parsing/catalog ownership stays outside this module.

//! FS wake signal for a single session file (audit CHAT-A04 residual item):
//! watch the target file's parent directory (non-recursive), filter events to
//! the target file name, and coalesce through a bounded(1) channel — the same
//! proven pattern as the HistoryWatcher in `watcher.rs`. Live consumers wait
//! in a select with "FS signal first, bounded timer backstop"; the same
//! signal is never enqueued twice, and event storms naturally coalesce into
//! a single wake.

use async_channel::{Receiver, Sender};
use notify::{RecursiveMode, Watcher};
use std::path::Path;

/// Dirty-signal source for one exact session file. When watcher setup fails
/// (permissions/platform limits), `start` returns None and consumers degrade
/// to the pure timer backstop.
pub struct FileWake {
    _watcher: notify::RecommendedWatcher,
    rx: Receiver<()>,
}

impl FileWake {
    pub fn start(file_path: &str) -> Option<Self> {
        let path = Path::new(file_path);
        let parent = path.parent()?;
        let file_name = path.file_name()?.to_os_string();
        let (tx, rx) = async_channel::bounded(1);
        let tx_signal: Sender<()> = tx;
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else { return };
                let touched = event
                    .paths
                    .iter()
                    .any(|changed| changed.file_name() == Some(file_name.as_os_str()));
                if touched {
                    let _ = tx_signal.try_send(());
                }
            })
            .ok()?;
        watcher.watch(parent, RecursiveMode::NonRecursive).ok()?;
        Some(Self {
            _watcher: watcher,
            rx,
        })
    }

    /// Receiving end of the dirty signal; `try_recv` drains after a sync
    /// cycle (burst catch-up).
    pub fn receiver(&self) -> Receiver<()> {
        self.rx.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    fn recv_within(rx: &Receiver<()>, millis: u64) -> bool {
        let deadline = Instant::now() + Duration::from_millis(millis);
        while Instant::now() < deadline {
            if rx.try_recv().is_ok() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn file_wake_signals_only_watched_file_and_coalesces() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let session = temp.path().join("session.jsonl");
        let sibling = temp.path().join("other.jsonl");
        let wake = FileWake::start(session.to_str().unwrap_or_else(|| panic!("utf8 path")))
            .unwrap_or_else(|| panic!("watcher unavailable"));
        let rx = wake.receiver();
        // Absorb any startup noise from watcher setup (macOS FSEvents may
        // replay recent directory events) so later assertions start from a
        // quiet state.
        let _ = recv_within(&rx, 150);

        // A write to the target file wakes.
        fs::write(&session, "first\n").unwrap_or_else(|error| panic!("{error}"));
        assert!(recv_within(&rx, 2_000), "target append must wake");
        // Quiet drain: wait until all duplicate/replayed events for the
        // target file are exhausted.
        while recv_within(&rx, 400) {}

        // Unrelated files do not wake.
        fs::write(&sibling, "noise").unwrap_or_else(|error| panic!("{error}"));
        assert!(
            !recv_within(&rx, 300),
            "unrelated sibling writes must not wake"
        );

        // Signal-storm coalescing: many consecutive writes produce at most a
        // tiny number of pending signals.
        for round in 0..8 {
            fs::write(&session, format!("burst {round}\n"))
                .unwrap_or_else(|error| panic!("{error}"));
        }
        assert!(recv_within(&rx, 2_000), "burst must produce a signal");
        // bounded(1) + try_send drops: quiet after draining all late-arriving
        // FS batch events.
        while recv_within(&rx, 500) {}
        assert!(
            !recv_within(&rx, 500),
            "storm must coalesce, not queue per-write"
        );
    }
}
