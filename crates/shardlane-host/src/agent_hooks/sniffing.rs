//! Tier 3 Process & Title Agent Status Sniffing.
//!
//! [INPUT]: foreground command basename and pane title from multiplexer
//! projections.
//! [OUTPUT]: sniff_agent_from_process_and_title (agent identity + status).
//! [POS]: Tier 3 兜底识别：Agent 身份只从精确二进制名判定，标题只参与
//! 状态推断、绝不参与身份判定——substring 标题匹配曾把无关进程/服务器
//! 误识别成 Agent（2026-09-19 行为修正）。上游权威仍是 Tier 1 hook IPC
//! （adapter 层的 registry 别名校验）与 Herdr 的 agent_session。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

/// Infers the agent identifier and basic status from foreground command and pane title.
///
/// Identity rule (strict): only an exact foreground-binary match against the
/// allowlist identifies an agent. Titles never identify agents — they carry
/// user-controlled text (file names, server names) and previously produced
/// false Agent projections.
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

    // 1. Identify Agent: exact binary name only.
    let agent = match clean_cmd.as_deref() {
        Some("claude" | "claude-code") => Some("claude".to_string()),
        Some("codex" | "opencodex") => Some("codex".to_string()),
        Some("opencode") => Some("opencode".to_string()),
        Some("pi" | "pi-agent") => Some("pi".to_string()),
        Some("command-code" | "commandcode") => Some("commandcode".to_string()),
        // Cursor 的终端 CLI 是 cursor-agent；裸 "cursor" 是 IDE 启动器，
        // 精确匹配它会把编辑器误报成 Agent。
        Some("cursor-agent") => Some("cursor".to_string()),
        Some("copilot") => Some("copilot".to_string()),
        Some("gemini") => Some("gemini".to_string()),
        Some("kimi" | "kimi-code") => Some("kimi".to_string()),
        Some("qoder") => Some("qoder".to_string()),
        Some("agy" | "antigravity") => Some("agy".to_string()),
        Some("omp") => Some("omp".to_string()),
        Some("grok") => Some("grok".to_string()),
        Some("kiro-cli" | "kiro") => Some("kiro".to_string()),
        Some("dsh") => Some("dsh".to_string()),
        _ => {
            // 标题不再参与身份判定：无精确命令命中即非 Agent。
            None
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

    /// 误识别回归（2026-09-19）：标题/文件名/服务器名绝不能产生 Agent。
    #[test]
    fn test_title_text_never_identifies_an_agent() {
        let cases: [(Option<&str>, &str); 6] = [
            (Some("vim"), "claude.md — vim"),
            (Some("node"), "mcp-server-gemini"),
            (Some("node"), "codex-proxy-server"),
            (Some("python3"), "claude-code-server.py"),
            (Some("zsh"), "wilson@MacBook-Pro: ~/work/antigravity"),
            (None, "opencode — http server"),
        ];
        for (cmd, title) in cases {
            let (agent, status) = sniff_agent_from_process_and_title(cmd, Some(title));
            assert!(
                agent.is_none(),
                "title {title:?} must not identify an agent"
            );
            assert!(status.is_none(), "no agent means no status");
        }
    }

    #[test]
    fn test_exact_binaries_cover_the_extended_allowlist() {
        for (binary, expected) in [
            ("agy", "agy"),
            ("antigravity", "agy"),
            ("kimi-code", "kimi"),
            ("cursor-agent", "cursor"),
            ("omp", "omp"),
            ("grok", "grok"),
            ("kiro-cli", "kiro"),
            ("dsh", "dsh"),
        ] {
            let (agent, _) = sniff_agent_from_process_and_title(Some(binary), None);
            assert_eq!(agent.as_deref(), Some(expected), "binary {binary}");
        }
        // 裸 "cursor" 是 IDE 启动器：不是 Agent。
        let (ide, _) = sniff_agent_from_process_and_title(Some("cursor"), None);
        assert_eq!(ide, None);
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
