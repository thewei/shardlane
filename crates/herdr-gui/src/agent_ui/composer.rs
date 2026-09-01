//! Shared AgentComposer: Shardlane's single Agent input card implementation.
//!
//! [INPUT]: depends on gpui-component's Input/InputState/Spinner/Button/Icon and
//! ActiveTheme; the caller provides owned state and callbacks via builders.
//! [OUTPUT]: AgentComposer (composer card geometry + input slot + attachment
//! strip + control-row left/right slots + Send/Busy/Disabled send button + the
//! footer row under the card), ComposerSendState, and the
//! composer_send_state/attachment_* pure functions.
//! [POS]: composer primitives for herdr-gui `agent_ui`, extracted bottom-up from
//! new_agent/page.rs on 2026-08-27; New Agent is the first consumer (launch
//! orchestration stays in new_agent), Live Chat the second (agent.prompt
//! semantics stay in chat). This component contains no Herdr calls, no provider
//! lifecycle, no Project/Branch source of truth, and no launch/send
//! orchestration — it renders only the slots the caller supplies.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, App, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    spinner::Spinner,
    v_flex, ActiveTheme as _, Icon, IconName as ComponentIconName, Sizable as _,
};

use crate::theme;

/// Send / remove-attachment callbacks (the caller owns all semantics; the
/// component only forwards clicks).
type WindowCallback = std::sync::Arc<dyn Fn(&mut Window, &mut App)>;
type IndexCallback = std::sync::Arc<dyn Fn(usize, &mut App)>;

/// The send button slot's three visual states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposerSendState {
    /// Not sendable (empty draft or caller state disallows it): ghost base +
    /// muted icon, not clickable.
    Disabled,
    /// Sendable: inverted base + clickable.
    Ready,
    /// Submitting: the send button slot becomes a spinner.
    Busy,
}

/// New Agent's existing semantics (submitting takes precedence over draft
/// state), extracted into a pure function for both consumers to reuse.
pub fn composer_send_state(submitting: bool, has_text: bool) -> ComposerSendState {
    if submitting {
        ComposerSendState::Busy
    } else if has_text {
        ComposerSendState::Ready
    } else {
        ComposerSendState::Disabled
    }
}

/// Attachment tile icon kind (folder/file).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachmentIconKind {
    Folder,
    File,
}

/// Attachment path → tile icon kind (folder wins).
pub fn attachment_icon_kind(path: &std::path::Path) -> AttachmentIconKind {
    if path.is_dir() {
        AttachmentIconKind::Folder
    } else {
        AttachmentIconKind::File
    }
}

/// Attachment path → tile label (file name; falls back to "file" when unnamed,
/// keeping New Agent's existing copy).
pub fn attachment_tile_label(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_string()
}

/// Send button geometry: 26×26 circle.
const SEND_SIZE: f32 = 26.0;

/// Shared Agent input card (composer design language: 720-wide r13 card, py10,
/// 14px bare field, mt8 control row, 26×26 circular send button, h28 footer row
/// under the card).
///
/// The caller owns and injects: the `InputState`, control-row left-cluster
/// chips, control-row right-cluster extra actions, the send callback, the
/// attachment-remove callback, and the footer row content. The component reads
/// the `InputState` text only for the border focus state and send button
/// availability visuals.
#[derive(IntoElement)]
pub struct AgentComposer {
    id: SharedString,
    input: Entity<InputState>,
    max_w: Pixels,
    attachments: Vec<std::path::PathBuf>,
    left_controls: Vec<AnyElement>,
    trailing: Vec<AnyElement>,
    send: ComposerSendState,
    on_send: Option<WindowCallback>,
    on_remove_attachment: Option<IndexCallback>,
    footer: Vec<AnyElement>,
}

impl AgentComposer {
    pub fn new(id: impl Into<SharedString>, input: Entity<InputState>) -> Self {
        Self {
            id: id.into(),
            input,
            max_w: px(720.0),
            attachments: Vec::new(),
            left_controls: Vec::new(),
            trailing: Vec::new(),
            send: ComposerSendState::Disabled,
            on_send: None,
            on_remove_attachment: None,
            footer: Vec::new(),
        }
    }

    /// Attachment tile data (New Agent passes its attachments snapshot).
    pub fn attachments(mut self, attachments: Vec<std::path::PathBuf>) -> Self {
        self.attachments = attachments;
        self
    }

    /// Control-row left-cluster chip (agent/mode/permission pickers, etc.).
    pub fn left_control(mut self, control: impl IntoElement) -> Self {
        self.left_controls.push(control.into_any_element());
        self
    }

    /// Control-row right-cluster extra actions (e.g. an @ mention entry),
    /// placed before the send button.
    pub fn trailing(mut self, control: impl IntoElement) -> Self {
        self.trailing.push(control.into_any_element());
        self
    }

    pub fn send_state(mut self, send: ComposerSendState) -> Self {
        self.send = send;
        self
    }

    /// Send click callback (the caller owns all submission semantics; the
    /// component initiates no calls).
    pub fn on_send(mut self, callback: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_send = Some(std::sync::Arc::new(callback));
        self
    }

    /// Attachment-remove callback (argument is the attachment index).
    pub fn on_remove_attachment(mut self, callback: impl Fn(usize, &mut App) + 'static) -> Self {
        self.on_remove_attachment = Some(std::sync::Arc::new(callback));
        self
    }

    /// Footer row content under the card (project/branch context, etc.).
    pub fn footer(mut self, children: Vec<AnyElement>) -> Self {
        self.footer = children;
        self
    }
}

impl RenderOnce for AgentComposer {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let dark = theme.mode.is_dark();
        let has_text = !self.input.read(cx).value().trim().is_empty();
        let send = self.send;
        let on_send = self.on_send.clone();
        let on_remove = self.on_remove_attachment.clone();
        let card_id = self.id.clone();

        let send_slot = match (send, on_send) {
            (ComposerSendState::Busy, _) => v_flex()
                .id(SharedString::from(format!("{card_id}-send-busy")))
                .size(px(SEND_SIZE))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.secondary)
                .child(Spinner::new().xsmall())
                .into_any_element(),
            (send_state, on_send) => {
                let enabled = send_state == ComposerSendState::Ready;
                let button = v_flex()
                    .id(SharedString::from(format!("{card_id}-send")))
                    .size(px(SEND_SIZE))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(if enabled {
                        theme.foreground
                    } else {
                        theme.secondary.opacity(0.7)
                    })
                    .when(enabled, |button| {
                        button
                            .cursor_default()
                            .hover(|style| style.opacity(0.9))
                            .active(|style| style.opacity(0.8))
                            .when_some(on_send, |button, on_send| {
                                button.on_click(move |_, window, app| {
                                    (on_send.clone())(window, app);
                                })
                            })
                    })
                    .child(
                        Icon::new(ComponentIconName::ArrowUp)
                            .with_size(px(16.0))
                            .text_color(if enabled {
                                theme.background
                            } else {
                                theme.muted_foreground
                            }),
                    );
                button.into_any_element()
            }
        };

        v_flex()
            .w_full()
            .max_w(self.max_w)
            .child(
                v_flex()
                    .w_full()
                    .overflow_hidden()
                    .rounded(px(13.0))
                    .border_1()
                    .border_color(if has_text {
                        theme.accent.opacity(0.6)
                    } else {
                        theme.border.opacity(0.58)
                    })
                    .bg(if dark {
                        theme.foreground.opacity(0.035)
                    } else {
                        gpui::rgb(0xFFFFFF).into()
                    })
                    .py(px(10.0))
                    .child(
                        div().w_full().pt(px(2.0)).child(
                            Input::new(&self.input)
                                .w_full()
                                .appearance(false)
                                .p_0()
                                .px(px(14.0)),
                        ),
                    )
                    .when(!self.attachments.is_empty(), |composer| {
                        composer.child(
                            h_flex()
                                .w_full()
                                .flex_wrap()
                                .px(px(10.0))
                                .pt(px(2.0))
                                .pb(px(6.0))
                                .gap(px(8.0))
                                .children(self.attachments.iter().enumerate().map(
                                    |(index, path)| {
                                        let on_remove = on_remove.clone();
                                        let name = attachment_tile_label(path);
                                        let icon = match attachment_icon_kind(path) {
                                            AttachmentIconKind::Folder => ComponentIconName::Folder,
                                            AttachmentIconKind::File => ComponentIconName::File,
                                        };
                                        div()
                                            .id(SharedString::from(format!(
                                                "{card_id}-attachment-{index}"
                                            )))
                                            .relative()
                                            .size(px(64.0))
                                            .rounded(px(8.0))
                                            .overflow_hidden()
                                            .border_1()
                                            .border_color(theme.border.opacity(0.8))
                                            .bg(theme.secondary.opacity(0.6))
                                            .child(
                                                v_flex()
                                                    .size_full()
                                                    .px(px(5.0))
                                                    .items_center()
                                                    .justify_center()
                                                    .gap(px(5.0))
                                                    .child(
                                                        Icon::new(icon)
                                                            .with_size(px(16.0))
                                                            .text_color(theme.muted_foreground),
                                                    )
                                                    .child(
                                                        div()
                                                            .w_full()
                                                            .flex()
                                                            .justify_center()
                                                            .child(
                                                                div()
                                                                    .max_w_full()
                                                                    .truncate()
                                                                    .text_size(theme::FONT_BODY)
                                                                    .text_color(
                                                                        theme.muted_foreground,
                                                                    )
                                                                    .child(name),
                                                            ),
                                                    ),
                                            )
                                            .child(
                                                Button::new(SharedString::from(format!(
                                                    "{card_id}-attachment-remove-{index}"
                                                )))
                                                .ghost()
                                                .xsmall()
                                                .absolute()
                                                .top(px(0.0))
                                                .right(px(0.0))
                                                .icon(ComponentIconName::Close)
                                                .tooltip("Remove attachment")
                                                .on_click(move |_, _, app| {
                                                    if let Some(on_remove) = on_remove.as_ref() {
                                                        (on_remove)(index, app);
                                                    }
                                                }),
                                            )
                                    },
                                )),
                        )
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .items_center()
                            // Control row: mt8 px10 gap4 12.5px, no wrap.
                            .gap(px(4.0))
                            .mt(px(8.0))
                            .px(px(10.0))
                            .text_size(theme::FONT_BODY)
                            .children(self.left_controls)
                            .child(div().flex_1().min_w_0())
                            .children(self.trailing)
                            .child(send_slot),
                    ),
            )
            .when(!self.footer.is_empty(), |root| {
                root.child(
                    h_flex()
                        .w_full()
                        .h(px(28.0))
                        .mt(px(4.0))
                        .pl(px(10.0))
                        .pr(px(10.0))
                        .flex()
                        .items_center()
                        .gap(px(2.0))
                        .text_size(theme::FONT_BODY)
                        .children(self.footer),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn send_state_maps_submissions_over_drafts() {
        assert_eq!(composer_send_state(true, true), ComposerSendState::Busy);
        assert_eq!(composer_send_state(true, false), ComposerSendState::Busy);
        assert_eq!(composer_send_state(false, true), ComposerSendState::Ready);
        assert_eq!(
            composer_send_state(false, false),
            ComposerSendState::Disabled
        );
    }

    #[test]
    fn attachment_helpers_keep_new_agent_labels_and_icons() {
        assert_eq!(
            attachment_tile_label(&PathBuf::from("/tmp/demo/report.md")),
            "report.md"
        );
        assert_eq!(
            attachment_tile_label(&PathBuf::from("/")),
            "file",
            "root path has no file name; keep the New Agent fallback label"
        );
        assert_eq!(
            attachment_icon_kind(&PathBuf::from("/tmp")),
            AttachmentIconKind::Folder
        );
        assert_eq!(
            attachment_icon_kind(&PathBuf::from("/etc/hostname")),
            AttachmentIconKind::File
        );
    }
}
