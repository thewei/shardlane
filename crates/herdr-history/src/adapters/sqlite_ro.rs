// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

use rusqlite::{Connection, OpenFlags};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Copy cap for whole-database fallback when WAL locks block direct reads:
/// beyond it, the copy fallback is abandoned (same path as "unreadable",
/// honored by the scanner's discovery-failure guard to skip cleanup) so huge
/// databases are not copied wholesale into /tmp repeatedly.
const SQLITE_COPY_MAX_BYTES: u64 = 256 * 1024 * 1024;

static TEMP_DIR_SEQ: AtomicU64 = AtomicU64::new(0);

pub(crate) fn copy_within_limit(len: u64, cap: u64) -> bool {
    len <= cap
}

pub struct SqliteRo {
    pub conn: Connection,
    _temp_dir: Option<TempDirGuard>,
}

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn open_sqlite_ro(db: &Path, tag: &str) -> Option<SqliteRo> {
    if !db.is_file() {
        return None;
    }

    if let Ok(conn) = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        let probe: rusqlite::Result<i64> =
            conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0));
        if probe.is_ok() {
            return Some(SqliteRo {
                conn,
                _temp_dir: None,
            });
        }
    }

    let temp_dir = std::env::temp_dir().join(format!(
        "herdr-history-{tag}-{}-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("reader"),
        TEMP_DIR_SEQ.fetch_add(1, Ordering::Relaxed),
    ));
    // Directory name carries an atomic sequence number: the scanner background
    // thread and the GUI fallback thread no longer collide into interleaved
    // torn copies when unnamed.
    let source_len = fs::metadata(db).map(|meta| meta.len()).unwrap_or(0);
    if !copy_within_limit(source_len, SQLITE_COPY_MAX_BYTES) {
        return None;
    }
    fs::create_dir_all(&temp_dir).ok()?;
    let guard = TempDirGuard(temp_dir.clone());
    let db_copy = temp_dir.join("db.sqlite");
    fs::copy(db, &db_copy).ok()?;
    for suffix in ["-wal", "-shm"] {
        let source = PathBuf::from(format!("{}{suffix}", db.display()));
        if source.is_file() {
            let _ = fs::copy(&source, temp_dir.join(format!("db.sqlite{suffix}")));
        }
    }
    let conn = Connection::open_with_flags(&db_copy, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    Some(SqliteRo {
        conn,
        _temp_dir: Some(guard),
    })
}

pub fn virtual_path(db: &Path, id: &str) -> String {
    format!("{}#{id}", db.display())
}
