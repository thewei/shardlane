// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

use super::parse_utils::*;
use super::{units_from_messages, AgentHistoryAdapter};
use crate::models::*;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

const KNOWN_SKIP_TYPES: &[&str] = &[
    "queue-operation",
    "mode",
    "last-prompt",
    "permission-mode",
    "file-history-snapshot",
    "file-history-delta",
    "pr-link",
    "frame-link",
    "attachment",
    "summary",
];

pub struct ClaudeAdapter {
    root: PathBuf,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        let root = home_dir().join(".claude").join("projects");
        Self { root }
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: PathBuf) -> Self {
        Self { root }
    }
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct ParseResult {
    pub(crate) messages: Vec<TranscriptMessage>,
    pub(crate) title: String,
    pub(crate) cwd: String,
    pub(crate) git_branch: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) tokens_used: i64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) unknown_lines: u32,
}

#[derive(Default)]
struct PendingAssistant {
    msg_id: Option<String>,
    text: Vec<String>,
    thinking: Vec<String>,
    tool_calls: Vec<ToolCallView>,
    timestamp: Option<i64>,
    model: Option<String>,
}

/// Current projection of the pending assistant group (shared by flush and the
/// live snapshot); returns None for an empty group.
fn pending_message(pending: &PendingAssistant) -> Option<TranscriptMessage> {
    let text = pending.text.join("\n\n");
    if text.is_empty() && pending.thinking.is_empty() && pending.tool_calls.is_empty() {
        return None;
    }
    let (text, truncated) = clip(&text, MAX_MSG_TEXT);
    let thinking =
        (!pending.thinking.is_empty()).then(|| clip(&pending.thinking.join("\n\n"), MAX_TOOL_IO).0);
    Some(TranscriptMessage {
        seq: 0,
        role: Role::Assistant,
        kind: MessageKind::Text,
        text,
        truncated,
        tool_calls: pending.tool_calls.clone(),
        thinking,
        timestamp: pending.timestamp,
        model: pending.model.clone(),
    })
}

/// Line-level interpretation state shared by full parsing and live incremental
/// decoding: `parse_jsonl` and `crate::live` feed the same `feed_line` path, so
/// both sides agree by construction.
pub(crate) struct ClaudeSession {
    include_sidechain: bool,
    messages: Vec<TranscriptMessage>,
    custom_title: String,
    fallback_title: String,
    pub(crate) cwd: String,
    pub(crate) git_branch: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) tokens_used: i64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) unknown_lines: u32,
    pending: Option<PendingAssistant>,
    tool_index: HashMap<String, (usize, usize)>,
    pub(crate) update: crate::live::DecoderUpdate,
}

impl ClaudeSession {
    pub(crate) fn new(include_sidechain: bool) -> Self {
        Self {
            include_sidechain,
            messages: Vec::new(),
            custom_title: String::new(),
            fallback_title: String::new(),
            cwd: String::new(),
            git_branch: None,
            model: None,
            tokens_used: 0,
            created_at: 0,
            updated_at: 0,
            unknown_lines: 0,
            pending: None,
            tool_index: HashMap::new(),
            update: crate::live::DecoderUpdate::default(),
        }
    }

    pub(crate) fn count_unknown_line(&mut self) {
        self.unknown_lines += 1;
    }

    fn push_message(&mut self, mut message: TranscriptMessage) {
        message.seq = self.messages.len() as i64;
        self.update.appended.push(self.messages.len());
        self.messages.push(message);
    }

    fn flush_pending(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let Some(message) = pending_message(&pending) else {
            return;
        };
        let message_index = self.messages.len();
        for (tool_index_in_message, tool_call) in message.tool_calls.iter().enumerate() {
            if !tool_call.id.is_empty() {
                self.tool_index
                    .insert(tool_call.id.clone(), (message_index, tool_index_in_message));
            }
        }
        self.push_message(message);
    }

    pub(crate) fn feed_line(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        let row: Value = match serde_json::from_str(line) {
            Ok(row) => row,
            Err(_) => {
                self.unknown_lines += 1;
                return;
            }
        };
        let row_type = row.get("type").and_then(Value::as_str).unwrap_or("");

        if row_type == "custom-title" {
            if let Some(title) = row.get("customTitle").and_then(Value::as_str) {
                if !title.trim().is_empty() {
                    self.custom_title = title.trim().to_string();
                }
            }
            return;
        }
        if !matches!(row_type, "user" | "assistant" | "system") {
            if row_type.is_empty() || !KNOWN_SKIP_TYPES.contains(&row_type) {
                self.unknown_lines += 1;
            }
            return;
        }

        if self.cwd.is_empty() {
            self.cwd = row
                .get("cwd")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
        }
        if let Some(branch) = row.get("gitBranch").and_then(Value::as_str) {
            if !branch.is_empty() {
                self.git_branch = Some(branch.to_string());
            }
        }
        let timestamp = row.get("timestamp").map(to_epoch_ms).unwrap_or(0);
        if timestamp > 0 {
            if self.created_at == 0 {
                self.created_at = timestamp;
            }
            self.updated_at = self.updated_at.max(timestamp);
        }
        if row.get("isSidechain").and_then(Value::as_bool) == Some(true) && !self.include_sidechain
        {
            return;
        }

        if row_type == "system" {
            self.flush_pending();
            if row.get("subtype").and_then(Value::as_str) == Some("compact_boundary") {
                self.push_message(plain_message(
                    Role::System,
                    MessageKind::CompactSummary,
                    "── Context compacted ──",
                    timestamp,
                ));
            } else if let Some(content) = row.get("content").and_then(Value::as_str) {
                if !content.is_empty() {
                    let (text, truncated) = clip(content, MAX_TOOL_IO);
                    self.push_message(TranscriptMessage {
                        seq: 0,
                        role: Role::System,
                        kind: MessageKind::Meta,
                        text,
                        truncated,
                        tool_calls: Vec::new(),
                        thinking: None,
                        timestamp: (timestamp > 0).then_some(timestamp),
                        model: None,
                    });
                }
            }
            return;
        }

        let Some(message) = row.get("message") else {
            return;
        };
        if row_type == "user" {
            self.flush_pending();
            let mut parts = Vec::new();
            let mut filled_tool_messages: Vec<usize> = Vec::new();
            match message.get("content") {
                Some(Value::String(text)) => parts.push(text.clone()),
                Some(Value::Array(blocks)) => {
                    for block in blocks {
                        match block.get("type").and_then(Value::as_str) {
                            Some("text") => {
                                if let Some(text) = block.get("text").and_then(Value::as_str) {
                                    parts.push(text.to_string());
                                }
                            }
                            Some("tool_result") => {
                                let id = block
                                    .get("tool_use_id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                if let Some(&(message_index, tool_index_in_message)) =
                                    self.tool_index.get(id)
                                {
                                    let output = stringify_tool_result(block.get("content"));
                                    if let Some(tool_call) = self
                                        .messages
                                        .get_mut(message_index)
                                        .and_then(|message: &mut TranscriptMessage| {
                                            message.tool_calls.get_mut(tool_index_in_message)
                                        })
                                    {
                                        tool_call.output = Some(clip(&output, MAX_TOOL_IO).0);
                                        tool_call.is_error =
                                            block.get("is_error").and_then(Value::as_bool)
                                                == Some(true);
                                        filled_tool_messages.push(message_index);
                                    }
                                }
                            }
                            Some("image") => parts.push("[image]".to_string()),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            for message_index in filled_tool_messages {
                self.update.changed.push(message_index);
            }
            let text = parts.join("\n\n").trim().to_string();
            if text.is_empty() {
                return;
            }
            let kind = if row.get("isCompactSummary").and_then(Value::as_bool) == Some(true) {
                MessageKind::CompactSummary
            } else if row.get("isMeta").and_then(Value::as_bool) == Some(true)
                || is_injected_user_content(&text)
            {
                MessageKind::Meta
            } else {
                MessageKind::Text
            };
            if kind == MessageKind::Text && self.fallback_title.is_empty() {
                self.fallback_title = clean_title_candidate(&text);
            }
            let (text, truncated) = clip(&text, MAX_MSG_TEXT);
            self.push_message(TranscriptMessage {
                seq: 0,
                role: Role::User,
                kind,
                text,
                truncated,
                tool_calls: Vec::new(),
                thinking: None,
                timestamp: (timestamp > 0).then_some(timestamp),
                model: None,
            });
            return;
        }

        let msg_id = message
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let need_new = match (&self.pending, &msg_id) {
            (None, _) => true,
            (Some(current), Some(next)) => current
                .msg_id
                .as_deref()
                .is_some_and(|current_id| current_id != next),
            (Some(_), None) => false,
        };
        if need_new {
            self.flush_pending();
            self.pending = Some(PendingAssistant {
                msg_id: msg_id.clone(),
                timestamp: (timestamp > 0).then_some(timestamp),
                ..Default::default()
            });
        }
        let Some(current) = self.pending.as_mut() else {
            return;
        };
        if current.msg_id.is_none() {
            current.msg_id = msg_id;
        }
        if let Some(model_name) = message.get("model").and_then(Value::as_str) {
            if !model_name.is_empty() && model_name != "<synthetic>" {
                current.model = Some(model_name.to_string());
                self.model = Some(model_name.to_string());
            }
        }
        if let Some(usage) = message.get("usage") {
            self.tokens_used += usage
                .get("input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                + usage
                    .get("output_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                + usage
                    .get("cache_creation_input_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
        }
        match message.get("content") {
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                if !text.trim().is_empty() {
                                    current.text.push(text.to_string());
                                }
                            }
                        }
                        Some("thinking") => {
                            if let Some(text) = block.get("thinking").and_then(Value::as_str) {
                                if !text.trim().is_empty() {
                                    current.thinking.push(text.to_string());
                                }
                            }
                        }
                        Some("tool_use") => {
                            let id = block
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                            let input = block.get("input").cloned().unwrap_or(Value::Null);
                            current
                                .tool_calls
                                .push(tool_call_view(id, name, &input, None, false));
                        }
                        _ => {}
                    }
                }
            }
            Some(Value::String(text)) if !text.trim().is_empty() => current.text.push(text.clone()),
            _ => {}
        }
    }

    pub(crate) fn resolved_title(&self) -> String {
        if !self.custom_title.is_empty() {
            self.custom_title.clone()
        } else if !self.fallback_title.is_empty() {
            self.fallback_title.clone()
        } else {
            UNTITLED.to_string()
        }
    }

    /// live current view: projects unflushed pending onto the last message
    /// (non-destructively), consistent with the message sequence `finish()`
    /// would produce over the same content.
    pub(crate) fn snapshot_messages(&self) -> Vec<TranscriptMessage> {
        let mut messages = self.messages.clone();
        if let Some(pending) = &self.pending {
            if let Some(mut message) = pending_message(pending) {
                message.seq = messages.len() as i64;
                messages.push(message);
            }
        }
        messages
    }

    /// live incremental channel: total message count (including the pending
    /// projection), consistent with the `snapshot_messages` length.
    pub(crate) fn live_message_count(&self) -> usize {
        self.messages.len() + self.pending.as_ref().and_then(pending_message).is_some() as usize
    }

    /// live incremental channel: clone a single message (index space of
    /// `live_message_count`).
    pub(crate) fn live_message_at(&self, index: usize) -> Option<TranscriptMessage> {
        if let Some(message) = self.messages.get(index) {
            return Some(message.clone());
        }
        let pending = self.pending.as_ref()?;
        if index == self.messages.len() {
            let mut message = pending_message(pending)?;
            message.seq = index as i64;
            return Some(message);
        }
        None
    }

    pub(crate) fn finish(mut self) -> ParseResult {
        self.flush_pending();
        let title = self.resolved_title();
        ParseResult {
            messages: self.messages,
            title,
            cwd: self.cwd,
            git_branch: self.git_branch,
            model: self.model,
            tokens_used: self.tokens_used,
            created_at: self.created_at,
            updated_at: self.updated_at,
            unknown_lines: self.unknown_lines,
        }
    }
}

fn parse_jsonl(path: &Path, include_sidechain: bool) -> Result<ParseResult> {
    let file = fs::File::open(path)?;
    let reader = BufReader::with_capacity(1 << 20, file);
    let mut session = ClaudeSession::new(include_sidechain);
    for line in reader.lines() {
        match line {
            Ok(line) => session.feed_line(&line),
            Err(_) => session.count_unknown_line(),
        }
    }
    Ok(session.finish())
}

fn plain_message(role: Role, kind: MessageKind, text: &str, timestamp: i64) -> TranscriptMessage {
    TranscriptMessage {
        seq: 0,
        role,
        kind,
        text: text.to_string(),
        truncated: false,
        tool_calls: Vec::new(),
        thinking: None,
        timestamp: (timestamp > 0).then_some(timestamp),
        model: None,
    }
}

fn stringify_tool_result(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match item.get("type").and_then(Value::as_str) {
                Some("text") => item.get("text").and_then(Value::as_str).map(str::to_string),
                Some("image") => Some("[image]".to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(value @ Value::Object(_)) => serde_json::to_string(value).unwrap_or_default(),
        _ => String::new(),
    }
}

fn build_meta(reference: &SessionFileRef, parsed: &ParseResult) -> SessionMeta {
    SessionMeta {
        key: format!("claude-code:{}", reference.native_id),
        id: reference.native_id.clone(),
        agent: AgentId::ClaudeCode,
        title: parsed.title.clone(),
        project_path: parsed.cwd.clone(),
        project_name: project_name_of(&parsed.cwd),
        file_path: reference.file_path.clone(),
        created_at: if parsed.created_at > 0 {
            parsed.created_at
        } else {
            reference.mtime_ms
        },
        updated_at: if parsed.updated_at > 0 {
            parsed.updated_at
        } else {
            reference.mtime_ms
        },
        message_count: parsed
            .messages
            .iter()
            .filter(|message| message.kind == MessageKind::Text)
            .count() as i64,
        size_bytes: reference.size,
        git_branch: parsed.git_branch.clone(),
        model: parsed.model.clone(),
        tokens_used: (parsed.tokens_used > 0).then_some(parsed.tokens_used),
        archived: false,
        source: None,
    }
}

fn subagents_dir(reference: &SessionFileRef) -> PathBuf {
    Path::new(&reference.file_path)
        .parent()
        .unwrap_or(Path::new("."))
        .join(&reference.native_id)
        .join("subagents")
}

fn list_sidechains(reference: &SessionFileRef) -> Vec<SidechainInfo> {
    let dir = subagents_dir(reference);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let id = name.strip_suffix(".jsonl")?.to_string();
            let mut info = SidechainInfo {
                id: id.clone(),
                agent_type: None,
                description: None,
                tool_use_id: None,
            };
            if let Ok(raw) = fs::read_to_string(dir.join(format!("{id}.meta.json"))) {
                if let Ok(meta) = serde_json::from_str::<Value>(&raw) {
                    info.agent_type = meta
                        .get("agentType")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    info.description = meta
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    info.tool_use_id = meta
                        .get("toolUseId")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
            }
            Some(info)
        })
        .collect()
}

impl AgentHistoryAdapter for ClaudeAdapter {
    fn agent(&self) -> AgentId {
        AgentId::ClaudeCode
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        // Missing root = not installed/deleted: a legitimate empty set. Root
        // exists but read fails = propagate the error.
        if !self.root.is_dir() {
            return Ok(Vec::new());
        }
        let projects = fs::read_dir(&self.root)
            .map_err(|error| anyhow::anyhow!("read {}: {error}", self.root.display()))?;
        let mut references = Vec::new();
        for project in projects.flatten() {
            let Ok(entries) = fs::read_dir(project.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                if let Some(reference) = default_file_ref(self.agent(), &entry.path()) {
                    references.push(reference);
                }
            }
        }
        Ok(references)
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession> {
        let parsed = parse_jsonl(Path::new(&reference.file_path), false)?;
        Ok(ParsedSession {
            meta: build_meta(reference, &parsed),
            units: units_from_messages(&parsed.messages),
            unknown_line_count: parsed.unknown_lines,
        })
    }

    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript> {
        let parsed = parse_jsonl(Path::new(&reference.file_path), false)?;
        let sidechains = list_sidechains(reference);
        let mut mainline = parsed.messages.clone();
        let sidechains_by_tool = sidechains
            .iter()
            .filter_map(|sidechain| {
                sidechain
                    .tool_use_id
                    .as_deref()
                    .map(|tool_use_id| (tool_use_id, sidechain.id.as_str()))
            })
            .collect::<HashMap<_, _>>();
        for message in &mut mainline {
            for tool_call in &mut message.tool_calls {
                if let Some(sidechain_id) = sidechains_by_tool.get(tool_call.id.as_str()) {
                    tool_call.sidechain_ref = Some((*sidechain_id).to_string());
                }
            }
        }
        Ok(ParsedTranscript {
            meta: build_meta(reference, &parsed),
            mainline,
            sidechains,
            unknown_line_count: parsed.unknown_lines,
        })
    }

    fn load_sidechain(
        &self,
        reference: &SessionFileRef,
        sidechain_id: &str,
    ) -> Result<Vec<TranscriptMessage>> {
        let path = subagents_dir(reference).join(format!("{sidechain_id}.jsonl"));
        if !path.is_file() {
            return Ok(Vec::new());
        }
        Ok(parse_jsonl(&path, true)?.messages)
    }

    fn file_ref(&self, path: &Path) -> Option<SessionFileRef> {
        let path_text = path.to_string_lossy();
        if path_text.contains("/subagents/") || path_text.contains("/memory/") {
            return None;
        }
        default_file_ref(self.agent(), path)
    }

    fn with_custom_root(&self, dir: PathBuf) -> Box<dyn AgentHistoryAdapter> {
        let root = if dir.join("projects").is_dir() {
            dir.join("projects")
        } else {
            dir
        };
        Box::new(Self { root })
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        vec![self.root.clone()]
    }
}
