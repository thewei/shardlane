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

pub struct KiroAdapter {
    root: PathBuf,
}

impl KiroAdapter {
    pub fn new() -> Self {
        Self {
            root: home_dir().join(".kiro/sessions/cli"),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_root(root: PathBuf) -> Self {
        Self { root }
    }
}

impl Default for KiroAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
struct Sidecar {
    cwd: String,
    title: String,
    created_ms: i64,
    updated_ms: i64,
}

fn read_sidecar(jsonl_path: &Path) -> Sidecar {
    let mut side = Sidecar::default();
    let Ok(raw) = fs::read_to_string(jsonl_path.with_extension("json")) else {
        return side;
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return side;
    };
    side.cwd = value
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    side.title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    side.created_ms = value
        .get("created_at")
        .and_then(Value::as_str)
        .map(iso_ms)
        .unwrap_or(0);
    side.updated_ms = value
        .get("updated_at")
        .and_then(Value::as_str)
        .map(iso_ms)
        .unwrap_or(0);
    side
}

fn parse_jsonl(path: &Path) -> Result<(Vec<TranscriptMessage>, u32)> {
    let file = fs::File::open(path)?;
    let reader = BufReader::with_capacity(1 << 20, file);
    let mut messages = Vec::new();
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
        let role = match row.get("kind").and_then(Value::as_str) {
            Some("Prompt") => Role::User,
            Some("AssistantMessage") => Role::Assistant,
            _ => {
                unknown += 1;
                continue;
            }
        };
        let Some(data) = row.get("data") else {
            unknown += 1;
            continue;
        };
        let text = data
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|block| block.get("kind").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("data").and_then(Value::as_str))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        if text.is_empty() {
            continue;
        }
        let timestamp = data
            .get("meta")
            .and_then(|meta| meta.get("timestamp"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
            * 1000;
        messages.push(text_msg(role, &text, timestamp));
    }
    assign_seq(&mut messages);
    Ok((messages, unknown))
}

fn build_meta(
    reference: &SessionFileRef,
    side: &Sidecar,
    messages: &[TranscriptMessage],
) -> SessionMeta {
    let title = Some(clean_title_candidate(&side.title))
        .filter(|title| !title.is_empty())
        .or_else(|| title_from_messages(messages))
        .unwrap_or_else(|| UNTITLED.to_string());
    let latest_message = messages
        .iter()
        .filter_map(|message| message.timestamp)
        .max()
        .unwrap_or(0);
    SessionMeta {
        key: format!("kiro:{}", reference.native_id),
        id: reference.native_id.clone(),
        agent: AgentId::Kiro,
        title,
        project_path: side.cwd.clone(),
        project_name: project_name_of(&side.cwd),
        file_path: reference.file_path.clone(),
        created_at: if side.created_ms > 0 {
            side.created_ms
        } else {
            reference.mtime_ms
        },
        updated_at: side.updated_ms.max(latest_message).max(reference.mtime_ms),
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

impl AgentHistoryAdapter for KiroAdapter {
    fn agent(&self) -> AgentId {
        AgentId::Kiro
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        list_jsonl_refs(&self.root, AgentId::Kiro, str::to_string)
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession> {
        let (messages, unknown) = parse_jsonl(Path::new(&reference.file_path))?;
        let side = read_sidecar(Path::new(&reference.file_path));
        Ok(ParsedSession {
            meta: build_meta(reference, &side, &messages),
            units: units_from_messages(&messages),
            unknown_line_count: unknown,
        })
    }

    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript> {
        let (messages, unknown) = parse_jsonl(Path::new(&reference.file_path))?;
        let side = read_sidecar(Path::new(&reference.file_path));
        Ok(ParsedTranscript::simple(
            build_meta(reference, &side, &messages),
            messages,
            unknown,
        ))
    }

    fn with_custom_root(&self, dir: PathBuf) -> Box<dyn AgentHistoryAdapter> {
        let root = if dir.join("sessions").join("cli").is_dir() {
            dir.join("sessions").join("cli")
        } else if dir.join("cli").is_dir() {
            dir.join("cli")
        } else {
            dir
        };
        Box::new(Self { root })
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        vec![self.root.clone()]
    }
}
