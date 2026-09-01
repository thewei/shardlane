//! One Host-derived Provider product-capability projection.
//!
//! [INPUT]: the shardlane-history provider registry (the sole authority for
//! exposure/live capabilities) with `resume_supported` (native resume
//! semantics), the `agent_integrations` registry (runtime integration
//! strategy), and caller-supplied user/environment facts (enabled, CLI
//! installed).
//! [OUTPUT]: `ProviderProductCapabilities` — the single product capability
//! row shared by New Agent / History Continue / Handoff / Settings; contains
//! no capability-decision logic itself.
//! [POS]: audit AF-11. This module only composes existing authorities and
//! does not duplicate lists; GUI/Remote must not maintain their own provider
//! filters. The lossless transfer field stays false until the M5 engine is
//! verified.

use crate::agent_integrations::{integration_for, AgentIntegrationStrategy};
use serde::{Deserialize, Serialize};
use shardlane_history::models::AgentId;
use shardlane_history::{resume_supported, LiveCapability, ProviderExposure};

/// Caller-supplied user/environment facts. The Host does not own GUI settings or
/// CLI discovery inputs; callers compose them into the projection through this
/// struct instead of each consumer re-deriving its own filter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProviderEnvironment {
    /// User enabled the provider in product settings.
    pub enabled: bool,
    /// The provider CLI executable was discovered and validated on this host.
    pub installed: bool,
}

/// Product exposure level, mirrored from the shardlane-history registry authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProductExposure {
    Stable,
    Preview,
    Hidden,
}

/// How Plan mode is expressed for one provider at launch.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderPlanSupport {
    /// Plan mode selected at launch through provider CLI flags.
    NativeCli,
    /// Plan mode entered after launch through verified keys.
    PostLaunchKeys,
    /// Plan mode requested through the initial prompt text only.
    PromptPrefix,
}

/// v1 semantic prompt behavior while an Agent is Working. Mid-turn immediate
/// delivery stays disabled until a provider passes repeated runtime verification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingPromptMode {
    /// Exactly one queued follow-up delivered after the current turn settles.
    QueueAfterTurn,
}

/// Provider bridge transport kind for live semantic events / companion interaction.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderBridgeKind {
    #[default]
    None,
    CommandHook,
    Extension,
    Plugin,
    SameSessionProtocol,
}

/// Provider semantic event observation capability.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionObserveCapability {
    #[default]
    None,
    TranscriptOnly,
    HookEvents,
    RichHookEvents,
}

/// Provider in-turn interaction response capability.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionRespondCapability {
    #[default]
    None,
    PermissionOnly,
    QuestionAndPermission,
}

/// One provider's product capabilities, composed — never re-derived — from the
/// lower-level authorities. A row must never be presented as selectable when the
/// requested operation cannot complete; `unavailable_reason` explains why not.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderProductCapabilities {
    pub provider: String,
    pub display_name: String,
    pub exposure: ProviderProductExposure,
    pub enabled: bool,
    pub installed: bool,
    pub startable: bool,
    pub semantic_live: bool,
    pub native_resume: bool,
    pub lossless_transfer_source: bool,
    pub lossless_transfer_target: bool,
    /// P0-08: live (in-flight) sources can be handed off, but the transfer is
    /// `verified-up-to-last-flush`, NOT proven-lossless: providers do not yet
    /// expose a completed-turn watermark. Product copy must not claim
    /// "full context" for live handoff while this flag is false.
    pub live_handoff_verified_lossless: bool,
    pub plan_mode: ProviderPlanSupport,
    pub permission_modes: bool,
    pub working_prompt_mode: WorkingPromptMode,
    pub bridge_kind: ProviderBridgeKind,
    pub interaction_observe: InteractionObserveCapability,
    pub interaction_respond: InteractionRespondCapability,
    pub unavailable_reason: Option<String>,
}

/// Whether the provider's runtime integration strategy can start an Agent at all.
/// `Deferred` is an explicit "not claimed yet" marker, not a degraded fallback.
fn integration_startable(agent: AgentId) -> bool {
    !matches!(
        integration_for(agent).strategy,
        AgentIntegrationStrategy::Deferred
    )
}

/// Plan-mode expression per provider. This is the single authority for the table
/// previously hard-coded in the GUI launch path; consumers must not fork it.
pub fn provider_plan_support(agent: AgentId) -> ProviderPlanSupport {
    match agent {
        AgentId::ClaudeCode | AgentId::Gemini => ProviderPlanSupport::NativeCli,
        AgentId::Codex => ProviderPlanSupport::PostLaunchKeys,
        _ => ProviderPlanSupport::PromptPrefix,
    }
}

pub fn provider_permission_modes(agent: AgentId) -> bool {
    matches!(
        agent,
        AgentId::ClaudeCode | AgentId::Codex | AgentId::Gemini
    )
}

pub fn provider_bridge_kind(agent: AgentId) -> ProviderBridgeKind {
    match agent {
        AgentId::ClaudeCode
        | AgentId::Codex
        | AgentId::Cursor
        | AgentId::Copilot
        | AgentId::Gemini
        | AgentId::Antigravity => ProviderBridgeKind::CommandHook,
        AgentId::Pi | AgentId::Omp => ProviderBridgeKind::Extension,
        AgentId::Opencode => ProviderBridgeKind::Plugin,
        _ => ProviderBridgeKind::None,
    }
}

pub fn provider_interaction_observe(agent: AgentId) -> InteractionObserveCapability {
    let registry = shardlane_history::provider_capabilities(agent);
    if registry.is_some_and(|entry| entry.live != LiveCapability::None) {
        InteractionObserveCapability::TranscriptOnly
    } else {
        InteractionObserveCapability::None
    }
}

// C28: `provider_interaction_respond(_agent)` was deleted — it ignored its
// input and always returned `None`; no provider passes interaction-response
// verification yet, so the capability is folded into the projection default
// below (serialized value unchanged).

fn unavailable_reason(
    agent: AgentId,
    exposure: ProviderProductExposure,
    environment: ProviderEnvironment,
    startable: bool,
) -> Option<String> {
    if exposure == ProviderProductExposure::Hidden {
        return Some(format!(
            "{} is not available in this build",
            agent.display_name()
        ));
    }
    if !environment.enabled {
        return Some(format!("{} is disabled in Settings", agent.display_name()));
    }
    if !environment.installed {
        return Some("Setup required".to_string());
    }
    if !startable {
        return Some(format!(
            "{} runtime integration is pending",
            agent.display_name()
        ));
    }
    None
}

/// Project one provider's product capabilities from its authorities plus the
/// caller-supplied environment facts.
pub fn provider_product_capabilities(
    agent: AgentId,
    environment: ProviderEnvironment,
) -> ProviderProductCapabilities {
    let registry = shardlane_history::provider_capabilities(agent);
    let exposure = match registry.map(|entry| entry.exposure) {
        Some(ProviderExposure::Stable) => ProviderProductExposure::Stable,
        Some(ProviderExposure::Preview) => ProviderProductExposure::Preview,
        _ => ProviderProductExposure::Hidden,
    };
    let semantic_live = registry.is_some_and(|entry| entry.live != LiveCapability::None);
    let startable = environment.enabled && environment.installed && integration_startable(agent);
    ProviderProductCapabilities {
        provider: agent.as_str().to_string(),
        display_name: agent.display_name().to_string(),
        exposure,
        enabled: environment.enabled,
        installed: environment.installed,
        startable,
        semantic_live,
        native_resume: resume_supported(agent),
        // P0-08: "lossless transfer source" is only PROVEN for closed
        // (history) sessions, where the file IS the complete conversation.
        // A LIVE source may still be missing its final completed turn at
        // handoff time (the completed-turn watermark does not exist yet), so
        // live handoff must not be advertised as lossless/full-context.
        // Exact pairs still fail closed at runtime.
        lossless_transfer_source: true,
        lossless_transfer_target: startable,
        live_handoff_verified_lossless: false,
        plan_mode: provider_plan_support(agent),
        permission_modes: provider_permission_modes(agent),
        working_prompt_mode: WorkingPromptMode::QueueAfterTurn,
        bridge_kind: provider_bridge_kind(agent),
        interaction_observe: provider_interaction_observe(agent),
        // C28: no provider has a verified in-turn interaction responder yet;
        // this is the constant the deleted input-ignoring function returned.
        interaction_respond: InteractionRespondCapability::None,
        unavailable_reason: unavailable_reason(agent, exposure, environment, startable),
    }
}

/// Snapshot every provider's product capabilities. `environment` supplies the
/// per-provider user/CLI facts the Host does not own.
pub fn all_provider_product_capabilities(
    environment: impl Fn(AgentId) -> ProviderEnvironment,
) -> Vec<ProviderProductCapabilities> {
    AgentId::ALL
        .iter()
        .copied()
        .map(|agent| provider_product_capabilities(agent, environment(agent)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shardlane_history::exposed_agents;

    fn ready(_agent: AgentId) -> ProviderEnvironment {
        ProviderEnvironment {
            enabled: true,
            installed: true,
        }
    }

    #[test]
    fn projection_composes_lower_level_authorities() {
        let claude = provider_product_capabilities(AgentId::ClaudeCode, ready(AgentId::ClaudeCode));
        assert_eq!(claude.exposure, ProviderProductExposure::Stable);
        assert!(
            claude.semantic_live,
            "registry live authority must flow through"
        );
        assert!(claude.native_resume, "resume_supported must flow through");
        assert!(
            claude.startable,
            "installed + official integration is startable"
        );
        assert_eq!(claude.plan_mode, ProviderPlanSupport::NativeCli);
        assert!(claude.permission_modes);
        assert_eq!(claude.unavailable_reason, None);
        assert_eq!(
            claude.working_prompt_mode,
            WorkingPromptMode::QueueAfterTurn
        );

        let codex = provider_product_capabilities(AgentId::Codex, ready(AgentId::Codex));
        assert_eq!(codex.plan_mode, ProviderPlanSupport::PostLaunchKeys);

        let pi = provider_product_capabilities(AgentId::Pi, ready(AgentId::Pi));
        assert_eq!(pi.plan_mode, ProviderPlanSupport::PromptPrefix);
        assert!(!pi.permission_modes, "Pi has no permission flags");
    }

    #[test]
    fn unavailable_providers_carry_a_reason() {
        let disabled = provider_product_capabilities(
            AgentId::Codex,
            ProviderEnvironment {
                enabled: false,
                installed: true,
            },
        );
        assert!(
            !disabled.startable,
            "disabled providers cannot start agents"
        );
        assert!(disabled.unavailable_reason.is_some());
        assert!(disabled
            .unavailable_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("disabled")));

        let missing_cli = provider_product_capabilities(
            AgentId::Cursor,
            ProviderEnvironment {
                enabled: true,
                installed: false,
            },
        );
        assert!(!missing_cli.startable);
        assert_eq!(
            missing_cli.unavailable_reason.as_deref(),
            Some("Setup required")
        );

        let hidden = provider_product_capabilities(
            AgentId::Kimi,
            ProviderEnvironment {
                enabled: true,
                installed: true,
            },
        );
        assert_eq!(hidden.exposure, ProviderProductExposure::Hidden);
        assert!(hidden.unavailable_reason.is_some());
    }

    #[test]
    fn deferred_integration_is_not_startable_even_when_installed() {
        let qoder = provider_product_capabilities(AgentId::Qoder, ready(AgentId::Qoder));
        assert!(!qoder.startable, "Deferred strategy must fail closed");
        assert!(qoder
            .unavailable_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("pending")));
    }

    #[test]
    fn lossless_transfer_follows_the_verified_engine() {
        for agent in AgentId::ALL {
            let caps = provider_product_capabilities(agent, ready(agent));
            assert!(caps.lossless_transfer_source);
            assert_eq!(caps.lossless_transfer_target, caps.startable);
        }
        // Non-startable targets are not transfer targets.
        let qoder = provider_product_capabilities(AgentId::Qoder, ready(AgentId::Qoder));
        assert!(!qoder.lossless_transfer_target);
    }

    #[test]
    fn exposure_projection_matches_the_history_registry() {
        let snapshot = all_provider_product_capabilities(|_| ProviderEnvironment {
            enabled: true,
            installed: true,
        });
        assert_eq!(snapshot.len(), AgentId::ALL.len());
        let exposed: Vec<AgentId> = snapshot
            .iter()
            .filter(|caps| caps.exposure != ProviderProductExposure::Hidden)
            .filter_map(|caps| AgentId::from_slug(&caps.provider))
            .collect();
        assert_eq!(
            exposed,
            exposed_agents(),
            "exposure must stay registry-derived"
        );
    }

    #[test]
    fn startable_requires_both_installation_and_integration() {
        let uninstalled = provider_product_capabilities(
            AgentId::ClaudeCode,
            ProviderEnvironment {
                enabled: true,
                installed: false,
            },
        );
        assert!(!uninstalled.startable);
    }
}
