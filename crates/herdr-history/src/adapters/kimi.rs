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

/// Kimi Code session format:
/// `~/.kimi-code/sessions/wd_<name>_<hash>/session_<uuid>/` — one directory
/// per session, main file `agents/main/wire.jsonl` (event-sourced: turn.prompt
/// is the user input, context.append_message is the complete message landed
/// into context, turn.* lifecycle and config lines are known and skipped).
/// The `state.json` sidecar provides title/times ("New Session" is a
/// placeholder); cwd comes from the root-level `session_index.jsonl`
/// sessionId→workDir mapping (the directory-name hash cannot be reversed).
/// `agents/<non-main>/` are subagents and are not listed.
pub struct KimiAdapter {
    root: PathBuf,
    index_path: PathBuf,
    /// sessionId → workDir, cached by index mtime (called per session on full
    /// refresh).
    cwd_cache: MtimeCache<HashMap<String, String>>,
}

impl KimiAdapter {
    pub fn new() -> Self {
        let home = home_dir().join(".kimi-code");
        Self::with_paths(home.join("sessions"), home.join("session_index.jsonl"))
    }

    fn with_paths(root: PathBuf, index_path: PathBuf) -> Self {
        Self {
            root,
            index_path,
            cwd_cache: MtimeCache::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: PathBuf, index_path: PathBuf) -> Self {
        Self::with_paths(root, index_path)
    }

    fn cwd_map(&self) -> HashMap<String, String> {
        let mtime = fs::metadata(&self.index_path)
            .map(|meta| mtime_ms(&meta))
            .unwrap_or(0);
        self.cwd_cache
            .get_or_try_build(mtime, || {
                let mut mapping = HashMap::new();
                let Ok(raw) = fs::read_to_string(&self.index_path) else {
                    return Some(mapping);
                };
                for line in raw.lines() {
                    let Ok(value) = serde_json::from_str::<Value>(line) else {
                        continue;
                    };
                    if let (Some(id), Some(work_dir)) = (
                        value.get("sessionId").and_then(Value::as_str),
                        value.get("workDir").and_then(Value::as_str),
                    ) {
                        mapping.insert(id.to_string(), work_dir.to_string());
                    }
                }
                Some(mapping)
            })
            .unwrap_or_default()
    }

    fn cwd_for(&self, native_id: &str) -> String {
        self.cwd_map().get(native_id).cloned().unwrap_or_default()
    }
}

/// `…/session_<uuid>/agents/main/wire.jsonl` → session directory.
/// The layout knowledge "three levels above wire.jsonl + session_ prefix"
/// lives only here.
fn session_dir_of(wire_path: &Path) -> Option<&Path> {
    let dir = wire_path.ancestors().nth(3)?;
    dir.file_name()?
        .to_string_lossy()
        .starts_with("session_")
        .then_some(dir)
}

/// state.json sidecar (in session dir session_<uuid>/, three levels above
/// wire.jsonl).
struct KimiState {
    title: String,
    created_ms: i64,
    updated_ms: i64,
}

fn read_state(wire_path: &Path) -> KimiState {
    let mut state = KimiState {
        title: String::new(),
        created_ms: 0,
        updated_ms: 0,
    };
    let Some(session_dir) = session_dir_of(wire_path) else {
        return state;
    };
    let Ok(raw) = fs::read_to_string(session_dir.join("state.json")) else {
        return state;
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return state;
    };
    if let Some(title) = value.get("title").and_then(Value::as_str) {
        // "New Session" is Kimi's placeholder title; never treat it as real.
        if title != "New Session" {
            state.title = title.to_string();
        }
    }
    state.created_ms = value
        .get("createdAt")
        .and_then(Value::as_str)
        .map(iso_ms)
        .unwrap_or(0);
    state.updated_ms = value
        .get("updatedAt")
        .and_then(Value::as_str)
        .map(iso_ms)
        .unwrap_or(0);
    state
}

/// content parts (turn.prompt's input / message.content) → plain text.
/// A part may be a bare string or {type:text,text}; media parts (blobref) are
/// skipped.
fn parts_text(value: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    match value {
        Value::String(text) => {
            if !text.trim().is_empty() {
                parts.push(text.trim().to_string());
            }
        }
        Value::Array(items) => {
            for part in items {
                match part {
                    Value::String(text) if !text.trim().is_empty() => {
                        parts.push(text.trim().to_string());
                    }
                    Value::Object(_) => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            if !text.trim().is_empty() {
                                parts.push(text.trim().to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    parts.join("\n\n")
}

/// Reusable replay state for wire.jsonl (PEX-3 / plan §7.2: full parsing and
/// live increments share the same line-level state machine — the single
/// source of truth for provider semantics). Fed line by line; every produced
/// message is an append (current semantics have no in-place updates).
pub(crate) struct KimiSession {
    pub(crate) messages: Vec<TranscriptMessage>,
    pub(crate) update: crate::live::DecoderUpdate,
    pub(crate) unknown_lines: u32,
}

impl KimiSession {
    pub(crate) fn fresh() -> Self {
        Self {
            messages: Vec::new(),
            update: crate::live::DecoderUpdate::default(),
            unknown_lines: 0,
        }
    }

    fn push_message(&mut self, mut message: TranscriptMessage) {
        message.seq = self.messages.len() as i64;
        self.update.appended.push(self.messages.len());
        self.messages.push(message);
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
        match row.get("type").and_then(Value::as_str) {
            Some("turn.prompt") | Some("turn.steer") => {
                let text = parts_text(row.get("input").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    self.push_message(text_msg(Role::User, &text, 0));
                }
            }
            Some("context.append_message") => {
                let Some(message) = row.get("message") else {
                    self.unknown_lines += 1;
                    return;
                };
                let role = match message.get("role").and_then(Value::as_str) {
                    Some("assistant") => Role::Assistant,
                    // User input is already covered by turn.prompt; context
                    // lines such as tool/system are skipped.
                    _ => return,
                };
                let text = parts_text(message.get("content").unwrap_or(&Value::Null));
                if !text.is_empty() {
                    self.push_message(text_msg(role, &text, 0));
                }
            }
            // Known config/lifecycle/tool event lines (tool details live in
            // loop_event and are not expanded).
            Some("metadata")
            | Some("config.update")
            | Some("tools.set_active_tools")
            | Some("context.append_loop_event") => {}
            Some(event_type) if event_type.starts_with("turn.") => {}
            _ => {
                self.unknown_lines += 1;
            }
        }
    }

    pub(crate) fn count_unknown_line(&mut self) {
        self.unknown_lines += 1;
    }
}

fn parse_wire(path: &Path) -> Result<(Vec<TranscriptMessage>, u32)> {
    let file = fs::File::open(path)?;
    let reader = BufReader::with_capacity(1 << 20, file);
    let mut session = KimiSession::fresh();

    for line in reader.lines() {
        match line {
            Ok(line) => session.feed_line(&line),
            Err(_) => session.count_unknown_line(),
        }
    }
    assign_seq(&mut session.messages);
    Ok((session.messages, session.unknown_lines))
}

fn build_meta(
    reference: &SessionFileRef,
    state: &KimiState,
    cwd: &str,
    messages: &[TranscriptMessage],
) -> SessionMeta {
    let title = Some(clean_title_candidate(&state.title))
        .filter(|title| !title.is_empty())
        .or_else(|| title_from_messages(messages))
        .unwrap_or_else(|| UNTITLED.to_string());
    SessionMeta {
        key: format!("kimi:{}", reference.native_id),
        id: reference.native_id.clone(),
        agent: AgentId::Kimi,
        title,
        project_path: cwd.to_string(),
        project_name: project_name_of(cwd),
        file_path: reference.file_path.clone(),
        created_at: if state.created_ms > 0 {
            state.created_ms
        } else {
            reference.mtime_ms
        },
        updated_at: if state.updated_ms > 0 {
            state.updated_ms
        } else {
            reference.mtime_ms
        },
        message_count: messages
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

/// The session directory name is the native_id (same shape as the sessionId
/// in session_index.jsonl).
fn native_id_of(wire_path: &Path) -> Option<String> {
    Some(
        session_dir_of(wire_path)?
            .file_name()?
            .to_string_lossy()
            .to_string(),
    )
}

impl AgentHistoryAdapter for KimiAdapter {
    fn agent(&self) -> AgentId {
        AgentId::Kimi
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        let mut references = Vec::new();
        let Ok(work_dirs) = fs::read_dir(&self.root) else {
            return Ok(references);
        };
        // Main-file checks (session_ prefix, exists, non-empty, native_id)
        // all go through file_ref.
        for work_dir in work_dirs.flatten() {
            let Ok(sessions) = fs::read_dir(work_dir.path()) else {
                continue;
            };
            for session in sessions.flatten() {
                let wire = session.path().join("agents/main/wire.jsonl");
                if let Some(reference) = self.file_ref(&wire) {
                    references.push(reference);
                }
            }
        }
        Ok(references)
    }

    fn file_ref(&self, path: &Path) -> Option<SessionFileRef> {
        // Only the main agent's wire.jsonl counts; agents/<other>/ are
        // subagents.
        let path_text = path.to_string_lossy();
        if !path_text.ends_with("/agents/main/wire.jsonl") {
            return None;
        }
        let native = native_id_of(path)?;
        let mut reference = default_file_ref(self.agent(), path)?;
        reference.native_id = native;
        Some(reference)
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession> {
        let (messages, unknown) = parse_wire(Path::new(&reference.file_path))?;
        let state = read_state(Path::new(&reference.file_path));
        let meta = build_meta(
            reference,
            &state,
            &self.cwd_for(&reference.native_id),
            &messages,
        );
        let units = units_from_messages(&messages);
        Ok(ParsedSession {
            meta,
            units,
            unknown_line_count: unknown,
        })
    }

    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript> {
        let (messages, unknown) = parse_wire(Path::new(&reference.file_path))?;
        let state = read_state(Path::new(&reference.file_path));
        Ok(ParsedTranscript::simple(
            build_meta(
                reference,
                &state,
                &self.cwd_for(&reference.native_id),
                &messages,
            ),
            messages,
            unknown,
        ))
    }

    fn with_custom_root(&self, dir: PathBuf) -> Box<dyn AgentHistoryAdapter> {
        let (root, index_path) = if dir.join("sessions").is_dir() {
            (dir.join("sessions"), dir.join("session_index.jsonl"))
        } else {
            let index_path = dir
                .parent()
                .map(|parent| parent.join("session_index.jsonl"))
                .unwrap_or_else(|| dir.join("session_index.jsonl"));
            (dir, index_path)
        };
        Box::new(Self::with_paths(root, index_path))
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        vec![self.root.clone()]
    }
}

impl Default for KimiAdapter {
    fn default() -> Self {
        Self::new()
    }
}
