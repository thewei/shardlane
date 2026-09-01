// SPDX-License-Identifier: MIT
// Resume CLI mapping and POSIX quoting retain upstream MIT-derived semantics.
//! [INPUT]: Depends on models::{AgentId, SessionMeta}.
//! [OUTPUT]: Exposes ResumeIntent, ResumeCommand, resume_supported,
//! prepare_resume, posix_quote.
//! [POS]: Session resume command generation core of shardlane-history; pure
//! functions, environment independent.

use crate::models::{AgentId, SessionMeta};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeIntent {
    pub agent: AgentId,
    pub native_session_id: String,
    pub project_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeCommand {
    pub intent: ResumeIntent,
    pub binary: String,
    pub args: Vec<String>,
}

impl ResumeIntent {
    pub fn from_session(meta: &SessionMeta) -> Self {
        Self {
            agent: meta.agent,
            native_session_id: meta.id.clone(),
            project_path: meta.project_path.clone(),
        }
    }
}

impl ResumeCommand {
    pub fn command_text(&self, executable: &str) -> String {
        std::iter::once(executable)
            .chain(self.args.iter().map(String::as_str))
            .map(posix_quote)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub fn resume_supported(agent: AgentId) -> bool {
    resume_parts(agent, "probe").is_some()
}

pub fn prepare_resume(intent: ResumeIntent) -> Result<ResumeCommand, String> {
    let (binary, args) =
        resume_parts(intent.agent, &intent.native_session_id).ok_or_else(|| {
            format!(
                "Resume isn't supported for {} yet",
                intent.agent.display_name()
            )
        })?;
    Ok(ResumeCommand {
        intent,
        binary: binary.to_string(),
        args,
    })
}

fn resume_parts(agent: AgentId, id: &str) -> Option<(&'static str, Vec<String>)> {
    match agent {
        AgentId::ClaudeCode => Some(("claude", vec!["--resume".to_string(), id.to_string()])),
        AgentId::Codex => Some(("codex", vec!["resume".to_string(), id.to_string()])),
        AgentId::Copilot => Some(("copilot", vec![format!("--resume={id}")])),
        AgentId::Cursor => Some(("cursor-agent", vec!["--resume".to_string(), id.to_string()])),
        AgentId::Pi => Some(("pi", vec!["--session".to_string(), id.to_string()])),
        AgentId::Omp => Some(("omp", vec!["--session".to_string(), id.to_string()])),
        AgentId::Opencode => Some(("opencode", vec!["--session".to_string(), id.to_string()])),
        AgentId::CommandCode => Some((
            "command-code",
            vec!["--session".to_string(), id.to_string()],
        )),
        AgentId::Antigravity => Some(("agy", vec!["--conversation".to_string(), id.to_string()])),
        AgentId::Gemini => Some(("gemini", vec!["--resume".to_string(), id.to_string()])),
        AgentId::Kiro => Some(("kiro-cli", vec!["--resume".to_string(), id.to_string()])),
        AgentId::Kimi => Some(("kimi", vec!["--resume".to_string(), id.to_string()])),
        AgentId::Grok => Some(("grok", vec!["--resume".to_string(), id.to_string()])),
        AgentId::Dsh => Some(("dsh", vec!["--resume".to_string(), id.to_string()])),
        AgentId::Qoder => Some(("qoder", vec!["--resume".to_string(), id.to_string()])),
    }
}

pub fn posix_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "_-./:=".contains(ch))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_and_codex_resume_shapes_match_native_clis() {
        assert_eq!(
            resume_parts(AgentId::ClaudeCode, "session-1"),
            Some((
                "claude",
                vec!["--resume".to_string(), "session-1".to_string()]
            ))
        );
        assert_eq!(
            resume_parts(AgentId::Codex, "session-2"),
            Some(("codex", vec!["resume".to_string(), "session-2".to_string()]))
        );
        assert_eq!(
            resume_parts(AgentId::Copilot, "session-3"),
            Some(("copilot", vec!["--resume=session-3".to_string()]))
        );
        assert_eq!(
            resume_parts(AgentId::Cursor, "session-4"),
            Some((
                "cursor-agent",
                vec!["--resume".to_string(), "session-4".to_string()]
            ))
        );
        assert_eq!(
            resume_parts(AgentId::Qoder, "session-qoder"),
            Some((
                "qoder",
                vec!["--resume".to_string(), "session-qoder".to_string()]
            ))
        );
        assert_eq!(
            resume_parts(AgentId::Pi, "session-5"),
            Some(("pi", vec!["--session".to_string(), "session-5".to_string()]))
        );
        assert_eq!(
            resume_parts(AgentId::CommandCode, "session-cc"),
            Some((
                "command-code",
                vec!["--session".to_string(), "session-cc".to_string()]
            ))
        );
        assert!(resume_supported(AgentId::ClaudeCode));
        assert!(resume_supported(AgentId::Copilot));
        assert!(resume_supported(AgentId::Pi));
        assert!(resume_supported(AgentId::CommandCode));
        assert!(resume_supported(AgentId::Antigravity));
        assert!(resume_supported(AgentId::Qoder));
    }

    #[test]
    fn prepare_resume_is_environment_independent() {
        let command = prepare_resume(ResumeIntent {
            agent: AgentId::ClaudeCode,
            native_session_id: "session-pure".to_string(),
            project_path: "/definitely/missing/herdr-project".to_string(),
        })
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(command.binary, "claude");
        assert_eq!(command.args, ["--resume", "session-pure"]);
    }

    #[test]
    fn posix_quote_preserves_safe_values_and_escapes_single_quotes() {
        assert_eq!(posix_quote("/tmp/demo"), "/tmp/demo");
        assert_eq!(posix_quote("hello world"), "'hello world'");
        assert_eq!(posix_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn command_text_quotes_executable_and_resume_id() {
        let command = ResumeCommand {
            intent: ResumeIntent {
                agent: AgentId::ClaudeCode,
                native_session_id: "id with space".to_string(),
                project_path: "/tmp/demo".to_string(),
            },
            binary: "claude".to_string(),
            args: vec!["--resume".to_string(), "id with space".to_string()],
        };
        assert_eq!(
            command.command_text("/Applications/Claude Code/bin/claude"),
            "'/Applications/Claude Code/bin/claude' --resume 'id with space'"
        );
    }
}
