// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

use super::parse_utils::*;
use super::sqlite_ro::{open_sqlite_ro, virtual_path, SqliteRo};
use super::{units_from_messages, AgentHistoryAdapter};
use crate::models::*;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// OpenCode stable: `opencode.db`; OpenCode 2 next channel: `opencode-next.db`.
/// The two databases can coexist and must be scanned in parallel rather than
/// picking one.
pub struct OpencodeAdapter {
    dbs: Vec<OcDb>,
}

struct OcDb {
    path: PathBuf,
    rows_cache: MtimeCache<Vec<OcRow>>,
}

const ROW_COLS: &str = "s.id, s.directory, s.title, s.time_created, s.time_updated,
        s.model, s.tokens_input + s.tokens_output + s.tokens_reasoning,
        s.time_archived";

fn has_table(conn: &rusqlite::Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

fn has_column(conn: &rusqlite::Connection, table: &str, column: &str) -> bool {
    let mut statement = match conn.prepare(&format!("PRAGMA table_info({table})")) {
        Ok(statement) => statement,
        Err(_) => return false,
    };
    let rows = match statement.query_map([], |row| row.get::<_, String>(1)) {
        Ok(rows) => rows,
        Err(_) => return false,
    };
    let mut has = false;
    for name in rows.flatten() {
        if name == column {
            has = true;
            break;
        }
    }
    has
}

fn root_predicate(conn: &rusqlite::Connection, table: &str) -> &'static str {
    if has_column(conn, table, "parent_id") {
        "s.parent_id IS NULL"
    } else {
        "1"
    }
}

fn version_expr(conn: &rusqlite::Connection, table: &str) -> &'static str {
    if has_column(conn, table, "version") {
        "s.version"
    } else {
        "''"
    }
}

fn select_from(table: &str, version: &str, content_len: &str, v2_messages: &str) -> String {
    format!(
        "SELECT {ROW_COLS}, {version} AS version, {content_len} AS content_len,
                {v2_messages} AS v2_messages
         FROM {table} s"
    )
}

/// Dynamically generate session-enumeration SQL compatible with
/// v1/v2/preview.
fn rows_sql(conn: &rusqlite::Connection) -> Option<String> {
    let has_session = has_table(conn, "session");
    let has_session_v2 = has_table(conn, "session_v2");
    let has_parts = has_table(conn, "part");
    let has_session_messages = has_table(conn, "session_message");
    if !has_session && !has_session_v2 {
        return None;
    }

    let part_len =
        "(SELECT COALESCE(SUM(LENGTH(p.data)), 0) FROM part p WHERE p.session_id = s.id)";
    let message_len = "(SELECT COALESCE(SUM(LENGTH(m.data)), 0) FROM session_message m \
                       WHERE m.session_id = s.id)";
    let message_exists = "EXISTS(SELECT 1 FROM session_message m WHERE m.session_id = s.id)";

    let mut selects = Vec::new();
    if has_session_v2 {
        let len = if has_session_messages {
            message_len
        } else {
            "0"
        };
        selects.push(format!(
            "{} WHERE {}",
            select_from("session_v2", version_expr(conn, "session_v2"), len, "1"),
            root_predicate(conn, "session_v2")
        ));
    }

    if has_session {
        let (len, v2) = match (has_parts, has_session_messages) {
            (true, true) => (
                format!("CASE WHEN {message_exists} THEN {message_len} ELSE {part_len} END"),
                format!("CASE WHEN {message_exists} THEN 1 ELSE 0 END"),
            ),
            (true, false) => (part_len.to_string(), "0".to_string()),
            (false, true) => (message_len.to_string(), "1".to_string()),
            (false, false) => ("0".to_string(), "0".to_string()),
        };
        let mut sql = format!(
            "{} WHERE {}",
            select_from("session", version_expr(conn, "session"), &len, &v2),
            root_predicate(conn, "session")
        );
        if has_session_v2 {
            sql.push_str(" AND s.id NOT IN (SELECT id FROM session_v2)");
        }
        selects.push(sql);
    }
    Some(selects.join(" UNION ALL "))
}

fn query_rows(conn: &rusqlite::Connection, id: Option<&str>) -> Result<Vec<OcRow>> {
    let sql = rows_sql(conn).ok_or_else(|| anyhow!("opencode database has no session table"))?;
    let sql = match id {
        Some(_) => format!("SELECT * FROM ({sql}) WHERE id = ?1"),
        None => sql,
    };
    let mut statement = conn.prepare(&sql)?;
    let rows = match id {
        Some(id) => statement
            .query_map([id], row_from)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        None => statement
            .query_map([], row_from)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    Ok(rows)
}

fn row_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<OcRow> {
    Ok(OcRow {
        id: row.get(0)?,
        directory: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
        created_ms: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
        updated_ms: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
        model_json: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        tokens: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
        archived: row.get::<_, Option<i64>>(7)?.is_some(),
        version: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
        content_len: row.get(9)?,
        v2_messages: row.get::<_, i64>(10)? != 0,
    })
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

fn known_db_paths(dir: &Path) -> Vec<PathBuf> {
    vec![dir.join("opencode.db"), dir.join("opencode-next.db")]
}

fn default_db_paths() -> Vec<PathBuf> {
    let default_dir = home_dir().join(".local/share/opencode");
    let xdg_dir = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(|value| PathBuf::from(value).join("opencode"));
    let active_dir = xdg_dir
        .as_ref()
        .filter(|dir| known_db_paths(dir).iter().any(|path| path.is_file()))
        .unwrap_or(&default_dir);

    let mut paths = Vec::new();
    if let Some(value) = std::env::var_os("OPENCODE_DB").filter(|value| !value.is_empty()) {
        let configured = PathBuf::from(value);
        if configured != Path::new(":memory:") {
            let configured = if configured.is_absolute() {
                configured
            } else {
                active_dir.join(configured)
            };
            if configured.is_file() {
                push_unique(&mut paths, configured);
            }
        }
    }
    for path in known_db_paths(active_dir) {
        push_unique(&mut paths, path);
    }
    if active_dir != &default_dir {
        for path in known_db_paths(&default_dir)
            .into_iter()
            .filter(|path| path.is_file())
        {
            push_unique(&mut paths, path);
        }
    }
    paths
}

fn custom_db_paths(dir: PathBuf) -> Vec<PathBuf> {
    if dir.is_file() {
        return vec![dir];
    }
    let nested = dir.join("opencode");
    let db_dir = if nested.is_dir() || known_db_paths(&nested).iter().any(|path| path.is_file()) {
        nested
    } else {
        dir
    };
    known_db_paths(&db_dir)
}

fn strip_virtual_db_path(path: &str) -> &str {
    path.split_once('#').map(|(db, _)| db).unwrap_or(path)
}

impl OcDb {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            rows_cache: MtimeCache::new(),
        }
    }
}

impl OpencodeAdapter {
    pub fn new() -> Self {
        Self {
            dbs: default_db_paths().into_iter().map(OcDb::new).collect(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_db(db: PathBuf) -> Self {
        Self {
            dbs: vec![OcDb::new(db)],
        }
    }

    #[cfg(test)]
    pub(crate) fn with_dbs(dbs: Vec<PathBuf>) -> Self {
        Self {
            dbs: dbs.into_iter().map(OcDb::new).collect(),
        }
    }

    fn open(db: &Path) -> Option<SqliteRo> {
        open_sqlite_ro(db, "opencode")
    }

    fn rows(db: &OcDb) -> Option<Vec<OcRow>> {
        let mtime = std::fs::metadata(&db.path)
            .map(|meta| mtime_ms(&meta))
            .unwrap_or(0);
        db.rows_cache.get_or_try_build(mtime, || {
            let database = Self::open(&db.path)?;
            query_rows(&database.conn, None).ok()
        })
    }

    fn db_for_ref(&self, reference: &SessionFileRef) -> Option<&OcDb> {
        let db_path = Path::new(strip_virtual_db_path(&reference.file_path));
        self.dbs.iter().find(|db| db.path == db_path)
    }

    fn build_meta(
        &self,
        reference: &SessionFileRef,
        row: &OcRow,
        db_path: &Path,
        message_count: i64,
    ) -> SessionMeta {
        let title = clean_title_candidate(&row.title);
        let model = serde_json::from_str::<Value>(&row.model_json)
            .ok()
            .and_then(|model| model.get("id").and_then(Value::as_str).map(str::to_string));
        SessionMeta {
            key: format!("opencode:{}", row.id),
            id: row.id.clone(),
            agent: AgentId::Opencode,
            title: if title.is_empty() {
                UNTITLED.to_string()
            } else {
                title
            },
            project_path: row.directory.clone(),
            project_name: project_name_of(&row.directory),
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
            git_branch: None,
            model,
            tokens_used: (row.tokens > 0).then_some(row.tokens),
            archived: row.archived,
            source: row.source(db_path),
        }
    }

    fn parse(
        &self,
        reference: &SessionFileRef,
    ) -> Result<(SessionMeta, Vec<TranscriptMessage>, u32)> {
        let db = self
            .db_for_ref(reference)
            .ok_or_else(|| anyhow!("opencode database is outside adapter roots"))?;
        let database = Self::open(&db.path).ok_or_else(|| anyhow!("cannot open opencode db"))?;
        let row = query_rows(&database.conn, Some(&reference.native_id))?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("opencode session {} not in db", reference.native_id))?;
        let (messages, unknown) = match row.v2_messages {
            true => parse_v2_messages(&database, &reference.native_id)?,
            false => parse_v1_messages(&database, &reference.native_id)?,
        };
        let count = messages
            .iter()
            .filter(|message| message.kind == MessageKind::Text)
            .count() as i64;
        Ok((
            self.build_meta(reference, &row, &db.path, count),
            messages,
            unknown,
        ))
    }
}

/// v1 body: message (role/time) + part (content block)
fn parse_v1_messages(
    database: &SqliteRo,
    session_id: &str,
) -> Result<(Vec<TranscriptMessage>, u32)> {
    let mut parts_by_message: HashMap<String, Vec<Value>> = HashMap::new();
    {
        let mut statement = database.conn.prepare(
            "SELECT message_id, data FROM part WHERE session_id = ?1 ORDER BY message_id, id",
        )?;
        let rows = statement.query_map([session_id], |part| {
            Ok((part.get::<_, String>(0)?, part.get::<_, String>(1)?))
        })?;
        for (message_id, data) in rows.flatten() {
            if let Ok(value) = serde_json::from_str::<Value>(&data) {
                parts_by_message.entry(message_id).or_default().push(value);
            }
        }
    }

    let mut messages = Vec::new();
    let mut unknown = 0;
    let mut statement = database
        .conn
        .prepare("SELECT id, data FROM message WHERE session_id = ?1 ORDER BY time_created, id")?;
    let rows = statement.query_map([session_id], |message| {
        Ok((message.get::<_, String>(0)?, message.get::<_, String>(1)?))
    })?;
    for (message_id, data) in rows.flatten() {
        let metadata: Value = match serde_json::from_str(&data) {
            Ok(metadata) => metadata,
            Err(_) => {
                unknown += 1;
                continue;
            }
        };
        let role = match metadata.get("role").and_then(Value::as_str) {
            Some("user") => Role::User,
            Some("assistant") => Role::Assistant,
            _ => Role::System,
        };
        let timestamp = metadata
            .get("time")
            .and_then(|time| time.get("created"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let mut blocks = BlockAccumulator::default();
        for part in parts_by_message.remove(&message_id).unwrap_or_default() {
            match part.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let text = part.get("text").and_then(Value::as_str).unwrap_or("");
                    if text.trim().is_empty() {
                        continue;
                    }
                    if part.get("synthetic").and_then(Value::as_bool) == Some(true) {
                        blocks.synthetic.push(text.trim().to_string());
                    } else {
                        blocks.text.push(text.trim().to_string());
                    }
                }
                Some("reasoning") => blocks.push_reasoning(&part),
                Some("tool") => blocks.push_tool(&part),
                Some("step-start") | Some("step-finish") | Some("snapshot") | Some("patch")
                | Some("file") => {}
                _ => unknown += 1,
            }
        }
        if let Some(message) = blocks.into_message(role, timestamp, None) {
            messages.push(message);
        }
    }

    assign_seq(&mut messages);
    Ok((messages, unknown))
}

/// v2 body: single session_message table with a type column distinguishing
/// user/synthetic/assistant and so on.
fn parse_v2_messages(
    database: &SqliteRo,
    session_id: &str,
) -> Result<(Vec<TranscriptMessage>, u32)> {
    let mut messages = Vec::new();
    let mut unknown = 0;
    let mut statement = database
        .conn
        .prepare("SELECT type, data FROM session_message WHERE session_id = ?1 ORDER BY seq")?;
    let rows = statement.query_map([session_id], |message| {
        Ok((message.get::<_, String>(0)?, message.get::<_, String>(1)?))
    })?;
    for (message_type, data) in rows.flatten() {
        let metadata: Value = match serde_json::from_str(&data) {
            Ok(metadata) => metadata,
            Err(_) => {
                unknown += 1;
                continue;
            }
        };
        let timestamp = metadata
            .get("time")
            .and_then(|time| time.get("created"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        match message_type.as_str() {
            "user" => {
                let text = metadata
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if !text.is_empty() {
                    messages.push(text_msg(Role::User, text, timestamp));
                }
            }
            "synthetic" => {
                let text = metadata
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if !text.is_empty() {
                    let mut message = text_msg(Role::User, text, timestamp);
                    message.kind = MessageKind::Meta;
                    messages.push(message);
                }
            }
            "system" => {
                let text = metadata
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if !text.is_empty() {
                    let mut message = text_msg(Role::System, text, timestamp);
                    message.kind = MessageKind::Meta;
                    messages.push(message);
                }
            }
            "shell" => {
                let command = metadata
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let input = serde_json::json!({ "command": command });
                let output = metadata
                    .get("output")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .map(str::to_string);
                let mut blocks = BlockAccumulator::default();
                blocks.tools.push(tool_call_view(
                    metadata
                        .get("callID")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    "shell",
                    &input,
                    output,
                    false,
                ));
                if let Some(message) = blocks.into_message(Role::Assistant, timestamp, None) {
                    messages.push(message);
                }
            }
            "assistant" => {
                let model = metadata
                    .pointer("/model/id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let mut blocks = BlockAccumulator::default();
                for block in metadata
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                if !text.trim().is_empty() {
                                    blocks.text.push(text.trim().to_string());
                                }
                            }
                        }
                        Some("reasoning") => blocks.push_reasoning(block),
                        Some("tool") => blocks.push_tool(block),
                        Some("step-start") | Some("step-finish") | Some("snapshot")
                        | Some("patch") | Some("file") => {}
                        _ => unknown += 1,
                    }
                }
                if let Some(message) = blocks.into_message(Role::Assistant, timestamp, model) {
                    messages.push(message);
                }
            }
            "compaction" => {
                let summary = metadata
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                if !summary.is_empty() {
                    let mut message = text_msg(Role::System, summary, timestamp);
                    message.kind = MessageKind::CompactSummary;
                    messages.push(message);
                }
            }
            "agent-switched" | "model-switched" => {}
            _ => unknown += 1,
        }
    }

    assign_seq(&mut messages);
    Ok((messages, unknown))
}

#[derive(Default)]
struct BlockAccumulator {
    text: Vec<String>,
    synthetic: Vec<String>,
    thinking: Vec<String>,
    tools: Vec<ToolCallView>,
}

impl BlockAccumulator {
    fn push_reasoning(&mut self, block: &Value) {
        if let Some(text) = block.get("text").and_then(Value::as_str) {
            if !text.trim().is_empty() {
                self.thinking.push(text.trim().to_string());
            }
        }
    }

    fn push_tool(&mut self, block: &Value) {
        let state = block.get("state").cloned().unwrap_or(Value::Null);
        let input = state.get("input").cloned().unwrap_or(Value::Null);
        let output = opencode_tool_output(&state);
        self.tools.push(tool_call_view(
            block
                .get("callID")
                .or_else(|| block.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            block
                .get("tool")
                .or_else(|| block.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("tool"),
            &input,
            output,
            state.get("status").and_then(Value::as_str) == Some("error"),
        ));
    }

    fn into_message(
        self,
        role: Role,
        timestamp: i64,
        model: Option<String>,
    ) -> Option<TranscriptMessage> {
        let (text, kind) = if self.text.is_empty() && !self.synthetic.is_empty() {
            (self.synthetic.join("\n\n"), MessageKind::Meta)
        } else {
            (self.text.join("\n\n"), MessageKind::Text)
        };
        if text.is_empty() && self.thinking.is_empty() && self.tools.is_empty() {
            return None;
        }
        let (text, truncated) = clip(&text, MAX_MSG_TEXT);
        Some(TranscriptMessage {
            seq: 0,
            role,
            kind,
            text,
            truncated,
            tool_calls: self.tools,
            thinking: (!self.thinking.is_empty())
                .then(|| clip(&self.thinking.join("\n\n"), MAX_TOOL_IO).0),
            timestamp: (timestamp > 0).then_some(timestamp),
            model,
        })
    }
}

fn opencode_tool_output(state: &Value) -> Option<String> {
    if let Some(output) = state.get("output") {
        return match output {
            Value::String(text) if !text.is_empty() => Some(text.clone()),
            value if !value.is_null() => serde_json::to_string(value).ok(),
            _ => None,
        };
    }
    if let Some(content) = state.get("content").and_then(Value::as_array) {
        let rendered = content
            .iter()
            .filter_map(|item| match item.get("type").and_then(Value::as_str) {
                Some("text") => item.get("text").and_then(Value::as_str).map(str::to_string),
                Some("file") => item
                    .get("name")
                    .or_else(|| item.get("uri"))
                    .and_then(Value::as_str)
                    .map(|name| format!("[file: {name}]")),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !rendered.is_empty() {
            return Some(rendered);
        }
    }
    if let Some(result) = state.get("result").filter(|value| !value.is_null()) {
        return match result {
            Value::String(text) => Some(text.clone()),
            value => serde_json::to_string(value).ok(),
        };
    }
    state
        .pointer("/error/message")
        .and_then(Value::as_str)
        .filter(|message| !message.is_empty())
        .map(str::to_string)
}

#[derive(Clone)]
struct OcRow {
    id: String,
    directory: String,
    title: String,
    created_ms: i64,
    updated_ms: i64,
    model_json: String,
    tokens: i64,
    archived: bool,
    version: String,
    content_len: i64,
    v2_messages: bool,
}

impl OcRow {
    fn source(&self, db_path: &Path) -> Option<String> {
        let next_db =
            db_path.file_name().and_then(|name| name.to_str()) == Some("opencode-next.db");
        let is_v2 = next_db
            || self.version.starts_with('2')
            || self.version.contains("beta")
            || self.version.contains("next");
        is_v2.then(|| "opencode2".to_string())
    }
}

impl Default for OpencodeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentHistoryAdapter for OpencodeAdapter {
    fn agent(&self) -> AgentId {
        AgentId::Opencode
    }

    fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
        let mut references = Vec::new();
        for db in &self.dbs {
            if !db.path.is_file() {
                continue;
            }
            let rows = Self::rows(db)
                .ok_or_else(|| anyhow!("session store unreadable: {}", db.path.display()))?;
            references.extend(
                rows.into_iter()
                    .filter(|row| row.content_len > 0)
                    .map(|row| SessionFileRef {
                        agent: AgentId::Opencode,
                        native_id: row.id.clone(),
                        file_path: virtual_path(&db.path, &row.id),
                        mtime_ms: if row.updated_ms > 0 {
                            row.updated_ms
                        } else {
                            std::fs::metadata(&db.path)
                                .map(|meta| mtime_ms(&meta))
                                .unwrap_or(0)
                        },
                        size: row.content_len,
                    }),
            );
        }
        Ok(references)
    }

    fn quick_meta(&self, references: &[SessionFileRef]) -> Option<HashMap<String, SessionMeta>> {
        let mut out = HashMap::new();
        let mut opened = false;
        for db in &self.dbs {
            if !db.path.is_file() {
                continue;
            }
            let rows = Self::rows(db)?;
            opened = true;
            let by_id: HashMap<&str, &OcRow> =
                rows.iter().map(|row| (row.id.as_str(), row)).collect();
            for reference in references.iter().filter(|reference| {
                Path::new(strip_virtual_db_path(&reference.file_path)) == db.path.as_path()
            }) {
                if let Some(row) = by_id.get(reference.native_id.as_str()) {
                    out.insert(
                        reference.file_path.clone(),
                        self.build_meta(reference, row, &db.path, 0),
                    );
                }
            }
        }
        opened.then_some(out)
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession> {
        let (meta, messages, unknown) = self.parse(reference)?;
        Ok(ParsedSession {
            meta,
            units: units_from_messages(&messages),
            unknown_line_count: unknown,
        })
    }

    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript> {
        let (meta, messages, unknown) = self.parse(reference)?;
        Ok(ParsedTranscript::simple(meta, messages, unknown))
    }

    fn with_custom_root(&self, dir: PathBuf) -> Box<dyn AgentHistoryAdapter> {
        Box::new(Self {
            dbs: custom_db_paths(dir).into_iter().map(OcDb::new).collect(),
        })
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        self.dbs.iter().map(|db| db.path.clone()).collect()
    }

    fn supports_individual_root_removal(&self) -> bool {
        true
    }

    fn excluding_data_roots(&self, roots: &[PathBuf]) -> Option<Box<dyn AgentHistoryAdapter>> {
        Some(Box::new(Self {
            dbs: self
                .dbs
                .iter()
                .filter(|db| !roots.contains(&db.path))
                .map(|db| OcDb::new(db.path.clone()))
                .collect(),
        }))
    }
}
