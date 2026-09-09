//! Client-owned auto update check for the Shardlane app itself.
//!
//! The release workflow uploads a small update manifest (latest.json) as a
//! release asset on every tag; this module fetches it through the stable
//! releases/latest/download permalink, compares it against the running
//! CARGO_PKG_VERSION, and records the result for the Settings card. All
//! transport is curl (system binary, bounded timeout) so the GUI crate keeps
//! zero HTTP-client dependencies.
//!
//! [INPUT]: depends on serde_json (manifest parsing), the system curl binary
//! (bounded GET), and the SHARDLANE_UPDATE_MANIFEST_URL env override
//! [OUTPUT]: exposes check / is_newer / fetch_manifest, the last-detected
//! update store (last_newer / record_newer / clear_newer), and
//! interval_duration
//! [POS]: a Shardlane-owned client surface (never a mux/backend concern); the
//! scheduler loop lives in main.rs and re-reads the on-disk toggle every
//! cycle; the Settings -> Behavior card projects the recorded result
//! [PROTOCOL]: Update this header on change, then check CLAUDE.md.

use serde::Deserialize;
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

/// The manifest permalink: always resolves to the newest release's copy.
const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/thewei/shardlane/releases/latest/download/latest.json";

/// Update manifest published by the release workflow on every tag.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct UpdateManifest {
    pub version: String,
    pub url: String,
    #[serde(default)]
    pub notes_url: Option<String>,
}

/// The manifest URL: SHARDLANE_UPDATE_MANIFEST_URL override (isolated tests /
/// future self-hosted channels), then the release permalink.
pub fn manifest_url() -> String {
    std::env::var("SHARDLANE_UPDATE_MANIFEST_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MANIFEST_URL.to_string())
}

/// Fetch and parse the manifest. Best-effort by contract: every failure mode
/// (curl missing, offline, malformed body) degrades to a plain error string.
pub fn fetch_manifest(url: &str) -> Result<UpdateManifest, String> {
    let output = Command::new("curl")
        .args(["-fsSL", "--max-time", "10", "--proto", "=https", url])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("curl spawn failed: {error}"))?;
    if !output.status.success() {
        return Err(format!("manifest fetch failed: {}", output.status));
    }
    let body = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str::<UpdateManifest>(body.trim())
        .map_err(|error| format!("manifest parse failed: {error}"))
}

/// Dotted numeric compare with zero padding: 0.1.9 < 0.1.10, 0.1 < 0.1.1,
/// non-numeric suffixes compare as their leading digits. Equal is not newer.
pub fn is_newer(current: &str, candidate: &str) -> bool {
    fn segments(version: &str) -> Vec<u64> {
        version
            .split('.')
            .map(|segment| {
                let digits: String = segment.chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse::<u64>().unwrap_or(0)
            })
            .collect()
    }
    let mut current = segments(current);
    let mut candidate = segments(candidate);
    let len = current.len().max(candidate.len());
    current.resize(len, 0);
    candidate.resize(len, 0);
    candidate > current
}

/// Fetch the manifest and report it only when it is newer than the running
/// version. Ok(None) = fetch worked and there is nothing newer.
pub fn check(current_version: &str) -> Result<Option<UpdateManifest>, String> {
    let manifest = fetch_manifest(&manifest_url())?;
    if is_newer(current_version, &manifest.version) {
        Ok(Some(manifest))
    } else {
        Ok(None)
    }
}

/// The cycle cadence: hours clamped to [1, 168] (a week).
pub fn interval_duration(hours: u32) -> Duration {
    Duration::from_secs(hours.clamp(1, 168) as u64 * 3600)
}

// --- Last-detection store (Settings card projection) ---

static LAST_NEWER: Mutex<Option<UpdateManifest>> = Mutex::new(None);
static NOTIFIED_VERSION: Mutex<Option<String>> = Mutex::new(None);

/// The newest release seen by the periodic check, if any. Read by the
/// Settings -> Behavior card.
pub fn last_newer() -> Option<UpdateManifest> {
    LAST_NEWER
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

/// Record a newer release; returns true the first time this exact version is
/// recorded so the notification fires once per version, not once per cycle.
pub fn record_newer(manifest: UpdateManifest) -> bool {
    let mut notified = NOTIFIED_VERSION
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let first_time = notified.as_deref() != Some(manifest.version.as_str());
    *notified = Some(manifest.version.clone());
    drop(notified);
    *LAST_NEWER
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = Some(manifest);
    first_time
}

/// The running version is current again (or the check succeeded with no
/// newer release): clear the Settings card projection.
pub fn clear_newer() {
    *LAST_NEWER
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = None;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_compares_components_numerically() {
        assert!(is_newer("0.1.14", "0.1.15"));
        assert!(is_newer("0.1.9", "0.1.10"), "numeric, not lexicographic");
        assert!(is_newer("0.1.14", "0.2.0"));
        assert!(is_newer("0.1", "0.1.1"), "candidate pads with zeros");
        assert!(!is_newer("0.1.14", "0.1.14"));
        assert!(!is_newer("0.1.14", "0.1.13"));
        assert!(!is_newer("0.2.0", "0.1.99"));
    }

    #[test]
    fn is_newer_tolerates_prerelease_suffixes_as_zero_padded_digits() {
        // "0-rc1" parses its leading digits (0): equal components are not newer.
        assert!(!is_newer("1.0.0", "1.0.0-rc1"));
        assert!(is_newer("1.0.0-rc1", "1.0.1"));
    }

    #[test]
    fn manifest_parses_the_release_workflow_shape() {
        let manifest: UpdateManifest = serde_json::from_str(
            r#"{"version":"0.1.15","url":"https://example.com/Shardlane.zip","notes_url":"https://example.com/tag/v0.1.15"}"#,
        )
        .unwrap();
        assert_eq!(manifest.version, "0.1.15");
        assert_eq!(
            manifest.notes_url.as_deref(),
            Some("https://example.com/tag/v0.1.15")
        );

        let minimal: UpdateManifest = serde_json::from_str(
            r#"{"version":"0.1.15","url":"https://example.com/Shardlane.zip"}"#,
        )
        .unwrap();
        assert_eq!(minimal.notes_url, None);

        // Missing required fields must fail closed, not produce a half manifest.
        assert!(serde_json::from_str::<UpdateManifest>(r#"{"version":"0.1.15"}"#).is_err());
    }

    #[test]
    fn interval_duration_clamps_to_one_week() {
        assert_eq!(interval_duration(0), Duration::from_secs(3600));
        assert_eq!(interval_duration(24), Duration::from_secs(24 * 3600));
        assert_eq!(interval_duration(1_000), Duration::from_secs(168 * 3600));
    }

    #[test]
    fn record_newer_notifies_once_per_version_and_clear_resets() {
        clear_newer();
        let manifest = UpdateManifest {
            version: "9.9.9".into(),
            url: "https://example.com/Shardlane.zip".into(),
            notes_url: None,
        };
        assert!(record_newer(manifest.clone()), "first sighting notifies");
        assert!(!record_newer(manifest.clone()), "same version stays quiet");
        assert_eq!(last_newer(), Some(manifest));
        clear_newer();
        assert_eq!(last_newer(), None);
        // A different version notifies again.
        let next = UpdateManifest {
            version: "9.9.10".into(),
            url: "https://example.com/Shardlane.zip".into(),
            notes_url: None,
        };
        assert!(record_newer(next));
        clear_newer();
    }
}
