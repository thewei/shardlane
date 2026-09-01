// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

use super::parse_utils::*;
use super::sqlite_ro::{open_sqlite_ro, virtual_path};
use super::{units_from_messages, AgentHistoryAdapter};
use crate::models::*;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct CopilotAdapter {
    db: PathBuf,
    rows_cache: Mutex<Option<(i64, Vec<CopilotRow>)>>,
}

impl CopilotAdapter {
    pub fn new() -> Self {
        Self {
            db: home_dir().join(".copilot/session-store.db"),
            rows_cache: Mutex::new(None),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_db(db: PathBuf) -> Self {
        Self {
            db,
            rows_cache: Mutex::new(None),
        }
    }

    fn db_mtime(&self) -> i64 {
        std::fs::metadata(&self.db)
            .map(|meta| mtime_ms(&meta))
            .unwrap_or(0)
    }

    fn rows(&self) -> Option<Vec<CopilotRow>> {
        let mtime = self.db_mtime();
        if let Ok(cache) = self.rows_cache.lock() {
            if let Some((cached_mtime, rows)) = cache.as_ref() {
                if *cached_mtime == mtime {
                    return Some(rows.clone());
                }
            }
        }
        let database = open_sqlite_ro(&self.db, "copilot")?;
        let mut statement = database
            .conn
            .prepare(
                "SELECT s.id, s.cwd, s.branch, s.summary, s.created_at, s.updated_at,
                        COALESCE(SUM(LENGTH(COALESCE(t.user_message,'')) + LENGTH(COALESCE(t.assistant_response,''))), 0),
                        COUNT(t.id)
                 FROM sessions s LEFT JOIN turns t ON t.session_id = s.id
                 GROUP BY s.id",
            )
            .ok()?;
        let rows = statement
            .query_map([], |row| {
                Ok(CopilotRow {
                    id: row.get(0)?,
                    cwd: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    branch: row.get(2)?,
                    summary: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    created_ms: sqlite_dt_ms(&row.get::<_, Option<String>>(4)?.unwrap_or_default()),
                    updated_ms: sqlite_dt_ms(&row.get::<_, Option<String>>(5)?.unwrap_or_default()),
                    content_len: row.get(6)?,
                    turn_count: row.get(7)?,
                })
            })
            .ok()?
            .collect::<rusqlite::Result<Vec<_>>>()
            .ok()?;
        if let Ok(mut cache) = self.rows_cache.lock() {
            *cache = Some((mtime, rows.clone()));
        }
        Some(rows)
    }

    fn build_meta(
        &self,
        reference: &SessionFileRef,
        row: &CopilotRow,
        message_count: i64,
    ) -> SessionMeta {
        let title = clean_title_candidate(&row.summary);
        SessionMeta {
            key: format!("copilot:{}", row.id),
            id: row.id.clone(),
            agent: AgentId::Copilot,
            title: if title.is_empty() {
                UNTITLED.to_string()
            } else {
                title
            },
            project_path: row.cwd.clone(),
            project_name: project_name_of(&row.cwd),
            file_path: reference.file_path.clone(),
            created_at: if row.created_ms > 0 {
                row.created_ms
            } else {
                reference.mtime_ms
            },
            updated_at: if row.updated_ms > 0 {
                row.updated_ms
            } else {
                reference.mtime_ms
            },
            message_count,
            size_bytes: reference.size,
            git_branch: row.branch.clone().filter(|branch| !branch.is_empty()),
            model: None,
            tokens_used: None,
            archived: false,
            source: None,
        }
    }

    fn parse(&self, reference: &SessionFileRef) -> Result<(SessionMeta, Vec<TranscriptMessage>)> {
        let database = open_sqlite_ro(&self.db, "copilot")
            .ok_or_else(|| anyhow!("cannot open copilot store"))?;
        let row = database
            .conn
            .query_row(
                "SELECT s.id, s.cwd, s.branch, s.summary, s.created_at, s.updated_at,
                        COALESCE(SUM(LENGTH(COALESCE(t.user_message,'')) + LENGTH(COALESCE(t.assistant_response,''))), 0),
                        COUNT(t.id)
                 FROM sessions s LEFT JOIN turns t ON t.session_id = s.id
                 WHERE s.id = ?1 GROUP BY s.id",
                [&reference.native_id],
                |row| {
                    Ok(CopilotRow {
                        id: row.get(0)?,
                        cwd: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        branch: row.get(2)?,
                        summary: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                        created_ms: sqlite_dt_ms(
                            &row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                        ),
                        updated_ms: sqlite_dt_ms(
                            &row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                        ),
                        content_len: row.get(6)?,
                        turn_count: row.get(7)?,
                    })
                },
            )
            .map_err(|_| anyhow!("copilot session {} not in store", reference.native_id))?;
        let mut statement = database.conn.prepare(
            "SELECT user_message, assistant_response, timestamp
             FROM turns WHERE session_id = ?1 ORDER BY turn_index",
        )?;
        let turns = statement.query_map([&reference.native_id], |turn| {
            Ok((
                turn.get::<_, Option<String>>(0)?,
                turn.get::<_, Option<String>>(1)?,
                turn.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut messages = Vec::new();
        for turn in turns.flatten() {
            let timestamp = sqlite_dt_ms(&turn.2.unwrap_or_default());
            if let Some(user) = turn.0.filter(|text| !text.trim().is_empty()) {
                messages.push(text_msg(Role::User, &user, timestamp));
            }
            if let Some(assistant) = turn.1.filter(|text| !text.trim().is_empty()) {
                messages.push(text_msg(Role::Assistant, &assistant, timestamp));
            }
        }
        assign_seq(&mut messages);
        let count = messages
            .iter()
            .filter(|message| message.kind == MessageKind::Text)
            .count() as i64;
        let mut meta = self.build_meta(reference, &row, count);
        if meta.title == UNTITLED {
            if let Some(title) = title_from_messages(&messages) {
                meta.title = title;
            }
        }
        Ok((meta, messages))
    }
}

impl Default for CopilotAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
struct CopilotRow {
    id: String,
    cwd: String,
    branch: Option<String>,
    summary: String,
    created_ms: i64,
    updated_ms: i64,
    content_len: i64,
    turn_count: i64,
}

impl AgentHistoryAdapter for CopilotAdapter {
    fn agent(&self) -> AgentId {
        AgentId::Copilot
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        // detect() already confirmed the db file exists; unreadable = a
        // transient failure such as WAL write state or a torn copy, so it
        // must be propagated rather than faked as an empty set (an empty set
        // would trigger catalog cleanup).
        let rows = self
            .rows()
            .ok_or_else(|| anyhow!("session store unreadable: {}", self.db.display()))?;
        Ok(rows
            .into_iter()
            .filter(|row| row.turn_count > 0)
            .map(|row| SessionFileRef {
                agent: AgentId::Copilot,
                native_id: row.id.clone(),
                file_path: virtual_path(&self.db, &row.id),
                mtime_ms: row.updated_ms,
                size: row.content_len,
            })
            .collect())
    }

    fn quick_meta(&self, references: &[SessionFileRef]) -> Option<HashMap<String, SessionMeta>> {
        let rows = self.rows()?;
        Some(resolve_rows_by_id(
            &rows,
            references,
            |row| row.id.as_str(),
            |reference, row| self.build_meta(reference, row, 0),
        ))
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession> {
        let (meta, messages) = self.parse(reference)?;
        Ok(ParsedSession {
            meta,
            units: units_from_messages(&messages),
            unknown_line_count: 0,
        })
    }

    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript> {
        let (meta, messages) = self.parse(reference)?;
        Ok(ParsedTranscript::simple(meta, messages, 0))
    }

    fn with_custom_root(&self, dir: PathBuf) -> Box<dyn AgentHistoryAdapter> {
        let db = if dir.is_file() {
            dir
        } else {
            dir.join("session-store.db")
        };
        Box::new(Self {
            db,
            rows_cache: Mutex::new(None),
        })
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        vec![self.db.clone()]
    }
}
