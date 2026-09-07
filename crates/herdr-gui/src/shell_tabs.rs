//! Native content-area Tab strip (Terminal setting `terminal.tab_bar_placement`).
//!
//! In `native` mode the active Project's Tabs render as a gpui-component `TabBar` above the
//! hosted Herdr TUI instead of as Sidebar rows. The strip is presentation only: the Tab order,
//! titles, and lifecycle stay Herdr-authoritative (`state.tabs`, `tab.moved` events, `tab.move`/
//! `tab.close`/`tab.rename` RPCs), exactly mirroring the Sidebar tab row's actions (Pin, Rename,
//! Copy Tab ID, Close, drag reorder) so hosting the strip never removes runtime capabilities.
//!
//! [INPUT]: ShardlaneApp state/config from the crate root (`super`), herdr Tab/Agent projections, gpui-component tab/menu/button/tooltip, sidebar's agent identity helper, ui drag-ghost/menu helpers.
//! [OUTPUT]: `ShardlaneApp::native_tabs_enabled`/`native_tab_bar_height`/`native_tab_bar` (the strip renderer), `NATIVE_TAB_BAR_HEIGHT`, and `NativeTabDrag` (drag payload + ghost).
//! [POS]: The `crates/herdr-gui` shell tabs responsibility domain; consumed by `shell_render::client_shell` and the terminal geometry helpers; sibling of the shell_* ShardlaneApp method shards.
use super::*;
use crate::assets::agent_brand_icon;
use crate::sidebar::agent_identity;
use crate::status::status_glyph_container;
use crate::theme::FONT_BODY;
use crate::ui::drag::{drag_ghost_row, DragGhostStyle};
use crate::ui::menus::{menu_action, menu_action_cx};
use crate::ui_metrics::{ROW_HEIGHT_SUB, SPACE_SM};
use ::gpui::{img, Div, Point, Stateful};
use gpui_component::tooltip::Tooltip;

/// Layout height of the native Tab strip (including its bottom divider). Terminal geometry
/// (`terminal_size`/`terminal_canvas_origin`) subtracts this so the hosted TUI grid stays
/// inside the visible area below the strip.
pub(super) const NATIVE_TAB_BAR_HEIGHT: f32 = 32.0;

/// Long Tab titles clip at this width; Herdr's `tab.rename` remains the way to shorten them.
const NATIVE_TAB_MAX_WIDTH: f32 = 200.0;

#[derive(Clone)]
pub(super) struct NativeTabDrag {
    tab_id: String,
    workspace_id: String,
    label: String,
    position: Point<Pixels>,
}

impl NativeTabDrag {
    fn position(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl Render for NativeTabDrag {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        drag_ghost_row(
            self.position,
            &self.label,
            DragGhostStyle {
                padding_x: SPACE_SM,
                height: ROW_HEIGHT_SUB,
                radius: cx.theme().radius,
                font_size: FONT_BODY,
                gap: None,
            },
            cx,
        )
    }
}

impl ShardlaneApp {
    pub(super) fn native_tabs_enabled(&self) -> bool {
        self.config.terminal.tab_bar_placement == settings::TabBarPlacement::Native
    }

    pub(super) fn native_tab_bar_height(&self) -> f64 {
        if self.native_tabs_enabled() {
            f64::from(NATIVE_TAB_BAR_HEIGHT)
        } else {
            0.0
        }
    }

    /// The content-area Tab strip for the active Project: click to switch (through the
    /// FocusIntent seam), right-click for the Herdr-aligned Tab menu, drag to reorder via
    /// Herdr `tab.move`, middle-click to close, trailing `+` for `tab.create`. Overflowing
    /// strips scroll horizontally with page-arrow buttons on both edges.
    pub(super) fn native_tab_bar(&self, theme: UiTheme, cx: &mut Context<Self>) -> Div {
        let herdr = cx.entity();
        let workspace_id = self.active_workspace_id().map(str::to_string);
        let tabs = self.visible_tabs();
        // Selection follows the same source the Sidebar rows use: the client-local focused
        // Tab id, falling back to Herdr's focused flag (never Herdr focus churn alone).
        let focused_tab_id = self.state.focused_tab_id.clone().or_else(|| {
            tabs.iter()
                .find(|tab| tab.focused)
                .map(|t| t.tab_id.clone())
        });
        let selected_index = focused_tab_id
            .as_deref()
            .and_then(|focused| tabs.iter().position(|tab| tab.tab_id == focused))
            .unwrap_or(usize::MAX);

        let new_tab_herdr = herdr.clone();
        let scroll_pos = self.native_tab_scroll.offset().x;
        let scroll_max = self.native_tab_scroll.max_offset().width;
        let overflowing = scroll_max > px(0.0);
        let can_scroll_left = scroll_pos > px(0.0);
        let can_scroll_right = scroll_pos < scroll_max;
        let scroll_left_herdr = herdr.clone();
        let scroll_right_herdr = herdr.clone();
        let scroll_left_handle = self.native_tab_scroll.clone();
        let scroll_right_handle = self.native_tab_scroll.clone();
        let plus_button = move || {
            let herdr = new_tab_herdr.clone();
            Button::new("native-tab-new")
                .xsmall()
                .ghost()
                .icon(ComponentIconName::Plus)
                .tooltip("New Tab (⌘T)")
                .on_click(move |_, window, app| {
                    herdr.update(app, |this, cx| {
                        this.new_tab(&NewTab, window, cx);
                    });
                })
        };

        let total_tabs = tabs.len();
        let mut tab_elements = Vec::new();
        for (index, tab) in tabs.into_iter().enumerate() {
            let is_selected = index == selected_index;
            let next_is_selected = index + 1 == selected_index;
            tab_elements.push(
                self.native_tab_item(tab, index, is_selected, workspace_id.clone(), theme, cx)
                    .into_any_element(),
            );

            // Subtle vertical separator between two adjacent inactive tabs
            if !is_selected && !next_is_selected && index + 1 < total_tabs {
                tab_elements.push(
                    div()
                        .w(px(1.0))
                        .h(px(12.0))
                        .my_auto()
                        .flex_none()
                        .bg(cx.theme().border.opacity(0.6))
                        .into_any_element(),
                );
            }
        }

        let mut scroll_container = h_flex()
            .id("native-tabs-scroll")
            .flex_1()
            .min_w_0()
            .h_full()
            .items_end()
            .overflow_x_scroll()
            .track_scroll(&self.native_tab_scroll)
            .gap(px(2.0))
            .children(tab_elements);

        if !overflowing {
            scroll_container = scroll_container.child(
                div()
                    .flex_none()
                    .mb(px(2.0))
                    .ml(px(2.0))
                    .child(plus_button()),
            );
        }

        let mut bar = h_flex()
            .id("native-tab-bar")
            .w_full()
            .h_full()
            .items_end()
            .px(px(4.0));

        if can_scroll_left {
            bar = bar.child(
                div().flex_none().mb(px(2.0)).child(
                    Button::new("native-tab-scroll-left")
                        .xsmall()
                        .ghost()
                        .icon(ComponentIconName::ChevronLeft)
                        .tooltip("Previous tabs")
                        .on_click(move |_, _, app| {
                            scroll_native_tabs(&scroll_left_handle, -1.0);
                            scroll_left_herdr.update(app, |_, cx| cx.notify());
                        }),
                ),
            );
        }

        bar = bar.child(scroll_container);

        if overflowing {
            bar = bar.child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .mb(px(2.0))
                    .gap_0p5()
                    .when(can_scroll_right, |row| {
                        row.child(
                            Button::new("native-tab-scroll-right")
                                .xsmall()
                                .ghost()
                                .icon(ComponentIconName::ChevronRight)
                                .tooltip("Next tabs")
                                .on_click(move |_, _, app| {
                                    scroll_native_tabs(&scroll_right_handle, 1.0);
                                    scroll_right_herdr.update(app, |_, cx| cx.notify());
                                }),
                        )
                    })
                    .child(plus_button()),
            );
        }

        let measure_herdr = cx.entity();
        let measure_scroll = self.native_tab_scroll.clone();
        let last_max = self.native_tab_scroll_max.clone();
        div()
            .relative()
            .h(px(NATIVE_TAB_BAR_HEIGHT))
            .flex_none()
            .w_full()
            .bg(rgb(theme.panel))
            .border_b_1()
            .border_color(rgb(theme.border))
            .flex()
            .child(
                canvas(
                    move |_, _, app| {
                        let max = measure_scroll.max_offset().width.to_f64();
                        if (last_max.get() - max).abs() > 0.5 {
                            last_max.set(max);
                            measure_herdr.update(app, |_, cx| cx.notify());
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(bar)
    }

    fn native_tab_item(
        &self,
        tab: Tab,
        index: usize,
        is_selected: bool,
        workspace_id: Option<String>,
        theme: UiTheme,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let component_theme = cx.theme().clone();
        let dark = theme.bg <= 0x808080;
        let tab_id = tab.tab_id.clone();
        let title = self.tab_title(&tab);
        let is_pinned = self.config.ui.sidebar.pinned_tabs.contains(&tab_id);
        let close_confirming = self.pending_close_tab.as_deref() == Some(tab_id.as_str());
        let tab_agent = self
            .state
            .agents
            .iter()
            .find(|agent| agent.tab_id.as_deref() == Some(tab_id.as_str()));
        let agent_status = tab_agent
            .and_then(|agent| {
                agent
                    .agent_status
                    .as_deref()
                    .or(agent.custom_status.as_deref())
            })
            .filter(|status| *status != "unknown")
            .map(crate::status::attention_for_raw_status);
        let brand_icon = tab_agent
            .and_then(agent_identity)
            .and_then(|identity| agent_brand_icon(identity, dark));

        let has_running_service = self
            .observed_services
            .iter()
            .any(|service| service.tab_id == tab_id)
            || self.scripts.scripts.iter().any(|script| {
                script.tab_id.as_deref() == Some(tab_id.as_str())
                    && script.runtime.status == crate::scripts::ScriptStatus::Running
            });

        let lead: AnyElement = match brand_icon {
            Some(path) => img(path).size(px(14.0)).into_any_element(),
            None => {
                let icon_el = Icon::empty()
                    .path("icons/square-terminal.svg")
                    .with_size(px(13.0))
                    .text_color(if is_selected {
                        component_theme.foreground.opacity(0.85)
                    } else {
                        component_theme.muted_foreground.opacity(0.8)
                    });
                if has_running_service {
                    div()
                        .relative()
                        .child(icon_el)
                        .child(
                            div()
                                .absolute()
                                .top(px(-1.5))
                                .right(px(-2.0))
                                .size(px(5.5))
                                .rounded_full()
                                .border_1()
                                .border_color(if is_selected {
                                    rgb(theme.bg)
                                } else {
                                    rgb(theme.panel)
                                })
                                .bg(component_theme.success),
                        )
                        .into_any_element()
                } else {
                    icon_el.into_any_element()
                }
            }
        };

        let close_herdr = cx.entity();
        let close_id = tab_id.clone();
        let close_danger = component_theme.danger;
        let close_text = component_theme.foreground;
        let close_muted = component_theme.muted_foreground;
        let close_btn = div()
            .id(SharedString::from(format!("native-tab-close-{tab_id}")))
            .size(px(16.0))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .opacity(if close_confirming {
                1.0
            } else if is_selected {
                0.65
            } else {
                0.0
            })
            .group_hover("native-tab-item", |s| s.opacity(1.0))
            .hover(move |s| {
                s.opacity(1.0).bg(if close_confirming {
                    close_danger.opacity(0.25)
                } else {
                    close_text.opacity(0.12)
                })
            })
            .when(close_confirming, |s| s.bg(close_danger.opacity(0.18)))
            .on_click(move |_, window, app| {
                app.stop_propagation();
                close_herdr.update(app, |this, cx| {
                    this.confirm_close_tab(close_id.clone(), window, cx);
                });
            })
            .tooltip(move |_, cx| {
                cx.new(|_| {
                    Tooltip::new(if close_confirming {
                        "Click again to close"
                    } else {
                        "Close Tab (⌘W)"
                    })
                })
                .into()
            })
            .child(
                Icon::empty()
                    .path("icons/x.svg")
                    .with_size(px(10.5))
                    .text_color(if close_confirming {
                        close_danger
                    } else {
                        close_muted
                    }),
            );

        let menu_herdr = cx.entity();
        let menu_tab_id = tab_id.clone();
        let menu_title = title.clone();
        let menu_overlay = div().absolute().inset_0().context_menu(move |menu, _, _| {
            let pin_label = if is_pinned { "Unpin Tab" } else { "Pin Tab" };
            menu.item({
                let tab_id = menu_tab_id.clone();
                menu_action_cx(pin_label, &menu_herdr, move |this, cx| {
                    this.toggle_pin_tab(tab_id.clone(), cx);
                })
            })
            .item({
                let tab_id = menu_tab_id.clone();
                let label = menu_title.clone();
                menu_action("Rename Tab…", &menu_herdr, move |this, window, cx| {
                    this.open_tab_rename(tab_id.clone(), label.clone(), window, cx);
                })
            })
            .item({
                let tab_id = menu_tab_id.clone();
                menu_action(
                    crate::i18n::t("shell.copy_tab_id"),
                    &menu_herdr,
                    move |this, window, cx| {
                        this.copy_runtime_id(tab_id.clone(), window, cx);
                    },
                )
            })
            .item(PopupMenuItem::separator())
            .item({
                let tab_id = menu_tab_id.clone();
                menu_action("Close Tab", &menu_herdr, move |this, window, cx| {
                    this.close_tab_by_id(tab_id.clone(), window, cx);
                })
            })
        });

        let click_herdr = cx.entity();
        let click_tab_id = tab_id.clone();
        let middle_close_herdr = cx.entity();
        let middle_close_id = tab_id.clone();
        let drag_over_tab_id = tab_id.clone();
        let drop_tab_id = tab_id.clone();
        let drop_herdr = cx.entity();
        let drop_color = rgb(theme.active);

        let drag = workspace_id.map(|workspace_id| NativeTabDrag {
            tab_id: tab_id.clone(),
            workspace_id,
            label: title.clone(),
            position: Point::default(),
        });

        div()
            .id(SharedString::from(format!("native-tab-{tab_id}")))
            .group("native-tab-item")
            .relative()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(8.0))
            .max_w(px(NATIVE_TAB_MAX_WIDTH))
            .min_w(px(80.0))
            .flex_shrink()
            .cursor_pointer()
            .when(is_selected, |s| {
                s.h(px(27.0))
                    .mb(px(-1.0))
                    .rounded_t(px(5.0))
                    .bg(rgb(theme.bg))
                    .border_t_1()
                    .border_l_1()
                    .border_r_1()
                    .border_color(rgb(theme.border))
            })
            .when(!is_selected, |s| {
                s.h(px(25.0))
                    .mb(px(1.0))
                    .rounded(px(4.0))
                    .hover(move |h| h.bg(component_theme.foreground.opacity(0.06)))
            })
            .child(menu_overlay)
            .child(lead)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(crate::theme::FONT_BODY)
                    .font_weight(if is_selected {
                        gpui::FontWeight::MEDIUM
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .text_color(if is_selected {
                        rgb(theme.text)
                    } else {
                        rgb(theme.muted)
                    })
                    .child(title.clone()),
            )
            .when_some(agent_status, |row, level| {
                row.child(status_glyph_container(
                    SharedString::from(format!("native-tab-status-{tab_id}")),
                    level,
                    cx,
                ))
            })
            .child(close_btn)
            .tooltip(crate::ui::tooltip::tooltip_fn(title))
            .on_click(move |_, window, app| {
                click_herdr.update(app, |this, cx| {
                    this.apply_focus_intent(FocusIntent::tab(click_tab_id.clone()), window, cx);
                });
            })
            .on_mouse_down(MouseButton::Middle, move |_, window, app| {
                middle_close_herdr.update(app, |this, cx| {
                    this.confirm_close_tab(middle_close_id.clone(), window, cx);
                });
            })
            .when_some(drag, |item, drag| {
                item.on_drag(drag, |drag, position, _, cx| {
                    let drag = drag.clone().position(position);
                    cx.new(|_| drag)
                })
                .can_drop(|value, _, _| value.downcast_ref::<NativeTabDrag>().is_some())
                .drag_over::<NativeTabDrag>(move |style, drag, _, _| {
                    if drag.tab_id != drag_over_tab_id {
                        style.border_l_2().border_color(drop_color)
                    } else {
                        style
                    }
                })
                .on_drop(move |drag: &NativeTabDrag, _, app| {
                    if drag.tab_id == drop_tab_id {
                        return;
                    }
                    let dragged_tab_id = drag.tab_id.clone();
                    let workspace_id = drag.workspace_id.clone();
                    drop_herdr.update(app, |this, cx| {
                        this.move_tab_to_index(dragged_tab_id, workspace_id, index, cx);
                    });
                })
            })
    }
}

/// Scroll the strip by roughly one visible page (`pages` = ±1), clamped to the content.
fn scroll_native_tabs(handle: &ScrollHandle, pages: f32) {
    let page = handle.bounds().size.width.max(px(160.0));
    let target = handle.offset().x + page * pages;
    let max = handle.max_offset().width;
    let clamped = if target < px(0.0) {
        px(0.0)
    } else if target > max {
        max
    } else {
        target
    };
    handle.set_offset(point(clamped, px(0.0)));
}
