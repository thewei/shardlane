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

/// DeepSeek Harness (dsh) session format:
/// `~/.dsh/sessions/--<escaped cwd>--/<escaped id>/session.jsonl[.zstd]`, one
/// directory per session with fixed file names. The default on-disk form is a
/// zstd **multiframe stream** (first frame is the header line, then one frame
/// per append); a standard streaming decode to EOF suffices. With the
/// `compression: none` configuration it is a plain-text .jsonl instead; both
/// suffixes are accepted. The first line {type:"session",id,createdAt,cwd,…}
/// provides the authoritative id/cwd/created — never inferred from the
/// directory name; origin=="subagent" or delegationDepth>0 marks a subagent
/// session, which is not listed. Event lines are {type,seq,time,data}:
/// assistant/message is the final composition of streamed chunks (assistant/
/// chunk and its packed *-chunks lines are skipped directly), tool/result
/// back-fills by toolCallId, session/title events are last-wins for the
/// title, and the model lives in the assistant message's source.
pub struct DshAdapter {
    root: PathBuf,
}

impl DshAdapter {
    pub fn new() -> Self {
        Self {
            root: home_dir().join(".dsh/sessions"),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: PathBuf) -> Self {
        Self { root }
    }
}

impl Default for DshAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Open a session log as a line reader, transparently decompressing by suffix
/// (.zstd = multiframe stream; the Decoder decodes to EOF by default). `cap`
/// is the buffer size: header-only paths use a small value so every listing
/// does not prefetch megabytes.
fn open_log(path: &Path, cap: usize) -> Result<Box<dyn BufRead>> {
    let file = fs::File::open(path)?;
    if path
        .extension()
        .is_some_and(|extension| extension == "zstd")
    {
        let decoder =
            zstd::stream::read::Decoder::with_buffer(BufReader::with_capacity(cap, file))?;
        Ok(Box::new(BufReader::with_capacity(cap, decoder)))
    } else {
        Ok(Box::new(BufReader::with_capacity(cap, file)))
    }
}

struct DshHeader {
    id: String,
    cwd: String,
    created_at: i64,
    subagent: bool,
}

fn parse_header(value: &serde_json::Value) -> Option<DshHeader> {
    if value.get("type").and_then(|kind| kind.as_str()) != Some("session") {
        return None;
    }
    Some(DshHeader {
        id: value.get("id")?.as_str()?.to_string(),
        cwd: value
            .get("cwd")
            .and_then(|cwd| cwd.as_str())
            .unwrap_or_default()
            .to_string(),
        created_at: value
            .get("createdAt")
            .and_then(|created| created.as_i64())
            .unwrap_or(0),
        subagent: value.get("origin").and_then(|origin| origin.as_str()) == Some("subagent")
            || value
                .get("delegationDepth")
                .and_then(|depth| depth.as_i64())
                .unwrap_or(0)
                > 0,
    })
}

/// Read only the first line to obtain the header (zstd lazy decoding never
/// decodes the whole file; non-session logs return None).
fn read_header(path: &Path) -> Option<DshHeader> {
    let mut reader = open_log(path, 8 << 10).ok()?;
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    parse_header(&serde_json::from_str(line.trim_end()).ok()?)
}

/// The complete event vocabulary of the current dsh version beyond content
/// events (source: known-event-types.ts), plus the three chunk packed-storage
/// lines. New upstream vocabulary counts as unknown to prompt follow-up.
const KNOWN_SKIP: &[&str] = &[
    "agent-preset/selected",
    "agent/inbox/spliced",
    "approval/asked",
    "approval/decided",
    "approval/policy",
    "assistant/chunk",
    "command/done",
    "command/run",
    "compaction/end",
    "compaction/prune",
    "compaction/start",
    "compaction/summary",
    "feedback/record",
    "goal/change",
    "hook/invoked",
    "hook/result",
    "llm/retry",
    "llm/retry-started",
    "permission/preset",
    "plan/mode",
    "request/header",
    "sandbox/mode",
    "schedule/change",
    "session/end-seed",
    "session/title-llm-request",
    "step/end",
    "step/start",
    "subagent/descriptor",
    "team/member",
    "team/message/delivered",
    "team/message/queued",
    "team/task",
    "todo/write",
    "tool-workflow/agent-end",
    "tool-workflow/agent-start",
    "tool-workflow/run-end",
    "tool-workflow/run-start",
    "tool/call",
    "tool/code-dispatch",
    "tool/code-dispatch-start",
    "turn/end",
    "turn/start",
    "web/deepseek-search-llm-request",
    "text-chunks",
    "reasoning-chunks",
    "tool-call-chunks",
];

struct DshParse {
    header: Option<DshHeader>,
    title: Option<String>,
    last_ts: i64,
    messages: Vec<TranscriptMessage>,
    model: Option<String>,
    tokens_used: Option<i64>,
    unknown_lines: u32,
}

fn parse_log(path: &Path) -> Result<DshParse> {
    let reader = open_log(path, 1 << 20)?;

    let mut parsed = DshParse {
        header: None,
        title: None,
        last_ts: 0,
        messages: Vec::new(),
        model: None,
        tokens_used: None,
        unknown_lines: 0,
    };
    // toolCallId → (message index, tool_calls index), used to back-fill
    // tool/result events.
    let mut tool_index: HashMap<String, (usize, usize)> = HashMap::new();

    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            // The final frame may be half-written (the writer appends one
            // frame per write, so scanning and dsh are naturally concurrent):
            // the zstd decoder returns UnexpectedEof **repeatedly** for a
            // truncated tail rather than EOF, so `continue` would be an
            // infinite loop — the scan thread pegs the CPU and refresh never
            // finishes. Truncate here; the already-parsed part is presented
            // as usual.
            Err(_) => {
                parsed.unknown_lines += 1;
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(row) = serde_json::from_str::<serde_json::Value>(&line) else {
            parsed.unknown_lines += 1;
            continue;
        };
        let event_type = row
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        // The envelope's top-level surfaceOp is a union type: replace is an
        // object, not a string, so a string comparison never matches.
        // Compaction and tool-result trimming both use it to swap the
        // start..end surface nodes for a shortened version — that is the
        // context fed to the model. Shardlane wants to restore what the user
        // originally saw, so such events are skipped entirely (leaving the
        // original nodes they shadowed). Missing or append are treated as
        // usual.
        if row
            .pointer("/surfaceOp/op")
            .and_then(|value| value.as_str())
            == Some("replace")
        {
            continue;
        }
        let timestamp = row
            .get("time")
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        parsed.last_ts = parsed.last_ts.max(timestamp);
        let data = row.get("data").unwrap_or(&serde_json::Value::Null);
        match event_type {
            "session" => {
                if let Some(header) = parse_header(&row) {
                    parsed.header = Some(header);
                }
            }
            "user/message" => {
                let text = blocks_text(data.get("content").unwrap_or(&serde_json::Value::Null));
                if text.is_empty() {
                    continue;
                }
                let kind = data.pointer("/source/kind").and_then(|kind| kind.as_str());
                let mut message = text_msg(Role::User, &text, timestamp);
                // dsh's source.kind authoritatively distinguishes real human
                // input from injected context (measured family:
                // "agent-instructions"/"plugin"/"skill-catalog", and it keeps
                // growing). Whitelist "user": anything not human goes to
                // Meta; a missing kind is treated as human (better to show
                // more).
                if kind.is_some_and(|kind| kind != "user") {
                    message.kind = MessageKind::Meta;
                }
                parsed.messages.push(message);
            }
            "assistant/message" => {
                let message = data.get("message").unwrap_or(&serde_json::Value::Null);
                let content = message.get("content").unwrap_or(&serde_json::Value::Null);
                // Single-pass bucketing: text into the body, reasoning into
                // thinking, tool-call into tools (blocks_text is type-blind
                // and would mix reasoning text into the body, so it does not
                // apply).
                let mut text_parts: Vec<&str> = Vec::new();
                let mut thinking_parts: Vec<&str> = Vec::new();
                let mut tools: Vec<ToolCallView> = Vec::new();
                for block in content.as_array().into_iter().flatten() {
                    let block_text = || {
                        block
                            .get("text")
                            .and_then(|value| value.as_str())
                            .map(str::trim)
                            .filter(|text| !text.is_empty())
                    };
                    match block.get("type").and_then(|value| value.as_str()) {
                        Some("text") => text_parts.extend(block_text()),
                        Some("reasoning") => thinking_parts.extend(block_text()),
                        Some("tool-call") => {
                            let id = block
                                .get("id")
                                .and_then(|value| value.as_str())
                                .unwrap_or_default();
                            let name = block
                                .get("name")
                                .and_then(|value| value.as_str())
                                .unwrap_or_default();
                            // arguments is the JSON string exactly as the
                            // model emitted it; on parse failure keep the
                            // raw text.
                            let raw = block
                                .get("arguments")
                                .and_then(|value| value.as_str())
                                .unwrap_or_default();
                            let input = serde_json::from_str::<serde_json::Value>(raw)
                                .unwrap_or(serde_json::Value::String(raw.to_string()));
                            tools.push(tool_call_view(id.to_string(), name, &input, None, false));
                        }
                        _ => {}
                    }
                }
                // Collect metadata before the empty check: an assistant/message
                // with empty content is dsh's dedicated carrier for usage
                // ("exists only to host usage", e.g. a call that hit
                // max-tokens); continuing early would drop this round's model
                // and tokens too — most visible when it lands at the end of a
                // session.
                let model = message
                    .pointer("/source/model")
                    .and_then(|value| value.as_str())
                    .map(String::from);
                if model.is_some() {
                    parsed.model = model.clone();
                }
                if let Some(usage) = data.get("usage") {
                    // TokenUsage accounts for "one model call" and the three
                    // input items are mutually exclusive (billed = their
                    // sum), so accumulate **per call**; last-wins would
                    // leave only the final call's amount when one turn has
                    // multiple steps.
                    let sum = [
                        "inputTokens",
                        "outputTokens",
                        "cacheReadTokens",
                        "cacheWriteTokens",
                    ]
                    .iter()
                    .filter_map(|key| usage.get(*key).and_then(|value| value.as_i64()))
                    .sum::<i64>();
                    if sum > 0 {
                        parsed.tokens_used = Some(parsed.tokens_used.unwrap_or(0) + sum);
                    }
                }
                if text_parts.is_empty() && tools.is_empty() && thinking_parts.is_empty() {
                    continue;
                }
                let text = text_parts.join("\n\n");
                // One turn has multiple steps, each step one assistant/message;
                // consecutive assistant lines (separated only by tool/result)
                // merge into one, so the detail page shows one assistant
                // message per round (the merge + whole-body clip mechanism
                // mirrors pi.rs; changes must be applied to both sides).
                if !matches!(parsed.messages.last(), Some(message) if message.role == Role::Assistant)
                {
                    parsed
                        .messages
                        .push(text_msg(Role::Assistant, "", timestamp));
                }
                let base = parsed.messages.len() - 1;
                let last = &mut parsed.messages[base];
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
                // thinking is clamped to MAX_TOOL_IO like the other adapters;
                // once the cap is reached, nothing more is appended.
                if !thinking_parts.is_empty()
                    && last.thinking.as_deref().map_or(0, str::len) < MAX_TOOL_IO
                {
                    let joined = thinking_parts.join("\n\n");
                    match &mut last.thinking {
                        Some(existing) => {
                            existing.push_str("\n\n");
                            existing.push_str(&joined);
                            if existing.len() > MAX_TOOL_IO {
                                *existing = clip(existing, MAX_TOOL_IO).0;
                            }
                        }
                        slot => *slot = Some(clip(&joined, MAX_TOOL_IO).0),
                    }
                }
                if model.is_some() {
                    last.model = model;
                }
                for tool_call in tools {
                    tool_index.insert(tool_call.id.clone(), (base, last.tool_calls.len()));
                    last.tool_calls.push(tool_call);
                }
            }
            "tool/result" => {
                // data.message.content = [tool-result block]: toolCallId +
                // nested content + isError.
                let Some(block) = data.pointer("/message/content/0") else {
                    continue;
                };
                let Some(call_id) = block.get("toolCallId").and_then(|value| value.as_str()) else {
                    continue;
                };
                if let Some(&(message_index, tool_position)) = tool_index.get(call_id) {
                    let tool_call = &mut parsed.messages[message_index].tool_calls[tool_position];
                    let text =
                        blocks_text(block.get("content").unwrap_or(&serde_json::Value::Null));
                    if !text.is_empty() {
                        tool_call.output = Some(clip(&text, MAX_TOOL_IO).0);
                    }
                    if block.get("isError").and_then(|value| value.as_bool()) == Some(true)
                        || data.get("error").is_some_and(|error| !error.is_null())
                    {
                        tool_call.is_error = true;
                    }
                }
            }
            "session/title" => {
                if let Some(title) = data.get("title").and_then(|value| value.as_str()) {
                    let title = title.trim();
                    if !title.is_empty() {
                        parsed.title = Some(clean_title_candidate(title));
                    }
                }
            }
            "request/context" => {
                // Routing metadata (recorded only on change);
                // assistant/message's source.model overrides it.
                if parsed.model.is_none() {
                    if let Some(model) = data.get("model").and_then(|value| value.as_str()) {
                        parsed.model = Some(model.to_string());
                    }
                }
            }
            // Envelope ignorable flag = writer-declared purely informational
            // record, safe for readers to skip.
            _ if row.get("ignorable").and_then(|value| value.as_bool()) == Some(true) => {}
            // The rest of the current version's known vocabulary (including
            // the three assistant/chunk packed-storage lines) is listed
            // explicitly; anything outside the vocabulary counts as unknown —
            // preserving the schema-drift canary.
            known if KNOWN_SKIP.contains(&known) => {}
            _ => {
                parsed.unknown_lines += 1;
            }
        }
    }
    assign_seq(&mut parsed.messages);
    Ok(parsed)
}

fn build_meta(reference: &SessionFileRef, parsed: &DshParse) -> SessionMeta {
    let (native, cwd, created) = match &parsed.header {
        Some(header) => (header.id.clone(), header.cwd.clone(), header.created_at),
        None => (reference.native_id.clone(), String::new(), 0),
    };
    let title = parsed
        .title
        .clone()
        .filter(|title| !title.is_empty())
        .or_else(|| title_from_messages(&parsed.messages))
        .unwrap_or_else(|| UNTITLED.to_string());
    SessionMeta {
        key: format!("dsh:{native}"),
        id: native,
        agent: AgentId::Dsh,
        title,
        project_path: cwd.clone(),
        project_name: project_name_of(&cwd),
        file_path: reference.file_path.clone(),
        created_at: if created > 0 {
            created
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

impl AgentHistoryAdapter for DshAdapter {
    fn agent(&self) -> AgentId {
        AgentId::Dsh
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        let mut references = Vec::new();
        // Fixed two levels: <project-dir>/<session-dir>/session.jsonl[.zstd];
        // no walkdir full recursion (session directories hold other artifacts
        // too). If the root is unreadable, degrade this adapter to empty
        // rather than blowing up the whole scan round.
        let Ok(projects) = fs::read_dir(&self.root) else {
            return Ok(references);
        };
        for project in projects.flatten() {
            if !project.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            for session in fs::read_dir(project.path()).into_iter().flatten().flatten() {
                if !session.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                // Both candidate names go through the file_ref funnel; when
                // both suffixes exist, the sibling arbitration guarantees at
                // most one passes.
                let dir = session.path();
                for name in ["session.jsonl.zstd", "session.jsonl"] {
                    if let Some(reference) = self.file_ref(&dir.join(name)) {
                        references.push(reference);
                        break;
                    }
                }
            }
        }
        Ok(references)
    }

    fn file_ref(&self, path: &Path) -> Option<SessionFileRef> {
        let name = path.file_name()?.to_string_lossy();
        if name != "session.jsonl" && name != "session.jsonl.zstd" {
            return None;
        }
        let meta = fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() == 0 {
            return None;
        }
        // Switching the compression configuration leaves old and new suffixes
        // coexisting: the stale one yields (on an mtime tie .zstd wins,
        // matching the writer's current default). Arbitration lives at this
        // single point, shared by the list and watcher entry paths — doing it
        // only in list would let the watcher parse a stale sibling's events
        // as the main file.
        let sibling = if name == "session.jsonl" {
            "session.jsonl.zstd"
        } else {
            "session.jsonl"
        };
        if let Ok(sibling_meta) = fs::metadata(path.with_file_name(sibling)) {
            let (own_mtime, sibling_mtime) = (mtime_ms(&meta), mtime_ms(&sibling_meta));
            if sibling_mtime > own_mtime || (sibling_mtime == own_mtime && name == "session.jsonl")
            {
                return None;
            }
        }
        // The first-line header provides the authoritative id (the directory
        // name is an escaped id); subagent sessions are filtered here.
        let header = read_header(path).filter(|header| !header.subagent)?;
        Some(SessionFileRef {
            agent: AgentId::Dsh,
            native_id: header.id,
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
