//! Composer control-row selector chip.
//!
//! [INPUT]: Depends on gpui-component's DropdownMenu/Selectable traits and ActiveTheme;
//! zero app state coupling.
//! [OUTPUT]: Exposes ComposerChip (icon/label/caret/selected/hover_tint configuration;
//! gains .dropdown_menu(f) popover capability via the DropdownMenu trait — a single
//! silent chip without DropdownButton's mandatory caret segment).
//! [POS]: herdr-gui's composer-specific chip component; shared by new_agent.rs's four
//! selector kinds (agent/mode/branch/project) and the title-sentence picker; zero sibling dependencies.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    px, AnyElement, App, Div, ElementId, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, Stateful, StyleRefinement, Styled, Window,
};
use gpui_component::{menu::DropdownMenu, ActiveTheme as _, Selectable};

use crate::theme;
use crate::ui_metrics::SPACE_ICON;

/// MenuChip geometry: h26 / px7 / r6 / gap6 / 13px.
const CHIP_HEIGHT: f32 = 26.0;
const CHIP_RADIUS: f32 = 6.0;

#[derive(IntoElement)]
pub(crate) struct ComposerChip {
    base: Stateful<Div>,
    icon: Option<AnyElement>,
    label: Option<AnyElement>,
    selected: bool,
    hover_tint: bool,
    /// Bare mode: skip chip geometry (h26/px7/r6) and keep only clickability and children —
    /// used by inline link-style pickers (the title sentence's Project slot).
    bare: bool,
}

impl ComposerChip {
    pub(crate) fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: ::gpui::div().id(id.into()),
            icon: None,
            label: None,
            selected: false,
            hover_tint: true,
            bare: false,
        }
    }

    /// Leading icon (brand img or Icon; caller supplies a 12–14px element).
    pub(crate) fn icon(mut self, icon: AnyElement) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Text label (muted 13px, truncated).
    pub(crate) fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(
            ::gpui::div()
                .min_w_0()
                .truncate()
                .child(label.into())
                .into_any_element(),
        );
        self
    }

    /// Custom label element (e.g. the title-sentence picker's dotted underline label).
    pub(crate) fn label_element(mut self, label: AnyElement) -> Self {
        self.label = Some(label);
        self
    }

    /// Disable the hover tint (used by inline link-style pickers).
    pub(crate) fn hover_tint(mut self, hover_tint: bool) -> Self {
        self.hover_tint = hover_tint;
        self
    }

    /// Bare mode: no chip geometry, just a clickable element (used by inline pickers).
    pub(crate) fn bare(mut self) -> Self {
        self.bare = true;
        self
    }
}

impl Styled for ComposerChip {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for ComposerChip {
    fn interactivity(&mut self) -> &mut ::gpui::Interactivity {
        self.base.interactivity()
    }
}

impl Selectable for ComposerChip {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl DropdownMenu for ComposerChip {}

impl RenderOnce for ComposerChip {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let tint = theme.foreground.opacity(0.05);
        self.base
            .when(!self.bare, |chip| {
                chip.h(px(CHIP_HEIGHT))
                    .px(px(7.0))
                    .rounded(px(CHIP_RADIUS))
                    .flex()
                    .items_center()
                    .gap(SPACE_ICON)
                    .text_size(theme::FONT_BODY)
            })
            .when(self.bare, |chip| chip.flex().items_baseline())
            .cursor_default()
            .when(self.selected, |chip| chip.bg(tint))
            .when(!self.selected && self.hover_tint, |chip| {
                chip.hover(|style| style.bg(tint))
            })
            .when_some(self.icon, |chip, icon| chip.child(icon))
            .when_some(self.label, |chip, label| {
                if self.bare {
                    chip.child(label)
                } else {
                    chip.child(
                        ::gpui::div()
                            .min_w_0()
                            .truncate()
                            .text_color(if self.selected {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .child(label),
                    )
                }
            })
    }
}
