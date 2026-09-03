// SPDX-License-Identifier: MIT
// Catalog shape and FTS strategy retain upstream MIT-derived semantics.

use crate::models::{
    normalize_path_key, AgentId, IndexUnit, ParsedTranscript, Role, SearchHit, SessionFileRef,
    SessionMeta, SessionSummary, SidechainInfo, TranscriptMessage,
};
use anyhow::Result;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

const TRANSCRIPT_CACHE_PAGE_SIZE: usize = 64;
pub(crate) const TRANSCRIPT_CACHE_MAX_SESSIONS: usize = 256;
pub(crate) const TRANSCRIPT_CACHE_MAX_LOGICAL_BYTES: i64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedTranscriptWindow {
    pub meta: SessionMeta,
    pub total_messages: usize,
    pub start: usize,
    pub messages: Vec<TranscriptMessage>,
    pub sidechains: Vec<SidechainInfo>,
    pub unknown_line_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CachedTranscriptMetaPayload {
    meta: SessionMeta,
    sidechains: Vec<SidechainInfo>,
    unknown_line_count: u32,
}

pub struct HistoryCatalog {
    conn: Connection,
}

impl HistoryCatalog {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_inner(path).map(|(catalog, _)| catalog)
    }

    /// [`Self::open`] plus the number of migration steps this open applied;
    /// test seam proving a current-version database reopens without running
    /// any migration write.
    #[cfg(test)]
    pub(crate) fn open_with_migration_count(path: &Path) -> Result<(Self, usize)> {
        Self::open_inner(path)
    }

    fn open_inner(path: &Path) -> Result<(Self, usize)> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(2))?;
        // WAL: scan-connection writes no longer block the GUI's concurrent
        // search/paging read connections with an exclusive lock, avoiding
        // global search silently returning zero results during a scan (the
        // read timeout gets swallowed upstream as an empty set).
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let applied = run_migrations(&mut conn)?;
        Ok((Self { conn }, applied))
    }

    pub fn open_initialized(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.busy_timeout(Duration::from_secs(2))?;
        // Foreign-key enforcement is per-connection and defaults to OFF:
        // write methods are callable on this connection type, and without
        // this pragma, deleting sessions rows here would leave page-cache
        // orphan rows that never get evicted.
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(Self { conn })
    }

    #[cfg(test)]
    pub(crate) fn memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.busy_timeout(Duration::from_secs(2))?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE sessions (
                key TEXT PRIMARY KEY,
                native_id TEXT NOT NULL,
                agent TEXT NOT NULL,
                title TEXT NOT NULL,
                project_path TEXT NOT NULL,
                project_key TEXT NOT NULL DEFAULT '',
                project_name TEXT NOT NULL,
                file_path TEXT NOT NULL UNIQUE,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                message_count INTEGER NOT NULL,
                size_bytes INTEGER NOT NULL,
                git_branch TEXT,
                model TEXT,
                tokens_used INTEGER,
                archived INTEGER NOT NULL DEFAULT 0,
                source TEXT,
                mtime_ms INTEGER NOT NULL,
                description TEXT NOT NULL DEFAULT ''
             );
             CREATE INDEX sessions_updated_at ON sessions(updated_at DESC);
             CREATE INDEX sessions_agent ON sessions(agent);
             CREATE INDEX sessions_agent_native
                ON sessions(agent, native_id, updated_at DESC);
             CREATE INDEX sessions_project_key ON sessions(project_key, updated_at DESC);
             CREATE TABLE transcript_page_meta (
                session_key TEXT PRIMARY KEY REFERENCES sessions(key) ON DELETE CASCADE,
                agent TEXT NOT NULL,
                native_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                mtime_ms INTEGER NOT NULL,
                size_bytes INTEGER NOT NULL,
                message_count INTEGER NOT NULL,
                payload BLOB NOT NULL
             );
             CREATE TABLE transcript_page_cache (
                session_key TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
                page_index INTEGER NOT NULL,
                payload BLOB NOT NULL,
                PRIMARY KEY(session_key, page_index)
             );
             CREATE TABLE transcript_message_index (
                session_key TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
                seq INTEGER NOT NULL,
                message_index INTEGER NOT NULL,
                PRIMARY KEY(session_key, seq)
             );
             CREATE VIRTUAL TABLE message_fts USING fts5(
                session_key UNINDEXED,
                seq UNINDEXED,
                role UNINDEXED,
                timestamp UNINDEXED,
                text,
                tokenize='trigram'
             );",
        )?;
        Ok(Self { conn })
    }

    pub fn known_files(&self) -> Result<HashMap<String, i64>> {
        let mut statement = self
            .conn
            .prepare("SELECT file_path, mtime_ms FROM sessions")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn write_session(
        &mut self,
        meta: &SessionMeta,
        mtime_ms: i64,
        units: &[IndexUnit],
    ) -> Result<()> {
        let project_key = normalized_project_key(&meta.project_path).unwrap_or_default();
        let description = session_description_from_units(units);
        let transaction = self.conn.transaction()?;
        transaction.execute(
            "INSERT INTO sessions (
                key, native_id, agent, title, project_path, project_key, project_name, file_path,
                created_at, updated_at, message_count, size_bytes, git_branch, model,
                tokens_used, archived, source, mtime_ms, description
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
             )
             ON CONFLICT(key) DO UPDATE SET
                native_id=excluded.native_id,
                agent=excluded.agent,
                title=excluded.title,
                project_path=excluded.project_path,
                project_key=excluded.project_key,
                project_name=excluded.project_name,
                file_path=excluded.file_path,
                created_at=excluded.created_at,
                updated_at=excluded.updated_at,
                message_count=excluded.message_count,
                size_bytes=excluded.size_bytes,
                git_branch=excluded.git_branch,
                model=excluded.model,
                tokens_used=excluded.tokens_used,
                archived=excluded.archived,
                source=excluded.source,
                mtime_ms=excluded.mtime_ms,
                description=excluded.description",
            params![
                meta.key,
                meta.id,
                meta.agent.as_str(),
                meta.title,
                meta.project_path,
                project_key,
                meta.project_name,
                meta.file_path,
                meta.created_at,
                meta.updated_at,
                meta.message_count,
                meta.size_bytes,
                meta.git_branch,
                meta.model,
                meta.tokens_used,
                i64::from(meta.archived),
                meta.source,
                mtime_ms,
                description,
            ],
        )?;
        transaction.execute(
            "DELETE FROM transcript_page_cache
             WHERE session_key = ?1 AND EXISTS (
                SELECT 1 FROM transcript_page_meta
                WHERE session_key = ?1
                  AND (agent != ?2 OR native_id != ?3 OR file_path != ?4 OR mtime_ms != ?5 OR size_bytes != ?6)
             )",
            params![
                meta.key,
                meta.agent.as_str(),
                meta.id,
                meta.file_path,
                mtime_ms,
                meta.size_bytes,
            ],
        )?;
        transaction.execute(
            "DELETE FROM transcript_message_index
             WHERE session_key = ?1 AND EXISTS (
                SELECT 1 FROM transcript_page_meta
                WHERE session_key = ?1
                  AND (agent != ?2 OR native_id != ?3 OR file_path != ?4 OR mtime_ms != ?5 OR size_bytes != ?6)
             )",
            params![
                meta.key,
                meta.agent.as_str(),
                meta.id,
                meta.file_path,
                mtime_ms,
                meta.size_bytes,
            ],
        )?;
        transaction.execute(
            "DELETE FROM transcript_page_meta
             WHERE session_key = ?1
               AND (agent != ?2 OR native_id != ?3 OR file_path != ?4 OR mtime_ms != ?5 OR size_bytes != ?6)",
            params![
                meta.key,
                meta.agent.as_str(),
                meta.id,
                meta.file_path,
                mtime_ms,
                meta.size_bytes,
            ],
        )?;
        transaction.execute(
            "DELETE FROM message_fts WHERE session_key = ?1",
            params![meta.key],
        )?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO message_fts (session_key, seq, role, timestamp, text)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for unit in units {
                insert.execute(params![
                    meta.key,
                    unit.seq,
                    unit.role.as_str(),
                    unit.timestamp,
                    unit.text,
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn remove_missing(&mut self, seen_paths: &HashSet<String>) -> Result<usize> {
        let current = self
            .known_files()?
            .into_keys()
            .filter(|path| !seen_paths.contains(path))
            .collect::<Vec<_>>();
        if current.is_empty() {
            return Ok(0);
        }
        let transaction = self.conn.transaction()?;
        for path in &current {
            let key: Option<String> = transaction
                .query_row(
                    "SELECT key FROM sessions WHERE file_path = ?1",
                    params![path],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(key) = key {
                transaction.execute(
                    "DELETE FROM message_fts WHERE session_key = ?1",
                    params![key],
                )?;
            }
            transaction.execute("DELETE FROM sessions WHERE file_path = ?1", params![path])?;
        }
        transaction.commit()?;
        Ok(current.len())
    }

    pub fn delete_session(&mut self, key: &str) -> Result<Option<String>> {
        let transaction = self.conn.transaction()?;
        let file_path: Option<String> = transaction
            .query_row(
                "SELECT file_path FROM sessions WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        transaction.execute(
            "DELETE FROM message_fts WHERE session_key = ?1",
            params![key],
        )?;
        transaction.execute(
            "DELETE FROM transcript_page_cache WHERE session_key = ?1",
            params![key],
        )?;
        transaction.execute(
            "DELETE FROM transcript_message_index WHERE session_key = ?1",
            params![key],
        )?;
        transaction.execute(
            "DELETE FROM transcript_page_meta WHERE session_key = ?1",
            params![key],
        )?;
        transaction.execute("DELETE FROM sessions WHERE key = ?1", params![key])?;
        transaction.commit()?;
        Ok(file_path)
    }

    pub fn transcript_source(&self, key: &str) -> Result<Option<SessionFileRef>> {
        self.conn
            .query_row(
                "SELECT agent, native_id, file_path, mtime_ms, size_bytes
                 FROM sessions WHERE key = ?1",
                params![key],
                |row| {
                    let agent_text: String = row.get(0)?;
                    let agent = AgentId::from_slug(&agent_text).ok_or_else(|| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("unknown history agent {agent_text}"),
                            )),
                        )
                    })?;
                    Ok(SessionFileRef {
                        agent,
                        native_id: row.get(1)?,
                        file_path: row.get(2)?,
                        mtime_ms: row.get(3)?,
                        size: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Look up the source file by (agent, exact native session id). Used as
    /// the sole authoritative path for live Chat binding; returns None when
    /// not found and never falls back to cwd/mtime heuristics.
    pub fn session_source_by_native(
        &self,
        agent: AgentId,
        native_id: &str,
    ) -> Result<Option<SessionFileRef>> {
        self.conn
            .query_row(
                "SELECT agent, native_id, file_path, mtime_ms, size_bytes
                 FROM sessions WHERE agent = ?1 AND native_id = ?2
                 ORDER BY updated_at DESC LIMIT 1",
                params![agent.as_str(), native_id],
                |row| {
                    let agent_text: String = row.get(0)?;
                    let agent = AgentId::from_slug(&agent_text).ok_or_else(|| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("unknown history agent {agent_text}"),
                            )),
                        )
                    })?;
                    Ok(SessionFileRef {
                        agent,
                        native_id: row.get(1)?,
                        file_path: row.get(2)?,
                        mtime_ms: row.get(3)?,
                        size: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn cache_transcript(
        &self,
        session_key: &str,
        source: &SessionFileRef,
        transcript: &ParsedTranscript,
    ) -> Result<()> {
        write_transcript_pages(&self.conn, session_key, source, transcript)
    }

    pub fn cached_transcript_window(
        &self,
        session_key: &str,
        source: &SessionFileRef,
        start: usize,
        limit: usize,
    ) -> Result<Option<CachedTranscriptWindow>> {
        let cached: Option<(i64, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT message_count, payload FROM transcript_page_meta
                 WHERE session_key = ?1 AND agent = ?2 AND native_id = ?3
                   AND file_path = ?4 AND mtime_ms = ?5 AND size_bytes = ?6",
                params![
                    session_key,
                    source.agent.as_str(),
                    source.native_id,
                    source.file_path,
                    source.mtime_ms,
                    source.size,
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((message_count, meta_payload)) = cached else {
            return Ok(None);
        };
        let meta: CachedTranscriptMetaPayload = match serde_json::from_slice(&meta_payload) {
            Ok(meta) => meta,
            Err(_) => {
                self.clear_transcript_page_cache(session_key)?;
                return Ok(None);
            }
        };
        let total_messages = usize::try_from(message_count.max(0)).unwrap_or(usize::MAX);
        let start = start.min(total_messages);
        if limit == 0 || start == total_messages {
            return Ok(Some(CachedTranscriptWindow {
                meta: meta.meta,
                total_messages,
                start,
                messages: Vec::new(),
                sidechains: meta.sidechains,
                unknown_line_count: meta.unknown_line_count,
            }));
        }
        let end = start.saturating_add(limit).min(total_messages);
        let first_page = start / TRANSCRIPT_CACHE_PAGE_SIZE;
        let last_page = end.saturating_sub(1) / TRANSCRIPT_CACHE_PAGE_SIZE;
        let mut statement = self.conn.prepare(
            "SELECT page_index, payload FROM transcript_page_cache
             WHERE session_key = ?1 AND page_index BETWEEN ?2 AND ?3
             ORDER BY page_index ASC",
        )?;
        let pages = statement.query_map(
            params![session_key, first_page as i64, last_page as i64],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )?;
        let mut messages = Vec::with_capacity(end - start);
        for page in pages {
            let (page_index, payload) = page?;
            let page_index = usize::try_from(page_index.max(0)).unwrap_or(usize::MAX);
            let decoded: Vec<TranscriptMessage> = match serde_json::from_slice(&payload) {
                Ok(decoded) => decoded,
                Err(_) => {
                    self.clear_transcript_page_cache(session_key)?;
                    return Ok(None);
                }
            };
            let page_start = page_index.saturating_mul(TRANSCRIPT_CACHE_PAGE_SIZE);
            for (offset, message) in decoded.into_iter().enumerate() {
                let index = page_start.saturating_add(offset);
                if index >= start && index < end {
                    messages.push(message);
                }
            }
        }
        if messages.len() != end - start {
            self.clear_transcript_page_cache(session_key)?;
            return Ok(None);
        }
        Ok(Some(CachedTranscriptWindow {
            meta: meta.meta,
            total_messages,
            start,
            messages,
            sidechains: meta.sidechains,
            unknown_line_count: meta.unknown_line_count,
        }))
    }

    pub fn cached_transcript_index_for_seq(
        &self,
        session_key: &str,
        source: &SessionFileRef,
        seq: i64,
    ) -> Result<Option<usize>> {
        let index = self
            .conn
            .query_row(
                "SELECT i.message_index
                 FROM transcript_message_index i
                 JOIN transcript_page_meta m ON m.session_key = i.session_key
                 WHERE i.session_key = ?1 AND i.seq = ?2
                   AND m.agent = ?3 AND m.native_id = ?4 AND m.file_path = ?5
                   AND m.mtime_ms = ?6 AND m.size_bytes = ?7",
                params![
                    session_key,
                    seq,
                    source.agent.as_str(),
                    source.native_id,
                    source.file_path,
                    source.mtime_ms,
                    source.size,
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        Ok(index.map(|index| usize::try_from(index.max(0)).unwrap_or(usize::MAX)))
    }

    fn clear_transcript_page_cache(&self, session_key: &str) -> Result<()> {
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM transcript_page_cache WHERE session_key = ?1",
            params![session_key],
        )?;
        transaction.execute(
            "DELETE FROM transcript_message_index WHERE session_key = ?1",
            params![session_key],
        )?;
        transaction.execute(
            "DELETE FROM transcript_page_meta WHERE session_key = ?1",
            params![session_key],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn prune_transcript_page_cache(&self) -> Result<usize> {
        self.prune_transcript_page_cache_to(
            TRANSCRIPT_CACHE_MAX_SESSIONS,
            TRANSCRIPT_CACHE_MAX_LOGICAL_BYTES,
        )
    }

    fn prune_transcript_page_cache_to(
        &self,
        max_sessions: usize,
        max_logical_bytes: i64,
    ) -> Result<usize> {
        if max_sessions == 0 || max_logical_bytes <= 0 {
            let keys = self
                .conn
                .prepare("SELECT session_key FROM transcript_page_meta")?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for key in &keys {
                self.clear_transcript_page_cache(key)?;
            }
            return Ok(keys.len());
        }

        // transcript_page_meta.session_key is its PRIMARY KEY, so grouping by
        // session_key alone is exact; LENGTH(p.payload) (not the raw BLOB)
        // keeps SQLite from comparing whole payloads per group.
        let mut statement = self.conn.prepare(
            "SELECT p.session_key,
                    s.updated_at,
                    LENGTH(p.payload)
                      + COALESCE(SUM(LENGTH(c.payload)), 0)
                      + (p.message_count * 24) AS logical_bytes
             FROM transcript_page_meta p
             JOIN sessions s ON s.key = p.session_key
             LEFT JOIN transcript_page_cache c ON c.session_key = p.session_key
             GROUP BY p.session_key, s.updated_at, LENGTH(p.payload), p.message_count
             ORDER BY s.updated_at DESC, p.session_key ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(2)?.max(0)))
        })?;

        let mut retained_sessions = 0usize;
        let mut retained_bytes = 0i64;
        let mut evict = Vec::new();
        for row in rows {
            let (session_key, logical_bytes) = row?;
            let next_bytes = retained_bytes.saturating_add(logical_bytes);
            if retained_sessions < max_sessions && next_bytes <= max_logical_bytes {
                retained_sessions += 1;
                retained_bytes = next_bytes;
            } else {
                evict.push(session_key);
            }
        }
        drop(statement);

        for session_key in &evict {
            self.clear_transcript_page_cache(session_key)?;
        }
        Ok(evict.len())
    }

    pub fn uncached_transcript_sources(
        &self,
        limit: usize,
        max_total_bytes: i64,
    ) -> Result<Vec<(SessionMeta, SessionFileRef)>> {
        if limit == 0 || max_total_bytes <= 0 {
            return Ok(Vec::new());
        }
        let candidate_limit = limit.saturating_mul(16).max(limit) as i64;
        let sql = format!(
            "SELECT {}, s.mtime_ms
             FROM sessions s
             LEFT JOIN transcript_page_meta p
               ON p.session_key = s.key
              AND p.agent = s.agent
              AND p.native_id = s.native_id
              AND p.file_path = s.file_path
              AND p.mtime_ms = s.mtime_ms
              AND p.size_bytes = s.size_bytes
             WHERE s.archived = 0 AND p.session_key IS NULL
             ORDER BY s.updated_at DESC
             LIMIT ?1",
            aliased_session_columns("s")
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params![candidate_limit], |row| {
            let meta = row_to_meta(row)?;
            let mtime_ms = row.get(16)?;
            let reference = SessionFileRef {
                agent: meta.agent,
                native_id: meta.id.clone(),
                file_path: meta.file_path.clone(),
                mtime_ms,
                size: meta.size_bytes,
            };
            Ok((meta, reference))
        })?;
        let mut selected = Vec::with_capacity(limit);
        let mut selected_bytes = 0i64;
        for row in rows {
            let (meta, reference) = row?;
            let size = reference.size.max(0);
            if size > max_total_bytes || selected_bytes.saturating_add(size) > max_total_bytes {
                continue;
            }
            selected_bytes = selected_bytes.saturating_add(size);
            selected.push((meta, reference));
            if selected.len() >= limit {
                break;
            }
        }
        Ok(selected)
    }

    /// Same listing as [`Self::list_session_summaries`] without the
    /// Description preview.
    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionMeta>> {
        Ok(self
            .list_session_summaries(limit)?
            .into_iter()
            .map(|summary| summary.meta)
            .collect())
    }

    /// List-page data: SessionMeta + Description preview (one query fetches
    /// everything, avoiding per-row lookups).
    pub fn list_session_summaries(&self, limit: usize) -> Result<Vec<SessionSummary>> {
        let sql = format!(
            "SELECT {SESSION_COLUMNS}, description
             FROM sessions
             WHERE archived = 0
             ORDER BY updated_at DESC
             LIMIT ?1"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(params![limit as i64], |row| {
            Ok(SessionSummary {
                meta: row_to_meta(row)?,
                description: row.get(16)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Backfill Description for legacy rows: excerpted from the first FTS
    /// messages. Loops internally until no rows are missing or the
    /// DESCRIPTION_BACKFILL_BUDGET time budget is exhausted (called from a
    /// background thread), ensuring the full backfill completes within the
    /// first few scan rounds after an upgrade instead of stalling on empty
    /// descriptions indefinitely.
    pub fn backfill_session_descriptions(&mut self) -> Result<usize> {
        let started = std::time::Instant::now();
        let mut updated = 0usize;
        loop {
            let pending_keys = {
                let mut statement = self
                    .conn
                    .prepare("SELECT key FROM sessions WHERE description = '' LIMIT 500")?;
                let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            if pending_keys.is_empty() {
                return Ok(updated);
            }
            let transaction = self.conn.transaction()?;
            for key in &pending_keys {
                let mut statement = transaction.prepare(
                    "SELECT role, text FROM message_fts WHERE session_key = ?1 ORDER BY seq LIMIT 4",
                )?;
                let units = statement
                    .query_map(params![key], |row| {
                        let role_text: String = row.get(0)?;
                        Ok(IndexUnit {
                            seq: 0,
                            sidechain_id: None,
                            role: match role_text.as_str() {
                                "assistant" => Role::Assistant,
                                "system" => Role::System,
                                _ => Role::User,
                            },
                            timestamp: None,
                            text: row.get(1)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                drop(statement);
                let description = session_description_from_units(&units);
                if description.is_empty() {
                    continue;
                }
                transaction.execute(
                    "UPDATE sessions SET description = ?1 WHERE key = ?2 AND description = ''",
                    params![description, key],
                )?;
                updated += 1;
            }
            transaction.commit()?;
            if started.elapsed() >= DESCRIPTION_BACKFILL_BUDGET {
                return Ok(updated);
            }
        }
    }

    /// Unscoped metadata search: identical policy and ordering as
    /// [`Self::search_session_metadata_scoped`] with no project/agent filter.
    pub fn search_session_metadata(&self, query: &str, limit: usize) -> Result<Vec<SessionMeta>> {
        self.search_session_metadata_scoped(query, &[], &[], limit)
    }

    pub fn search_session_metadata_scoped(
        &self,
        query: &str,
        project_paths: &[String],
        agents: &[AgentId],
        limit: usize,
    ) -> Result<Vec<SessionMeta>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let terms = search_terms(query);
        let project_keys = normalized_project_keys(project_paths);
        if !project_paths.is_empty() && project_keys.is_empty() {
            return Ok(Vec::new());
        }

        let mut conditions = vec!["archived = 0".to_string()];
        let mut values = Vec::new();
        if !terms.is_empty() {
            let condition = "(title LIKE ? ESCAPE '\\' OR project_path LIKE ? ESCAPE '\\' OR project_name LIKE ? ESCAPE '\\' OR native_id LIKE ? ESCAPE '\\' OR file_path LIKE ? ESCAPE '\\' OR agent LIKE ? ESCAPE '\\')";
            conditions.push(
                std::iter::repeat_n(condition, terms.len())
                    .collect::<Vec<_>>()
                    .join(" AND "),
            );
            for term in &terms {
                let value = format!("%{}%", escape_like(term));
                values.extend(std::iter::repeat_n(value, 6));
            }
        }
        if !project_keys.is_empty() {
            conditions.push(format!(
                "project_key IN ({})",
                std::iter::repeat_n("?", project_keys.len())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
            values.extend(project_keys);
        }
        if !agents.is_empty() {
            conditions.push(format!(
                "agent IN ({})",
                std::iter::repeat_n("?", agents.len())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
            values.extend(agents.iter().map(|agent| agent.as_str().to_string()));
        }
        if terms.is_empty() && project_paths.is_empty() && agents.is_empty() {
            return Ok(Vec::new());
        }

        let sql = format!(
            "SELECT {SESSION_COLUMNS}
             FROM sessions
             WHERE {}
             ORDER BY updated_at DESC
             LIMIT ?",
            conditions.join(" AND ")
        );
        values.push(limit.to_string());
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(values), row_to_meta)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn count_sessions(&self) -> Result<usize> {
        self.conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|count| count.max(0) as usize)
            .map_err(Into::into)
    }

    pub fn sessions_for_project(
        &self,
        project_path: &str,
        limit: usize,
    ) -> Result<(usize, Vec<SessionMeta>)> {
        let Some(project_key) = normalized_project_key(project_path) else {
            return Ok((0, Vec::new()));
        };
        let total = self.conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE archived = 0 AND project_key = ?1",
            params![project_key],
            |row| row.get::<_, i64>(0),
        )?;
        if limit == 0 {
            return Ok((total.max(0) as usize, Vec::new()));
        }

        let sql = format!(
            "SELECT {SESSION_COLUMNS}
             FROM sessions
             WHERE archived = 0 AND project_key = ?1
             ORDER BY updated_at DESC, key ASC
             LIMIT ?2"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let sql_limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = statement.query_map(params![project_key, sql_limit], row_to_meta)?;
        let sessions = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((total.max(0) as usize, sessions))
    }

    pub fn session(&self, key: &str) -> Result<Option<SessionMeta>> {
        let sql = format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE key = ?1");
        self.conn
            .query_row(&sql, params![key], row_to_meta)
            .optional()
            .map_err(Into::into)
    }

    /// Unscoped message search: identical policy, ordering, and limits as
    /// [`Self::search_scoped`] with no project/agent filter.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        self.search_scoped(query, &[], &[], limit)
    }

    pub fn search_scoped(
        &self,
        query: &str,
        project_paths: &[String],
        agents: &[AgentId],
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let terms = search_terms(query);
        if terms.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        if terms.iter().any(|term| {
            term.chars().count() == 1 && !term.chars().next().is_some_and(is_cjk_search_char)
        }) {
            return Ok(Vec::new());
        }
        let project_keys = normalized_project_keys(project_paths);
        if !project_paths.is_empty() && project_keys.is_empty() {
            return Ok(Vec::new());
        }
        if terms.iter().any(|term| term.chars().count() < 3) {
            return self.search_like_scoped(&terms, &project_keys, agents, limit);
        }

        let fts_query = terms
            .iter()
            .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        let mut conditions = vec![
            "s.archived = 0".to_string(),
            "message_fts MATCH ?".to_string(),
        ];
        let mut values = vec![fts_query];
        if !project_keys.is_empty() {
            conditions.push(format!(
                "s.project_key IN ({})",
                std::iter::repeat_n("?", project_keys.len())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
            values.extend(project_keys);
        }
        if !agents.is_empty() {
            conditions.push(format!(
                "s.agent IN ({})",
                std::iter::repeat_n("?", agents.len())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
            values.extend(agents.iter().map(|agent| agent.as_str().to_string()));
        }
        let sql = format!(
            "SELECT
                {},
                f.seq, f.role,
                snippet(message_fts, 4, '', '', ' … ', 28),
                f.timestamp
             FROM message_fts f
             JOIN sessions s ON s.key = f.session_key
             WHERE {}
             ORDER BY bm25(message_fts), s.updated_at DESC
             LIMIT ?",
            aliased_session_columns("s"),
            conditions.join(" AND ")
        );
        values.push(limit.to_string());
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(values), search_hit_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    fn search_like_scoped(
        &self,
        terms: &[String],
        project_keys: &[String],
        agents: &[AgentId],
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let mut conditions = std::iter::repeat_n("f.text LIKE ? ESCAPE '\\'", terms.len())
            .map(str::to_string)
            .collect::<Vec<_>>();
        conditions.insert(0, "s.archived = 0".to_string());
        let mut values = terms
            .iter()
            .map(|term| format!("%{}%", escape_like(term)))
            .collect::<Vec<_>>();
        if !project_keys.is_empty() {
            conditions.push(format!(
                "s.project_key IN ({})",
                std::iter::repeat_n("?", project_keys.len())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
            values.extend(project_keys.iter().cloned());
        }
        if !agents.is_empty() {
            conditions.push(format!(
                "s.agent IN ({})",
                std::iter::repeat_n("?", agents.len())
                    .collect::<Vec<_>>()
                    .join(",")
            ));
            values.extend(agents.iter().map(|agent| agent.as_str().to_string()));
        }
        let sql = format!(
            "SELECT
                {},
                f.seq, f.role, f.text, f.timestamp
             FROM message_fts f
             JOIN sessions s ON s.key = f.session_key
             WHERE {}
             ORDER BY s.updated_at DESC
             LIMIT ?",
            aliased_session_columns("s"),
            conditions.join(" AND ")
        );
        values.push(limit.to_string());
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(values), search_hit_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }
}

/// The single transcript cache write: page-addressable meta + message pages +
/// seq index for one source identity. Shared by
/// [`HistoryCatalog::cache_transcript`] and the schema migration that folds
/// legacy `transcript_cache` blobs into the page cache.
fn write_transcript_pages(
    conn: &Connection,
    session_key: &str,
    source: &SessionFileRef,
    transcript: &ParsedTranscript,
) -> Result<()> {
    let meta_payload = serde_json::to_vec(&CachedTranscriptMetaPayload {
        meta: transcript.meta.clone(),
        sidechains: transcript.sidechains.clone(),
        unknown_line_count: transcript.unknown_line_count,
    })?;
    let transaction = conn.unchecked_transaction()?;
    transaction.execute(
        "INSERT INTO transcript_page_meta (
            session_key, agent, native_id, file_path, mtime_ms, size_bytes, message_count, payload
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(session_key) DO UPDATE SET
            agent=excluded.agent,
            native_id=excluded.native_id,
            file_path=excluded.file_path,
            mtime_ms=excluded.mtime_ms,
            size_bytes=excluded.size_bytes,
            message_count=excluded.message_count,
            payload=excluded.payload",
        params![
            session_key,
            source.agent.as_str(),
            source.native_id,
            source.file_path,
            source.mtime_ms,
            source.size,
            transcript.mainline.len() as i64,
            meta_payload,
        ],
    )?;
    transaction.execute(
        "DELETE FROM transcript_page_cache WHERE session_key = ?1",
        params![session_key],
    )?;
    transaction.execute(
        "DELETE FROM transcript_message_index WHERE session_key = ?1",
        params![session_key],
    )?;
    {
        let mut insert_page = transaction.prepare(
            "INSERT INTO transcript_page_cache (session_key, page_index, payload)
             VALUES (?1, ?2, ?3)",
        )?;
        for (page_index, page) in transcript
            .mainline
            .chunks(TRANSCRIPT_CACHE_PAGE_SIZE)
            .enumerate()
        {
            let page_payload = serde_json::to_vec(page)?;
            insert_page.execute(params![session_key, page_index as i64, page_payload])?;
        }
    }
    {
        let mut insert_index = transaction.prepare(
            "INSERT OR IGNORE INTO transcript_message_index (session_key, seq, message_index)
             VALUES (?1, ?2, ?3)",
        )?;
        for (message_index, message) in transcript.mainline.iter().enumerate() {
            insert_index.execute(params![session_key, message.seq, message_index as i64])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

/// Current catalog schema version (`PRAGMA user_version`). A database already
/// stamped with this version skips every schema write on open. New migrations
/// append to [`MIGRATIONS`] with the next unused version; existing entries
/// never change or reorder.
const SCHEMA_VERSION: i64 = 3;

type SchemaMigration = fn(&mut Connection) -> Result<()>;

/// Ordered one-shot migrations. `open` runs every entry whose version exceeds
/// the stored `PRAGMA user_version`, bumping the stored version after each
/// step so an interrupted upgrade resumes where it stopped.
const MIGRATIONS: &[(i64, SchemaMigration)] = &[
    (1, migrate_base_schema),
    (2, migrate_rekey_project_keys),
    (3, migrate_retire_transcript_cache),
];

fn run_migrations(conn: &mut Connection) -> Result<usize> {
    let stored: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if stored >= SCHEMA_VERSION {
        return Ok(0);
    }
    let mut applied = 0usize;
    for &(version, step) in MIGRATIONS {
        if stored >= version {
            continue;
        }
        step(conn)?;
        conn.pragma_update(None, "user_version", version)?;
        applied += 1;
    }
    Ok(applied)
}

/// Version 1: current-shape catalog (sessions + page-addressable transcript
/// cache + FTS), the project_key/description columns, and their backfills.
/// Every statement is additive/idempotent so pre-versioned databases upgrade
/// in place without losing sessions.
fn migrate_base_schema(conn: &mut Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sessions (
            key TEXT PRIMARY KEY,
            native_id TEXT NOT NULL,
            agent TEXT NOT NULL,
            title TEXT NOT NULL,
            project_path TEXT NOT NULL,
            project_key TEXT NOT NULL DEFAULT '',
            project_name TEXT NOT NULL,
            file_path TEXT NOT NULL UNIQUE,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            message_count INTEGER NOT NULL,
            size_bytes INTEGER NOT NULL,
            git_branch TEXT,
            model TEXT,
            tokens_used INTEGER,
            archived INTEGER NOT NULL DEFAULT 0,
            source TEXT,
            mtime_ms INTEGER NOT NULL,
            description TEXT NOT NULL DEFAULT ''
         );
         CREATE INDEX IF NOT EXISTS sessions_updated_at ON sessions(updated_at DESC);
         CREATE INDEX IF NOT EXISTS sessions_agent ON sessions(agent);
         CREATE INDEX IF NOT EXISTS sessions_agent_native
            ON sessions(agent, native_id, updated_at DESC);
         CREATE TABLE IF NOT EXISTS transcript_page_meta (
            session_key TEXT PRIMARY KEY REFERENCES sessions(key) ON DELETE CASCADE,
            agent TEXT NOT NULL,
            native_id TEXT NOT NULL,
            file_path TEXT NOT NULL,
            mtime_ms INTEGER NOT NULL,
            size_bytes INTEGER NOT NULL,
            message_count INTEGER NOT NULL,
            payload BLOB NOT NULL
         );
         CREATE TABLE IF NOT EXISTS transcript_page_cache (
            session_key TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
            page_index INTEGER NOT NULL,
            payload BLOB NOT NULL,
            PRIMARY KEY(session_key, page_index)
         );
         CREATE TABLE IF NOT EXISTS transcript_message_index (
            session_key TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
            seq INTEGER NOT NULL,
            message_index INTEGER NOT NULL,
            PRIMARY KEY(session_key, seq)
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
            session_key UNINDEXED,
            seq UNINDEXED,
            role UNINDEXED,
            timestamp UNINDEXED,
            text,
            tokenize='trigram'
         );",
    )?;
    ensure_project_key_column(conn)?;
    ensure_description_column(conn)?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS sessions_project_key ON sessions(project_key, updated_at DESC)",
        [],
    )?;
    backfill_project_keys(conn)
}

/// Version 2 (D31): project keys unified on the preserve-spelling rule. Rows
/// were keyed under the old `fs::canonicalize` rule, so every project_key is
/// cleared and re-derived once from its stored project_path spelling.
fn migrate_rekey_project_keys(conn: &mut Connection) -> Result<()> {
    conn.execute("UPDATE sessions SET project_key = ''", [])?;
    backfill_project_keys(conn)
}

/// One legacy `transcript_cache` row eligible for folding.
struct LegacyTranscriptCacheRow {
    session_key: String,
    agent: String,
    native_id: String,
    file_path: String,
    mtime_ms: i64,
    size_bytes: i64,
    payload: Vec<u8>,
}

/// Version 3 (D29): retire the legacy whole-transcript blob table. Rows still
/// matching their session's current source identity are folded into the
/// page-addressable cache (sessions already covered by the page cache are
/// never re-parsed), then the table is dropped.
fn migrate_retire_transcript_cache(conn: &mut Connection) -> Result<()> {
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'transcript_cache'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)?;
    if !table_exists {
        return Ok(());
    }
    let legacy = {
        let mut statement = conn.prepare(
            "SELECT c.session_key, c.agent, c.native_id, c.file_path,
                    c.mtime_ms, c.size_bytes, c.payload
             FROM transcript_cache c
             JOIN sessions s ON s.key = c.session_key
                AND s.agent = c.agent
                AND s.native_id = c.native_id
                AND s.file_path = c.file_path
                AND s.mtime_ms = c.mtime_ms
                AND s.size_bytes = c.size_bytes
             WHERE NOT EXISTS (
                SELECT 1 FROM transcript_page_meta p WHERE p.session_key = c.session_key
             )",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(LegacyTranscriptCacheRow {
                session_key: row.get(0)?,
                agent: row.get(1)?,
                native_id: row.get(2)?,
                file_path: row.get(3)?,
                mtime_ms: row.get(4)?,
                size_bytes: row.get(5)?,
                payload: row.get(6)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for row in legacy {
        let Some(agent) = AgentId::from_slug(&row.agent) else {
            continue;
        };
        let Ok(transcript) = serde_json::from_slice::<ParsedTranscript>(&row.payload) else {
            continue;
        };
        write_transcript_pages(
            conn,
            &row.session_key,
            &SessionFileRef {
                agent,
                native_id: row.native_id,
                file_path: row.file_path,
                mtime_ms: row.mtime_ms,
                size: row.size_bytes,
            },
            &transcript,
        )?;
    }
    conn.execute("DROP TABLE transcript_cache", [])?;
    Ok(())
}

fn sessions_has_column(conn: &Connection, column: &str) -> Result<bool> {
    let mut statement = conn.prepare("PRAGMA table_info(sessions)")?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for name in columns {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ensure_project_key_column(conn: &mut Connection) -> Result<()> {
    if !sessions_has_column(conn, "project_key")? {
        conn.execute(
            "ALTER TABLE sessions ADD COLUMN project_key TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    Ok(())
}

/// Legacy-database upgrade: add the description column (list-row Description
/// preview) to the sessions table.
fn ensure_description_column(conn: &mut Connection) -> Result<()> {
    if !sessions_has_column(conn, "description")? {
        conn.execute(
            "ALTER TABLE sessions ADD COLUMN description TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    Ok(())
}

/// Re-derive project_key for every row still keyed '' from its stored
/// project_path (used by the base schema and by the D31 re-key migration).
fn backfill_project_keys(conn: &mut Connection) -> Result<()> {
    let pending_paths = {
        let mut statement =
            conn.prepare("SELECT DISTINCT project_path FROM sessions WHERE project_key = ''")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    if pending_paths.is_empty() {
        return Ok(());
    }
    let transaction = conn.transaction()?;
    for project_path in pending_paths {
        let project_key = normalized_project_key(&project_path).unwrap_or_default();
        transaction.execute(
            "UPDATE sessions SET project_key = ?1 WHERE project_key = '' AND project_path = ?2",
            params![project_key, project_path],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn normalized_project_keys(project_paths: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    project_paths
        .iter()
        .filter_map(|path| normalized_project_key(path))
        .filter(|key| seen.insert(key.clone()))
        .collect()
}

/// Catalog project_key: the shared preserve-spelling normalization
/// ([`normalize_path_key`]) of the stored project path. It never touches the
/// filesystem, so the same directory yields one key whether or not it
/// currently exists — the old `fs::canonicalize` rule could split one
/// directory across `/var` and `/private/var` spellings.
fn normalized_project_key(path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    Some(
        normalize_path_key(Path::new(path))
            .to_string_lossy()
            .into_owned(),
    )
}

/// Session Description preview: prefer the first user message, falling back to
/// the first message of any role; collapse all whitespace onto one line and
/// truncate Unicode-safely (DESCRIPTION_MAX_CHARS characters + …).
pub(crate) fn session_description_from_units(units: &[IndexUnit]) -> String {
    let candidate = units
        .iter()
        .find(|unit| unit.role == Role::User && !unit.text.trim().is_empty())
        .or_else(|| units.iter().find(|unit| !unit.text.trim().is_empty()));
    let Some(unit) = candidate else {
        return String::new();
    };
    let mut collapsed = String::with_capacity(DESCRIPTION_MAX_CHARS + 4);
    let mut pending_space = false;
    let mut truncated = false;
    let mut char_count = 0usize;
    for ch in unit.text.chars() {
        if ch.is_whitespace() {
            // Collapse whitespace into a single space; no space at line start
            // or at the truncation tail.
            pending_space = !collapsed.is_empty();
            continue;
        }
        if char_count >= DESCRIPTION_MAX_CHARS {
            truncated = true;
            break;
        }
        if pending_space {
            collapsed.push(' ');
            pending_space = false;
        }
        collapsed.push(ch);
        char_count += 1;
    }
    if truncated {
        collapsed.push('…');
    }
    collapsed
}

const DESCRIPTION_MAX_CHARS: usize = 240;
/// Time budget for one backfill pass: internally loops in 500-row batches
/// until no rows are missing or the budget is exhausted.
const DESCRIPTION_BACKFILL_BUDGET: Duration = Duration::from_millis(1_500);

/// The single SessionMeta projection: `row_to_meta` reads exactly these 16
/// columns in this order, so every session SELECT that maps through
/// `row_to_meta` must select them first and in this order.
const SESSION_COLUMNS: &str = "key, native_id, agent, title, project_path, project_name, file_path, created_at, updated_at, message_count, size_bytes, git_branch, model, tokens_used, archived, source";

/// [`SESSION_COLUMNS`] qualified by a table alias (message_fts join queries).
fn aliased_session_columns(alias: &str) -> String {
    SESSION_COLUMNS
        .split(", ")
        .map(|column| format!("{alias}.{column}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn row_to_meta(row: &Row<'_>) -> rusqlite::Result<SessionMeta> {
    let agent_text: String = row.get(2)?;
    let agent = AgentId::from_slug(&agent_text).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown history agent {agent_text}"),
            )),
        )
    })?;
    Ok(SessionMeta {
        key: row.get(0)?,
        id: row.get(1)?,
        agent,
        title: row.get(3)?,
        project_path: row.get(4)?,
        project_name: row.get(5)?,
        file_path: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        message_count: row.get(9)?,
        size_bytes: row.get(10)?,
        git_branch: row.get(11)?,
        model: row.get(12)?,
        tokens_used: row.get(13)?,
        archived: row.get::<_, i64>(14)? != 0,
        source: row.get(15)?,
    })
}

fn search_hit_from_row(row: &Row<'_>) -> rusqlite::Result<SearchHit> {
    Ok(SearchHit {
        session: row_to_meta(row)?,
        seq: row.get(16)?,
        role: row.get(17)?,
        snippet: row.get(18)?,
        timestamp: row.get(19)?,
    })
}

fn search_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_cjk_search_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{3040}'..='\u{309F}'
            | '\u{30A0}'..='\u{30FF}'
            | '\u{31F0}'..='\u{31FF}'
            | '\u{AC00}'..='\u{D7AF}'
    )
}

fn escape_like(term: &str) -> String {
    term.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MessageKind, ParsedSession, ParsedTranscript, Role, TranscriptMessage};

    #[test]
    fn archived_sessions_are_hidden_from_listing_and_search_but_reachable_by_key() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut catalog = HistoryCatalog::open(&temp.path().join("catalog.db"))?;
        let live = session("live-1", "live session", "/work/demo/a.jsonl", 1_000);
        let mut archived = session("arch-1", "archived secret", "/work/demo/b.jsonl", 2_000);
        archived.meta.archived = true;
        catalog.write_session(&live.meta, 1_000, &live.units)?;
        catalog.write_session(&archived.meta, 2_000, &archived.units)?;

        // Library listing and metadata search: only non-archived visible.
        let listed = catalog.list_sessions(10)?;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key, "live-1");
        assert_eq!(catalog.search_session_metadata("secret", 10)?.len(), 0);
        assert_eq!(catalog.search_session_metadata("session", 10)?.len(), 1);

        // Message-level FTS / LIKE: archived sessions never match,
        // non-archived do.
        assert_eq!(catalog.search("secret", 10)?.len(), 0);
        assert_eq!(catalog.search("session", 10)?.len(), 1);
        assert_eq!(catalog.search("arc", 10)?.len(), 0);

        // Direct access preserved: detail-page reachability is unbroken.
        assert_eq!(
            catalog.session("arch-1")?.map(|meta| meta.key),
            Some("arch-1".to_string())
        );
        Ok(())
    }

    #[test]
    fn wal_journal_mode_is_enabled_for_file_catalogs() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let catalog = HistoryCatalog::open(&temp.path().join("catalog.db"))?;
        let mode: String = catalog
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?;
        assert_eq!(mode, "wal");
        Ok(())
    }

    #[test]
    fn open_initialized_enforces_foreign_keys() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("catalog.db");
        {
            let mut catalog = HistoryCatalog::open(&path)?;
            let parsed = session("k1", "demo", "/work/demo/s1.jsonl", 1_000);
            catalog.write_session(&parsed.meta, 1_000, &parsed.units)?;
            let file_ref = SessionFileRef {
                agent: parsed.meta.agent,
                native_id: "s1".to_string(),
                file_path: "/work/demo/s1.jsonl".to_string(),
                mtime_ms: 1_000,
                size: 10,
            };
            let transcript = ParsedTranscript {
                meta: parsed.meta.clone(),
                mainline: vec![TranscriptMessage {
                    seq: 0,
                    role: Role::User,
                    kind: MessageKind::Text,
                    text: "hello".to_string(),
                    truncated: false,
                    tool_calls: Vec::new(),
                    thinking: None,
                    timestamp: Some(1_000),
                    model: None,
                }],
                sidechains: Vec::new(),
                unknown_line_count: 0,
            };
            catalog.cache_transcript("k1", &file_ref, &transcript)?;
        }
        let catalog = HistoryCatalog::open_initialized(&path)?;
        let fk: i64 = catalog
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
        assert_eq!(fk, 1);
        // Deleting session rows through this connection must cascade-clean the
        // page cache, otherwise orphan rows never become eviction candidates.
        catalog
            .conn
            .execute("DELETE FROM sessions WHERE key = 'k1'", [])?;
        let orphan_pages: i64 = catalog.conn.query_row(
            "SELECT COUNT(*) FROM transcript_page_cache WHERE session_key = 'k1'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(orphan_pages, 0);
        Ok(())
    }

    #[test]
    fn test_delete_session_removes_session_and_cascades() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("history.sqlite");
        let mut catalog = HistoryCatalog::open(&path)?;
        let parsed = session("k_del", "to delete", "/work/demo/s_del.jsonl", 1_000);
        catalog.write_session(&parsed.meta, 1_000, &parsed.units)?;
        assert!(catalog.session("k_del")?.is_some());

        let deleted_path = catalog.delete_session("k_del")?;
        assert_eq!(deleted_path, Some("/work/demo/s_del.jsonl".to_string()));
        assert!(catalog.session("k_del")?.is_none());

        let non_existent = catalog.delete_session("non_existent")?;
        assert_eq!(non_existent, None);
        Ok(())
    }

    fn session(key: &str, title: &str, path: &str, updated_at: i64) -> ParsedSession {
        let meta = SessionMeta {
            key: key.to_string(),
            id: key.to_string(),
            agent: AgentId::ClaudeCode,
            title: title.to_string(),
            project_path: "/work/demo".to_string(),
            project_name: "demo".to_string(),
            file_path: path.to_string(),
            created_at: updated_at - 10,
            updated_at,
            message_count: 1,
            size_bytes: 10,
            git_branch: None,
            model: None,
            tokens_used: None,
            archived: false,
            source: None,
        };
        let message = TranscriptMessage {
            seq: 0,
            role: Role::User,
            kind: MessageKind::Text,
            text: title.to_string(),
            truncated: false,
            tool_calls: Vec::new(),
            thinking: None,
            timestamp: Some(updated_at),
            model: None,
        };
        ParsedSession {
            meta,
            units: vec![IndexUnit {
                seq: 0,
                sidechain_id: None,
                role: message.role,
                timestamp: message.timestamp,
                text: message.text,
            }],
            unknown_line_count: 0,
        }
    }

    #[test]
    fn opening_legacy_catalog_adds_page_cache_tables_without_losing_sessions() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("history.sqlite3");
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE sessions (
                key TEXT PRIMARY KEY,
                native_id TEXT NOT NULL,
                agent TEXT NOT NULL,
                title TEXT NOT NULL,
                project_path TEXT NOT NULL,
                project_name TEXT NOT NULL,
                file_path TEXT NOT NULL UNIQUE,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                message_count INTEGER NOT NULL,
                size_bytes INTEGER NOT NULL,
                git_branch TEXT,
                model TEXT,
                tokens_used INTEGER,
                archived INTEGER NOT NULL DEFAULT 0,
                source TEXT,
                mtime_ms INTEGER NOT NULL
             );
             CREATE TABLE transcript_cache (
                session_key TEXT PRIMARY KEY REFERENCES sessions(key) ON DELETE CASCADE,
                agent TEXT NOT NULL,
                native_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                mtime_ms INTEGER NOT NULL,
                size_bytes INTEGER NOT NULL,
                payload BLOB NOT NULL
             );
             INSERT INTO sessions (
                key, native_id, agent, title, project_path, project_name, file_path,
                created_at, updated_at, message_count, size_bytes, archived, mtime_ms
             ) VALUES (
                'legacy', 'legacy', 'claude-code', 'Legacy session', '/work/demo', 'demo',
                '/tmp/legacy.jsonl', 1, 2, 3, 4, 0, 5
             );",
        )?;
        drop(conn);

        let catalog = HistoryCatalog::open(&path)?;
        assert_eq!(catalog.count_sessions()?, 1);
        for table in [
            "transcript_page_meta",
            "transcript_page_cache",
            "transcript_message_index",
        ] {
            let exists: i64 = catalog.conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                params![table],
                |row| row.get(0),
            )?;
            assert_eq!(exists, 1, "missing migrated table {table}");
        }
        // The legacy whole-blob table is retired by the versioned migrator.
        let legacy_cache: i64 = catalog.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'transcript_cache'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(legacy_cache, 0);
        let version: i64 = catalog
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        assert_eq!(version, SCHEMA_VERSION);
        let project_key: String = catalog.conn.query_row(
            "SELECT project_key FROM sessions WHERE key = 'legacy'",
            [],
            |row| row.get(0),
        )?;
        let expected_project_key = normalized_project_key("/work/demo")
            .ok_or_else(|| anyhow::anyhow!("legacy project path did not normalize"))?;
        assert_eq!(project_key, expected_project_key);
        let project_index_exists: i64 = catalog.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'sessions_project_key'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(project_index_exists, 1);
        let (total, sessions) = catalog.sessions_for_project("/work/demo/./", 10)?;
        assert_eq!(total, 1);
        assert_eq!(sessions[0].key, "legacy");
        Ok(())
    }

    #[test]
    fn current_version_reopen_skips_all_migration_writes() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("catalog.db");
        {
            let (mut catalog, applied) = HistoryCatalog::open_with_migration_count(&path)?;
            assert_eq!(
                applied, SCHEMA_VERSION as usize,
                "a fresh database runs every migration step"
            );
            let parsed = session("k1", "demo", "/tmp/k1.jsonl", 1_000);
            catalog.write_session(&parsed.meta, 1_000, &parsed.units)?;
        }
        let (catalog, applied) = HistoryCatalog::open_with_migration_count(&path)?;
        assert_eq!(
            applied, 0,
            "a current-version reopen must run no migration writes"
        );
        let version: i64 = catalog
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(catalog.count_sessions()?, 1);
        let project_key: String = catalog.conn.query_row(
            "SELECT project_key FROM sessions WHERE key = 'k1'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(project_key, "/work/demo");
        Ok(())
    }

    #[test]
    fn legacy_transcript_cache_blob_folds_into_page_cache_and_drops_table() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("history.sqlite3");
        let transcript = ParsedTranscript {
            meta: session("legacy", "Legacy blob", "/tmp/legacy.jsonl", 2).meta,
            mainline: vec![TranscriptMessage {
                seq: 42,
                role: Role::User,
                kind: MessageKind::Text,
                text: "folded from the legacy blob".to_string(),
                truncated: false,
                tool_calls: Vec::new(),
                thinking: None,
                timestamp: Some(2),
                model: None,
            }],
            sidechains: Vec::new(),
            unknown_line_count: 1,
        };
        {
            let conn = Connection::open(&path)?;
            // Pre-migration database: current sessions shape plus the legacy
            // blob table, seeded with one current-identity row (folds), one
            // stale-identity row (identity mismatch: dropped, not folded), and
            // one row already covered by the page cache (never re-parsed).
            conn.execute_batch(
                "CREATE TABLE sessions (
                    key TEXT PRIMARY KEY,
                    native_id TEXT NOT NULL,
                    agent TEXT NOT NULL,
                    title TEXT NOT NULL,
                    project_path TEXT NOT NULL,
                    project_key TEXT NOT NULL DEFAULT '',
                    project_name TEXT NOT NULL,
                    file_path TEXT NOT NULL UNIQUE,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    message_count INTEGER NOT NULL,
                    size_bytes INTEGER NOT NULL,
                    git_branch TEXT,
                    model TEXT,
                    tokens_used INTEGER,
                    archived INTEGER NOT NULL DEFAULT 0,
                    source TEXT,
                    mtime_ms INTEGER NOT NULL,
                    description TEXT NOT NULL DEFAULT ''
                 );
                 CREATE TABLE transcript_cache (
                    session_key TEXT PRIMARY KEY REFERENCES sessions(key) ON DELETE CASCADE,
                    agent TEXT NOT NULL,
                    native_id TEXT NOT NULL,
                    file_path TEXT NOT NULL,
                    mtime_ms INTEGER NOT NULL,
                    size_bytes INTEGER NOT NULL,
                    payload BLOB NOT NULL
                 );
                 INSERT INTO sessions (
                    key, native_id, agent, title, project_path, project_key, project_name,
                    file_path, created_at, updated_at, message_count, size_bytes, mtime_ms
                 ) VALUES
                    ('legacy', 'legacy', 'claude-code', 'Legacy blob', '/work/demo',
                     '/work/demo', 'demo', '/tmp/legacy.jsonl', 1, 2, 3, 4, 5),
                    ('stale', 'stale', 'claude-code', 'Stale blob', '/work/demo',
                     '/work/demo', 'demo', '/tmp/stale.jsonl', 1, 2, 3, 4, 5);",
            )?;
            let payload = serde_json::to_vec(&transcript)?;
            conn.execute(
                "INSERT INTO transcript_cache (
                    session_key, agent, native_id, file_path, mtime_ms, size_bytes, payload
                 ) VALUES ('legacy', 'claude-code', 'legacy', '/tmp/legacy.jsonl', 5, 4, ?1)",
                params![payload],
            )?;
            conn.execute(
                "INSERT INTO transcript_cache (
                    session_key, agent, native_id, file_path, mtime_ms, size_bytes, payload
                 ) VALUES ('stale', 'claude-code', 'stale', '/tmp/stale.jsonl', 999, 4, ?1)",
                params![payload],
            )?;
        }

        let (catalog, applied) = HistoryCatalog::open_with_migration_count(&path)?;
        assert!(applied >= 1);
        let legacy_cache: i64 = catalog.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'transcript_cache'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(legacy_cache, 0, "transcript_cache must be dropped");
        let version: i64 = catalog
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        assert_eq!(version, SCHEMA_VERSION);

        let source = SessionFileRef {
            agent: AgentId::ClaudeCode,
            native_id: "legacy".to_string(),
            file_path: "/tmp/legacy.jsonl".to_string(),
            mtime_ms: 5,
            size: 4,
        };
        let window = catalog
            .cached_transcript_window("legacy", &source, 0, 10)?
            .ok_or_else(|| anyhow::anyhow!("legacy blob was not folded into the page cache"))?;
        assert_eq!(window.messages, transcript.mainline);
        assert_eq!(window.unknown_line_count, 1);
        assert_eq!(
            catalog.cached_transcript_index_for_seq("legacy", &source, 42)?,
            Some(0)
        );
        // The stale-identity blob is gone without polluting the page cache.
        let stale_meta: i64 = catalog.conn.query_row(
            "SELECT COUNT(*) FROM transcript_page_meta WHERE session_key = 'stale'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(stale_meta, 0);

        // A database already migrated reopens cleanly with no further writes.
        drop(catalog);
        let (_catalog, applied) = HistoryCatalog::open_with_migration_count(&path)?;
        assert_eq!(applied, 0);
        Ok(())
    }

    #[test]
    fn rekey_migration_unifies_project_key_under_the_stored_spelling() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("history.sqlite3");
        {
            let conn = Connection::open(&path)?;
            // Pre-migration row keyed under the old fs::canonicalize rule: the
            // /var spelling was rewritten to its /private/var target.
            conn.execute_batch(
                "CREATE TABLE sessions (
                    key TEXT PRIMARY KEY,
                    native_id TEXT NOT NULL,
                    agent TEXT NOT NULL,
                    title TEXT NOT NULL,
                    project_path TEXT NOT NULL,
                    project_key TEXT NOT NULL DEFAULT '',
                    project_name TEXT NOT NULL,
                    file_path TEXT NOT NULL UNIQUE,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    message_count INTEGER NOT NULL,
                    size_bytes INTEGER NOT NULL,
                    git_branch TEXT,
                    model TEXT,
                    tokens_used INTEGER,
                    archived INTEGER NOT NULL DEFAULT 0,
                    source TEXT,
                    mtime_ms INTEGER NOT NULL,
                    description TEXT NOT NULL DEFAULT ''
                 );
                 INSERT INTO sessions (
                    key, native_id, agent, title, project_path, project_key, project_name,
                    file_path, created_at, updated_at, message_count, size_bytes, mtime_ms
                 ) VALUES (
                    'legacy', 'legacy', 'claude-code', 'Symlink root', '/var/lab',
                    '/private/var/lab', 'lab', '/tmp/lab.jsonl', 1, 2, 1, 4, 5
                 );",
            )?;
        }
        let (catalog, applied) = HistoryCatalog::open_with_migration_count(&path)?;
        assert!(applied >= 1);

        // The re-key re-derives from the stored project_path spelling, so the
        // directory lives under exactly one deterministic key again.
        let project_key: String = catalog.conn.query_row(
            "SELECT project_key FROM sessions WHERE key = 'legacy'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(project_key, "/var/lab");
        let (total, sessions) = catalog.sessions_for_project("/var/lab", 10)?;
        assert_eq!(total, 1);
        assert_eq!(sessions[0].key, "legacy");
        let scoped =
            catalog.search_session_metadata_scoped("", &["/var/lab".to_string()], &[], 10)?;
        assert_eq!(scoped.len(), 1);
        // The old canonicalized spelling is a distinct key now and matches
        // nothing — the fs no longer decides which key a directory maps to.
        assert_eq!(catalog.sessions_for_project("/private/var/lab", 10)?.0, 0);
        Ok(())
    }

    #[test]
    fn page_cache_invalidates_with_source_identity() -> Result<()> {
        let mut catalog = HistoryCatalog::memory()?;
        let first = session("a", "cached transcript", "/tmp/a.jsonl", 10);
        catalog.write_session(&first.meta, 100, &first.units)?;
        let source = catalog
            .transcript_source("a")?
            .ok_or_else(|| anyhow::anyhow!("missing source identity"))?;
        assert_eq!(source.mtime_ms, 100);
        assert_eq!(source.size, first.meta.size_bytes);
        assert!(catalog
            .cached_transcript_window("a", &source, 0, 60)?
            .is_none());

        let transcript = ParsedTranscript {
            meta: first.meta.clone(),
            mainline: vec![TranscriptMessage {
                seq: 0,
                role: Role::User,
                kind: MessageKind::Text,
                text: "cached transcript".to_string(),
                truncated: false,
                tool_calls: Vec::new(),
                thinking: None,
                timestamp: Some(10),
                model: None,
            }],
            sidechains: Vec::new(),
            unknown_line_count: 0,
        };
        catalog.cache_transcript("a", &source, &transcript)?;
        let window = catalog
            .cached_transcript_window("a", &source, 0, 60)?
            .ok_or_else(|| anyhow::anyhow!("missing paged cache"))?;
        assert_eq!(window.messages, transcript.mainline);

        catalog.write_session(&first.meta, 101, &first.units)?;
        let changed_source = catalog
            .transcript_source("a")?
            .ok_or_else(|| anyhow::anyhow!("missing changed source identity"))?;
        assert_eq!(changed_source.mtime_ms, 101);
        assert!(catalog
            .cached_transcript_window("a", &source, 0, 60)?
            .is_none());
        assert!(catalog
            .cached_transcript_window("a", &changed_source, 0, 60)?
            .is_none());
        Ok(())
    }

    #[test]
    fn transcript_page_cache_prunes_oldest_entries_by_count_and_budget() -> Result<()> {
        let mut catalog = HistoryCatalog::memory()?;
        for (key, updated_at) in [("old", 10), ("middle", 20), ("new", 30)] {
            let mut parsed = session(key, key, &format!("/tmp/{key}.jsonl"), updated_at);
            parsed.meta.size_bytes = 2_048;
            catalog.write_session(&parsed.meta, updated_at, &parsed.units)?;
            let source = catalog
                .transcript_source(key)?
                .ok_or_else(|| anyhow::anyhow!("missing source for {key}"))?;
            let transcript = ParsedTranscript {
                meta: parsed.meta.clone(),
                mainline: vec![TranscriptMessage {
                    seq: updated_at,
                    role: Role::Assistant,
                    kind: MessageKind::Text,
                    text: "x".repeat(1_024),
                    truncated: false,
                    tool_calls: Vec::new(),
                    thinking: None,
                    timestamp: Some(updated_at),
                    model: None,
                }],
                sidechains: Vec::new(),
                unknown_line_count: 0,
            };
            catalog.cache_transcript(key, &source, &transcript)?;
        }

        assert_eq!(catalog.prune_transcript_page_cache_to(2, i64::MAX)?, 1);
        let old_source = catalog
            .transcript_source("old")?
            .ok_or_else(|| anyhow::anyhow!("missing old source"))?;
        let new_source = catalog
            .transcript_source("new")?
            .ok_or_else(|| anyhow::anyhow!("missing new source"))?;
        assert!(catalog
            .cached_transcript_window("old", &old_source, 0, 1)?
            .is_none());
        assert!(catalog
            .cached_transcript_window("new", &new_source, 0, 1)?
            .is_some());

        assert_eq!(catalog.prune_transcript_page_cache_to(2, 1)?, 2);
        assert!(catalog
            .cached_transcript_window("new", &new_source, 0, 1)?
            .is_none());
        Ok(())
    }

    #[test]
    fn transcript_page_cache_reads_requested_window_and_resolves_seq_index() -> Result<()> {
        let mut catalog = HistoryCatalog::memory()?;
        let mut parsed = session("paged", "paged transcript", "/tmp/paged.jsonl", 10);
        parsed.meta.message_count = 150;
        parsed.meta.size_bytes = 150_000;
        catalog.write_session(&parsed.meta, 100, &parsed.units)?;
        let source = catalog
            .transcript_source("paged")?
            .ok_or_else(|| anyhow::anyhow!("missing paged source identity"))?;
        let transcript = ParsedTranscript {
            meta: parsed.meta.clone(),
            mainline: (0..150)
                .map(|index| TranscriptMessage {
                    seq: 1_000 + index,
                    role: if index % 2 == 0 {
                        Role::User
                    } else {
                        Role::Assistant
                    },
                    kind: MessageKind::Text,
                    text: format!("message-{index}"),
                    truncated: false,
                    tool_calls: Vec::new(),
                    thinking: None,
                    timestamp: Some(index),
                    model: None,
                })
                .collect(),
            sidechains: Vec::new(),
            unknown_line_count: 7,
        };
        catalog.cache_transcript("paged", &source, &transcript)?;
        let window = catalog
            .cached_transcript_window("paged", &source, 50, 60)?
            .ok_or_else(|| anyhow::anyhow!("missing cached window"))?;
        assert_eq!(window.total_messages, 150);
        assert_eq!(window.start, 50);
        assert_eq!(window.messages.len(), 60);
        assert_eq!(
            window.messages.first().map(|message| message.seq),
            Some(1_050)
        );
        assert_eq!(
            window.messages.last().map(|message| message.seq),
            Some(1_109)
        );
        assert_eq!(
            catalog.cached_transcript_index_for_seq("paged", &source, 1_123)?,
            Some(123)
        );

        catalog.write_session(&parsed.meta, 101, &parsed.units)?;
        let changed_source = catalog
            .transcript_source("paged")?
            .ok_or_else(|| anyhow::anyhow!("missing changed paged source identity"))?;
        assert!(catalog
            .cached_transcript_window("paged", &changed_source, 0, 60)?
            .is_none());
        assert_eq!(
            catalog.cached_transcript_index_for_seq("paged", &changed_source, 1_123)?,
            None
        );
        let stale_pages: i64 = catalog.conn.query_row(
            "SELECT COUNT(*) FROM transcript_page_cache WHERE session_key = 'paged'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(stale_pages, 0);
        Ok(())
    }

    #[test]
    fn transcript_page_cache_keeps_first_index_for_duplicate_seq() -> Result<()> {
        let mut catalog = HistoryCatalog::memory()?;
        let mut parsed = session("dupe", "duplicate seq", "/tmp/dupe.jsonl", 10);
        parsed.meta.message_count = 3;
        catalog.write_session(&parsed.meta, 100, &parsed.units)?;
        let source = catalog
            .transcript_source("dupe")?
            .ok_or_else(|| anyhow::anyhow!("missing duplicate-seq source identity"))?;
        let mut message = TranscriptMessage {
            seq: 7,
            role: Role::Assistant,
            kind: MessageKind::Text,
            text: "first".to_string(),
            truncated: false,
            tool_calls: Vec::new(),
            thinking: None,
            timestamp: None,
            model: None,
        };
        let first = message.clone();
        message.text = "second".to_string();
        let second = message.clone();
        message.seq = 8;
        message.text = "third".to_string();
        let transcript = ParsedTranscript {
            meta: parsed.meta,
            mainline: vec![first, second, message],
            sidechains: Vec::new(),
            unknown_line_count: 0,
        };
        catalog.cache_transcript("dupe", &source, &transcript)?;
        assert_eq!(
            catalog.cached_transcript_index_for_seq("dupe", &source, 7)?,
            Some(0)
        );
        let window = catalog
            .cached_transcript_window("dupe", &source, 0, 60)?
            .ok_or_else(|| anyhow::anyhow!("missing duplicate-seq window"))?;
        assert_eq!(window.messages.len(), 3);
        Ok(())
    }

    #[test]
    #[ignore = "local performance smoke; run explicitly on the development Mac"]
    fn transcript_page_cache_large_history_window_stays_within_local_budget() -> Result<()> {
        use std::time::Instant;

        let temp = tempfile::tempdir()?;
        let db_path = temp.path().join("history.sqlite3");
        let mut catalog = HistoryCatalog::open(&db_path)?;
        let mut parsed = session("perf", "large transcript", "/tmp/perf.jsonl", 10);
        parsed.meta.message_count = 10_000;
        parsed.meta.size_bytes = 10_000_000;
        catalog.write_session(&parsed.meta, 100, &parsed.units)?;
        let source = catalog
            .transcript_source("perf")?
            .ok_or_else(|| anyhow::anyhow!("missing perf source identity"))?;
        let transcript = ParsedTranscript {
            meta: parsed.meta.clone(),
            mainline: (0..10_000)
                .map(|index| TranscriptMessage {
                    seq: index,
                    role: Role::Assistant,
                    kind: MessageKind::Text,
                    text: format!("message-{index}-{}", "x".repeat(512)),
                    truncated: false,
                    tool_calls: Vec::new(),
                    thinking: None,
                    timestamp: Some(index),
                    model: None,
                })
                .collect(),
            sidechains: Vec::new(),
            unknown_line_count: 0,
        };
        let build_started = Instant::now();
        catalog.cache_transcript("perf", &source, &transcript)?;
        let build_elapsed = build_started.elapsed();
        let build_ms = build_elapsed.as_secs_f64() * 1_000.0;
        eprintln!("history page cache build: messages=10000 {build_ms:.2}ms");
        assert!(
            build_ms < 2_000.0,
            "10k-message page cache build took {build_ms:.2}ms"
        );

        drop(catalog);
        let samples = 64usize;
        let started = Instant::now();
        for sample in 0..samples {
            let start = (sample * 137) % (10_000 - 60);
            let catalog = HistoryCatalog::open_initialized(&db_path)?;
            let window = catalog
                .cached_transcript_window("perf", &source, start, 60)?
                .ok_or_else(|| anyhow::anyhow!("missing perf window"))?;
            assert_eq!(window.start, start);
            assert_eq!(window.messages.len(), 60);
        }
        let elapsed = started.elapsed();
        let average_ms = elapsed.as_secs_f64() * 1_000.0 / samples as f64;
        eprintln!(
            "history page cache: samples={samples} total={:.2}ms avg={average_ms:.2}ms",
            elapsed.as_secs_f64() * 1_000.0
        );
        assert!(
            average_ms < 50.0,
            "cached 60-message window averaged {average_ms:.2}ms"
        );
        Ok(())
    }

    #[test]
    fn uncached_transcript_backfill_selection_respects_count_and_byte_budget() -> Result<()> {
        let mut catalog = HistoryCatalog::memory()?;
        let mut huge = session("huge", "huge", "/tmp/huge.jsonl", 30);
        let mut recent = session("recent", "recent", "/tmp/recent.jsonl", 20);
        let mut older = session("older", "older", "/tmp/older.jsonl", 10);
        huge.meta.size_bytes = 20 * 1024 * 1024;
        recent.meta.size_bytes = 4 * 1024 * 1024;
        older.meta.size_bytes = 3 * 1024 * 1024;
        catalog.write_session(&huge.meta, 300, &huge.units)?;
        catalog.write_session(&recent.meta, 200, &recent.units)?;
        catalog.write_session(&older.meta, 100, &older.units)?;

        let selected = catalog.uncached_transcript_sources(2, 8 * 1024 * 1024)?;
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].0.key, "recent");
        assert_eq!(selected[1].0.key, "older");
        assert!(selected.iter().all(|(meta, _)| meta.key != "huge"));
        Ok(())
    }

    #[test]
    fn catalog_counts_all_sessions_and_pages_project_metadata() -> Result<()> {
        let mut catalog = HistoryCatalog::memory()?;
        let mut first = session("a", "first", "/tmp/a.jsonl", 10);
        let mut second = session("b", "second", "/tmp/b.jsonl", 20);
        let mut other = session("c", "other", "/tmp/c.jsonl", 30);
        first.meta.project_path = "/work/demo".to_string();
        second.meta.project_path = "/work/demo/./".to_string();
        other.meta.project_path = "/work/other".to_string();
        catalog.write_session(&first.meta, 100, &first.units)?;
        catalog.write_session(&second.meta, 200, &second.units)?;
        catalog.write_session(&other.meta, 300, &other.units)?;

        assert_eq!(catalog.count_sessions()?, 3);
        let (total, sessions) = catalog.sessions_for_project("/work/demo", 1)?;
        assert_eq!(total, 2);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].key, "b");
        Ok(())
    }

    #[test]
    fn scoped_history_search_filters_messages_and_metadata_by_workspace_and_agent() -> Result<()> {
        let mut catalog = HistoryCatalog::memory()?;
        let mut claude_a = session("claude-a", "Parser issue", "/tmp/claude-a.jsonl", 30);
        claude_a.meta.project_path = "/work/a".to_string();
        claude_a.meta.project_name = "a".to_string();
        claude_a.units[0].text = "parser regression in workspace a".to_string();

        let mut codex_a = session("codex-a", "Parser issue", "/tmp/codex-a.jsonl", 20);
        codex_a.meta.agent = AgentId::Codex;
        codex_a.meta.project_path = "/work/a/./".to_string();
        codex_a.meta.project_name = "a".to_string();
        codex_a.units[0].text = "parser regression from codex".to_string();

        let mut claude_b = session("claude-b", "Parser issue", "/tmp/claude-b.jsonl", 10);
        claude_b.meta.project_path = "/work/b".to_string();
        claude_b.meta.project_name = "b".to_string();
        claude_b.units[0].text = "parser regression in workspace b".to_string();

        for parsed in [&claude_a, &codex_a, &claude_b] {
            catalog.write_session(&parsed.meta, parsed.meta.updated_at, &parsed.units)?;
        }

        let project_paths = vec!["/work/a".to_string()];
        let agents = vec![AgentId::ClaudeCode];
        let messages = catalog.search_scoped("parser", &project_paths, &agents, 10)?;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].session.key, "claude-a");

        let metadata = catalog.search_session_metadata_scoped("", &project_paths, &agents, 10)?;
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].key, "claude-a");
        Ok(())
    }

    #[test]
    fn catalog_orders_searches_cjk_and_removes_missing() -> Result<()> {
        let mut catalog = HistoryCatalog::memory()?;
        let first = session("a", "修复 parser regression", "/tmp/a.jsonl", 10);
        let second = session("b", "cargo::metadata regression", "/tmp/b.jsonl", 20);
        catalog.write_session(&first.meta, 100, &first.units)?;
        catalog.write_session(&second.meta, 200, &second.units)?;

        let listed = catalog.list_sessions(10)?;
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].key, "b");
        assert_eq!(catalog.search("修复 parser", 10)?.len(), 1);
        assert_eq!(catalog.search("修", 10)?.len(), 1);
        assert!(catalog.search("c", 10)?.is_empty());
        assert_eq!(catalog.search("ca", 10)?.len(), 1);
        assert_eq!(catalog.search("cargo::meta", 10)?.len(), 1);

        let mut metadata_first = first.meta.clone();
        metadata_first.title = "Parser recovery plan".to_string();
        metadata_first.project_path = "/work/parser-lab".to_string();
        metadata_first.project_name = "parser-lab".to_string();
        catalog.write_session(&metadata_first, 100, &first.units)?;
        assert_eq!(catalog.search_session_metadata("recovery", 10)?.len(), 1);
        assert_eq!(catalog.search_session_metadata("parser-lab", 10)?.len(), 1);
        assert_eq!(
            catalog
                .search_session_metadata("parser recovery", 10)?
                .len(),
            1
        );
        assert!(catalog
            .search_session_metadata("missing-project", 10)?
            .is_empty());

        let seen = HashSet::from(["/tmp/b.jsonl".to_string()]);
        assert_eq!(catalog.remove_missing(&seen)?, 1);
        assert!(catalog.session("a")?.is_none());
        assert!(catalog.session("b")?.is_some());
        Ok(())
    }

    #[test]
    fn session_summaries_carry_description_derived_from_first_user_message() -> Result<()> {
        let mut catalog = HistoryCatalog::memory()?;
        let mut parsed = session("a", "Fix login flow", "/tmp/a.jsonl", 10);
        parsed.units.insert(
            0,
            IndexUnit {
                seq: -1,
                sidechain_id: None,
                role: Role::System,
                timestamp: None,
                text: "system preamble should be skipped".to_string(),
            },
        );
        catalog.write_session(&parsed.meta, 100, &parsed.units)?;

        let summaries = catalog.list_session_summaries(10)?;
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].description, "Fix login flow");
        assert_eq!(summaries[0].meta.key, "a");
        Ok(())
    }

    #[test]
    fn description_backfill_fills_legacy_rows_from_fts_in_one_pass() -> Result<()> {
        let mut catalog = HistoryCatalog::memory()?;
        let first = session("a", "Legacy one", "/tmp/a.jsonl", 10);
        let second = session("b", "Legacy two", "/tmp/b.jsonl", 20);
        for parsed in [&first, &second] {
            catalog.write_session(&parsed.meta, 100, &parsed.units)?;
        }
        // Simulate a legacy database: clear the description column.
        catalog
            .conn
            .execute("UPDATE sessions SET description = ''", [])?;

        // One call's internal loop clears every missing row (500-row batches
        // within the time budget).
        assert_eq!(catalog.backfill_session_descriptions()?, 2);
        let summaries = catalog.list_session_summaries(10)?;
        assert!(summaries
            .iter()
            .all(|summary| summary.description == "Legacy one"
                || summary.description == "Legacy two"));
        // No-op when no rows are missing.
        assert_eq!(catalog.backfill_session_descriptions()?, 0);
        Ok(())
    }

    #[test]
    fn session_description_is_unicode_safe_single_line_and_bounded() {
        let units = |text: String| {
            vec![IndexUnit {
                seq: 0,
                sidechain_id: None,
                role: Role::User,
                timestamp: None,
                text,
            }]
        };
        // CJK truncation stays on character boundaries and appends an
        // ellipsis.
        let long = "你".repeat(300);
        let description = session_description_from_units(&units(long));
        assert!(description.ends_with('…'));
        assert_eq!(description.chars().count(), 241);
        // Multi-line input collapses onto one line.
        assert_eq!(
            session_description_from_units(&units("first\n\nsecond\tline".to_string())),
            "first second line"
        );
        // Trailing whitespace at the cap does not fabricate an ellipsis.
        assert_eq!(
            session_description_from_units(&units(format!("{}   ", "a".repeat(240)))),
            "a".repeat(240)
        );
        // A non-whitespace character after the cap truncates with the
        // ellipsis.
        let tail = session_description_from_units(&units(format!("{}   more", "a".repeat(240))));
        assert!(tail.ends_with('…'));
        assert_eq!(tail.chars().count(), 241);
        // Empty input yields an empty string.
        assert_eq!(session_description_from_units(&[]), "");
    }

    #[test]
    fn unscoped_queries_match_scoped_queries_with_empty_filters() -> Result<()> {
        let mut catalog = HistoryCatalog::memory()?;
        let mut a = session("a", "Parser regression", "/tmp/a.jsonl", 30);
        a.units[0].text = "parser regression detail".to_string();
        let mut b = session("b", "Parser followup", "/tmp/b.jsonl", 20);
        b.meta.agent = AgentId::Codex;
        b.units[0].text = "parser followup note".to_string();
        let c = session("c", "Unrelated", "/tmp/c.jsonl", 10);
        for parsed in [&a, &b, &c] {
            catalog.write_session(&parsed.meta, parsed.meta.updated_at, &parsed.units)?;
        }

        for limit in [0usize, 1, 2, 10] {
            let summaries = catalog.list_session_summaries(limit)?;
            assert_eq!(
                catalog.list_sessions(limit)?,
                summaries
                    .into_iter()
                    .map(|summary| summary.meta)
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                catalog.search_session_metadata("parser", limit)?,
                catalog.search_session_metadata_scoped("parser", &[], &[], limit)?
            );
            // Three characters: FTS path. Two characters: the LIKE fallback.
            assert_eq!(
                catalog.search("parser", limit)?,
                catalog.search_scoped("parser", &[], &[], limit)?
            );
            assert_eq!(
                catalog.search("pa", limit)?,
                catalog.search_scoped("pa", &[], &[], limit)?
            );
        }
        // Newest first across both agents, unscoped.
        assert_eq!(
            catalog
                .search_session_metadata("parser", 10)?
                .into_iter()
                .map(|meta| meta.key)
                .collect::<Vec<_>>(),
            vec!["a".to_string(), "b".to_string()]
        );
        // Single-character non-CJK terms are rejected by the shared message
        // search policy.
        assert!(catalog.search("p", 10)?.is_empty());
        Ok(())
    }

    #[test]
    fn session_source_by_native_returns_newest_and_has_composite_index() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let mut catalog = HistoryCatalog::open(&temp.path().join("catalog.db"))?;
        let mut older = session("old", "older", "/tmp/old.jsonl", 100);
        older.meta.id = "shared-native".to_string();
        let mut newer = session("new", "newer", "/tmp/new.jsonl", 200);
        newer.meta.id = "shared-native".to_string();
        catalog.write_session(&older.meta, 100, &older.units)?;
        catalog.write_session(&newer.meta, 200, &newer.units)?;

        let source = catalog
            .session_source_by_native(AgentId::ClaudeCode, "shared-native")?
            .ok_or_else(|| anyhow::anyhow!("missing native source"))?;
        assert_eq!(source.native_id, "shared-native");
        assert_eq!(source.file_path, "/tmp/new.jsonl");
        assert!(catalog
            .session_source_by_native(AgentId::ClaudeCode, "missing")?
            .is_none());

        // The (agent, native_id, updated_at DESC) index backs the exact
        // binding lookup used by every Chat attach.
        let index_exists: i64 = catalog.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'index' AND name = 'sessions_agent_native'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(index_exists, 1);
        Ok(())
    }

    #[test]
    fn transcript_page_cache_prune_accounts_for_meta_and_all_pages() -> Result<()> {
        let mut catalog = HistoryCatalog::memory()?;
        let mut parsed = session("multi", "multi page", "/tmp/multi.jsonl", 10);
        parsed.meta.message_count = 130;
        catalog.write_session(&parsed.meta, 100, &parsed.units)?;
        let source = catalog
            .transcript_source("multi")?
            .ok_or_else(|| anyhow::anyhow!("missing multi source identity"))?;
        let transcript = ParsedTranscript {
            meta: parsed.meta.clone(),
            mainline: (0..130)
                .map(|index| TranscriptMessage {
                    seq: index,
                    role: Role::User,
                    kind: MessageKind::Text,
                    text: format!("message-{index}"),
                    truncated: false,
                    tool_calls: Vec::new(),
                    thinking: None,
                    timestamp: Some(index),
                    model: None,
                })
                .collect(),
            sidechains: Vec::new(),
            unknown_line_count: 0,
        };
        catalog.cache_transcript("multi", &source, &transcript)?;
        assert_eq!(
            catalog
                .cached_transcript_window("multi", &source, 0, 130)?
                .ok_or_else(|| anyhow::anyhow!("missing cached window"))?
                .messages
                .len(),
            130
        );

        // The pruner's own logical-size figure: meta payload + every page
        // payload + 24 bytes per message. A budget at the figure retains the
        // session; one byte under evicts it.
        let logical_bytes: i64 = {
            let meta_payload: i64 = catalog.conn.query_row(
                "SELECT LENGTH(payload) FROM transcript_page_meta WHERE session_key = 'multi'",
                [],
                |row| row.get(0),
            )?;
            let page_payloads: i64 = catalog.conn.query_row(
                "SELECT COALESCE(SUM(LENGTH(payload)), 0)
                 FROM transcript_page_cache WHERE session_key = 'multi'",
                [],
                |row| row.get(0),
            )?;
            meta_payload + page_payloads + 130 * 24
        };
        assert_eq!(catalog.prune_transcript_page_cache_to(8, logical_bytes)?, 0);
        assert!(catalog
            .cached_transcript_window("multi", &source, 0, 130)?
            .is_some());
        assert_eq!(
            catalog.prune_transcript_page_cache_to(8, logical_bytes - 1)?,
            1
        );
        assert!(catalog
            .cached_transcript_window("multi", &source, 0, 130)?
            .is_none());
        Ok(())
    }
}
