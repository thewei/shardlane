//! Header git status snapshot: +N/−M change counts and branch info (same semantics as the header's script status block).
//! Purely read-only observation: reads directly via a git subprocess, never touches the work tree, and is decoupled from the Herdr runtime.
//!
//! [INPUT]: Depends on std::process::Command / std::time::Instant; zero app state coupling.
//! [OUTPUT]: Exposes GitStatusSnapshot, the is_fresh_for freshness check, and the
//! git_status_snapshot(path) collector; numstat/ahead-behind parsing is pure functions (testable).
//! [POS]: herdr-gui's read-only git observation layer, collected by main.rs's background refresh task and
//! consumed by header_view's changes pill and Info popover; sibling reference: new_agent.rs's branch snapshot.

use std::path::Path;
use std::process::Command;
use std::time::Instant;

/// Refresh threshold: the header may request this every frame; snapshots are reused
/// within 12s (zero git processes on the hot path).
pub(crate) const GIT_STATUS_MAX_AGE_SECS: u64 = 12;

#[derive(Clone, Debug)]
pub(crate) struct GitStatusSnapshot {
    pub(crate) path: String,
    pub(crate) branch: String,
    pub(crate) additions: u64,
    pub(crate) deletions: u64,
    pub(crate) files_changed: u64,
    pub(crate) ahead: u64,
    pub(crate) behind: u64,
    pub(crate) fetched_at: Instant,
}

impl GitStatusSnapshot {
    pub(crate) fn is_fresh_for(&self, path: &str) -> bool {
        self.path == path && self.fetched_at.elapsed().as_secs() < GIT_STATUS_MAX_AGE_SECS
    }
}

fn git_stdout(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// `--numstat` line parsing: `12\t0\tpath.rs` → (12, 0); binary `-\t-\tbin` counts the file but not lines.
pub(crate) fn parse_numstat(numstat: &str) -> (u64, u64, u64) {
    let mut additions = 0u64;
    let mut deletions = 0u64;
    let mut files = 0u64;
    for line in numstat.lines() {
        if line.trim().is_empty() {
            continue;
        }
        files += 1;
        let mut parts = line.split('\t');
        if let (Some(add), Some(del)) = (parts.next(), parts.next()) {
            additions += add.parse::<u64>().unwrap_or(0);
            deletions += del.parse::<u64>().unwrap_or(0);
        }
    }
    (additions, deletions, files)
}

/// `rev-list --left-right --count @{u}...HEAD` → (behind, ahead);
/// no upstream / parse failure → (0, 0) (presentation semantics: treated simply as no divergence).
pub(crate) fn parse_ahead_behind(counts: &str) -> (u64, u64) {
    let mut parts = counts.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some(behind), Some(ahead)) => (behind.parse().unwrap_or(0), ahead.parse().unwrap_or(0)),
        _ => (0, 0),
    }
}

pub(crate) fn git_status_snapshot(path: &str) -> Option<GitStatusSnapshot> {
    let dir = Path::new(path);
    let branch = git_stdout(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?
        .trim()
        .to_string();
    let (additions, deletions, files_changed) =
        parse_numstat(&git_stdout(dir, &["diff", "HEAD", "--numstat"]).unwrap_or_default());
    let (behind, ahead) = parse_ahead_behind(
        &git_stdout(dir, &["rev-list", "--left-right", "--count", "@{u}...HEAD"])
            .unwrap_or_default(),
    );
    Some(GitStatusSnapshot {
        path: path.to_string(),
        branch,
        additions,
        deletions,
        files_changed,
        ahead,
        behind,
        fetched_at: Instant::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_sums_skipping_binary_lines() {
        let (add, del, files) = parse_numstat("12\t0\ta.rs\n0\t3\tb.rs\n-\t-\tbin.png\n");
        assert_eq!((add, del, files), (12, 3, 3));
        assert_eq!(parse_numstat(""), (0, 0, 0));
    }

    #[test]
    fn ahead_behind_parses_left_right_counts() {
        assert_eq!(parse_ahead_behind("2\t5\n"), (2, 5));
        assert_eq!(parse_ahead_behind(""), (0, 0));
        assert_eq!(parse_ahead_behind("garbage"), (0, 0));
    }
}
