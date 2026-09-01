//! [INPUT]: depends on std::fs/io (atomic write: same-directory temp file +
//! rename).
//! [OUTPUT]: exposes `install_managed_file` (idempotent, atomic install of a
//! Shardlane-managed file) and `ManagedFileOutcome`.
//! [POS]: the landing primitive for third-party config safety in plan §7.5:
//! **the file name is the owned entry** — only files under a Shardlane-
//! managed name are created/replaced by this function (a same-named file
//! with different content is treated as our own stale version and atomically
//! repaired); foreign files are never touched; any write failure propagates
//! as-is for the caller to present as NeedsManualInstall, and a half-write
//! never happens.

use std::fs;
use std::io;
use std::path::Path;

/// Managed file install outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedFileOutcome {
    /// New content was written this time (first install or an atomic repair
    /// after content drift).
    Installed,
    /// The destination already exists and matches the managed content byte
    /// for byte; nothing was written.
    Unchanged,
}

/// Idempotently installs a Shardlane-managed file (atomic replacement).
pub fn install_managed_file(dest: &Path, content: &str) -> io::Result<ManagedFileOutcome> {
    if let Ok(existing) = fs::read_to_string(dest) {
        if existing == content {
            return Ok(ManagedFileOutcome::Unchanged);
        }
    }
    let Some(parent) = dest.parent() else {
        return Err(io::Error::other(
            "managed file destination has no parent directory",
        ));
    };
    fs::create_dir_all(parent)?;
    let file_name = dest
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("shardlane-managed");
    let tmp = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    fs::write(&tmp, content)?;
    match fs::rename(&tmp, dest) {
        Ok(()) => Ok(ManagedFileOutcome::Installed),
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn installs_then_is_unchanged_then_repairs_drift() {
        let dir = TempDir::new().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let dest = dir.path().join("nested").join("mod.ts");

        assert_eq!(
            install_managed_file(&dest, "v1").unwrap_or_else(|error| panic!("install: {error}")),
            ManagedFileOutcome::Installed
        );
        assert_eq!(
            install_managed_file(&dest, "v1").unwrap_or_else(|error| panic!("reinstall: {error}")),
            ManagedFileOutcome::Unchanged,
            "reinstalling identical content must not write"
        );
        assert_eq!(
            install_managed_file(&dest, "v2").unwrap_or_else(|error| panic!("repair: {error}")),
            ManagedFileOutcome::Installed,
            "content drift is treated as a stale version and repaired atomically"
        );
        let on_disk = fs::read_to_string(&dest).unwrap_or_else(|error| panic!("read: {error}"));
        assert_eq!(on_disk, "v2");
        // Atomic replacement leaves no temp files behind.
        let leftovers: Vec<_> = fs::read_dir(dest.parent().unwrap_or_else(|| panic!("parent")))
            .unwrap_or_else(|error| panic!("read_dir: {error}"))
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(leftovers, vec!["mod.ts"]);
    }

    #[test]
    fn never_touches_a_foreign_file() {
        let dir = TempDir::new().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let foreign = dir.path().join("user-mod.ts");
        fs::write(&foreign, "user content").unwrap_or_else(|error| panic!("seed foreign: {error}"));
        // This function only works on the path the caller passes in; verify
        // only that an unmanaged path is not touched as a side effect.
        assert_eq!(
            install_managed_file(&dir.path().join("other.ts"), "x")
                .unwrap_or_else(|error| panic!("install: {error}")),
            ManagedFileOutcome::Installed
        );
        let still = fs::read_to_string(&foreign).unwrap_or_else(|error| panic!("read: {error}"));
        assert_eq!(still, "user content");
    }
}
