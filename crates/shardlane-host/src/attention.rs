//! AttentionPolicy：Agent 完成事件 → 注意力决策的纯策略层（去重 + 抑制）。
//!
//! [INPUT]: 依赖 sha2 的内容签名摘要；不依赖 runtime、socket、GUI 或文件 I/O。
//! [OUTPUT]: 对外提供 AgentCompletionEvent、completion_signature、AttentionDecision、
//!           evaluate、AttentionTracker——Activity 路由 / Agent 行未来消费
//!           NeedsAttention 时的唯一注意力仲裁入口。
//! [POS]: shardlane-host 事实层的策略模块，与 agent_usage 同层；无状态副作用
//!        （AttentionTracker 的内存记忆除外），v1 无系统通知，escalate 恒 false。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use std::collections::HashMap;

use sha2::{Digest, Sha256};

// ----------------------------------------------------------------------------
// 内容签名
// ----------------------------------------------------------------------------

/// 完成事件的内容签名：sha256 摘要的前 8 字节（大端 u64）。
///
/// 与 Moshi lastStopSignature 同型：同一 agent 连续两次相同完成输出产生
/// 相同签名，是去重规则（规则 1）的唯一依据。签名只绑定内容，不绑定时间。
pub fn completion_signature(content: &str) -> u64 {
    let digest = Sha256::digest(content.as_bytes());
    let mut prefix = [0u8; 8];
    prefix.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(prefix)
}

/// 一次 Agent 完成事件的事实切片：(session_id, 内容签名) 即去重键。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentCompletionEvent {
    /// Agent 的稳定会话身份（Herdr session / provider session id）。
    pub session_id: String,
    /// 完成内容签名（见 completion_signature）。
    pub signature: u64,
}

impl AgentCompletionEvent {
    /// 从完成文本构造事件；签名由内容派生，调用方无需自行哈希。
    pub fn new(session_id: impl Into<String>, content: &str) -> Self {
        Self {
            session_id: session_id.into(),
            signature: completion_signature(content),
        }
    }
}

// ----------------------------------------------------------------------------
// 决策
// ----------------------------------------------------------------------------

/// 注意力决策：徽标与升级两个独立通道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttentionDecision {
    /// Activity / Agent 行徽标（NeedsAttention 轴）。
    pub badge: bool,
    /// 未来系统 / mobile 推送挂点。v1 无系统通知，恒 false；
    /// 开启时应要求 !dup && !parent_alive && !focused_same_agent。
    pub escalate: bool,
}

/// 纯函数评估一次完成事件是否值得用户注意。
///
/// 三条规则（全部可独立单测）：
/// 1. 去重：签名与上次相同 → 抑制（徽标与升级都不产生）。
/// 2. 嵌套抑制：parent_alive（活父存在）→ 不产生 NeedsAttention，只入时间线。
/// 3. 活跃静默：focused_same_agent（窗口聚焦且正选中该 agent）→ 徽标保留、
///    仅静默未来的升级通道（“只徽标，不抢焦点”）。
pub fn evaluate(
    event: &AgentCompletionEvent,
    last_signature: Option<u64>,
    parent_alive: bool,
    // v1 只静默升级通道（escalate 恒 false），决策值本身不消费该输入；
    // 保留参数以固定策略签名，升级通道开启时即由此参与判定。
    _focused_same_agent: bool,
) -> AttentionDecision {
    // 规则 1：同签名重复完成不重复打扰。
    let dup = last_signature == Some(event.signature);
    // 规则 2：子 agent 完成只入时间线，不升级为用户注意力。
    let badge = !dup && !parent_alive;
    AttentionDecision {
        badge,
        escalate: false,
    }
}

// ----------------------------------------------------------------------------
// 每会话记忆
// ----------------------------------------------------------------------------

/// 每会话的 last-signature 记忆，是规则 1 的唯一状态持有者。
///
/// 规模有界：每个活跃 agent 会话各占一条；会话结束即随调用方丢弃，
/// 不做持久化（去重是会话内语义，跨重启重复提醒是可接受的 v1 行为）。
#[derive(Debug, Default)]
pub struct AttentionTracker {
    last_signature_by_session: HashMap<String, u64>,
}

impl AttentionTracker {
    /// 评估并更新记忆：无论决策如何，都记录本次签名（重复签名反复到达
    /// 时保持抑制，新签名到达时替换记忆）。
    pub fn evaluate(
        &mut self,
        event: &AgentCompletionEvent,
        parent_alive: bool,
        focused_same_agent: bool,
    ) -> AttentionDecision {
        let last = self
            .last_signature_by_session
            .get(&event.session_id)
            .copied();
        let decision = evaluate(event, last, parent_alive, focused_same_agent);
        self.last_signature_by_session
            .insert(event.session_id.clone(), event.signature);
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "sess-1";

    fn event(signature: u64) -> AgentCompletionEvent {
        AgentCompletionEvent {
            session_id: SESSION.into(),
            signature,
        }
    }

    #[test]
    fn completion_signature_is_stable_and_content_bound() {
        let first = completion_signature("done: 3 files changed");
        assert_eq!(first, completion_signature("done: 3 files changed"));
        assert_ne!(first, completion_signature("done: 4 files changed"));
    }

    #[test]
    fn rule1_dedup_suppresses_same_signature_only() {
        let sig = completion_signature("done");
        // 同签名重复 → 抑制。
        assert!(!evaluate(&event(sig), Some(sig), false, false).badge);
        // 新签名 → 徽标。
        assert!(evaluate(&event(sig + 1), Some(sig), false, false).badge);
        // 首次完成（无记忆）→ 徽标。
        assert!(evaluate(&event(sig), None, false, false).badge);
    }

    #[test]
    fn rule2_parent_alive_suppresses_badge() {
        let sig = completion_signature("child done");
        assert!(!evaluate(&event(sig), None, true, false).badge);
    }

    #[test]
    fn rule3_focused_keeps_badge_and_only_guards_escalation() {
        // “只徽标，不抢焦点”：聚焦同一 agent 时徽标保留，升级通道静默。
        let decision = evaluate(&event(completion_signature("done")), None, false, true);
        assert!(decision.badge);
        assert!(!decision.escalate);
    }

    #[test]
    fn escalate_is_always_false_in_v1() {
        let sig = completion_signature("done");
        for last in [None, Some(sig), Some(sig + 1)] {
            for parent in [false, true] {
                for focused in [false, true] {
                    assert!(!evaluate(&event(sig), last, parent, focused).escalate);
                }
            }
        }
    }

    #[test]
    fn combined_suppression_wins() {
        let sig = completion_signature("done");
        // 重复 + 活父 + 聚焦：全抑制面叠加时徽标仍为 false。
        assert!(!evaluate(&event(sig), Some(sig), true, true).badge);
    }

    #[test]
    fn tracker_dedups_per_session_and_updates_memory() {
        let mut tracker = AttentionTracker::default();
        let first = AgentCompletionEvent::new(SESSION, "done: 3 files");
        assert!(tracker.evaluate(&first, false, false).badge);
        // 完全相同的完成 → 第二次抑制。
        assert!(!tracker.evaluate(&first, false, false).badge);
        // 新内容 → 徽标恢复。
        let revised = AgentCompletionEvent::new(SESSION, "done: 4 files");
        assert!(tracker.evaluate(&revised, false, false).badge);
    }

    #[test]
    fn tracker_keys_are_session_scoped() {
        // 去重键是 (session_id, 签名)：不同会话的相同签名互不抑制。
        let mut tracker = AttentionTracker::default();
        let a = AgentCompletionEvent::new("sess-a", "done");
        let b = AgentCompletionEvent::new("sess-b", "done");
        assert!(tracker.evaluate(&a, false, false).badge);
        assert!(tracker.evaluate(&b, false, false).badge);
    }
}
