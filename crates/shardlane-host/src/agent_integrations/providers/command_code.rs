//! [INPUT]: depends on super::super::managed_file (atomic managed install),
//! herdr_official::EnsureOutcome, and the Mod asset embedded via include_str!.
//! [OUTPUT]: exposes the idempotent install of the Command Code managed Mod
//! (`ensure_mod_installed` / `ensure_mod_installed_at`), user mods directory
//! resolution (`user_mods_dir` / `managed_mod_path`), and the embedded Mod
//! source (`managed_mod_source`).
//! [POS]: the Rust side of coverage Batch C: the Mod file
//! is managed by Shardlane (the file name is the owned entry; content drift
//! is repaired atomically) and installs into Command Code's user-level mods
//! directory (verified against the v1.15.1 package:
//! `~/.commandcode/mods/<name>.ts`, personal scope, auto-loaded next
//! session; the project scope has its own trust gate and is therefore not
//! used). All runtime state semantics live in the Mod asset; this module
//! only installs and is not a monitor.

use super::super::herdr_official::EnsureOutcome;
use super::super::managed_file::{install_managed_file, ManagedFileOutcome};
use std::env;
use std::io;
use std::path::{Path, PathBuf};

pub const MANAGED_MOD_FILE_NAME: &str = "shardlane-herdr-agent-state.ts";

const MANAGED_MOD_SOURCE: &str = include_str!("../assets/shardlane-herdr-agent-state.ts");

/// The embedded managed Mod source (pinned by test contract + shown in
/// diagnostics).
pub fn managed_mod_source() -> &'static str {
    MANAGED_MOD_SOURCE
}

/// Command Code's user-level mods directory (None when HOME is missing).
pub fn user_mods_dir() -> Option<PathBuf> {
    env::var_os("HOME").map(|home| PathBuf::from(home).join(".commandcode").join("mods"))
}

/// Target path of the managed Mod.
pub fn managed_mod_path() -> Option<PathBuf> {
    user_mods_dir().map(|dir| dir.join(MANAGED_MOD_FILE_NAME))
}

/// Idempotently installs the Shardlane-managed Command Code Mod into the
/// user-level mods directory. Aligned with the herdr official integration
/// semantics: `Installed` / `AlreadyCurrent`.
pub fn ensure_mod_installed() -> Result<EnsureOutcome, io::Error> {
    let Some(dest) = managed_mod_path() else {
        return Err(io::Error::other(
            "HOME is not set; cannot locate the Command Code mods directory",
        ));
    };
    ensure_mod_installed_at(&dest)
}

/// Install implementation with an injectable path (for tests; production
/// goes through `ensure_mod_installed`).
pub fn ensure_mod_installed_at(dest: &Path) -> Result<EnsureOutcome, io::Error> {
    match install_managed_file(dest, MANAGED_MOD_SOURCE)? {
        ManagedFileOutcome::Installed => Ok(EnsureOutcome::Installed),
        ManagedFileOutcome::Unchanged => Ok(EnsureOutcome::AlreadyCurrent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ensure_maps_install_outcomes_and_is_idempotent() {
        let dir = TempDir::new().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let dest = dir.path().join(MANAGED_MOD_FILE_NAME);

        assert_eq!(
            ensure_mod_installed_at(&dest).unwrap_or_else(|error| panic!("install: {error}")),
            EnsureOutcome::Installed
        );
        assert_eq!(
            ensure_mod_installed_at(&dest).unwrap_or_else(|error| panic!("reinstall: {error}")),
            EnsureOutcome::AlreadyCurrent,
            "idempotent re-entry must not rewrite the managed Mod"
        );
        assert!(dest.exists());
    }

    #[test]
    fn embedded_mod_source_matches_the_verified_v1151_contract() {
        let source = managed_mod_source();
        // Event surface subscribed to (event names verified to exist in the
        // package's cli.mjs).
        for event in [
            "session_start",
            "run_start",
            "run_end",
            "interrupted",
            "run_error",
            "session_shutdown",
        ] {
            assert!(
                source.contains(&format!("cmd.on(\"{event}\"")),
                "the managed Mod must subscribe to {event}"
            );
        }
        // Identity and protocol contract (plan §5/§6.13).
        assert!(source.contains(SOURCE_ID), "source id missing");
        assert!(
            source.contains("commandcode"),
            "herdr protocol label missing"
        );
        assert!(source.contains("HERDR_PANE_ID"), "pane guard missing");
        assert!(source.contains("HERDR_BIN_PATH"), "CLI guard missing");
        assert!(
            source.contains("--agent-session-id"),
            "session identity missing"
        );
        assert!(
            source.contains("sessionId"),
            "run_start payload field missing (the package's run_start event is verified to carry sessionId)"
        );
        // blocked gap: the current Mod API has no approval-waiting event; do
        // not fabricate one.
        assert!(
            !source.contains("\"blocked\""),
            "fabricating a blocked state is forbidden (plan §6.13)"
        );
        // Sequence contract (including release: live testing verified that a
        // release without seq is silently ignored by the watermark guard).
        assert!(source.contains("--seq"), "monotonic sequence missing");
        assert!(
            source.contains("releaseArgs"),
            "the release path must carry a monotonic seq (verified against Herdr 0.8.2)"
        );
        // Exit anchor contract: v1.15.1 process exit does not trigger
        // session_shutdown, so release must also hook Node exit (verified
        // live).
        assert!(
            source.contains("process.once(\"exit\""),
            "process-exit release anchor missing"
        );
        // Session identity uses the dedicated method (for custom labels,
        // report-agent's --agent-session-id does not project agent_session;
        // verified against Herdr 0.8.2).
        assert!(
            source.contains("report-agent-session"),
            "dedicated session report method missing"
        );
    }

    const SOURCE_ID: &str = "shardlane:commandcode:v1";

    #[test]
    #[ignore = "live: writes the real ~/.commandcode/mods/ (product install path); not run in CI"]
    fn live_ensure_installs_user_mod() {
        match ensure_mod_installed() {
            Ok(EnsureOutcome::Installed) => {
                eprintln!("installed managed mod to {:?}", managed_mod_path());
            }
            Ok(EnsureOutcome::AlreadyCurrent) => {
                eprintln!("managed mod already current at {:?}", managed_mod_path());
            }
            outcome => panic!("unexpected outcome: {outcome:?}"),
        }
    }
}
