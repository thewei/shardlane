//! Shared rendering for drag ghost rows: position offset + popover pill.
//!
//! [INPUT]: gpui Div/Point/Pixels/Styled, gpui-component ActiveTheme
//! [OUTPUT]: `drag_ghost_row` — a parameterized ghost row constructor
//! [POS]: the ui module's drag ghost helper layer; consumed by sidebar/rows

use gpui::{div, px, App, Div, ParentElement, Pixels, Point, Styled};
use gpui_component::ActiveTheme as _;

pub(crate) struct DragGhostStyle {
    pub padding_x: Pixels,
    pub height: Pixels,
    pub radius: Pixels,
    pub font_size: Pixels,
    pub gap: Option<Pixels>,
}

pub(crate) fn drag_ghost_row(
    position: Point<Pixels>,
    label: &str,
    style: DragGhostStyle,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let mut inner = div()
        .px(style.padding_x)
        .h(style.height)
        .flex()
        .items_center()
        .rounded(style.radius)
        .bg(theme.popover)
        .text_size(style.font_size)
        .text_color(theme.popover_foreground)
        .shadow_md()
        .child(label.to_string());
    if let Some(gap) = style.gap {
        inner = inner.gap(gap);
    }
    div()
        .pl(position.x + px(8.0))
        .pt(position.y + px(8.0))
        .child(inner)
}
