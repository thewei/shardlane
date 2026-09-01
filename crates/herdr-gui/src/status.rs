//! Shared operational status/attention semantics for Shardlane Activity.
//!
//! Pure domain module: no GPUI, no render, no IO. All types and functions are
//! testable independently. UI rendering consumes these primitives from a
//! separate presentation layer.
//!
//! [INPUT]: Agent/Script/Service runtime state projections from herdr.rs and scripts/model.rs
//! [OUTPUT]: AttentionLevel — shared status primitives consumed by Sidebar,
//!           Header, and notifications (ActivityTransition/classify_agent_transition
//!           and AttentionSummary/aggregate_attention were removed along with the
//!           Activity panel, notate 2026-08-29)
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
