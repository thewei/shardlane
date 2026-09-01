use super::antigravity::AntigravityAdapter;
use super::claude::ClaudeAdapter;
use super::codex::CodexAdapter;
use super::command_code::CommandCodeAdapter;
use super::copilot::CopilotAdapter;
use super::cursor::CursorAdapter;
use super::dsh::DshAdapter;
use super::gemini::GeminiAdapter;
use super::grok::GrokAdapter;
use super::kimi::KimiAdapter;
use super::kiro::KiroAdapter;
use super::opencode::OpencodeAdapter;
use super::pi::PiAdapter;
use super::{adapter_ix_for, path_owns, AgentHistoryAdapter};
use crate::models::{AgentId, MessageKind, Role};
use anyhow::Result;
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn claude_adapter_parses_mainline_and_tool_output() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let project_dir = temp.path().join("project-a");
    fs::create_dir_all(&project_dir)?;
    let session = project_dir.join("claude-session.jsonl");
    let jsonl = concat!(
        r#"{"type":"user","cwd":"/work/demo","gitBranch":"main","timestamp":"2026-08-01T01:00:00Z","message":{"content":"Fix the parser"}}"#,
        "\n",
        r#"{"type":"assistant","cwd":"/work/demo","timestamp":"2026-08-01T01:00:01Z","message":{"id":"m1","model":"claude-sonnet-4-5","usage":{"input_tokens":10,"output_tokens":5},"content":[{"type":"thinking","thinking":"Inspect first"},{"type":"tool_use","id":"tool-1","name":"Read","input":{"file_path":"src/lib.rs"}},{"type":"text","text":"I found it."}]}}"#,
        "\n",
        r#"{"type":"user","cwd":"/work/demo","timestamp":"2026-08-01T01:00:02Z","message":{"content":[{"type":"tool_result","tool_use_id":"tool-1","content":"source text"}]}}"#,
        "\n",
    );
    fs::write(&session, jsonl)?;

    let adapter = ClaudeAdapter::with_root(temp.path().to_path_buf());
    let refs = adapter.list_session_files()?;
    assert_eq!(refs.len(), 1);
    let parsed = adapter.parse_session(&refs[0])?;
    assert_eq!(parsed.meta.agent, AgentId::ClaudeCode);
    assert_eq!(parsed.meta.title, "Fix the parser");
    assert_eq!(parsed.meta.project_path, "/work/demo");
    assert_eq!(parsed.meta.git_branch.as_deref(), Some("main"));
    assert_eq!(parsed.meta.model.as_deref(), Some("claude-sonnet-4-5"));
    assert_eq!(parsed.meta.tokens_used, Some(15));
    assert_eq!(parsed.meta.message_count, 2);
    assert_eq!(parsed.units.len(), 2);

    let transcript = adapter.parse_transcript(&refs[0])?;
    assert_eq!(transcript.mainline.len(), 2);
    assert_eq!(transcript.mainline[0].role, Role::User);
    assert_eq!(transcript.mainline[1].role, Role::Assistant);
    assert_eq!(
        transcript.mainline[1].thinking.as_deref(),
        Some("Inspect first")
    );
    assert_eq!(transcript.mainline[1].tool_calls.len(), 1);
    assert_eq!(transcript.mainline[1].tool_calls[0].name, "Read");
    assert_eq!(
        transcript.mainline[1].tool_calls[0].output.as_deref(),
        Some("source text")
    );
    Ok(())
}

#[test]
fn codex_adapter_parses_rollout_and_read_only_state_meta() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let sessions_dir = temp.path().join("sessions/2026/08/02");
    fs::create_dir_all(&sessions_dir)?;
    let session_id = "22222222-aaaa-bbbb-cccc-000000000002";
    let rollout = sessions_dir.join(format!("rollout-2026-08-02T09-15-00-{session_id}.jsonl"));
    let rollout_text = concat!(
        r#"{"timestamp":"2026-08-02T09:15:00Z","type":"session_meta","payload":{"cwd":"/work/codex-demo","originator":"codex_cli_rs","git":{"branch":"feature/history"}}}"#,
        "\n",
        r#"{"timestamp":"2026-08-02T09:15:01Z","type":"turn_context","payload":{"model":"gpt-5"}}"#,
        "\n",
        r#"{"timestamp":"2026-08-02T09:15:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Build history"}]}}"#,
        "\n",
        r#"{"timestamp":"2026-08-02T09:15:03Z","type":"response_item","payload":{"type":"function_call","call_id":"call-1","name":"shell","arguments":"{\"command\":\"cargo test\"}"}}"#,
        "\n",
        r#"{"timestamp":"2026-08-02T09:15:04Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"ok"}}"#,
        "\n",
        r#"{"timestamp":"2026-08-02T09:15:05Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Implemented."}]}}"#,
        "\n",
        r#"{"timestamp":"2026-08-02T09:15:06Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":42}}}}"#,
        "\n",
    );
    fs::write(&rollout, rollout_text)?;
    build_codex_state_db(
        temp.path().join("state_5.sqlite").as_path(),
        &rollout,
        session_id,
    )?;

    let adapter = CodexAdapter::with_root(temp.path().to_path_buf());
    let refs = adapter.list_session_files()?;
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].native_id, session_id);

    let parsed = adapter.parse_session(&refs[0])?;
    assert_eq!(parsed.meta.title, "Build history");
    assert_eq!(parsed.meta.project_path, "/work/codex-demo");
    assert_eq!(parsed.meta.model.as_deref(), Some("gpt-5"));
    assert_eq!(parsed.meta.tokens_used, Some(42));
    assert_eq!(parsed.meta.source.as_deref(), Some("CLI"));
    assert_eq!(parsed.units.len(), 3);
    assert!(parsed
        .units
        .iter()
        .any(|unit| unit.text.contains("shell cargo test")));

    let transcript = adapter.parse_transcript(&refs[0])?;
    assert!(transcript
        .mainline
        .iter()
        .any(|message| message.kind == MessageKind::Text && message.text == "Implemented."));
    let tool = transcript
        .mainline
        .iter()
        .flat_map(|message| message.tool_calls.iter())
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing codex tool call"))?;
    assert_eq!(tool.input_preview, "cargo test");
    assert_eq!(tool.output.as_deref(), Some("ok"));

    let quick = adapter
        .quick_meta(&refs)
        .ok_or_else(|| anyhow::anyhow!("missing quick meta"))?;
    let quick_meta = quick
        .get(&refs[0].file_path)
        .ok_or_else(|| anyhow::anyhow!("missing quick meta row"))?;
    assert_eq!(quick_meta.title, "Pinned thread title");
    assert_eq!(quick_meta.tokens_used, Some(99));

    let merged = adapter.merge_quick_meta(parsed.meta, quick_meta);
    assert_eq!(merged.title, "Pinned thread title");
    assert_eq!(merged.tokens_used, Some(42));
    assert_eq!(merged.git_branch.as_deref(), Some("feature/history"));
    Ok(())
}

#[test]
fn kiro_adapter_reads_sidecar_and_chat_jsonl() -> Result<()> {
    let temp = tempfile::tempdir()?;
    fs::write(
        temp.path().join("kiro-1.jsonl"),
        concat!(
            r#"{"kind":"Prompt","data":{"content":[{"kind":"text","data":"Kiro question"}],"meta":{"timestamp":1754100000}}}"#,
            "\n",
            r#"{"kind":"AssistantMessage","data":{"content":[{"kind":"text","data":"Kiro answer"}],"meta":{"timestamp":1754100001}}}"#,
            "\n",
        ),
    )?;
    fs::write(
        temp.path().join("kiro-1.json"),
        r#"{"cwd":"/work/kiro","title":"Kiro title","created_at":"2026-08-02T00:00:00Z","updated_at":"2026-08-02T00:00:01Z"}"#,
    )?;
    let adapter = KiroAdapter::with_root(temp.path().to_path_buf());
    let refs = adapter.list_session_files()?;
    assert_eq!(refs.len(), 1);
    let parsed = adapter.parse_session(&refs[0])?;
    assert_eq!(parsed.meta.agent, AgentId::Kiro);
    assert_eq!(parsed.meta.title, "Kiro title");
    assert_eq!(parsed.meta.project_path, "/work/kiro");
    assert_eq!(parsed.units.len(), 2);
    Ok(())
}

#[test]
fn gemini_adapter_replays_latest_snapshot_and_maps_project_slug() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("tmp");
    let chats = root.join("demo-slug/chats");
    fs::create_dir_all(&chats)?;
    let session = chats.join("session-demo.jsonl");
    fs::write(
        &session,
        concat!(
            r#"{"sessionId":"gemini-native","startTime":"2026-08-03T00:00:00Z","lastUpdated":"2026-08-03T00:00:02Z"}"#,
            "\n",
            r#"{"$set":{"messages":[{"type":"user","timestamp":"2026-08-03T00:00:01Z","content":[{"text":"Old question"}]}]}}"#,
            "\n",
            r#"{"$set":{"messages":[{"type":"user","timestamp":"2026-08-03T00:00:01Z","content":[{"text":"Gemini question"}]},{"type":"model","timestamp":"2026-08-03T00:00:02Z","content":[{"text":"Gemini answer"}]}]}}"#,
            "\n",
        ),
    )?;
    let projects = temp.path().join("projects.json");
    fs::write(&projects, r#"{"projects":{"/work/gemini":"demo-slug"}}"#)?;
    let adapter = GeminiAdapter::with_root(root, projects);
    let refs = adapter.list_session_files()?;
    assert_eq!(refs.len(), 1);
    let parsed = adapter.parse_session(&refs[0])?;
    assert_eq!(parsed.meta.id, "gemini-native");
    assert_eq!(parsed.meta.title, "Gemini question");
    assert_eq!(parsed.meta.project_path, "/work/gemini");
    assert_eq!(parsed.units.len(), 2);
    Ok(())
}

#[test]
fn cursor_adapter_extracts_user_query_and_tool_call() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let session_dir = temp
        .path()
        .join("Users-demo-project/agent-transcripts/cursor-native");
    fs::create_dir_all(&session_dir)?;
    fs::write(
        session_dir.join("cursor-native.jsonl"),
        concat!(
            r#"{"role":"user","message":{"content":[{"type":"text","text":"<timestamp>Thursday, Jul 23, 2026, 4:00 PM (UTC+8)</timestamp><user_query>Cursor question</user_query>"}]}}"#,
            "\n",
            r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"path":"src/main.rs"}},{"type":"text","text":"Cursor answer"}]}}"#,
            "\n",
            r#"{"type":"turn_ended"}"#,
            "\n",
        ),
    )?;
    let adapter = CursorAdapter::with_root(temp.path().to_path_buf());
    let refs = adapter.list_session_files()?;
    assert_eq!(refs.len(), 1);
    let transcript = adapter.parse_transcript(&refs[0])?;
    assert_eq!(transcript.meta.title, "Cursor question");
    assert_eq!(transcript.mainline.len(), 2);
    assert_eq!(transcript.mainline[1].tool_calls[0].name, "Read");
    Ok(())
}

#[test]
fn copilot_adapter_reads_sqlite_store_without_writing_source() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let db = temp.path().join("session-store.db");
    let conn = Connection::open(&db)?;
    conn.execute_batch(
        "CREATE TABLE sessions (
            id TEXT PRIMARY KEY, cwd TEXT, branch TEXT, summary TEXT,
            created_at TEXT, updated_at TEXT
         );
         CREATE TABLE turns (
            id INTEGER PRIMARY KEY, session_id TEXT, turn_index INTEGER,
            user_message TEXT, assistant_response TEXT, timestamp TEXT
         );",
    )?;
    conn.execute(
        "INSERT INTO sessions VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            "copilot-native",
            "/work/copilot",
            "main",
            "Copilot title",
            "2026-08-04T00:00:00Z",
            "2026-08-04T00:00:01Z"
        ],
    )?;
    conn.execute(
        "INSERT INTO turns (session_id, turn_index, user_message, assistant_response, timestamp)
         VALUES (?1, 0, ?2, ?3, ?4)",
        rusqlite::params![
            "copilot-native",
            "Copilot question",
            "Copilot answer",
            "2026-08-04T00:00:01Z"
        ],
    )?;
    drop(conn);
    let adapter = CopilotAdapter::with_db(db.clone());
    let refs = adapter.list_session_files()?;
    assert_eq!(refs.len(), 1);
    let parsed = adapter.parse_session(&refs[0])?;
    assert_eq!(parsed.meta.title, "Copilot title");
    assert_eq!(parsed.meta.project_path, "/work/copilot");
    assert_eq!(parsed.units.len(), 2);
    assert!(Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).is_ok());
    Ok(())
}

#[test]
fn opencode_adapter_reads_normalized_message_parts() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let db = temp.path().join("opencode.db");
    let conn = Connection::open(&db)?;
    conn.execute_batch(
        "CREATE TABLE session (
            id TEXT PRIMARY KEY, directory TEXT, title TEXT, time_created INTEGER,
            time_updated INTEGER, model TEXT, tokens_input INTEGER, tokens_output INTEGER,
            tokens_reasoning INTEGER, time_archived INTEGER, parent_id TEXT
         );
         CREATE TABLE message (
            id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT
         );
         CREATE TABLE part (
            id TEXT PRIMARY KEY, session_id TEXT, message_id TEXT, data TEXT
         );",
    )?;
    conn.execute(
        "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, 10, 20, 5, NULL, NULL)",
        rusqlite::params![
            "opencode-native",
            "/work/opencode",
            "OpenCode title",
            1_754_300_000_000_i64,
            1_754_300_001_000_i64,
            r#"{"id":"gpt-5"}"#
        ],
    )?;
    conn.execute(
        "INSERT INTO message VALUES ('m1', ?1, 1, ?2), ('m2', ?1, 2, ?3)",
        rusqlite::params![
            "opencode-native",
            r#"{"role":"user","time":{"created":1754300000000}}"#,
            r#"{"role":"assistant","time":{"created":1754300001000}}"#
        ],
    )?;
    conn.execute(
        "INSERT INTO part VALUES
            ('p1', ?1, 'm1', ?2),
            ('p2', ?1, 'm2', ?3),
            ('p3', ?1, 'm2', ?4)",
        rusqlite::params![
            "opencode-native",
            r#"{"type":"text","text":"OpenCode question"}"#,
            r#"{"type":"reasoning","text":"Thinking"}"#,
            r#"{"type":"tool","callID":"call-1","tool":"bash","state":{"input":{"command":"cargo test"},"output":"ok","status":"completed"}}"#
        ],
    )?;
    drop(conn);
    let adapter = OpencodeAdapter::with_db(db);
    let refs = adapter.list_session_files()?;
    assert_eq!(refs.len(), 1);
    let parsed = adapter.parse_transcript(&refs[0])?;
    assert_eq!(parsed.meta.title, "OpenCode title");
    assert_eq!(parsed.meta.model.as_deref(), Some("gpt-5"));
    assert_eq!(parsed.meta.tokens_used, Some(35));
    assert_eq!(parsed.meta.source, None);
    assert_eq!(parsed.mainline.len(), 2);
    assert_eq!(parsed.mainline[1].thinking.as_deref(), Some("Thinking"));
    assert_eq!(
        parsed.mainline[1].tool_calls[0].output.as_deref(),
        Some("ok")
    );
    Ok(())
}

#[test]
fn opencode_adapter_parses_next_channel_session_messages() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let db = temp.path().join("opencode-next.db");
    let conn = Connection::open(&db)?;
    conn.execute_batch(
        "CREATE TABLE session (
            id TEXT PRIMARY KEY, directory TEXT, title TEXT, time_created INTEGER,
            time_updated INTEGER, model TEXT, tokens_input INTEGER, tokens_output INTEGER,
            tokens_reasoning INTEGER, time_archived INTEGER, parent_id TEXT, version TEXT
         );
         CREATE TABLE session_message (
            id TEXT PRIMARY KEY, session_id TEXT, seq INTEGER, type TEXT, data TEXT
         );",
    )?;
    conn.execute(
        "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, 7, 8, 9, NULL, NULL, ?7)",
        rusqlite::params![
            "opencode2-native",
            "/work/opencode2",
            "OpenCode2 title",
            1_754_300_010_000_i64,
            1_754_300_011_000_i64,
            r#"{"id":"gpt-5.5"}"#,
            "0.0.0-beta-17639"
        ],
    )?;
    conn.execute(
        "INSERT INTO session_message VALUES
            ('sm1', ?1, 0, 'user', ?2),
            ('sm2', ?1, 1, 'assistant', ?3)",
        rusqlite::params![
            "opencode2-native",
            r#"{"text":"OpenCode2 question","time":{"created":1754300010000}}"#,
            r#"{"time":{"created":1754300011000},"model":{"id":"gpt-5.5"},"content":[{"type":"text","text":"Handled in next channel"},{"type":"reasoning","text":"Inspect db schema"},{"type":"tool","id":"call-2","name":"bash","state":{"input":{"command":"pwd"},"result":"/work/opencode2"}}]}"#
        ],
    )?;
    drop(conn);

    let adapter = OpencodeAdapter::with_db(db);
    let refs = adapter.list_session_files()?;
    assert_eq!(refs.len(), 1);
    let parsed = adapter.parse_transcript(&refs[0])?;
    assert_eq!(parsed.meta.title, "OpenCode2 title");
    assert_eq!(parsed.meta.project_path, "/work/opencode2");
    assert_eq!(parsed.meta.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(parsed.meta.tokens_used, Some(24));
    assert_eq!(parsed.meta.source.as_deref(), Some("opencode2"));
    assert_eq!(parsed.mainline.len(), 2);
    assert_eq!(parsed.mainline[0].text, "OpenCode2 question");
    assert_eq!(
        parsed.mainline[1].thinking.as_deref(),
        Some("Inspect db schema")
    );
    assert_eq!(
        parsed.mainline[1].tool_calls[0].output.as_deref(),
        Some("/work/opencode2")
    );
    Ok(())
}

#[test]
fn opencode_adapter_scans_stable_and_next_databases_together() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let stable_db = temp.path().join("opencode.db");
    let next_db = temp.path().join("opencode-next.db");

    let stable = Connection::open(&stable_db)?;
    stable.execute_batch(
        "CREATE TABLE session (
            id TEXT PRIMARY KEY, directory TEXT, title TEXT, time_created INTEGER,
            time_updated INTEGER, model TEXT, tokens_input INTEGER, tokens_output INTEGER,
            tokens_reasoning INTEGER, time_archived INTEGER, parent_id TEXT
         );
         CREATE TABLE message (
            id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT
         );
         CREATE TABLE part (
            id TEXT PRIMARY KEY, session_id TEXT, message_id TEXT, data TEXT
         );",
    )?;
    stable.execute(
        "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 2, 3, NULL, NULL)",
        rusqlite::params![
            "stable-native",
            "/work/stable",
            "Stable title",
            1_754_300_020_000_i64,
            1_754_300_021_000_i64,
            r#"{"id":"gpt-5"}"#
        ],
    )?;
    stable.execute(
        "INSERT INTO message VALUES ('m1', ?1, 1, ?2)",
        rusqlite::params![
            "stable-native",
            r#"{"role":"user","time":{"created":1754300020000}}"#
        ],
    )?;
    stable.execute(
        "INSERT INTO part VALUES ('p1', ?1, 'm1', ?2)",
        rusqlite::params![
            "stable-native",
            r#"{"type":"text","text":"stable message"}"#
        ],
    )?;
    drop(stable);

    let next = Connection::open(&next_db)?;
    next.execute_batch(
        "CREATE TABLE session (
            id TEXT PRIMARY KEY, directory TEXT, title TEXT, time_created INTEGER,
            time_updated INTEGER, model TEXT, tokens_input INTEGER, tokens_output INTEGER,
            tokens_reasoning INTEGER, time_archived INTEGER, parent_id TEXT, version TEXT
         );
         CREATE TABLE session_message (
            id TEXT PRIMARY KEY, session_id TEXT, seq INTEGER, type TEXT, data TEXT
         );",
    )?;
    next.execute(
        "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 1, 1, NULL, NULL, ?7)",
        rusqlite::params![
            "next-native",
            "/work/next",
            "Next title",
            1_754_300_030_000_i64,
            1_754_300_031_000_i64,
            r#"{"id":"gpt-5.5"}"#,
            "2.0.0"
        ],
    )?;
    next.execute(
        "INSERT INTO session_message VALUES ('sm1', ?1, 0, 'user', ?2)",
        rusqlite::params![
            "next-native",
            r#"{"text":"next message","time":{"created":1754300030000}}"#
        ],
    )?;
    drop(next);

    let adapter = OpencodeAdapter::with_dbs(vec![stable_db.clone(), next_db.clone()]);
    let refs = adapter.list_session_files()?;
    assert_eq!(refs.len(), 2);
    assert!(refs.iter().any(|reference| reference
        .file_path
        .starts_with(&format!("{}#", stable_db.display()))));
    assert!(refs.iter().any(|reference| reference
        .file_path
        .starts_with(&format!("{}#", next_db.display()))));

    let stable_ref = refs
        .iter()
        .find(|reference| reference.native_id == "stable-native")
        .ok_or_else(|| anyhow::anyhow!("missing stable opencode reference"))?;
    let next_ref = refs
        .iter()
        .find(|reference| reference.native_id == "next-native")
        .ok_or_else(|| anyhow::anyhow!("missing next opencode reference"))?;
    assert_eq!(adapter.parse_session(stable_ref)?.meta.source, None);
    assert_eq!(
        adapter.parse_session(next_ref)?.meta.source.as_deref(),
        Some("opencode2")
    );
    Ok(())
}

#[test]
fn command_code_adapter_parses_main_session_and_ignores_checkpoints() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let project_dir = temp.path().join("project-demo");
    fs::create_dir_all(&project_dir)?;
    let session = project_dir.join("cc-session.jsonl");
    fs::write(
        &session,
        concat!(
            r#"{"type":"session","version":3,"id":"cc-native","timestamp":"2026-08-26T00:00:00Z","cwd":"/work/command-code"}"#,
            "\n",
            r#"{"type":"message","id":"m1","timestamp":"2026-08-26T00:00:00Z","message":{"role":"user","content":[{"type":"text","text":"Command Code question"}]}}"#,
            "\n",
            r#"{"type":"message","id":"m2","timestamp":"2026-08-26T00:00:01Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"Plan first"},{"type":"text","text":"I will inspect files."},{"type":"tool_use","id":"tool-1","name":"shell_command","input":{"command":"pwd"}}]},"usage":{"inputTokens":10,"outputTokens":5},"model":"deepseek/deepseek-v4-flash"}"#,
            "\n",
            r#"{"type":"message","id":"m3","timestamp":"2026-08-26T00:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","content":[{"type":"text","text":"ok"}]}]}}"#,
            "\n",
            r#"{"type":"message","id":"m4","timestamp":"2026-08-26T00:00:03Z","message":{"role":"assistant","content":[{"type":"text","text":"Done."}]},"usage":{"inputTokens":20,"outputTokens":10},"model":"deepseek/deepseek-v4-flash"}"#,
            "\n",
        ),
    )?;
    fs::write(
        project_dir.join("cc-session.meta.json"),
        r#"{"title":"Command Code Session"}"#,
    )?;
    fs::write(
        project_dir.join("cc-session.checkpoints.jsonl"),
        r#"{"type":"session","id":"cc-checkpoint"}"#,
    )?;

    let adapter = CommandCodeAdapter::with_root(temp.path().to_path_buf());
    let refs = adapter.list_session_files()?;
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].native_id, "cc-native");
    assert!(refs[0].file_path.ends_with("cc-session.jsonl"));

    let parsed = adapter.parse_session(&refs[0])?;
    assert_eq!(parsed.meta.agent, AgentId::CommandCode);
    assert_eq!(parsed.meta.title, "Command Code Session");
    assert_eq!(parsed.meta.project_path, "/work/command-code");
    assert_eq!(
        parsed.meta.model.as_deref(),
        Some("deepseek/deepseek-v4-flash")
    );
    assert_eq!(parsed.meta.tokens_used, Some(30));
    assert_eq!(parsed.meta.source.as_deref(), Some("command-code"));
    assert_eq!(parsed.meta.message_count, 3);
    assert_eq!(parsed.units.len(), 3);

    let transcript = adapter.parse_transcript(&refs[0])?;
    assert_eq!(transcript.mainline.len(), 3);
    assert_eq!(transcript.mainline[1].tool_calls.len(), 1);
    assert_eq!(
        transcript.mainline[1].tool_calls[0].output.as_deref(),
        Some("ok")
    );
    assert_eq!(
        transcript.mainline[1].thinking.as_deref(),
        Some("Plan first")
    );
    Ok(())
}

fn build_codex_state_db(path: &Path, rollout: &Path, session_id: &str) -> Result<()> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            rollout_path TEXT NOT NULL,
            cwd TEXT NOT NULL,
            title TEXT NOT NULL,
            name TEXT,
            tokens_used INTEGER,
            archived INTEGER NOT NULL,
            git_branch TEXT,
            model TEXT,
            source TEXT,
            created_at_ms INTEGER,
            updated_at_ms INTEGER
        );",
    )?;
    conn.execute(
        "INSERT INTO threads (
            id, rollout_path, cwd, title, name, tokens_used, archived,
            git_branch, model, source, created_at_ms, updated_at_ms
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            session_id,
            rollout.to_string_lossy().to_string(),
            "/work/codex-demo",
            "DB fallback title",
            "Pinned thread title",
            99_i64,
            "feature/history",
            "gpt-5",
            "cli",
            1_754_100_900_000_i64,
            1_754_100_999_000_i64,
        ],
    )?;
    drop(conn);
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[test]
fn copilot_unreadable_store_reports_discovery_error() -> Result<()> {
    // db of garbage bytes: after copying, sqlite cannot open it → rows() is
    // None. Before the fix this returned Ok(empty set), which would trigger
    // the scanner to wrongly purge the entire Copilot index.
    let temp = tempfile::tempdir()?;
    let db = temp.path().join("session-store.db");
    fs::write(&db, b"this is not a sqlite database")?;
    let adapter = CopilotAdapter::with_db(db);
    assert!(adapter.list_session_files().is_err());
    Ok(())
}

#[test]
fn sqlite_copy_size_limit_boundaries() {
    let cap = 1024_u64;
    assert!(super::sqlite_ro::copy_within_limit(0, cap));
    assert!(super::sqlite_ro::copy_within_limit(1024, cap));
    assert!(!super::sqlite_ro::copy_within_limit(1025, cap));
}

#[test]
fn sqlite_temp_dirs_are_unique_per_open() -> Result<()> {
    // Two concurrent opens of the same locked database no longer collide on
    // one temp directory: the name carries an atomic sequence number. A
    // garbage-byte database forces both open_sqlite_ro calls down the copy
    // path.
    let temp = tempfile::tempdir()?;
    let db = temp.path().join("copilot-session-store.db");
    fs::write(&db, b"garbage")?;
    let first = super::sqlite_ro::open_sqlite_ro(&db, "test");
    let second = super::sqlite_ro::open_sqlite_ro(&db, "test");
    // A garbage database opens after copying (sqlite lazy-opens); both calls
    // should get connections and not interfere.
    assert!(first.is_some());
    assert!(second.is_some());
    drop(first);
    drop(second);
    Ok(())
}

// ---------------------------------------------------------------- pi / omp

const PI_SESSION_JSONL: &str = concat!(
    r#"{"type":"session","version":3,"id":"66666666-aaaa-bbbb-cccc-000000000006","timestamp":"2026-08-06T10:00:00.000Z","cwd":"/Users/tester/Github/demo"}"#,
    "\n",
    r#"{"type":"model_change","id":"m1","timestamp":"2026-08-06T10:00:01.000Z","provider":"openai-codex","modelId":"gpt-5.5"}"#,
    "\n",
    r#"{"type":"thinking_level_change","id":"t1","timestamp":"2026-08-06T10:00:01.100Z","thinkingLevel":"medium"}"#,
    "\n",
    r#"{"type":"message","id":"u1","timestamp":"2026-08-06T10:00:05.000Z","message":{"role":"user","content":[{"type":"text","text":"Pi check the useEffect() cleanup in the QR component"}]}}"#,
    "\n",
    r#"{"type":"message","id":"a1","timestamp":"2026-08-06T10:00:08.000Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"call_pi_1","name":"bash","arguments":{"command":"rg useEffect src/"}}],"model":"gpt-5.5","usage":{"input":100,"output":20,"cacheRead":0,"cacheWrite":0,"totalTokens":4242}}}"#,
    "\n",
    r#"{"type":"message","id":"r1","timestamp":"2026-08-06T10:00:09.000Z","message":{"role":"toolResult","toolCallId":"call_pi_1","toolName":"bash","content":[{"type":"text","text":"src/QrScanner.tsx: useEffect(() => watch())"}]}}"#,
    "\n",
    r#"{"type":"message","id":"a2","timestamp":"2026-08-06T10:00:12.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Found the leak and added the cleanup callback."}],"model":"gpt-5.5","usage":{"input":200,"output":30,"cacheRead":0,"cacheWrite":0,"totalTokens":4300}}}"#,
    "\n",
    r#"{"type":"wibble-line","id":"x1"}"#,
    "\n",
);

#[test]
fn pi_adapter_merges_assistant_turns_and_backfills_tool_results() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let sessions = temp.path().join("--Users-tester-Github-demo--");
    fs::create_dir_all(&sessions)?;
    fs::write(
        sessions.join("2026-08-06T10-00-00-000Z_66666666-aaaa-bbbb-cccc-000000000006.jsonl"),
        PI_SESSION_JSONL,
    )?;

    let adapter = PiAdapter::with_root(AgentId::Pi, temp.path().to_path_buf());
    // file_ref is a public API: <timestamp>_<uuid>.jsonl should strip out the
    // uuid as native_id.
    let path = sessions.join("2026-08-06T10-00-00-000Z_66666666-aaaa-bbbb-cccc-000000000006.jsonl");
    let reference = adapter
        .file_ref(&path)
        .ok_or_else(|| anyhow::anyhow!("pi file_ref"))?;
    assert_eq!(reference.native_id, "66666666-aaaa-bbbb-cccc-000000000006");

    let references = adapter.list_session_files()?;
    assert_eq!(references.len(), 1);
    let parsed = adapter.parse_session(&references[0])?;
    let transcript = adapter.parse_transcript(&references[0])?;

    assert_eq!(parsed.meta.agent, AgentId::Pi);
    assert_eq!(
        parsed.meta.title,
        "Pi check the useEffect() cleanup in the QR component"
    );
    assert_eq!(parsed.meta.key, "pi:66666666-aaaa-bbbb-cccc-000000000006");
    // cwd comes from the session first line, never inferred from the lossily
    // encoded directory name.
    assert_eq!(parsed.meta.project_path, "/Users/tester/Github/demo");
    assert_eq!(parsed.meta.model.as_deref(), Some("gpt-5.5"));
    // tokens take the last assistant's totalTokens.
    assert_eq!(parsed.meta.tokens_used, Some(4300));
    assert_eq!(parsed.meta.message_count, 2);
    // wibble-line counts as unknown; model_change/thinking_level_change do
    // not.
    assert_eq!(parsed.unknown_line_count, 1);

    // Consecutive assistant lines (separated only by toolResult) merge into
    // one.
    assert_eq!(transcript.mainline.len(), 2);
    assert_eq!(transcript.mainline[0].role, Role::User);
    assert_eq!(transcript.mainline[1].role, Role::Assistant);
    assert_eq!(
        transcript.mainline[1].text,
        "Found the leak and added the cleanup callback."
    );
    assert_eq!(transcript.mainline[1].tool_calls.len(), 1);
    assert_eq!(transcript.mainline[1].tool_calls[0].name, "bash");
    // toolResult is a separate role row, back-filled by toolCallId.
    assert!(transcript.mainline[1].tool_calls[0]
        .output
        .as_deref()
        .unwrap_or_default()
        .contains("QrScanner"));
    assert!(!transcript.mainline[1].tool_calls[0].is_error);
    assert_eq!(
        parsed.units.iter().map(|unit| unit.seq).collect::<Vec<_>>(),
        vec![0, 1]
    );

    // omp is a fork of pi: same parsing core, only AgentId/key prefix
    // differ.
    let omp = PiAdapter::with_root(AgentId::Omp, temp.path().to_path_buf());
    let omp_parsed = omp.parse_session(&references[0])?;
    assert_eq!(omp_parsed.meta.agent, AgentId::Omp);
    assert_eq!(
        omp_parsed.meta.key,
        "omp:66666666-aaaa-bbbb-cccc-000000000006"
    );
    Ok(())
}

// ---------------------------------------------------------------- grok

#[test]
fn grok_adapter_replays_chunks_by_role_and_reads_summary() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let session_dir = temp
        .path()
        .join("%2FUsers%2Ftester%2FGithub%2Fdemo")
        .join("77777777-aaaa-bbbb-cccc-000000000007");
    fs::create_dir_all(&session_dir)?;
    fs::write(
        session_dir.join("updates.jsonl"),
        concat!(
            r#"{"timestamp":1786014300,"method":"session/update","params":{"sessionId":"77777777","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"Grok inspect the QR scan,"}}}}"#,
            "\n",
            r#"{"timestamp":1786014301,"method":"session/update","params":{"sessionId":"77777777","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"focus on useEffect() cleanup"}}}}"#,
            "\n",
            r#"{"timestamp":1786014302,"method":"session/update","params":{"sessionId":"77777777","update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"user wants the scan component effect leak"}}}}"#,
            "\n",
            r#"{"timestamp":1786014303,"method":"session/update","params":{"sessionId":"77777777","update":{"sessionUpdate":"tool_call","toolCallId":"call-grok-1","title":"Grep","rawInput":{"pattern":"useEffect","glob":"**/*.tsx"}}}}"#,
            "\n",
            r#"{"timestamp":1786014304,"method":"session/update","params":{"sessionId":"77777777","update":{"sessionUpdate":"tool_call_update","toolCallId":"call-grok-1","status":"completed","content":[{"type":"content","content":{"type":"text","text":"found 2 matches in QrScanner.tsx"}}],"rawOutput":{"type":"GrepSearch","stdout":[102,111,111]}}}}"#,
            "\n",
            r#"{"timestamp":1786014305,"method":"session/update","params":{"sessionId":"77777777","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Located the leak,"}}}}"#,
            "\n",
            r#"{"timestamp":1786014306,"method":"session/update","params":{"sessionId":"77777777","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"added the cleanup callback."}}}}"#,
            "\n",
            r#"{"timestamp":1786014307,"method":"_x.ai/session/update","params":{"sessionId":"77777777","update":{"sessionUpdate":"auto_compact_started","tokens_used":100}}}"#,
            "\n",
            r#"{"timestamp":1786014308,"method":"session/update","params":{"sessionId":"77777777","update":{"sessionUpdate":"wibble_update"}}}"#,
            "\n",
        ),
    )?;
    fs::write(
        session_dir.join("summary.json"),
        concat!(
            "{\"info\":{\"id\":\"77777777\",\"cwd\":\"/Users/tester/Github/demo\"},",
            "\"generated_title\":\"Grok QR scan cleanup\",",
            "\"created_at\":\"2026-08-06T11:00:00.000000Z\",\"updated_at\":\"2026-08-06T11:20:00.000000Z\",",
            "\"head_branch\":\"feat/qr\",\"current_model_id\":\"grok-composer-2.5-fast\"}"
        ),
    )?;
    // chat_history is a compressed context snapshot, not a session main
    // file.
    fs::write(session_dir.join("chat_history.jsonl"), "{}\n")?;

    let adapter = GrokAdapter::with_root(temp.path().to_path_buf());
    let path = session_dir.join("updates.jsonl");
    let reference = adapter
        .file_ref(&path)
        .ok_or_else(|| anyhow::anyhow!("grok file_ref"))?;
    assert_eq!(reference.native_id, "77777777-aaaa-bbbb-cccc-000000000007");
    assert!(adapter
        .file_ref(&path.with_file_name("chat_history.jsonl"))
        .is_none());

    let references = adapter.list_session_files()?;
    assert_eq!(references.len(), 1);
    let parsed = adapter.parse_session(&references[0])?;
    let transcript = adapter.parse_transcript(&references[0])?;

    assert_eq!(parsed.meta.title, "Grok QR scan cleanup");
    assert_eq!(parsed.meta.project_path, "/Users/tester/Github/demo");
    assert_eq!(parsed.meta.git_branch.as_deref(), Some("feat/qr"));
    assert_eq!(parsed.meta.model.as_deref(), Some("grok-composer-2.5-fast"));
    assert_eq!(parsed.meta.message_count, 2);
    // wibble_update counts as unknown; auto_compact_started does not.
    assert_eq!(parsed.unknown_line_count, 1);

    // chunk stream merges per role segment: two user chunks join into one,
    // thought/message/tool all merge into the same assistant.
    assert_eq!(transcript.mainline.len(), 2);
    assert_eq!(
        transcript.mainline[0].text,
        "Grok inspect the QR scan,focus on useEffect() cleanup"
    );
    assert_eq!(transcript.mainline[0].timestamp, Some(1786014300000));
    let assistant = &transcript.mainline[1];
    assert_eq!(
        assistant.text,
        "Located the leak,added the cleanup callback."
    );
    assert!(assistant
        .thinking
        .as_deref()
        .unwrap_or_default()
        .contains("effect leak"));
    assert_eq!(assistant.tool_calls.len(), 1);
    assert_eq!(assistant.tool_calls[0].name, "Grep");
    // tool_call_update's content text back-fills output; byte-array rawOutput
    // untouched.
    assert!(assistant.tool_calls[0]
        .output
        .as_deref()
        .unwrap_or_default()
        .contains("found 2 matches"));
    assert!(!assistant.tool_calls[0].is_error);
    assert_eq!(
        parsed.units.iter().map(|unit| unit.seq).collect::<Vec<_>>(),
        vec![0, 1]
    );
    Ok(())
}

// ---------------------------------------------------------------- kimi

#[test]
fn kimi_adapter_reads_state_sidecar_and_index_cwd() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let main = temp
        .path()
        .join("sessions/wd_demo_abc123/session_88888888-aaaa-bbbb-cccc-000000000008/agents/main");
    fs::create_dir_all(&main)?;
    fs::write(
        main.join("wire.jsonl"),
        concat!(
            r#"{"type":"metadata","protocol_version":"1.3","created_at":1786200000000}"#,
            "\n",
            r#"{"type":"config.update","profileName":"agent","systemPrompt":"You are Kimi Code CLI."}"#,
            "\n",
            r#"{"type":"tools.set_active_tools","names":["Read","Write","Bash"]}"#,
            "\n",
            r#"{"type":"turn.prompt","input":[{"type":"text","text":"Kimi fix the useEffect() memory leak in the QR component"}],"origin":{"kind":"user"}}"#,
            "\n",
            r#"{"type":"context.append_message","message":{"role":"assistant","content":[{"type":"text","text":"Located the QrScanner leak and added the cleanup callback."}]}}"#,
            "\n",
            r#"{"type":"context.append_loop_event","event":{"type":"tool.result","toolCallId":"k1","result":{"output":"ok","isError":false}}}"#,
            "\n",
            r#"{"type":"turn.ended","turnId":1,"reason":"completed"}"#,
            "\n",
            r#"{"type":"wibble.record","x":1}"#,
            "\n",
        ),
    )?;
    fs::write(
        main.join("../../state.json"),
        "{\"createdAt\":\"2026-08-06T12:00:00.000Z\",\"updatedAt\":\"2026-08-06T12:30:00.000Z\",\"title\":\"Kimi QR fix\"}",
    )?;
    // cwd relies on the root-level session_index.jsonl sessionId→workDir
    // mapping.
    fs::write(
        temp.path().join("session_index.jsonl"),
        "{\"sessionId\":\"session_88888888-aaaa-bbbb-cccc-000000000008\",\"workDir\":\"/Users/tester/Github/demo\"}\n",
    )?;
    // Subagent directory: agents/<non-main>/ is not listed.
    let subagent = temp
        .path()
        .join("sessions/wd_demo_abc123/session_88888888-aaaa-bbbb-cccc-000000000008/agents/helper");
    fs::create_dir_all(&subagent)?;
    fs::write(subagent.join("wire.jsonl"), "{\"type\":\"metadata\"}\n")?;

    let adapter = KimiAdapter::with_root(
        temp.path().join("sessions"),
        temp.path().join("session_index.jsonl"),
    );
    let path = main.join("wire.jsonl");
    let reference = adapter
        .file_ref(&path)
        .ok_or_else(|| anyhow::anyhow!("kimi file_ref"))?;
    assert_eq!(
        reference.native_id,
        "session_88888888-aaaa-bbbb-cccc-000000000008"
    );
    assert!(adapter.file_ref(&subagent.join("wire.jsonl")).is_none());

    let references = adapter.list_session_files()?;
    assert_eq!(references.len(), 1);
    let parsed = adapter.parse_session(&references[0])?;
    assert_eq!(parsed.meta.title, "Kimi QR fix");
    assert_eq!(parsed.meta.project_path, "/Users/tester/Github/demo");
    assert_eq!(parsed.meta.message_count, 2);
    // wibble.record counts as unknown; metadata/config/tools/turn.*/
    // append_loop_event do not.
    assert_eq!(parsed.unknown_line_count, 1);
    assert_eq!(
        parsed.units.iter().map(|unit| unit.seq).collect::<Vec<_>>(),
        vec![0, 1]
    );

    // "New Session" is a placeholder title; must fall back to the first user
    // message.
    let placeholder_dir = temp
        .path()
        .join("sessions/wd_demo_abc123/session_99999999-aaaa-bbbb-cccc-000000000009/agents/main");
    fs::create_dir_all(&placeholder_dir)?;
    fs::write(
        placeholder_dir.join("wire.jsonl"),
        "{\"type\":\"turn.prompt\",\"input\":[{\"type\":\"text\",\"text\":\"Placeholder session falls back to this line\"}]}\n",
    )?;
    fs::write(
        placeholder_dir.join("../../state.json"),
        "{\"createdAt\":\"2026-08-06T12:00:00.000Z\",\"updatedAt\":\"2026-08-06T12:30:00.000Z\",\"title\":\"New Session\"}",
    )?;
    let placeholder = adapter
        .file_ref(&placeholder_dir.join("wire.jsonl"))
        .ok_or_else(|| anyhow::anyhow!("kimi placeholder file_ref"))?;
    let placeholder_parsed = adapter.parse_session(&placeholder)?;
    assert_eq!(
        placeholder_parsed.meta.title,
        "Placeholder session falls back to this line"
    );
    Ok(())
}

// ---------------------------------------------------------------- antigravity

#[test]
fn antigravity_adapter_projects_encrypted_summaries() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let db = temp.path().join("conversation_summaries.db");
    let conn = Connection::open(&db)?;
    conn.execute_batch(
        r#"
        CREATE TABLE conversation_summaries (
            conversation_id text, title text NOT NULL DEFAULT "",
            preview text NOT NULL DEFAULT "", step_count integer NOT NULL DEFAULT 0,
            last_modified_time datetime NOT NULL, workspace_uris text NOT NULL,
            parent_conversation_id text NOT NULL DEFAULT "",
            nesting_depth integer NOT NULL DEFAULT 0,
            last_user_input_time datetime NOT NULL,
            PRIMARY KEY (conversation_id)
        );
        INSERT INTO conversation_summaries
            (conversation_id, title, preview, step_count, last_modified_time,
             workspace_uris, parent_conversation_id, nesting_depth, last_user_input_time)
        VALUES
            ('ag-0001', '', 'QR overlay polish', 12, '2026-08-06 13:00:00.000000+00:00',
             '["file:///Users/tester/Github/sample%20fx"]', '', 0, '2026-08-06 13:00:00.000000+00:00'),
            ('ag-0002', '', 'Child convo', 3, '2026-08-06 13:05:00.000000+00:00',
             '["file:///Users/tester/Github/sample%20fx"]', 'ag-0001', 1, '0001-01-01 00:00:00+00:00');
        "#,
    )?;
    drop(conn);

    let adapter = AntigravityAdapter::with_db(db.clone());
    // Child sessions (non-empty parent_conversation_id) are not listed.
    let references = adapter.list_session_files()?;
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].native_id, "ag-0001");
    assert!(references[0].file_path.ends_with("#ag-0001"));

    let parsed = adapter.parse_session(&references[0])?;
    let transcript = adapter.parse_transcript(&references[0])?;

    // Title lives in the preview column (title column mostly empty).
    assert_eq!(parsed.meta.title, "QR overlay polish");
    // workspace file:// URI percent-decode.
    assert_eq!(parsed.meta.project_path, "/Users/tester/Github/sample fx");
    assert_eq!(parsed.meta.project_name, "sample fx");
    assert_eq!(parsed.meta.message_count, 12);

    // Encrypted body: a single System message carries the preview and the
    // note; FTS finds the preview.
    assert_eq!(transcript.mainline.len(), 1);
    assert_eq!(transcript.mainline[0].role, Role::System);
    assert!(transcript.mainline[0].text.contains("QR overlay polish"));
    assert!(transcript.mainline[0].text.contains("encrypted"));
    assert_eq!(parsed.units.len(), 1);
    assert!(parsed.units[0].text.contains("QR overlay polish"));

    let quick = adapter
        .quick_meta(&references)
        .ok_or_else(|| anyhow::anyhow!("missing antigravity quick meta"))?;
    assert!(quick.contains_key(&references[0].file_path));
    assert!(Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).is_ok());
    Ok(())
}

// ---------------------------------------------------------------- dsh

fn write_dsh_log(path: &std::path::Path) -> Result<()> {
    fs::write(
        path,
        concat!(
            r#"{"type":"session","version":1,"id":"dsh-e2e4-0001","createdAt":1786100000000,"cwd":"/Users/tester/Github/demo","origin":"main","delegationDepth":0}"#,
            "\n",
            r#"{"type":"turn/start","time":1786100000500,"seq":1}"#,
            "\n",
            r#"{"type":"user/message","time":1786100001000,"data":{"content":[{"type":"text","text":"dsh fix the QR scan cleanup"}],"source":{"kind":"user"}}}"#,
            "\n",
            r#"{"type":"assistant/message","time":1786100002000,"data":{"message":{"content":[{"type":"reasoning","text":"dependency array is missing device"},{"type":"tool-call","id":"tc-1","name":"read_file","arguments":"{\"path\":\"src/QrScanner.tsx\"}"},{"type":"text","text":"Found the leak"}],"source":{"model":"deepseek-chat-v4"}},"usage":{"inputTokens":1000,"outputTokens":480}}}"#,
            "\n",
            r#"{"type":"tool/result","time":1786100003000,"data":{"message":{"content":[{"type":"tool-result","toolCallId":"tc-1","content":[{"type":"text","text":"useEffect(() => watch())"}],"isError":false}]}}}"#,
            "\n",
            r#"{"type":"assistant/message","time":1786100004000,"data":{"message":{"content":[{"type":"text","text":"cleaned up the callback"}],"source":{"model":"deepseek-chat-v4"}},"usage":{"inputTokens":800,"outputTokens":760}}}"#,
            "\n",
            r#"{"type":"compaction/summary","time":1786100005000,"surfaceOp":{"op":"replace","start":0,"end":2},"data":{"text":"shortened"}}"#,
            "\n",
            r#"{"type":"user/message","time":1786100005500,"data":{"content":[{"type":"text","text":"<agent-instructions>policy</agent-instructions>"}],"source":{"kind":"agent-instructions"}}}"#,
            "\n",
            r#"{"type":"session/title","time":1786100006000,"data":{"title":"QR scan fix"}}"#,
            "\n",
            r#"{"type":"mystery-row","time":1786100007000}"#,
            "\n",
        ),
    )?;
    Ok(())
}

#[test]
fn dsh_adapter_parses_event_log_and_filters_subagents() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let session_dir = temp
        .path()
        .join("--users-tester-github-demo--/dsh-e2e4-0001");
    fs::create_dir_all(&session_dir)?;
    let log = session_dir.join("session.jsonl");
    write_dsh_log(&log)?;
    // Subagent sessions (origin=subagent) are not listed.
    let subagent_dir = temp
        .path()
        .join("--users-tester-github-demo--/dsh-sub-0002");
    fs::create_dir_all(&subagent_dir)?;
    fs::write(
        subagent_dir.join("session.jsonl"),
        "{\"type\":\"session\",\"version\":1,\"id\":\"dsh-sub-0002\",\"createdAt\":1786100000000,\"cwd\":\"/Users/tester/Github/demo\",\"origin\":\"subagent\",\"delegationDepth\":1}\n",
    )?;

    let adapter = DshAdapter::with_root(temp.path().to_path_buf());
    // native_id comes from the header's authoritative id (the directory name
    // is an escaped id).
    let reference = adapter
        .file_ref(&log)
        .ok_or_else(|| anyhow::anyhow!("dsh file_ref"))?;
    assert_eq!(reference.native_id, "dsh-e2e4-0001");
    assert!(adapter
        .file_ref(&subagent_dir.join("session.jsonl"))
        .is_none());

    let references = adapter.list_session_files()?;
    assert_eq!(references.len(), 1);
    let parsed = adapter.parse_session(&references[0])?;
    let transcript = adapter.parse_transcript(&references[0])?;

    // session/title events are last-wins for the title.
    assert_eq!(parsed.meta.title, "QR scan fix");
    assert_eq!(parsed.meta.key, "dsh:dsh-e2e4-0001");
    assert_eq!(parsed.meta.project_path, "/Users/tester/Github/demo");
    assert_eq!(parsed.meta.model.as_deref(), Some("deepseek-chat-v4"));
    // usage accounts per model call and accumulates per call (1480 + 1560).
    assert_eq!(parsed.meta.tokens_used, Some(3040));
    assert_eq!(parsed.meta.created_at, 1786100000000);
    assert_eq!(parsed.meta.updated_at, 1786100007000);
    // Injected context counts as Meta and is not counted; surfaceOp replace
    // and turn boundaries produce no bubbles.
    assert_eq!(parsed.meta.message_count, 2);
    // mystery-row counts as unknown; *-chunks packed lines and turn/step
    // boundaries do not.
    assert_eq!(parsed.unknown_line_count, 1);

    // Consecutive assistant steps (separated only by tool/result) merge into
    // one; injected context with source.kind other than "user" counts as
    // Meta.
    assert_eq!(transcript.mainline.len(), 3);
    assert_eq!(transcript.mainline[0].role, Role::User);
    assert_eq!(transcript.mainline[0].kind, MessageKind::Text);
    assert_eq!(transcript.mainline[1].role, Role::Assistant);
    assert_eq!(transcript.mainline[2].role, Role::User);
    assert_eq!(transcript.mainline[2].kind, MessageKind::Meta);
    let assistant = &transcript.mainline[1];
    assert!(assistant.text.contains("Found the leak"));
    assert!(assistant.text.contains("cleaned up the callback"));
    // reasoning blocks separate into thinking, never mixed into the body.
    assert!(assistant
        .thinking
        .as_deref()
        .unwrap_or_default()
        .contains("dependency array is missing device"));
    assert_eq!(assistant.tool_calls.len(), 1);
    assert_eq!(assistant.tool_calls[0].name, "read_file");
    // tool/result events back-fill output by toolCallId.
    assert!(assistant.tool_calls[0]
        .output
        .as_deref()
        .unwrap_or_default()
        .contains("useEffect"));
    assert!(!assistant.tool_calls[0].is_error);
    assert_eq!(
        parsed.units.iter().map(|unit| unit.seq).collect::<Vec<_>>(),
        vec![0, 1]
    );

    // Switching the compression configuration leaves both suffixes in place:
    // with a newer sibling, the stale one yields at file_ref (arbitration at
    // a single point, shared by list and watcher).
    let zstd_log = session_dir.join("session.jsonl.zstd");
    let plain = fs::read_to_string(&log)?;
    let header_line = plain.lines().next().unwrap_or_default().to_string();
    let mut encoder = zstd::stream::write::Encoder::new(fs::File::create(&zstd_log)?, 0)?;
    use std::io::Write;
    writeln!(encoder, "{header_line}")?;
    encoder.finish()?;
    let now = std::time::SystemTime::now();
    let stale = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    fs::OpenOptions::new()
        .write(true)
        .open(&log)?
        .set_modified(stale)?;
    fs::OpenOptions::new()
        .write(true)
        .open(&zstd_log)?
        .set_modified(now)?;
    assert!(
        adapter.file_ref(&log).is_none(),
        "stale sibling should yield"
    );
    let promoted = adapter
        .file_ref(&zstd_log)
        .ok_or_else(|| anyhow::anyhow!("promoted zstd file_ref"))?;
    assert_eq!(promoted.native_id, "dsh-e2e4-0001");
    Ok(())
}

#[test]
fn dsh_torn_zstd_frame_terminates() -> Result<()> {
    // Half-written final frame: the writer appends one frame per write and
    // scanning is naturally concurrent with dsh, so it will be read. The
    // zstd decoder returns UnexpectedEof repeatedly for a truncated tail
    // rather than EOF — without terminating in place the parser would loop
    // forever. Independent tempdir keeps the main contract test untouched.
    let temp = tempfile::tempdir()?;
    let session_dir = temp
        .path()
        .join("--users-tester-github-demo--/dsh-torn-0003");
    fs::create_dir_all(&session_dir)?;
    let zstd_log = session_dir.join("session.jsonl.zstd");
    {
        use std::io::Write;
        // Two frames simulate dsh's multiframe append: first frame the
        // header, second frame the body.
        let plain = concat!(
            "{\"type\":\"session\",\"version\":1,\"id\":\"dsh-torn-0003\",\"createdAt\":1786100000000,\"cwd\":\"/Users/tester/Github/demo\"}\n",
            "{\"type\":\"user/message\",\"time\":1786100001000,\"data\":{\"content\":[{\"type\":\"text\",\"text\":\"torn frame session\"}],\"source\":{\"kind\":\"user\"}}}\n",
        );
        let (header, rest) = plain.split_once('\n').unwrap_or((plain, ""));
        let mut first = zstd::stream::write::Encoder::new(fs::File::create(&zstd_log)?, 0)?;
        writeln!(first, "{header}")?;
        first.finish()?;
        let append = fs::OpenOptions::new().append(true).open(&zstd_log)?;
        let mut second = zstd::stream::write::Encoder::new(append, 0)?;
        second.write_all(rest.as_bytes())?;
        second.finish()?;
    }
    let full = fs::read(&zstd_log)?;
    fs::write(&zstd_log, &full[..full.len() - 12])?;

    let adapter = DshAdapter::with_root(temp.path().to_path_buf());
    // The header is in the first frame and intact, so the torn session still
    // lists normally (content ends at the tear point).
    let reference = adapter
        .file_ref(&zstd_log)
        .ok_or_else(|| anyhow::anyhow!("torn file_ref"))?;
    assert_eq!(reference.native_id, "dsh-torn-0003");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let torn_adapter = DshAdapter::with_root(temp.path().to_path_buf());
        sender
            .send(torn_adapter.parse_transcript(&reference).is_ok())
            .ok();
    });
    let ok = receiver
        .recv_timeout(std::time::Duration::from_secs(10))
        .map_err(|_| anyhow::anyhow!("torn frame stuck the parser (infinite loop)"))?;
    assert!(
        ok,
        "torn tail should terminate gracefully, not fail the session"
    );
    Ok(())
}

// ---------------------------------------------------------------- source roster contracts

/// Minimal source adapter used to keep routing tests independent from any
/// provider filesystem format.
struct RootOnlyAdapter {
    agent: AgentId,
    root: PathBuf,
}

impl AgentHistoryAdapter for RootOnlyAdapter {
    fn agent(&self) -> AgentId {
        self.agent
    }

    fn list_session_files(&self) -> Result<Vec<super::super::models::SessionFileRef>> {
        Ok(Vec::new())
    }

    fn parse_session(
        &self,
        _reference: &super::super::models::SessionFileRef,
    ) -> Result<super::super::models::ParsedSession> {
        anyhow::bail!("unused in source routing test")
    }

    fn parse_transcript(
        &self,
        _reference: &super::super::models::SessionFileRef,
    ) -> Result<super::super::models::ParsedTranscript> {
        anyhow::bail!("unused in source routing test")
    }

    fn with_custom_root(&self, root: PathBuf) -> Box<dyn AgentHistoryAdapter> {
        Box::new(Self {
            agent: self.agent,
            root,
        })
    }

    fn data_roots(&self) -> Vec<PathBuf> {
        vec![self.root.clone()]
    }
}

#[test]
fn path_owns_respects_filesystem_and_sqlite_boundaries() {
    assert!(path_owns("/sessions", "/sessions"));
    assert!(path_owns("/sessions", "/sessions/file.jsonl"));
    assert!(!path_owns("/sessions", "/sessions-old/file.jsonl"));
    assert!(path_owns("/", "/Users/demo/session.jsonl"));
    assert!(path_owns("/store.sqlite", "/store.sqlite#thread-1"));
    assert!(!path_owns("/store.sqlite", "/store.sqlite-old#thread-1"));
    assert!(path_owns(r"C:\sessions", r"C:\sessions\nested\file.jsonl"));
    assert!(!path_owns(r"C:\sessions", r"C:\sessions-old\file.jsonl"));
    assert!(path_owns(r"\\nas\agents", r"\\nas\agents\session.jsonl"));
    assert!(!path_owns(
        r"\\nas\agents",
        r"\\nas\agents-old\session.jsonl"
    ));
}

#[test]
fn adapter_ix_for_uses_longest_matching_root() {
    let adapters: Vec<Box<dyn AgentHistoryAdapter>> = vec![
        Box::new(RootOnlyAdapter {
            agent: AgentId::Codex,
            root: PathBuf::from("/tmp/codex"),
        }),
        Box::new(RootOnlyAdapter {
            agent: AgentId::Codex,
            root: PathBuf::from("/tmp/codex/custom"),
        }),
    ];
    assert_eq!(
        adapter_ix_for(&adapters, AgentId::Codex, "/tmp/codex/custom/file.jsonl"),
        Some(1)
    );
    assert_eq!(
        adapter_ix_for(&adapters, AgentId::Codex, "/tmp/codex-other/file.jsonl"),
        Some(0)
    );
}
