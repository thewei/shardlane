// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

/**
 * [INPUT]: 依赖 rusqlite 的只读连接与 OpenFlags、std::fs 的拷贝/权限原语、
 *          上游传入的外部 Agent SQLite 数据库路径
 * [OUTPUT]: 对外提供 SqliteRo（临时只读镜像连接）与 virtual_path（镜像虚拟路径
 *           拼装）；内部维护拷贝上限与临时目录守卫
 * [POS]: adapters 的 SQLite 只读镜像器——把外部只读数据库以受控副本暴露给
 *        目录层，平台差异（unix 权限位 / windows 默认 ACL）收敛在
 *        harden_temp_dir 与 open_hardened_copy 两个 helper 内
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

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

/// Tighten a freshly created temp directory to owner-only access. Unix does
/// this with mode bits (0700); windows leans on the user-profile default ACL,
/// which already scopes the directory to the running user.
fn harden_temp_dir(temp_dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = fs::set_permissions(temp_dir, fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = temp_dir;
}

/// Open the hardened copy exclusively: `create_new` rejects symlinks and
/// pre-planted files, so the copy cannot be redirected to an
/// attacker-controlled location. Unix additionally creates it with 0600
/// instead of chmod-after-write.
fn open_hardened_copy(db_copy: &Path) -> Option<fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(db_copy)
            .ok()
    }
    #[cfg(not(unix))]
    {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(db_copy)
            .ok()
    }
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
    // 多用户主机加固：目录收紧到仅属主可见，目标文件 create_new 独占创建——
    // create_new 天然拒绝符号链接与预置文件，拷贝不会被引向
    // 攻击者可读的位置（目录名含 pid/seq，本可被预测抢注）。
    harden_temp_dir(&temp_dir);
    let mut output = open_hardened_copy(&db_copy)?;
    let mut input = fs::File::open(db).ok()?;
    std::io::copy(&mut input, &mut output).ok()?;
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
