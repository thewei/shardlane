//! [INPUT]: depends on crate::herdr::herdr_cli_path (the single CLI
//! discovery entry; a second PATH resolution is forbidden) and on the
//! registry in super::registry.
//! [OUTPUT]: exposes `HerdrOfficialProvisioner` (read-only audit and
//! idempotent ensure over `herdr integration status/install`),
//! `EnsureOutcome`, `IntegrationError`, `OfficialIntegrationStatus`, and the
//! text-parsing pure functions.
//! [POS]: coverage Batch A: provisioning for official
//! Providers. Kiro/Gemini/CommandCode/Dsh are explicitly
//! `EnsureOutcome::Deferred` at this layer (until Batch B–D lands) and
//! third-party config is never silently touched. This module carries no
//! working/idle/blocked semantics — that is Herdr runtime data (plan §14).

use super::registry::AgentIntegrationEntry;
use crate::herdr::herdr_cli_path;
use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IntegrationError {
    #[error("herdr CLI not found")]
    CliMissing,
    #[error("herdr integration status failed: {0}")]
    Status(String),
    #[error("herdr integration install {target} failed: {message}")]
    Install {
        target: &'static str,
        message: String,
    },
    #[error("managed integration asset for {provider} failed: {message}")]
    ManagedAsset {
        provider: &'static str,
        message: String,
    },
}

/// Parsed result of one `herdr integration status` line.
///
/// `raw_state` keeps the CLI's original text (e.g. `current (v8)` /
/// `not installed`) for diagnostics; `is_current` only accepts a `current`
/// prefix, everything else is treated as needing install — the install is
/// idempotent and performed by Herdr's official path, so convergence is
/// safe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OfficialIntegrationStatus {
    pub target: String,
    pub raw_state: String,
    pub is_current: bool,
}

/// Parses the text output of `herdr integration status` (pure function; no
/// --json surface).
///
/// Line format: `<target>: <state> [(vN)] [(path)]`. Empty lines and lines
/// without `: ` are skipped (staying robust if herdr adds header comments in
/// the future); unknown targets are kept as-is.
pub fn parse_integration_status(output: &str) -> Vec<OfficialIntegrationStatus> {
    output
        .lines()
        .filter_map(|line| {
            let (target, raw_state) = line.split_once(": ")?;
            let target = target.trim();
            if target.is_empty() {
                return None;
            }
            let raw_state = raw_state.trim();
            Some(OfficialIntegrationStatus {
                target: target.to_string(),
                raw_state: raw_state.to_string(),
                is_current: raw_state.starts_with("current"),
            })
        })
        .collect()
}

/// Explicit result of ensure. `Deferred` is the by-design "not this layer's
/// job" (a non-HerdrOfficial strategy) — neither a failure nor a silent
/// success.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnsureOutcome {
    /// Already current; nothing was modified (idempotent fast path).
    AlreadyCurrent,
    /// `herdr integration install` ran this time and the re-check confirmed
    /// current.
    Installed,
    /// Non-HerdrOfficial strategy: owned by the provider-native bridges of
    /// Batch B–D.
    Deferred,
}

/// Auditor/provisioner for the Herdr official integration.
///
/// The herdr CLI path is injectable (fake scripts for tests); production
/// uses [`Self::from_user_cli`], reusing `herdr_cli_path()`'s login-shell
/// PATH resolution so no second discovery logic is introduced.
#[derive(Clone, Debug)]
pub struct HerdrOfficialProvisioner {
    herdr_bin: PathBuf,
}

impl HerdrOfficialProvisioner {
    /// Builds from the user-level herdr CLI; returns None when the CLI is
    /// not discovered (the caller decides how to quiet that).
    pub fn from_user_cli() -> Option<Self> {
        herdr_cli_path().map(|herdr_bin| Self { herdr_bin })
    }

    /// Test injection point.
    pub fn with_binary(herdr_bin: PathBuf) -> Self {
        Self { herdr_bin }
    }

    /// `herdr integration status` snapshot (read-only).
    pub fn status(&self) -> Result<Vec<OfficialIntegrationStatus>, IntegrationError> {
        let output = Command::new(&self.herdr_bin)
            .args(["integration", "status"])
            .output()
            .map_err(|error| IntegrationError::Status(error.to_string()))?;
        if !output.status.success() {
            return Err(IntegrationError::Status(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(parse_integration_status(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    /// Idempotently ensures a Provider's Herdr integration is ready
    /// (plan §7.2/§7.4).
    ///
    /// - `HerdrOfficial`: status is current → `AlreadyCurrent` (no writes);
    ///   otherwise `herdr integration install <target>` and re-check to
    ///   converge as `Installed`.
    /// - Other strategies → `Deferred`: this layer never touches
    ///   third-party config.
    ///
    /// The caller is responsible for running this on a background thread
    /// (the GUI launch seam is already a background task); on failure the
    /// caller leaves a diagnostic trail in lag_log without blocking startup
    /// (plan §7.4 non-blocking requirement).
    pub fn ensure(
        &self,
        target_entry: &AgentIntegrationEntry,
    ) -> Result<EnsureOutcome, IntegrationError> {
        let Some(target) = target_entry.strategy.official_target() else {
            return Ok(EnsureOutcome::Deferred);
        };
        if self.is_current(target)? {
            return Ok(EnsureOutcome::AlreadyCurrent);
        }
        let output = Command::new(&self.herdr_bin)
            .args(["integration", "install", target])
            .output()
            .map_err(|error| IntegrationError::Install {
                target,
                message: error.to_string(),
            })?;
        if !output.status.success() {
            return Err(IntegrationError::Install {
                target,
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        if !self.is_current(target)? {
            return Err(IntegrationError::Install {
                target,
                message: "status did not report current after install".to_string(),
            });
        }
        Ok(EnsureOutcome::Installed)
    }

    fn is_current(&self, target: &str) -> Result<bool, IntegrationError> {
        Ok(self
            .status()?
            .into_iter()
            .find(|status| status.target == target)
            .is_some_and(|status| status.is_current))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_integrations::registry::{all_integrations, integration_for};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn entry_of(provider: shardlane_history::AgentId) -> AgentIntegrationEntry {
        integration_for(provider)
    }

    /// Fake herdr: status reads a state file in the same directory; install
    /// appends to a log and flips the target line to current. The script is
    /// self-contained (no state beyond env/argv), so it is safe for parallel
    /// tests; any unexpected subcommand writes to the log and exits 64 for
    /// the "CLI untouched" assertions.
    fn fake_herdr(
        dir: &TempDir,
        initial_state: &str,
        install_fails: bool,
    ) -> HerdrOfficialProvisioner {
        let bin = dir.path().join("herdr");
        let fail_line = if install_fails { "exit 1" } else { "true" };
        let script = format!(
            "#!/bin/sh\n\
             HERE=$(cd \"$(dirname \"$0\")\" && pwd)\n\
             STATE=\"$HERE/state\"\n\
             LOG=\"$HERE/log\"\n\
             case \"$1 $2\" in\n\
             \x20 \"integration status\") cat \"$STATE\" ;;\n\
             \x20 \"integration install\") echo \"$3\" >> \"$LOG\"; {fail_line}; \
             grep -v \"^$3:\" \"$STATE\" > \"$STATE.new\"; \
             echo \"$3: current (v9) (/fake)\" >> \"$STATE.new\"; \
             mv \"$STATE.new\" \"$STATE\" ;;\n\
             \x20 *) echo \"unexpected: $*\" >> \"$LOG\"; exit 64 ;;\n\
             esac\n"
        );
        fs::write(&bin, &script).unwrap_or_else(|error| panic!("write fake herdr: {error}"));
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|error| panic!("chmod fake herdr: {error}"));
        fs::write(dir.path().join("state"), initial_state)
            .unwrap_or_else(|error| panic!("write state: {error}"));
        HerdrOfficialProvisioner::with_binary(bin)
    }

    fn install_log(dir: &TempDir) -> Option<String> {
        fs::read_to_string(dir.path().join("log")).ok()
    }

    #[test]
    fn parse_reads_current_missing_and_unknown_states() {
        let parsed = parse_integration_status(
            "pi: current (v8) (/Users/x/.pi/agent/extensions/herdr-agent-state.ts)\n\
             antigravity-cli: current (v2) (/Users/x/.gemini/config/hooks/herdr-agent-state.sh)\n\
             kimi: not installed (/Users/x/.kimi-code/hooks/herdr-agent-state.sh)\n\
             \n\
             malformed line without colon\n",
        );
        assert_eq!(parsed.len(), 3);
        assert!(parsed[0].is_current);
        assert_eq!(parsed[0].target, "pi");
        assert_eq!(
            parsed[1].target, "antigravity-cli",
            "official targets with hyphens must not be truncated"
        );
        assert!(!parsed[2].is_current);
        assert_eq!(
            parsed[2].raw_state,
            "not installed (/Users/x/.kimi-code/hooks/herdr-agent-state.sh)"
        );
    }

    #[test]
    fn ensure_is_already_current_without_any_install() {
        let dir = TempDir::new().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let provisioner = fake_herdr(&dir, "pi: current (v8) (/x)\n", false);
        match provisioner.ensure(&entry_of(shardlane_history::AgentId::Pi)) {
            Ok(EnsureOutcome::AlreadyCurrent) => {}
            outcome => panic!("current integration should be AlreadyCurrent, got {outcome:?}"),
        }
        assert!(
            install_log(&dir).is_none(),
            "a current state must never trigger install"
        );
    }

    #[test]
    fn ensure_installs_once_then_stays_current() {
        let dir = TempDir::new().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let provisioner = fake_herdr(&dir, "kimi: not installed (/x)\n", false);
        match provisioner.ensure(&entry_of(shardlane_history::AgentId::Kimi)) {
            Ok(EnsureOutcome::Installed) => {}
            outcome => {
                panic!("an uninstalled official integration should be Installed, got {outcome:?}")
            }
        }
        let log = install_log(&dir).unwrap_or_default();
        assert_eq!(log.lines().count(), 1, "install should run exactly once");
        // Second ensure: already converged to current → idempotent no-op.
        match provisioner.ensure(&entry_of(shardlane_history::AgentId::Kimi)) {
            Ok(EnsureOutcome::AlreadyCurrent) => {}
            outcome => panic!("ensure after install should be AlreadyCurrent, got {outcome:?}"),
        }
        assert_eq!(
            install_log(&dir).unwrap_or_default().lines().count(),
            1,
            "idempotent re-entry must not install twice"
        );
    }

    #[test]
    fn ensure_reports_install_failure_with_target() {
        let dir = TempDir::new().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let provisioner = fake_herdr(&dir, "grok: not installed (/x)\n", true);
        match provisioner.ensure(&entry_of(shardlane_history::AgentId::Grok)) {
            Err(IntegrationError::Install { target, .. }) => assert_eq!(target, "grok"),
            outcome => panic!("install failure should return an Install error, got {outcome:?}"),
        }
    }

    #[test]
    fn ensure_defers_non_official_strategies_without_touching_the_cli() {
        let dir = TempDir::new().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let provisioner = fake_herdr(&dir, "", false);
        for provider in [
            shardlane_history::AgentId::Kiro,
            shardlane_history::AgentId::Gemini,
            shardlane_history::AgentId::CommandCode,
            shardlane_history::AgentId::Dsh,
        ] {
            match provisioner.ensure(&entry_of(provider)) {
                Ok(EnsureOutcome::Deferred) => {}
                outcome => panic!("{provider:?} should be explicitly Deferred, got {outcome:?}"),
            }
        }
        assert!(
            install_log(&dir).is_none(),
            "bridge strategies must not touch the herdr CLI before Batch B–D lands"
        );
    }

    #[test]
    fn status_fails_cleanly_when_cli_is_missing() {
        let provisioner = HerdrOfficialProvisioner::with_binary(PathBuf::from(
            "/nonexistent/herdr-for-agent-integrations-test",
        ));
        match provisioner.status() {
            Err(IntegrationError::Status(_)) => {}
            outcome => panic!("a missing CLI should return a Status error, got {outcome:?}"),
        }
    }

    #[test]
    #[ignore = "live evidence: depends on the real user machine's herdr CLI and user-level integration state (plan §10.3 precondition probe); not run in CI"]
    fn live_official_targets_resolve_and_current_ensure_is_noop() {
        let Some(provisioner) = HerdrOfficialProvisioner::from_user_cli() else {
            panic!("live smoke needs the user-level herdr CLI");
        };
        let statuses = match provisioner.status() {
            Ok(statuses) => statuses,
            Err(error) => panic!("herdr integration status failed: {error}"),
        };
        assert!(
            !statuses.is_empty(),
            "integration status produced no output"
        );
        for entry in all_integrations() {
            if let Some(target) = entry.strategy.official_target() {
                assert!(
                    statuses.iter().any(|status| status.target == target),
                    "the real CLI does not know registry target {target}"
                );
            }
        }
        // For any current official integration: ensure must be a no-op with
        // zero writes.
        let current = all_integrations().into_iter().find(|entry| {
            entry.strategy.official_target().is_some_and(|target| {
                statuses
                    .iter()
                    .any(|status| status.target == target && status.is_current)
            })
        });
        let Some(entry) = current else {
            return;
        };
        match provisioner.ensure(&entry) {
            Ok(EnsureOutcome::AlreadyCurrent) => {}
            outcome => panic!("ensure on a current integration should be a no-op, got {outcome:?}"),
        }
    }
}
