use shardlane_history::AgentId;
use std::process::Command;

/// AgentId → Herdr protocol short id (the value domain of the agent field, aligned
/// with server.agent_manifests: claude/agy/…; history slugs such as "claude-code"/
/// "antigravity" are not protocol values).
pub(crate) fn herdr_agent_id(agent: AgentId) -> &'static str {
    match agent {
        AgentId::ClaudeCode => "claude",
        AgentId::Codex => "codex",
        AgentId::Copilot => "copilot",
        AgentId::Cursor => "cursor",
        AgentId::Opencode => "opencode",
        AgentId::CommandCode => "commandcode",
        AgentId::Kiro => "kiro",
        AgentId::Gemini => "gemini",
        AgentId::Pi => "pi",
        AgentId::Omp => "omp",
        AgentId::Grok => "grok",
        AgentId::Kimi => "kimi",
        AgentId::Antigravity => "agy",
        AgentId::Dsh => "dsh",
        AgentId::Qoder => "qoder",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedAgentLaunch {
    pub(crate) agent: AgentId,
    pub(crate) executable: String,
}

pub(crate) fn resolve_agent_launch(agent: AgentId) -> Result<ResolvedAgentLaunch, String> {
    let candidates: &[&str] = match agent {
        AgentId::ClaudeCode => &["claude"],
        AgentId::Codex => &["codex"],
        AgentId::Copilot => &["copilot"],
        AgentId::Cursor => &["cursor-agent", "agent"],
        AgentId::Opencode => &["opencode", "opencode2"],
        AgentId::CommandCode => &["command-code", "commandcode", "cmd"],
        AgentId::Kiro => &["kiro-cli", "kiro"],
        AgentId::Gemini => &["gemini"],
        AgentId::Pi => &["pi"],
        AgentId::Omp => &["omp"],
        AgentId::Grok => &["grok"],
        AgentId::Kimi => &["kimi", "kimi-code"],
        AgentId::Antigravity => &["agy", "antigravity"],
        AgentId::Dsh => &["dsh"],
        AgentId::Qoder => &["qoder"],
    };
    let executable = candidates
        .iter()
        .find_map(|binary| resolve_binary(binary))
        .ok_or_else(|| {
            format!(
                "{} CLI not found — install it or make it available in your login shell PATH",
                agent.display_name()
            )
        })?;
    Ok(ResolvedAgentLaunch { agent, executable })
}

fn resolve_binary(binary: &str) -> Option<String> {
    let script = format!(
        "command -v {}",
        shardlane_history::resume::posix_quote(binary)
    );
    let output = Command::new("/bin/zsh")
        .args(["-lic", &script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    path.starts_with('/').then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_shell_resolution_returns_absolute_path_for_standard_binary() {
        let path = resolve_binary("sh").unwrap_or_else(|| panic!("login shell should resolve sh"));
        assert!(path.starts_with('/'));
        assert!(path.ends_with("/sh"));
    }

    #[test]
    fn herdr_agent_ids_match_host_integration_registry() {
        // Coverage contract: registry labels and the agent_cli protocol
        // short ids must stay pinned together — any drift in either mapping fails here.
        for agent in AgentId::ALL {
            let entry = shardlane_host::agent_integrations::integration_for(agent);
            assert_eq!(entry.provider, agent);
            assert_eq!(
                entry.herdr_label,
                herdr_agent_id(agent),
                "registry label drifted from herdr_agent_id: {agent:?}"
            );
        }
    }

    #[test]
    fn herdr_agent_ids_use_protocol_short_names_not_history_slugs() {
        assert_eq!(herdr_agent_id(AgentId::ClaudeCode), "claude");
        assert_eq!(herdr_agent_id(AgentId::Antigravity), "agy");
        assert_eq!(herdr_agent_id(AgentId::Pi), "pi");
        assert_eq!(herdr_agent_id(AgentId::CommandCode), "commandcode");
        // Every mapping must be a protocol short id: distinct from the history slugs
        // (claude-code/antigravity) and aligned with the server.agent_manifests value domain.
        for agent in AgentId::ALL {
            let id = herdr_agent_id(agent);
            assert!(!id.contains('-'), "{id} is not a Herdr short id");
            assert!(!id.is_empty());
        }
    }
}
