// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.
//! Equivalence contract tests for live incremental decoding.
//!
//! Core contract: `parse_full(fixture) == incremental decode(fixture split at
//! arbitrary byte boundaries)`. The full side uses the adapter public API
//! (`parse_transcript`) as an independent reference; the incremental side
//! starts from an empty file, appends chunk by chunk at split points + `sync`,
//! finally `settle`s, and compares the message sequence and parsed facts.

use super::LiveChange;
use super::LiveSession;
use crate::adapters::AgentHistoryAdapter as _;
use crate::catalog::HistoryCatalog;
use crate::models::{AgentId, Role, SessionFileRef, SessionMeta};
use anyhow::Result;
use std::io::Write as _;
use std::path::Path;

const CLAUDE_FIXTURE: &str = concat!(
    r#"{"type":"user","cwd":"/work/中文 demo","gitBranch":"main","timestamp":"2026-08-01T01:00:00Z","message":{"content":"修复解析器 🚀"}}"#,
    "\n",
    r#"{"type":"assistant","cwd":"/work/中文 demo","timestamp":"2026-08-01T01:00:01Z","message":{"id":"m1","model":"claude-sonnet-4-5","usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":3},"content":[{"type":"thinking","thinking":"先看代码 🤔"},{"type":"tool_use","id":"tool-1","name":"Read","input":{"file_path":"src/lib.rs"}},{"type":"text","text":"我找到了。"}]}}"#,
    "\n",
    r#"{"type":"user","timestamp":"2026-08-01T01:00:02Z","message":{"content":[{"type":"tool_result","tool_use_id":"tool-1","content":"源码文本"}]}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2026-08-01T01:00:03Z","message":{"id":"m2","content":[{"type":"text","text":"继续"}]}}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2026-08-01T01:00:04Z","message":{"id":"m2","content":[{"type":"text","text":"补充说明"}]}}"#,
    "\n",
    r#"{"type":"unknown-kind","timestamp":"2026-08-01T01:00:05Z"}"#,
    "\n",
    r#"{"type":"file-history-snapshot"}"#,
    "\n",
    r#"{"type":"system","subtype":"compact_boundary","timestamp":"2026-08-01T01:00:06Z"}"#,
    "\n",
    r#"{"type":"user","timestamp":"2026-08-01T01:00:07Z","isMeta":true,"message":{"content":"<environment_context>meta</environment_context>"}}"#,
    "\n",
    r#"{"type":"custom-title","customTitle":"自定义标题"}"#,
    "\n",
    r#"{"type":"assistant","timestamp":"2026-08-01T01:00:08Z","isSidechain":true,"message":{"id":"s1","content":[{"type":"text","text":"sidechain text"}]}}"#,
    "\n",
    r#"{"type":"user","timestamp":"2026-08-01T01:00:09Z","message":{"content":"第二条用户消息"}}"#,
    // Deliberately not newline-terminated: settle must interpret the last
    // line as a complete line, equivalent to full parsing.
);

const CODEX_FIXTURE: &str = concat!(
    r#"{"timestamp":"2026-08-02T09:15:00Z","type":"session_meta","payload":{"cwd":"/work/codex-中文","originator":"codex_cli_rs","git":{"branch":"feature/live"}}}"#,
    "\n",
    r#"{"timestamp":"2026-08-02T09:15:01Z","type":"turn_context","payload":{"model":"gpt-5.2"}}"#,
    "\n",
    r#"{"timestamp":"2026-08-02T09:15:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"构建 live tail 🇨🇳"}]}}"#,
    "\n",
    r#"{"timestamp":"2026-08-02T09:15:03Z","type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"先想想"}]}}"#,
    "\n",
    r#"{"timestamp":"2026-08-02T09:15:04Z","type":"response_item","payload":{"type":"function_call","call_id":"call-1","name":"shell","arguments":"{\"command\":\"cargo test\"}"}}"#,
    "\n",
    r#"{"timestamp":"2026-08-02T09:15:05Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"ok ✅"}}"#,
    "\n",
    r#"{"timestamp":"2026-08-02T09:15:06Z","type":"response_item","payload":{"type":"local_shell_call","call_id":"call-2","action":{"type":"exec","command":["ls","-la"]}}}"#,
    "\n",
    r#"{"timestamp":"2026-08-02T09:15:07Z","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"call-2","output":{"content":"listed"}}}"#,
    "\n",
    r#"{"timestamp":"2026-08-02T09:15:08Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"完成了 🎉"}]}}"#,
    "\n",
    r#"{"timestamp":"2026-08-02T09:15:09Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":42}}}}"#,
    "\n",
    r#"{"timestamp":"2026-08-02T09:15:10Z","type":"event_msg","payload":{"type":"agent_message","message":"fallback 视图"}}"#,
    "\n",
    r#"{"type":"totally-unknown","timestamp":"2026-08-02T09:15:11Z"}"#,
    "\n",
    r#"{"timestamp":"2026-08-02T09:15:12Z","type":"compacted"}"#,
    "\n",
);

const PI_FIXTURE: &str = concat!(
    r#"{"type":"session","version":3,"id":"pi-session-中文","timestamp":"2026-08-03T10:00:00Z","cwd":"/work/pi-emoji"}"#,
    "\n",
    r#"{"type":"model_change","modelId":"pi-2"}"#,
    "\n",
    r#"{"type":"message","timestamp":"2026-08-03T10:00:01Z","message":{"role":"user","content":[{"type":"text","text":"你好 Pi 👋"}]}}"#,
    "\n",
    r#"{"type":"message","timestamp":"2026-08-03T10:00:02Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"call-pi-1","name":"bash","arguments":{"command":"ls"}},{"type":"text","text":"我来查看"}],"model":"pi-2","usage":{"totalTokens":77}}}"#,
    "\n",
    r#"{"type":"message","timestamp":"2026-08-03T10:00:03Z","message":{"role":"toolResult","toolCallId":"call-pi-1","content":[{"type":"text","text":"文件列表"}],"isError":false}}"#,
    "\n",
    r#"{"type":"message","timestamp":"2026-08-03T10:00:04Z","message":{"role":"assistant","content":[{"type":"text","text":"看完了 ✨"}]}}"#,
    "\n",
    r#"{"type":"unknown-record","timestamp":"2026-08-03T10:00:05Z"}"#,
    "\n",
    r#"{"type":"message","timestamp":"2026-08-03T10:00:06Z","message":{"role":"user","content":[{"type":"text","text":"再来一条"}]}}"#,
    "\n",
);

const KIMI_FIXTURE: &str = concat!(
    r#"{"type":"metadata","protocol_version":"1.3","created_at":1786200000000}"#,
    "\n",
    r#"{"type":"config.update","profileName":"agent","systemPrompt":"You are Kimi Code CLI."}"#,
    "\n",
    r#"{"type":"turn.prompt","input":[{"type":"text","text":"Kimi 修一下 QR 组件的内存泄漏"}],"origin":{"kind":"user"}}"#,
    "\n",
    r#"{"type":"context.append_message","message":{"role":"assistant","content":[{"type":"text","text":"定位到 QrScanner 的泄漏，已补上清理回调。"}]}}"#,
    "\n",
    r#"{"type":"context.append_loop_event","event":{"type":"tool.result","toolCallId":"k1","result":{"output":"ok","isError":false}}}"#,
    "\n",
    r#"{"type":"turn.ended","turnId":1,"reason":"completed"}"#,
    "\n",
    r#"{"type":"turn.steer","input":"顺便看看 resize 监听"}"#,
    "\n",
    r#"{"type":"context.append_message","message":{"role":"assistant","content":[{"type":"text","text":"resize 监听也已移除。"}]}}"#,
    "\n",
    r#"{"type":"wibble.record","x":1}"#,
    // Deliberately not newline-terminated: settle must interpret the last
    // line as a complete line.
);

const CURSOR_FIXTURE: &str = concat!(
    r#"{"role":"user","message":{"content":[{"type":"text","text":"<timestamp>Thursday, Jul 23, 2026, 4:00 PM (UTC+8)</timestamp><user_query>Cursor 第一问</user_query>"}]}}"#,
    "\n",
    r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"path":"src/main.rs"}}]}}"#,
    "\n",
    r#"{"role":"assistant","message":{"content":[{"type":"text","text":"第一答（合并块）。"}]}}"#,
    "\n",
    r#"{"type":"turn_ended"}"#,
    "\n",
    r#"{"role":"user","message":{"content":[{"type":"text","text":"<timestamp>Thursday, Jul 23, 2026, 4:01 PM (UTC+8)</timestamp><user_query>第二问</user_query>"}]}}"#,
    "\n",
    r#"{"role":"assistant","message":{"content":[{"type":"text","text":"第二答"}]}}"#,
    "\n",
    r#"{"role":"assistant","message":{"content":[{"type":"text","text"}]}}"#,
    "\n",
    r#"{"type":"wobble.line"}"#,
    // Deliberately not newline-terminated: settle must interpret the last
    // line as a complete line (empty text block skipped).
);

const CC_FIXTURE: &str = concat!(
    r#"{"type":"session","version":3,"id":"cc-live","timestamp":"2026-08-27T00:00:00Z","cwd":"/work/cc"}"#,
    "\n",
    r#"{"type":"message","id":"m1","parentId":null,"timestamp":"2026-08-27T00:00:00Z","message":{"role":"user","content":[{"type":"text","text":"CC 第一问"}]}}"#,
    "\n",
    r#"{"type":"message","id":"m2","parentId":"m1","timestamp":"2026-08-27T00:00:01Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"先规划"},{"type":"text","text":"我来查看。"},{"type":"tool_use","id":"t1","name":"shell_command","input":{"command":"pwd"}}]},"model":"deepseek/deepseek-v4-flash"}"#,
    "\n",
    r#"{"type":"message","id":"m3","parentId":"m2","timestamp":"2026-08-27T00:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"ok"}]}]}}"#,
    "\n",
    r#"{"type":"message","id":"m4","parentId":"m3","timestamp":"2026-08-27T00:00:03Z","message":{"role":"assistant","content":[{"type":"text","text":"完成。"}]}}"#,
    "\n",
    r#"{"type":"torn-record""#,
    "\n",
    r#"{"type":"message","id":"m5","parentId":"m4","timestamp":"2026-08-27T00:00:04Z","message":{"role":"user","content":[{"type":"text","text":"CC 第二问"}]}}"#,
    // Deliberately not newline-terminated: settle must interpret the last
    // line as a complete line.
);

fn session_ref(path: &Path, agent: AgentId) -> SessionFileRef {
    SessionFileRef {
        agent,
        native_id: "native-test-id".to_string(),
        file_path: path.to_string_lossy().to_string(),
        mtime_ms: 0,
        size: 0,
    }
}

fn append_chunk(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    Ok(())
}

/// Deterministic set of split points: several strides + one chunk + midpoint,
/// including cuts inside UTF-8 multi-byte sequences.
fn cut_sets(content: &str) -> Vec<Vec<usize>> {
    let bytes = content.as_bytes();
    let mut sets = Vec::new();
    for stride in [3usize, 7, 13, 29] {
        sets.push((1..bytes.len()).step_by(stride).collect());
    }
    sets.push(vec![bytes.len()]);
    sets.push(vec![1, bytes.len() / 2, bytes.len() - 1]);
    // Find an internal byte position of a multi-byte character to prove the
    // partial-line buffer concatenates safely at byte level.
    if let Some(position) = bytes
        .iter()
        .position(|&byte| byte & 0xE0 == 0xC0 || byte & 0xF0 == 0xE0 || byte & 0xF8 == 0xF0)
    {
        sets.push(vec![position + 1, bytes.len() - 2]);
    }
    sets
}

/// Start from an empty file, append chunk by chunk at split points + sync,
/// finally settle.
fn drive(path: &Path, agent: AgentId, content: &str, cuts: &[usize]) -> Result<LiveSession> {
    std::fs::write(path, b"")?;
    let mut session = LiveSession::open(session_ref(path, agent))?;
    let bytes = content.as_bytes();
    let mut start = 0usize;
    for &cut in cuts {
        let end = cut.min(bytes.len()).max(start);
        if end == start {
            continue;
        }
        append_chunk(path, &bytes[start..end])?;
        session.sync()?;
        start = end;
    }
    if start < bytes.len() {
        append_chunk(path, &bytes[start..])?;
        session.sync()?;
    }
    session.settle();
    Ok(session)
}

/// Parse the complete fixture via the adapter public API as an independent
/// reference (not through the live state machine).
fn expected_transcript(agent: AgentId, path: &Path) -> Result<crate::models::ParsedTranscript> {
    let reference = session_ref(path, agent);
    match agent {
        AgentId::ClaudeCode => {
            crate::adapters::claude::ClaudeAdapter::with_root(std::path::PathBuf::from("/"))
                .parse_transcript(&reference)
        }
        AgentId::Codex => {
            crate::adapters::codex::CodexAdapter::with_root(std::path::PathBuf::from("/"))
                .parse_transcript(&reference)
        }
        AgentId::Pi | AgentId::Omp => {
            crate::adapters::pi::PiAdapter::with_root(agent, std::path::PathBuf::from("/"))
                .parse_transcript(&reference)
        }
        AgentId::Cursor => {
            crate::adapters::cursor::CursorAdapter::with_root(std::path::PathBuf::from("/"))
                .parse_transcript(&reference)
        }
        AgentId::CommandCode => crate::adapters::command_code::CommandCodeAdapter::with_root(
            std::path::PathBuf::from("/"),
        )
        .parse_transcript(&reference),
        AgentId::Kimi => {
            // cwd goes through session_index; the equivalence comparison
            // surface is only mainline messages, so the index is left empty.
            crate::adapters::kimi::KimiAdapter::with_root(
                std::path::Path::new(&reference.file_path)
                    .ancestors()
                    .nth(4)
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_default(),
                std::path::PathBuf::from("/nonexistent-index"),
            )
            .parse_transcript(&reference)
        }
        other => anyhow::bail!("unexpected agent {other:?}"),
    }
}

fn facts_from(transcript: &crate::models::ParsedTranscript, agent: AgentId) -> super::LiveFacts {
    let meta = &transcript.meta;
    super::LiveFacts {
        title: meta.title.clone(),
        cwd: meta.project_path.clone(),
        git_branch: meta.git_branch.clone(),
        model: meta.model.clone(),
        source: meta.source.clone(),
        tokens_used: meta.tokens_used.unwrap_or(0),
        created_at: meta.created_at,
        updated_at: meta.updated_at,
        unknown_lines: transcript.unknown_line_count,
        session_id: matches!(agent, AgentId::Pi | AgentId::Omp).then(|| meta.id.clone()),
    }
}

fn assert_full_eq_incremental(agent: AgentId, content: &str, path: &Path) -> Result<()> {
    std::fs::write(path, content.as_bytes())?;
    let transcript = expected_transcript(agent, path)?;
    let expected = facts_from(&transcript, agent);

    for cuts in cut_sets(content) {
        let session = drive(path, agent, content, &cuts)?;
        let snapshot = session.snapshot();
        assert_eq!(
            snapshot.messages, transcript.mainline,
            "{agent:?}: incremental messages diverge from full parse at {cuts:?}"
        );
        assert_eq!(
            snapshot.facts, expected,
            "{agent:?}: incremental facts diverge from full parse at {cuts:?}"
        );
    }
    Ok(())
}

#[test]
fn claude_full_parse_equals_incremental_at_arbitrary_splits() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("claude-live.jsonl");
    assert_full_eq_incremental(AgentId::ClaudeCode, CLAUDE_FIXTURE, &path)
}

#[test]
fn codex_full_parse_equals_incremental_at_arbitrary_splits() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("codex-live.jsonl");
    assert_full_eq_incremental(AgentId::Codex, CODEX_FIXTURE, &path)
}

#[test]
fn pi_full_parse_equals_incremental_at_arbitrary_splits() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("pi-live.jsonl");
    assert_full_eq_incremental(AgentId::Pi, PI_FIXTURE, &path)
}

#[test]
fn claude_fixture_produces_expected_semantics() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("claude-live.jsonl");
    let session = drive(&path, AgentId::ClaudeCode, CLAUDE_FIXTURE, &[usize::MAX])?;
    let snapshot = session.snapshot();
    assert_eq!(snapshot.messages.len(), 6);
    assert_eq!(snapshot.messages[0].role, Role::User);
    assert_eq!(snapshot.messages[0].text, "修复解析器 🚀");
    let assistant = &snapshot.messages[1];
    assert_eq!(assistant.thinking.as_deref(), Some("先看代码 🤔"));
    assert_eq!(assistant.tool_calls.len(), 1);
    assert_eq!(assistant.tool_calls[0].output.as_deref(), Some("源码文本"));
    // Two lines with the same msg_id merge into the same pending and flush
    // as one message.
    assert_eq!(snapshot.messages[2].text, "继续\n\n补充说明");
    assert_eq!(
        snapshot.messages[3].kind,
        crate::models::MessageKind::CompactSummary
    );
    assert_eq!(snapshot.messages[4].kind, crate::models::MessageKind::Meta);
    assert_eq!(snapshot.facts.title, "自定义标题");
    assert_eq!(snapshot.facts.unknown_lines, 1);
    assert_eq!(snapshot.facts.model.as_deref(), Some("claude-sonnet-4-5"));
    assert_eq!(snapshot.facts.tokens_used, 18);
    Ok(())
}

#[test]
fn duplicate_sync_without_growth_is_idempotent() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("wake.jsonl");
    std::fs::write(&path, "")?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::Codex))?;
    let line = CODEX_FIXTURE
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("fixture has no lines"))?;
    append_chunk(&path, format!("{line}\n").as_bytes())?;
    let first = session.sync()?;
    assert_eq!(first.change, LiveChange::Appended);
    assert_eq!(first.lines_fed, 1);
    let snapshot_after_first = session.snapshot();

    // Duplicate FS wakes: no new bytes → Unchanged, zero consumption, zero
    // changes.
    for _ in 0..3 {
        let repeat = session.sync()?;
        assert!(repeat.is_unchanged());
        assert_eq!(repeat.appended, Vec::<usize>::new());
        assert_eq!(repeat.changed, Vec::<usize>::new());
    }
    assert_eq!(session.snapshot(), snapshot_after_first);
    // sync/settle after settle are equally idempotent.
    session.settle();
    assert!(session.settle().is_unchanged());
    assert!(session.sync()?.is_unchanged());
    Ok(())
}

#[test]
fn partial_line_is_held_until_completed() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("partial.jsonl");
    std::fs::write(&path, "")?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::ClaudeCode))?;
    let line = CLAUDE_FIXTURE
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("fixture has no lines"))?;
    let bytes = line.as_bytes();
    let half = bytes.len() / 2;

    append_chunk(&path, &bytes[..half])?;
    let sync = session.sync()?;
    assert_eq!(sync.change, LiveChange::Appended);
    assert_eq!(sync.lines_fed, 0);
    assert!(session.snapshot().messages.is_empty());

    // Completing the partial line (with newline): decodes exactly one user
    // message without re-consuming the first half.
    append_chunk(&path, &bytes[half..])?;
    append_chunk(&path, b"\n")?;
    let sync = session.sync()?;
    assert_eq!(sync.lines_fed, 1);
    assert_eq!(sync.appended, vec![0]);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.messages[0].text, "修复解析器 🚀");
    Ok(())
}

#[test]
fn delayed_tool_result_updates_one_activity() -> Result<()> {
    // Claude: tool_use arrives first; tool_result only in a later chunk.
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("claude-delayed.jsonl");
    std::fs::write(&path, "")?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::ClaudeCode))?;
    let assistant = r#"{"type":"assistant","cwd":"/w","timestamp":"2026-08-01T01:00:00Z","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}"#;
    let result = r#"{"type":"user","timestamp":"2026-08-01T01:00:01Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"out"}]}}"#;
    append_chunk(&path, format!("{assistant}\n").as_bytes())?;
    let sync = session.sync()?;
    assert_eq!(session.snapshot().messages[0].tool_calls[0].output, None);
    assert!(sync.changed.is_empty());

    append_chunk(&path, format!("{result}\n").as_bytes())?;
    let sync = session.sync()?;
    // On the first sync the pending assistant is visible only as a
    // projection; when the tool_result line arrives the pending truly
    // flushes to row 0 (appended) and the output is then back-filled in
    // place (changed).
    // Snapshot consumers upsert idempotently by row index; the pending
    // projection and the flush result share the same index.
    assert_eq!(sync.appended, vec![0]);
    assert_eq!(sync.changed, vec![0]);
    let messages = session.snapshot().messages;
    assert_eq!(messages.len(), 1, "tool result must not create a new row");
    assert_eq!(messages[0].tool_calls[0].output.as_deref(), Some("out"));

    // Pi: toolCall arrives first, toolResult later.
    let path_pi = temp.path().join("pi-delayed.jsonl");
    std::fs::write(&path_pi, "")?;
    let mut session = LiveSession::open(session_ref(&path_pi, AgentId::Pi))?;
    let call = r#"{"type":"message","timestamp":"2026-08-03T10:00:00Z","message":{"role":"assistant","content":[{"type":"toolCall","id":"c1","name":"bash","arguments":{}}]}}"#;
    let result = r#"{"type":"message","timestamp":"2026-08-03T10:00:01Z","message":{"role":"toolResult","toolCallId":"c1","content":[{"type":"text","text":"pi out"}],"isError":true}}"#;
    append_chunk(&path_pi, format!("{call}\n").as_bytes())?;
    session.sync()?;
    append_chunk(&path_pi, format!("{result}\n").as_bytes())?;
    let sync = session.sync()?;
    assert_eq!(sync.changed, vec![0]);
    let messages = session.snapshot().messages;
    assert_eq!(messages.len(), 1);
    assert!(messages[0].tool_calls[0].is_error);
    assert_eq!(messages[0].tool_calls[0].output.as_deref(), Some("pi out"));

    // Codex: function_call arrives first, function_call_output later.
    let path_codex = temp.path().join("codex-delayed.jsonl");
    std::fs::write(&path_codex, "")?;
    let mut session = LiveSession::open(session_ref(&path_codex, AgentId::Codex))?;
    let call = r#"{"timestamp":"2026-08-02T09:00:00Z","type":"response_item","payload":{"type":"function_call","call_id":"k1","name":"shell","arguments":"{}"}}"#;
    let output = r#"{"timestamp":"2026-08-02T09:00:01Z","type":"response_item","payload":{"type":"function_call_output","call_id":"k1","output":"codex out"}}"#;
    append_chunk(&path_codex, format!("{call}\n").as_bytes())?;
    session.sync()?;
    append_chunk(&path_codex, format!("{output}\n").as_bytes())?;
    let sync = session.sync()?;
    assert_eq!(sync.changed, vec![0]);
    let messages = session.snapshot().messages;
    assert_eq!(messages.len(), 1);
    assert_eq!(
        messages[0].tool_calls[0].output.as_deref(),
        Some("codex out")
    );
    Ok(())
}

#[test]
fn truncate_resets_generation_and_matches_full_reparse() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("truncate.jsonl");
    let Some(head) = CODEX_FIXTURE
        .match_indices("\n")
        .take(4)
        .last()
        .map(|(index, _)| &CODEX_FIXTURE[..index + 1])
    else {
        anyhow::bail!("fixture has too few lines");
    };
    std::fs::write(&path, head)?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::Codex))?;
    assert!(!session.snapshot().messages.is_empty());

    // Truncated to shorter, entirely new content: Reset, generation +1, and
    // the snapshot matches a full re-parse.
    let fresh = r#"{"timestamp":"2026-08-02T10:00:00Z","type":"session_meta","payload":{"cwd":"/work/fresh"}}"#;
    std::fs::write(&path, fresh.as_bytes())?;
    let sync = session.sync()?;
    assert_eq!(sync.change, LiveChange::Reset);
    assert_eq!(sync.generation, 1);
    assert_eq!(session.generation(), 1);
    session.settle();
    let snapshot = session.snapshot();
    let expected = expected_transcript(AgentId::Codex, &path)?;
    assert_eq!(snapshot.messages, expected.mainline);
    assert_eq!(snapshot.facts, facts_from(&expected, AgentId::Codex));
    assert_eq!(snapshot.facts.cwd, "/work/fresh");
    Ok(())
}

#[test]
fn truncate_to_empty_then_append_decodes_normally() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("truncate-empty.jsonl");
    std::fs::write(&path, PI_FIXTURE)?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::Pi))?;
    assert_eq!(session.snapshot().messages.len(), 3);

    std::fs::write(&path, b"")?;
    let sync = session.sync()?;
    assert_eq!(sync.change, LiveChange::Reset);
    assert!(session.snapshot().messages.is_empty());

    append_chunk(&path, PI_FIXTURE.as_bytes())?;
    session.sync()?;
    session.settle();
    let expected = expected_transcript(AgentId::Pi, &path)?;
    assert_eq!(session.snapshot().messages, expected.mainline);
    Ok(())
}

#[test]
fn multiple_records_in_one_chunk_decode_together() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("multi.jsonl");
    std::fs::write(&path, "")?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::ClaudeCode))?;
    let lines: Vec<&str> = CLAUDE_FIXTURE.lines().take(3).collect();
    append_chunk(&path, format!("{}\n", lines.join("\n")).as_bytes())?;
    let sync = session.sync()?;
    assert_eq!(sync.lines_fed, 3);
    // Two user/assistant messages + one pure tool_result line (no new
    // message, back-fill only).
    assert_eq!(sync.appended.len(), 2);
    assert_eq!(sync.changed, vec![1]);
    Ok(())
}

#[test]
fn codex_fallback_view_swaps_once_real_content_arrives() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("codex-fallback.jsonl");
    std::fs::write(&path, "")?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::Codex))?;
    let fallback = r#"{"timestamp":"2026-08-02T09:00:00Z","type":"event_msg","payload":{"type":"agent_message","message":"仅 event 兜底"}}"#;
    append_chunk(&path, format!("{fallback}\n").as_bytes())?;
    session.sync()?;
    let snapshot = session.snapshot();
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.messages[0].text, "仅 event 兜底");

    // Once real response_item content arrives, the view switches back to the
    // main one wholesale; the snapshot matches full parsing.
    let real = concat!(
        r#"{"timestamp":"2026-08-02T09:00:01Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"真实内容"}]}}"#,
        "\n",
    );
    append_chunk(&path, real.as_bytes())?;
    session.sync()?;
    session.settle();
    let expected = expected_transcript(AgentId::Codex, &path)?;
    assert_eq!(session.snapshot().messages, expected.mainline);
    assert_eq!(session.snapshot().messages[0].text, "真实内容");
    Ok(())
}

#[test]
fn unsupported_agent_is_rejected() {
    assert!(!LiveSession::supports(AgentId::Gemini));
    assert!(LiveSession::supports(AgentId::ClaudeCode));
    assert!(LiveSession::supports(AgentId::Codex));
    assert!(LiveSession::supports(AgentId::Pi));
    let reference = SessionFileRef {
        agent: AgentId::Gemini,
        native_id: "x".to_string(),
        file_path: "/definitely/missing.jsonl".to_string(),
        mtime_ms: 0,
        size: 0,
    };
    assert!(LiveSession::open(reference).is_err());
}

#[test]
fn catalog_exact_native_lookup_binds_unique_source() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let claude_path = temp.path().join("claude-session.jsonl");
    std::fs::write(&claude_path, CLAUDE_FIXTURE)?;
    let codex_path = temp.path().join("codex-session.jsonl");
    std::fs::write(&codex_path, CODEX_FIXTURE)?;
    let mut catalog = HistoryCatalog::memory()?;

    let claude_meta = SessionMeta {
        key: "claude-code:session-a".to_string(),
        id: "session-a".to_string(),
        agent: AgentId::ClaudeCode,
        title: "t".to_string(),
        project_path: "/work".to_string(),
        project_name: "work".to_string(),
        file_path: claude_path.to_string_lossy().to_string(),
        created_at: 1,
        updated_at: 2,
        message_count: 1,
        size_bytes: 10,
        git_branch: None,
        model: None,
        tokens_used: None,
        archived: false,
        source: None,
    };
    catalog.write_session(&claude_meta, 0, &[])?;
    let codex_meta = SessionMeta {
        key: "codex:session-b".to_string(),
        id: "session-b".to_string(),
        agent: AgentId::Codex,
        title: "t".to_string(),
        project_path: "/work".to_string(),
        project_name: "work".to_string(),
        file_path: codex_path.to_string_lossy().to_string(),
        created_at: 1,
        updated_at: 2,
        message_count: 1,
        size_bytes: 10,
        git_branch: None,
        model: None,
        tokens_used: None,
        archived: false,
        source: None,
    };
    catalog.write_session(&codex_meta, 0, &[])?;

    let found = catalog
        .session_source_by_native(AgentId::ClaudeCode, "session-a")?
        .ok_or_else(|| anyhow::anyhow!("exact claude lookup must hit"))?;
    assert_eq!(found.file_path, claude_path.to_string_lossy());
    let found = catalog
        .session_source_by_native(AgentId::Codex, "session-b")?
        .ok_or_else(|| anyhow::anyhow!("exact codex lookup must hit"))?;
    assert_eq!(found.file_path, codex_path.to_string_lossy());
    // Exact-match semantics: any difference in provider or id must miss.
    assert!(catalog
        .session_source_by_native(AgentId::ClaudeCode, "session-b")?
        .is_none());
    assert!(catalog
        .session_source_by_native(AgentId::Codex, "session-a")?
        .is_none());
    assert!(catalog
        .session_source_by_native(AgentId::Pi, "session-a")?
        .is_none());
    Ok(())
}

#[test]
fn appended_bytes_only_are_decoded_on_normal_append() -> Result<()> {
    // Structured evidence for the normal-append path: sync feeds only
    // [consumed, size) to the decoder. The line count after append plus
    // snapshot prefix stability prove there is no full re-parse semantics:
    // appending one line at the tail of a long session keeps existing line
    // content pointer-stable identical (seq/content fully equal) and
    // lines_fed is exactly 1.
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("append-only.jsonl");
    std::fs::write(&path, CODEX_FIXTURE)?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::Codex))?;
    let before = session.snapshot();

    let extra = r#"{"timestamp":"2026-08-02T09:16:00Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"追加的一条"}]}}"#;
    append_chunk(&path, format!("{extra}\n").as_bytes())?;
    let sync = session.sync()?;
    assert_eq!(sync.change, LiveChange::Appended);
    assert_eq!(sync.lines_fed, 1, "append must decode exactly the new line");
    let after = session.snapshot();
    assert_eq!(after.messages.len(), before.messages.len() + 1);
    assert_eq!(
        &after.messages[..before.messages.len()],
        &before.messages[..]
    );
    assert_eq!(sync.appended, vec![before.messages.len()]);
    Ok(())
}

/// Reference implementation of delta application: idempotent upsert by index
/// (Batch C consumer-side contract).
fn upsert_message(
    messages: &mut Vec<Option<crate::models::TranscriptMessage>>,
    index: usize,
    message: crate::models::TranscriptMessage,
) {
    if messages.len() <= index {
        messages.resize(index + 1, None);
    }
    messages[index] = Some(message);
}

fn apply_sync(
    messages: &mut Vec<Option<crate::models::TranscriptMessage>>,
    sync: &super::LiveSync,
) {
    for (index, message) in sync.appended.iter().zip(&sync.appended_messages) {
        upsert_message(messages, *index, message.clone());
    }
    for (index, message) in sync.changed.iter().zip(&sync.changed_messages) {
        upsert_message(messages, *index, message.clone());
    }
}

/// Delta contract: the message sequence obtained by applying
/// appended/changed upserts batch by batch at arbitrary split points is
/// entry-for-entry identical to the full snapshot after settle (Batch A02's
/// equivalence evidence).
#[test]
fn delta_upserts_reconstruct_full_snapshot() -> Result<()> {
    for (agent, content) in [
        (AgentId::ClaudeCode, CLAUDE_FIXTURE),
        (AgentId::Codex, CODEX_FIXTURE),
        (AgentId::Pi, PI_FIXTURE),
        (AgentId::Kimi, KIMI_FIXTURE),
        (AgentId::Cursor, CURSOR_FIXTURE),
        (AgentId::CommandCode, CC_FIXTURE),
    ] {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("delta-parity.jsonl");
        let transcript = {
            std::fs::write(&path, content.as_bytes())?;
            expected_transcript(agent, &path)?
        };
        for cuts in cut_sets(content) {
            std::fs::write(&path, b"")?;
            let mut session = LiveSession::open(session_ref(&path, agent))?;
            let mut applied: Vec<Option<crate::models::TranscriptMessage>> = Vec::new();
            let bytes = content.as_bytes();
            let mut start = 0usize;
            for &cut in &cuts {
                let end = cut.min(bytes.len()).max(start);
                if end == start {
                    continue;
                }
                append_chunk(&path, &bytes[start..end])?;
                let sync = session.sync()?;
                assert_eq!(sync.change, LiveChange::Appended);
                assert_eq!(
                    sync.appended.len(),
                    sync.appended_messages.len(),
                    "appended indices and content must align"
                );
                assert_eq!(
                    sync.changed.len(),
                    sync.changed_messages.len(),
                    "changed indices and content must align"
                );
                apply_sync(&mut applied, &sync);
                start = end;
            }
            append_chunk(&path, &bytes[start..])?;
            apply_sync(&mut applied, &session.sync()?);
            apply_sync(&mut applied, &session.settle());
            let applied = applied
                .into_iter()
                .map(|slot| slot.ok_or_else(|| anyhow::anyhow!("delta left a gap")))
                .collect::<Result<Vec<_>>>()?;
            assert_eq!(
                applied, transcript.mainline,
                "agent {agent:?} cuts {cuts:?}"
            );
        }
    }
    Ok(())
}

/// Pending continuation contract: Claude's same-id assistant lines only
/// extend the tail pending projection and produce no decoder bookkeeping;
/// LiveSync must still deliver the grown content as a changed upsert.
#[test]
fn pending_streaming_growth_is_reported_as_tail_change() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("pending-growth.jsonl");
    let first = r#"{"type":"assistant","cwd":"/w","timestamp":"2026-08-01T01:00:00Z","message":{"id":"m1","content":[{"type":"text","text":"第一段"}]}}"#;
    let second = r#"{"type":"assistant","cwd":"/w","timestamp":"2026-08-01T01:00:01Z","message":{"id":"m1","content":[{"type":"text","text":"第二段"}]}}"#;
    std::fs::write(&path, b"")?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::ClaudeCode))?;

    append_chunk(&path, format!("{first}\n").as_bytes())?;
    let sync = session.sync()?;
    assert_eq!(sync.appended, vec![0]);
    assert_eq!(sync.appended_messages[0].text, "第一段");

    // Same-id continuation line: no appended bookkeeping, but the tail
    // projection content grew → changed [0].
    append_chunk(&path, format!("{second}\n").as_bytes())?;
    let sync = session.sync()?;
    assert!(
        sync.appended.is_empty(),
        "same-id continuation must not append"
    );
    assert_eq!(sync.changed, vec![0]);
    assert_eq!(sync.changed_messages[0].text, "第一段\n\n第二段");
    assert_eq!(session.snapshot().messages[0].text, "第一段\n\n第二段");

    // Duplicate wakes without new bytes stay idempotent.
    assert!(session.sync()?.is_unchanged());
    Ok(())
}

/// Reset contract: truncate/replace must deliver a complete new snapshot
/// (a full clone is only allowed on this path).
#[test]
fn reset_delivers_full_snapshot() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("reset-snapshot.jsonl");
    std::fs::write(&path, CLAUDE_FIXTURE)?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::ClaudeCode))?;

    let replacement = r#"{"type":"user","cwd":"/w","timestamp":"2026-08-01T02:00:00Z","message":{"content":"重置后的新会话"}}"#;
    std::fs::write(&path, format!("{replacement}\n"))?;
    let sync = session.sync()?;
    assert_eq!(sync.change, LiveChange::Reset);
    let snapshot = sync
        .snapshot
        .ok_or_else(|| anyhow::anyhow!("reset must carry the new snapshot"))?;
    assert_eq!(snapshot.messages.len(), 1);
    assert_eq!(snapshot.messages[0].text, "重置后的新会话");
    assert_eq!(session.snapshot(), snapshot);
    assert_eq!(session.generation(), 1);
    Ok(())
}

/// Initial delivery contract: after open pre-advances the bookkeeping,
/// initial_delivery must deliver the hydrated content in one batch; the next
/// sync returns to Unchanged ("stuck on Connecting" regression, root cause
/// confirmed in two acceptance rounds).
#[test]
fn initial_delivery_delivers_hydrated_content_exactly_once() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("initial-delivery.jsonl");
    std::fs::write(&path, CODEX_FIXTURE)?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::Codex))?;
    let hydrated = session.snapshot().messages.len();
    assert!(hydrated > 0);

    let initial = session.initial_delivery();
    assert_eq!(initial.change, LiveChange::Appended);
    assert_eq!(initial.appended.len(), hydrated);
    assert_eq!(initial.appended_messages.len(), hydrated);
    let applied = initial.appended_messages.to_vec();
    assert_eq!(applied, session.snapshot().messages);

    // The first sync (no new file content) must be Unchanged, and
    // initial_delivery has already advanced the bookkeeping to the full set
    // (no duplicate delivery).
    let repeat = session.initial_delivery();
    assert!(repeat.appended.is_empty());
    assert!(session.sync()?.is_unchanged());
    Ok(())
}

/// Kimi live contract (plan §7.2): steer enters the main timeline; known
/// config/loop_event lines are skipped; unknown counted; a missing trailing
/// newline is settled; duplicate wakes are idempotent; normal appends never
/// reset.
#[test]
fn kimi_wire_live_semantics() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("kimi-wire.jsonl");
    std::fs::write(&path, b"")?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::Kimi))?;

    // Append chunk by chunk (first 3 lines → the rest + no trailing newline),
    // covering partial-line holding and settle.
    let content = KIMI_FIXTURE;
    let bytes = content.as_bytes();
    let cut = content.find("\n").map(|pos| {
        content[pos + 1..]
            .find("\n")
            .map(|next| pos + 1 + next + 1)
            .unwrap_or(bytes.len())
    });
    let cut = cut.unwrap_or(bytes.len());
    append_chunk(&path, &bytes[..cut])?;
    let sync = session.sync()?;
    assert_eq!(sync.change, LiveChange::Appended);
    append_chunk(&path, &bytes[cut..])?;
    let sync = session.sync()?;
    // The tail line has no newline and is still held in the partial-line
    // buffer, so it is not counted in lines_fed (settle finishes it).
    assert_eq!(sync.lines_fed, content[cut..].matches('\n').count());
    apply_sync(&mut Vec::new(), &sync); // upsert alignment assertion (no panic = indices/content aligned)

    session.settle();
    let final_snapshot = session.snapshot();
    let reference = {
        std::fs::write(&path, content.as_bytes())?;
        expected_transcript(AgentId::Kimi, &path)?
    };
    assert_eq!(final_snapshot.messages, reference.mainline);
    assert_eq!(
        final_snapshot.facts.unknown_lines, reference.unknown_line_count,
        "wibble.record must count as unknown"
    );

    // Duplicate wake / duplicate settle are idempotent.
    assert!(session.sync()?.is_unchanged());
    assert!(session.settle().is_unchanged());

    // Normal append: zero resets, the steer appends as a user line.
    let steer = r#"{"type":"turn.steer","input":"live 追加的一条"}"#;
    append_chunk(&path, format!("{steer}\n").as_bytes())?;
    let sync = session.sync()?;
    assert_eq!(sync.change, LiveChange::Appended);
    assert_eq!(sync.appended_messages.len(), 1);
    assert_eq!(sync.appended_messages[0].role, Role::User);
    assert_eq!(sync.appended_messages[0].text, "live 追加的一条");
    Ok(())
}

/// Cursor live contract (plan §7.1): consecutive assistant lines merge across
/// syncs into one; the pending tail projection is first delivered as
/// appended, its streaming growth is reported as changed via the tail
/// signature, and turn_ended flush closes it with an upsert at the same
/// index; duplicate wakes are idempotent.
#[test]
fn cursor_transcript_live_merge_semantics() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("cursor-live.jsonl");
    std::fs::write(&path, b"")?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::Cursor))?;
    let line = |json: &str| format!("{json}\n");

    // chunk1: user line + assistant(tool_use) line. The pending is visible
    // immediately as the tail projection.
    append_chunk(
        &path,
        line(r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>Cursor 第一问</user_query>"}]}}"#).as_bytes(),
    )?;
    append_chunk(
        &path,
        line(r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"path":"src/main.rs"}}]}}"#).as_bytes(),
    )?;
    let sync = session.sync()?;
    assert_eq!(sync.appended, vec![0, 1]);
    assert_eq!(sync.appended_messages[1].tool_calls.len(), 1);
    assert_eq!(session.snapshot().messages.len(), 2);

    // chunk2: the assistant text line merges into the same pending → no
    // appended, but the tail projection content grew → the tail signature
    // reports it as changed [1].
    append_chunk(
        &path,
        line(r#"{"role":"assistant","message":{"content":[{"type":"text","text":"第一答（合并块）。"}]}}"#).as_bytes(),
    )?;
    let sync = session.sync()?;
    assert!(sync.appended.is_empty(), "merge must not append a new row");
    assert_eq!(sync.changed, vec![1]);
    assert_eq!(sync.changed_messages[0].text, "第一答（合并块）。");
    assert_eq!(sync.changed_messages[0].tool_calls.len(), 1);
    assert_eq!(session.snapshot().messages.len(), 2);

    // Duplicate wakes are idempotent.
    assert!(session.sync()?.is_unchanged());

    // chunk3: turn_ended flush → same-index upsert closes it out (content
    // matches the projection).
    append_chunk(&path, line(r#"{"type":"turn_ended"}"#).as_bytes())?;
    let sync = session.sync()?;
    assert_eq!(sync.appended, vec![1]);
    let messages = session.snapshot().messages;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].text, "第一答（合并块）。");
    assert_eq!(messages[1].tool_calls[0].name, "Read");
    Ok(())
}

/// Command Code live contract (plan §7.4): linear growth goes through
/// appended/changed; continuing after a rewind (a new entry pointing at a
/// historical node) triggers a wholesale projection replacement (Reset +
/// full snapshot; the visible sequence shrinks and forks); torn lines count
/// as unknown; linear appends continue after the replacement.
#[test]
fn command_code_lineage_rewind_replaces_projection() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("cc-lineage.jsonl");
    std::fs::write(&path, b"")?;
    let mut session = LiveSession::open(session_ref(&path, AgentId::CommandCode))?;
    let line = |json: &str| format!("{json}\n");

    let header = r#"{"type":"session","version":3,"id":"cc-lin","timestamp":"2026-08-27T01:00:00Z","cwd":"/work/cc"}"#;
    let user_q = |id: &str, parent: &str, text: &str| {
        format!(
            r#"{{"type":"message","id":"{id}","parentId":"{parent}","timestamp":"2026-08-27T01:00:01Z","message":{{"role":"user","content":[{{"type":"text","text":"{text}"}}]}}}}"#
        )
    };
    let asst = |id: &str, parent: &str, text: &str| {
        format!(
            r#"{{"type":"message","id":"{id}","parentId":"{parent}","timestamp":"2026-08-27T01:00:02Z","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
        )
    };

    // Linear: q1 → a1 → q2 → a2 (4 projected messages).
    append_chunk(&path, line(header).as_bytes())?;
    session.sync()?;
    for row in [
        user_q("m1", "", "问一"),
        asst("m2", "m1", "答一"),
        user_q("m3", "m2", "问二"),
        asst("m4", "m3", "答二"),
    ] {
        append_chunk(&path, line(&row).as_bytes())?;
        session.sync()?;
    }
    assert_eq!(session.snapshot().messages.len(), 4);
    assert_eq!(session.snapshot().messages[3].text, "答二");

    // Continue after a rewind: the new entry points at m2 (a mid-chain node,
    // after a1) → the projection is replaced with q1/a1/q3 (a2 and the prior
    // linear tail are discarded).
    append_chunk(
        &path,
        line(&user_q("m5", "m2", "问三（rewind 后）")).as_bytes(),
    )?;
    let sync = session.sync()?;
    assert_eq!(
        sync.change,
        LiveChange::Reset,
        "lineage change must replace"
    );
    let snapshot = sync
        .snapshot
        .ok_or_else(|| anyhow::anyhow!("projection replacement must carry snapshot"))?;
    let texts: Vec<&str> = snapshot.messages.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, vec!["问一", "答一", "问三（rewind 后）"]);
    assert_eq!(session.snapshot(), snapshot);

    // Linear appends continue after the replacement.
    append_chunk(&path, line(&asst("m6", "m5", "答三")).as_bytes())?;
    let sync = session.sync()?;
    assert_eq!(sync.change, LiveChange::Appended);
    assert_eq!(sync.appended_messages.len(), 1);
    assert_eq!(sync.appended_messages[0].text, "答三");

    // A torn line counts as unknown, producing no message.
    append_chunk(&path, b"{\"type\":\"torn\"}\n")?;
    session.sync()?;
    assert_eq!(session.snapshot().facts.unknown_lines, 1);
    Ok(())
}
