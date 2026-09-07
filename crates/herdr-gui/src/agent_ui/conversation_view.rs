//! Shared conversation timeline visual primitives: User / Reasoning pill /
//! Working (shared by History and Chat).
//!
//! [INPUT]: depends on shardlane_history::TranscriptMessage, the crate root's
//! ContentSurfaceTheme, and gpui/gpui-component primitives; callbacks are
//! injected by the caller.
//! [OUTPUT]: user_prompt_card / reasoning_row / working_row.
//! [POS]: the conversation visual primitives layer for herdr-gui `agent_ui`
//! (audit CHAT-A05 convergence). `conversation.rs` owns the pure row
//! projection; `activity.rs` owns tools/footer; this module owns the
//! single styling for the User card, Reasoning block, and Working indicator.
//! History (static expandable pill) and Chat (live thinking pill with tail
//! streaming + local pending row) configure it through parameters instead of
//! duplicating styles. Answer bodies go through `agent_ui::markdown` (one
//! cache + one engine); outer spacing is owned by each surface.

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::{h_flex, v_flex, Icon, IconName as ComponentIconName, Sizable as _};

use gpui_component::WindowExt as _;

use crate::ui_metrics::SPACE_ICON;
use crate::ContentSurfaceTheme;

/// "Thought for Xs" duration rendering: compact, human units.
fn format_thinking_duration(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

/// Expand/collapse toggle callback (the first two on_click event parameters;
/// the entity is captured by the caller).
pub type ToggleHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Copy button at the bottom right of a message row on hover (notate 08-29,
/// five rounds): reveals with the row's group ("shardlane-msg-row") hover;
/// clicking copies the full raw message text.
/// The row container provides `.group("shardlane-msg-row").relative()` and
/// adds the button as a child.
pub(crate) fn hover_copy_message_button(
    id: impl Into<gpui::ElementId>,
    text: String,
    theme: &ContentSurfaceTheme,
) -> AnyElement {
    div()
        .id(id.into())
        .absolute()
        .bottom(px(2.0))
        .right(px(6.0))
        .invisible()
        .group_hover("shardlane-msg-row", |style| style.visible())
        .size(px(22.0))
        .rounded(px(5.0))
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.hover)
        .border_1()
        .border_color(theme.border)
        .cursor_pointer()
        .hover(|style| style.bg(theme.active))
        .tooltip(crate::ui::tooltip::tooltip_fn(crate::i18n::t(
            "conversation.copy_message",
        )))
        .on_click(move |_, window, cx| {
            cx.write_to_clipboard(crepuscularity_gpui::ClipboardItem::new_string(text.clone()));
            window.push_notification(crate::i18n::t("conversation.message_copied"), cx);
        })
        .child(
            Icon::empty()
                .path("icons/copy.svg")
                .with_size(px(12.0))
                .text_color(theme.muted),
        )
        .into_any_element()
}

/// Presentation modes for a Reasoning row (one visual language across both
/// surfaces).
pub enum ReasoningPresentation {
    /// Live Chat: the block streams a tail while the turn is still thinking;
    /// it collapses once the turn starts talking (ChatGPT contract). The
    /// caller computes the effective expanded state (streaming default open,
    /// settled default closed, explicit user state wins).
    Live {
        streaming_thinking: bool,
        duration: Option<std::time::Duration>,
        expanded: bool,
        on_toggle: ToggleHandler,
    },
    /// History: expandable; the toggle callback is injected by the caller
    /// (semantic Button, A17).
    Expandable {
        duration: Option<std::time::Duration>,
        expanded: bool,
        on_toggle: ToggleHandler,
    },
}

/// Consolidated turn thinking block (audit CHAT-A05, 2026-09-06 revision):
/// one block per turn — "Thought for Xs" when measurable, full chain expands
/// on click. Only non-empty thinking produces a row; callers skip on that
/// basis.
pub fn reasoning_row(
    thinking: &str,
    presentation: ReasoningPresentation,
    theme: &ContentSurfaceTheme,
) -> AnyElement {
    // The first thinking line is the pill's preview; the state label next to
    // it is the i18n surface (no duplicated "Thinking ·" prefix).
    let hint = thinking
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string();
    match presentation {
        ReasoningPresentation::Live {
            streaming_thinking,
            duration,
            expanded,
            on_toggle,
        } => {
            let state_label = if streaming_thinking {
                crate::i18n::t("conversation.thinking_live")
            } else if let Some(duration) = duration {
                crate::i18n::t_with(
                    "conversation.thought_for",
                    &[("duration", format_thinking_duration(duration))],
                )
            } else {
                crate::i18n::t("conversation.thought_process")
            };
            let mut pill = reasoning_pill_shell(theme, Some((expanded, on_toggle)))
                .child(reasoning_header(&state_label, &hint, theme, Some(expanded)));
            if expanded {
                pill = pill.child(reasoning_full_body(thinking, theme));
            } else if streaming_thinking {
                pill = pill.child(reasoning_tail_body(thinking, theme));
            }
            pill.into_any_element()
        }
        ReasoningPresentation::Expandable {
            duration,
            expanded,
            on_toggle,
        } => {
            let state_label = match duration {
                Some(duration) => crate::i18n::t_with(
                    "conversation.thought_for",
                    &[("duration", format_thinking_duration(duration))],
                ),
                None => crate::i18n::t("conversation.thinking"),
            };
            let mut think_box = reasoning_pill_shell(theme, Some((expanded, on_toggle)))
                .child(reasoning_header(&state_label, &hint, theme, Some(expanded)));
            if expanded {
                think_box = think_box.child(reasoning_full_body(thinking, theme));
            }
            think_box.into_any_element()
        }
    }
}

/// Shared block shell (one background/border for both surfaces). With a
/// toggle handler the whole shell is the click target — no separate "Show
/// thinking" button (user feedback 2026-09-06: click the block itself).
fn reasoning_pill_shell(
    theme: &ContentSurfaceTheme,
    toggle: Option<(bool, ToggleHandler)>,
) -> gpui::Stateful<gpui::Div> {
    let mut shell = v_flex()
        .w_full()
        .min_w_0()
        .p(px(10.0))
        .rounded(px(8.0))
        .bg(theme.background.opacity(0.6))
        .border_1()
        .border_color(theme.border.opacity(0.35))
        .gap(SPACE_ICON)
        .id("reasoning-block");
    if let Some((_, on_toggle)) = toggle {
        shell = shell
            .cursor_pointer()
            .hover(|style| style.bg(theme.background.opacity(0.9)))
            .on_click(move |_, window: &mut Window, app: &mut App| on_toggle(window, app))
    }
    shell
}

/// Block header: chevron + state label + first-line hint (truncated). The
/// chevron mirrors the expand state; the click target is the whole shell.
fn reasoning_header(
    state_label: &str,
    hint: &str,
    theme: &ContentSurfaceTheme,
    chevron: Option<bool>,
) -> AnyElement {
    let header = h_flex()
        .w_full()
        .min_w_0()
        .gap(SPACE_ICON)
        .items_center()
        .when_some(chevron, |header, expanded| {
            header.child(
                Icon::new(if expanded {
                    ComponentIconName::ChevronUp
                } else {
                    ComponentIconName::ChevronDown
                })
                .with_size(px(12.0))
                .flex_shrink_0()
                .text_color(theme.muted),
            )
        })
        .child(
            Icon::new(ComponentIconName::Info)
                .with_size(px(12.0))
                .text_color(theme.muted),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(crate::theme::FONT_META)
                .text_color(theme.muted)
                .child(state_label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .italic()
                .text_size(crate::theme::FONT_META)
                .text_color(theme.muted)
                .child(hint.to_string()),
        );
    header.into_any_element()
}

/// Expanded body: the full thinking text.
fn reasoning_full_body(thinking: &str, theme: &ContentSurfaceTheme) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .pt(px(4.0))
        .whitespace_normal()
        .text_size(crate::theme::FONT_META)
        .line_height(px(17.0))
        .text_color(theme.muted)
        .child(thinking.to_string())
        .into_any_element()
}

/// Live tail body: the newest thinking lines, so the pill feels alive while
/// the provider streams. Text-only (no animation): the frame cost stays a few
/// muted lines re-measured by the row signature diff.
fn reasoning_tail_body(thinking: &str, theme: &ContentSurfaceTheme) -> AnyElement {
    const TAIL_LINES: usize = 3;
    let mut tail: Vec<&str> = thinking
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(TAIL_LINES)
        .collect();
    tail.reverse();
    div()
        .w_full()
        .min_w_0()
        .pt(px(4.0))
        .whitespace_normal()
        .text_size(crate::theme::FONT_META)
        .text_color(theme.muted)
        .line_height(px(17.0))
        .child(tail.join("\n"))
        .into_any_element()
}

/// User card (audit CHAT-A05): the single visual for timeline user messages.
/// `body` is constructed by the caller (Chat = plain text; History =
/// preview/expanded body; Chat pending = translucent plain text). ChatGPT
/// layout (2026-09-06): right-aligned at ~3/4 width, no "You" chrome, the
/// timestamp sits at the bottom-left like every other row; `pending`
/// appends a Sending… marker to the header and lowers contrast.
pub fn user_prompt_card(
    time_label: Option<String>,
    pending: bool,
    body: AnyElement,
    theme: &ContentSurfaceTheme,
) -> AnyElement {
    div()
        .flex()
        .justify_end()
        .child(
            v_flex()
                .w(gpui::relative(0.75))
                .min_w_0()
                .rounded(px(10.0))
                .bg(theme.hover.opacity(if pending { 0.35 } else { 0.6 }))
                .border_1()
                .border_color(theme.border.opacity(if pending { 0.25 } else { 0.4 }))
                .px(px(14.0))
                .py(px(10.0))
                .gap(SPACE_ICON)
                .child(body)
                .child(
                    h_flex()
                        .w_full()
                        .gap(SPACE_ICON)
                        .items_center()
                        .text_size(px(11.0))
                        .text_color(theme.muted)
                        .when(pending, |meta| {
                            meta.child(gpui_component::spinner::Spinner::new().xsmall())
                                .child(div().child(crate::i18n::t("conversation.sending")))
                        })
                        .when_some(time_label, |meta, label| meta.child(div().child(label))),
                ),
        )
        .into_any_element()
}

/// Working indicator row: appears only in Chat's live busy projection.
pub fn working_row(theme: &ContentSurfaceTheme) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .px(px(16.0))
        .py(px(8.0))
        .flex()
        .justify_center()
        .child(
            h_flex()
                .gap(px(8.0))
                .items_center()
                .text_size(crate::theme::FONT_META)
                .text_color(theme.muted)
                .child(gpui_component::spinner::Spinner::new().xsmall())
                .child(div().child(crate::i18n::t("conversation.working"))),
        )
        .into_any_element()
}

/// Failed-run marker (Codex "Stopped" contract): a quiet danger line that
/// closes the timeline; the partial scene above stays untouched.
pub fn stopped_row(theme: &ContentSurfaceTheme) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .px(px(16.0))
        .py(px(8.0))
        .flex()
        .justify_center()
        .child(
            h_flex()
                .gap(px(8.0))
                .items_center()
                .text_size(crate::theme::FONT_META)
                .text_color(theme.danger.opacity(0.85))
                .child(Icon::new(ComponentIconName::CircleX).with_size(px(12.0)))
                .child(div().child(crate::i18n::t("conversation.stopped"))),
        )
        .into_any_element()
}

/// Context compaction / system boundary row (V1.1 Context Boundary):
/// presents a "Context compacted" or system phase divider line, silently,
/// without breaking the reading flow. The compacted body is collapsed by
/// default; clicking the divider toggles it (user feedback 2026-09-06).
pub fn context_boundary_row(
    message: &shardlane_history::TranscriptMessage,
    expanded: bool,
    on_toggle: Option<ToggleHandler>,
    theme: &ContentSurfaceTheme,
) -> AnyElement {
    let summary_text = message.text.trim();
    let has_summary = !summary_text.is_empty();

    let show_body = expanded && has_summary;
    let mut divider = h_flex()
        .w_full()
        .items_center()
        .gap(px(10.0))
        .child(div().flex_1().h(px(1.0)).bg(theme.border.opacity(0.3)))
        .child(
            h_flex()
                .items_center()
                .gap(px(5.0))
                .text_size(px(11.0))
                .text_color(theme.muted)
                .child(
                    Icon::empty()
                        .path("icons/minimize-2.svg")
                        .with_size(px(11.0))
                        .text_color(theme.muted),
                )
                .child(
                    Icon::new(if expanded {
                        ComponentIconName::ChevronUp
                    } else {
                        ComponentIconName::ChevronDown
                    })
                    .with_size(px(11.0))
                    .text_color(theme.muted),
                )
                .child(crate::i18n::t("conversation.context_compacted")),
        )
        .child(div().flex_1().h(px(1.0)).bg(theme.border.opacity(0.3)))
        .id("context-boundary");
    if let Some(on_toggle) = on_toggle.filter(|_| has_summary) {
        divider = divider
            .cursor_pointer()
            .hover(|style| style.text_color(theme.foreground))
            .on_click(move |_, window: &mut Window, app: &mut App| on_toggle(window, app));
    }

    if show_body {
        v_flex()
            .w_full()
            .min_w_0()
            .py(px(8.0))
            .gap(px(6.0))
            .child(divider)
            .child(
                div()
                    .w_full()
                    .px(px(12.0))
                    .py(px(6.0))
                    .rounded(px(6.0))
                    .bg(theme.hover.opacity(0.3))
                    .text_size(px(11.0))
                    .text_color(theme.muted)
                    .line_height(px(16.0))
                    .child(summary_text.to_string()),
            )
            .into_any_element()
    } else {
        div()
            .w_full()
            .min_w_0()
            .py(px(8.0))
            .child(divider)
            .into_any_element()
    }
}
