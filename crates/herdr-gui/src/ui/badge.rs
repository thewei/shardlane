//! Compact pill badges: filled and outlined variants.
//!
//! [INPUT]: gpui Div/Hsla/Pixels/Styled, gpui-component ActiveTheme
//! [OUTPUT]: `badge` (filled), `outline_badge` (outlined)
//! [POS]: the ui module's badge primitive layer; consumed by
//! history/transcript_view, reusable later for sidebar chips

use gpui::{div, px, Div, Hsla, ParentElement, Styled};

use crate::theme;

/// Filled small badge (header project name, git branch, etc.).
pub(crate) fn badge(text: String, bg: Hsla, fg: Hsla) -> Div {
    div()
        .px(px(6.0))
        .py(px(1.0))
        .rounded(px(5.0))
        .bg(bg)
        .text_color(fg)
        .text_size(theme::FONT_META)
        .max_w(px(220.0))
        .truncate()
        .child(text)
}

/// Outlined small badge (model / source).
pub(crate) fn outline_badge(text: String, color: Hsla) -> Div {
    div()
        .px(px(6.0))
        .py(px(1.0))
        .rounded(px(5.0))
        .border_1()
        .border_color(color.opacity(0.55))
        .text_color(color)
        .text_size(theme::FONT_META)
        .max_w(px(220.0))
        .truncate()
        .child(text)
}
