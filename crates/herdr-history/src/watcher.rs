// SPDX-License-Identifier: MIT
// Upstream MIT-derived watch-root strategy; parsing/catalog ownership stays outside this module.
//! [INPUT]: Adapter watch paths supplied by the roster.
//! [OUTPUT]: A merged, rate-limited source dirty signal; no file parsing, no
//! catalog writes.
//! [POS]: File-event boundary of the history core; lifecycle managed by the
//! roster generation held by the GUI.

use crate::adapters::AgentHistoryAdapter;
use async_channel::{Receiver, Sender};
use notify::{RecursiveMode, Watcher};
use std::path::PathBuf;

pub struct HistoryWatcher {
    _watcher: notify::RecommendedWatcher,
    dirty_rx: Receiver<()>,
}

impl HistoryWatcher {
    pub fn start(adapters: &[Box<dyn AgentHistoryAdapter>]) -> Option<Self> {
        let roots = adapters
            .iter()
            .flat_map(|adapter| adapter.watch_paths())
            .collect::<Vec<_>>();
        Self::start_paths(roots)
    }

    fn start_paths(paths: Vec<PathBuf>) -> Option<Self> {
        if paths.is_empty() {
            return None;
        }
        let (dirty_tx, dirty_rx) = async_channel::bounded(1);
        let callback_dirty = dirty_tx.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_ok() {
                    signal_dirty(&callback_dirty);
                }
            })
            .ok()?;

        let mut watched = 0usize;
        for path in paths {
            if watcher.watch(&path, RecursiveMode::Recursive).is_ok() {
                watched += 1;
            }
        }
        if watched == 0 {
            return None;
        }
        Some(Self {
            _watcher: watcher,
            dirty_rx,
        })
    }

    pub fn dirty_receiver(&self) -> Receiver<()> {
        self.dirty_rx.clone()
    }
}

fn signal_dirty(sender: &Sender<()>) {
    let _ = sender.try_send(());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn watcher_coalesces_file_events_into_one_dirty_signal() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let watcher = HistoryWatcher::start_paths(vec![temp.path().to_path_buf()])
            .unwrap_or_else(|| panic!("watcher unavailable"));
        fs::write(temp.path().join("session.jsonl"), "first")
            .unwrap_or_else(|error| panic!("{error}"));
        fs::write(temp.path().join("session.jsonl"), "second")
            .unwrap_or_else(|error| panic!("{error}"));

        let receiver = watcher.dirty_receiver();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && receiver.try_recv().is_err() {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            Instant::now() < deadline,
            "watcher did not observe file update"
        );
        assert!(
            receiver.try_recv().is_err(),
            "dirty signal should be coalesced"
        );
    }
}
