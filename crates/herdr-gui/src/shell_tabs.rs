//! Native content-area Tab strip (Terminal setting `terminal.tab_bar_placement`).
//!
//! In `native` mode the active Project's Tabs render as a gpui-component `TabBar` above the
//! hosted Herdr TUI instead of as Sidebar rows. The strip is presentation only: the Tab order,
//! titles, and lifecycle stay Herdr-authoritative (`state.tabs`, `tab.moved` events, `tab.move`/
//! `tab.close`/`tab.rename` RPCs), exactly mirroring the Sidebar tab row's actions (Pin, Rename,
//! Close, drag reorder) so hosting the strip never removes runtime capabilities.
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
use ::gpui::{img, Div, Point};
use gpui_component::tab::{Tab as TabItem, TabBar};
use gpui_component::tooltip::Tooltip;

/// Layout height of the native Tab strip (including its bottom divider). Terminal geometry
/// (`terminal_size`/`terminal_canvas_origin`) subtracts this so the hosted TUI grid stays
/// inside the visible area below the strip. 32px equals the default `Tab` variant height, so
/// a Tab's highlight fills the strip edge-to-edge (Ghostty-style integrated block).
pub(super) const NATIVE_TAB_BAR_HEIGHT: f32 = 32.0;

/// Long Tab titles clip at this width; Herdr's `tab.rename` remains the way to shorten them.
const NATIVE_TAB_MAX_WIDTH: f32 = 200.0;

/// Fixed width of the trailing `+` slot. The slot exists in both strip states (inline after
/// the last Tab, or as a blank spacer while `+` is pinned to the fixed trailing edge), so the
/// overflow decision cannot flip-flop with the `+`'s own width.
const NATIVE_TAB_PLUS_SLOT: f32 = 26.0;

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
        let dark = theme.bg <= 0x808080;
        let tab_ids = tabs
            .iter()
            .map(|tab| tab.tab_id.clone())
            .collect::<Vec<_>>();

        let items = tabs
            .into_iter()
            .enumerate()
            .map(|(index, tab)| self.native_tab_item(tab, index, workspace_id.clone(), dark, cx))
            .collect::<Vec<_>>();

        let new_tab_herdr = herdr.clone();
        let click_herdr = herdr.clone();
        // Page arrows appear only when the strip actually overflows in that direction.
        // GPUI notifies the view whenever a tracked scroll offset changes, so reading the
        // handle here keeps the arrows honest after wheel scrolling too.
        let scroll_pos = self.native_tab_scroll.offset().x;
        let scroll_max = self.native_tab_scroll.max_offset().width;
        let overflowing = scroll_max > px(0.0);
        let can_scroll_left = scroll_pos > px(0.0);
        let can_scroll_right = scroll_pos < scroll_max;
        let scroll_left_herdr = herdr.clone();
        let scroll_right_herdr = herdr.clone();
        let scroll_left_handle = self.native_tab_scroll.clone();
        let scroll_right_handle = self.native_tab_scroll.clone();
        let plus_button = || {
            Button::new("native-tab-new")
                .xsmall()
                .ghost()
                .icon(ComponentIconName::Plus)
                .tooltip("New Tab")
                .on_click(move |_, window, app| {
                    new_tab_herdr.update(app, |this, cx| {
                        this.new_tab(&NewTab, window, cx);
                    });
                })
        };
        // Default `Tab` variant: square corners, filled rectangular highlight, and a Tab
        // height equal to the strip height, so the highlight reads as one integrated block.
        let mut bar = TabBar::new("native-tab-bar")
            .track_scroll(&self.native_tab_scroll)
            .selected_index(selected_index)
            .children(items)
            // Tab switching goes through the one FocusIntent seam (same as Sidebar rows);
            // tab selection is client-local, the hosted TUI follows the focus chain.
            .on_click(move |index, window, app| {
                let Some(tab_id) = tab_ids.get(*index).cloned() else {
                    return;
                };
                click_herdr.update(app, |this, cx| {
                    this.apply_focus_intent(FocusIntent::tab(tab_id), window, cx);
                });
            });
        if can_scroll_left {
            bar = bar.prefix(
                Button::new("native-tab-scroll-left")
                    .xsmall()
                    .ghost()
                    .icon(ComponentIconName::ChevronLeft)
                    .tooltip("Previous tabs")
                    .on_click(move |_, _, app| {
                        scroll_native_tabs(&scroll_left_handle, -1.0);
                        scroll_left_herdr.update(app, |_, cx| cx.notify());
                    }),
            );
        }
        let mut strip_suffix = h_flex().gap_0p5();
        if can_scroll_right {
            strip_suffix = strip_suffix.child(
                Button::new("native-tab-scroll-right")
                    .xsmall()
                    .ghost()
                    .icon(ComponentIconName::ChevronRight)
                    .tooltip("Next tabs")
                    .on_click(move |_, _, app| {
                        scroll_native_tabs(&scroll_right_handle, 1.0);
                        scroll_right_herdr.update(app, |_, cx| cx.notify());
                    }),
            );
        }
        // The `+` sits right after the last Tab; once the strip overflows it pins to the
        // fixed trailing slot so it never scrolls out of reach. Both states reserve the same
        // fixed slot width, so the overflow test (which includes the slot) stays stable.
        let bar = if overflowing {
            strip_suffix = strip_suffix.child(plus_button());
            bar.last_empty_space(div().w(px(NATIVE_TAB_PLUS_SLOT)))
                .suffix(strip_suffix)
        } else {
            bar.last_empty_space(
                div()
                    .flex()
                    .justify_center()
                    .w(px(NATIVE_TAB_PLUS_SLOT))
                    .child(plus_button()),
            )
            .suffix(strip_suffix)
        };

        // The TabBar owns the strip's background and bottom divider; the prefix/suffix slots
        // keep the arrows and `+` inside that background so the divider stays continuous.
        // The passive measure canvas re-renders the strip once when the scroll max changes
        // (it is only written during paint, so a render-time read alone would lag a frame
        // with no follow-up notify — the same bootstrap pattern as the terminal measure canvas).
        let measure_herdr = cx.entity();
        let measure_scroll = self.native_tab_scroll.clone();
        let last_max = self.native_tab_scroll_max.clone();
        div()
            .relative()
            .h(px(NATIVE_TAB_BAR_HEIGHT))
            .flex_none()
            .w_full()
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
            .child(bar.h_full().flex_1())
    }

    fn native_tab_item(
        &self,
        tab: Tab,
        index: usize,
        workspace_id: Option<String>,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> TabItem {
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

        // Inner horizontal insets (Ghostty-tab look): the variant's label padding only covers
        // the text box, so the prefix icon and trailing suffix carry their own margins.
        let lead: AnyElement = match brand_icon {
            Some(path) => img(path).size(px(14.0)).ml(px(7.0)).into_any_element(),
            None => Icon::empty()
                .path("icons/terminal.svg")
                .with_size(px(13.0))
                .ml(px(7.0))
                .text_color(cx.theme().muted_foreground)
                .into_any_element(),
        };

        let mut suffix = h_flex().gap_1().mr(px(6.0));
        if let Some(level) = agent_status {
            suffix = suffix.child(status_glyph_container(
                SharedString::from(format!("native-tab-status-{tab_id}")),
                level,
                cx,
            ));
        }
        let close_herdr = cx.entity();
        let close_id = tab_id.clone();
        let close_hover_bg = cx.theme().foreground.opacity(0.12);
        // Ghostty-tab convention: the close affordance appears only while the pointer is on
        // the tab (group hover), and the confirming state keeps it visible for the second click.
        suffix = suffix.child(
            div()
                .id(SharedString::from(format!("native-tab-close-{tab_id}")))
                .size(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.0))
                .cursor_pointer()
                .opacity(if close_confirming { 1.0 } else { 0.0 })
                .group_hover("native-tab-item", |s| s.opacity(1.0))
                .hover(move |s| {
                    let s = s.opacity(1.0);
                    if close_confirming {
                        s
                    } else {
                        s.bg(close_hover_bg)
                    }
                })
                .when(close_confirming, |s| s.bg(cx.theme().danger.opacity(0.18)))
                .active(|s| s.bg(cx.theme().danger.opacity(0.24)))
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
                            "Close Tab"
                        })
                    })
                    .into()
                })
                .child(
                    Icon::empty()
                        .path("icons/x.svg")
                        .with_size(px(11.0))
                        .text_color(if close_confirming {
                            cx.theme().danger
                        } else {
                            cx.theme().muted_foreground
                        }),
                ),
        );

        // The context menu mounts on a transparent overlay covering the tab (gpui-component's
        // ContextMenuExt cannot wrap a `Tab` child inside TabBar, and Tab only renders extra
        // children when no `icon` is set — the prefix/suffix slots stay outside the overlay).
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
            .item(PopupMenuItem::separator())
            .item({
                let tab_id = menu_tab_id.clone();
                menu_action("Close Tab", &menu_herdr, move |this, window, cx| {
                    this.close_tab_by_id(tab_id.clone(), window, cx);
                })
            })
        });

        let drag = workspace_id.clone().map(|workspace_id| NativeTabDrag {
            tab_id: tab_id.clone(),
            workspace_id,
            label: title.clone(),
            position: Point::default(),
        });
        let drop_color = cx.theme().primary;
        let drag_over_tab_id = tab_id.clone();
        let drop_tab_id = tab_id.clone();
        let drop_herdr = cx.entity();
        let middle_close_herdr = cx.entity();
        let middle_close_id = tab_id.clone();

        TabItem::new()
            .relative()
            .group("native-tab-item")
            .max_w(px(NATIVE_TAB_MAX_WIDTH))
            .label(title.clone())
            .prefix(lead)
            .suffix(suffix)
            .child(menu_overlay)
            // Clipped long titles stay readable via the tooltip.
            .tooltip(crate::ui::tooltip::tooltip_fn(title))
            // Middle-click close also goes through the two-click confirm (deletion is always
            // confirmed); the confirming state keeps the ✕ visible for the second click.
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
                        // Drop-before semantics: insertion line on the leading edge.
                        style.border_l_1().border_color(drop_color)
                    } else {
                        style
                    }
                })
                .on_drop(move |drag: &NativeTabDrag, _, app| {
                    if drag.tab_id == drop_tab_id {
                        return;
                    }
                    // `move_tab_to_index` re-validates workspace membership against the
                    // authoritative projection before issuing `tab.move`.
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
