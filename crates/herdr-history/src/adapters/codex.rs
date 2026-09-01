// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

use super::parse_utils::*;
use super::sqlite_ro::open_sqlite_ro;
use super::{units_from_messages, AgentHistoryAdapter};
use crate::models::*;
use anyhow::Result;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

pub struct CodexAdapter {
    sessions_dir: PathBuf,
    archived_dir: PathBuf,
    state_db: PathBuf,
    scan_sessions: bool,
    scan_archived: bool,
}

impl CodexAdapter {
    pub fn new() -> Self {
        let root = std::env::var_os("CODEX_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .filter(|root| {
                root.join("sessions").is_dir() || root.join("archived_sessions").is_dir()
            })
            .unwrap_or_else(|| home_dir().join(".codex"));
        Self {
            sessions_dir: root.join("sessions"),
            archived_dir: root.join("archived_sessions"),
            state_db: root.join("state_5.sqlite"),
            scan_sessions: true,
            scan_archived: true,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: PathBuf) -> Self {
        Self {
            sessions_dir: root.join("sessions"),
            archived_dir: root.join("archived_sessions"),
            state_db: root.join("state_5.sqlite"),
            scan_sessions: true,
            scan_archived: true,
        }
    }
}

impl Default for CodexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn is_rollout_store(dir: &Path) -> bool {
    !dir.join("sessions").is_dir()
        && !dir.join("archived_sessions").is_dir()
        && fs::read_dir(dir)
            .map(|entries| {
                entries.flatten().any(|entry| {
                    let name = entry.file_name();
                    let Some(name) = name.to_str() else {
                        return false;
                    };
                    (entry.path().is_dir()
                        && name.len() == 4
                        && name.bytes().all(|byte| byte.is_ascii_digit()))
                        || (name.starts_with("rollout-") && name.ends_with(".jsonl"))
                })
            })
            .unwrap_or(false)
}

pub(crate) fn normalize_custom_root(dir: PathBuf) -> PathBuf {
    let name = dir.file_name().and_then(|name| name.to_str());
    let looks_data_dir =
        is_rollout_store(&dir) || matches!(name, Some("sessions") | Some("archived_sessions"));
    if looks_data_dir {
        if let Some(parent) = dir.parent() {
            let sibling = match name {
                Some("sessions") => parent.join("archived_sessions").is_dir(),
                Some("archived_sessions") => parent.join("sessions").is_dir(),
                _ => parent.join("sessions").is_dir() || parent.join("archived_sessions").is_dir(),
            };
            if parent.join("state_5.sqlite").is_file() || sibling {
                return parent.to_path_buf();
            }
        }
    }
    dir
}

#[derive(Debug)]
struct ThreadRow {
    id: String,
    rollout_path: String,
    pub(crate) cwd: String,
    title: String,
    name: Option<String>,
    pub(crate) tokens_used: Option<i64>,
    archived: bool,
    pub(crate) git_branch: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) source: Option<String>,
    created_at_ms: Option<i64>,
    updated_at_ms: Option<i64>,
}

fn read_threads(state_db: &Path) -> Option<Vec<ThreadRow>> {
    let read = |conn: &Connection| -> rusqlite::Result<Vec<ThreadRow>> {
        let mut statement = conn.prepare(
            "SELECT id, rollout_path, cwd, title, name, tokens_used, archived,
                    git_branch, model, source, created_at_ms, updated_at_ms
             FROM threads",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ThreadRow {
                id: row.get(0)?,
                rollout_path: row.get(1)?,
                cwd: row.get(2)?,
                title: row.get(3)?,
                name: row.get(4)?,
                tokens_used: row.get(5)?,
                archived: row.get::<_, i64>(6)? == 1,
                git_branch: row.get(7)?,
                model: row.get(8)?,
                source: row.get(9)?,
                created_at_ms: row.get(10)?,
                updated_at_ms: row.get(11)?,
            })
        })?;
        rows.collect()
    };
    let database = open_sqlite_ro(state_db, "codex")?;
    read(&database.conn).ok()
}

pub(crate) struct CodexParse {
    pub(crate) messages: Vec<TranscriptMessage>,
    cwd: String,
    git_branch: Option<String>,
    model: Option<String>,
    source: Option<String>,
    tokens_used: i64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) unknown_lines: u32,
}

fn friendly_source(originator: &str) -> Option<String> {
    Some(match originator {
        "codex_cli_rs" | "codex-tui" => "CLI".to_string(),
        "codex_exec" => "exec".to_string(),
        "codex_vscode" => "IDE extension".to_string(),
        "codex_work_desktop" => "Codex Desktop".to_string(),
        "" => return None,
        other => other.to_string(),
    })
}

/// Line-level interpretation state shared by full parsing and live incremental
/// decoding: `parse_rollout` and `crate::live` feed the same `feed_line` path,
/// so both sides agree by construction. `event_fallback` (an event_msg
/// fallback view when no real content exists) is presented as one or the
/// other only at `resolved`/`finish`; arrival of a real message switches the
/// whole view back to the main one.
pub(crate) struct CodexSession {
    messages: Vec<TranscriptMessage>,
    event_fallback: Vec<TranscriptMessage>,
    tool_index: HashMap<String, (usize, usize)>,
    saw_session_meta: bool,
    pub(crate) cwd: String,
    pub(crate) git_branch: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) tokens_used: i64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) unknown_lines: u32,
    pub(crate) update: crate::live::DecoderUpdate,
}

impl CodexSession {
    pub(crate) fn new() -> Self {
        Self {
            messages: Vec::new(),
            event_fallback: Vec::new(),
            tool_index: HashMap::new(),
            saw_session_meta: false,
            cwd: String::new(),
            git_branch: None,
            model: None,
            source: None,
            tokens_used: 0,
            created_at: 0,
            updated_at: 0,
            unknown_lines: 0,
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

    fn push_fallback(&mut self, mut message: TranscriptMessage) {
        message.seq = self.event_fallback.len() as i64;
        self.event_fallback.push(message);
    }

    /// Whether the fallback view is currently active (no real content and a
    /// non-empty event fallback).
    pub(crate) fn fallback_active(&self) -> bool {
        let has_real = self
            .messages
            .iter()
            .any(|message| message.kind == MessageKind::Text && !message.text.is_empty());
        !has_real && !self.event_fallback.is_empty()
    }

    /// The message view that should currently be presented (including the
    /// fallback switch).
    pub(crate) fn snapshot_messages(&self) -> Vec<TranscriptMessage> {
        if self.fallback_active() {
            self.event_fallback.clone()
        } else {
            self.messages.clone()
        }
    }

    /// live incremental channel: total message count, consistent with the
    /// `snapshot_messages` length.
    pub(crate) fn live_message_count(&self) -> usize {
        if self.fallback_active() {
            self.event_fallback.len()
        } else {
            self.messages.len()
        }
    }

    /// live incremental channel: clone a single message (index space of
    /// `live_message_count`).
    pub(crate) fn live_message_at(&self, index: usize) -> Option<TranscriptMessage> {
        if self.fallback_active() {
            self.event_fallback.get(index).cloned()
        } else {
            self.messages.get(index).cloned()
        }
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
        let timestamp = row.get("timestamp").map(to_epoch_ms).unwrap_or(0);
        if timestamp > 0 {
            if self.created_at == 0 {
                self.created_at = timestamp;
            }
            self.updated_at = self.updated_at.max(timestamp);
        }
        let row_type = row.get("type").and_then(Value::as_str).unwrap_or("");
        let Some(payload) = row.get("payload") else {
            if !matches!(row_type, "compacted" | "world_state") {
                self.unknown_lines += 1;
            }
            return;
        };

        match row_type {
            "session_meta" => {
                if !self.saw_session_meta {
                    self.saw_session_meta = true;
                    self.cwd = payload
                        .get("cwd")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    self.source = payload
                        .get("originator")
                        .and_then(Value::as_str)
                        .and_then(friendly_source);
                    self.git_branch = payload
                        .get("git")
                        .and_then(|git| git.get("branch"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
            }
            "turn_context" => {
                if self.cwd.is_empty() {
                    self.cwd = payload
                        .get("cwd")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                }
                if let Some(model_name) = payload.get("model").and_then(Value::as_str) {
                    self.model = Some(model_name.to_string());
                }
            }
            "response_item" => parse_response_item(payload, timestamp, self),
            "event_msg" => match payload.get("type").and_then(Value::as_str).unwrap_or("") {
                "token_count" => {
                    if let Some(total) = payload
                        .get("info")
                        .and_then(|info| info.get("total_token_usage"))
                        .and_then(|usage| usage.get("total_tokens"))
                        .and_then(Value::as_i64)
                    {
                        self.tokens_used = total;
                    }
                }
                "user_message" => {
                    if let Some(text) = payload.get("message").and_then(Value::as_str) {
                        if !text.trim().is_empty() {
                            self.push_fallback(message(
                                Role::User,
                                user_kind(text),
                                text.trim(),
                                timestamp,
                            ));
                        }
                    }
                }
                "agent_message" => {
                    if let Some(text) = payload.get("message").and_then(Value::as_str) {
                        if !text.trim().is_empty() {
                            self.push_fallback(message(
                                Role::Assistant,
                                MessageKind::Text,
                                text.trim(),
                                timestamp,
                            ));
                        }
                    }
                }
                _ => {}
            },
            "compacted" => self.push_message(message(
                Role::System,
                MessageKind::CompactSummary,
                "── Context compacted ──",
                timestamp,
            )),
            "world_state" => {}
            _ => self.unknown_lines += 1,
        }
    }

    pub(crate) fn finish(self) -> CodexParse {
        let messages = self.snapshot_messages();
        CodexParse {
            messages,
            cwd: self.cwd,
            git_branch: self.git_branch,
            model: self.model,
            source: self.source,
            tokens_used: self.tokens_used,
            created_at: self.created_at,
            updated_at: self.updated_at,
            unknown_lines: self.unknown_lines,
        }
    }
}

fn parse_rollout(path: &Path) -> Result<CodexParse> {
    let file = fs::File::open(path)?;
    let reader = BufReader::with_capacity(1 << 20, file);
    let mut session = CodexSession::new();
    for line in reader.lines() {
        match line {
            Ok(line) => session.feed_line(&line),
            Err(_) => session.count_unknown_line(),
        }
    }
    Ok(session.finish())
}

fn parse_response_item(payload: &Value, timestamp: i64, session: &mut CodexSession) {
    match payload.get("type").and_then(Value::as_str).unwrap_or("") {
        "message" => {
            let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
            let mut parts = Vec::new();
            match payload.get("content") {
                Some(Value::Array(blocks)) => {
                    for block in blocks {
                        if matches!(
                            block.get("type").and_then(Value::as_str),
                            Some("input_text") | Some("output_text") | Some("text")
                        ) {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                parts.push(text.to_string());
                            }
                        }
                    }
                }
                Some(Value::String(text)) => parts.push(text.clone()),
                _ => {}
            }
            let text = parts.join("\n\n").trim().to_string();
            if text.is_empty() {
                return;
            }
            session.push_message(match role {
                "user" => message(Role::User, user_kind(&text), &text, timestamp),
                "assistant" => message(Role::Assistant, MessageKind::Text, &text, timestamp),
                _ => message(Role::System, MessageKind::Meta, &text, timestamp),
            });
        }
        "reasoning" => {
            let thinking = payload
                .get("summary")
                .and_then(Value::as_array)
                .map(|summary| {
                    summary
                        .iter()
                        .filter_map(|item| item.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n\n")
                })
                .filter(|text| !text.is_empty());
            if let Some(thinking) = thinking {
                let thinking = clip(&thinking, MAX_TOOL_IO).0;
                let merged = {
                    let messages = &mut session.messages;
                    let mut merged = false;
                    if let Some(last) = messages.last_mut() {
                        if last.role == Role::Assistant
                            && last.text.is_empty()
                            && last.thinking.is_none()
                        {
                            last.thinking = Some(thinking.clone());
                            session.update.changed.push(messages.len() - 1);
                            merged = true;
                        }
                    }
                    merged
                };
                if !merged {
                    let mut host = message(Role::Assistant, MessageKind::Text, "", timestamp);
                    host.thinking = Some(thinking);
                    session.push_message(host);
                }
            }
        }
        "function_call" | "custom_tool_call" | "local_shell_call" => {
            let call_id = payload
                .get("call_id")
                .or_else(|| payload.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let name = payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("exec")
                .to_string();
            let raw_input = payload
                .get("arguments")
                .and_then(Value::as_str)
                .or_else(|| payload.get("input").and_then(Value::as_str))
                .map(str::to_string)
                .unwrap_or_else(|| {
                    payload
                        .get("action")
                        .and_then(|action| serde_json::to_string(action).ok())
                        .unwrap_or_default()
                });
            let preview_source = serde_json::from_str(&raw_input)
                .unwrap_or_else(|_| Value::String(raw_input.clone()));
            let call = ToolCallView {
                id: call_id.clone(),
                name,
                input_preview: make_preview(&preview_source),
                input: (!raw_input.is_empty()).then(|| clip(&raw_input, MAX_TOOL_IO).0),
                output: None,
                is_error: false,
                sidechain_ref: None,
            };
            let need_host = !matches!(
                session.messages.last(),
                Some(last) if last.role == Role::Assistant && last.kind == MessageKind::Text
            );
            if need_host {
                session.push_message(message(Role::Assistant, MessageKind::Text, "", timestamp));
            }
            let message_index = session.messages.len().saturating_sub(1);
            if let Some(host) = session.messages.last_mut() {
                host.tool_calls.push(call);
                if !call_id.is_empty() {
                    session
                        .tool_index
                        .insert(call_id, (message_index, host.tool_calls.len() - 1));
                }
                session.update.changed.push(message_index);
            }
        }
        "function_call_output" | "custom_tool_call_output" => {
            let call_id = payload.get("call_id").and_then(Value::as_str).unwrap_or("");
            if let Some(&(message_index, tool_index_in_message)) = session.tool_index.get(call_id) {
                let output = match payload.get("output") {
                    Some(Value::String(text)) => text.clone(),
                    Some(value @ Value::Object(_)) => value
                        .get("content")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| serde_json::to_string(value).ok())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                if let Some(tool_call) = session
                    .messages
                    .get_mut(message_index)
                    .and_then(|message| message.tool_calls.get_mut(tool_index_in_message))
                {
                    tool_call.output = Some(clip(&output, MAX_TOOL_IO).0);
                    session.update.changed.push(message_index);
                }
            }
        }
        _ => {}
    }
}

fn message(role: Role, kind: MessageKind, text: &str, timestamp: i64) -> TranscriptMessage {
    let (text, truncated) = clip(text, MAX_MSG_TEXT);
    TranscriptMessage {
        seq: 0,
        role,
        kind,
        text,
        truncated,
        tool_calls: Vec::new(),
        thinking: None,
        timestamp: (timestamp > 0).then_some(timestamp),
        model: None,
    }
}

pub(crate) fn rollout_native_id(stem: &str) -> String {
    if let Some(rest) = stem.strip_prefix("rollout-") {
        if rest.len() > 20 && rest.as_bytes().get(10) == Some(&b'T') {
            return rest[20..].to_string();
        }
    }
    stem.to_string()
}

fn build_meta(reference: &SessionFileRef, parsed: &CodexParse, archived_dir: &Path) -> SessionMeta {
    SessionMeta {
        key: format!("codex:{}", reference.native_id),
        id: reference.native_id.clone(),
        agent: AgentId::Codex,
        title: title_from_messages(&parsed.messages).unwrap_or_else(|| UNTITLED.to_string()),
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
        archived: super::path_owns(
            archived_dir.to_string_lossy().as_ref(),
            &reference.file_path,
        ),
        source: parsed.source.clone(),
    }
}

impl AgentHistoryAdapter for CodexAdapter {
    fn agent(&self) -> AgentId {
        AgentId::Codex
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        let mut references = if self.scan_sessions {
            list_jsonl_refs(&self.sessions_dir, AgentId::Codex, rollout_native_id)?
        } else {
            Vec::new()
        };
        if self.scan_archived {
            references.extend(list_jsonl_refs(
                &self.archived_dir,
                AgentId::Codex,
                rollout_native_id,
            )?);
        }
        Ok(references)
    }

    fn quick_meta(&self, references: &[SessionFileRef]) -> Option<HashMap<String, SessionMeta>> {
        let rows = read_threads(&self.state_db)?;
        let rows_by_path = rows
            .iter()
            .map(|row| (row.rollout_path.as_str(), row))
            .collect::<HashMap<_, _>>();
        let mut output = HashMap::new();
        for reference in references {
            let Some(row) = rows_by_path.get(reference.file_path.as_str()) else {
                continue;
            };
            let title = row
                .name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .map(str::to_string)
                .or_else(|| {
                    if is_injected_user_content(&row.title) {
                        None
                    } else {
                        let title = clean_title_candidate(&row.title);
                        (!title.is_empty()).then_some(title)
                    }
                })
                .unwrap_or_else(|| UNTITLED.to_string());
            output.insert(
                reference.file_path.clone(),
                SessionMeta {
                    key: format!("codex:{}", row.id),
                    id: row.id.clone(),
                    agent: AgentId::Codex,
                    title,
                    project_path: row.cwd.clone(),
                    project_name: project_name_of(&row.cwd),
                    file_path: reference.file_path.clone(),
                    created_at: row.created_at_ms.unwrap_or(reference.mtime_ms),
                    updated_at: row.updated_at_ms.unwrap_or(reference.mtime_ms),
                    message_count: 0,
                    size_bytes: reference.size,
                    git_branch: row.git_branch.clone(),
                    model: row.model.clone(),
                    tokens_used: row.tokens_used,
                    archived: row.archived,
                    source: row.source.clone(),
                },
            );
        }
        Some(output)
    }

    fn merge_quick_meta(&self, mut parsed: SessionMeta, quick: &SessionMeta) -> SessionMeta {
        if quick.title != UNTITLED {
            parsed.title = quick.title.clone();
        }
        if parsed.project_path.is_empty() {
            parsed.project_path = quick.project_path.clone();
            parsed.project_name = quick.project_name.clone();
        }
        parsed.archived = quick.archived;
        if quick.git_branch.is_some() {
            parsed.git_branch = quick.git_branch.clone();
        }
        if quick.source.is_some() {
            parsed.source = quick.source.clone();
        }
        if parsed.model.is_none() {
            parsed.model = quick.model.clone();
        }
        if parsed.tokens_used.is_none() {
            parsed.tokens_used = quick.tokens_used;
        }
        parsed
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession> {
        let parsed = parse_rollout(Path::new(&reference.file_path))?;
        Ok(ParsedSession {
            meta: build_meta(reference, &parsed, &self.archived_dir),
            units: units_from_messages(&parsed.messages),
            unknown_line_count: parsed.unknown_lines,
        })
    }

    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript> {
        let parsed = parse_rollout(Path::new(&reference.file_path))?;
        Ok(ParsedTranscript::simple(
            build_meta(reference, &parsed, &self.archived_dir),
            parsed.messages,
            parsed.unknown_lines,
        ))
    }

    fn with_custom_root(&self, dir: PathBuf) -> Box<dyn AgentHistoryAdapter> {
        let name = dir.file_name().and_then(|name| name.to_str());
        let (sessions_dir, archived_dir) = match name {
            Some("archived_sessions") => (dir.join("sessions"), dir.clone()),
            Some("sessions") => (dir.clone(), dir.join("archived_sessions")),
            _ if is_rollout_store(&dir) => (dir.clone(), dir.join("archived_sessions")),
            _ => (dir.join("sessions"), dir.join("archived_sessions")),
        };
        Box::new(Self {
            sessions_dir,
            archived_dir,
            state_db: dir.join("state_5.sqlite"),
            scan_sessions: true,
            scan_archived: true,
        })
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::with_capacity(2);
        if self.scan_sessions {
            roots.push(self.sessions_dir.clone());
        }
        if self.scan_archived {
            roots.push(self.archived_dir.clone());
        }
        roots
    }

    fn supports_individual_root_removal(&self) -> bool {
        true
    }

    fn excluding_data_roots(&self, roots: &[PathBuf]) -> Option<Box<dyn AgentHistoryAdapter>> {
        Some(Box::new(Self {
            sessions_dir: self.sessions_dir.clone(),
            archived_dir: self.archived_dir.clone(),
            state_db: self.state_db.clone(),
            scan_sessions: self.scan_sessions && !roots.contains(&self.sessions_dir),
            scan_archived: self.scan_archived && !roots.contains(&self.archived_dir),
        }))
    }
}
