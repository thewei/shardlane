// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

use super::parse_utils::*;
use super::sqlite_ro::{open_sqlite_ro, virtual_path};
use super::{units_from_messages, AgentHistoryAdapter};
use crate::models::*;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::PathBuf;

/// Antigravity CLI (Google, binary `agy`): session bodies are encrypted .pb;
/// the only plaintext is `~/.gemini/antigravity-cli/conversation_summaries.db`
/// (WAL) — only metadata-level session cards are possible: title lives in the
/// preview column (the title column is mostly empty), plus time and
/// workspace. The detail page is carried by a single System message holding
/// the preview and an "encrypted body" note, which is the only thing FTS can
/// find. There are no per-session files, so SessionFileRef uses virtual
/// paths; opening always goes through the sqlite_ro three-tier ladder.
pub struct AntigravityAdapter {
    db: PathBuf,
    /// The whole table is tiny (metadata rows); cache by db mtime for
    /// repeated calls within one scan round.
    rows_cache: MtimeCache<Vec<AgRow>>,
}

impl AntigravityAdapter {
    pub fn new() -> Self {
        Self {
            db: home_dir().join(".gemini/antigravity-cli/conversation_summaries.db"),
            rows_cache: MtimeCache::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_db(db: PathBuf) -> Self {
        Self {
            db,
            rows_cache: MtimeCache::new(),
        }
    }

    fn rows(&self) -> Option<Vec<AgRow>> {
        let mtime = std::fs::metadata(&self.db)
            .map(|meta| mtime_ms(&meta))
            .unwrap_or(0);
        self.rows_cache.get_or_try_build(mtime, || {
            let read_only = open_sqlite_ro(&self.db, "antigravity")?;
            let mut statement = read_only
                .conn
                .prepare(
                    "SELECT conversation_id, title, preview, step_count, last_modified_time, workspace_uris
                     FROM conversation_summaries
                     WHERE parent_conversation_id = '' AND nesting_depth = 0",
                )
                .ok()?;
            let rows = statement
                .query_map([], |row| {
                    Ok(AgRow {
                        id: row.get(0)?,
                        title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        preview: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        step_count: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                        modified_ms: sqlite_dt_ms(
                            row.get::<_, Option<String>>(4)?
                                .unwrap_or_default()
                                .trim(),
                        ),
                        cwd: first_workspace(
                            &row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                        ),
                    })
                })
                .ok()?
                .collect::<rusqlite::Result<Vec<_>>>()
                .ok()?;
            Some(rows)
        })
    }

    fn build_meta(&self, reference: &SessionFileRef, row: &AgRow) -> SessionMeta {
        let title = Some(clean_title_candidate(&row.title))
            .filter(|title| !title.is_empty())
            .or_else(|| Some(clean_title_candidate(&row.preview)).filter(|title| !title.is_empty()))
            .unwrap_or_else(|| UNTITLED.to_string());
        // The database has only one timestamp, last_modified; created/updated
        // share its source.
        let timestamp = if row.modified_ms > 0 {
            row.modified_ms
        } else {
            reference.mtime_ms
        };
        SessionMeta {
            key: format!("antigravity:{}", row.id),
            id: row.id.clone(),
            agent: AgentId::Antigravity,
            title,
            project_path: row.cwd.clone(),
            project_name: project_name_of(&row.cwd),
            file_path: reference.file_path.clone(),
            created_at: timestamp,
            updated_at: timestamp,
            message_count: row.step_count,
            size_bytes: reference.size,
            git_branch: None,
            model: None,
            tokens_used: None,
            archived: false,
            source: None,
        }
    }

    fn parse(&self, reference: &SessionFileRef) -> Result<(SessionMeta, Vec<TranscriptMessage>)> {
        let rows = self
            .rows()
            .ok_or_else(|| anyhow!("cannot open antigravity summaries store"))?;
        let row = rows
            .iter()
            .find(|row| row.id == reference.native_id)
            .ok_or_else(|| {
                anyhow!(
                    "antigravity conversation {} not in store",
                    reference.native_id
                )
            })?;

        // Encrypted body is unreadable: one System message carries the
        // preview, giving both the detail page and FTS something to show.
        let mut text = String::new();
        if !row.preview.trim().is_empty() {
            text.push_str(row.preview.trim());
            text.push_str("\n\n");
        }
        text.push_str(
            "Antigravity stores conversation content encrypted — only this summary is available in Shardlane.",
        );
        let mut messages = vec![text_msg(Role::System, &text, row.modified_ms)];
        assign_seq(&mut messages);
        Ok((self.build_meta(reference, row), messages))
    }
}

impl Default for AntigravityAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
struct AgRow {
    id: String,
    title: String,
    preview: String,
    step_count: i64,
    modified_ms: i64,
    cwd: String,
}

/// First item of a workspace_uris JSON array ("[\"file:///Users/…\"]") →
/// local path.
fn first_workspace(raw: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return String::new();
    };
    let Some(uri) = value
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.as_str())
    else {
        return String::new();
    };
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    percent_decode(path)
}

/// Minimal percent-decode for file:// URIs (paths with spaces/CJK characters
/// are %XX encoded).
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

impl AgentHistoryAdapter for AntigravityAdapter {
    fn agent(&self) -> AgentId {
        AgentId::Antigravity
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        let Some(rows) = self.rows() else {
            return Ok(Vec::new());
        };
        Ok(rows
            .into_iter()
            .map(|row| SessionFileRef {
                agent: AgentId::Antigravity,
                native_id: row.id.clone(),
                file_path: virtual_path(&self.db, &row.id),
                mtime_ms: row.modified_ms,
                // Body unreadable, so the title/preview length is the content
                // fingerprint (used for dirty detection).
                size: (row.title.len() + row.preview.len()) as i64,
            })
            .collect())
    }

    fn quick_meta(&self, references: &[SessionFileRef]) -> Option<HashMap<String, SessionMeta>> {
        let rows = self.rows()?;
        Some(resolve_rows_by_id(
            &rows,
            references,
            |row| row.id.as_str(),
            |reference, row| self.build_meta(reference, row),
        ))
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession> {
        let (meta, messages) = self.parse(reference)?;
        let units = units_from_messages(&messages);
        Ok(ParsedSession {
            meta,
            units,
            unknown_line_count: 0,
        })
    }

    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript> {
        let (meta, messages) = self.parse(reference)?;
        Ok(ParsedTranscript::simple(meta, messages, 0))
    }

    fn with_custom_root(&self, dir: PathBuf) -> Box<dyn AgentHistoryAdapter> {
        let nested = dir
            .join("antigravity-cli")
            .join("conversation_summaries.db");
        let db = if dir.is_file() {
            dir
        } else if nested.is_file() {
            nested
        } else {
            dir.join("conversation_summaries.db")
        };
        Box::new(Self {
            db,
            rows_cache: MtimeCache::new(),
        })
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        vec![self.db.clone()]
    }
}
