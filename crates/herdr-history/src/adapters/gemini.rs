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
use std::sync::Mutex;

pub struct GeminiAdapter {
    root: PathBuf,
    projects_json: PathBuf,
    slug_cache: Mutex<Option<(i64, HashMap<String, String>)>>,
}

impl GeminiAdapter {
    pub fn new() -> Self {
        let home = home_dir().join(".gemini");
        Self {
            root: home.join("tmp"),
            projects_json: home.join("projects.json"),
            slug_cache: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: PathBuf, projects_json: PathBuf) -> Self {
        Self {
            root,
            projects_json,
            slug_cache: Mutex::new(None),
        }
    }

    fn slug_map(&self) -> HashMap<String, String> {
        let mtime = fs::metadata(&self.projects_json)
            .map(|meta| mtime_ms(&meta))
            .unwrap_or(0);
        if let Ok(cache) = self.slug_cache.lock() {
            if let Some((cached_mtime, map)) = cache.as_ref() {
                if *cached_mtime == mtime {
                    return map.clone();
                }
            }
        }
        let mut output = HashMap::new();
        if let Ok(raw) = fs::read_to_string(&self.projects_json) {
            if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                if let Some(Value::Object(projects)) = value.get("projects") {
                    for (path, slug) in projects {
                        if let Some(slug) = slug.as_str() {
                            output.insert(slug.to_string(), path.clone());
                        }
                    }
                }
            }
        }
        if let Ok(mut cache) = self.slug_cache.lock() {
            *cache = Some((mtime, output.clone()));
        }
        output
    }

    fn cwd_for(&self, reference: &SessionFileRef) -> String {
        slug_of(Path::new(&reference.file_path))
            .and_then(|slug| self.slug_map().get(&slug).cloned())
            .unwrap_or_default()
    }
}

impl Default for GeminiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

struct GeminiParse {
    session_id: Option<String>,
    messages: Vec<TranscriptMessage>,
    created_at: i64,
    updated_at: i64,
    unknown_lines: u32,
}

fn parse_jsonl(path: &Path) -> Result<GeminiParse> {
    let file = fs::File::open(path)?;
    let reader = BufReader::with_capacity(1 << 20, file);
    let mut session_id = None;
    let mut created_at = 0;
    let mut updated_at = 0;
    let mut unknown = 0;
    let mut last_set = None;

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
        if let Some(set) = row.get("$set") {
            if set.get("messages").and_then(Value::as_array).is_some() {
                last_set = Some(set.clone());
            }
            continue;
        }
        if let Some(id) = row.get("sessionId").and_then(Value::as_str) {
            session_id = Some(id.to_string());
            created_at = row
                .get("startTime")
                .and_then(Value::as_str)
                .map(iso_ms)
                .unwrap_or(created_at);
            updated_at = row
                .get("lastUpdated")
                .and_then(Value::as_str)
                .map(iso_ms)
                .unwrap_or(updated_at);
            continue;
        }
        unknown += 1;
    }

    let mut messages = Vec::new();
    if let Some(set) = last_set {
        if let Some(items) = set.get("messages").and_then(Value::as_array) {
            for item in items {
                let role = match item.get("type").and_then(Value::as_str) {
                    Some("user") => Role::User,
                    Some(_) => Role::Assistant,
                    None => {
                        unknown += 1;
                        continue;
                    }
                };
                let text = item
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|block| block.get("text").and_then(Value::as_str))
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n");
                if text.is_empty() {
                    continue;
                }
                let timestamp = item
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .map(iso_ms)
                    .unwrap_or(0);
                if timestamp > 0 {
                    if created_at == 0 {
                        created_at = timestamp;
                    }
                    updated_at = updated_at.max(timestamp);
                }
                messages.push(text_msg(role, &text, timestamp));
            }
        }
    }
    assign_seq(&mut messages);
    Ok(GeminiParse {
        session_id,
        messages,
        created_at,
        updated_at,
        unknown_lines: unknown,
    })
}

fn build_meta(reference: &SessionFileRef, parsed: &GeminiParse, cwd: &str) -> SessionMeta {
    let native_id = parsed
        .session_id
        .clone()
        .unwrap_or_else(|| reference.native_id.clone());
    SessionMeta {
        key: format!("gemini:{native_id}"),
        id: native_id,
        agent: AgentId::Gemini,
        title: title_from_messages(&parsed.messages).unwrap_or_else(|| UNTITLED.to_string()),
        project_path: cwd.to_string(),
        project_name: project_name_of(cwd),
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

fn slug_of(path: &Path) -> Option<String> {
    path.ancestors()
        .nth(2)
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().to_string())
}

impl AgentHistoryAdapter for GeminiAdapter {
    fn agent(&self) -> AgentId {
        AgentId::Gemini
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        let mut references = Vec::new();
        // Missing root = not installed/deleted: a legitimate empty set. Root
        // exists but read fails = propagate the error.
        if !self.root.is_dir() {
            return Ok(references);
        }
        let slugs = fs::read_dir(&self.root)
            .map_err(|error| anyhow::anyhow!("read {}: {error}", self.root.display()))?;
        for slug in slugs.flatten() {
            let chats = slug.path().join("chats");
            let Ok(entries) = fs::read_dir(chats) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with("session-") || !name.ends_with(".jsonl") {
                    continue;
                }
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                if !meta.is_file() || meta.len() == 0 {
                    continue;
                }
                references.push(SessionFileRef {
                    agent: AgentId::Gemini,
                    native_id: name.trim_end_matches(".jsonl").to_string(),
                    file_path: entry.path().to_string_lossy().to_string(),
                    mtime_ms: mtime_ms(&meta),
                    size: meta.len() as i64,
                });
            }
        }
        Ok(references)
    }

    fn file_ref(&self, path: &Path) -> Option<SessionFileRef> {
        let name = path.file_name()?.to_string_lossy();
        if !name.starts_with("session-") || !path.to_string_lossy().contains("/chats/") {
            return None;
        }
        default_file_ref(self.agent(), path)
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession> {
        let parsed = parse_jsonl(Path::new(&reference.file_path))?;
        Ok(ParsedSession {
            meta: build_meta(reference, &parsed, &self.cwd_for(reference)),
            units: units_from_messages(&parsed.messages),
            unknown_line_count: parsed.unknown_lines,
        })
    }

    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript> {
        let parsed = parse_jsonl(Path::new(&reference.file_path))?;
        Ok(ParsedTranscript::simple(
            build_meta(reference, &parsed, &self.cwd_for(reference)),
            parsed.messages,
            parsed.unknown_lines,
        ))
    }

    fn with_custom_root(&self, dir: PathBuf) -> Box<dyn AgentHistoryAdapter> {
        let (root, projects_json) = if dir.join("tmp").is_dir() {
            (dir.join("tmp"), dir.join("projects.json"))
        } else {
            let projects_json = dir
                .parent()
                .map(|parent| parent.join("projects.json"))
                .unwrap_or_else(|| dir.join("projects.json"));
            (dir, projects_json)
        };
        Box::new(Self {
            root,
            projects_json,
            slug_cache: Mutex::new(None),
        })
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        vec![self.root.clone()]
    }
}
