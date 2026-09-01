// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.
//! [INPUT]: JSON/text lines from provider parsers and the user-directory
//! environment.
//! [OUTPUT]: Time, text, title, path, and home-directory parsing utilities
//! shared across adapters.
//! [POS]: Pure parsing helper layer inside adapters; never scans directories
//! directly or writes any persistent state.

use crate::models::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Resolve the user home once per adapter construction. Explicit environment
/// values keep tests and portable launches deterministic; platform dirs remains
/// the final fallback.
pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_default()
}

/// Return a non-empty environment directory only when explicitly configured.
/// Empty values are treated as unset so a shell-exported placeholder cannot
/// hide the provider's default history root.
pub fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub fn project_name_of(cwd: &str) -> String {
    if cwd.is_empty() {
        return "Unknown project".to_string();
    }
    Path::new(cwd)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "Unknown project".to_string())
}

pub fn user_kind(text: &str) -> MessageKind {
    if is_injected_user_content(text) {
        MessageKind::Meta
    } else {
        MessageKind::Text
    }
}

pub fn text_msg(role: Role, text: &str, timestamp: i64) -> TranscriptMessage {
    let (text, truncated) = clip(text.trim(), MAX_MSG_TEXT);
    TranscriptMessage {
        seq: 0,
        role,
        kind: if role == Role::User {
            user_kind(&text)
        } else {
            MessageKind::Text
        },
        text,
        truncated,
        tool_calls: Vec::new(),
        thinking: None,
        timestamp: (timestamp > 0).then_some(timestamp),
        model: None,
    }
}

pub fn assign_seq(messages: &mut [TranscriptMessage]) {
    for (index, message) in messages.iter_mut().enumerate() {
        message.seq = index as i64;
    }
}

pub fn title_from_messages(messages: &[TranscriptMessage]) -> Option<String> {
    messages
        .iter()
        .find(|message| message.role == Role::User && message.kind == MessageKind::Text)
        .map(|message| clean_title_candidate(&message.text))
        .filter(|title| !title.is_empty())
}

pub fn default_file_ref(agent: AgentId, path: &Path) -> Option<SessionFileRef> {
    let name = path.file_name()?.to_string_lossy();
    if name.starts_with('.') {
        return None;
    }
    let stem = name.strip_suffix(".jsonl")?;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() == 0 {
        return None;
    }
    Some(SessionFileRef {
        agent,
        native_id: stem.to_string(),
        file_path: path.to_string_lossy().to_string(),
        mtime_ms: mtime_ms(&meta),
        size: meta.len() as i64,
    })
}

pub fn list_jsonl_refs(
    dir: &Path,
    agent: AgentId,
    native_id: impl Fn(&str) -> String,
) -> anyhow::Result<Vec<SessionFileRef>> {
    // Missing root = not installed/deleted: a legitimate empty set (cleanup
    // semantics stay correct).
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut references = Vec::new();
    for entry in walkdir::WalkDir::new(dir) {
        // Walk failure = directory contents cannot be confirmed: propagate so
        // the scanner skips this round's cleanup.
        let entry = entry.map_err(|error| anyhow::anyhow!("walk {}: {error}", dir.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        let Some(stem) = name.strip_suffix(".jsonl") else {
            continue;
        };
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.len() == 0 {
            continue;
        }
        references.push(SessionFileRef {
            agent,
            native_id: native_id(stem),
            file_path: entry.path().to_string_lossy().to_string(),
            mtime_ms: mtime_ms(&meta),
            size: meta.len() as i64,
        });
    }
    Ok(references)
}

/// content block array → plain text: join only each block's `text` field;
/// non-text blocks (blobref/toolCall etc.) are skipped naturally. Shared by
/// adapters for block-shaped formats such as pi/dsh.
pub fn blocks_text(value: &Value) -> String {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Single-value cache invalidated by mtime: repeated calls within one scan
/// round reuse sidecar maps/whole SQLite tables. When build returns None the
/// failure is not cached and will be retried next time even with an unchanged
/// mtime; anomalies such as poisoned locks degrade to a cache miss and never
/// panic.
pub struct MtimeCache<T: Clone>(std::sync::Mutex<Option<(i64, T)>>);

impl<T: Clone> MtimeCache<T> {
    pub fn new() -> Self {
        Self(std::sync::Mutex::new(None))
    }

    pub fn get_or_try_build(&self, mtime: i64, build: impl FnOnce() -> Option<T>) -> Option<T> {
        if let Ok(cache) = self.0.lock() {
            if let Some((cached_mtime, value)) = cache.as_ref() {
                if *cached_mtime == mtime {
                    return Some(value.clone());
                }
            }
        }
        let value = build()?;
        if let Ok(mut cache) = self.0.lock() {
            *cache = Some((mtime, value.clone()));
        }
        Some(value)
    }
}

impl<T: Clone> Default for MtimeCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn tool_call_view(
    id: String,
    name: &str,
    input: &Value,
    output: Option<String>,
    is_error: bool,
) -> ToolCallView {
    let input_json = if input.is_null() {
        String::new()
    } else {
        serde_json::to_string_pretty(input).unwrap_or_default()
    };
    ToolCallView {
        id,
        name: if name.is_empty() {
            "tool".to_string()
        } else {
            name.to_string()
        },
        input_preview: make_preview(input),
        input: (!input_json.is_empty()).then(|| clip(&input_json, MAX_TOOL_IO).0),
        output: output.map(|value| clip(&value, MAX_TOOL_IO).0),
        is_error,
        sidechain_ref: None,
    }
}

pub fn clip(text: &str, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text.to_string(), false);
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (format!("{}\n… (truncated)", &text[..end]), true)
}

pub fn iso_ms(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| date.timestamp_millis())
        .unwrap_or(0)
}

pub fn sqlite_dt_ms(value: &str) -> i64 {
    let parsed = iso_ms(value);
    if parsed > 0 {
        return parsed;
    }
    let normalized = value.replace(' ', "T");
    let normalized = if normalized.ends_with('Z') {
        normalized
    } else {
        format!("{normalized}Z")
    };
    iso_ms(&normalized)
}

pub fn to_epoch_ms(value: &Value) -> i64 {
    match value {
        Value::Number(number) => {
            let number = number.as_f64().unwrap_or(0.0);
            if number > 1e12 {
                number as i64
            } else if number > 0.0 {
                (number * 1000.0) as i64
            } else {
                0
            }
        }
        Value::String(value) => iso_ms(value),
        _ => 0,
    }
}

pub fn clean_title_candidate(raw: &str) -> String {
    let mut text = strip_tag_block(raw, "system-reminder");
    text = strip_tag_block(&text, "local-command-caveat");
    text = strip_tag_block(&text, "local-command-stdout");

    let args = extract_tag(&text, "command-args");
    let name = extract_tag(&text, "command-name");
    if args.is_some() || name.is_some() {
        text = args
            .filter(|value| !value.trim().is_empty())
            .or(name)
            .unwrap_or_default();
    }

    let mut output = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '<' {
            output.push(ch);
            continue;
        }
        let mut tag = String::new();
        let mut closed = false;
        for next in chars.by_ref() {
            if next == '>' {
                closed = true;
                break;
            }
            if next == '\n' || tag.len() > 60 {
                break;
            }
            tag.push(next);
        }
        if !closed {
            output.push('<');
            output.push_str(&tag);
        } else {
            output.push(' ');
        }
    }

    let compact = output.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars = compact.chars().collect::<Vec<_>>();
    if chars.len() > MAX_TITLE {
        let mut title = chars[..MAX_TITLE].iter().collect::<String>();
        title.push('…');
        title
    } else {
        compact
    }
}

fn strip_tag_block(text: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let Some(index) = rest.find(&open) else {
            output.push_str(rest);
            return output;
        };
        output.push_str(&rest[..index]);
        output.push(' ');
        let Some(end) = rest[index..].find(&close) else {
            return output;
        };
        rest = &rest[index + end + close.len()..];
    }
}

pub(crate) fn extract_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)?;
    let body = &text[start + open.len()..];
    let end = body.find(&close)?;
    Some(body[..end].to_string())
}

pub fn is_injected_user_content(text: &str) -> bool {
    let text = text.trim_start();
    const PREFIXES: &[&str] = &[
        "<recommended_plugins",
        "<environment_context",
        "<user_instructions",
        "<permissions",
        "<workspace",
        "<system-",
        "<context ",
        "<session_context",
        "IMPORTANT: Do NOT read",
        "Caveat: The messages below",
        "# Files pasted by the user",
    ];
    PREFIXES.iter().any(|prefix| text.starts_with(prefix))
        || text.contains("/.codex/plugins/")
        || (text.contains("/plugins/cache/") && text.contains("SKILL.md"))
}

pub fn make_preview(input: &Value) -> String {
    const MAX: usize = 200;
    let candidate = if let Value::Object(object) = input {
        [
            "command",
            "file_path",
            "path",
            "pattern",
            "query",
            "url",
            "description",
        ]
        .iter()
        .find_map(|key| {
            object
                .get(*key)
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
        })
        .map(str::to_string)
        .or_else(|| serde_json::to_string(input).ok())
    } else if let Value::String(value) = input {
        Some(value.clone())
    } else {
        serde_json::to_string(input).ok()
    };
    let single_line = candidate
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let chars = single_line.chars().collect::<Vec<_>>();
    if chars.len() > MAX {
        let mut preview = chars[..MAX].iter().collect::<String>();
        preview.push('…');
        preview
    } else {
        single_line
    }
}

pub fn resolve_rows_by_id<'a, R>(
    rows: &'a [R],
    references: &[SessionFileRef],
    id_fn: impl Fn(&'a R) -> &'a str,
    meta_fn: impl Fn(&SessionFileRef, &'a R) -> SessionMeta,
) -> HashMap<String, SessionMeta> {
    let by_id: HashMap<&str, &R> = rows.iter().map(|row| (id_fn(row), row)).collect();
    let mut output = HashMap::new();
    for reference in references {
        if let Some(row) = by_id.get(reference.native_id.as_str()) {
            output.insert(reference.file_path.clone(), meta_fn(reference, row));
        }
    }
    output
}
