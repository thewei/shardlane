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

/// Grok Build session format: `~/.grok/sessions/<url-encoded cwd>/<uuid>/`,
/// one directory per session. The main file `updates.jsonl` is a complete
/// ACP-style stream ({timestamp (unix seconds),
/// method:"session/update",params:{update:{sessionUpdate:…}}}); streaming
/// chunks on disk must be merged per role segment. `chat_history.jsonl` is a
/// compressed context snapshot and must not be treated as full history. The
/// `summary.json` sidecar provides cwd/title/git info. The sibling
/// `session_search.sqlite` is Grok's own index and `prompt_history.jsonl`
/// lives at the cwd-directory level — neither is a session file.
pub struct GrokAdapter {
    root: PathBuf,
}

impl GrokAdapter {
    pub fn new() -> Self {
        Self {
            root: home_dir().join(".grok/sessions"),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: PathBuf) -> Self {
        Self { root }
    }
}

impl Default for GrokAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// summary.json sidecar.
struct Summary {
    cwd: String,
    title: String,
    created_ms: i64,
    updated_ms: i64,
    git_branch: Option<String>,
    model: Option<String>,
}

fn read_summary(updates_path: &Path) -> Summary {
    let mut summary = Summary {
        cwd: String::new(),
        title: String::new(),
        created_ms: 0,
        updated_ms: 0,
        git_branch: None,
        model: None,
    };
    let Ok(raw) = fs::read_to_string(updates_path.with_file_name("summary.json")) else {
        return summary;
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return summary;
    };
    summary.cwd = value
        .pointer("/info/cwd")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    // generated_title and session_summary usually hold the same value; the
    // former is more semantic.
    for key in ["generated_title", "session_summary"] {
        if let Some(title) = value.get(key).and_then(Value::as_str) {
            if !title.trim().is_empty() {
                summary.title = title.to_string();
                break;
            }
        }
    }
    summary.created_ms = value
        .get("created_at")
        .and_then(Value::as_str)
        .map(iso_ms)
        .unwrap_or(0);
    summary.updated_ms = value
        .get("updated_at")
        .and_then(Value::as_str)
        .map(iso_ms)
        .unwrap_or(0);
    summary.git_branch = value
        .get("head_branch")
        .and_then(Value::as_str)
        .filter(|branch| !branch.is_empty())
        .map(String::from);
    summary.model = value
        .get("current_model_id")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .map(String::from);
    summary
}

/// Segment state for stream replay: one user turn breaks a segment; during
/// assistant turns, thought/message/tool interleaves all merge into the same
/// assistant message.
struct Replay {
    messages: Vec<TranscriptMessage>,
    tool_index: HashMap<String, (usize, usize)>,
    /// Currently accumulating segment; None = outside a segment.
    current_role: Option<Role>,
    current_text: String,
    current_thinking: String,
    current_tools: Vec<ToolCallView>,
    current_timestamp: i64,
}

impl Replay {
    fn new() -> Self {
        Self {
            messages: Vec::new(),
            tool_index: HashMap::new(),
            current_role: None,
            current_text: String::new(),
            current_thinking: String::new(),
            current_tools: Vec::new(),
            current_timestamp: 0,
        }
    }

    fn flush(&mut self) {
        let Some(role) = self.current_role.take() else {
            return;
        };
        let text = std::mem::take(&mut self.current_text);
        let thinking = std::mem::take(&mut self.current_thinking);
        let tools = std::mem::take(&mut self.current_tools);
        if text.trim().is_empty() && thinking.trim().is_empty() && tools.is_empty() {
            return;
        }
        let mut message = text_msg(role, &text, self.current_timestamp);
        if !thinking.trim().is_empty() {
            message.thinking = Some(clip(thinking.trim(), MAX_TOOL_IO).0);
        }
        let base = self.messages.len();
        for (position, tool_call) in tools.into_iter().enumerate() {
            self.tool_index
                .insert(tool_call.id.clone(), (base, position));
            message.tool_calls.push(tool_call);
        }
        self.messages.push(message);
    }

    /// Enter (or continue) a role segment.
    fn ensure(&mut self, role: Role, timestamp: i64) {
        if self.current_role != Some(role) {
            self.flush();
            self.current_role = Some(role);
            self.current_timestamp = timestamp;
        }
    }
}

/// content text of a user/agent chunk ({content:{type:text,text}}); borrows
/// with zero allocation.
fn chunk_text(update: &Value) -> &str {
    update
        .pointer("/content/text")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn tool_content_text(content: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    for item in content.as_array().into_iter().flatten() {
        let inner = item.get("content").unwrap_or(item);
        let text = inner
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !text.trim().is_empty() {
            parts.push(text.trim().to_string());
        }
    }
    parts.join("\n")
}

/// tool_call_update back-fill: completed states carry content (text output,
/// overwriting with the final state); status failed marks an error; when the
/// initial tool_call lacks rawInput, the update supplies it.
fn apply_tool_update(tool_call: &mut ToolCallView, update: &Value) {
    if let Some(content) = update.get("content") {
        let text = tool_content_text(content);
        if !text.is_empty() {
            tool_call.output = Some(clip(&text, MAX_TOOL_IO).0);
        }
    }
    if update.get("status").and_then(Value::as_str) == Some("failed") {
        tool_call.is_error = true;
    }
    if tool_call.input.is_none() {
        if let Some(raw) = update.get("rawInput").filter(|raw| !raw.is_null()) {
            tool_call.input_preview = make_preview(raw);
            if let Ok(json) = serde_json::to_string_pretty(raw) {
                tool_call.input = Some(clip(&json, MAX_TOOL_IO).0);
            }
        }
    }
}

fn parse_updates(path: &Path) -> Result<(Vec<TranscriptMessage>, u32)> {
    let file = fs::File::open(path)?;
    let reader = BufReader::with_capacity(1 << 20, file);
    let mut replay = Replay::new();
    let mut unknown = 0;

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => {
                unknown += 1;
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let row: Value = match serde_json::from_str(&line) {
            Ok(row) => row,
            Err(_) => {
                unknown += 1;
                continue;
            }
        };
        let timestamp = row.get("timestamp").map(to_epoch_ms).unwrap_or(0);
        let Some(update) = row.pointer("/params/update") else {
            unknown += 1;
            continue;
        };
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("user_message_chunk") => {
                replay.ensure(Role::User, timestamp);
                replay.current_text.push_str(chunk_text(update));
            }
            Some("agent_message_chunk") => {
                replay.ensure(Role::Assistant, timestamp);
                replay.current_text.push_str(chunk_text(update));
            }
            Some("agent_thought_chunk") => {
                replay.ensure(Role::Assistant, timestamp);
                replay.current_thinking.push_str(chunk_text(update));
            }
            Some("tool_call") => {
                replay.ensure(Role::Assistant, timestamp);
                let id = update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let name = update
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let input = update.get("rawInput").cloned().unwrap_or(Value::Null);
                replay.current_tools.push(tool_call_view(
                    id.to_string(),
                    name,
                    &input,
                    None,
                    false,
                ));
            }
            Some("tool_call_update") => {
                let Some(id) = update.get("toolCallId").and_then(Value::as_str) else {
                    continue;
                };
                // The target tool may still be in the current unflushed
                // segment or already in messages.
                if let Some(tool_call) = replay
                    .current_tools
                    .iter_mut()
                    .find(|tool_call| tool_call.id == id)
                {
                    apply_tool_update(tool_call, update);
                } else if let Some(&(message_index, tool_position)) = replay.tool_index.get(id) {
                    apply_tool_update(
                        &mut replay.messages[message_index].tool_calls[tool_position],
                        update,
                    );
                }
            }
            // Known non-content updates: task backgrounding/compaction etc.
            Some(
                "task_backgrounded"
                | "task_completed"
                | "auto_compact_started"
                | "auto_compact_completed"
                | "compaction_checkpoint"
                | "plan"
                | "current_mode_update",
            ) => {}
            _ => {
                unknown += 1;
            }
        }
    }
    replay.flush();
    let mut messages = replay.messages;
    assign_seq(&mut messages);
    Ok((messages, unknown))
}

fn build_meta(
    reference: &SessionFileRef,
    summary: &Summary,
    messages: &[TranscriptMessage],
) -> SessionMeta {
    let title = Some(clean_title_candidate(&summary.title))
        .filter(|title| !title.is_empty())
        .or_else(|| title_from_messages(messages))
        .unwrap_or_else(|| UNTITLED.to_string());
    let latest_message = messages
        .iter()
        .filter_map(|message| message.timestamp)
        .max()
        .unwrap_or(0);
    SessionMeta {
        key: format!("grok:{}", reference.native_id),
        id: reference.native_id.clone(),
        agent: AgentId::Grok,
        title,
        project_path: summary.cwd.clone(),
        project_name: project_name_of(&summary.cwd),
        file_path: reference.file_path.clone(),
        created_at: if summary.created_ms > 0 {
            summary.created_ms
        } else {
            reference.mtime_ms
        },
        updated_at: match summary.updated_ms.max(latest_message) {
            timestamp if timestamp > 0 => timestamp,
            _ => reference.mtime_ms,
        },
        message_count: messages
            .iter()
            .filter(|message| message.kind == MessageKind::Text)
            .count() as i64,
        size_bytes: reference.size,
        git_branch: summary.git_branch.clone(),
        model: summary.model.clone(),
        tokens_used: None,
        archived: false,
        source: None,
    }
}

impl AgentHistoryAdapter for GrokAdapter {
    fn agent(&self) -> AgentId {
        AgentId::Grok
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        let mut references = Vec::new();
        // Root-level files such as session_search.sqlite naturally fail
        // read_dir and are skipped; main-file checks (exists, non-empty,
        // native_id) all go through file_ref.
        let Ok(cwd_dirs) = fs::read_dir(&self.root) else {
            return Ok(references);
        };
        for cwd_dir in cwd_dirs.flatten() {
            let Ok(sessions) = fs::read_dir(cwd_dir.path()) else {
                continue;
            };
            for session in sessions.flatten() {
                if let Some(reference) = self.file_ref(&session.path().join("updates.jsonl")) {
                    references.push(reference);
                }
            }
        }
        Ok(references)
    }

    fn file_ref(&self, path: &Path) -> Option<SessionFileRef> {
        // Only updates.jsonl inside a session directory counts;
        // prompt_history/chat_history/events are not main files.
        if path.file_name()?.to_string_lossy() != "updates.jsonl" {
            return None;
        }
        let session_dir = path.parent()?;
        let mut reference = default_file_ref(self.agent(), path)?;
        reference.native_id = session_dir.file_name()?.to_string_lossy().to_string();
        Some(reference)
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession> {
        let (messages, unknown) = parse_updates(Path::new(&reference.file_path))?;
        let summary = read_summary(Path::new(&reference.file_path));
        let meta = build_meta(reference, &summary, &messages);
        let units = units_from_messages(&messages);
        Ok(ParsedSession {
            meta,
            units,
            unknown_line_count: unknown,
        })
    }

    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript> {
        let (messages, unknown) = parse_updates(Path::new(&reference.file_path))?;
        let summary = read_summary(Path::new(&reference.file_path));
        Ok(ParsedTranscript::simple(
            build_meta(reference, &summary, &messages),
            messages,
            unknown,
        ))
    }

    fn with_custom_root(&self, dir: PathBuf) -> Box<dyn AgentHistoryAdapter> {
        let root = if dir.join("sessions").is_dir() {
            dir.join("sessions")
        } else {
            dir
        };
        Box::new(Self { root })
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        vec![self.root.clone()]
    }
}
