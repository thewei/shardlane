//! Section empty-state placeholder: a uniform "No …" style.
//!
//! [INPUT]: gpui Div/Styled, crate::theme tokens
//! [OUTPUT]: `empty_state` — p16 + muted + FONT_BODY section placeholder
//! [POS]: the ui module's empty-state primitive; consumed by right_panel,
//! shell_panes, and other section empty states

use gpui::{div, px, Div, Hsla, ParentElement, Styled};

use crate::theme;

/// Section empty-state placeholder row: p(16) + FONT_BODY + the given muted
/// color.
///
/// sidebar's `sidebar_hint_row` is an indented inline variant with different
/// semantics; both implementations are kept.
pub(crate) fn empty_state(text: impl Into<String>, muted: Hsla) -> Div {
    div()
        .p(px(16.0))
        .text_size(theme::FONT_BODY)
        .text_color(muted)
        .child(text.into())
}
