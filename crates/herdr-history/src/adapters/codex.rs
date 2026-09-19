// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

//! [INPUT]: Depends on the Codex rollout JSONL shape (session_meta /
//! turn_context / response_item / event_msg), crate::live's DecoderUpdate +
//! LiveApproval, and the vendored Codex binary's TUI keymap contract
//! (approval.approve|approve_for_session|deny default chords, verifiable in
//! codex-rs tui/src/keymap.rs built_in_defaults) with user overrides from
//! [tui.keymap.approval] in CODEX_HOME/config.toml.
//! [OUTPUT]: CodexAdapter (history scanning/parsing), CodexSession (the
//! line-level interpretation state shared by full parsing and live), and the
//! pending-approval live facts (exec/apply_patch approval requests with
//! replayable key options).
//! [POS]: The Codex provider knowledge authority of shardlane-history: the
//! only place Codex rollout semantics and TUI key bindings are interpreted.
//! Replay options are emitted only when they can be derived from verified
//! rules; anything else is left to Terminal (never-blind-send).
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use super::parse_utils::*;
use super::sqlite_ro::open_sqlite_ro;
use super::{units_from_messages, AgentHistoryAdapter};
use crate::live::{LiveApproval, LiveApprovalOption};
use crate::models::*;
use anyhow::Result;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

// ============================================================================
// Codex TUI 审批键位契约（永不盲发的 keys 来源）
// ============================================================================

/// Codex TUI approval-modal action keys. Defaults are the vendor's
/// `built_in_defaults` (codex-rs tui/src/keymap.rs); the user may rebind via
/// config.toml `[tui.keymap.approval]` — sending a stale default after a
/// rebind would be a blind send, so overrides are loaded when available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexApprovalKeys {
    pub(crate) approve: Vec<u8>,
    pub(crate) approve_for_session: Vec<u8>,
    pub(crate) deny: Vec<u8>,
}

impl CodexApprovalKeys {
    /// Vendor defaults, verified against codex-rs keymap.rs (approve = "y",
    /// approve_for_session = "a", deny = "d").
    pub(crate) fn built_in() -> Self {
        Self {
            approve: vec![b'y'],
            approve_for_session: vec![b'a'],
            deny: vec![b'd'],
        }
    }

    /// Apply parsed config overrides (already scoped to approval actions).
    pub(crate) fn with_overrides(mut self, overrides: &[(String, Vec<u8>)]) -> Self {
        for (action, keys) in overrides {
            match action.as_str() {
                "approve" => self.approve = keys.clone(),
                "approve_for_session" => self.approve_for_session = keys.clone(),
                "deny" => self.deny = keys.clone(),
                _ => {}
            }
        }
        self
    }
}

/// Minimal line scan of config.toml for `[tui.keymap.approval]` bindings
/// (the form the TUI /keymap flow writes) plus `[tui.keymap]` dotted
/// `approval.<action> = "x"` entries. Anything exotic keeps the vendor
/// default — the grid-signature guard still protects the replay, and a
/// stale default hits an unbound key (no-op) rather than a wrong action in
/// the common rebind case.
pub(crate) fn parse_approval_keymap_overrides(config: &str) -> Vec<(String, Vec<u8>)> {
    let mut overrides = Vec::new();
    let mut section = String::new();
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        // Only simple string bindings: name = "x".
        let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
            continue;
        };
        let action = if section == "tui.keymap.approval" {
            key.to_string()
        } else if section == "tui.keymap" {
            match key.strip_prefix("approval.") {
                Some(action) => action.to_string(),
                None => continue,
            }
        } else {
            continue;
        };
        overrides.push((action, inner.as_bytes().to_vec()));
    }
    overrides
}

/// Load the effective approval keys once per process: built-in defaults with
/// CODEX_HOME/config.toml overrides applied when the file is readable.
pub(crate) fn approval_keys() -> CodexApprovalKeys {
    use std::sync::OnceLock;
    static KEYS: OnceLock<CodexApprovalKeys> = OnceLock::new();
    KEYS.get_or_init(|| {
        let config = std::env::var_os("CODEX_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".codex"))
            .join("config.toml");
        let overrides = fs::read_to_string(&config)
            .map(|text| parse_approval_keymap_overrides(&text))
            .unwrap_or_default();
        CodexApprovalKeys::built_in().with_overrides(&overrides)
    })
    .clone()
}

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
    /// Pending approval request (live-ephemeral): set by an
    /// exec/apply_patch approval request event, cleared by the next
    /// turn-progress evidence. Full parsing never surfaces it.
    pub(crate) pending_approval: Option<LiveApproval>,
    /// Approval action keys in effect for this session (loaded from the
    /// vendored TUI keymap defaults + user config on the live path; built-ins
    /// in pure/test constructions).
    pub(crate) approval_keys: CodexApprovalKeys,
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
            pending_approval: None,
            approval_keys: CodexApprovalKeys::built_in(),
        }
    }

    /// Live-path construction: built-in defaults with the user's
    /// [tui.keymap.approval] overrides applied (loaded once per process).
    pub(crate) fn with_configured_approval_keys() -> Self {
        let mut session = Self::new();
        session.approval_keys = approval_keys();
        session
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
        self.track_pending_approval(row_type, payload);

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

/// Turn-progress event types that prove an approval menu is no longer up
/// (the turn moved on, so the pending request resolved — usually by the user
/// answering in the TUI). Anything NOT in this list (e.g. token_count) keeps
/// the pending request alive; the Chat-side grid guard still protects every
/// replay, so a missed clear degrades to Stale, never to a blind send.
fn approval_cleared_by(row_type: &str, event_type: &str) -> bool {
    match row_type {
        "response_item" => true,
        "event_msg" => matches!(
            event_type,
            "exec_command_begin"
                | "agent_message"
                | "user_message"
                | "item_started"
                | "item_completed"
                | "turn_completed"
                | "turn_aborted"
                | "task_started"
                | "task_complete"
        ),
        _ => false,
    }
}

impl CodexSession {
    /// Update the pending-approval projection from one decoded rollout row.
    fn track_pending_approval(&mut self, row_type: &str, payload: &Value) {
        let event_type = payload.get("type").and_then(Value::as_str).unwrap_or("");
        if row_type == "event_msg" {
            match event_type {
                "exec_approval_request" | "apply_patch_approval_request" => {
                    self.pending_approval = self.decode_approval_request(event_type, payload);
                }
                _ if approval_cleared_by(row_type, event_type) => {
                    self.pending_approval = None;
                }
                _ => {}
            }
            return;
        }
        if approval_cleared_by(row_type, event_type) {
            self.pending_approval = None;
        }
    }

    /// Build the provider-neutral approval facts from a decoded request
    /// event. Requires a call_id (unidentifiable requests are skipped —
    /// fail-closed); options come only from the verified keymap contract.
    fn decode_approval_request(&self, event_type: &str, payload: &Value) -> Option<LiveApproval> {
        let call_id = payload.get("call_id").and_then(Value::as_str)?;
        if call_id.is_empty() {
            return None;
        }
        let reason = payload
            .get("reason")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty());
        let prompt = if event_type == "apply_patch_approval_request" {
            let summary = payload
                .get("summary")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .unwrap_or("apply patch");
            match reason {
                Some(reason) => format!("{summary} — {reason}"),
                None => summary.to_string(),
            }
        } else {
            let command = payload
                .get("command")
                .and_then(Value::as_array)
                .map(|argv| {
                    argv.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "exec command".to_string());
            match reason {
                Some(reason) => format!("{command} — {reason}"),
                None => command,
            }
        };
        // Option labels are the TUI's own action names; keys come from the
        // verified keymap (defaults + user overrides). "when available"
        // actions are safe to expose: an unavailable action key is a no-op in
        // the TUI, and the grid guard + frame confirmation still gate the
        // replay end-to-end.
        let options = vec![
            LiveApprovalOption {
                label: "Approve".to_string(),
                keys: self.approval_keys.approve.clone(),
            },
            LiveApprovalOption {
                label: "Approve for session".to_string(),
                keys: self.approval_keys.approve_for_session.clone(),
            },
            LiveApprovalOption {
                label: "Deny".to_string(),
                keys: self.approval_keys.deny.clone(),
            },
        ];
        Some(LiveApproval {
            id: call_id.to_string(),
            kind: "tool".to_string(),
            prompt,
            options,
        })
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
