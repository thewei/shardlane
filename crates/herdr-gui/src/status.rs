//! Shared operational status/attention semantics for Shardlane Activity.
//!
//! Pure domain module: no GPUI, no render, no IO. All types and functions are
//! testable independently. UI rendering consumes these primitives from a
//! separate presentation layer.
//!
//! [INPUT]: Agent/Script/Service runtime state projections from herdr.rs and scripts/model.rs
//! [OUTPUT]: AttentionLevel + classify_agent_transition — shared status
//!           primitives consumed by Sidebar, Header, notifications, and the
//!           client unread/review markers (AttentionSummary/aggregate_attention
//!           were removed along with the Activity panel, notate 2026-08-29;
//!           classify_agent_transition returned 2026-09-19 for the Agent
//!           unread/review flow)
//! [POS]: Shared domain layer below UI; no GPUI dependency

use crate::herdr::Agent;
use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{div, px, AnyElement, App, ElementId, Hsla, IntoElement, Styled};
use gpui_component::{ActiveTheme as _, Icon, IconName as ComponentIconName, Sizable as _};

/// Operational attention level for any monitored entity.
/// Explicit priority values make product semantics reviewable;
/// do not rely on enum declaration order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum AttentionLevel {
    Idle,
    ReadyForReview,
    Working,
    NeedsAttention,
}

/// ACT-07: the agent's effective status string (falls back to custom_status when
/// agent_status is absent), shared by operational_summary / status bar derivations.
pub(crate) fn agent_effective_status(agent: &Agent) -> Option<&str> {
    agent
        .agent_status
        .as_deref()
        .or(agent.custom_status.as_deref())
}

/// Map a raw Herdr status string (from agent/script projections) to AttentionLevel.
/// Used by Sidebar rows for compact glyph display.
pub(crate) fn attention_for_raw_status(status: &str) -> AttentionLevel {
    match status {
        "blocked" | "failed" => AttentionLevel::NeedsAttention,
        "launch_pending" | "working" | "starting" | "running" => AttentionLevel::Working,
        "done" => AttentionLevel::ReadyForReview,
        _ => AttentionLevel::Idle,
    }
}

/// Result of classifying one Agent status transition. Client-owned presentation
/// semantics only: nothing here drives runtime behavior, and every flag is
/// derived from the same (previous, next) pair so all consumers stay consistent.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct AgentTransition {
    /// The Agent entered a state that wants the user to look at it
    /// (done / blocked / failed) — drives the unread marker when the
    /// Agent was not on screen at that moment.
    pub(crate) attention_started: bool,
    /// The Agent entered `done` — starts the review-pending window
    /// ("finished, waiting for the user's review").
    pub(crate) review_started: bool,
    /// The Agent left `done` for anything other than another terminal state
    /// (typically a new `working` turn) — the previous review is moot.
    pub(crate) review_cleared: bool,
}

const ATTENTION_RAW_STATUSES: [&str; 3] = ["done", "blocked", "failed"];

/// Classify one Agent status transition into client presentation events.
/// A transition only exists when both sides are known and different.
pub(crate) fn classify_agent_transition(
    previous: Option<&str>,
    next: Option<&str>,
) -> AgentTransition {
    let (Some(previous), Some(next)) = (previous, next) else {
        return AgentTransition::default();
    };
    if previous == next {
        return AgentTransition::default();
    }
    let was_done = previous == "done";
    let next_done = next == "done";
    AgentTransition {
        attention_started: ATTENTION_RAW_STATUSES.contains(&next),
        review_started: next_done,
        review_cleared: was_done && !next_done,
    }
}

// --- Rendering primitives (GPUI presentation layer) ---

const GLYPH_SIZE: f32 = 14.0;
const DOT_SIZE: f32 = 6.0;

impl AttentionLevel {
    pub(crate) fn color(self, cx: &App) -> Hsla {
        let theme = cx.theme();
        match self {
            Self::NeedsAttention => theme.danger,
            Self::Working => theme.primary,
            Self::ReadyForReview => theme.success,
            Self::Idle => theme.muted_foreground,
        }
    }

    /// The single live raw-status → display-text mapping (audit A20): the status bar menu and
    /// the Header's Chat title both label agents through `attention_for_raw_status(...).label()`,
    /// so a level's wording can only exist here.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::ReadyForReview => "Done",
            Self::Working => "Working",
            Self::NeedsAttention => "Needs Attention",
        }
    }
}

/// Compact fixed-footprint status glyph for Sidebar rows and Header.
///
/// - Working: static loader icon in primary color
/// - NeedsAttention: stable dot with attention color
/// - ReadyForReview: distinct ring/dot with success color
/// - Idle: muted dot
///
/// R0 performance containment (2026-08-26 audit): gpui-component `Spinner`
/// drives a window-level `Animation`, and every animation frame rebuilds the
/// entire window element tree. Working spinners on persistent surfaces
/// (Sidebar rows, Header) were measured at ~96.5% CPU with 287/300 recent
/// lag-log lines being root renders. Until GPUI gains partial redraw, Working
/// is always a static icon; state semantics live in color.
pub(crate) fn status_glyph(
    _id: impl Into<ElementId>,
    level: AttentionLevel,
    cx: &App,
) -> AnyElement {
    let color = level.color(cx);

    match level {
        AttentionLevel::Working => Icon::new(ComponentIconName::LoaderCircle)
            .xsmall()
            .text_color(color)
            .into_any_element(),
        AttentionLevel::NeedsAttention => Icon::new(ComponentIconName::TriangleAlert)
            .xsmall()
            .text_color(color)
            .into_any_element(),
        AttentionLevel::ReadyForReview => Icon::new(ComponentIconName::CircleCheck)
            .xsmall()
            .text_color(color)
            .into_any_element(),
        AttentionLevel::Idle => div()
            .size(px(DOT_SIZE))
            .rounded(px(DOT_SIZE / 2.0))
            .bg(color)
            .flex_shrink_0()
            .into_any_element(),
    }
}

/// Fixed container for glyph to prevent row width jitter.
pub(crate) fn status_glyph_container(
    id: impl Into<ElementId>,
    level: AttentionLevel,
    cx: &App,
) -> AnyElement {
    div()
        .size(px(GLYPH_SIZE))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .child(status_glyph(id, level, cx))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_transition_requires_known_previous_and_change() {
        // No previous status (new agent): nothing to mark.
        assert_eq!(
            classify_agent_transition(None, Some("done")),
            AgentTransition::default()
        );
        // Same-status echo events are not transitions.
        assert_eq!(
            classify_agent_transition(Some("working"), Some("working")),
            AgentTransition::default()
        );
    }

    #[test]
    fn classify_transition_marks_review_and_attention_starts() {
        let t = classify_agent_transition(Some("working"), Some("done"));
        assert!(t.attention_started);
        assert!(t.review_started);
        assert!(!t.review_cleared);

        let t = classify_agent_transition(Some("working"), Some("blocked"));
        assert!(t.attention_started);
        assert!(!t.review_started);
        assert!(!t.review_cleared);
    }

    #[test]
    fn classify_transition_clears_review_on_new_turn() {
        let t = classify_agent_transition(Some("done"), Some("working"));
        assert!(!t.attention_started);
        assert!(!t.review_started);
        assert!(t.review_cleared);
    }

    #[test]
    fn attention_for_raw_status_maps_hook_vocabulary() {
        assert_eq!(
            attention_for_raw_status("blocked"),
            AttentionLevel::NeedsAttention
        );
        assert_eq!(
            attention_for_raw_status("done"),
            AttentionLevel::ReadyForReview
        );
        assert_eq!(attention_for_raw_status("working"), AttentionLevel::Working);
        assert_eq!(attention_for_raw_status("idle"), AttentionLevel::Idle);
    }
}
