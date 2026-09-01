//! Shared conversation timeline visual primitives: User / Reasoning / Working
//! (shared by History and Chat).
//!
//! [INPUT]: depends on shardlane_history::TranscriptMessage, the crate root's
//! ContentSurfaceTheme, and gpui/gpui-component primitives; callbacks are
//! injected by the caller.
//! [OUTPUT]: user_prompt_card / reasoning_row / working_row.
//! [POS]: the conversation visual primitives layer for herdr-gui `agent_ui`
//! (audit CHAT-A05 convergence). `conversation.rs` owns the pure row
//! projection; `activity.rs` owns tools/folds/footer; this module owns the
//! single styling for the User card, Reasoning block, and Working indicator.
//! History (expandable thinking) and Chat (live hint/local pending row)
//! configure it through parameters instead of duplicating styles. Answer
//! bodies go through `agent_ui::markdown` (one cache + one engine); outer
//! spacing is owned by each surface.

use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, App, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::{
    button::ButtonVariants as _, h_flex, v_flex, Icon, IconName as ComponentIconName, Sizable as _,
};

use gpui_component::WindowExt as _;

use crate::ui_metrics::SPACE_ICON;
use crate::ContentSurfaceTheme;

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
        .tooltip(crate::ui::tooltip::tooltip_fn("Copy message"))
        .on_click(move |_, window, cx| {
            cx.write_to_clipboard(crepuscularity_gpui::ClipboardItem::new_string(text.clone()));
            window.push_notification("Message copied", cx);
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
    /// Live Chat: a single truncated hint line, not expandable (streaming
    /// detail is carried by Working/Answer).
    LiveHint,
    /// History: expandable; the toggle callback is injected by the caller
    /// (semantic Button, A17).
    Expandable {
        expanded: bool,
        on_toggle: ToggleHandler,
    },
}

/// Reasoning row (audit CHAT-A05): the single visual for thinking.
/// Only non-empty thinking produces a row; callers skip on that basis.
pub fn reasoning_row(
    message: &shardlane_history::TranscriptMessage,
    presentation: ReasoningPresentation,
    theme: &ContentSurfaceTheme,
) -> AnyElement {
    let Some(thinking) = message.thinking.as_ref() else {
        return div().into_any_element();
    };
    let hint = SharedString::from(format!(
        "Thinking · {}",
        thinking
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
    ));
    match presentation {
        ReasoningPresentation::LiveHint => div()
            .w_full()
            .min_w_0()
            .px(px(16.0))
            .py(px(4.0))
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .p(px(10.0))
                    .rounded(px(8.0))
                    .bg(theme.background.opacity(0.6))
                    .border_1()
                    .border_color(theme.border.opacity(0.35))
                    .gap(px(4.0))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap(SPACE_ICON)
                            .items_center()
                            .child(
                                Icon::new(ComponentIconName::Info)
                                    .with_size(px(12.0))
                                    .text_color(theme.muted),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .italic()
                                    .text_size(crate::theme::FONT_META)
                                    .text_color(theme.muted)
                                    .child(hint),
                            ),
                    ),
            )
            .into_any_element(),
        ReasoningPresentation::Expandable {
            expanded,
            on_toggle,
        } => {
            let (toggle_icon, toggle_label) = if expanded {
                (ComponentIconName::ChevronUp, "Hide thinking")
            } else {
                (ComponentIconName::ChevronDown, "Show thinking")
            };
            let mut think_box = v_flex()
                .w_full()
                .min_w_0()
                .p(px(10.0))
                .rounded(px(8.0))
                .bg(theme.background.opacity(0.6))
                .border_1()
                .border_color(theme.border.opacity(0.35))
                .gap(SPACE_ICON);
            think_box = think_box.child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .items_center()
                            .child(
                                Icon::new(ComponentIconName::Info)
                                    .with_size(px(12.0))
                                    .text_color(theme.muted),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(crate::theme::FONT_META)
                                    .italic()
                                    .text_color(theme.muted)
                                    .line_clamp(1)
                                    .text_ellipsis()
                                    .child(hint),
                            ),
                    )
                    .child(
                        gpui_component::button::Button::new("reasoning-toggle")
                            .ghost()
                            .xsmall()
                            .icon(toggle_icon)
                            .label(toggle_label)
                            .on_click(move |_, window, app| on_toggle(window, app)),
                    ),
            );
            if expanded {
                think_box = think_box.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .pt(px(4.0))
                        .whitespace_normal()
                        .text_size(crate::theme::FONT_META)
                        .text_color(theme.muted)
                        .child(thinking.clone()),
                );
            }
            think_box.into_any_element()
        }
    }
}

/// User card (audit CHAT-A05): the single visual for timeline user messages.
/// `body` is constructed by the caller (Chat = plain text; History =
/// preview/expanded body; Chat pending = translucent plain text); `pending`
/// appends a Sending… marker to the header and lowers contrast.
pub fn user_prompt_card(
    time_label: Option<String>,
    pending: bool,
    body: AnyElement,
    theme: &ContentSurfaceTheme,
) -> AnyElement {
    v_flex()
        .w_full()
        .min_w_0()
        .rounded(px(10.0))
        .bg(theme.hover.opacity(if pending { 0.35 } else { 0.6 }))
        .border_1()
        .border_color(theme.border.opacity(if pending { 0.25 } else { 0.4 }))
        .px(px(14.0))
        .py(px(10.0))
        .gap(SPACE_ICON)
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .gap(SPACE_ICON)
                .items_center()
                .text_size(crate::theme::FONT_META)
                .text_color(theme.muted)
                .child(
                    Icon::new(ComponentIconName::User)
                        .with_size(px(12.0))
                        .text_color(if pending { theme.muted } else { theme.primary }),
                )
                .child(div().font_weight(FontWeight::MEDIUM).child("You"))
                .when(pending, |header| header.child(div().child("Sending…")))
                .when_some(time_label, |meta, label| meta.child(div().child(label))),
        )
        .child(body)
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
                .child(div().child("Working…")),
        )
        .into_any_element()
}

/// Context compaction / system boundary row (V1.1 Context Boundary):
/// presents a "Context compacted" or system phase divider line, silently,
/// without breaking the reading flow.
pub fn context_boundary_row(
    message: &shardlane_history::TranscriptMessage,
    theme: &ContentSurfaceTheme,
) -> AnyElement {
    let summary_text = message.text.trim();
    let has_summary = !summary_text.is_empty();

    let divider = h_flex()
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
                .child("Context compacted"),
        )
        .child(div().flex_1().h(px(1.0)).bg(theme.border.opacity(0.3)));

    if has_summary {
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
