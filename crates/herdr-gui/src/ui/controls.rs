//! Form control family: Toggle (monochrome switch), Segmented (segmented
//! selection), ControlMenu (dropdown selection).
//!
//! Design conventions:
//! - Colors are always injected via [`ControlSurface`] (foreground +
//!   background); the controls derive their own border/hover/selected washes —
//!   the settings page injects the terminal palette surface
//!   (content_surface_theme), dialogs inject the gpui-component global theme.
//! - The controls are dumb elements: values are held by the caller, and
//!   changes flow back through on_change callbacks (side effects stay with the
//!   caller).
//! - ControlMenu attaches its popover through gpui-component's DropdownMenu
//!   trait (same path as composer_chip). Avoiding DropdownButton's forced
//!   caret segmentation is what makes the bordered-trigger visual possible.
//!
//! [INPUT]: depends on interaction.rs's shardlane_interactive and
//! gpui-component's PopupMenu/PopupMenuItem/DropdownMenu/Selectable/Icon/ActiveTheme
//! [OUTPUT]: exposes ControlSurface, Toggle, Segmented, ControlMenu
//! [POS]: the ui module's form control layer; consumed by settings_view.rs
//! (settings page) and scripts.rs (script dialogs). The sibling ui_metrics.rs
//! holds only measurement constants; this file holds composite elements, with
//! zero app-state coupling.

use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Animation, AnimationExt as _, App, Div, ElementId, Hsla, InteractiveElement,
    IntoElement, ParentElement, RenderOnce, SharedString, Stateful, StyleRefinement, Styled,
    Window,
};
use gpui_component::{
    menu::{DropdownMenu, PopupMenuItem},
    ActiveTheme as _, Icon, IconName, Selectable, Sizable as _,
};

use crate::interaction::InteractiveSurfaceExt as _;
use crate::ui_metrics::SPACE_ICON;

/// Boolean switch change callback.
type ToggleChange = Rc<dyn Fn(bool, &mut Window, &mut App)>;
/// Option value change callback (shared by Segmented/ControlMenu).
type ValueChange<T> = Rc<dyn Fn(&T, &mut Window, &mut App)>;

/// Injected colors for form controls: the caller supplies the foreground and
/// background of the surface the controls sit on; the control family derives
/// the remaining state colors. The settings page passes
/// content_surface_theme (following the terminal palette), dialogs pass the
/// gpui-component global theme.
#[derive(Clone, Copy)]
pub(crate) struct ControlSurface {
    pub foreground: Hsla,
    pub background: Hsla,
}

impl ControlSurface {
    /// Border/unselected track: 15% foreground wash.
    fn border(self) -> Hsla {
        self.foreground.opacity(0.15)
    }

    /// Selected/hover base wash (the accent tier): 7% foreground.
    pub(crate) fn accent(self) -> Hsla {
        self.foreground.opacity(0.07)
    }

    /// Hairline between rows inside a card: 12% foreground.
    pub(crate) fn hairline(self) -> Hsla {
        self.foreground.opacity(0.12)
    }

    /// Secondary text: 75% foreground (the text-secondary tier).
    fn text_secondary(self) -> Hsla {
        self.foreground.opacity(0.75)
    }

    /// Injected colors for gpui-component global-theme surfaces such as
    /// dialogs.
    pub(crate) fn from_app_theme(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            foreground: theme.foreground,
            background: theme.background,
        }
    }
}

// ---------------------------------------------------------------
// Toggle: monochrome switch
// ---------------------------------------------------------------

/// Toggle rendition: 36×20 monochrome capsule (on = foreground track /
/// background thumb, off = 8% track / 40% thumb) with a 150ms thumb-slide
/// animation; the focus wash sits on the outer wrapper (it does not cover the
/// track's selected color).
const TOGGLE_WIDTH: f32 = 36.0;
const TOGGLE_HEIGHT: f32 = 20.0;
const TOGGLE_THUMB: f32 = 14.0;
const TOGGLE_INSET: f32 = 2.0;
const TOGGLE_ANIMATION_MS: u64 = 150;

#[derive(IntoElement)]
pub(crate) struct Toggle {
    id: ElementId,
    checked: bool,
    surface: ControlSurface,
    on_change: ToggleChange,
}

impl Toggle {
    pub(crate) fn new(id: impl Into<ElementId>, surface: ControlSurface) -> Self {
        Self {
            id: id.into(),
            checked: false,
            surface,
            on_change: Rc::new(|_, _, _| {}),
        }
    }

    pub(crate) fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub(crate) fn on_change(
        mut self,
        on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Rc::new(on_change);
        self
    }
}

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let checked = self.checked;
        let on_change = self.on_change.clone();
        let (track_bg, track_border, thumb_bg) = if checked {
            // On state: border-foreground + bg-foreground are the same color
            // (the border melts into the track).
            (
                self.surface.foreground,
                self.surface.foreground,
                self.surface.background,
            )
        } else {
            (
                self.surface.foreground.opacity(0.08),
                self.surface.border(),
                self.surface.foreground.opacity(0.4),
            )
        };

        // Slide animation: keyed state remembers the previous frame's checked
        // state, and the animation plays only on the frame where the state
        // flips; the first frame lands in place so the thumb does not slide in
        // when the page opens (same mechanism as gpui-component's Switch).
        let toggle_state = window.use_keyed_state(self.id.clone(), cx, |_, _| checked);
        let prev_checked = *toggle_state.read(cx);
        let max_x = px(TOGGLE_WIDTH - TOGGLE_THUMB - TOGGLE_INSET * 2.0);
        let thumb = div().size(px(TOGGLE_THUMB)).rounded_full().bg(thumb_bg);
        let thumb = if prev_checked != checked {
            cx.spawn({
                let toggle_state = toggle_state.clone();
                async move |cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(TOGGLE_ANIMATION_MS))
                        .await;
                    _ = toggle_state.update(cx, |this, _| *this = checked);
                }
            })
            .detach();
            thumb
                .with_animation(
                    ElementId::NamedInteger("toggle-move".into(), checked as u64),
                    Animation::new(Duration::from_millis(TOGGLE_ANIMATION_MS)),
                    move |this, delta| {
                        let x = if checked {
                            max_x * delta
                        } else {
                            max_x - max_x * delta
                        };
                        this.left(x)
                    },
                )
                .into_any_element()
        } else {
            thumb
                .left(if checked { max_x } else { px(0.0) })
                .into_any_element()
        };

        // Focus/keyboard semantics hang on the wrapper (p2 lets the focus wash
        // show a ring outside the track); the track colors are not covered by
        // the wash.
        div()
            .id(self.id)
            .flex_none()
            .p(px(2.0))
            .rounded_full()
            .shardlane_interactive(self.surface.foreground, move |window, app| {
                on_change(!checked, window, app)
            })
            .child(
                div()
                    .w(px(TOGGLE_WIDTH))
                    .h(px(TOGGLE_HEIGHT))
                    .rounded_full()
                    .border(px(TOGGLE_INSET))
                    .border_color(track_border)
                    .bg(track_bg)
                    .flex()
                    .items_center()
                    .child(thumb),
            )
    }
}

// ---------------------------------------------------------------
// Segmented: segmented selection
// ---------------------------------------------------------------

/// Segmented rendition: compact segments inside an r7 bordered container
/// (h26, 10.5px, selected = accent base, hover brightens text only),
/// replacing gpui-component ButtonGroup for form usage.
const SEGMENTED_RADIUS: f32 = 7.0;
const SEGMENTED_HEIGHT: f32 = 26.0;

#[derive(IntoElement)]
pub(crate) struct Segmented<T: Clone + PartialEq + 'static> {
    id: SharedString,
    surface: ControlSurface,
    options: Vec<(T, SharedString)>,
    value: Option<T>,
    on_change: ValueChange<T>,
}

impl<T: Clone + PartialEq + 'static> Segmented<T> {
    pub(crate) fn new(id: impl Into<SharedString>, surface: ControlSurface) -> Self {
        Self {
            id: id.into(),
            surface,
            options: Vec::new(),
            value: None,
            on_change: Rc::new(|_, _, _| {}),
        }
    }

    pub(crate) fn option(mut self, value: T, label: impl Into<SharedString>) -> Self {
        self.options.push((value, label.into()));
        self
    }

    /// Currently selected value (no selection when outside the option set).
    pub(crate) fn value(mut self, value: T) -> Self {
        self.value = Some(value);
        self
    }

    pub(crate) fn on_change(
        mut self,
        on_change: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Rc::new(on_change);
        self
    }
}

impl<T: Clone + PartialEq + 'static> RenderOnce for Segmented<T> {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let surface = self.surface;
        div()
            .flex()
            .flex_none()
            .overflow_hidden()
            .rounded(px(SEGMENTED_RADIUS))
            .border_1()
            .border_color(surface.border())
            .children(
                self.options
                    .into_iter()
                    .enumerate()
                    .map(|(ix, (value, label))| {
                        let selected = self.value.as_ref() == Some(&value);
                        let on_change = self.on_change.clone();
                        div()
                            .id((self.id.clone(), ix))
                            .h(px(SEGMENTED_HEIGHT))
                            .px(px(11.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .whitespace_nowrap()
                            .text_size(crate::theme::FONT_DECORATIVE)
                            .text_color(if selected {
                                surface.foreground
                            } else {
                                surface.text_secondary()
                            })
                            .when(selected, |segment| segment.bg(surface.accent()))
                            // Hover brightens only the text, never the base.
                            .when(!selected, |segment| {
                                segment.hover(|style| style.text_color(surface.foreground))
                            })
                            .shardlane_interactive(surface.foreground, move |window, app| {
                                on_change(&value, window, app)
                            })
                            .child(label)
                    }),
            )
    }
}

// ---------------------------------------------------------------
// ControlMenu: dropdown selection
// ---------------------------------------------------------------

/// ControlMenu rendition: bordered trigger (h32/w116, label left + chevron
/// right, 12px) + a downward right-aligned popover with a check on the
/// selected item. The popover attaches through the DropdownMenu trait to a
/// gpui-component PopupMenu (same path as composer_chip); the trigger
/// highlights while the popover is open (Selectable).
const CONTROL_MENU_WIDTH: f32 = 116.0;
const CONTROL_MENU_HEIGHT: f32 = 32.0;

#[derive(IntoElement)]
pub(crate) struct ControlMenu<T: Clone + PartialEq + 'static> {
    base: Stateful<Div>,
    surface: ControlSurface,
    label: SharedString,
    options: Vec<(T, SharedString)>,
    value: Option<T>,
    on_change: ValueChange<T>,
    selected: bool,
}

impl<T: Clone + PartialEq + 'static> ControlMenu<T> {
    pub(crate) fn new(id: impl Into<ElementId>, surface: ControlSurface) -> Self {
        Self {
            base: ::gpui::div().id(id.into()),
            surface,
            label: SharedString::default(),
            options: Vec::new(),
            value: None,
            on_change: Rc::new(|_, _, _| {}),
            selected: false,
        }
    }

    /// The current value shown on the trigger.
    pub(crate) fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub(crate) fn option(mut self, value: T, label: impl Into<SharedString>) -> Self {
        self.options.push((value, label.into()));
        self
    }

    /// Currently selected value (drives the check state inside the menu).
    pub(crate) fn value(mut self, value: T) -> Self {
        self.value = Some(value);
        self
    }

    pub(crate) fn on_change(
        mut self,
        on_change: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Rc::new(on_change);
        self
    }

    /// Attaches the downward popover (the menu's right edge aligns with the
    /// trigger's right edge), checking the selected item.
    /// Scrolling is always on: a non-scrollable menu has no height clamp, so a
    /// long option list (e.g. the system font enumeration) would overflow the
    /// window; with scrollable on, the default max_h is half the window
    /// height.
    pub(crate) fn menu_below_right(self) -> impl IntoElement {
        let value = self.value.clone();
        let on_change = self.on_change.clone();
        let options = self.options.clone();
        self.dropdown_menu_with_anchor(gpui::Corner::BottomRight, move |mut menu, _, _| {
            menu = menu.scrollable(true);
            for (option_value, label) in &options {
                let checked = value.as_ref() == Some(option_value);
                let on_change = on_change.clone();
                let option_value = option_value.clone();
                menu = menu.item(
                    PopupMenuItem::new(label.clone())
                        .checked(checked)
                        .on_click(move |_, window, app| on_change(&option_value, window, app)),
                );
            }
            menu
        })
    }
}

impl<T: Clone + PartialEq + 'static> Styled for ControlMenu<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl<T: Clone + PartialEq + 'static> InteractiveElement for ControlMenu<T> {
    fn interactivity(&mut self) -> &mut gpui::Interactivity {
        self.base.interactivity()
    }
}

impl<T: Clone + PartialEq + 'static> Selectable for ControlMenu<T> {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl<T: Clone + PartialEq + 'static> DropdownMenu for ControlMenu<T> {}

impl<T: Clone + PartialEq + 'static> RenderOnce for ControlMenu<T> {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let surface = self.surface;
        self.base
            .w(px(CONTROL_MENU_WIDTH))
            .h(px(CONTROL_MENU_HEIGHT))
            .rounded(px(6.0))
            .border_1()
            .border_color(surface.border())
            .bg(surface.background)
            .px(px(12.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(SPACE_ICON)
            .text_size(crate::theme::FONT_BODY)
            .cursor_default()
            // Highlight while the popover is open (DropdownMenuPopover sets
            // selected).
            .when(self.selected, |trigger| {
                trigger
                    .bg(surface.accent())
                    .border_color(surface.foreground.opacity(0.4))
            })
            .when(!self.selected, |trigger| {
                trigger.hover(|style| style.bg(surface.accent()))
            })
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(if self.selected {
                        surface.foreground
                    } else {
                        surface.text_secondary()
                    })
                    .child(self.label.clone()),
            )
            .child(
                Icon::new(IconName::ChevronDown)
                    .with_size(px(12.0))
                    .text_color(surface.foreground.opacity(0.45))
                    .into_any_element(),
            )
    }
}
