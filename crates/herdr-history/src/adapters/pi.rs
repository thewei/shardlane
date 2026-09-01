// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

use super::parse_utils::*;
use super::{units_from_messages, AgentHistoryAdapter};
use crate::models::*;
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Pi session format (omp is a fork of pi with an identical format; only the
/// data root and AgentId differ):
/// `~/.pi/agent/sessions/<lossily-encoded dir>/<timestamp>_<uuid>.jsonl`.
/// The first line {type:session,version,id,timestamp,cwd} provides the
/// authoritative id/cwd — never inferred from the directory name; subsequent
/// lines are {type:message,message:{role:user|assistant|toolResult,content:[…]}}.
/// toolResult is a separate role row back-filled by toolCallId; known
/// non-content lines such as model_change are skipped silently.
pub struct PiAdapter {
    agent: AgentId,
    root: PathBuf,
}

impl PiAdapter {
    pub fn new() -> Self {
        Self::for_agent_root(AgentId::Pi, home_dir().join(".pi/agent/sessions"))
    }

    /// Oh My Pi variant: `~/.omp/agent/sessions`; the parsing core is fully
    /// shared.
    pub fn omp() -> Self {
        Self::for_agent_root(AgentId::Omp, home_dir().join(".omp/agent/sessions"))
    }

    fn for_agent_root(agent: AgentId, root: PathBuf) -> Self {
        Self { agent, root }
    }

    #[cfg(test)]
    pub(crate) fn with_root(agent: AgentId, root: PathBuf) -> Self {
        Self { agent, root }
    }
}

impl Default for PiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// File-name stem `<timestamp>_<uuid>` → uuid; falls back to the whole stem
/// when there is no `_`.
fn native_id_of(stem: &str) -> String {
    stem.rsplit('_').next().unwrap_or(stem).to_string()
}

pub(crate) struct PiParse {
    pub(crate) session_id: Option<String>,
    pub(crate) cwd: String,
    pub(crate) created_at: i64,
    /// Time of the last message line (merged messages keep the round's first
    /// line time; updated is tracked separately).
    pub(crate) last_ts: i64,
    pub(crate) messages: Vec<TranscriptMessage>,
    pub(crate) model: Option<String>,
    pub(crate) tokens_used: Option<i64>,
    pub(crate) unknown_lines: u32,
}

/// Line-level interpretation state shared by full parsing and live incremental
/// decoding: `parse_jsonl` and `crate::live` feed the same `feed_line` path, so
/// both sides agree by construction.
pub(crate) struct PiSession {
    pub(crate) session_id: Option<String>,
    pub(crate) cwd: String,
    pub(crate) created_at: i64,
    pub(crate) last_ts: i64,
    messages: Vec<TranscriptMessage>,
    pub(crate) model: Option<String>,
    pub(crate) tokens_used: Option<i64>,
    pub(crate) unknown_lines: u32,
    /// toolCallId → (message index, tool_calls index), used to back-fill
    /// toolResult rows.
    tool_index: HashMap<String, (usize, usize)>,
    pub(crate) update: crate::live::DecoderUpdate,
}

impl PiSession {
    pub(crate) fn new() -> Self {
        Self {
            session_id: None,
            cwd: String::new(),
            created_at: 0,
            last_ts: 0,
            messages: Vec::new(),
            model: None,
            tokens_used: None,
            unknown_lines: 0,
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

    pub(crate) fn snapshot_messages(&self) -> Vec<TranscriptMessage> {
        self.messages.clone()
    }

    /// live incremental channel: total message count, consistent with the
    /// `snapshot_messages` length.
    pub(crate) fn live_message_count(&self) -> usize {
        self.messages.len()
    }

    /// live incremental channel: clone a single message (index space of
    /// `live_message_count`).
    pub(crate) fn live_message_at(&self, index: usize) -> Option<TranscriptMessage> {
        self.messages.get(index).cloned()
    }

    pub(crate) fn feed_line(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        let row: serde_json::Value = match serde_json::from_str(line) {
            Ok(row) => row,
            Err(_) => {
                self.unknown_lines += 1;
                return;
            }
        };
        match row.get("type").and_then(|value| value.as_str()) {
            Some("session") => {
                if let Some(id) = row.get("id").and_then(|value| value.as_str()) {
                    self.session_id = Some(id.to_string());
                }
                if let Some(cwd) = row.get("cwd").and_then(|value| value.as_str()) {
                    self.cwd = cwd.to_string();
                }
                if let Some(timestamp) = row.get("timestamp").and_then(|value| value.as_str()) {
                    self.created_at = iso_ms(timestamp);
                }
            }
            Some("message") => {
                let Some(message) = row.get("message") else {
                    self.unknown_lines += 1;
                    return;
                };
                let timestamp = row
                    .get("timestamp")
                    .and_then(|value| value.as_str())
                    .map(iso_ms)
                    .unwrap_or(0);
                self.last_ts = self.last_ts.max(timestamp);
                let content = message.get("content").unwrap_or(&serde_json::Value::Null);
                match message.get("role").and_then(|value| value.as_str()) {
                    Some("user") => {
                        let text = blocks_text(content);
                        if !text.is_empty() {
                            self.push_message(text_msg(Role::User, &text, timestamp));
                        }
                    }
                    Some("assistant") => {
                        let text = blocks_text(content);
                        let mut tools: Vec<ToolCallView> = Vec::new();
                        for block in content.as_array().into_iter().flatten() {
                            if block.get("type").and_then(|value| value.as_str())
                                == Some("toolCall")
                            {
                                let id = block
                                    .get("id")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or_default();
                                let name = block
                                    .get("name")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or_default();
                                let input = block
                                    .get("arguments")
                                    .cloned()
                                    .unwrap_or(serde_json::Value::Null);
                                tools.push(tool_call_view(
                                    id.to_string(),
                                    name,
                                    &input,
                                    None,
                                    false,
                                ));
                            }
                        }
                        if text.is_empty() && tools.is_empty() {
                            return;
                        }
                        let model = message
                            .get("model")
                            .and_then(|value| value.as_str())
                            .map(String::from);
                        if model.is_some() {
                            self.model = model.clone();
                        }
                        if let Some(total) = message
                            .pointer("/usage/totalTokens")
                            .and_then(|value| value.as_i64())
                        {
                            self.tokens_used = Some(total);
                        }
                        // Consecutive assistant lines (separated only by
                        // toolResult) merge into one: the detail page presents
                        // one assistant message per agentic round.
                        if !matches!(self.messages.last(), Some(message)
                            if message.role == Role::Assistant)
                        {
                            self.push_message(text_msg(Role::Assistant, "", timestamp));
                        }
                        let base = self.messages.len() - 1;
                        let last = &mut self.messages[base];
                        // After merging, clamp against MAX_MSG_TEXT overall
                        // (the whole round becomes one message, so the
                        // single-line text_msg clip does not apply).
                        if !text.is_empty() && last.text.len() < MAX_MSG_TEXT {
                            if !last.text.is_empty() {
                                last.text.push_str("\n\n");
                            }
                            last.text.push_str(&text);
                            if last.text.len() > MAX_MSG_TEXT {
                                let (clipped, _) = clip(&last.text, MAX_MSG_TEXT);
                                last.text = clipped;
                                last.truncated = true;
                            }
                        }
                        if model.is_some() {
                            last.model = model;
                        }
                        for tool_call in tools {
                            self.tool_index
                                .insert(tool_call.id.clone(), (base, last.tool_calls.len()));
                            last.tool_calls.push(tool_call);
                        }
                        self.update.changed.push(base);
                    }
                    Some("toolResult") => {
                        let Some(call_id) =
                            message.get("toolCallId").and_then(|value| value.as_str())
                        else {
                            return;
                        };
                        if let Some(&(message_index, tool_index_position)) =
                            self.tool_index.get(call_id)
                        {
                            let tool_call =
                                &mut self.messages[message_index].tool_calls[tool_index_position];
                            let text = blocks_text(content);
                            if !text.is_empty() {
                                tool_call.output = Some(clip(&text, MAX_TOOL_IO).0);
                            }
                            if message.get("isError").and_then(|value| value.as_bool())
                                == Some(true)
                            {
                                tool_call.is_error = true;
                            }
                            self.update.changed.push(message_index);
                        }
                    }
                    _ => {
                        self.unknown_lines += 1;
                    }
                }
            }
            // Known non-content lines (model/thinking-level switches etc.)
            // skipped silently.
            Some("model_change") | Some("thinking_level_change") => {}
            _ => {
                self.unknown_lines += 1;
            }
        }
    }

    pub(crate) fn finish(self) -> PiParse {
        PiParse {
            session_id: self.session_id,
            cwd: self.cwd,
            created_at: self.created_at,
            last_ts: self.last_ts,
            messages: self.messages,
            model: self.model,
            tokens_used: self.tokens_used,
            unknown_lines: self.unknown_lines,
        }
    }
}

fn parse_jsonl(path: &Path) -> Result<PiParse> {
    let file = fs::File::open(path)?;
    let reader = BufReader::with_capacity(1 << 20, file);
    let mut session = PiSession::new();
    for line in reader.lines() {
        match line {
            Ok(line) => session.feed_line(&line),
            Err(_) => session.count_unknown_line(),
        }
    }
    Ok(session.finish())
}

fn build_meta(agent: AgentId, reference: &SessionFileRef, parsed: &PiParse) -> SessionMeta {
    let native = parsed
        .session_id
        .clone()
        .unwrap_or_else(|| reference.native_id.clone());
    let title = title_from_messages(&parsed.messages).unwrap_or_else(|| UNTITLED.to_string());
    SessionMeta {
        key: format!("{}:{native}", agent.as_str()),
        id: native,
        agent,
        title,
        project_path: parsed.cwd.clone(),
        project_name: project_name_of(&parsed.cwd),
        file_path: reference.file_path.clone(),
        created_at: if parsed.created_at > 0 {
            parsed.created_at
        } else {
            reference.mtime_ms
        },
        updated_at: if parsed.last_ts > 0 {
            parsed.last_ts
        } else {
            reference.mtime_ms
        },
        message_count: parsed
            .messages
            .iter()
            .filter(|message| message.kind == MessageKind::Text)
            .count() as i64,
        size_bytes: reference.size,
        git_branch: None,
        model: parsed.model.clone(),
        tokens_used: parsed.tokens_used,
        archived: false,
        source: None,
    }
}

impl AgentHistoryAdapter for PiAdapter {
    fn agent(&self) -> AgentId {
        self.agent
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        list_jsonl_refs(&self.root, self.agent, native_id_of)
    }

    fn file_ref(&self, path: &Path) -> Option<SessionFileRef> {
        let mut reference = default_file_ref(self.agent, path)?;
        reference.native_id = native_id_of(&reference.native_id);
        Some(reference)
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession> {
        let parsed = parse_jsonl(Path::new(&reference.file_path))?;
        let meta = build_meta(self.agent, reference, &parsed);
        let units = units_from_messages(&parsed.messages);
        Ok(ParsedSession {
            meta,
            units,
            unknown_line_count: parsed.unknown_lines,
        })
    }

    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript> {
        let parsed = parse_jsonl(Path::new(&reference.file_path))?;
        Ok(ParsedTranscript::simple(
            build_meta(self.agent, reference, &parsed),
            parsed.messages,
            parsed.unknown_lines,
        ))
    }

    fn with_custom_root(&self, dir: PathBuf) -> Box<dyn AgentHistoryAdapter> {
        let nested = dir.join("agent").join("sessions");
        let root = if nested.is_dir() { nested } else { dir };
        Box::new(Self {
            agent: self.agent,
            root,
        })
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        vec![self.root.clone()]
    }
}
