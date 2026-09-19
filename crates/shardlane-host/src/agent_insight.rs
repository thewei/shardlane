//! Host Agent Session Insight projection (M8).
//!
//! [INPUT]: exact session facts — LiveSnapshot (reusing the live decoder)
//! or a History SessionMeta — plus optional Provider allowance facts and
//! agent_usage 的 AccountUsage/UsageWindow 窗口事实.
//! [OUTPUT]: AgentSessionInsight — the bounded fact projection shared by
//! the Chat HUD / Sidebar pressure signals: model, context/tokens,
//! message/turn/tool counts, session duration, freshness/compact state,
//! and provider usage windows; allowance_from_usage + with_usage 是
//! with_allowance 的唯一调用路径（provider 可证窗口事实 → allowance）。
//! Unknown fields stay None; never guessed.
//! [POS]: plan M8 / audit AF-25. No second full provider parser is added;
//! the rendering layer does zero I/O; there is no token dashboard page.

use crate::agent_usage::{AccountUsage, UsageWindow};
use shardlane_history::models::{MessageKind, SessionMeta, TranscriptMessage};
use shardlane_history::LiveSnapshot;

/// Bounded insight into one Agent session. Built from existing parser facts;
/// every field is optional because providers differ in what they report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentSessionInsight {
    pub model: Option<String>,
    /// Provider-reported token usage when authoritative.
    pub tokens_used: Option<i64>,
    /// Provider-reported context window percentage used (0.0 ..= 100.0).
    pub context_used_percent: Option<f32>,
    /// Cumulative cache hit rate percentage for session (0.0 ..= 100.0).
    pub cache_hit_percent: Option<f32>,
    /// Approximate cache TTL estimate in seconds if available.
    pub cache_ttl_secs: Option<u64>,
    pub message_count: u64,
    pub turn_count: u64,
    pub tool_call_count: u64,
    /// Session duration in milliseconds derived from first/last message
    /// timestamps when both are known.
    pub session_duration_ms: Option<u64>,
    /// Last activity time (epoch ms) when known.
    pub last_activity_ms: Option<u64>,
    /// The transcript contains a compaction boundary: context was summarized.
    pub compacted: bool,
    /// The source has not changed for a long window while the Agent idles
    /// (staleness pressure hint).
    pub stale: bool,
    /// Provider allowance facts when authoritative (e.g. limit windows).
    pub allowance: Option<ProviderAllowance>,
    /// Provider 用量窗口事实（agent_usage 聚合器供数）；空 = 未暴露。
    pub usage_windows: Vec<UsageWindow>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProviderAllowance {
    pub used: i64,
    pub limit: i64,
    /// Window label as reported by the provider (never invented).
    pub window: Option<String>,
}

impl ProviderAllowance {
    /// Fraction of the allowance consumed, when a positive limit is known.
    pub fn used_fraction(&self) -> Option<f64> {
        if self.limit <= 0 {
            return None;
        }
        Some(self.used as f64 / self.limit as f64)
    }

    pub fn critical(&self) -> bool {
        self.used_fraction().is_some_and(|fraction| fraction >= 0.9)
    }
}

/// Context pressure classification shared by the HUD and Sidebar micro-signal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InsightPressure {
    None,
    HighContext,
    CriticalAllowance,
    Stale,
}

impl AgentSessionInsight {
    /// The single most actionable pressure signal, or `None` when healthy.
    /// At most one is ever surfaced per Agent row (plan §M8.3).
    pub fn pressure(&self) -> InsightPressure {
        if self
            .context_used_percent
            .is_some_and(|fraction| fraction >= 85.0)
        {
            return InsightPressure::HighContext;
        }
        if self
            .allowance
            .as_ref()
            .is_some_and(ProviderAllowance::critical)
        {
            return InsightPressure::CriticalAllowance;
        }
        if self.stale {
            return InsightPressure::Stale;
        }
        InsightPressure::None
    }

    /// Build the insight from a live snapshot's existing decoded facts.
    pub fn from_live_snapshot(snapshot: &LiveSnapshot, now_ms: u64, stale_after_ms: u64) -> Self {
        let mut insight = count_messages(&snapshot.messages);
        insight.model = snapshot
            .facts
            .model
            .clone()
            .filter(|model| !model.is_empty());
        insight.tokens_used =
            (snapshot.facts.tokens_used > 0).then_some(snapshot.facts.tokens_used);
        insight.last_activity_ms = (snapshot.facts.updated_at > 0)
            .then_some(u64::try_from(snapshot.facts.updated_at).unwrap_or(0));
        insight.compacted = snapshot
            .messages
            .iter()
            .any(|message| message.kind == MessageKind::CompactSummary);
        insight.stale = insight
            .last_activity_ms
            .is_some_and(|last| now_ms.saturating_sub(last) > stale_after_ms);
        insight
    }

    /// Build the insight from indexed History metadata (no transcript parse).
    pub fn from_history_meta(meta: &SessionMeta, now_ms: u64, stale_after_ms: u64) -> Self {
        Self {
            model: meta.model.clone(),
            tokens_used: meta.tokens_used,
            context_used_percent: None,
            cache_hit_percent: None,
            cache_ttl_secs: None,
            message_count: meta.message_count.max(0) as u64,
            turn_count: 0,
            tool_call_count: 0,
            session_duration_ms: None,
            last_activity_ms: u64::try_from(meta.updated_at.max(0)).ok(),
            compacted: false,
            stale: u64::try_from(meta.updated_at.max(0))
                .ok()
                .is_some_and(|updated| now_ms.saturating_sub(updated) > stale_after_ms),
            allowance: None,
            usage_windows: Vec::new(),
        }
    }

    /// Enrich with provider allowance facts when the caller has an
    /// authoritative source (bounded background collector).
    pub fn with_allowance(mut self, allowance: ProviderAllowance) -> Self {
        self.allowance = Some(allowance);
        self
    }

    /// with_allowance 的调用路径：从 agent_usage 聚合行填充窗口事实与
    /// allowance。仅 provider 可证窗口参与；无窗口时 allowance 保持
    /// None（Unknown stays None; never guessed）。
    pub fn with_usage(mut self, usage: &AccountUsage) -> Self {
        self.usage_windows = usage.windows.clone();
        if let Some(allowance) = allowance_from_usage(&usage.windows) {
            self.allowance = Some(allowance);
        }
        self
    }
}

/// provider 窗口事实 → 有界 allowance：取用量最高的窗口作为单一
/// 压力信号（used = used_percentage, limit = 100, window = provider 原文）。
pub fn allowance_from_usage(windows: &[UsageWindow]) -> Option<ProviderAllowance> {
    windows
        .iter()
        .max_by_key(|window| window.used_percentage)
        .map(|window| ProviderAllowance {
            used: i64::from(window.used_percentage),
            limit: 100,
            window: Some(window.label.clone()),
        })
}

fn count_messages(messages: &[TranscriptMessage]) -> AgentSessionInsight {
    let mut insight = AgentSessionInsight::default();
    let mut first_ts: Option<i64> = None;
    let mut last_ts: Option<i64> = None;
    let mut turns: u64 = 0;
    let mut in_user_turn = false;
    for message in messages {
        insight.message_count += 1;
        insight.tool_call_count += message.tool_calls.len() as u64;
        if message
            .timestamp
            .is_some_and(|timestamp| first_ts.is_none_or(|current| timestamp < current))
        {
            first_ts = message.timestamp;
        }
        if let Some(timestamp) = message.timestamp {
            if last_ts.is_none_or(|last| timestamp > last) {
                last_ts = Some(timestamp);
            }
        }
        // A turn starts at each user message that follows assistant output.
        let is_user = matches!(message.kind, MessageKind::Text)
            && matches!(message.role, shardlane_history::Role::User);
        if is_user && !in_user_turn {
            turns += 1;
        }
        in_user_turn = is_user;
    }
    insight.turn_count = turns;
    if let (Some(first), Some(last)) = (first_ts, last_ts) {
        if last >= first {
            // TranscriptMessage timestamps are already epoch milliseconds
            // (parse_utils `to_epoch_ms`); the old `* 1_000` inflated every
            // duration a thousandfold (C01).
            insight.session_duration_ms = u64::try_from(last - first).ok();
        }
    }
    insight
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use shardlane_history::models::Role;

    fn message(seq: i64, role: Role, text: &str, timestamp: i64) -> TranscriptMessage {
        TranscriptMessage {
            seq,
            role,
            kind: MessageKind::Text,
            text: text.into(),
            truncated: false,
            tool_calls: Vec::new(),
            thinking: None,
            timestamp: Some(timestamp),
            model: Some("model-x".into()),
        }
    }

    fn snapshot(messages: Vec<TranscriptMessage>) -> LiveSnapshot {
        LiveSnapshot {
            messages,
            facts: shardlane_history::LiveFacts {
                title: "T".into(),
                cwd: "/work".into(),
                git_branch: Some("main".into()),
                model: Some("claude-sonnet".into()),
                source: None,
                tokens_used: 42,
                created_at: 0,
                updated_at: 1_000,
                unknown_lines: 0,
                session_id: Some("s1".into()),
                pending_approval: None,
            },
            generation: 3,
        }
    }

    #[test]
    fn live_facts_flow_into_the_insight() {
        // Realistic epoch-ms fixtures (C01): duration derives directly from
        // the millisecond timestamps, never scaled again.
        let messages = vec![
            message(1, Role::User, "first", 1_700_000_000_000),
            message(2, Role::Assistant, "answer", 1_700_000_000_100),
            TranscriptMessage {
                seq: 3,
                role: Role::Assistant,
                kind: MessageKind::CompactSummary,
                text: "compacted".into(),
                truncated: false,
                tool_calls: Vec::new(),
                thinking: None,
                timestamp: Some(1_700_000_000_200),
                model: None,
            },
            message(4, Role::User, "second", 1_700_000_000_300),
        ];
        let insight = AgentSessionInsight::from_live_snapshot(&snapshot(messages), 2_000, 60_000);
        assert_eq!(insight.model.as_deref(), Some("claude-sonnet"));
        assert_eq!(insight.tokens_used, Some(42));
        assert_eq!(insight.message_count, 4);
        assert_eq!(insight.turn_count, 2);
        assert_eq!(insight.tool_call_count, 0);
        assert_eq!(insight.session_duration_ms, Some(300));
        assert_eq!(insight.last_activity_ms, Some(1_000));
        assert!(insight.compacted);
        assert!(!insight.stale);
        assert_eq!(insight.pressure(), InsightPressure::None);
    }

    #[test]
    fn stale_sessions_surface_pressure() {
        let insight = AgentSessionInsight::from_live_snapshot(
            &snapshot(vec![message(1, Role::User, "old", 1)]),
            10 * 60 * 1_000,
            60_000,
        );
        assert!(insight.stale);
        assert_eq!(insight.pressure(), InsightPressure::Stale);
    }

    #[test]
    fn high_context_surfaces_pressure() {
        let insight = AgentSessionInsight {
            context_used_percent: Some(88.5),
            stale: true,
            ..AgentSessionInsight::default()
        };
        assert_eq!(insight.pressure(), InsightPressure::HighContext);
    }

    #[test]
    fn critical_allowance_outranks_other_pressure() {
        let insight = AgentSessionInsight {
            stale: true,
            allowance: Some(ProviderAllowance {
                used: 95,
                limit: 100,
                window: Some("5h".into()),
            }),
            ..AgentSessionInsight::default()
        };
        assert_eq!(insight.pressure(), InsightPressure::CriticalAllowance);
        let allowance = ProviderAllowance {
            used: 95,
            limit: 100,
            window: None,
        };
        assert!(allowance.critical());
        let unlimited = ProviderAllowance {
            used: 1_000,
            limit: 0,
            window: None,
        };
        assert!(!unlimited.critical());
    }

    #[test]
    fn allowance_from_usage_picks_the_highest_window() {
        let windows = vec![
            UsageWindow {
                label: "7d".into(),
                used_percentage: 8,
                resets_at: None,
            },
            UsageWindow {
                label: "5h".into(),
                used_percentage: 51,
                resets_at: Some(1_758_240_000),
            },
        ];
        let allowance = allowance_from_usage(&windows).expect("windows exist");
        assert_eq!(allowance.used, 51);
        assert_eq!(allowance.limit, 100);
        assert_eq!(allowance.window.as_deref(), Some("5h"));
        assert!(!allowance.critical());
        assert!(allowance_from_usage(&[]).is_none());
    }

    #[test]
    fn with_usage_fills_windows_and_allowance_only_from_provable_windows() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 9, 19).expect("valid date");
        let usage = AccountUsage {
            provider: shardlane_history::AgentId::Codex,
            account_id: None,
            label_masked: "codex:1111…".into(),
            day,
            tokens_used: 3_800_000,
            windows: vec![UsageWindow {
                label: "5h".into(),
                used_percentage: 51,
                resets_at: None,
            }],
        };
        let insight = AgentSessionInsight::default().with_usage(&usage);
        assert_eq!(insight.usage_windows.len(), 1);
        assert_eq!(insight.allowance.map(|a| a.used), Some(51));
        // 窗口缺省路径：无窗口 → usage_windows 空且 allowance 保持 None。
        let tokens_only = AccountUsage {
            windows: Vec::new(),
            ..usage
        };
        let insight = AgentSessionInsight::default().with_usage(&tokens_only);
        assert!(insight.usage_windows.is_empty());
        assert!(insight.allowance.is_none());
    }

    #[test]
    fn tool_calls_are_counted_from_existing_parsed_facts() {
        let mut tool_message = message(2, Role::Assistant, "run", 10);
        tool_message.tool_calls = vec![shardlane_history::models::ToolCallView {
            id: "t1".into(),
            name: "shell".into(),
            input_preview: "ls".into(),
            input: None,
            output: None,
            is_error: false,
            sidechain_ref: None,
        }];
        let insight = AgentSessionInsight::from_live_snapshot(
            &snapshot(vec![message(1, Role::User, "go", 1), tool_message]),
            0,
            60_000,
        );
        assert_eq!(insight.tool_call_count, 1);
        assert_eq!(insight.turn_count, 1);
    }

    #[test]
    fn history_meta_projection_stays_bounded() {
        let meta = SessionMeta {
            key: "claude-code:s".into(),
            id: "s".into(),
            agent: shardlane_history::AgentId::ClaudeCode,
            title: "T".into(),
            project_path: "/work".into(),
            project_name: "work".into(),
            file_path: "/tmp/s.jsonl".into(),
            created_at: 0,
            updated_at: 5_000,
            message_count: 12,
            size_bytes: 100,
            git_branch: None,
            model: Some("m".into()),
            tokens_used: Some(7),
            archived: false,
            source: None,
        };
        let insight = AgentSessionInsight::from_history_meta(&meta, 6_000, 60_000);
        assert_eq!(insight.message_count, 12);
        assert_eq!(insight.tokens_used, Some(7));
        assert_eq!(insight.model.as_deref(), Some("m"));
        assert_eq!(insight.last_activity_ms, Some(5_000));
        assert!(!insight.stale);
    }
}
