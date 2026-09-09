//! Tier 3 Process & Title Agent Status Sniffing.
//!
//! Non-invasive detection of coding agents running in tmux / uuyc / terminal panes
//! without requiring explicit hooks (Tier 3 degradation).

/// Infers the agent identifier and basic status from foreground command and pane title.
///
/// Returns `(Option<agent_name>, Option<agent_status>)`.
pub fn sniff_agent_from_process_and_title(
    command: Option<&str>,
    title: Option<&str>,
) -> (Option<String>, Option<String>) {
    let clean_cmd = command.map(|c| {
        let trimmed = c.trim();
        // Extract binary basename (e.g. /opt/homebrew/bin/claude -> claude)
        trimmed.rsplit('/').next().unwrap_or(trimmed).to_lowercase()
    });

    let lower_title = title.map(|t| t.to_lowercase());

    // 1. Identify Agent
    let agent = match clean_cmd.as_deref() {
        Some("claude" | "claude-code") => Some("claude".to_string()),
        Some("codex" | "opencodex") => Some("codex".to_string()),
        Some("opencode") => Some("opencode".to_string()),
        Some("pi" | "pi-agent") => Some("pi".to_string()),
        Some("command-code" | "commandcode") => Some("commandcode".to_string()),
        Some("cursor") => Some("cursor".to_string()),
        Some("copilot") => Some("copilot".to_string()),
        Some("gemini") => Some("gemini".to_string()),
        Some("kimi") => Some("kimi".to_string()),
        Some("qoder") => Some("qoder".to_string()),
        _ => {
            // Check title if command was generic or absent
            if let Some(t) = &lower_title {
                if t.contains("claude code") || t.contains("claude") {
                    Some("claude".to_string())
                } else if t.contains("codex") || t.contains("opencodex") {
                    Some("codex".to_string())
                } else if t.contains("opencode") {
                    Some("opencode".to_string())
                } else if t.contains("command code") || t.contains("commandcode") {
                    Some("commandcode".to_string())
                } else if t.contains("pi agent") {
                    Some("pi".to_string())
                } else if t.contains("gemini") {
                    Some("gemini".to_string())
                } else if t.contains("qoder") {
                    Some("qoder".to_string())
                } else {
                    None
                }
            } else {
                None
            }
        }
    };

    if agent.is_none() {
        return (None, None);
    }

    // 2. Identify Status
    let status = if let Some(t) = &lower_title {
        if t.contains("[working]")
            || t.contains("(working)")
            || t.contains("working")
            || t.contains("thinking")
            || t.contains("running")
            || t.contains("busy")
        {
            Some("working".to_string())
        } else if t.contains("[blocked]")
            || t.contains("(waiting)")
            || t.contains("waiting")
            || t.contains("permission")
            || t.contains("confirm")
            || t.contains("blocked")
        {
            Some("blocked".to_string())
        } else if t.contains("[idle]")
            || t.contains("(idle)")
            || t.contains("idle")
            || t.contains("ready")
            || t.contains("done")
        {
            Some("idle".to_string())
        } else {
            Some("running".to_string())
        }
    } else {
        Some("running".to_string())
    };

    (agent, status)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_sniff_claude_process_and_title() {
        let (agent, status) = sniff_agent_from_process_and_title(
            Some("/usr/local/bin/claude"),
            Some("Claude Code - [Working]"),
        );
        assert_eq!(agent.as_deref(), Some("claude"));
        assert_eq!(status.as_deref(), Some("working"));
    }

    #[test]
    fn test_sniff_codex_blocked() {
        let (agent, status) = sniff_agent_from_process_and_title(
            Some("codex"),
            Some("Codex (waiting for user input)"),
        );
        assert_eq!(agent.as_deref(), Some("codex"));
        assert_eq!(status.as_deref(), Some("blocked"));
    }

    #[test]
    fn test_sniff_from_title_fallback() {
        let (agent, status) =
            sniff_agent_from_process_and_title(Some("node"), Some("opencode - idle"));
        assert_eq!(agent.as_deref(), Some("opencode"));
        assert_eq!(status.as_deref(), Some("idle"));
    }

    #[test]
    fn test_sniff_non_agent_silent_fallback() {
        let (agent, status) =
            sniff_agent_from_process_and_title(Some("zsh"), Some("wilson@MacBook-Pro: ~/work"));
        assert_eq!(agent, None);
        assert_eq!(status, None);
    }

    #[test]
    fn test_four_tier_degradation_matrix() {
        // Tier 3: Known Agent with active title
        let (agent3, status3) =
            sniff_agent_from_process_and_title(Some("claude"), Some("Claude Code - Running"));
        assert_eq!(agent3.as_deref(), Some("claude"));
        assert_eq!(status3.as_deref(), Some("working"));

        // Tier 3: Known Agent with idle title
        let (agent3_idle, status3_idle) =
            sniff_agent_from_process_and_title(Some("pi"), Some("pi - ready"));
        assert_eq!(agent3_idle.as_deref(), Some("pi"));
        assert_eq!(status3_idle.as_deref(), Some("idle"));

        // Tier 3: Known Agent without title clue -> defaults to running
        let (agent3_generic, status3_generic) =
            sniff_agent_from_process_and_title(Some("commandcode"), Some("bash"));
        assert_eq!(agent3_generic.as_deref(), Some("commandcode"));
        assert_eq!(status3_generic.as_deref(), Some("running"));

        // Tier 4: Silent Fallback (standard shell, editor, tools)
        for non_agent in &["vim", "nano", "htop", "git", "cargo", "zsh", "bash", "ssh"] {
            let (agent4, status4) =
                sniff_agent_from_process_and_title(Some(non_agent), Some("terminal window"));
            assert_eq!(agent4, None, "command {non_agent} must fall back silently");
            assert_eq!(status4, None, "command {non_agent} must have None status");
        }
    }
}
