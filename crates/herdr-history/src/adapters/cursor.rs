// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

use super::parse_utils::*;
use super::{units_from_messages, AgentHistoryAdapter};
use crate::models::*;
use anyhow::Result;
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

pub struct CursorAdapter {
    root: PathBuf,
}

impl CursorAdapter {
    pub fn new() -> Self {
        Self {
            root: home_dir().join(".cursor/projects"),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: PathBuf) -> Self {
        Self { root }
    }
}

impl Default for CursorAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn decode_slug(slug: &str) -> String {
    let parts = slug.split('-').collect::<Vec<_>>();
    fn dfs(base: PathBuf, parts: &[&str]) -> Option<PathBuf> {
        if parts.is_empty() {
            return Some(base);
        }
        let mut segment = String::new();
        for index in 0..parts.len() {
            if index > 0 {
                segment.push('-');
            }
            segment.push_str(parts[index]);
            let candidate = base.join(&segment);
            if candidate.is_dir() {
                if let Some(hit) = dfs(candidate, &parts[index + 1..]) {
                    return Some(hit);
                }
            }
        }
        None
    }
    dfs(PathBuf::from("/"), &parts)
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("/{}", slug.replace('-', "/")))
}

fn cursor_ts_ms(value: &str) -> i64 {
    (|| -> Option<i64> {
        let (datetime, timezone) = value.rsplit_once(" (")?;
        let naive =
            chrono::NaiveDateTime::parse_from_str(datetime.trim(), "%A, %b %d, %Y, %I:%M %p")
                .ok()?;
        let offset = timezone.trim_end_matches(')').strip_prefix("UTC")?;
        let (sign, rest) = match offset.as_bytes().first()? {
            b'+' => (1_i32, &offset[1..]),
            b'-' => (-1_i32, &offset[1..]),
            _ => (1_i32, offset),
        };
        let seconds = match rest.split_once(':') {
            Some((hours, minutes)) => {
                hours.parse::<i32>().ok()? * 3600 + minutes.parse::<i32>().ok()? * 60
            }
            None => rest.parse::<i32>().ok()? * 3600,
        };
        let fixed = chrono::FixedOffset::east_opt(sign * seconds)?;
        use chrono::TimeZone as _;
        Some(
            fixed
                .from_local_datetime(&naive)
                .single()?
                .timestamp_millis(),
        )
    })()
    .unwrap_or(0)
}

struct CursorParse {
    messages: Vec<TranscriptMessage>,
    created_at: i64,
    updated_at: i64,
    unknown_lines: u32,
}

#[derive(Default)]
pub(crate) struct PendingAssistant {
    pub(crate) text: Vec<String>,
    pub(crate) tool_calls: Vec<ToolCallView>,
    pub(crate) timestamp: Option<i64>,
}

/// Current projection of the pending group (shared by flush and the live
/// snapshot); returns None for an empty group.
pub(crate) fn pending_message(pending: &PendingAssistant) -> Option<TranscriptMessage> {
    let text = pending.text.join("\n\n");
    if text.is_empty() && pending.tool_calls.is_empty() {
        return None;
    }
    let (text, truncated) = clip(&text, MAX_MSG_TEXT);
    Some(TranscriptMessage {
        seq: 0,
        role: Role::Assistant,
        kind: MessageKind::Text,
        text,
        truncated,
        tool_calls: pending.tool_calls.clone(),
        thinking: None,
        timestamp: pending.timestamp,
        model: None,
    })
}

/// Reusable replay state for Cursor agent-transcripts (PEX-3 / plan §7.1:
/// full parsing and live increments share the same line-level state machine).
/// Consecutive assistant lines merge into `pending` (text/tool accumulation);
/// user lines / `turn_ended` flush. During live, pending is visible as the
/// tail projection (same as the Claude pending pattern; streaming growth
/// across syncs is backstopped by a tail signature).
pub(crate) struct CursorSession {
    pub(crate) messages: Vec<TranscriptMessage>,
    pub(crate) pending: Option<PendingAssistant>,
    pub(crate) update: crate::live::DecoderUpdate,
    pub(crate) unknown_lines: u32,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

impl CursorSession {
    pub(crate) fn fresh() -> Self {
        Self {
            messages: Vec::new(),
            pending: None,
            update: crate::live::DecoderUpdate::default(),
            unknown_lines: 0,
            created_at: 0,
            updated_at: 0,
        }
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
        let Some(mut message) = pending_message(&pending) else {
            return;
        };
        message.tool_calls = pending.tool_calls;
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
        let Some(role) = row.get("role").and_then(Value::as_str) else {
            if row.get("type").and_then(Value::as_str) == Some("turn_ended") {
                self.flush_pending();
            } else {
                self.unknown_lines += 1;
            }
            return;
        };
        let blocks = row
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        match role {
            "user" => {
                self.flush_pending();
                let mut parts = Vec::new();
                let mut timestamp = 0;
                for block in &blocks {
                    if block.get("type").and_then(Value::as_str) != Some("text") {
                        continue;
                    }
                    let Some(raw) = block.get("text").and_then(Value::as_str) else {
                        continue;
                    };
                    if let Some(value) = extract_tag(raw, "timestamp") {
                        let parsed = cursor_ts_ms(&value);
                        if parsed > 0 {
                            timestamp = parsed;
                        }
                    }
                    let body = extract_tag(raw, "user_query").unwrap_or_else(|| raw.to_string());
                    if !body.trim().is_empty() {
                        parts.push(body.trim().to_string());
                    }
                }
                let text = parts.join("\n\n");
                if text.is_empty() {
                    return;
                }
                if timestamp > 0 {
                    if self.created_at == 0 {
                        self.created_at = timestamp;
                    }
                    self.updated_at = self.updated_at.max(timestamp);
                }
                self.push_message(text_msg(Role::User, &text, timestamp));
            }
            "assistant" => {
                let pending = self.pending.get_or_insert_with(PendingAssistant::default);
                for block in &blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                if !text.trim().is_empty() {
                                    pending.text.push(text.to_string());
                                }
                            }
                        }
                        Some("tool_use") => {
                            let input = block.get("input").cloned().unwrap_or(Value::Null);
                            let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                            pending.tool_calls.push(tool_call_view(
                                String::new(),
                                name,
                                &input,
                                None,
                                false,
                            ));
                        }
                        _ => {}
                    }
                }
            }
            _ => self.unknown_lines += 1,
        }
    }

    pub(crate) fn count_unknown_line(&mut self) {
        self.unknown_lines += 1;
    }

    /// EOF flush (full-parse wrap-up).
    fn finish(&mut self) {
        self.flush_pending();
    }

    /// live tail projection (visible as the last message while pending is
    /// unflushed).
    pub(crate) fn live_message_count(&self) -> usize {
        self.messages.len() + self.pending.as_ref().and_then(pending_message).is_some() as usize
    }

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
}

fn parse_jsonl(path: &Path) -> Result<CursorParse> {
    let file = fs::File::open(path)?;
    let reader = BufReader::with_capacity(1 << 20, file);
    let mut session = CursorSession::fresh();

    for line in reader.lines() {
        match line {
            Ok(line) => session.feed_line(&line),
            Err(_) => session.count_unknown_line(),
        }
    }
    session.finish();
    assign_seq(&mut session.messages);
    Ok(CursorParse {
        messages: session.messages,
        created_at: session.created_at,
        updated_at: session.updated_at,
        unknown_lines: session.unknown_lines,
    })
}

fn subagents_dir(reference: &SessionFileRef) -> PathBuf {
    Path::new(&reference.file_path)
        .parent()
        .unwrap_or(Path::new("."))
        .join("subagents")
}

fn build_meta(reference: &SessionFileRef, parsed: &CursorParse) -> SessionMeta {
    let cwd = Path::new(&reference.file_path)
        .ancestors()
        .nth(3)
        .and_then(Path::file_name)
        .map(|name| decode_slug(&name.to_string_lossy()))
        .unwrap_or_default();
    SessionMeta {
        key: format!("cursor:{}", reference.native_id),
        id: reference.native_id.clone(),
        agent: AgentId::Cursor,
        title: title_from_messages(&parsed.messages).unwrap_or_else(|| UNTITLED.to_string()),
        project_path: cwd.clone(),
        project_name: project_name_of(&cwd),
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
        git_branch: None,
        model: None,
        tokens_used: None,
        archived: false,
        source: None,
    }
}

impl AgentHistoryAdapter for CursorAdapter {
    fn agent(&self) -> AgentId {
        AgentId::Cursor
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        let mut references = Vec::new();
        // Missing root = not installed/deleted: a legitimate empty set. Root
        // exists but read fails = propagate the error.
        if !self.root.is_dir() {
            return Ok(references);
        }
        let projects = fs::read_dir(&self.root)
            .map_err(|error| anyhow::anyhow!("read {}: {error}", self.root.display()))?;
        for project in projects.flatten() {
            let transcripts = project.path().join("agent-transcripts");
            let Ok(sessions) = fs::read_dir(transcripts) else {
                continue;
            };
            for session in sessions.flatten() {
                let Ok(entries) = fs::read_dir(session.path()) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !name.ends_with(".jsonl") || name.starts_with('.') {
                        continue;
                    }
                    let Ok(meta) = entry.metadata() else {
                        continue;
                    };
                    if !meta.is_file() || meta.len() == 0 {
                        continue;
                    }
                    references.push(SessionFileRef {
                        agent: AgentId::Cursor,
                        native_id: name.trim_end_matches(".jsonl").to_string(),
                        file_path: entry.path().to_string_lossy().to_string(),
                        mtime_ms: mtime_ms(&meta),
                        size: meta.len() as i64,
                    });
                }
            }
        }
        Ok(references)
    }

    fn file_ref(&self, path: &Path) -> Option<SessionFileRef> {
        let path_text = path.to_string_lossy();
        if !path_text.contains("/agent-transcripts/") || path_text.contains("/subagents/") {
            return None;
        }
        default_file_ref(self.agent(), path)
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession> {
        let parsed = parse_jsonl(Path::new(&reference.file_path))?;
        Ok(ParsedSession {
            meta: build_meta(reference, &parsed),
            units: units_from_messages(&parsed.messages),
            unknown_line_count: parsed.unknown_lines,
        })
    }

    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript> {
        let parsed = parse_jsonl(Path::new(&reference.file_path))?;
        let sidechains = fs::read_dir(subagents_dir(reference))
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                name.strip_suffix(".jsonl").map(|id| SidechainInfo {
                    id: id.to_string(),
                    agent_type: None,
                    description: None,
                    tool_use_id: None,
                })
            })
            .collect();
        Ok(ParsedTranscript {
            meta: build_meta(reference, &parsed),
            mainline: parsed.messages,
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
        Ok(parse_jsonl(&path)?.messages)
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
