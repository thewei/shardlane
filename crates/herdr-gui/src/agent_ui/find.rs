//! In-conversation find (⌘F, notate 08-29 five rounds): inline find state, hit
//! collection, and the floating find bar shared by Chat and History.
//!
//! [INPUT]: the markdown engine's `markdown_search_matches`/`SearchHighlights`
//! (hit ordinals and rendering share one block-ordinal scheme), gpui-component
//! Input, the ShardlaneApp entity.
//! [OUTPUT]: ConversationFind (one per surface: input + hit set + active),
//! collect_find_hits / highlights_for (render-time lookup), conversation_find_bar.
//! [POS]: shared find presentation layer for agent_ui; hit sets belong to each
//! surface controller (chat/surface.rs and history/state.rs); this module holds
//! no conversation data.

use std::rc::Rc;

use gpui::{
    div, px, AnyElement, App, Context, ElementId, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, StatefulInteractiveElement as _, Styled as _,
    Window,
};
use gpui_component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    ActiveTheme as _, Icon, IconName as ComponentIconName, Sizable as _,
};

use super::markdown::render::{markdown_search_matches, SearchHighlights, TextSearchMatch};

/// Per-message hit cap (guards against pathological regexes / huge messages
/// flooding the hit set).
const FIND_MATCH_CAP: usize = 500;

/// One hit: message seq plus the block ordinal and byte range within that
/// message's rendering.
#[derive(Clone, Debug)]
pub(crate) struct FindHit {
    pub seq: i64,
    pub match_: TextSearchMatch,
}

/// In-conversation find state (one per surface).
pub(crate) struct ConversationFind {
    pub input: Entity<InputState>,
    /// Holds the live gpui::Subscription handles: never read by value, but
    /// dropping them would unsubscribe the find input's Change/Enter handling.
    #[allow(dead_code)]
    pub subscriptions: Vec<gpui::Subscription>,
    pub hits: Vec<FindHit>,
    pub active: usize,
}

impl ConversationFind {
    pub(crate) fn active_label(&self) -> String {
        if self.hits.is_empty() {
            "0/0".to_string()
        } else {
            format!("{}/{}", self.active + 1, self.hits.len())
        }
    }
}

/// Collect hits in message order (messages = (seq, body)). The query is a
/// literal substring, case-insensitive.
pub(crate) fn collect_find_hits(messages: &[(i64, &str)], query: &str) -> Vec<FindHit> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let Ok(regex) = regex::Regex::new(&format!("(?i){}", regex::escape(trimmed))) else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for (seq, text) in messages {
        if text.is_empty() {
            continue;
        }
        let (matches, _capped) = markdown_search_matches(text, &regex, FIND_MATCH_CAP);
        hits.extend(
            matches
                .into_iter()
                .map(|match_| FindHit { seq: *seq, match_ }),
        );
    }
    hits
}

/// Render highlights for one message; the active hit is also highlighted when
/// it lands on that message.
pub(crate) fn highlights_for(
    hits: &[FindHit],
    seq: i64,
    active: usize,
) -> Option<SearchHighlights> {
    let own: Vec<TextSearchMatch> = hits
        .iter()
        .filter(|hit| hit.seq == seq)
        .map(|hit| hit.match_.clone())
        .collect();
    if own.is_empty() {
        return None;
    }
    let active = hits
        .get(active)
        .filter(|hit| hit.seq == seq)
        .map(|hit| hit.match_.clone());
    Some(SearchHighlights {
        matches: Rc::new(own),
        active,
    })
}

/// Inline find bar (floats at the conversation area's top right): input + n/m
/// count + previous/next/close.
/// Esc is captured by the container's key_down; stepping and close go through
/// entity callbacks into the surface controller.
pub(crate) fn conversation_find_bar(
    find: &ConversationFind,
    herdr: &Entity<crate::ShardlaneApp>,
    cx: &Context<crate::ShardlaneApp>,
) -> AnyElement {
    let theme = cx.theme().clone();
    let step_herdr = herdr.clone();
    let close_herdr = herdr.clone();
    let esc_herdr = herdr.clone();
    let count = find.active_label();
    let input = find.input.clone();
    h_flex()
        .absolute()
        .top(px(8.0))
        .right(px(16.0))
        .h(px(34.0))
        .w(px(340.0))
        .px(px(8.0))
        .gap(px(4.0))
        .items_center()
        .rounded(px(8.0))
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow_md()
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if event.keystroke.key != "escape" || event.keystroke.modifiers.modified() {
                return;
            }
            cx.stop_propagation();
            let herdr = esc_herdr.clone();
            herdr.update(cx, |this, cx| {
                this.conversation_find_close(window, cx);
            });
        })
        .child(
            Icon::new(ComponentIconName::Search)
                .small()
                .text_color(theme.muted_foreground),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Input::new(&input).small().appearance(false)),
        )
        .child(
            div()
                .flex_none()
                .text_size(crate::theme::FONT_META)
                .text_color(theme.muted_foreground)
                .child(count),
        )
        .child(find_step_button(
            "conversation-find-prev",
            theme.muted_foreground,
            {
                let herdr = step_herdr.clone();
                move |window, app| {
                    herdr.update(app, |this, cx| {
                        this.conversation_find_step(window, cx, -1);
                    });
                }
            },
            ComponentIconName::ChevronUp,
            "Previous match (Shift+Enter)",
        ))
        .child(find_step_button(
            "conversation-find-next",
            theme.muted_foreground,
            {
                let herdr = step_herdr.clone();
                move |window, app| {
                    herdr.update(app, |this, cx| {
                        this.conversation_find_step(window, cx, 1);
                    });
                }
            },
            ComponentIconName::ChevronDown,
            "Next match (Enter)",
        ))
        .child(find_step_button(
            "conversation-find-close",
            theme.muted_foreground,
            {
                let herdr = close_herdr.clone();
                move |window, app| {
                    herdr.update(app, |this, cx| {
                        this.conversation_find_close(window, cx);
                    });
                }
            },
            ComponentIconName::Close,
            "Close find (Esc)",
        ))
        .into_any_element()
}

fn find_step_button(
    id: impl Into<ElementId>,
    muted: gpui::Hsla,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
    icon: ComponentIconName,
    label: &'static str,
) -> impl IntoElement {
    div()
        .id(id.into())
        .flex_none()
        .size(px(22.0))
        .rounded(px(5.0))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(muted)
        .hover(|style| style.bg(gpui::black().opacity(0.08)))
        .tooltip(crate::ui::tooltip::tooltip_fn(label))
        .on_click(move |_, window, app| on_click(window, app))
        .child(Icon::new(icon).with_size(px(13.0)))
}

/// Establish the find bar's standard subscriptions on a shared input
/// (Change → re-run; Enter/Shift+Enter → next/previous). The re-run callback is
/// injected by the caller (each surface's controller).
pub(crate) fn subscribe_find_input(
    input: &Entity<InputState>,
    herdr: &Entity<crate::ShardlaneApp>,
    window: &mut Window,
    cx: &mut Context<crate::ShardlaneApp>,
    rerun: impl Fn(&mut crate::ShardlaneApp, &mut Window, &mut Context<crate::ShardlaneApp>) + 'static,
) -> Vec<gpui::Subscription> {
    // Nested-update guard (crash record 08-29, six rounds): subscribe_in's
    // callback already runs inside ShardlaneApp's update (the first parameter
    // is the entity), so calling `.update` on the same entity inside the
    // callback triggers gpui's "already being updated" panic (hit when typing
    // fires InputEvent::Change). Use `this` directly; no second update.
    let _ = input;
    let change = cx.subscribe_in(
        input,
        window,
        move |this, _, event: &InputEvent, window, cx| match event {
            InputEvent::Change => rerun(this, window, cx),
            InputEvent::PressEnter { secondary } => {
                let step: isize = if *secondary { -1 } else { 1 };
                this.conversation_find_step(window, cx, step);
            }
            _ => {}
        },
    );
    let focus_handle = input.read(cx).focus_handle(cx);
    let herdr_for_focus = herdr.clone();
    let focus_sub = cx.on_focus(
        &focus_handle,
        window,
        move |this: &mut crate::ShardlaneApp, _, cx| {
            this.set_conversation_find_focused(true, cx);
            let _ = herdr_for_focus;
        },
    );
    let blur_sub = cx.on_blur(&focus_handle, window, |this, _, cx| {
        this.set_conversation_find_focused(false, cx);
    });
    vec![change, focus_sub, blur_sub]
}
