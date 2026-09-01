//! [INPUT]: depends on super::registry (the strategy registry) and
//! super::herdr_official (status audit).
//! [OUTPUT]: exposes read-only `IntegrationHealth` plus
//! `audit_integrations(_with)`.
//! [POS]: the diagnostics surface of plan §14: describes install/health only
//! (cli_present / installed / current / version / path), **never carries**
//! working/idle/blocked — those are Herdr runtime data; to diagnose the live
//! state use `herdr agent list` / `herdr agent explain`.

use super::herdr_official::{HerdrOfficialProvisioner, OfficialIntegrationStatus};
use super::registry::{all_integrations, AgentIntegrationEntry, AgentIntegrationStrategy};
use shardlane_history::AgentId;

/// Read-only integration health snapshot for a single Provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntegrationHealth {
    pub provider: AgentId,
    pub herdr_label: &'static str,
    pub strategy: AgentIntegrationStrategy,
    /// Whether the user-level herdr CLI is discoverable (without a CLI the
    /// official targets cannot be audited).
    pub herdr_cli_present: bool,
    /// Populated only for the HerdrOfficial strategy; bridge strategies stay
    /// None until Batch B–D lands.
    pub official: Option<OfficialIntegrationStatus>,
}

fn audit_entry(entry: &AgentIntegrationEntry, health: &IntegrationAudit) -> IntegrationHealth {
    let official = entry
        .strategy
        .official_target()
        .and_then(|target| {
            health
                .statuses
                .iter()
                .find(|status| status.target == target)
        })
        .cloned();
    IntegrationHealth {
        provider: entry.provider,
        herdr_label: entry.herdr_label,
        strategy: entry.strategy,
        herdr_cli_present: health.cli_present,
        official,
    }
}

struct IntegrationAudit {
    cli_present: bool,
    statuses: Vec<OfficialIntegrationStatus>,
}

/// Audits integration health for all 15 current Providers (read-only: at
/// most one `herdr integration status`, never install). When the CLI is
/// missing or status fails, the official fields are empty; no panic.
pub fn audit_integrations_with(
    provisioner: Option<&HerdrOfficialProvisioner>,
) -> Vec<IntegrationHealth> {
    let audit = match provisioner {
        Some(provisioner) => IntegrationAudit {
            cli_present: true,
            statuses: provisioner.status().unwrap_or_default(),
        },
        None => IntegrationAudit {
            cli_present: false,
            statuses: Vec::new(),
        },
    };
    all_integrations()
        .iter()
        .map(|entry| audit_entry(entry, &audit))
        .collect()
}

/// Convenience entry point: audits using the user-level herdr CLI.
pub fn audit_integrations() -> Vec<IntegrationHealth> {
    audit_integrations_with(HerdrOfficialProvisioner::from_user_cli().as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_integrations::herdr_official::parse_integration_status;
    use crate::agent_integrations::registry::integration_for;
    use shardlane_history::AgentId;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn fake_herdr(dir: &TempDir, status_text: &str) -> HerdrOfficialProvisioner {
        let bin = dir.path().join("herdr");
        let script = format!(
            "#!/bin/sh\nprintf '%s' '{}'\n",
            status_text.replace('\'', "'\\''")
        );
        fs::write(&bin, &script).unwrap_or_else(|error| panic!("write fake herdr: {error}"));
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|error| panic!("chmod fake herdr: {error}"));
        HerdrOfficialProvisioner::with_binary(bin)
    }

    #[test]
    fn audit_reports_official_state_and_leaves_bridges_unprovisioned() {
        let dir = TempDir::new().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let provisioner = fake_herdr(&dir, "kimi: current (v1) (/x)\npi: current (v8) (/y)\n");
        let health = audit_integrations_with(Some(&provisioner));
        assert_eq!(health.len(), AgentId::ALL.len());

        let kimi = health
            .iter()
            .find(|h| h.provider == AgentId::Kimi)
            .unwrap_or_else(|| panic!("kimi health missing"));
        let kimi_status = kimi
            .official
            .as_ref()
            .unwrap_or_else(|| panic!("official integration should have a status"));
        assert!(kimi_status.is_current);

        let kiro = health
            .iter()
            .find(|h| h.provider == AgentId::Kiro)
            .unwrap_or_else(|| panic!("kiro health missing"));
        assert!(
            kiro.official.is_none(),
            "session bridge strategy must not fake official status before Batch B lands"
        );
        assert!(kiro.herdr_cli_present);
    }

    #[test]
    fn audit_without_cli_marks_cli_missing() {
        let health = audit_integrations_with(None);
        assert_eq!(health.len(), AgentId::ALL.len());
        for entry in &health {
            assert!(!entry.herdr_cli_present);
            assert!(entry.official.is_none());
        }
        // The registry entry still agrees with integration_for.
        assert_eq!(
            health[0].provider,
            integration_for(AgentId::ClaudeCode).provider
        );
    }

    #[test]
    fn audit_survives_status_failure() {
        let dir = TempDir::new().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let script = "#!/bin/sh\necho boom >&2\nexit 3\n";
        let bin = dir.path().join("herdr");
        fs::write(&bin, script).unwrap_or_else(|error| panic!("write: {error}"));
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|error| panic!("chmod: {error}"));
        let provisioner = HerdrOfficialProvisioner::with_binary(bin);
        let health = audit_integrations_with(Some(&provisioner));
        assert_eq!(health.len(), AgentId::ALL.len());
        assert!(health.iter().all(|entry| entry.official.is_none()));
        assert!(parse_integration_status("").is_empty());
    }
}
