// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

use super::parse_utils::*;
use super::{units_from_messages, AgentHistoryAdapter};
use crate::models::*;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Command Code session format:
/// `~/.commandcode/projects/<project>/<session>.jsonl`; the first line is a
/// session header followed by message lines. `<session>.meta.json` in the
/// same directory provides the title; `*.checkpoints.jsonl` is an internal
/// checkpoint log and not part of the main session history.
pub struct CommandCodeAdapter {
    root: PathBuf,
}

impl CommandCodeAdapter {
    pub fn new() -> Self {
        Self {
            root: home_dir().join(".commandcode/projects"),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: PathBuf) -> Self {
        Self { root }
    }
}

impl Default for CommandCodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CcHeader {
    pub(crate) id: String,
    pub(crate) cwd: String,
    pub(crate) created_at: i64,
}

#[derive(Debug)]
struct CcParse {
    header: Option<CcHeader>,
    title: Option<String>,
    messages: Vec<TranscriptMessage>,
    model: Option<String>,
    tokens_used: Option<i64>,
    created_at: i64,
    updated_at: i64,
    unknown_lines: u32,
}

/// One entry on the graph: id + parent + parsed message (a None parent means
/// attach to the previous entry when the linkage is linear).
pub(crate) struct CcEntry {
    pub(crate) id: String,
    pub(crate) parent: Option<String>,
    pub(crate) message: Option<TranscriptMessage>,
}

/// Reusable replay state for Command Code sessions (PEX-3 / plan §7.4: full
/// parsing and live increments share the same line-level state machine).
/// Entries form a logical tree via `id`/`parentId` — the file is append-only,
/// but `/rewind`/fork move the visible lineage. The projection is the active
/// chain found by walking parents back from the newest entry; linear growth
/// uses appended/changed deltas, while a lineage change (a new entry's parent
/// is not at the chain tail) triggers `projection_reset`, delivered by
/// LiveSync as a Reset (full projection replacement) — an upsert cannot
/// express shrinking.
pub(crate) struct CommandCodeSession {
    pub(crate) header: Option<CcHeader>,
    pub(crate) entries: Vec<CcEntry>,
    by_id: HashMap<String, usize>,
    lineage: Vec<usize>,
    pub(crate) messages: Vec<TranscriptMessage>,
    /// call_id → (projected message index, tool index, entry index);
    /// tool_result back-fill must hit both the projection and the graph node
    /// (otherwise branch rebuilds lose the back-filled output).
    tool_index: HashMap<String, (usize, usize, usize)>,
    pub(crate) update: crate::live::DecoderUpdate,
    pub(crate) title: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) tokens_used: Option<i64>,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    pub(crate) unknown_lines: u32,
}

impl CommandCodeSession {
    pub(crate) fn fresh() -> Self {
        Self {
            header: None,
            entries: Vec::new(),
            by_id: HashMap::new(),
            lineage: Vec::new(),
            messages: Vec::new(),
            tool_index: HashMap::new(),
            update: crate::live::DecoderUpdate::default(),
            title: None,
            model: None,
            tokens_used: None,
            created_at: 0,
            updated_at: 0,
            unknown_lines: 0,
        }
    }

    fn push_projection(
        &mut self,
        mut message: TranscriptMessage,
        entry_index: usize,
        entry_message: TranscriptMessage,
    ) {
        let message_index = self.messages.len();
        message.seq = message_index as i64;
        self.update.appended.push(message_index);
        for (call_index, call) in message.tool_calls.iter().enumerate() {
            if !call.id.is_empty() {
                self.tool_index
                    .insert(call.id.clone(), (message_index, call_index, entry_index));
            }
        }
        self.messages.push(message);
        self.entries.push(CcEntry {
            id: String::new(),
            parent: None,
            message: Some(entry_message),
        });
    }

    /// Active-chain rebuild: walk parents from entry `leaf` back to the root,
    /// then reverse into root→leaf order. Returns the new chain and its
    /// messages.
    fn chain_from(&self, leaf: usize) -> (Vec<usize>, Vec<TranscriptMessage>) {
        let mut chain = Vec::new();
        let mut cursor = Some(leaf);
        while let Some(index) = cursor {
            chain.push(index);
            cursor = self.entries[index]
                .parent
                .as_ref()
                .and_then(|parent| self.by_id.get(parent))
                .copied();
        }
        chain.reverse();
        let messages = chain
            .iter()
            .filter_map(|index| self.entries[*index].message.clone())
            .collect();
        (chain, messages)
    }

    pub(crate) fn ingest_row(&mut self, row: &Value, timestamp: i64) {
        if timestamp > 0 {
            if self.created_at == 0 {
                self.created_at = timestamp;
            }
            self.updated_at = self.updated_at.max(timestamp);
        }
        if self.model.is_none() {
            if let Some(model) = row.get("model").and_then(Value::as_str) {
                self.model = Some(model.to_string());
            }
        }
        if let Some(tokens) = row.get("usage").and_then(usage_total_tokens) {
            self.tokens_used = Some(self.tokens_used.map_or(tokens, |prev| prev.max(tokens)));
        }

        match row.get("type").and_then(Value::as_str).unwrap_or_default() {
            "session" => {
                if self.header.is_none() {
                    self.header = parse_header(row);
                    if let Some(header) = self.header.as_ref() {
                        if self.created_at == 0 {
                            self.created_at = header.created_at;
                        }
                    }
                }
                return;
            }
            "message" => {}
            _ => {
                self.unknown_lines += 1;
                return;
            }
        }

        // Line parsing: text/thinking/tool calls go into this entry's message;
        // tool_result immediately back-fills the target call in the current
        // projection (real data: results follow calls).
        let message = row.get("message").unwrap_or(&Value::Null);
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut text_parts = Vec::<&str>::new();
        let mut thinking_parts = Vec::<&str>::new();
        let mut tool_calls = Vec::<ToolCallView>::new();
        let mut tool_results = Vec::<(String, String, bool)>::new();

        for block in message
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let text_field = || {
                block
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
            };
            match block
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
            {
                "text" => text_parts.extend(text_field()),
                "thinking" => {
                    if let Some(thinking) = block
                        .get("thinking")
                        .and_then(Value::as_str)
                        .or_else(|| block.get("text").and_then(Value::as_str))
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                    {
                        thinking_parts.push(thinking);
                    }
                }
                "tool_use" => {
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let input = block.get("input").unwrap_or(&Value::Null);
                    tool_calls.push(tool_call_view(id, name, input, None, false));
                }
                "tool_result" => {
                    let Some(call_id) = block
                        .get("tool_use_id")
                        .or_else(|| block.get("toolUseId"))
                        .and_then(Value::as_str)
                    else {
                        continue;
                    };
                    let output = tool_result_text(block.get("content").unwrap_or(&Value::Null));
                    let is_error = block
                        .get("is_error")
                        .or_else(|| block.get("isError"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    tool_results.push((call_id.to_string(), output, is_error));
                }
                _ => {}
            }
        }

        for (call_id, output, is_error) in tool_results {
            if let Some(&(message_index, call_index, entry_index)) =
                self.tool_index.get(call_id.as_str())
            {
                for target in [
                    self.messages
                        .get_mut(message_index)
                        .and_then(|m| m.tool_calls.get_mut(call_index)),
                    self.entries
                        .get_mut(entry_index)
                        .and_then(|entry| entry.message.as_mut())
                        .and_then(|m| m.tool_calls.get_mut(call_index)),
                ]
                .into_iter()
                .flatten()
                {
                    if !output.is_empty() {
                        target.output = Some(clip(&output, MAX_TOOL_IO).0);
                    }
                    if is_error {
                        target.is_error = true;
                    }
                }
                self.update.changed.push(message_index);
            }
        }

        let text = text_parts.join("\n\n");
        let thinking = if thinking_parts.is_empty() {
            None
        } else {
            Some(clip(&thinking_parts.join("\n\n"), MAX_TOOL_IO).0)
        };
        // Empty-text lines (pure tool_result etc.) produce no projected
        // message but must still create a graph node: later entries link
        // through it (e.g. m4.parent = m3).
        let transcript = match role {
            "assistant" => {
                if text.is_empty() && thinking.is_none() && tool_calls.is_empty() {
                    None
                } else {
                    Some(text_msg(Role::Assistant, &text, timestamp))
                }
            }
            "user" => {
                if text.is_empty() {
                    None
                } else {
                    Some(text_msg(Role::User, &text, timestamp))
                }
            }
            _ => {
                if text.is_empty() {
                    None
                } else {
                    let mut message = text_msg(Role::System, &text, timestamp);
                    message.kind = MessageKind::Meta;
                    Some(message)
                }
            }
        };
        let mut transcript = transcript;
        if let Some(message) = transcript.as_mut() {
            message.thinking = thinking;
            message.model = row.get("model").and_then(Value::as_str).map(str::to_string);
            if message.model.is_some() && self.model.is_none() {
                self.model = message.model.clone();
            }
            message.tool_calls = tool_calls;
        }

        // Graph attach: explicit parent attaches to that parent; the default
        // attaches to the previous entry (linear compatibility with old
        // data).
        let entry_id = row
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("#{}", self.entries.len()));
        let parent = row
            .get("parentId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let attaches_linear = match (&parent, self.lineage.last()) {
            (Some(parent_id), Some(&last)) => self
                .entries
                .get(last)
                .is_some_and(|entry| &entry.id == parent_id),
            (None, _) => true,
            (Some(_), None) => false,
        };
        let entry_index = self.entries.len();
        if attaches_linear {
            match transcript {
                Some(message) => {
                    let entry_message = message.clone();
                    self.push_projection(message, entry_index, entry_message);
                }
                None => self.entries.push(CcEntry {
                    id: String::new(),
                    parent: None,
                    message: None,
                }),
            }
            self.entries[entry_index].id = entry_id.clone();
            self.entries[entry_index].parent = parent;
            self.lineage.push(entry_index);
            self.by_id.insert(entry_id, entry_index);
            return;
        }
        // Branch/rewind: the new entry's parent is not at the chain tail.
        // Attach to the graph, rebuild the active chain with the new entry as
        // leaf, and replace the projection wholesale (Reset) — never splice
        // across branches.
        let chain = {
            self.entries.push(CcEntry {
                id: entry_id.clone(),
                parent,
                message: transcript,
            });
            self.by_id.insert(entry_id, entry_index);
            self.lineage.clear();
            self.messages.clear();
            self.tool_index.clear();
            self.update.projection_reset = true;
            let (chain, messages) = self.chain_from(entry_index);
            for (entry_index_of_chain, message) in chain.iter().zip(messages.iter()) {
                let mut message = message.clone();
                let message_index = self.messages.len();
                message.seq = message_index as i64;
                self.update.appended.push(message_index);
                for (call_index, call) in message.tool_calls.iter().enumerate() {
                    if !call.id.is_empty() {
                        self.tool_index.insert(
                            call.id.clone(),
                            (message_index, call_index, *entry_index_of_chain),
                        );
                    }
                }
                self.messages.push(message);
            }
            chain
        };
        self.lineage = chain;
    }
}

fn parse_header(row: &Value) -> Option<CcHeader> {
    if row.get("type").and_then(Value::as_str) != Some("session") {
        return None;
    }
    let id = row.get("id").and_then(Value::as_str)?.trim();
    if id.is_empty() {
        return None;
    }
    Some(CcHeader {
        id: id.to_string(),
        cwd: row
            .get("cwd")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        created_at: row
            .get("timestamp")
            .and_then(Value::as_str)
            .map(iso_ms)
            .unwrap_or(0),
    })
}

fn read_header(path: &Path) -> Option<CcHeader> {
    let file = fs::File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let row: Value = serde_json::from_str(line.trim_end()).ok()?;
    parse_header(&row)
}

fn sidecar_title(path: &Path) -> Option<String> {
    let sidecar = path.with_extension("meta.json");
    let raw = fs::read_to_string(sidecar).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let title = value.get("title").and_then(Value::as_str)?;
    let title = clean_title_candidate(title);
    (!title.is_empty()).then_some(title)
}

fn usage_total_tokens(usage: &Value) -> Option<i64> {
    usage
        .get("totalTokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            let sum = [
                "inputTokens",
                "outputTokens",
                "cacheReadTokens",
                "cacheWriteTokens",
            ]
            .iter()
            .filter_map(|key| usage.get(*key).and_then(Value::as_i64))
            .sum::<i64>();
            (sum > 0).then_some(sum)
        })
}

fn tool_result_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.trim().to_string(),
        Value::Array(_) => {
            let text = blocks_text(value);
            if text.is_empty() {
                serde_json::to_string(value).unwrap_or_default()
            } else {
                text
            }
        }
        Value::Object(object) => {
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                let text = text.trim();
                if !text.is_empty() {
                    return text.to_string();
                }
            }
            if let Some(content) = object.get("content") {
                let text = tool_result_text(content);
                if !text.is_empty() {
                    return text;
                }
            }
            serde_json::to_string(value).unwrap_or_default()
        }
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn parse_log(path: &Path) -> Result<CcParse> {
    let file = fs::File::open(path)?;
    let reader = BufReader::with_capacity(1 << 20, file);
    let mut session = CommandCodeSession::fresh();
    session.title = sidecar_title(path);

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => {
                session.unknown_lines += 1;
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let row: Value = match serde_json::from_str(&line) {
            Ok(row) => row,
            Err(_) => {
                session.unknown_lines += 1;
                continue;
            }
        };
        let timestamp = row
            .get("timestamp")
            .and_then(Value::as_str)
            .map(iso_ms)
            .unwrap_or(0);
        session.ingest_row(&row, timestamp);
    }

    assign_seq(&mut session.messages);
    Ok(CcParse {
        header: session.header,
        title: session.title,
        messages: session.messages,
        model: session.model,
        tokens_used: session.tokens_used,
        created_at: session.created_at,
        updated_at: session.updated_at,
        unknown_lines: session.unknown_lines,
    })
}

fn build_meta(reference: &SessionFileRef, parsed: &CcParse) -> SessionMeta {
    let (id, cwd, created_header) = match parsed.header.as_ref() {
        Some(header) => (header.id.clone(), header.cwd.clone(), header.created_at),
        None => (reference.native_id.clone(), String::new(), 0),
    };
    let created_at = if created_header > 0 {
        created_header
    } else if parsed.created_at > 0 {
        parsed.created_at
    } else {
        reference.mtime_ms
    };
    let updated_at = if parsed.updated_at > 0 {
        parsed.updated_at
    } else {
        reference.mtime_ms
    };
    SessionMeta {
        key: format!("command-code:{id}"),
        id,
        agent: AgentId::CommandCode,
        title: parsed
            .title
            .clone()
            .or_else(|| title_from_messages(&parsed.messages))
            .unwrap_or_else(|| UNTITLED.to_string()),
        project_path: cwd.clone(),
        project_name: project_name_of(&cwd),
        file_path: reference.file_path.clone(),
        created_at,
        updated_at,
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
        source: Some("command-code".to_string()),
    }
}

impl AgentHistoryAdapter for CommandCodeAdapter {
    fn agent(&self) -> AgentId {
        AgentId::CommandCode
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        if !self.root.is_dir() {
            return Ok(Vec::new());
        }
        let mut references = Vec::new();
        for entry in walkdir::WalkDir::new(&self.root) {
            let entry = entry.map_err(|error| anyhow!("walk {}: {error}", self.root.display()))?;
            if !entry.file_type().is_file() {
                continue;
            }
            if let Some(reference) = self.file_ref(entry.path()) {
                references.push(reference);
            }
        }
        Ok(references)
    }

    fn file_ref(&self, path: &Path) -> Option<SessionFileRef> {
        let name = path.file_name()?.to_string_lossy();
        if !name.ends_with(".jsonl") || name.ends_with(".checkpoints.jsonl") {
            return None;
        }
        let stem = name.strip_suffix(".jsonl")?;
        let meta = fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() == 0 {
            return None;
        }
        let native_id = read_header(path)
            .map(|header| header.id)
            .unwrap_or_else(|| stem.to_string());
        Some(SessionFileRef {
            agent: AgentId::CommandCode,
            native_id,
            file_path: path.to_string_lossy().to_string(),
            mtime_ms: mtime_ms(&meta),
            size: meta.len() as i64,
        })
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession> {
        let parsed = parse_log(Path::new(&reference.file_path))?;
        let meta = build_meta(reference, &parsed);
        let units = units_from_messages(&parsed.messages);
        Ok(ParsedSession {
            meta,
            units,
            unknown_line_count: parsed.unknown_lines,
        })
    }

    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript> {
        let parsed = parse_log(Path::new(&reference.file_path))?;
        Ok(ParsedTranscript::simple(
            build_meta(reference, &parsed),
            parsed.messages,
            parsed.unknown_lines,
        ))
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
