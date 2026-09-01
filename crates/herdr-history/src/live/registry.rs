// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

//! Provider capability registry: the sole authority for aliases and live transport capability,
//! living in the History/Agent domain rather than the render layer.
//! Wave 1 (PEX-3): Kimi (`agents/main/wire.jsonl`), Cursor
//! (`agent-transcripts/<id>.jsonl`), and Command Code
//! (`projects/<slug>/<id>.jsonl`, lineage-aware) have AppendLog live enabled.
//!
//! [INPUT]: models::AgentId; zero I/O, zero GUI.
//! [OUTPUT]: The PROVIDERS capability table, resolve_agent (alias
//! resolution), capabilities, live_capable_agents.
//! [POS]: Consolidates provider knowledge previously scattered across Chat
//! normalize_provider / LiveDecoder::supports / user-facing copy. This
//! registry does not own process launching — that remains Herdr's authority.
//! The planned fidelity/interaction/protocol_status fields will be added when
//! any provider has authoritative data (§10: a concept is created only when
//! data exists and product behavior exists).

use crate::models::AgentId;

/// Live semantic-source transport capability (basis for PEX-2 transport
/// policy selection).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveCapability {
    /// append-only JSONL tailing (AppendLogTransport; Claude/Codex/Pi/Omp).
    AppendLog,
    /// No live semantic source currently available (History-only /
    /// metadata-only).
    None,
}

/// Product exposure tier (R4 / audit CS-08): technical implementation ≠
/// product readiness. Hidden = code retained but excluded from every
/// user-visible list (Settings/New Agent/Continue-as/Chat).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderExposure {
    Stable,
    Preview,
    Hidden,
}

/// Capability declaration for a single provider.
#[derive(Clone, Copy, Debug)]
pub struct ProviderCapabilities {
    pub agent: AgentId,
    /// Provider-normalized aliases (lowercase comparison; from the existing
    /// resume/live matching semantics).
    pub aliases: &'static [&'static str],
    pub live: LiveCapability,
    /// Product exposure tier (one of the user-visibility gates; user
    /// enablement lives in the config layer).
    pub exposure: ProviderExposure,
}

const CLAUDE_ALIASES: &[&str] = &["claude", "claude-code", "claude_code"];
const CODEX_ALIASES: &[&str] = &["codex", "codex-cli"];
const PI_ALIASES: &[&str] = &["pi", "pi-coding-agent"];
const OMP_ALIASES: &[&str] = &["omp", "oh-my-pi"];
const KIMI_ALIASES: &[&str] = &["kimi"];
const CURSOR_ALIASES: &[&str] = &["cursor"];
const COMMAND_CODE_ALIASES: &[&str] = &["command-code", "commandcode"];
const QODER_ALIASES: &[&str] = &["qoder", "qoder-cli"];

/// Capability table for all providers (sole authority; consumers derive from
/// it rather than maintaining their own lists).
pub const PROVIDERS: &[ProviderCapabilities] = &[
    ProviderCapabilities {
        agent: AgentId::ClaudeCode,
        aliases: CLAUDE_ALIASES,
        live: LiveCapability::AppendLog,
        exposure: ProviderExposure::Stable,
    },
    ProviderCapabilities {
        agent: AgentId::Codex,
        aliases: CODEX_ALIASES,
        live: LiveCapability::AppendLog,
        exposure: ProviderExposure::Stable,
    },
    ProviderCapabilities {
        agent: AgentId::Copilot,
        aliases: &[],
        live: LiveCapability::None,
        exposure: ProviderExposure::Hidden,
    },
    ProviderCapabilities {
        agent: AgentId::Cursor,
        aliases: CURSOR_ALIASES,
        live: LiveCapability::AppendLog,
        exposure: ProviderExposure::Preview,
    },
    ProviderCapabilities {
        agent: AgentId::Opencode,
        aliases: &[],
        live: LiveCapability::None,
        exposure: ProviderExposure::Hidden,
    },
    ProviderCapabilities {
        agent: AgentId::CommandCode,
        aliases: COMMAND_CODE_ALIASES,
        live: LiveCapability::AppendLog,
        exposure: ProviderExposure::Preview,
    },
    ProviderCapabilities {
        agent: AgentId::Kiro,
        aliases: &[],
        live: LiveCapability::None,
        exposure: ProviderExposure::Hidden,
    },
    ProviderCapabilities {
        agent: AgentId::Gemini,
        aliases: &[],
        live: LiveCapability::None,
        exposure: ProviderExposure::Hidden,
    },
    ProviderCapabilities {
        agent: AgentId::Pi,
        aliases: PI_ALIASES,
        live: LiveCapability::AppendLog,
        exposure: ProviderExposure::Stable,
    },
    ProviderCapabilities {
        agent: AgentId::Omp,
        aliases: OMP_ALIASES,
        live: LiveCapability::AppendLog,
        exposure: ProviderExposure::Stable,
    },
    ProviderCapabilities {
        agent: AgentId::Grok,
        aliases: &[],
        live: LiveCapability::None,
        exposure: ProviderExposure::Hidden,
    },
    ProviderCapabilities {
        agent: AgentId::Kimi,
        aliases: KIMI_ALIASES,
        live: LiveCapability::AppendLog,
        exposure: ProviderExposure::Hidden,
    },
    ProviderCapabilities {
        agent: AgentId::Antigravity,
        aliases: &[],
        live: LiveCapability::None,
        exposure: ProviderExposure::Hidden,
    },
    ProviderCapabilities {
        agent: AgentId::Dsh,
        aliases: &[],
        live: LiveCapability::None,
        exposure: ProviderExposure::Hidden,
    },
    ProviderCapabilities {
        agent: AgentId::Qoder,
        aliases: QODER_ALIASES,
        live: LiveCapability::None,
        exposure: ProviderExposure::Preview,
    },
];

/// Normalize by the provider-declared aliases: `primary` is Herdr's
/// `agent_session.agent`, `secondary` is the Agent projection's `agent`
/// field; a hit on either returns.
pub fn resolve_agent_alias(primary: &str, secondary: Option<&str>) -> Option<AgentId> {
    let candidates = [Some(primary.to_string()), secondary.map(str::to_string)];
    for candidate in candidates.into_iter().flatten() {
        let lowered = candidate.to_lowercase();
        for entry in PROVIDERS {
            if entry.aliases.iter().any(|alias| *alias == lowered) {
                return Some(entry.agent);
            }
        }
    }
    None
}

/// Capability declaration for a single provider (returns None for
/// unregistered agents).
pub fn capabilities(agent: AgentId) -> Option<&'static ProviderCapabilities> {
    PROVIDERS.iter().find(|entry| entry.agent == agent)
}

/// Whether the provider is product-visible (not Hidden).
pub fn provider_exposed(agent: AgentId) -> bool {
    capabilities(agent).is_some_and(|caps| caps.exposure != ProviderExposure::Hidden)
}

/// Providers that are product-visible (not Hidden) (technical front for
/// Settings/picker).
pub fn exposed_agents() -> Vec<AgentId> {
    PROVIDERS
        .iter()
        .filter(|entry| entry.exposure != ProviderExposure::Hidden)
        .map(|entry| entry.agent)
        .collect()
}

/// Providers with a live semantic source (sole source for UI descriptive
/// copy).
pub fn live_capable_agents() -> Vec<AgentId> {
    PROVIDERS
        .iter()
        .filter(|entry| entry.live != LiveCapability::None)
        .map(|entry| entry.agent)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Migration contract: the registry's alias resolution matches the old
    /// Chat normalize_provider semantics.
    #[test]
    fn resolve_agent_matches_previous_chat_aliases() {
        assert_eq!(
            resolve_agent_alias("claude-code", None),
            Some(AgentId::ClaudeCode)
        );
        assert_eq!(
            resolve_agent_alias("claude", Some("codex")),
            Some(AgentId::ClaudeCode)
        );
        assert_eq!(resolve_agent_alias("codex-cli", None), Some(AgentId::Codex));
        assert_eq!(resolve_agent_alias("pi", None), Some(AgentId::Pi));
        assert_eq!(resolve_agent_alias("oh-my-pi", None), Some(AgentId::Omp));
        assert_eq!(resolve_agent_alias("kimi", None), Some(AgentId::Kimi));
        assert_eq!(resolve_agent_alias("cursor", None), Some(AgentId::Cursor));
        assert_eq!(
            resolve_agent_alias("command-code", None),
            Some(AgentId::CommandCode)
        );
        assert_eq!(resolve_agent_alias("qoder-cli", None), Some(AgentId::Qoder));
        // Case normalization.
        assert_eq!(
            resolve_agent_alias("Claude", None),
            Some(AgentId::ClaudeCode)
        );
        // Unregistered aliases and unknown agents must miss.
        assert_eq!(resolve_agent_alias("unknown-agent", Some("shell")), None);
        assert_eq!(resolve_agent_alias("agy", None), None);
        assert_eq!(resolve_agent_alias("antigravity", Some("agy")), None);
    }

    #[test]
    fn qoder_is_history_preview_only_until_herdr_hook_binding_is_verified() {
        let caps = capabilities(AgentId::Qoder).unwrap_or_else(|| panic!("qoder missing"));
        assert_eq!(caps.exposure, ProviderExposure::Preview);
        assert_eq!(caps.live, LiveCapability::None);
    }

    /// Live capability list (includes Kimi after Wave 1); cross-consistency
    /// between the registry and the decoders is guaranteed by the ALL
    /// traversal assertions.
    #[test]
    fn live_capability_list_is_the_authority() {
        let live: Vec<AgentId> = live_capable_agents();
        assert_eq!(
            live,
            vec![
                AgentId::ClaudeCode,
                AgentId::Codex,
                AgentId::Cursor,
                AgentId::CommandCode,
                AgentId::Pi,
                AgentId::Omp,
                AgentId::Kimi,
            ]
        );
        for agent in AgentId::ALL {
            let supported = live.contains(&agent);
            assert_eq!(
                capabilities(agent).map(|caps| caps.live != LiveCapability::None),
                Some(supported),
                "registry/live decoder disagree on {agent:?}"
            );
        }
    }

    #[test]
    fn registry_covers_every_agent_exactly_once() {
        for agent in AgentId::ALL {
            let count = PROVIDERS
                .iter()
                .filter(|entry| entry.agent == agent)
                .count();
            assert_eq!(count, 1, "agent {agent:?} must appear exactly once");
        }
    }
}
