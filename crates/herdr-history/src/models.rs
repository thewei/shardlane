// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_MSG_TEXT: usize = 32 * 1024;
pub const MAX_TOOL_IO: usize = 16 * 1024;
pub const MAX_TITLE: usize = 80;
pub const UNTITLED: &str = "Untitled";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentId {
    ClaudeCode,
    Codex,
    Copilot,
    Cursor,
    Opencode,
    CommandCode,
    Kiro,
    Gemini,
    Pi,
    Omp,
    Grok,
    Kimi,
    Antigravity,
    Dsh,
    Qoder,
}

impl AgentId {
    pub const ALL: [Self; 15] = [
        Self::ClaudeCode,
        Self::Codex,
        Self::Copilot,
        Self::Cursor,
        Self::Opencode,
        Self::CommandCode,
        Self::Kiro,
        Self::Gemini,
        Self::Pi,
        Self::Omp,
        Self::Grok,
        Self::Kimi,
        Self::Antigravity,
        Self::Dsh,
        Self::Qoder,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
            Self::Cursor => "cursor",
            Self::Opencode => "opencode",
            Self::CommandCode => "command-code",
            Self::Kiro => "kiro",
            Self::Gemini => "gemini",
            Self::Pi => "pi",
            Self::Omp => "omp",
            Self::Grok => "grok",
            Self::Kimi => "kimi",
            Self::Antigravity => "antigravity",
            Self::Dsh => "dsh",
            Self::Qoder => "qoder",
        }
    }

    pub fn from_slug(value: &str) -> Option<Self> {
        match value {
            "claude-code" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            "copilot" => Some(Self::Copilot),
            "cursor" => Some(Self::Cursor),
            "opencode" => Some(Self::Opencode),
            "command-code" | "commandcode" => Some(Self::CommandCode),
            "kiro" => Some(Self::Kiro),
            "gemini" => Some(Self::Gemini),
            "pi" => Some(Self::Pi),
            "omp" => Some(Self::Omp),
            "grok" => Some(Self::Grok),
            "kimi" => Some(Self::Kimi),
            "antigravity" => Some(Self::Antigravity),
            "dsh" => Some(Self::Dsh),
            "qoder" => Some(Self::Qoder),
            _ => None,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Copilot => "Copilot CLI",
            Self::Cursor => "Cursor",
            Self::Opencode => "OpenCode",
            Self::CommandCode => "Command Code",
            Self::Kiro => "Kiro",
            Self::Gemini => "Gemini CLI",
            Self::Pi => "Pi",
            Self::Omp => "Oh My Pi",
            Self::Grok => "Grok Build",
            Self::Kimi => "Kimi Code",
            Self::Antigravity => "Antigravity CLI",
            Self::Dsh => "DeepSeek Harness",
            Self::Qoder => "Qoder",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFileRef {
    pub agent: AgentId,
    pub native_id: String,
    pub file_path: String,
    pub mtime_ms: i64,
    pub size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub key: String,
    pub id: String,
    pub agent: AgentId,
    pub title: String,
    pub project_path: String,
    pub project_name: String,
    pub file_path: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i64,
    pub size_bytes: i64,
    pub git_branch: Option<String>,
    pub model: Option<String>,
    pub tokens_used: Option<i64>,
    pub archived: bool,
    pub source: Option<String>,
}

/// List-row summary: SessionMeta plus a one-line Description preview
/// (catalog-owned derived data, excerpted from the first user message; legacy
/// rows are an empty string until backfilled).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub meta: SessionMeta,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallView {
    pub id: String,
    pub name: String,
    pub input_preview: String,
    pub input: Option<String>,
    pub output: Option<String>,
    pub is_error: bool,
    pub sidechain_ref: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MessageKind {
    Text,
    Meta,
    CompactSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptMessage {
    pub seq: i64,
    pub role: Role,
    pub kind: MessageKind,
    pub text: String,
    pub truncated: bool,
    pub tool_calls: Vec<ToolCallView>,
    pub thinking: Option<String>,
    pub timestamp: Option<i64>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidechainInfo {
    pub id: String,
    pub agent_type: Option<String>,
    pub description: Option<String>,
    pub tool_use_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedTranscript {
    pub meta: SessionMeta,
    pub mainline: Vec<TranscriptMessage>,
    pub sidechains: Vec<SidechainInfo>,
    pub unknown_line_count: u32,
}

impl ParsedTranscript {
    pub fn simple(
        meta: SessionMeta,
        mainline: Vec<TranscriptMessage>,
        unknown_line_count: u32,
    ) -> Self {
        Self {
            meta,
            mainline,
            sidechains: Vec::new(),
            unknown_line_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexUnit {
    pub seq: i64,
    pub sidechain_id: Option<String>,
    pub role: Role,
    pub timestamp: Option<i64>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSession {
    pub meta: SessionMeta,
    pub units: Vec<IndexUnit>,
    pub unknown_line_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub session: SessionMeta,
    pub seq: i64,
    pub role: String,
    pub snippet: String,
    pub timestamp: Option<i64>,
}

pub type ConversationMeta = SessionMeta;
pub type ConversationRef = SessionFileRef;
pub type ParsedConversation = ParsedSession;
pub type ToolCall = ToolCallView;

/// The one path-key normalization rule, shared by the history source policy
/// and the catalog `project_key`: collapse redundant separators and `.`
/// components while deliberately preserving the user's spelling — no
/// `fs::canonicalize`, so symlinks and macOS `/var` ↔ `/private/var` aliases
/// stay exactly as written and one directory always maps to one key
/// regardless of filesystem state.
pub fn normalize_path_key(path: &Path) -> PathBuf {
    path.components().collect::<PathBuf>()
}
