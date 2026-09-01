// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.
//! [INPUT]: Qoder CLI project-root JSONL files (`~/.qoder/projects` or
//!           `QODER_CONFIG_DIR/projects`) and their read-only session metadata.
//! [OUTPUT]: `QoderAdapter` with normalized session metadata/transcripts,
//!           active-leaf branch selection, fragment/tool aggregation, and
//!           resumable native session identities.
//! [POS]: Qoder's History-only adapter boundary; it never starts Qoder,
//!        observes a process, or owns Herdr runtime state.

use super::parse_utils::*;
use super::{units_from_messages, AgentHistoryAdapter};
use crate::models::*;
use anyhow::Result;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Qoder stores sessions as `projects/<project-key>/<session-id>.jsonl`.
/// Project directories may contain `state`, `subagents`, and `memory`
/// sidecars; only JSONL files directly owned by the project directory are
/// top-level sessions.
pub struct QoderAdapter {
    root: PathBuf,
}

impl QoderAdapter {
    pub fn new() -> Self {
        let default = home_dir().join(".qoder").join("projects");
        let configured = env_dir("QODER_CONFIG_DIR").map(|dir| dir.join("projects"));
        Self {
            root: select_root(default, configured),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: PathBuf) -> Self {
        Self { root }
    }
}

impl Default for QoderAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Prefer an environment-configured root only when it contains real session
/// files. An empty/stale env directory must not hide the default root.
fn select_root(default: PathBuf, configured: Option<PathBuf>) -> PathBuf {
    configured
        .filter(|root| contains_session_file(root))
        .unwrap_or(default)
}

fn direct_jsonl_refs(dir: &Path) -> Vec<SessionFileRef> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let mut reference = default_file_ref(AgentId::Qoder, &entry.path())?;
            reference.agent = AgentId::Qoder;
            Some(reference)
        })
        .collect()
}

fn is_sidecar_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("state") | Some("subagents") | Some("memory")
    )
}

/// The default root contains project-key directories, while a custom source
/// may point directly at one project-key directory. Probe both depths, never
/// recursing into sidecar directories.
fn list_refs(root: &Path) -> Vec<SessionFileRef> {
    let mut references = direct_jsonl_refs(root);
    let Ok(entries) = fs::read_dir(root) else {
        return references;
    };
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|file_type| file_type.is_dir())
            && !is_sidecar_dir(&entry.path())
        {
            references.extend(direct_jsonl_refs(&entry.path()));
        }
    }
    references.sort_by(|left, right| left.file_path.cmp(&right.file_path));
    references
}

fn contains_session_file(root: &Path) -> bool {
    !list_refs(root).is_empty()
}

#[derive(Clone)]
struct MessageNode {
    row: Value,
    parent: Option<String>,
}

enum ActiveLeaf {
    Missing,
    Empty,
    Uuid(String),
}

#[derive(Default)]
struct PendingAssistant {
    message_id: Option<String>,
    text: Vec<String>,
    thinking: Vec<String>,
    tool_calls: Vec<ToolCallView>,
    timestamp: Option<i64>,
    model: Option<String>,
}

struct QoderParse {
    messages: Vec<TranscriptMessage>,
    custom_title: String,
    ai_title: String,
    last_prompt: String,
    cwd: String,
    git_branch: Option<String>,
    model: Option<String>,
    tokens_used: i64,
    created_at: i64,
    updated_at: i64,
    unknown_lines: u32,
}

const KNOWN_METADATA_TYPES: &[&str] = &[
    "summary",
    "custom-title",
    "ai-title",
    "last-prompt",
    "tag",
    "workspace-directories",
    "runtime-config",
    "mode",
    "content-replacement",
    "file-history-snapshot",
    "token-stats",
    "active-leaf",
    "relocated",
    "worktree-state",
];

fn optional_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn usage_tokens(usage: &Value) -> i64 {
    let total = usage
        .get("total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if total > 0 {
        return total;
    }
    let message_tokens: i64 = [
        "input_tokens",
        "output_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ]
    .iter()
    .map(|key| usage.get(*key).and_then(Value::as_i64).unwrap_or(0))
    .sum();
    if message_tokens > 0 {
        return message_tokens;
    }
    ["prompt_tokens", "completion_tokens"]
        .iter()
        .map(|key| usage.get(*key).and_then(Value::as_i64).unwrap_or(0))
        .sum()
}

fn stringify_tool_result(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| item.as_str().map(str::to_string))
                    .or_else(|| serde_json::to_string(item).ok())
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Null) | None => String::new(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn flush_assistant(
    pending: &mut Option<PendingAssistant>,
    messages: &mut Vec<TranscriptMessage>,
    tool_index: &mut HashMap<String, (usize, usize)>,
) {
    let Some(pending) = pending.take() else {
        return;
    };
    let text = pending.text.join("\n\n");
    if text.trim().is_empty() && pending.tool_calls.is_empty() && pending.thinking.is_empty() {
        return;
    }
    let (text, truncated) = clip(text.trim(), MAX_MSG_TEXT);
    let thinking =
        (!pending.thinking.is_empty()).then(|| clip(&pending.thinking.join("\n\n"), MAX_TOOL_IO).0);
    let message_index = messages.len();
    for (tool_index_in_message, tool) in pending.tool_calls.iter().enumerate() {
        if !tool.id.is_empty() {
            tool_index.insert(tool.id.clone(), (message_index, tool_index_in_message));
        }
    }
    messages.push(TranscriptMessage {
        seq: 0,
        role: Role::Assistant,
        kind: MessageKind::Text,
        text,
        truncated,
        tool_calls: pending.tool_calls,
        thinking,
        timestamp: pending.timestamp,
        model: pending.model,
    });
}

/// Resolve the active branch from the UUID tree. Broken/missing leaves fall
/// back to the last stored node; explicit null means an intentionally empty
/// transcript. A visited set prevents malformed cycles from hanging scans.
fn active_chain(
    nodes: &HashMap<String, MessageNode>,
    order: &[String],
    active_leaf: &ActiveLeaf,
    unknown: &mut u32,
) -> Vec<Value> {
    let mut cursor = match active_leaf {
        ActiveLeaf::Empty => return Vec::new(),
        ActiveLeaf::Uuid(id) if nodes.contains_key(id) => id.clone(),
        ActiveLeaf::Missing | ActiveLeaf::Uuid(_) => match order.last() {
            Some(id) => id.clone(),
            None => return Vec::new(),
        },
    };
    let mut seen = HashSet::new();
    let mut chain_ids = Vec::new();
    loop {
        if !seen.insert(cursor.clone()) {
            *unknown += 1;
            break;
        }
        let Some(node) = nodes.get(&cursor) else {
            *unknown += 1;
            break;
        };
        chain_ids.push(cursor.clone());
        let Some(parent) = node.parent.as_ref() else {
            break;
        };
        cursor = parent.clone();
    }
    chain_ids.reverse();

    // Assistant fragments share message.id but may be siblings in the UUID
    // tree. Include all fragments belonging to an active message, then attach
    // their tool_result children so the transcript has one coherent message.
    let mut fragments_by_message: HashMap<String, Vec<String>> = HashMap::new();
    let mut message_by_fragment: HashMap<String, String> = HashMap::new();
    for id in order {
        let Some(node) = nodes.get(id) else {
            continue;
        };
        if node.row.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(message_id) = optional_string(node.row.pointer("/message/id")) else {
            continue;
        };
        fragments_by_message
            .entry(message_id.clone())
            .or_default()
            .push(id.clone());
        message_by_fragment.insert(id.clone(), message_id);
    }
    let mut results_by_message: HashMap<String, Vec<String>> = HashMap::new();
    for id in order {
        let Some(node) = nodes.get(id) else {
            continue;
        };
        let is_tool_result = node.row.get("type").and_then(Value::as_str) == Some("user")
            && node
                .row
                .pointer("/message/content")
                .and_then(Value::as_array)
                .is_some_and(|blocks| {
                    blocks.iter().any(|block| {
                        block.get("type").and_then(Value::as_str) == Some("tool_result")
                    })
                });
        if !is_tool_result {
            continue;
        }
        let Some(message_id) = optional_string(node.row.get("parentUuid"))
            .and_then(|parent| message_by_fragment.get(&parent).cloned())
        else {
            continue;
        };
        results_by_message
            .entry(message_id)
            .or_default()
            .push(id.clone());
    }

    let mut output = Vec::new();
    let mut emitted = HashSet::new();
    for id in chain_ids {
        let Some(node) = nodes.get(&id) else {
            continue;
        };
        let Some(message_id) = message_by_fragment.get(&id) else {
            if emitted.insert(id) {
                output.push(node.row.clone());
            }
            continue;
        };
        if let Some(fragment_ids) = fragments_by_message.get(message_id) {
            for fragment_id in fragment_ids {
                if emitted.insert(fragment_id.clone()) {
                    if let Some(fragment) = nodes.get(fragment_id) {
                        output.push(fragment.row.clone());
                    }
                }
            }
        }
        if let Some(result_ids) = results_by_message.get(message_id) {
            for result_id in result_ids {
                if emitted.insert(result_id.clone()) {
                    if let Some(result) = nodes.get(result_id) {
                        output.push(result.row.clone());
                    }
                }
            }
        }
    }
    output
}

fn parse_qoder_jsonl(path: &Path) -> Result<QoderParse> {
    let file = fs::File::open(path)?;
    let reader = BufReader::with_capacity(1 << 20, file);
    let mut nodes: HashMap<String, MessageNode> = HashMap::new();
    let mut order = Vec::new();
    let mut active_leaf = ActiveLeaf::Missing;
    let mut custom_title = String::new();
    let mut ai_title = String::new();
    let mut last_prompt = String::new();
    let mut relocated_cwd = String::new();
    let mut worktree_cwd = String::new();
    let mut worktree_branch: Option<String> = None;
    let mut runtime_model: Option<String> = None;
    let mut unknown = 0u32;

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
        let row_type = row.get("type").and_then(Value::as_str).unwrap_or("");
        match row_type {
            "custom-title" => {
                if let Some(title) = optional_string(row.get("customTitle")) {
                    custom_title = title;
                }
            }
            "ai-title" => {
                if let Some(title) = optional_string(row.get("aiTitle")) {
                    ai_title = title;
                }
            }
            "last-prompt" => {
                if let Some(prompt) = optional_string(row.get("lastPrompt")) {
                    last_prompt = prompt;
                }
            }
            "active-leaf" => match row.get("leafUuid") {
                Some(Value::Null) => active_leaf = ActiveLeaf::Empty,
                Some(Value::String(id)) => active_leaf = ActiveLeaf::Uuid(id.clone()),
                _ => unknown += 1,
            },
            "relocated" => {
                if let Some(cwd) = optional_string(row.get("relocatedCwd")) {
                    relocated_cwd = cwd;
                }
            }
            "worktree-state" => match row.get("worktreeSession") {
                Some(Value::Null) => {
                    worktree_cwd.clear();
                    worktree_branch = None;
                }
                Some(Value::Object(worktree)) => {
                    worktree_cwd =
                        optional_string(worktree.get("worktreePath")).unwrap_or_default();
                    worktree_branch = optional_string(worktree.get("worktreeBranch"));
                }
                _ => unknown += 1,
            },
            "runtime-config" => {
                runtime_model = optional_string(row.get("model"))
                    .or_else(|| optional_string(row.pointer("/model/name")));
            }
            // Attachments can be parents of later messages, so retain them in
            // the tree but omit them from the visible transcript.
            "user" | "assistant" | "system" | "attachment" => {
                if row.get("isSidechain").and_then(Value::as_bool) == Some(true) {
                    continue;
                }
                let Some(uuid) = optional_string(row.get("uuid")) else {
                    unknown += 1;
                    continue;
                };
                let parent = optional_string(row.get("logicalParentUuid"))
                    .or_else(|| optional_string(row.get("parentUuid")));
                if !nodes.contains_key(&uuid) {
                    order.push(uuid.clone());
                }
                nodes.insert(uuid, MessageNode { row, parent });
            }
            other if KNOWN_METADATA_TYPES.contains(&other) => {}
            _ => unknown += 1,
        }
    }

    let chain = active_chain(&nodes, &order, &active_leaf, &mut unknown);
    let mut messages = Vec::new();
    let mut pending: Option<PendingAssistant> = None;
    let mut tool_index: HashMap<String, (usize, usize)> = HashMap::new();
    let mut usage_seen = HashSet::new();
    let mut cwd = String::new();
    let mut git_branch = None;
    let mut model = runtime_model;
    let mut tokens_used = 0i64;
    let mut created_at = 0i64;
    let mut updated_at = 0i64;

    for row in chain {
        let row_type = row.get("type").and_then(Value::as_str).unwrap_or("");
        if let Some(value) = optional_string(row.get("cwd")) {
            cwd = value;
        }
        if let Some(value) = optional_string(row.get("gitBranch")) {
            git_branch = Some(value);
        }
        let timestamp = row.get("timestamp").map(to_epoch_ms).unwrap_or(0);
        if timestamp > 0 {
            if created_at == 0 {
                created_at = timestamp;
            }
            updated_at = updated_at.max(timestamp);
        }

        if row_type == "system" {
            flush_assistant(&mut pending, &mut messages, &mut tool_index);
            if row.get("subtype").and_then(Value::as_str) == Some("compact_boundary") {
                messages.push(TranscriptMessage {
                    seq: 0,
                    role: Role::System,
                    kind: MessageKind::CompactSummary,
                    text: "── Context compacted ──".to_string(),
                    truncated: false,
                    tool_calls: Vec::new(),
                    thinking: None,
                    timestamp: (timestamp > 0).then_some(timestamp),
                    model: None,
                });
            }
            continue;
        }
        if row_type == "attachment" {
            continue;
        }

        let Some(message) = row.get("message") else {
            unknown += 1;
            continue;
        };
        if row_type == "user" {
            flush_assistant(&mut pending, &mut messages, &mut tool_index);
            let mut parts = Vec::new();
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
                            Some("image") => parts.push("[image]".to_string()),
                            Some("tool_result") => {
                                let id = block
                                    .get("tool_use_id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                if let Some(&(message_index, tool_index_in_message)) =
                                    tool_index.get(id)
                                {
                                    if let Some(tool) =
                                        messages.get_mut(message_index).and_then(|message| {
                                            message.tool_calls.get_mut(tool_index_in_message)
                                        })
                                    {
                                        tool.output = Some(
                                            clip(
                                                &stringify_tool_result(block.get("content")),
                                                MAX_TOOL_IO,
                                            )
                                            .0,
                                        );
                                        tool.is_error = block
                                            .get("is_error")
                                            .and_then(Value::as_bool)
                                            .unwrap_or(false);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            let text = parts.join("\n\n").trim().to_string();
            if text.is_empty() {
                continue;
            }
            let compact = row.get("isCompactSummary").and_then(Value::as_bool) == Some(true);
            let meta = row.get("isMeta").and_then(Value::as_bool) == Some(true)
                || row
                    .get("isVisibleInTranscriptOnly")
                    .and_then(Value::as_bool)
                    == Some(true)
                || row
                    .pointer("/origin/kind")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind != "human")
                || is_injected_user_content(&text);
            let kind = if compact {
                MessageKind::CompactSummary
            } else if meta {
                MessageKind::Meta
            } else {
                MessageKind::Text
            };
            let (text, truncated) = clip(&text, MAX_MSG_TEXT);
            messages.push(TranscriptMessage {
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
            continue;
        }

        let message_id = optional_string(message.get("id"));
        let need_new = match (&pending, &message_id) {
            (None, _) => true,
            (Some(current), Some(id)) => current.message_id.as_deref().is_some_and(|old| old != id),
            (Some(_), None) => false,
        };
        if need_new {
            flush_assistant(&mut pending, &mut messages, &mut tool_index);
            pending = Some(PendingAssistant {
                message_id: message_id.clone(),
                timestamp: (timestamp > 0).then_some(timestamp),
                ..Default::default()
            });
        }
        let Some(current) = pending.as_mut() else {
            continue;
        };
        if current.message_id.is_none() {
            current.message_id = message_id.clone();
        }
        if let Some(value) = optional_string(message.get("model")) {
            if value != "<synthetic>" {
                current.model = Some(value.clone());
                model = Some(value);
            }
        }
        let usage_key = message_id
            .clone()
            .or_else(|| optional_string(row.get("uuid")))
            .unwrap_or_default();
        if usage_seen.insert(usage_key) {
            if let Some(usage) = message.get("usage") {
                tokens_used += usage_tokens(usage);
            }
        }
        match message.get("content") {
            Some(Value::String(text)) if !text.trim().is_empty() => current.text.push(text.clone()),
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
            _ => {}
        }
    }
    flush_assistant(&mut pending, &mut messages, &mut tool_index);
    assign_seq(&mut messages);

    if !worktree_cwd.is_empty() {
        cwd = worktree_cwd;
    } else if !relocated_cwd.is_empty() {
        cwd = relocated_cwd;
    }
    if worktree_branch.is_some() {
        git_branch = worktree_branch;
    }
    Ok(QoderParse {
        messages,
        custom_title,
        ai_title,
        last_prompt,
        cwd,
        git_branch,
        model,
        tokens_used,
        created_at,
        updated_at,
        unknown_lines: unknown,
    })
}

fn build_meta(reference: &SessionFileRef, parsed: &QoderParse) -> SessionMeta {
    let title = [
        parsed.custom_title.as_str(),
        parsed.ai_title.as_str(),
        parsed.last_prompt.as_str(),
    ]
    .into_iter()
    .map(clean_title_candidate)
    .find(|title| !title.is_empty())
    .or_else(|| title_from_messages(&parsed.messages))
    .unwrap_or_else(|| UNTITLED.to_string());
    SessionMeta {
        key: format!("qoder:{}", reference.native_id),
        id: reference.native_id.clone(),
        agent: AgentId::Qoder,
        title,
        project_path: parsed.cwd.clone(),
        project_name: project_name_of(&parsed.cwd),
        file_path: reference.file_path.clone(),
        created_at: if parsed.created_at > 0 {
            parsed.created_at
        } else {
            reference.mtime_ms
        },
        updated_at: parsed.updated_at.max(reference.mtime_ms),
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

impl AgentHistoryAdapter for QoderAdapter {
    fn agent(&self) -> AgentId {
        AgentId::Qoder
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        Ok(list_refs(&self.root))
    }

    fn file_ref(&self, path: &Path) -> Option<SessionFileRef> {
        let relative = path.strip_prefix(&self.root).ok()?;
        if relative.components().count() > 2
            || relative
                .components()
                .any(|component| is_sidecar_dir(Path::new(component.as_os_str())))
        {
            return None;
        }
        default_file_ref(self.agent(), path)
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession> {
        let parsed = parse_qoder_jsonl(Path::new(&reference.file_path))?;
        Ok(ParsedSession {
            meta: build_meta(reference, &parsed),
            units: units_from_messages(&parsed.messages),
            unknown_line_count: parsed.unknown_lines,
        })
    }

    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript> {
        let parsed = parse_qoder_jsonl(Path::new(&reference.file_path))?;
        Ok(ParsedTranscript::simple(
            build_meta(reference, &parsed),
            parsed.messages,
            parsed.unknown_lines,
        ))
    }

    fn with_custom_root(&self, root: PathBuf) -> Box<dyn AgentHistoryAdapter> {
        let root = if root.join("projects").is_dir() {
            root.join("projects")
        } else {
            root
        };
        Box::new(Self { root })
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        vec![self.root.clone()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn env_root_is_selected_only_when_it_contains_a_session() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let default = temp.path().join("default");
        let configured = temp.path().join("configured");
        fs::create_dir_all(configured.join("project"))
            .unwrap_or_else(|error| panic!("mkdir: {error}"));
        fs::write(configured.join("project/session.jsonl"), "{}\n")
            .unwrap_or_else(|error| panic!("write: {error}"));
        assert_eq!(select_root(default.clone(), None), default);
        assert_eq!(select_root(default.clone(), Some(default.clone())), default);
        assert_eq!(select_root(default, Some(configured.clone())), configured);
    }

    #[test]
    fn qoder_lists_only_project_direct_sessions_and_excludes_sidecars() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let project = temp.path().join("project-key");
        fs::create_dir_all(project.join("state/subagents"))
            .unwrap_or_else(|error| panic!("mkdir: {error}"));
        fs::write(project.join("session.jsonl"), "{}\n")
            .unwrap_or_else(|error| panic!("write: {error}"));
        fs::write(project.join("state/subagents/child.jsonl"), "{}\n")
            .unwrap_or_else(|error| panic!("write: {error}"));
        let adapter = QoderAdapter::with_root(temp.path().to_path_buf());
        let references = adapter
            .list_session_files()
            .unwrap_or_else(|error| panic!("list: {error}"));
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].native_id, "session");

        // A user may choose the project-key directory itself. Its sidecars
        // must remain excluded at that custom-root depth as well.
        let project_adapter = QoderAdapter::with_root(project.clone());
        let project_refs = project_adapter
            .list_session_files()
            .unwrap_or_else(|error| panic!("list custom project: {error}"));
        assert_eq!(project_refs.len(), 1);
        assert!(project_adapter
            .file_ref(&project.join("state/subagents/child.jsonl"))
            .is_none());
    }

    #[test]
    fn active_leaf_replays_branch_fragments_and_tool_results() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let project = temp.path().join("project-key");
        fs::create_dir_all(&project).unwrap_or_else(|error| panic!("mkdir: {error}"));
        let path = project.join("session.jsonl");
        let content = concat!(
            r#"{"type":"user","uuid":"u1","cwd":"/work/qoder","timestamp":"2026-08-29T00:00:00Z","message":{"content":"Question"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-08-29T00:00:01Z","message":{"id":"m1","model":"qoder-model","content":[{"type":"thinking","thinking":"Think"},{"type":"tool_use","id":"tool-1","name":"Read","input":{"path":"src/lib.rs"}}],"usage":{"input_tokens":4,"output_tokens":3}}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"a1","timestamp":"2026-08-29T00:00:02Z","message":{"id":"m1","content":[{"type":"text","text":"Answer"}]}}"#,
            "\n",
            r#"{"type":"user","uuid":"r1","parentUuid":"a2","timestamp":"2026-08-29T00:00:03Z","message":{"content":[{"type":"tool_result","tool_use_id":"tool-1","content":"source"}]}}"#,
            "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"u1","message":{"content":"Other branch"}}"#,
            "\n",
            r#"{"type":"active-leaf","leafUuid":"a2"}"#,
            "\n",
            r#"{"type":"custom-title","customTitle":"Qoder title"}"#,
            "\n",
        );
        fs::write(&path, content).unwrap_or_else(|error| panic!("write: {error}"));
        let adapter = QoderAdapter::with_root(temp.path().to_path_buf());
        let reference = adapter
            .list_session_files()
            .unwrap_or_else(|error| panic!("list: {error}"))
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("missing session"));
        let transcript = adapter
            .parse_transcript(&reference)
            .unwrap_or_else(|error| panic!("parse: {error}"));
        assert_eq!(transcript.meta.title, "Qoder title");
        assert_eq!(transcript.meta.project_path, "/work/qoder");
        assert_eq!(transcript.meta.model.as_deref(), Some("qoder-model"));
        assert_eq!(transcript.meta.tokens_used, Some(7));
        assert_eq!(transcript.mainline.len(), 2);
        assert!(transcript
            .mainline
            .iter()
            .all(|message| !message.text.contains("Other branch")));
        let assistant = transcript
            .mainline
            .iter()
            .find(|message| message.role == Role::Assistant)
            .unwrap_or_else(|| panic!("missing assistant"));
        assert_eq!(assistant.text, "Answer");
        assert_eq!(assistant.thinking.as_deref(), Some("Think"));
        assert_eq!(assistant.tool_calls.len(), 1);
        assert_eq!(assistant.tool_calls[0].output.as_deref(), Some("source"));
    }

    #[test]
    fn broken_or_cyclic_active_leaf_is_bounded_and_falls_back() {
        let mut nodes = HashMap::new();
        nodes.insert(
            "a".to_string(),
            MessageNode {
                row: serde_json::json!({"type":"user","uuid":"a","message":{"content":"A"}}),
                parent: Some("a".to_string()),
            },
        );
        let mut unknown = 0;
        let output = active_chain(
            &nodes,
            &["a".to_string()],
            &ActiveLeaf::Uuid("missing".to_string()),
            &mut unknown,
        );
        assert_eq!(output.len(), 1);
        assert!(
            unknown > 0,
            "cycle detection must account for malformed parent"
        );
        let output = active_chain(
            &nodes,
            &["a".to_string()],
            &ActiveLeaf::Uuid("a".to_string()),
            &mut unknown,
        );
        assert_eq!(output.len(), 1);
        assert!(unknown > 0);
    }
}
