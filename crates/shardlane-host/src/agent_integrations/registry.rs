//! [INPUT]: depends on shardlane_history::AgentId (the single source of the
//! Provider set).
//! [OUTPUT]: exposes `AgentIntegrationStrategy` / `AgentIntegrationEntry` and
//! the exhaustive registry (`integration_for` / `all_integrations` /
//! `official_target`).
//! [POS]: the invariant carrier for integration coverage: every `AgentId::ALL` Provider gets exactly one explicit
//! strategy and the match has no wildcard — adding a new AgentId variant
//! fails to compile here, the strongest form of "registry coverage tests
//! must fail". This module is pure data and performs no IO.

use shardlane_history::AgentId;

use AgentIntegrationStrategy::{
    Deferred, HerdrOfficial, HerdrScreenWithManagedSessionBridge, ManagedLifecycleBridge,
};

/// The Herdr integration strategy per Provider (plan §7.3).
///
/// Variants deliberately carry no provider payload: the registry is keyed by
/// provider, and the Herdr label plus official install target are carried
/// separately by [`AgentIntegrationEntry`], avoiding two stores for one fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentIntegrationStrategy {
    /// The Herdr official integration is the sole runtime authority;
    /// Shardlane only ensures `herdr integration install <target>` is
    /// current (no forking the official hooks, no parallel second install).
    /// Once the official lifecycle authority takes effect, Herdr no longer
    /// runs the screen fallback for the same authority (plan §2.1).
    HerdrOfficial { target: &'static str },
    /// Lifecycle is handed entirely to the Herdr screen manifest; no
    /// session/lifecycle bridge.
    HerdrScreenOnly,
    /// Lifecycle stays with the Herdr screen authority; Shardlane only adds
    /// a provider-native session identity bridge
    /// (`pane.report_agent_session`) and never report-agent — otherwise it
    /// would displace the more complete screen fallback (plan §2.2/§5.2).
    HerdrScreenWithManagedSessionBridge,
    /// Shardlane maintains the provider-native lifecycle bridge (the
    /// `pane.report_agent` family + `pane.release_agent`); Herdr remains the
    /// sole state authority. Only for Providers Herdr does not yet natively
    /// recognize (currently: Command Code, DeepSeek Harness).
    ManagedLifecycleBridge,
    /// Runtime integration is deliberately not claimed yet. History may be
    /// available, but hooks/screen identity are not verified enough to expose
    /// a lifecycle or session bridge.
    Deferred,
}

impl AgentIntegrationStrategy {
    /// The install target for [`HerdrOfficial`](Self::HerdrOfficial); None
    /// for the other strategies.
    pub fn official_target(self) -> Option<&'static str> {
        match self {
            Self::HerdrOfficial { target } => Some(target),
            _ => None,
        }
    }
}

/// The registry entry for a single Provider.
///
/// `herdr_label` is the Herdr protocol short id (from the `agent.list` /
/// `server.agent_manifests` value domain, e.g. `claude`/`agy`); the mapping
/// to the GUI's `agent_cli::herdr_agent_id` is pinned consistent by
/// cross-tests on the GUI side (the history slugs
/// `claude-code`/`antigravity` are not protocol values).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentIntegrationEntry {
    pub provider: AgentId,
    pub herdr_label: &'static str,
    pub strategy: AgentIntegrationStrategy,
}

/// The single registry entry point for all Providers. Exhaustiveness is
/// guaranteed by the wildcard-free match; adding a Provider requires picking
/// a strategy explicitly here before it compiles.
pub fn integration_for(provider: AgentId) -> AgentIntegrationEntry {
    match provider {
        AgentId::ClaudeCode => entry(provider, "claude", HerdrOfficial { target: "claude" }),
        AgentId::Codex => entry(provider, "codex", HerdrOfficial { target: "codex" }),
        AgentId::Copilot => entry(provider, "copilot", HerdrOfficial { target: "copilot" }),
        AgentId::Cursor => entry(provider, "cursor", HerdrOfficial { target: "cursor" }),
        AgentId::Opencode => entry(provider, "opencode", HerdrOfficial { target: "opencode" }),
        // Herdr does not yet natively recognize these two Providers
        // (plan §2.4): the official install target list has no
        // commandcode/dsh, so they need provider-native lifecycle bridges
        // (Batch C/D).
        AgentId::CommandCode => entry(provider, "commandcode", ManagedLifecycleBridge),
        // Kiro/Gemini already have a Herdr screen manifest (plan §2.3): add
        // session identity only, do not seize lifecycle authority (Batch B).
        AgentId::Kiro => entry(provider, "kiro", HerdrScreenWithManagedSessionBridge),
        AgentId::Gemini => entry(provider, "gemini", HerdrScreenWithManagedSessionBridge),
        AgentId::Pi => entry(provider, "pi", HerdrOfficial { target: "pi" }),
        AgentId::Omp => entry(provider, "omp", HerdrOfficial { target: "omp" }),
        AgentId::Grok => entry(provider, "grok", HerdrOfficial { target: "grok" }),
        AgentId::Kimi => entry(provider, "kimi", HerdrOfficial { target: "kimi" }),
        // The history slug is antigravity, the protocol short id is agy, and
        // the official target is antigravity-cli.
        AgentId::Antigravity => entry(
            provider,
            "agy",
            HerdrOfficial {
                target: "antigravity-cli",
            },
        ),
        AgentId::Dsh => entry(provider, "dsh", ManagedLifecycleBridge),
        // Qoder History is supported in Shardlane, but its hooks have not yet
        // been safely merged into Herdr; do not invent a second runtime path.
        AgentId::Qoder => entry(provider, "qoder", Deferred),
    }
}

fn entry(
    provider: AgentId,
    herdr_label: &'static str,
    strategy: AgentIntegrationStrategy,
) -> AgentIntegrationEntry {
    AgentIntegrationEntry {
        provider,
        herdr_label,
        strategy,
    }
}

/// Full registry snapshot (order = `AgentId::ALL` order).
pub fn all_integrations() -> Vec<AgentIntegrationEntry> {
    AgentId::ALL.iter().copied().map(integration_for).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_covers_agent_id_all_exactly() {
        let all = all_integrations();
        assert_eq!(all.len(), AgentId::ALL.len());
        for (entry, provider) in all.iter().zip(AgentId::ALL.iter()) {
            assert_eq!(entry.provider, *provider, "provider drift out of order");
        }
        let seen: HashSet<AgentId> = all.iter().map(|e| e.provider).collect();
        assert_eq!(seen.len(), AgentId::ALL.len(), "providers must not repeat");
    }

    #[test]
    fn official_targets_are_the_documented_ten() {
        let mut targets: Vec<&'static str> = all_integrations()
            .into_iter()
            .filter_map(|e| e.strategy.official_target())
            .collect();
        targets.sort_unstable();
        assert_eq!(
            targets,
            [
                "antigravity-cli",
                "claude",
                "codex",
                "copilot",
                "cursor",
                "grok",
                "kimi",
                "omp",
                "opencode",
                "pi"
            ]
        );
    }

    #[test]
    fn kiro_and_gemini_are_session_only_and_cannot_take_lifecycle_authority() {
        for provider in [AgentId::Kiro, AgentId::Gemini] {
            assert_eq!(
                integration_for(provider).strategy,
                AgentIntegrationStrategy::HerdrScreenWithManagedSessionBridge,
                "{provider:?} must keep screen authority + session-only bridge"
            );
        }
    }

    #[test]
    fn managed_lifecycle_bridges_are_command_code_and_dsh_only() {
        let bridges: Vec<AgentId> = all_integrations()
            .into_iter()
            .filter(|e| e.strategy == AgentIntegrationStrategy::ManagedLifecycleBridge)
            .map(|e| e.provider)
            .collect();
        assert_eq!(bridges, vec![AgentId::CommandCode, AgentId::Dsh]);
    }

    #[test]
    fn qoder_runtime_integration_is_explicitly_deferred() {
        assert_eq!(
            integration_for(AgentId::Qoder).strategy,
            AgentIntegrationStrategy::Deferred
        );
    }

    #[test]
    fn herdr_labels_are_protocol_short_ids() {
        for entry in all_integrations() {
            assert!(!entry.herdr_label.is_empty());
            assert!(
                !entry.herdr_label.contains('-'),
                "{} is not a herdr protocol short id (history slugs must not leak in)",
                entry.herdr_label
            );
        }
    }
}
