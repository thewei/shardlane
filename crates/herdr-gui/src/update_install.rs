//! Client-owned staged update install for the Shardlane app itself.
//!
//! Completes the story started by update_check: once a newer release
//! manifest is on record, this module downloads the release ZIP (curl,
//! bounded), verifies its SHA-256 against the release's .sha256 asset
//! (shasum), extracts the bundle with ditto into a versioned staging
//! directory, and -- on an explicit "Update and Restart" -- swaps the
//! running .app with the staged one (rename the current bundle to a backup,
//! move the staged bundle into place, roll back on any failure) and
//! relaunches through the macOS "open" service. Every filesystem path is
//! derived from the running bundle or from the app-owned
//! ~/.shardlane/updates root; nothing outside those roots is touched. Dev
//! builds (not running from a .app) degrade to a disabled card and keep the
//! old open-the-releases-page behavior.
//!
//! [INPUT]: depends on update_check::UpdateManifest, the system curl/shasum/
//! ditto/open binaries, gpui App/AsyncApp/WeakEntity for the download
//! pipeline, and crate::notifications
//! [OUTPUT]: exposes InstallState + install_state, begin_staged_download,
//! install_and_restart, cleanup_after_restart, can_install, and the pure
//! helpers parse_checksum / safe_version / archive_name_from_url /
//! format_progress
//! [POS]: herdr-gui's update-install lifecycle beside update_check.rs (the
//! check answers "is something newer", install answers "get it on disk and
//! swap"); the Settings Behavior card projects the state machine, and the
//! main.rs scheduler triggers auto-downloads plus the startup cleanup
//! [PROTOCOL]: Update this header on change, then check CLAUDE.md.

use crate::update_check::UpdateManifest;
use gpui::{App, AsyncApp, WeakEntity};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Bundle layout constants (matches scripts/package-macos.sh).
const APP_BUNDLE_NAME: &str = "Shardlane.app";
const BACKUP_PREFIX: &str = "Shardlane.app.backup-";

/// The staged-install lifecycle: Idle -> Downloading -> Ready -> (restart,
/// the new process starts Idle again) or Failed at any step. Downloads are
/// single-flight: begin_staged_download rejects while a download is running.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallState {
    Idle,
    Downloading {
        version: String,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    Ready {
        version: String,
    },
    Failed {
        version: String,
        error: String,
    },
}

static INSTALL_STATE: Mutex<InstallState> = Mutex::new(InstallState::Idle);
/// Generation guard: only the newest download may write terminal state, so a
/// stale pipeline can never resurrect superseded progress. Single-flight
/// state rejection makes this belt-and-suspenders rather than load-bearing.
static DOWNLOAD_GEN: AtomicU64 = AtomicU64::new(0);

pub fn install_state() -> InstallState {
    INSTALL_STATE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

fn set_state(state: InstallState) {
    *INSTALL_STATE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = state;
}

fn current_generation() -> u64 {
    DOWNLOAD_GEN.load(Ordering::SeqCst)
}

fn set_state_if_current(generation: u64, state: InstallState) {
    if generation == current_generation() {
        set_state(state);
    }
}

// --- Paths ---

/// The app-owned staging root. SHARDLANE_UPDATE_STAGING_ROOT overrides it
/// (isolated tests / future channels); HOME follows the settings.rs
/// convention.
pub fn staging_root() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("SHARDLANE_UPDATE_STAGING_ROOT") {
        if !root.is_empty() {
            return Some(PathBuf::from(root));
        }
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".shardlane").join("updates"))
}

/// The .app bundle the current process runs from, if any. None = dev build
/// (cargo run / target dir), where in-place updates do not apply.
pub fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .skip(1)
        .find(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
        })
        .map(Path::to_path_buf)
}

pub fn can_install() -> bool {
    running_bundle().is_some()
}

fn running_executable_name() -> Option<std::ffi::OsString> {
    std::env::current_exe()
        .ok()?
        .file_name()
        .map(|name| name.to_os_string())
}

fn reset_dir(path: &Path) -> Result<(), String> {
    if path.exists() {
        std::fs::remove_dir_all(path).map_err(|error| format!("staging reset failed: {error}"))?;
    }
    std::fs::create_dir_all(path).map_err(|error| format!("staging setup failed: {error}"))
}

// --- Pure helpers ---

/// The checksum asset is "shasum -a 256 <archive>" output: "<hash>  <name>".
pub fn parse_checksum(body: &str) -> Option<String> {
    let line = body.lines().map(str::trim).find(|line| !line.is_empty())?;
    let token = line.split_whitespace().next()?;
    (token.len() == 64 && token.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| token.to_ascii_lowercase())
}

/// Manifest versions become path components; accept only plain release tags.
pub fn safe_version(version: &str) -> Option<&str> {
    let ok = !version.is_empty()
        && version.len() <= 64
        && !version.contains("..")
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    ok.then_some(version)
}

pub fn archive_name_from_url(url: &str) -> Option<String> {
    let name = url.rsplit('/').next()?.trim();
    (name.len() > 4 && name.ends_with(".zip") && !name.contains('?')).then(|| name.to_string())
}

pub fn format_progress(downloaded: u64, total: Option<u64>) -> String {
    fn mb(bytes: u64) -> String {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
    match total {
        Some(total) if total > 0 => format!(
            "{} of {} ({:.0}%)",
            mb(downloaded),
            mb(total),
            downloaded.min(total) as f64 / total as f64 * 100.0
        ),
        _ => mb(downloaded),
    }
}

// --- System wrappers (curl / shasum / ditto / open) ---

fn curl_bytes(url: &str, max_time: &str) -> Result<Vec<u8>, String> {
    let output = Command::new("curl")
        .args(["-fsSL", "--proto", "=https", "--max-time", max_time])
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("curl spawn failed: {error}"))?;
    if !output.status.success() {
        return Err(format!("request failed: {}", output.status));
    }
    Ok(output.stdout)
}

fn fetch_checksum(archive_url: &str) -> Result<String, String> {
    let body = curl_bytes(&format!("{archive_url}.sha256"), "30")?;
    parse_checksum(&String::from_utf8_lossy(&body))
        .ok_or_else(|| "checksum asset is malformed".into())
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("shasum spawn failed: {error}"))?;
    if !output.status.success() {
        return Err(format!("checksum failed: {}", output.status));
    }
    parse_checksum(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "shasum output is malformed".into())
}

fn content_length(url: &str) -> Option<u64> {
    let output = Command::new("curl")
        .args(["-sIL", "--proto", "=https", "--max-time", "30"])
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.to_ascii_lowercase().starts_with("content-length:"))
        .next_back()
        .and_then(|line| line.split(':').nth(1))
        .and_then(|value| value.trim().parse().ok())
}

fn verify_archive(zip: &Path, archive_url: &str) -> Result<(), String> {
    let expected = fetch_checksum(archive_url)?;
    let actual = file_sha256(zip)?;
    if !expected.eq_ignore_ascii_case(&actual) {
        return Err("SHA-256 mismatch -- the download is corrupted or tampered with".into());
    }
    Ok(())
}

fn extract_archive(zip: &Path, into: &Path) -> Result<(), String> {
    let output = Command::new("ditto")
        .args(["-x", "-k"])
        .arg(zip)
        .arg(into)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("ditto spawn failed: {error}"))?;
    if !output.status.success() {
        return Err(format!("extraction failed: {}", output.status));
    }
    Ok(())
}

// --- Download pipeline ---

/// Kick off the staged download: verify preconditions, project the
/// Downloading state immediately, and run the pipeline on the background
/// executor. The weak entity, when given, is notified as progress advances
/// so an open Settings card live-updates.
pub fn begin_staged_download(
    manifest: UpdateManifest,
    notify: Option<WeakEntity<crate::ShardlaneApp>>,
    cx: &mut App,
) -> Result<(), String> {
    if !can_install() {
        return Err("Shardlane is not running from an app bundle".into());
    }
    if matches!(install_state(), InstallState::Downloading { .. }) {
        return Err("a download is already in progress".into());
    }
    let Some(base) = staging_root() else {
        return Err("cannot resolve the update staging directory".into());
    };
    if safe_version(&manifest.version).is_none() {
        return Err("manifest version is not a safe path component".into());
    }
    let total_bytes = content_length(&manifest.url);
    set_state(InstallState::Downloading {
        version: manifest.version.clone(),
        downloaded_bytes: 0,
        total_bytes,
    });
    let generation = DOWNLOAD_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    cx.spawn(async move |cx| {
        let version = manifest.version.clone();
        let outcome = download_pipeline(manifest, base, total_bytes, generation, cx).await;
        match outcome {
            Ok(()) => {
                set_state_if_current(generation, InstallState::Ready { version: version.clone() });
                crate::notifications::show(
                    "Shardlane update staged",
                    &format!(
                        "Version {version} is verified and ready. Settings -> Behavior -> Update and Restart."
                    ),
                );
            }
            Err(error) => set_state_if_current(generation, InstallState::Failed { version, error }),
        }
        if let Some(weak) = &notify {
            let _ = weak.update(cx, |_, cx| cx.notify());
        }
    })
    .detach();
    Ok(())
}

async fn download_pipeline(
    manifest: UpdateManifest,
    base: PathBuf,
    total_bytes: Option<u64>,
    generation: u64,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    let version_dir = base.join(&manifest.version);
    let zip_path = version_dir
        .join(archive_name_from_url(&manifest.url).ok_or("manifest URL has no archive name")?);
    let url = manifest.url.clone();
    let version = manifest.version.clone();

    // 1. Reset this version's staging area (bounded to our own directory).
    let dir = version_dir.clone();
    cx.background_executor()
        .spawn(async move { reset_dir(&dir) })
        .await?;

    // 2. Stream the archive with curl; progress comes from statting the file
    //    against the HEAD content-length, never from parsing curl output.
    let zip = zip_path.clone();
    let curl_url = url.clone();
    let mut child = cx
        .background_executor()
        .spawn(async move {
            Command::new("curl")
                .args([
                    "-fSL",
                    "--proto",
                    "=https",
                    "--connect-timeout",
                    "15",
                    "--max-time",
                    "1800",
                    "-o",
                ])
                .arg(&zip)
                .arg(&curl_url)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| format!("curl spawn failed: {error}"))
        })
        .await?;
    let mut last_reported = 0u64;
    let progress_zip = zip_path.clone();
    loop {
        cx.background_executor()
            .timer(Duration::from_millis(500))
            .await;
        let progress_zip = progress_zip.clone();
        let (child_back, status, downloaded) = cx
            .background_executor()
            .spawn(async move {
                let status = child.try_wait();
                let downloaded = std::fs::metadata(&progress_zip)
                    .map(|meta| meta.len())
                    .unwrap_or(0);
                (child, status, downloaded)
            })
            .await;
        child = child_back;
        if generation != current_generation() {
            return Ok(());
        }
        if downloaded != last_reported {
            last_reported = downloaded;
            set_state(InstallState::Downloading {
                version: version.clone(),
                downloaded_bytes: downloaded,
                total_bytes,
            });
        }
        match status {
            Ok(Some(exit)) if exit.success() => break,
            Ok(Some(exit)) => return Err(format!("download failed ({exit})")),
            Ok(None) => continue,
            Err(error) => return Err(format!("download failed: {error}")),
        }
    }

    // 3. Verify, extract, sanity-check the staged bundle.
    let verify_zip = zip_path.clone();
    let verify_url = url.clone();
    cx.background_executor()
        .spawn(async move { verify_archive(&verify_zip, &verify_url) })
        .await?;
    let extract_zip = zip_path.clone();
    let extract_dir = version_dir.clone();
    cx.background_executor()
        .spawn(async move { extract_archive(&extract_zip, &extract_dir) })
        .await?;
    let check_dir = version_dir.clone();
    let staged_executable_present = cx
        .background_executor()
        .spawn(async move {
            running_executable_name()
                .and_then(|name| {
                    let executable = check_dir
                        .join(APP_BUNDLE_NAME)
                        .join("Contents")
                        .join("MacOS")
                        .join(name);
                    executable.is_file().then_some(())
                })
                .is_some()
        })
        .await;
    if !staged_executable_present {
        return Err("staged bundle is missing its executable".into());
    }
    let _ = std::fs::remove_file(&zip_path);
    Ok(())
}

// --- Install and restart ---

/// Swap the running bundle with the staged one and relaunch. Renames are
/// same-directory (atomic on one volume); the ditto fallback covers a
/// staging root on a different volume. Every failure rolls the previous
/// bundle back into place before surfacing the error.
pub fn install_and_restart() -> Result<(), String> {
    let InstallState::Ready { version } = install_state() else {
        return Err("no staged update is ready to install".into());
    };
    let Some(current) = running_bundle() else {
        return Err("Shardlane is not running from an app bundle".into());
    };
    let Some(base) = staging_root() else {
        return Err("cannot resolve the update staging directory".into());
    };
    let Some(exe_name) = running_executable_name() else {
        return Err("cannot resolve the running executable".into());
    };
    let Some(parent) = current.parent().map(Path::to_path_buf) else {
        return Err("the app bundle has no parent directory".into());
    };
    let staged = base
        .join(safe_version(&version).ok_or("staged version is not a safe path component")?)
        .join(APP_BUNDLE_NAME);
    if !staged
        .join("Contents")
        .join("MacOS")
        .join(&exe_name)
        .is_file()
    {
        return Err("staged bundle is missing its executable".into());
    }
    let target = parent.join(APP_BUNDLE_NAME);
    let backup = parent.join(format!("{BACKUP_PREFIX}{version}"));

    // 1. Move the running bundle aside (same directory: atomic rename).
    if backup.exists() {
        std::fs::remove_dir_all(&backup)
            .map_err(|error| format!("could not clear the previous backup: {error}"))?;
    }
    std::fs::rename(&current, &backup)
        .map_err(|error| format!("could not back up the current app: {error}"))?;

    // 2. Move the staged bundle into place; fall back to a copy across
    //    volumes, rolling everything back if that fails too.
    if std::fs::rename(&staged, &target).is_err() {
        let copied = Command::new("ditto")
            .arg(&staged)
            .arg(&target)
            .stdin(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !copied {
            let _ = std::fs::rename(&backup, &current);
            return Err("could not move the staged app into place".into());
        }
    }

    // 3. Relaunch through LaunchServices AFTER this process is gone: the
    //    detached shell survives the exit, and "open -n" forces a fresh
    //    instance (same bundle id would otherwise just activate the dying
    //    process). The new instance clears the backup on startup.
    let launched = Command::new("/bin/sh")
        .args(["-c", "sleep 0.3; open -n \"$1\"", "sh"])
        .arg(&target)
        .spawn()
        .is_ok();
    if !launched {
        let _ = std::fs::remove_dir_all(&target);
        let _ = std::fs::rename(&backup, &current);
        return Err("could not relaunch the updated app".into());
    }
    set_state(InstallState::Idle);
    std::process::exit(0);
}

/// After a successful restart: clear the previous instance's backup bundle
/// and any leftover staging. Only the bundle's own parent directory and the
/// app-owned staging root are ever touched, and only exact name matches.
pub fn cleanup_after_restart() {
    if let Some(current) = running_bundle() {
        if let Some(parent) = current.parent() {
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if name.to_string_lossy().starts_with(BACKUP_PREFIX) && entry.path().is_dir() {
                        let _ = std::fs::remove_dir_all(entry.path());
                    }
                }
            }
        }
    }
    if let Some(base) = staging_root() {
        let _ = std::fs::remove_dir_all(base);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_checksum_takes_the_first_hash_token() {
        let digest = "0123456789abcdef".repeat(4);
        assert_eq!(
            parse_checksum(&format!("{digest}  Shardlane-macos-aarch64.zip\n")),
            Some(digest.clone())
        );
        assert_eq!(
            parse_checksum(&format!("{}  archive.zip", digest.to_uppercase())),
            Some(digest),
            "lowercases the digest"
        );
        assert_eq!(parse_checksum(""), None);
        assert_eq!(
            parse_checksum("abc123  file.zip"),
            None,
            "64 hex chars only"
        );
        assert_eq!(parse_checksum("zzzz  file.zip"), None, "hex only");
    }

    #[test]
    fn safe_version_rejects_path_tricks() {
        assert_eq!(safe_version("0.1.16"), Some("0.1.16"));
        assert_eq!(safe_version("1.0.0-rc1"), Some("1.0.0-rc1"));
        assert_eq!(safe_version(""), None);
        assert_eq!(safe_version("../etc"), None);
        assert_eq!(safe_version("a/b"), None);
        assert_eq!(safe_version("a b"), None);
    }

    #[test]
    fn archive_name_requires_a_zip_tail() {
        assert_eq!(
            archive_name_from_url("https://example.com/Shardlane-macos-aarch64.zip"),
            Some("Shardlane-macos-aarch64.zip".into())
        );
        assert_eq!(
            archive_name_from_url("https://example.com/latest.json"),
            None
        );
        assert_eq!(archive_name_from_url("https://example.com/"), None);
    }

    #[test]
    fn format_progress_reports_percent_when_total_known() {
        let mib = 1024.0 * 1024.0;
        assert_eq!(
            format_progress((10.0 * mib) as u64, Some((40.0 * mib) as u64)),
            "10.0 MB of 40.0 MB (25%)"
        );
        assert_eq!(format_progress((10.0 * mib) as u64, None), "10.0 MB");
        assert_eq!(
            format_progress(0, Some(0)),
            "0.0 MB",
            "unknown total degrades"
        );
    }

    #[test]
    fn reset_dir_clears_and_recreates_a_bounded_directory() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "shardlane-update-install-{}-{unique}",
            std::process::id()
        ));
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("artifact"), b"x").unwrap();
        reset_dir(&dir).unwrap();
        assert!(dir.is_dir());
        assert!(!nested.exists(), "contents are cleared");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn running_bundle_is_none_outside_a_bundle() {
        // The test harness runs from target/deps, never inside a .app.
        if std::env::current_exe().unwrap().ancestors().any(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
        }) {
            return;
        }
        assert!(!can_install());
    }
}
