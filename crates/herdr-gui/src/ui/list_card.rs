//! Unified two/three-row list card: the shared presentation for the History
//! catalog rows and the Sidebar Agents cards.
//!
//! [INPUT]: gpui Div/Styled/SharedString/Hsla/Pixels/AnyElement, crate::theme
//!          font tokens, ui_metrics spacing
//! [OUTPUT]: `ListCardColors` + `list_card` — the card body only (padding,
//!           radius, selected/hover surfaces, lead+title row, optional
//!           two-line description, optional meta row, optional trailing
//!           element). Interaction (roving rows, context menus, click
//!           handlers) stays with the caller by design: History wires
//!           `shardlane_interactive` + context menu, the Sidebar wires
//!           `shardlane_roving_row`.
//! [POS]: the ui module's list-card primitive; consumed by history/page_view
//! and sidebar/rows.

use gpui::prelude::*;
use gpui::{
    div, img, px, AnyElement, Div, FontWeight, Hsla, ParentElement, Pixels, SharedString, Styled,
};
use gpui_component::Sizable as _;

use crate::theme;
use crate::ui_metrics::SPACE_ICON;

#[derive(Clone, Copy)]
pub(crate) struct ListCardColors {
    pub foreground: Hsla,
    pub secondary: Hsla,
    pub selected_bg: Hsla,
    pub hover_bg: Hsla,
    pub selected_foreground: Hsla,
}

/// The card body. `fixed_height` is required by History's `uniform_list`
/// virtualization (single item height); the Sidebar passes `None` and lets
/// the card hug its rows. `description` renders up to two lines (History's
/// session description); the Sidebar omits it. `trailing` sits at the title
/// row's end (Sidebar status glyph).
#[allow(clippy::too_many_arguments)]
pub(crate) fn list_card(
    colors: ListCardColors,
    selected: bool,
    lead: AnyElement,
    title: impl Into<SharedString>,
    description: Option<SharedString>,
    meta: Option<SharedString>,
    trailing: Option<AnyElement>,
    fixed_height: Option<Pixels>,
) -> Div {
    let foreground = if selected {
        colors.selected_foreground
    } else {
        colors.foreground
    };
    div()
        .w_full()
        .px_3()
        .py_2()
        .rounded(px(6.0))
        .overflow_hidden()
        .when_some(fixed_height, |card, height| card.h(height))
        .when(selected, |card| card.bg(colors.selected_bg))
        .when(!selected, |card| {
            card.hover(|style| style.bg(colors.hover_bg))
        })
        .child(
            div()
                .w_full()
                .h_full()
                .flex()
                .flex_col()
                .justify_center()
                .gap(SPACE_ICON)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .min_w_0()
                        .child(lead)
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(foreground)
                                .child(title.into()),
                        )
                        .children(trailing),
                )
                .children(description.map(|description| {
                    div()
                        .min_w_0()
                        .line_clamp(2)
                        .text_ellipsis()
                        .whitespace_normal()
                        .text_size(theme::FONT_DESCRIPTION)
                        .text_color(colors.secondary)
                        .child(description)
                }))
                .children(meta.map(|meta| {
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(theme::FONT_META)
                        .text_color(colors.secondary)
                        .child(meta)
                })),
        )
}

/// The History catalog's brand-or-fallback lead: the session's Agent brand
/// icon, else the neutral book-open mark.
pub(crate) fn history_card_lead(brand_icon: Option<String>) -> AnyElement {
    match brand_icon {
        Some(path) => img(path).size(px(15.0)).into_any_element(),
        None => gpui_component::Icon::new(gpui_component::IconName::BookOpen)
            .small()
            .into_any_element(),
    }
}
