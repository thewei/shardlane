// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

//! Shardlane Hook Journal decoder: the provider-neutral live semantic source
//! behind `LiveCapability::HookJournal`.
//!
//! [INPUT]: normalized journal records appended by the Host hook adapter
//! (`shardlane-host` `agent_hooks::adapter`); one JSON object per line.
//! [OUTPUT]: `JournalSession` decoder state turning journal lines into
//! `TranscriptMessage`s through the shared `LiveDecoderState` seam.
//! [POS]: live/ 的 provider 中立解码器：与 per-provider 文件解码器并列，
//! 由 capability registry 的 `HookJournal` 档位选中；它只解释 Shardlane
//! 自有 journal 格式，永远不读 provider 原生文件、不做 TUI/ANSI 推断。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use crate::live::{DecoderUpdate, LiveDecoderState, LiveFacts};
use crate::models::{MessageKind, Role, TranscriptMessage, UNTITLED};
use serde::Deserialize;

/// One journal record. The wire format is versioned (`v`); unknown versions
/// or unknown event kinds count as unknown lines instead of guessing.
#[derive(Debug, Clone, Deserialize)]
struct JournalRecord {
    v: u32,
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    ts: i64,
    #[serde(default)]
    session: Option<String>,
    cwd: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

const JOURNAL_VERSION: u32 = 1;

/// Decoder state for the Shardlane Hook Journal (append-only JSONL written
/// by the Host hook adapter; tailed by the stock `AppendLogTransport`).
#[derive(Default)]
pub(crate) struct JournalSession {
    messages: Vec<TranscriptMessage>,
    update: DecoderUpdate,
    unknown_lines: u32,
    session_id: Option<String>,
    cwd: String,
    updated_at: i64,
}

impl JournalSession {
    pub(crate) fn fresh() -> Self {
        Self::default()
    }

    fn apply(&mut self, record: JournalRecord) {
        if record.v != JOURNAL_VERSION {
            self.unknown_lines = self.unknown_lines.saturating_add(1);
            return;
        }
        match record.kind.as_str() {
            // 会话元数据类事件：只更新 facts，不产生消息。
            "session_start" | "turn_complete" | "status" => {
                if record.session.is_some() {
                    self.session_id = record.session;
                }
                if let Some(cwd) = record.cwd {
                    if !cwd.is_empty() {
                        self.cwd = cwd;
                    }
                }
                self.updated_at = record.ts;
            }
            "user_prompt" => {
                if record.text.is_empty() {
                    self.unknown_lines = self.unknown_lines.saturating_add(1);
                    return;
                }
                if record.session.is_some() {
                    self.session_id = record.session;
                }
                let index = self.messages.len();
                self.messages.push(TranscriptMessage {
                    seq: index as i64 + 1,
                    role: Role::User,
                    kind: MessageKind::Text,
                    text: record.text,
                    truncated: false,
                    tool_calls: Vec::new(),
                    thinking: None,
                    timestamp: nonzero(record.ts),
                    model: None,
                });
                self.update.appended.push(index);
                self.updated_at = record.ts;
            }
            "assistant_message" => {
                if record.text.is_empty() {
                    self.unknown_lines = self.unknown_lines.saturating_add(1);
                    return;
                }
                if record.session.is_some() {
                    self.session_id = record.session;
                }
                let index = self.messages.len();
                self.messages.push(TranscriptMessage {
                    seq: index as i64 + 1,
                    role: Role::Assistant,
                    kind: MessageKind::Text,
                    text: record.text,
                    truncated: false,
                    tool_calls: Vec::new(),
                    thinking: None,
                    timestamp: nonzero(record.ts),
                    model: record.model,
                });
                self.update.appended.push(index);
                self.updated_at = record.ts;
            }
            // 未来的 tool/permission 等事件：先按 unknown 计数保证
            // fail-closed，等 Host 侧格式稳定后再升级为消息投影。
            _ => {
                self.unknown_lines = self.unknown_lines.saturating_add(1);
            }
        }
    }
}

fn nonzero(ts: i64) -> Option<i64> {
    (ts > 0).then_some(ts)
}

impl LiveDecoderState for JournalSession {
    fn fresh() -> Self
    where
        Self: Sized,
    {
        Self::default()
    }

    fn feed_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        match serde_json::from_str::<JournalRecord>(trimmed) {
            Ok(record) => self.apply(record),
            Err(_) => {
                self.unknown_lines = self.unknown_lines.saturating_add(1);
            }
        }
    }

    fn count_unknown_line(&mut self) {
        self.unknown_lines = self.unknown_lines.saturating_add(1);
    }

    fn snapshot_messages(&self) -> Vec<TranscriptMessage> {
        self.messages.clone()
    }

    fn take_update(&mut self) -> DecoderUpdate {
        std::mem::take(&mut self.update)
    }

    fn facts(&self) -> LiveFacts {
        LiveFacts {
            title: crate::adapters::parse_utils::title_from_messages(&self.messages)
                .unwrap_or_else(|| UNTITLED.to_string()),
            cwd: self.cwd.clone(),
            git_branch: None,
            model: None,
            source: Some("shardlane-hook-journal".to_string()),
            tokens_used: 0,
            created_at: 0,
            updated_at: self.updated_at,
            unknown_lines: self.unknown_lines,
            session_id: self.session_id.clone(),
        }
    }

    fn message_count(&self) -> usize {
        self.messages.len()
    }

    fn message_at(&self, index: usize) -> Option<TranscriptMessage> {
        self.messages.get(index).cloned()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn feed_all(session: &mut JournalSession, lines: &[&str]) {
        for line in lines {
            session.feed_line(line);
        }
    }

    #[test]
    fn journal_records_project_to_user_and_assistant_messages() {
        let mut session = JournalSession::fresh();
        feed_all(
            &mut session,
            &[
                r#"{"v":1,"kind":"session_start","ts":100,"session":"s-1","cwd":"/tmp/p"}"#,
                r#"{"v":1,"kind":"user_prompt","text":"帮我看下这个 bug","ts":101}"#,
                r#"{"v":1,"kind":"assistant_message","text":"我先看看日志","ts":102,"model":"gemini-3-pro"}"#,
            ],
        );
        let messages = session.snapshot_messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].text, "帮我看下这个 bug");
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[1].model.as_deref(), Some("gemini-3-pro"));
        let facts = session.facts();
        assert_eq!(facts.cwd, "/tmp/p");
        assert_eq!(facts.session_id.as_deref(), Some("s-1"));
        assert_eq!(facts.source.as_deref(), Some("shardlane-hook-journal"));
        assert_eq!(session.message_count(), 2);
        assert!(session.message_at(1).is_some());
    }

    #[test]
    fn unknown_versions_and_kinds_fail_closed_as_unknown_lines() {
        let mut session = JournalSession::fresh();
        feed_all(
            &mut session,
            &[
                r#"{"v":2,"kind":"user_prompt","text":"future"}"#,
                r#"{"v":1,"kind":"tool_started","text":"whatever"}"#,
                "not-json",
                "",
            ],
        );
        assert_eq!(session.snapshot_messages().len(), 0);
        assert_eq!(session.facts().unknown_lines, 3);
    }

    #[test]
    fn empty_payload_events_count_as_unknown_not_as_messages() {
        let mut session = JournalSession::fresh();
        feed_all(
            &mut session,
            &[
                r#"{"v":1,"kind":"user_prompt","text":""}"#,
                r#"{"v":1,"kind":"assistant_message"}"#,
            ],
        );
        assert_eq!(session.snapshot_messages().len(), 0);
        assert_eq!(session.facts().unknown_lines, 2);
    }
}
