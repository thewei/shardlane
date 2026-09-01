//! Shared interaction presentation primitives: Question / Permission /
//! PlanApproval cards and their state machine (shared by History and Chat).
//!
//! [INPUT]: depends on shardlane-host's ConversationInteraction model and DTO,
//! ContentSurfaceTheme, and gpui/gpui-component primitives; interaction
//! callbacks are injected by the caller.
//! [OUTPUT]: interaction_card.
//! [POS]: the interaction presentation layer for herdr-gui `agent_ui`
//! (Phase 3 closed loop). Q&A and permission requests from all providers
//! (Claude / Codex / Pi / OpenCode / Synthetic) project uniformly into this
//! semantic component; History provides read-only/resolved presentation, Live
//! Chat provides interactive operation and state transitions. Clicking
//! "Open in Terminal" triggers the Native Fallback sequence.

use std::collections::HashSet;
use std::rc::Rc;

use gpui::{
    div, px, AnyElement, App, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{h_flex, v_flex, Disableable as _, Icon, Sizable as _};

use shardlane_host::conversation_interactions::{
    ConversationInteraction, ConversationInteractionKind, ConversationInteractionState,
    InteractionResponse, PermissionScope,
};

use crate::ContentSurfaceTheme;

pub type InteractionChoiceHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;
pub type InteractionCustomInputHandler = Rc<dyn Fn(String, &mut Window, &mut App)>;
pub type InteractionSubmitHandler = Rc<dyn Fn(InteractionResponse, &mut Window, &mut App)>;
pub type InteractionDelegateHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Interaction card presentation mode (Live Chat live operation; settled
/// interactions render their static content through the same mode).
pub enum InteractionPresentation {
    /// Live Chat: operable mode, carrying the selected choice set, custom
    /// input, and submit callbacks.
    Interactive {
        selected_choices: HashSet<String>,
        custom_input: Option<String>,
        in_flight: bool,
        on_select_choice: InteractionChoiceHandler,
        on_toggle_choice: InteractionChoiceHandler,
        on_custom_input_change: InteractionCustomInputHandler,
        on_submit: InteractionSubmitHandler,
        on_delegate_terminal: InteractionDelegateHandler,
    },
}

/// The unified interaction card render function.
pub fn interaction_card(
    interaction: &ConversationInteraction,
    presentation: InteractionPresentation,
    theme: &ContentSurfaceTheme,
) -> AnyElement {
    let kind = interaction.kind;
    let state = interaction.state;

    let is_pending = state == ConversationInteractionState::Pending;
    let is_resolving = state == ConversationInteractionState::Resolving;

    let (badge_text, badge_icon, badge_color) = match kind {
        ConversationInteractionKind::Question => (
            "Needs your input",
            "icons/message-square.svg",
            theme.foreground,
        ),
        ConversationInteractionKind::Permission => {
            ("Approval required", "icons/shield.svg", theme.danger)
        }
        ConversationInteractionKind::PlanApproval => (
            "Plan approval required",
            "icons/check-circle.svg",
            theme.foreground,
        ),
        ConversationInteractionKind::Authentication => {
            ("Authentication required", "icons/key.svg", theme.danger)
        }
        ConversationInteractionKind::Other => {
            ("Interaction required", "icons/info.svg", theme.foreground)
        }
    };

    let header = h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(
            h_flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    Icon::empty()
                        .path(badge_icon)
                        .with_size(px(14.0))
                        .text_color(badge_color),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(badge_color)
                        .child(badge_text),
                ),
        )
        .child(
            h_flex()
                .items_center()
                .gap(px(6.0))
                .child(state_badge(state, theme))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme.muted)
                        .child(interaction.provider.clone()),
                ),
        );

    let prompt_view = div()
        .w_full()
        .text_size(px(13.0))
        .font_weight(FontWeight::NORMAL)
        .text_color(theme.foreground)
        .line_height(px(19.0))
        .child(interaction.prompt.clone());

    let content_block = match presentation {
        InteractionPresentation::Interactive {
            ref selected_choices,
            ref custom_input,
            in_flight,
            ref on_select_choice,
            ref on_toggle_choice,
            ref on_custom_input_change,
            ref on_submit,
            ref on_delegate_terminal,
        } => {
            if is_pending || is_resolving {
                interactive_content(
                    interaction,
                    selected_choices,
                    custom_input.as_deref(),
                    in_flight || is_resolving,
                    on_select_choice.clone(),
                    on_toggle_choice.clone(),
                    on_custom_input_change.clone(),
                    on_submit.clone(),
                    on_delegate_terminal.clone(),
                    theme,
                )
            } else {
                read_only_content(interaction, theme)
            }
        }
    };

    v_flex()
        .w_full()
        .min_w_0()
        .p(px(14.0))
        .rounded(px(10.0))
        .bg(theme.background)
        .border_1()
        .border_color(if is_pending {
            theme.border
        } else {
            theme.border.opacity(0.5)
        })
        .gap(px(10.0))
        .child(header)
        .child(prompt_view)
        .child(content_block)
        .into_any_element()
}

/// Status badge.
fn state_badge(state: ConversationInteractionState, theme: &ContentSurfaceTheme) -> AnyElement {
    let (label, bg, fg) = match state {
        ConversationInteractionState::Pending => {
            ("Pending", theme.danger.opacity(0.15), theme.danger)
        }
        ConversationInteractionState::Resolving => ("Submitting...", theme.hover, theme.muted),
        ConversationInteractionState::Resolved => {
            ("Resolved", theme.success.opacity(0.15), theme.success)
        }
        ConversationInteractionState::Rejected => {
            ("Rejected", theme.danger.opacity(0.15), theme.danger)
        }
        ConversationInteractionState::Cancelled => ("Cancelled", theme.hover, theme.muted),
        ConversationInteractionState::NativeFallback => {
            ("In Terminal", theme.hover, theme.foreground)
        }
        ConversationInteractionState::Expired => ("Expired", theme.hover, theme.muted),
    };

    div()
        .px(px(6.0))
        .py(px(1.5))
        .rounded(px(4.0))
        .bg(bg)
        .text_size(px(10.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .child(label)
        .into_any_element()
}

/// Read-only mode content display.
fn read_only_content(
    interaction: &ConversationInteraction,
    theme: &ContentSurfaceTheme,
) -> AnyElement {
    if interaction.choices.is_empty() {
        return div().into_any_element();
    }

    let mut list = v_flex().w_full().gap(px(6.0));
    for choice in &interaction.choices {
        list = list.child(
            h_flex()
                .w_full()
                .items_center()
                .gap(px(8.0))
                .px(px(8.0))
                .py(px(4.0))
                .rounded(px(4.0))
                .bg(theme.hover.opacity(0.5))
                .text_size(px(12.0))
                .text_color(theme.muted)
                .child("○")
                .child(choice.label.clone()),
        );
    }

    list.into_any_element()
}

/// Interactive mode content rendering.
#[allow(clippy::too_many_arguments)]
fn interactive_content(
    interaction: &ConversationInteraction,
    selected_choices: &HashSet<String>,
    custom_input: Option<&str>,
    in_flight: bool,
    on_select_choice: InteractionChoiceHandler,
    on_toggle_choice: InteractionChoiceHandler,
    _on_custom_input_change: InteractionCustomInputHandler,
    on_submit: InteractionSubmitHandler,
    on_delegate_terminal: InteractionDelegateHandler,
    theme: &ContentSurfaceTheme,
) -> AnyElement {
    let is_multiple = interaction.multiple;
    let mut block = v_flex().w_full().gap(px(10.0));

    match interaction.kind {
        ConversationInteractionKind::Question => {
            // Choices list
            let mut choices_view = v_flex().w_full().gap(px(6.0));
            for (idx, choice) in interaction.choices.iter().enumerate() {
                let choice_id = choice.id.clone();
                let is_selected = selected_choices.contains(&choice_id);

                let indicator = if is_multiple {
                    if is_selected {
                        "☑"
                    } else {
                        "☐"
                    }
                } else if is_selected {
                    "●"
                } else {
                    "○"
                };

                let on_select = on_select_choice.clone();
                let on_toggle = on_toggle_choice.clone();
                let cid = choice_id.clone();

                let row = div()
                    .id(("interaction-choice", idx))
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p(px(8.0))
                    .rounded(px(6.0))
                    .bg(if is_selected {
                        theme.hover
                    } else {
                        theme.background
                    })
                    .border_1()
                    .border_color(if is_selected {
                        theme.border
                    } else {
                        theme.border.opacity(0.4)
                    })
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.hover))
                    .on_click(move |_, window, cx| {
                        if !in_flight {
                            if is_multiple {
                                on_toggle(&cid, window, cx);
                            } else {
                                on_select(&cid, window, cx);
                            }
                        }
                    })
                    .child(
                        h_flex()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(if is_selected {
                                        theme.foreground
                                    } else {
                                        theme.muted
                                    })
                                    .child(indicator),
                            )
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(if is_selected {
                                        FontWeight::MEDIUM
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(theme.foreground)
                                    .child(choice.label.clone()),
                            ),
                    );

                choices_view = choices_view.child(row);
            }

            block = block.child(choices_view);

            // Action row: Open in Terminal (left) + Continue (right)
            let has_selection =
                !selected_choices.is_empty() || custom_input.is_some_and(|t| !t.trim().is_empty());

            let on_sub = on_submit.clone();
            let selected_list: Vec<String> = selected_choices.iter().cloned().collect();
            let is_multi = is_multiple;
            let on_del = on_delegate_terminal.clone();

            let action_row = h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .pt(px(4.0))
                .child(
                    Button::new("open-in-terminal")
                        .ghost()
                        .small()
                        .label("Open in Terminal")
                        .disabled(in_flight)
                        .on_click(move |_, window, cx| {
                            on_del(window, cx);
                        }),
                )
                .child(
                    Button::new("continue-interaction")
                        .primary()
                        .small()
                        .label("Continue")
                        .disabled(!has_selection || in_flight)
                        .on_click(move |_, window, cx| {
                            if is_multi {
                                on_sub(
                                    InteractionResponse::MultiChoice {
                                        option_ids: selected_list.clone(),
                                    },
                                    window,
                                    cx,
                                );
                            } else if let Some(first) = selected_list.first() {
                                on_sub(
                                    InteractionResponse::Choice {
                                        option_id: first.clone(),
                                    },
                                    window,
                                    cx,
                                );
                            }
                        }),
                );

            block = block.child(action_row);
        }
        ConversationInteractionKind::Permission => {
            // Permission options: Allow once, Allow for session, Reject, Open in Terminal
            let on_allow_once = on_submit.clone();
            let on_allow_session = on_submit.clone();
            let on_deny = on_submit.clone();
            let on_del = on_delegate_terminal.clone();

            let action_row = h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .pt(px(4.0))
                .child(
                    Button::new("open-in-terminal-perm")
                        .ghost()
                        .small()
                        .label("Open in Terminal")
                        .disabled(in_flight)
                        .on_click(move |_, window, cx| {
                            on_del(window, cx);
                        }),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            Button::new("deny-permission")
                                .ghost()
                                .small()
                                .label("Reject")
                                .disabled(in_flight)
                                .on_click(move |_, window, cx| {
                                    on_deny(InteractionResponse::Deny { reason: None }, window, cx);
                                }),
                        )
                        .child(
                            Button::new("allow-session-perm")
                                .ghost()
                                .small()
                                .label("Allow for session")
                                .disabled(in_flight)
                                .on_click(move |_, window, cx| {
                                    on_allow_session(
                                        InteractionResponse::Allow {
                                            scope: PermissionScope::Session,
                                        },
                                        window,
                                        cx,
                                    );
                                }),
                        )
                        .child(
                            Button::new("allow-once-perm")
                                .primary()
                                .small()
                                .label("Allow once")
                                .disabled(in_flight)
                                .on_click(move |_, window, cx| {
                                    on_allow_once(
                                        InteractionResponse::Allow {
                                            scope: PermissionScope::Once,
                                        },
                                        window,
                                        cx,
                                    );
                                }),
                        ),
                );

            block = block.child(action_row);
        }
        ConversationInteractionKind::PlanApproval => {
            let on_approve = on_submit.clone();
            let on_deny = on_submit.clone();
            let on_del = on_delegate_terminal.clone();

            let action_row = h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .pt(px(4.0))
                .child(
                    Button::new("open-in-terminal-plan")
                        .ghost()
                        .small()
                        .label("Open in Terminal")
                        .disabled(in_flight)
                        .on_click(move |_, window, cx| {
                            on_del(window, cx);
                        }),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            Button::new("reject-plan")
                                .ghost()
                                .small()
                                .label("Reject")
                                .disabled(in_flight)
                                .on_click(move |_, window, cx| {
                                    on_deny(InteractionResponse::Deny { reason: None }, window, cx);
                                }),
                        )
                        .child(
                            Button::new("approve-plan")
                                .primary()
                                .small()
                                .label("Approve")
                                .disabled(in_flight)
                                .on_click(move |_, window, cx| {
                                    on_approve(
                                        InteractionResponse::Allow {
                                            scope: PermissionScope::Once,
                                        },
                                        window,
                                        cx,
                                    );
                                }),
                        ),
                );

            block = block.child(action_row);
        }
        _ => {
            let on_del = on_delegate_terminal.clone();
            let action_row = h_flex().w_full().items_center().justify_end().child(
                Button::new("open-in-terminal-other")
                    .ghost()
                    .small()
                    .label("Open in Terminal")
                    .disabled(in_flight)
                    .on_click(move |_, window, cx| {
                        on_del(window, cx);
                    }),
            );
            block = block.child(action_row);
        }
    }

    block.into_any_element()
}
