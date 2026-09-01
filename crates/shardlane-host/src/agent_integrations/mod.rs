//! [INPUT]: depends on shardlane-history (the AgentId::ALL exhaustive
//! invariant) and crate::herdr (herdr_cli_path reuse; a second CLI discovery
//! path is forbidden).
//! [OUTPUT]: the install/health layer for Agent Provider → Herdr
//! integration: the exhaustive strategy registry (registry), the unified
//! provisioning entry point `ensure_provider` (dispatching by strategy:
//! HerdrOfficial → `herdr integration install`; CommandCode managed Mod →
//! providers::command_code; remaining bridges → Deferred), idempotent
//! ensure/audit for the Herdr official integration (herdr_official), atomic
//! managed-file primitives (managed_file), and read-only IntegrationHealth
//! (health).
//! [POS]: landing spot for the Agent-integration coverage batches.
//! Responsibilities are install/health only, **not a second Agent runtime
//! state**: working/idle/blocked always remain Herdr's data, and the runtime
//! projection keeps going exclusively through crate::herdr's agent.list and
//! events (plan §7/§8); consumed by the GUI launch seam
//! (new_agent/launch, history/resume).

pub mod health;
pub mod herdr_official;
pub mod managed_file;
pub mod providers;
pub mod registry;

pub use health::*;
pub use herdr_official::*;
pub use managed_file::*;
pub use registry::*;

use shardlane_history::AgentId;

/// Unified provisioning entry point (plan §7.2/§7.4): dispatches by registry
/// strategy, idempotent.
///
/// - `HerdrOfficial`: `herdr integration install <target>` (current → no
///   writes).
/// - `ManagedLifecycleBridge`: currently only CommandCode is implemented
///   (Batch C, managed Mod install); DSH waits for Batch D.
/// - Remaining strategies: `Deferred` (Batch B/D) — explicitly not done
///   rather than silently succeeding.
pub fn ensure_provider(provider: AgentId) -> Result<EnsureOutcome, IntegrationError> {
    let entry = integration_for(provider);
    match entry.strategy {
        AgentIntegrationStrategy::HerdrOfficial { .. } => {
            let provisioner =
                HerdrOfficialProvisioner::from_user_cli().ok_or(IntegrationError::CliMissing)?;
            provisioner.ensure(&entry)
        }
        AgentIntegrationStrategy::ManagedLifecycleBridge => match provider {
            AgentId::CommandCode => {
                providers::command_code::ensure_mod_installed().map_err(|error| {
                    IntegrationError::ManagedAsset {
                        provider: entry.herdr_label,
                        message: error.to_string(),
                    }
                })
            }
            AgentId::Dsh => Ok(EnsureOutcome::Deferred),
            _ => Ok(EnsureOutcome::Deferred),
        },
        AgentIntegrationStrategy::Deferred => Ok(EnsureOutcome::Deferred),
        _ => Ok(EnsureOutcome::Deferred),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ensure_provider_defers_batch_b_and_d_strategies_without_any_side_effect() {
        for provider in [AgentId::Kiro, AgentId::Gemini, AgentId::Dsh] {
            assert_eq!(
                ensure_provider(provider).unwrap_or_else(|error| panic!("defer: {error}")),
                EnsureOutcome::Deferred,
                "{provider:?} bridge must be explicitly Deferred until implemented"
            );
        }
    }

    #[test]
    fn ensure_provider_installs_command_code_managed_mod_idempotently() {
        let dir = TempDir::new().unwrap_or_else(|error| panic!("tempdir: {error}"));
        // Exercise the dispatch target's implementation directly (HOME is a
        // process global; the test uses an injectable path).
        let dest = dir
            .path()
            .join(providers::command_code::MANAGED_MOD_FILE_NAME);
        assert_eq!(
            providers::command_code::ensure_mod_installed_at(&dest)
                .unwrap_or_else(|error| panic!("install: {error}")),
            EnsureOutcome::Installed
        );
        assert_eq!(
            providers::command_code::ensure_mod_installed_at(&dest)
                .unwrap_or_else(|error| panic!("reinstall: {error}")),
            EnsureOutcome::AlreadyCurrent
        );
        assert!(
            dest.is_file(),
            "the managed Mod must land inside the mods directory"
        );
    }
}
