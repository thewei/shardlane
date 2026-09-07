//! [INPUT]: Depends on the ShardlaneApp type from the crate root (super) and existing types/imports (use super::*); no independent external dependencies.
//! [OUTPUT]: Exposes ShardlaneApp's shell chrome render tree: client_shell/sidebar/search bar/overlay assembly (including the native-tab-strip placement of the work surface and the full-content file_preview_page branch), plus the hosted Herdr TUI's GPUI native Pane context menu (Copy/Paste/Select All + Herdr Rename/Move/Swap/Split/Zoom/Process Info/Copy IDs + Close); mouse down/up wiring covers the full lifecycle of drag-selection and protocol mouse; terminal geometry (terminal_size/terminal_canvas_origin) accounts for the native tab strip height.
//! [POS]: The `crates/herdr-gui` shell render responsibility domain, mechanically split out of main.rs; together with sibling shell_* modules it forms ShardlaneApp's method surface.
use super::*;
use crate::ui::menus::{menu_action, menu_action_cx};

#[derive(Clone)]
struct TuiContextPane {
    pane_id: String,
    workspace_id: String,
    label: String,
    move_targets: Vec<(String, String)>,
}

impl ShardlaneApp {
    fn tui_context_pane_at_position(
        &self,
        position: crepuscularity_gpui::Point<Pixels>,
    ) -> Option<TuiContextPane> {
        let tab_id = self.state.focused_tab_id.as_deref()?;
        let layout = self.state.layout_for_tab(tab_id)?;
        let geometry = self.full_terminal_selection_geometry();
        let local_x = (position.x.to_f64() - geometry.origin_x).max(0.0);
        let local_y = (position.y.to_f64() - geometry.origin_y).max(0.0);
        let raw_col = (local_x / self.terminal_cell_width()).floor().max(0.0) as u32
            + u32::from(self.tui_chrome_projection.left);
        let raw_row = (local_y / self.terminal_cell_height()).floor().max(0.0) as u32
            + u32::from(self.tui_chrome_projection.top);
        let layout_pane = layout.panes.iter().find(|pane| {
            raw_col >= pane.rect.x
                && raw_col < pane.rect.x.saturating_add(pane.rect.width)
                && raw_row >= pane.rect.y
                && raw_row < pane.rect.y.saturating_add(pane.rect.height)
        })?;
        let pane = self
            .state
            .panes
            .iter()
            .find(|pane| pane.pane_id == layout_pane.pane_id)?;
        let workspace_id = pane
            .workspace_id
            .clone()
            .or_else(|| layout.workspace_id.clone())?;
        let label = pane
            .label
            .clone()
            .or_else(|| pane.title.clone())
            .or_else(|| pane.terminal_title.clone())
            .unwrap_or_else(|| pane.pane_id.clone());
        let move_targets = self
            .state
            .tabs
            .iter()
            .filter(|tab| {
                tab.workspace_id.as_deref() == Some(workspace_id.as_str()) && tab.tab_id != tab_id
            })
            .map(|tab| (tab.tab_id.clone(), self.tab_title(tab)))
            .collect();
        Some(TuiContextPane {
            pane_id: pane.pane_id.clone(),
            workspace_id,
            label,
            move_targets,
        })
    }

    pub(super) fn terminal_content_padding(&self) -> f32 {
        self.config.terminal.padding.clamp(0.0, 32.0)
    }

    pub(super) fn terminal_cell_width(&self) -> f64 {
        f64::from(self.terminal_geometry.cell_width)
    }

    pub(super) fn terminal_cell_height(&self) -> f64 {
        f64::from(self.terminal_geometry.cell_height)
    }

    pub(super) fn client_shell(
        &mut self,
        theme: UiTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let sidebar = if self.history.open {
            self.history_sidebar(theme, cx)
        } else if self.show_settings {
            self.settings_sidebar(window, cx)
        } else {
            self.cached_sidebar().into_any_element()
        };
        let terminal = if self.show_settings {
            // render_settings_content: keep a titlebar-height drag strip at the top of the content
            // column (armed-move; without a Header the content no longer touches the window top).
            // The whole column is painted with the page background first (content palette) — the
            // drag strip matches the page color, so no root-background band shows at the top.
            let titlebar_strip = self.titlebar_drag_strip("settings-content-titlebar", cx);
            let settings_bg = self.content_surface_theme(window).background;
            v_flex()
                .size_full()
                .bg(settings_bg)
                .child(titlebar_strip)
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .child(self.settings_page(window, cx)),
                )
                .into_any_element()
        } else if self.history.open {
            self.history_page(theme, window, cx)
        } else if self.new_agent_open {
            self.new_agent_page(window, cx)
        } else if self.file_preview.is_some() {
            // Full-content file preview: a native secondary surface covering
            // the hosted TUI (which stays alive underneath) — the sanctioned
            // cover-without-teardown semantics.
            self.file_preview_page(theme, window, cx)
        } else {
            // TUI-only cutover: the normal work surface's only Terminal presentation
            // is the hosted Herdr TUI. Chat is a semantic sidecar presentation of the same Herdr Agent
            // (not a second Terminal implementation): the host stays alive throughout and switching only
            // flips the local presentation.
            self.ensure_tui_surface(window, cx, false);
            // Chat binding's full focus revalidation (bind/rebind/park): navigating to an unsupported
            // Pane must clear the old binding, and returning to a supported Agent rebinds automatically (CHAT-A08).
            if self.chat.model.mode == crate::chat::WorkSurfaceMode::Chat {
                self.ensure_chat_source(cx);
            }
            let chat_active = self.chat.model.mode == crate::chat::WorkSurfaceMode::Chat
                && self.chat.model.binding.is_some();
            let content = if chat_active {
                let content_theme = self.content_surface_theme(window);
                self.chat_surface_view(&content_theme, window, cx)
            } else {
                self.tui_surface_view(theme, cx)
            };
            let term_opacity = self.terminal_fade_opacity();
            let surface = div()
                .relative()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .opacity(term_opacity)
                .child(content)
                .when(self.navigation_loading, |el| {
                    el.child(self.navigation_overlay(cx))
                })
                // Help is a retained native overlay; it used to hang off the deleted Embedded
                // template and is now mounted at the content area root (same layer as the navigation overlay).
                .when(self.show_help, |el| {
                    el.child(crate::help::help_overlay(
                        cx.theme().colors,
                        &self.config.shortcuts,
                    ))
                });
            // Native-tab placement (terminal.tab_bar_placement): a Tab strip above the work
            // surface. Pure presentation over Herdr's authoritative Tab order; `native_tab_bar_height`
            // keeps the terminal geometry (grid size + hit-test origin) in sync.
            if self.native_tabs_enabled() {
                let tab_bar = self.native_tab_bar(theme, cx);
                v_flex()
                    .size_full()
                    .child(tab_bar)
                    .child(surface)
                    .into_any_element()
            } else {
                v_flex().size_full().child(surface).into_any_element()
            }
        };
        let sidebar_width = self.shell_sidebar_width as f32;

        // The shell is a pure flex tiling: the sidebar is fixed-width flex-none, and the content
        // area takes the remaining width as flex-1. No more h_resizable — its built-in resize_handle's
        // always-visible 1px line (theme.border) was the source of the white line between sidebar and
        // content; the divider and drag-resize are both handled by the root-level #sidebar-divider
        // overlay, and the width truth lives only in shell_sidebar_width/config.
        // Slide animation (200ms WidthTween): the container width = this frame's rendered width,
        // clipped with overflow_hidden while sliding; the inner layer keeps the target full width —
        // the sidebar list doesn't reflow per frame.
        let sidebar_rendered = self.sidebar_rendered_width as f32;
        let shell = div()
            .id("herdr-client-shell")
            .size_full()
            .flex()
            .overflow_hidden()
            .when(sidebar_rendered > 0.0, |shell| {
                shell.child(
                    div()
                        .relative()
                        .h_full()
                        .flex_none()
                        .w(px(sidebar_rendered))
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .flex_none()
                                .w(px(sidebar_width))
                                .child(sidebar),
                        ),
                )
            })
            .child(
                div()
                    .relative()
                    .flex()
                    .h_full()
                    .flex_1()
                    .min_w(px(TERMINAL_MIN_WIDTH as f32))
                    .overflow_hidden()
                    .child(terminal),
            );

        // The divider doesn't live at this layer: Header is client_shell's sibling above (ui.crepus root
        // template), and placing it here would break the line at the Header's bottom edge. The full-height
        // divider is handled by the root-level sidebar_divider_overlay; the drag move/up listeners also
        // moved up to the root so the overlay can receive them.
        div().relative().size_full().overflow_hidden().child(shell)
    }

    /// Global search overlay (notate 08-29 round five): mounted at the ui.crepus root rather than inside
    /// client_shell — the scrim must cover the app Header (TitleBar); otherwise the upper half of the
    /// window behind the popover stays interactive and visually split.
    pub(super) fn client_picker_overlay(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.client_picker.as_ref() {
            Some(picker) => render_client_picker_overlay(picker, window, cx),
            None => div().into_any_element(),
        }
    }

    pub(super) fn navigation_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(cx.theme().background.opacity(0.92))
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Spinner::new().xsmall())
                    .child(Label::new("Switching…")),
            )
            .into_any_element()
    }

    pub(super) fn settings_sidebar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let back_herdr = cx.entity();
        let component_theme = cx.theme();
        // Settings sidebar: back row (h34/px9/r8/13px) + bordered search field (h28) +
        // navigation rows (h36/px11/r8/gap10/13px, selected 6% bg, hover 6%, active 9%);
        // navigation filters by the search term (visible_settings_pages semantics).
        let query = self
            .settings_sidebar_search
            .as_ref()
            .map(|s| s.read(cx).value().to_lowercase())
            .unwrap_or_default();
        let sections = SettingsSection::ALL
            .into_iter()
            .filter(|section| query.is_empty() || section.label().to_lowercase().contains(&query))
            .collect::<Vec<_>>();
        let mut navigation = div().flex().flex_col().gap(px(3.0));
        for (ix, section) in sections.into_iter().enumerate() {
            let selected = section == self.settings_section;
            let herdr = cx.entity();
            navigation = navigation.child(
                div()
                    .id(("settings-sidebar-section", ix))
                    .w_full()
                    .h(px(36.0))
                    .px(px(11.0))
                    .rounded(px(8.0))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .text_size(crate::theme::FONT_LIST_TITLE)
                    .text_color(if selected {
                        component_theme.foreground
                    } else {
                        component_theme.muted_foreground
                    })
                    .when(selected, |row| {
                        row.bg(component_theme.foreground.opacity(crate::theme::WASH_HOVER))
                    })
                    .hover(|style| {
                        style.bg(component_theme.foreground.opacity(crate::theme::WASH_HOVER))
                    })
                    .active(|style| {
                        style.bg(component_theme
                            .foreground
                            .opacity(crate::theme::WASH_ACTIVE))
                    })
                    .child(section.icon().with_size(px(15.0)).text_color(if selected {
                        component_theme.muted_foreground
                    } else {
                        component_theme.muted_foreground.opacity(0.65)
                    }))
                    .child(div().flex_1().min_w_0().truncate().child(section.label()))
                    .shardlane_interactive(
                        component_theme.primary.opacity(INTERACTIVE_FOCUS_OPACITY),
                        move |_, app| {
                            herdr.update(app, |this, cx| this.set_settings_section(section, cx));
                        },
                    ),
            );
        }

        // The Settings page has no shared Header (mirroring settings-sidebar-titlebar):
        // an APP_TITLEBAR_HEIGHT-tall window drag strip at the top, with traffic-light clearance
        // on the left; armed-move mode (mouse_down records armed, move calls start_window_move).
        let titlebar = self.titlebar_drag_strip("settings-sidebar-titlebar", cx);

        v_flex()
            .size_full()
            .bg(component_theme.sidebar)
            .child(titlebar)
            .child(
                v_flex()
                    .w_full()
                    .flex_shrink_0()
                    .px(px(12.0))
                    .pt(px(8.0))
                    .pb(px(10.0))
                    .gap_2()
                    .child(sidebar::secondary_sidebar_back_row(
                        "settings-back-to-app",
                        move |_, app| {
                            back_herdr.update(app, |this, cx| this.return_to_app_surface(cx));
                        },
                        cx,
                    ))
                    .child(
                        // settings-search-field (TextField): h28/px8/r6, border_1 (focused accent /
                        // otherwise border_strong), inset bg, gap6, 12.5px/lh16; a leading 13px search
                        // icon in tertiary; an embedded borderless Input.
                        self.settings_sidebar_search
                            .as_ref()
                            .map(|search| {
                                let focused = search.read(cx).focus_handle(cx).is_focused(window);
                                div()
                                    .id("settings-search-field")
                                    .h(px(28.0))
                                    .px(px(8.0))
                                    .rounded(px(6.0))
                                    .border_1()
                                    .border_color(if focused {
                                        component_theme.primary
                                    } else {
                                        component_theme.border
                                    })
                                    .bg(component_theme.foreground.opacity(0.045))
                                    .flex()
                                    .items_center()
                                    .gap(SPACE_ICON)
                                    .text_size(crate::theme::FONT_BODY)
                                    .line_height(px(16.0))
                                    .child(
                                        Icon::empty()
                                            .path("icons/search.svg")
                                            .with_size(px(13.0))
                                            .text_color(component_theme.muted_foreground),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .flex_1()
                                            .child(Input::new(search).appearance(false).p_0()),
                                    )
                                    .into_any_element()
                            })
                            .unwrap_or_else(|| div().into_any_element()),
                    ),
            )
            .child(div().h(px(18.0)))
            .child(div().px(px(12.0)).child(navigation))
            .into_any_element()
    }

    /// Audit A15: the single armed-move titlebar drag strip builder (mouse_down arms,
    /// mouse_move triggers one start_window_move, mouse_up/out disarm) — previously duplicated
    /// verbatim for the Settings sidebar and content surfaces.
    pub(crate) fn titlebar_drag_strip(
        &self,
        id: &'static str,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .w_full()
            .flex_none()
            .h(px(APP_TITLEBAR_HEIGHT as f32))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, _| {
                    this.settings_titlebar_drag_armed = true;
                }),
            )
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, _| {
                this.settings_titlebar_drag_armed = false;
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    this.settings_titlebar_drag_armed = false;
                }),
            )
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, window, _| {
                if this.settings_titlebar_drag_armed {
                    this.settings_titlebar_drag_armed = false;
                    window.start_window_move();
                }
            }))
    }

    /// The active Project row source: focused id → focused flag → first row.
    pub(super) fn active_workspace(&self) -> Option<&Workspace> {
        if let Some(id) = self.state.focused_workspace_id.as_deref() {
            if let Some(workspace) = self
                .state
                .workspaces
                .iter()
                .find(|workspace| workspace.workspace_id == id)
            {
                return Some(workspace);
            }
        }
        self.state
            .workspaces
            .iter()
            .find(|workspace| workspace.focused)
            .or_else(|| self.state.workspaces.first())
    }

    pub(super) fn active_workspace_id(&self) -> Option<&str> {
        self.state.focused_workspace_id.as_deref().or_else(|| {
            self.active_workspace()
                .map(|workspace| workspace.workspace_id.as_str())
        })
    }

    pub(super) fn active_tab(&self) -> Option<&Tab> {
        if let Some(id) = self.state.focused_tab_id.as_deref() {
            if let Some(tab) = self.state.tabs.iter().find(|tab| tab.tab_id == id) {
                return Some(tab);
            }
        }
        self.state
            .tabs
            .iter()
            .find(|tab| tab.focused)
            .or_else(|| self.state.tabs.first())
    }

    // Rendered from ui/ui.crepus while `initializing` (view_file! expands the template,
    // so this method has no .rs-level caller — do not delete it as dead code).
    pub(super) fn startup_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .top(px(APP_TITLEBAR_HEIGHT as f32))
            .left_0()
            .right_0()
            .bottom_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .child(Spinner::new().small())
                    .child(Label::new(match self.bound_project() {
                        Some(binding) => format!("Opening {}…", binding.project_name),
                        None => "Starting Herdr".to_string(),
                    }))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("Synchronizing Projects, Tabs and terminal state"),
                    ),
            )
    }

    pub(super) fn tabs_for_workspace(&self, workspace_id: &str) -> Vec<Tab> {
        // `state.tabs` preserves Herdr `tab.list` / `tab.moved` authority. Pinning is
        // Shardlane-only metadata and must never create a second visual order.
        self.state
            .tabs
            .iter()
            .filter(|tab| tab.workspace_id.as_deref() == Some(workspace_id))
            .cloned()
            .collect()
    }

    pub(super) fn visible_tabs(&self) -> Vec<Tab> {
        let Some(workspace_id) = self.active_workspace_id() else {
            return Vec::new();
        };
        let mut tabs = self.tabs_for_workspace(workspace_id);
        if tabs.is_empty() && !self.navigation_loading {
            tabs = self
                .state
                .tabs
                .iter()
                .filter(|tab| tab.workspace_id.is_none())
                .cloned()
                .collect();
        }
        tabs
    }

    pub(super) fn shell_sidebar_drag_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(SidebarDrag::Shell(start_x, start_width)) = self.sidebar_drag else {
            return;
        };
        // Semantics: the cap is also constrained by the viewport width so the terminal keeps its minimum
        // width (the old h_resizable's size_range arbitration was removed with it; this takes over).
        let maximum = SIDEBAR_MAX_WIDTH
            .min(window.bounds().size.width.to_f64() - TERMINAL_MIN_WIDTH)
            .max(SIDEBAR_MIN_WIDTH);
        let next =
            (start_width + (event.position.x.to_f64() - start_x)).clamp(SIDEBAR_MIN_WIDTH, maximum);
        if (next - self.shell_sidebar_width).abs() > 0.5 {
            self.shell_sidebar_width = next;
            self.config.ui.sidebar.width = next;
            self.sync_terminal_geometry(window, cx);
            self.notify_sidebar(cx);
            cx.notify();
        }
    }

    /// Right panel resize: dragging left widens it (dx negated); the cap is also constrained by viewport −
    /// terminal minimum width − sidebar width (RIGHT_PANEL_MAX/MIN semantics, 280–1000).
    pub(super) fn right_panel_drag_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(SidebarDrag::RightPanel(start_x, start_width)) = self.sidebar_drag else {
            return;
        };
        let viewport_width = window.bounds().size.width.to_f64();
        let maximum = 1000_f64
            .min(viewport_width - TERMINAL_MIN_WIDTH - self.sidebar_width())
            .max(280.0);
        let next = (start_width - (event.position.x.to_f64() - start_x)).clamp(280.0, maximum);
        if (next - self.right_panel.width as f64).abs() > 0.5 {
            self.right_panel.width = next as f32;
            self.config.ui.right_panel.width = next;
            cx.notify();
        }
    }

    pub(super) fn shell_sidebar_drag_end(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_drag.take().is_some() {
            self.schedule_config_save(cx);
            self.notify_sidebar(cx);
            cx.notify();
        }
    }

    pub(super) fn sidebar_width(&self) -> f64 {
        if self.sidebar_collapsed || self.sidebar_auto_collapsed {
            0.0
        } else {
            self.shell_sidebar_width
        }
    }

    pub(super) fn terminal_fade_opacity(&self) -> f32 {
        const FADE_DURATION_MS: f32 = 120.0;
        match &self.terminal_fade_start {
            Some(started) => {
                let elapsed = started.elapsed().as_secs_f32() * 1000.0;
                (elapsed / FADE_DURATION_MS).min(1.0)
            }
            None => 1.0,
        }
    }

    /// Whether the sidebar currently occupies space in the shell: ⌘+B manual collapse applies on all surfaces.
    pub(super) fn shell_sidebar_visible(&self) -> bool {
        !self.sidebar_collapsed && !self.sidebar_auto_collapsed
    }

    /// Sidebar|content divider (resize-handle semantics) · root-level overlay: spans the full height
    /// across the Header and the content area (the handle hangs on the content column that includes
    /// the Header, achieving the same effect).
    /// Always-visible subtle line (mirroring sidebar_border), darkening on hover, strongest while dragging;
    /// the hit strip is the drag hot zone — mouse_down enters Shell resize and stop_propagation, so
    /// when overlapping the Header's drag strip it takes priority as the divider (the handle presses
    /// over the header the same way).
    /// Right panel full-height overlay (layout): placed side by side with the (Header+shell) left
    /// column, which narrows as it slides in — the right panel owns its own Header, just as the
    /// content column does. Container width = this frame's rendered width, clipped while sliding; the
    /// panel body is pinned to the right edge and reveals from right to left.
    pub(super) fn right_panel_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rendered = self.right_panel_rendered_width as f32;
        if rendered <= 0.0 {
            // Panel fully collapsed: hide all webviews (overlays don't leave the element tree on their own).
            crate::right_panel::webview::hide_all(&self.browser_webviews);
            return div().into_any_element();
        }
        let sliding = self.right_panel_slide.is_some();
        let target = self.right_panel.width.clamp(280.0, 1000.0);
        div()
            .id("right-panel-overlay")
            .h_full()
            .flex_none()
            .relative()
            .w(px(rendered))
            .when(sliding, |c| c.overflow_hidden())
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .h_full()
                    .w(px(target))
                    .child(self.render_right_panel(window, cx)),
            )
            .into_any_element()
    }

    pub(super) fn sidebar_divider_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        // Rendered width > 0 means on screen (including mid-slide): the divider follows this frame's rendered width.
        if self.sidebar_rendered_width <= 0.0 {
            return div().into_any_element();
        }
        let sidebar_width = self.sidebar_rendered_width as f32;
        let foreground = cx.theme().foreground;
        let line_quiet = foreground.opacity(0.08);
        let line_hover = foreground.opacity(0.18);
        let line_active = foreground.opacity(0.30);
        let is_resizing = matches!(self.sidebar_drag, Some(SidebarDrag::Shell(..)));
        let divider_down = cx.listener(|this, event: &MouseDownEvent, _, cx| {
            // begin_panel_resize semantics: dragging tracks the pointer directly; an unfinished slide yields.
            this.sidebar_slide = None;
            this.sidebar_drag = Some(SidebarDrag::Shell(
                event.position.x.to_f64(),
                this.shell_sidebar_width,
            ));
            cx.stop_propagation();
            cx.notify();
        });
        div()
            .id("sidebar-divider")
            .group("sidebar-divider")
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(sidebar_width - 3.0))
            .w(px(6.0))
            .cursor(gpui::CursorStyle::ResizeLeftRight)
            .on_mouse_down(MouseButton::Left, divider_down)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(2.5))
                    .w(px(1.0))
                    .bg(if is_resizing { line_active } else { line_quiet })
                    .group_hover("sidebar-divider", move |s| {
                        s.bg(if is_resizing { line_active } else { line_hover })
                    }),
            )
            .into_any_element()
    }

    pub(super) fn chrome_height(&self) -> f64 {
        APP_TITLEBAR_HEIGHT
    }

    pub(super) fn terminal_canvas_origin(&self) -> (f64, f64) {
        // The native Tab strip sits between the title bar and the terminal canvas, so the
        // grid origin (mouse hit-testing, selection mapping) starts below it.
        (
            self.sidebar_width(),
            self.chrome_height() + self.native_tab_bar_height(),
        )
    }

    pub(super) fn terminal_size(&self, window: &Window) -> TerminalSize {
        let size = window.bounds().size;
        // Terminal geometry must reflect the real visible surface. Artificial minimum pixel/
        // cell sizes make a small window generate a larger Ghostty grid than GPUI can paint,
        // which clips the right edge and bottom rows.
        let width = (size.width.to_f64() - self.sidebar_width()).max(1.0);
        let height =
            (size.height.to_f64() - self.chrome_height() - self.native_tab_bar_height()).max(1.0);
        let padding = f64::from(self.terminal_content_padding()) * 2.0;
        let content_width = (width - padding).max(1.0);
        let content_height = (height - padding - TERMINAL_PAINT_FUDGE).max(1.0);
        // Single pixel convention (audit B19): content-exact pixels via the shared helper,
        // identical to the steady-state canvas path, so attach can never send a different
        // pixel size for the same grid.
        grid_size_for(
            content_width,
            content_height,
            self.terminal_cell_width(),
            self.terminal_cell_height(),
        )
    }

    /// Built inside SidebarPane::render so spaces toggle reflows tabs in-pane only.
    pub(super) fn build_sidebar(
        &self,
        projection: sidebar::SidebarProjection<'_>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let started = Instant::now();
        let theme = self.theme(window);
        let width = self.shell_sidebar_width as f32;
        let sidebar = self.sidebar(projection, theme, cx).into_any_element();
        let built = div()
            .h_full()
            .w(px(width))
            .flex()
            .overflow_hidden()
            .child(sidebar)
            .into_any_element();
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        if ms > 1.0 {
            lag_log(format_args!(
                "build_sidebar {ms:.2}ms width={width:.0} workspaces={} tabs={} agents={}",
                self.state.workspaces.len(),
                self.state.tabs.len(),
                self.state.agents.len(),
            ));
        }
        built
    }

    /// Paint-cache sidebar entity so terminal paints do not rebuild the whole sidebar.
    ///
    /// IMPORTANT: GPUI cached views use this StyleRefinement as the *layout shell*
    /// when not dirty. Width must live here — otherwise the sidebar collapses to 0.
    pub(super) fn cached_sidebar(&self) -> AnyView {
        AnyView::from(self.sidebar_pane.clone())
            .cached(StyleRefinement::default().size_full().overflow_hidden())
    }

    pub(super) fn terminal_input_bridge(
        &self,
        pane_id: String,
        target: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let herdr = cx.entity();
        let focus_handle = self.focus_handle.clone();
        let cell_width = self.terminal_cell_width() as f32;
        let cell_height = self.terminal_cell_height() as f32;
        let content_padding = self.terminal_content_padding();
        canvas(
            |_, _, _| {},
            move |bounds, _, window, app| {
                let padding = px(content_padding);
                let input_bounds = Bounds::new(
                    point(bounds.origin.x + padding, bounds.origin.y + padding),
                    size(
                        bounds.size.width - padding - padding,
                        bounds.size.height - padding - padding,
                    ),
                );
                window.handle_input(
                    &focus_handle,
                    TerminalInputHandler {
                        herdr: herdr.clone(),
                        pane_id: pane_id.clone(),
                        target: target.clone(),
                        bounds: input_bounds,
                        cell_width,
                        cell_height,
                    },
                    app,
                );
            },
        )
        .absolute()
        .size_full()
        .into_any_element()
    }

    pub(super) fn ime_preedit_overlay(
        &self,
        target: &str,
        frame: &TerminalFrame,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.ime_target.as_deref() != Some(target) || self.ime_marked_text.is_empty() {
            return None;
        }
        let cursor = frame.cursor.unwrap_or((0, 0));
        let cell_width = self.terminal_cell_width() as f32;
        let cell_height = self.terminal_cell_height() as f32;
        let content_padding = self.terminal_content_padding();
        Some(
            div()
                .absolute()
                .left(px(content_padding + cursor.0 as f32 * cell_width))
                .top(px(content_padding + cursor.1 as f32 * cell_height))
                .h(px(cell_height))
                .px_1()
                .flex()
                .items_center()
                .font(self.terminal_geometry.font())
                .text_size(px(self.terminal_font_size() as f32))
                .text_color(cx.theme().foreground)
                .bg(cx.theme().background)
                .border_b_1()
                .border_color(cx.theme().ring)
                .child(self.ime_marked_text.clone())
                .into_any_element(),
        )
    }

    pub(super) fn terminal_only_view(&self, theme: UiTheme, cx: &Context<Self>) -> AnyElement {
        let measure_herdr = cx.entity();
        // TUI-only: the host canvas = measurement canvas + single-slot Ghostty frame projection. The
        // Embedded local-scrollback wheel, deep-history hint rows, and Split/Zoom/Close context-menu
        // forks were deleted with the per-pane render path; the wheel is owned by tui_surface_view's
        // SGR reporting, and right-click by its native Copy/Paste/Select All menu.
        let terminal = v_flex()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .text_color(rgb(theme.text))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        canvas(
                            move |bounds, _, app| {
                                let width = bounds.size.width.to_f64();
                                let height = bounds.size.height.to_f64();
                                measure_herdr.update(app, |this, cx| {
                                    this.sync_main_terminal_surface_size(width, height, cx);
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(cached_terminal(self.terminal_pane.clone())),
            );
        terminal.into_any_element()
    }

    /// Herdr TUI host surface: the content area renders the single host terminal directly (audit TUI-01 +
    /// user ruling 2026-08-26: no extra UI whatsoever — no composer/hint rows/search bar; input, wheel,
    /// selection, and IME all reuse the single-slot terminal's existing channels, target = TUI_TARGET).
    /// When the host fails, an inline recovery placeholder (Restart) is shown.
    pub(super) fn tui_surface_view(
        &mut self,
        theme: UiTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let host_dead = self.terminal_target.as_deref() != Some(herdr_tui::TUI_TARGET)
            && self.terminal_attach_target.as_deref() != Some(herdr_tui::TUI_TARGET)
            && self.tui_host.status == crate::herdr_tui::HerdrTuiHostStatus::Failed;
        if host_dead {
            return self.tui_failure_view(cx);
        }
        let input_bridge = self.terminal_input_bridge(
            herdr_tui::TUI_TARGET.to_string(),
            herdr_tui::TUI_TARGET.to_string(),
            cx,
        );
        let preedit = self.ime_preedit_overlay(herdr_tui::TUI_TARGET, &self.terminal_frame, cx);
        let has_selection = self.focused_has_selection();
        let menu_herdr = cx.entity();
        div()
            .relative()
            .flex()
            .flex_1()
            .h_full()
            .bg(rgb(self.hosted_terminal_background_rgb_from_theme(theme)))
            .cursor_text()
            // UX fix: with the root context always ShardlaneApp, the "HerdrTui"-specific bindings
            // (cmd-v/c/a, cmd-=/-/0, cmd-f, cmd-up/down) mount on this surface's sub-context,
            // coexisting with the root ancestor stack (GPUI's dispatch_tree matches any ancestor context per layer).
            .key_context("HerdrTui")
            // Keyboard dispatch for the hosted TUI lives in the app-global keystroke observer
            // (`handle_tui_keyboard`): GPUI 0.2.2 only delivers key events along the root focus
            // path, so element-scoped `.on_key_down` here never fired. Printable text is owned
            // by AppKit/GPUI InputHandler (IME/NSTextInputClient); named and modified keys are
            // encoded by Ghostty in the observer branch. Mouse/wheel stay hit-test based here.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, window, _| {
                    window.focus(&this.focus_handle);
                }),
            )
            .on_scroll_wheel(cx.listener(Self::handle_tui_scroll_wheel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::handle_tui_mouse_down))
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(Self::handle_tui_mouse_down),
            )
            // Plain right-click is deliberately not forwarded to the hosted PTY. Herdr's TUI
            // menu is therefore suppressed and GPUI owns the standard macOS terminal menu.
            // Right-button motion is also withheld so the SGR mouse stream never receives a
            // drag without a matching press.
            .on_mouse_move(
                cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
                    if event
                        .pressed_button
                        .as_ref()
                        .is_some_and(shell_tui::tui_native_context_menu_owns_button)
                    {
                        return;
                    }
                    let geometry = this.full_terminal_selection_geometry();
                    this.handle_terminal_mouse_move_target(
                        herdr_tui::TUI_TARGET,
                        geometry,
                        event,
                        cx,
                    );
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, event, _window, cx| {
                    let geometry = this.full_terminal_selection_geometry();
                    this.handle_terminal_mouse_up_target(
                        herdr_tui::TUI_TARGET,
                        geometry,
                        event,
                        cx,
                    );
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, event, _window, cx| {
                    let geometry = this.full_terminal_selection_geometry();
                    this.handle_terminal_mouse_up_target(
                        herdr_tui::TUI_TARGET,
                        geometry,
                        event,
                        cx,
                    );
                }),
            )
            .context_menu(move |menu, window, cx| {
                let copy_herdr = menu_herdr.clone();
                let paste_herdr = menu_herdr.clone();
                let select_all_herdr = menu_herdr.clone();
                let mut menu = menu
                    .item(
                        PopupMenuItem::new("Copy")
                            .disabled(!has_selection)
                            .on_click(move |_, window, app| {
                                copy_herdr.update(app, |this, cx| this.copy(&Copy, window, cx));
                            }),
                    )
                    .item(PopupMenuItem::new("Paste").on_click(move |_, window, app| {
                        paste_herdr.update(app, |this, cx| this.paste(&Paste, window, cx));
                    }))
                    .item(
                        PopupMenuItem::new("Select All").on_click(move |_, window, app| {
                            select_all_herdr
                                .update(app, |this, cx| this.select_all(&SelectAll, window, cx));
                        }),
                    );

                let Some(context_pane) = menu_herdr
                    .read(cx)
                    .tui_context_pane_at_position(window.mouse_position())
                else {
                    return menu;
                };
                let TuiContextPane {
                    pane_id,
                    workspace_id,
                    label,
                    move_targets,
                } = context_pane;
                // Copy IDs: the pane's Tab and this window's bound instance are
                // resolved at menu-build time so unavailable items never render.
                let (pane_tab_id, session_id) = {
                    let app = menu_herdr.read(cx);
                    (
                        app.state
                            .panes
                            .iter()
                            .find(|pane| pane.pane_id == pane_id)
                            .and_then(|pane| pane.tab_id.clone()),
                        app.binding
                            .as_ref()
                            .map(|binding| binding.session_name().to_string()),
                    )
                };

                menu = menu
                    .item(PopupMenuItem::separator())
                    .item({
                        let pane_id = pane_id.clone();
                        menu_action("Rename Pane…", &menu_herdr, move |this, window, cx| {
                            this.open_pane_rename(pane_id.clone(), label.clone(), window, cx)
                        })
                    })
                    .item({
                        let pane_id = pane_id.clone();
                        let workspace_id = workspace_id.clone();
                        menu_action("Move to New Tab", &menu_herdr, move |this, window, cx| {
                            this.move_pane_to_new_tab_by_id(
                                pane_id.clone(),
                                workspace_id.clone(),
                                window,
                                cx,
                            )
                        })
                    });

                if !move_targets.is_empty() {
                    menu = menu.submenu("Move to Tab", window, cx, {
                        let herdr = menu_herdr.clone();
                        let pane_id = pane_id.clone();
                        move |submenu, _, _| {
                            move_targets
                                .iter()
                                .fold(submenu, |submenu, (tab_id, title)| {
                                    let pane_id = pane_id.clone();
                                    let tab_id = tab_id.clone();
                                    submenu.item(menu_action(
                                        title.clone(),
                                        &herdr,
                                        move |this, window, cx| {
                                            this.move_pane_to_tab_by_id(
                                                pane_id.clone(),
                                                tab_id.clone(),
                                                window,
                                                cx,
                                            )
                                        },
                                    ))
                                })
                        }
                    });
                }

                menu.submenu("Swap Pane", window, cx, {
                    let herdr = menu_herdr.clone();
                    let pane_id = pane_id.clone();
                    move |submenu, _, _| {
                        [
                            ("Left", "left"),
                            ("Right", "right"),
                            ("Up", "up"),
                            ("Down", "down"),
                        ]
                        .into_iter()
                        .fold(submenu, |submenu, (label, direction)| {
                            let pane_id = pane_id.clone();
                            submenu.item(menu_action_cx(label, &herdr, move |this, cx| {
                                this.swap_pane_direction_by_id(pane_id.clone(), direction, cx)
                            }))
                        })
                    }
                })
                .item(PopupMenuItem::separator())
                .item({
                    let pane_id = pane_id.clone();
                    menu_action_cx("Split Right", &menu_herdr, move |this, cx| {
                        this.split_pane_right_by_id(pane_id.clone(), cx)
                    })
                })
                .item({
                    let pane_id = pane_id.clone();
                    menu_action_cx("Split Down", &menu_herdr, move |this, cx| {
                        this.split_pane_down_by_id(pane_id.clone(), cx)
                    })
                })
                .item({
                    let pane_id = pane_id.clone();
                    menu_action_cx("Toggle Pane Zoom", &menu_herdr, move |this, cx| {
                        this.toggle_pane_zoom_by_id(pane_id.clone(), cx)
                    })
                })
                .item({
                    let pane_id = pane_id.clone();
                    menu_action("Process Info…", &menu_herdr, move |this, window, cx| {
                        this.show_pane_process_info_by_id(pane_id.clone(), window, cx)
                    })
                })
                .submenu(crate::i18n::t("shell.copy_ids"), window, cx, {
                    let herdr = menu_herdr.clone();
                    let pane_id = pane_id.clone();
                    move |submenu, _, _| {
                        let submenu = submenu.item({
                            let pane_id = pane_id.clone();
                            menu_action(
                                crate::i18n::t("shell.copy_pane_id"),
                                &herdr,
                                move |this, window, cx| {
                                    this.copy_runtime_id(pane_id.clone(), window, cx)
                                },
                            )
                        });
                        let submenu = match pane_tab_id.clone() {
                            Some(tab_id) => submenu.item({
                                let tab_id = tab_id.clone();
                                menu_action(
                                    crate::i18n::t("shell.copy_tab_id"),
                                    &herdr,
                                    move |this, window, cx| {
                                        this.copy_runtime_id(tab_id.clone(), window, cx)
                                    },
                                )
                            }),
                            None => submenu,
                        };
                        match session_id.clone() {
                            Some(session) => submenu.item({
                                let session = session.clone();
                                menu_action(
                                    crate::i18n::t("shell.copy_session_id"),
                                    &herdr,
                                    move |this, window, cx| {
                                        this.copy_runtime_id(session.clone(), window, cx)
                                    },
                                )
                            }),
                            None => submenu,
                        }
                    }
                })
                .item(PopupMenuItem::separator())
                .item({
                    let pane_id = pane_id.clone();
                    menu_action_cx("Close Pane", &menu_herdr, move |this, cx| {
                        this.close_pane_by_id(pane_id.clone(), workspace_id.clone(), cx)
                    })
                })
            })
            .child(input_bridge)
            .child(self.terminal_only_view(theme, cx))
            .when_some(preedit, |el, preedit| el.child(preedit))
            // B21: while the host is Starting (attach/restart before the first frame) the
            // content area otherwise paints only the blank terminal fill — visually
            // indistinguishable from a hung surface. A small centered, muted status label
            // keeps restart/cooldown recognizably distinct. Presentation only; existing
            // theme tokens.
            .when(
                self.tui_host.status == crate::herdr_tui::HerdrTuiHostStatus::Starting,
                |el| {
                    el.child(
                        div()
                            .absolute()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .text_size(theme::FONT_META)
                                    .text_color(rgb(theme.muted))
                                    .child(format!("Herdr {}", self.tui_host.status.label())),
                            ),
                    )
                },
            )
            .into_any_element()
    }

    /// TUI host failure placeholder: an inline recovery surface in the content area. After TUI-only
    /// convergence there is no Embedded fallback — only Restart and the error explanation (plan §19; the
    /// old "Switch to Shardlane Terminal" recovery action was removed along with mode selection).
    fn tui_failure_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let herdr = cx.entity();
        let error = self
            .tui_host
            .last_error
            .clone()
            .unwrap_or_else(|| "unknown error".to_string());
        v_flex()
            .flex_1()
            .h_full()
            .items_center()
            .justify_center()
            .gap(px(10.0))
            .text_color(theme.muted_foreground)
            .child(
                div()
                    .text_size(px(15.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.foreground)
                    .child("Herdr terminal unavailable"),
            )
            .child(
                div()
                    .text_size(theme::FONT_META)
                    .max_w(px(420.0))
                    .text_align(crepuscularity_gpui::TextAlign::Center)
                    .child(error),
            )
            .child(
                h_flex().gap(px(8.0)).pt(px(6.0)).child(
                    Button::new("tui-failure-restart")
                        .primary()
                        .small()
                        .label("Restart")
                        .on_click(move |_, window, app| {
                            herdr.update(app, |this, cx| {
                                this.restart_tui_surface(window, cx);
                            });
                        }),
                ),
            )
            .into_any_element()
    }
}

/// The Project picker page: the only surface an unbound ⌘N window renders, and an
/// overlay page any bound window can open to jump to another Project's window (or
/// rebind itself). One Herdr instance per Project; one Project per window.
pub(crate) fn project_picker_page_impl(
    this: &mut ShardlaneApp,
    window: &mut Window,
    cx: &mut Context<ShardlaneApp>,
) -> AnyElement {
    let component_theme = cx.theme().clone();
    // Creation mode: name first — the name is written into the session's
    // metadata file (workspace.json) when the session is created, and the
    // switcher/sidebar read it back from there.
    if this.picker_page == PickerPage::Creating {
        let herdr = cx.entity();
        if this.new_workspace_name.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Workspace name…"));
            let subscription = cx.subscribe_in(
                &input,
                window,
                |this: &mut ShardlaneApp,
                 _,
                 event: &gpui_component::input::InputEvent,
                 window,
                 cx| {
                    if matches!(event, gpui_component::input::InputEvent::PressEnter { .. }) {
                        this.create_workspace_now(window, cx);
                    }
                },
            );
            this.new_workspace_name = Some(input);
            this._new_workspace_name_sub = Some(subscription);
        }
        if let Some(input) = this.new_workspace_name.as_ref() {
            let handle = input.read(cx).focus_handle(cx);
            if !handle.is_focused(window) {
                input.update(cx, |state, cx| state.focus(window, cx));
            }
        }
        let name_control = this
            .new_workspace_name
            .as_ref()
            .map(|input| {
                Input::new(input)
                    .small()
                    .appearance(false)
                    .w_full()
                    .text_size(px(13.0))
                    .into_any_element()
            })
            .unwrap_or_else(|| div().into_any_element());
        let create_herdr = herdr.clone();
        let cancel_herdr = herdr.clone();
        return div()
            .id("shardlane-project-picker")
            .size_full()
            .key_context("ShardlaneApp")
            .on_action(cx.listener(|this, _: &PickerCancel, _, cx| {
                this.cancel_workspace_creation(cx);
            }))
            // Early render return skips the shell's action registrations;
            // keep the Window menu alive in creation/picker windows.
            .on_action(cx.listener(ShardlaneApp::new_window))
            .on_action(cx.listener(ShardlaneApp::merge_all_windows))
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(360.0))
                    .rounded(px(10.0))
                    .border_1()
                    .border_color(component_theme.border)
                    .p(px(12.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child("New Workspace"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(component_theme.muted_foreground)
                            .child(
                                "Pick a name — it is stored with the Herdr session. Enter ↵ to create.",
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(34.0))
                            .px(px(10.0))
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(component_theme.border)
                            .flex()
                            .items_center()
                            .child(name_control),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .id("workspace-create")
                                    .flex_1()
                                    .h(px(32.0))
                                    .rounded(px(6.0))
                                    .bg(component_theme.primary)
                                    .text_color(component_theme.primary_foreground)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(12.0))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(component_theme.primary.opacity(0.85)))
                                    .on_click(move |_, window, app| {
                                        create_herdr.update(app, |this, cx| {
                                            this.create_workspace_now(window, cx)
                                        });
                                    })
                                    .child("Create"),
                            )
                            .child(
                                div()
                                    .id("workspace-create-cancel")
                                    .w(px(90.0))
                                    .h(px(32.0))
                                    .rounded(px(6.0))
                                    .border_1()
                                    .border_color(component_theme.border)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(12.0))
                                    .text_color(component_theme.muted_foreground)
                                    .cursor_pointer()
                                    .hover(|s| {
                                        s.bg(component_theme
                                            .foreground
                                            .opacity(crate::theme::WASH_HOVER))
                                    })
                                    .on_click(move |_, window, app| {
                                        cancel_herdr.update(app, |this, cx| {
                                            this.cancel_workspace_creation(cx);
                                            this.close_project_picker(window, cx);
                                        });
                                    })
                                    .child("Cancel"),
                            ),
                    ),
            )
            .into_any_element();
    }
    // The full workspace LIST page was removed: switching happens in the
    // switcher panel (sidebar footer chip / header breadcrumbs). This page
    // remains only for unbound windows and workspace creation.
    let herdr = cx.entity();
    let new_project_herdr = herdr.clone();
    let close_herdr = herdr.clone();
    let unbound = this.binding.is_none();
    let dismiss_text = if unbound { "Close window" } else { "Close" };
    // The workspace switcher panel's content: the selected device's header
    // over its workspace rows. A new window offers every workspace, plus
    // creation below; clicking a workspace opens it here (or jumps to its
    // existing window).
    let machines = crate::switcher_panel::build_picker_machines(this);
    let selected_device = crate::switcher_panel::selected_panel_device(this);
    if this.project_picker_filter.is_none() {
        this.project_picker_filter =
            Some(cx.new(|cx| {
                InputState::new(window, cx).placeholder(crate::i18n::t("workspace.filter"))
            }));
    }
    let filter_value = this
        .project_picker_filter
        .as_ref()
        .map(|input| input.read(cx).value().trim().to_lowercase())
        .unwrap_or_default();
    let sections = crate::switcher_panel::workspace_switcher_device_sections(
        &herdr,
        &machines,
        &selected_device,
        &filter_value,
        None,
        cx.theme(),
        &this.collapsed_switcher_groups,
    );
    let filter_input = this.project_picker_filter.clone();
    div()
        .id("shardlane-project-picker")
        .size_full()
        .key_context("ShardlaneApp")
        .on_action(cx.listener(|this, _: &PickerCancel, window, cx| {
            this.close_project_picker(window, cx);
        }))
        // Early render return skips the shell's action registrations;
        // keep the Window menu alive in creation/picker windows.
        .on_action(cx.listener(ShardlaneApp::new_window))
        .on_action(cx.listener(ShardlaneApp::merge_all_windows))
        .bg(cx.theme().background)
        .text_color(cx.theme().foreground)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(360.0))
                .rounded(px(10.0))
                .border_1()
                .border_color(component_theme.border)
                .p(px(12.0))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .child(if unbound {
                            "Open a workspace".to_string()
                        } else {
                            "Workspace".to_string()
                        }),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(component_theme.muted_foreground)
                        .child(if unbound {
                            "Choose a workspace to open in this window, or create a new one."
                        } else {
                            "Switch workspaces from the switcher — the ● button at the bottom of the sidebar (or the breadcrumb above)."
                        }),
                )
                // No scroll wrapper: in the main window, GPUI 0.2.2 does not
                // deliver clicks to children inside an overflow_y_scroll
                // container, and the full-page card has room to grow anyway.
                .child(sections)
                .child(
                    h_flex()
                        .w_full()
                        .h(px(30.0))
                        .px(px(6.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(component_theme.border)
                        .gap(px(6.0))
                        .items_center()
                        .child(
                            Icon::empty()
                                .path("icons/list-filter.svg")
                                .with_size(px(12.0))
                                .text_color(component_theme.muted_foreground)
                                .flex_shrink_0(),
                        )
                        .child(match filter_input.as_ref() {
                            Some(input) => Input::new(input)
                                .small()
                                .appearance(false)
                                .w_full()
                                .text_size(px(12.0))
                                .into_any_element(),
                            None => div().into_any_element(),
                        }),
                )
                .child(
                    div()
                        .id("picker-new-project")
                        .w_full()
                        .h(px(36.0))
                        .px(px(12.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(component_theme.border)
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .cursor_pointer()
                        .text_size(px(12.0))
                        .text_color(component_theme.foreground)
                        .hover(|s| {
                            s.bg(component_theme.foreground.opacity(crate::theme::WASH_HOVER))
                        })
                        .on_click(move |_, window, app| {
                            new_project_herdr.update(app, |this, cx| {
                                this.begin_workspace_creation(window, cx)
                            });
                        })
                        .child("+ New Workspace…"),
                )
                .child(
                    div()
                        .id("picker-dismiss")
                        .w_full()
                        .h(px(30.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.0))
                        .text_color(component_theme.muted_foreground)
                        .cursor_pointer()
                        .hover(|s| {
                            s.bg(component_theme.foreground.opacity(crate::theme::WASH_HOVER))
                        })
                        .on_click(move |_, window, app| {
                            close_herdr.update(app, |this, cx| {
                                this.close_project_picker(window, cx);
                            });
                        })
                        .child(dismiss_text),
                ),
        )
        .into_any_element()
}
/// The workspace settings page: rename (metadata write) and delete (two-step
/// confirm). Full-page, same form as the New-Workspace page.
pub(crate) fn workspace_settings_page_impl(
    this: &mut ShardlaneApp,
    window: &mut Window,
    cx: &mut Context<ShardlaneApp>,
) -> AnyElement {
    let component_theme = cx.theme().clone();
    let Some(session) = this.workspace_settings_session.clone() else {
        return div().into_any_element();
    };
    let display = this.shared.display_name(&session);
    // Lazily create the name input for this session, prefilled.
    if this.workspace_settings_name.is_none() {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Workspace name…"));
        input.update(cx, |state, cx| {
            state.set_value(display.clone(), window, cx);
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut ShardlaneApp, _, event: &gpui_component::input::InputEvent, window, cx| {
                if matches!(event, gpui_component::input::InputEvent::PressEnter { .. }) {
                    this.rename_workspace_from_settings(window, cx);
                }
            },
        );
        this.workspace_settings_name = Some(input);
        this._workspace_settings_name_sub = Some(subscription);
    }
    let name_control = this
        .workspace_settings_name
        .as_ref()
        .map(|input| {
            Input::new(input)
                .small()
                .appearance(false)
                .w_full()
                .text_size(px(13.0))
                .into_any_element()
        })
        .unwrap_or_else(|| div().into_any_element());
    let armed = this.workspace_delete_armed;
    let rename_herdr = cx.entity();
    let delete_herdr = cx.entity();
    let cancel_herdr = cx.entity();

    div()
        .id("shardlane-project-picker")
        .size_full()
        .key_context("ShardlaneApp")
        .on_action(cx.listener(|this, _: &PickerCancel, _, cx| {
            this.close_settings_page(cx);
        }))
        // Early render return skips the shell's action registrations;
        // keep the Window menu alive on full-page surfaces.
        .on_action(cx.listener(ShardlaneApp::new_window))
        .on_action(cx.listener(ShardlaneApp::merge_all_windows))
        .bg(cx.theme().background)
        .text_color(cx.theme().foreground)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(360.0))
                .rounded(px(10.0))
                .border_1()
                .border_color(component_theme.border)
                .p(px(12.0))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .child("Workspace Settings"),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(component_theme.muted_foreground)
                        .child(SharedString::from(format!("herdr session · {session}"))),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(34.0))
                        .px(px(10.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(component_theme.border)
                        .flex()
                        .items_center()
                        .child(name_control),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap(px(8.0))
                        .child(
                            div()
                                .id("workspace-settings-rename")
                                .flex_1()
                                .h(px(32.0))
                                .rounded(px(6.0))
                                .bg(component_theme.primary)
                                .text_color(component_theme.primary_foreground)
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(12.0))
                                .cursor_pointer()
                                .hover(|s| s.bg(component_theme.primary.opacity(0.85)))
                                .on_click(move |_, window, app| {
                                    rename_herdr.update(app, |this, cx| {
                                        this.rename_workspace_from_settings(window, cx)
                                    });
                                })
                                .child("Rename"),
                        )
                        .child(
                            div()
                                .id("workspace-settings-delete")
                                .flex_1()
                                .h(px(32.0))
                                .rounded(px(6.0))
                                .border_1()
                                .border_color(if armed {
                                    component_theme.danger
                                } else {
                                    component_theme.border
                                })
                                .text_color(if armed {
                                    component_theme.danger_foreground
                                } else {
                                    component_theme.danger
                                })
                                .bg(if armed {
                                    component_theme.danger
                                } else {
                                    gpui::transparent_black()
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(12.0))
                                .cursor_pointer()
                                .hover(|s| {
                                    if armed {
                                        s.bg(component_theme.danger.opacity(0.85))
                                    } else {
                                        s.bg(component_theme
                                            .danger
                                            .opacity(crate::theme::WASH_HOVER))
                                    }
                                })
                                .on_click(move |_, window, app| {
                                    delete_herdr.update(app, |this, cx| {
                                        this.delete_workspace_from_settings(window, cx)
                                    });
                                })
                                .child(if armed { "Confirm delete" } else { "Delete" }),
                        ),
                )
                .child(
                    div()
                        .id("workspace-settings-close")
                        .w_full()
                        .h(px(30.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.0))
                        .text_color(component_theme.muted_foreground)
                        .cursor_pointer()
                        .hover(|s| {
                            s.bg(component_theme.foreground.opacity(crate::theme::WASH_HOVER))
                        })
                        .on_click(move |_, _window, app| {
                            cancel_herdr.update(app, |this, cx| this.close_settings_page(cx));
                        })
                        .child("Close"),
                ),
        )
        .into_any_element()
}

/// The device settings/switcher page: management moved out of Settings →
/// Machines, plus switching — clicking a device focuses the workspace
/// switcher panel on that device's workspaces.
pub(crate) fn device_settings_page_impl(
    this: &mut ShardlaneApp,
    window: &mut Window,
    cx: &mut Context<ShardlaneApp>,
) -> AnyElement {
    let component_theme = cx.theme().clone();
    // Lazily create the SSH-target input (InputState needs a live Window).
    if this.machine_ssh_input.is_none() {
        this.machine_ssh_input =
            Some(cx.new(|cx| InputState::new(window, cx).placeholder("user@host or SSH alias…")));
    }
    let ssh_control = this
        .machine_ssh_input
        .as_ref()
        .map(|input| {
            Input::new(input)
                .small()
                .appearance(false)
                .w_full()
                .text_size(px(12.0))
                .into_any_element()
        })
        .unwrap_or_else(|| div().into_any_element());

    let connect_herdr = cx.entity();
    let cancel_herdr = cx.entity();
    let devices = this.config.devices.clone();
    let local_name = crate::remote_display_host_name();
    let local_version =
        shardlane_host::herdr::installed_cli_version().unwrap_or_else(|| "—".to_string());
    let selected = this
        .panel_device
        .clone()
        .unwrap_or_else(|| "local".to_string());

    let mut device_rows = v_flex().gap(px(4.0));
    for device in &devices {
        let device_id = device.id.clone();
        let is_selected = device.id == selected;
        let bridged = device.ssh_target.is_none()
            || this
                .shared
                .ssh_bridges
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .any(|bridge| bridge.device_id == device.id);
        let (title, detail) = match device.ssh_target.clone() {
            Some(target) => (device.name.clone(), target),
            None => (
                format!("{local_name} (local)"),
                format!("herdr {local_version}"),
            ),
        };
        let row_herdr = cx.entity();
        device_rows =
            device_rows.child(
                h_flex()
                    .id(SharedString::from(format!("device-row-{}", device.id)))
                    .w_full()
                    .h(px(44.0))
                    .px(px(12.0))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(if is_selected {
                        component_theme.primary
                    } else {
                        component_theme.border
                    })
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .hover(|s| s.bg(component_theme.foreground.opacity(crate::theme::WASH_HOVER)))
                    .on_click(move |_, _window, app| {
                        row_herdr.update(app, |this, cx| {
                            this.select_panel_device(device_id.clone(), cx);
                        });
                    })
                    .child(
                        h_flex()
                            .gap(px(8.0))
                            .items_center()
                            .min_w_0()
                            .child(div().size(px(7.0)).rounded_full().flex_shrink_0().bg(
                                if bridged {
                                    component_theme.success
                                } else {
                                    component_theme.muted_foreground.opacity(0.45)
                                },
                            ))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(component_theme.foreground)
                                    .min_w_0()
                                    .truncate()
                                    .child(title),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.0))
                            .items_center()
                            .flex_shrink_0()
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(component_theme.muted_foreground)
                                    .child(detail),
                            )
                            .child(if is_selected {
                                Icon::empty()
                                    .path("icons/check.svg")
                                    .with_size(px(12.0))
                                    .text_color(component_theme.success)
                                    .flex_shrink_0()
                                    .into_any_element()
                            } else {
                                div().into_any_element()
                            }),
                    ),
            );
    }

    div()
        .id("shardlane-project-picker")
        .size_full()
        .key_context("ShardlaneApp")
        .on_action(cx.listener(|this, _: &PickerCancel, _, cx| {
            this.close_settings_page(cx);
        }))
        // Early render return skips the shell's action registrations;
        // keep the Window menu alive on full-page surfaces.
        .on_action(cx.listener(ShardlaneApp::new_window))
        .on_action(cx.listener(ShardlaneApp::merge_all_windows))
        .bg(cx.theme().background)
        .text_color(cx.theme().foreground)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .child(
            div()
                .w(px(420.0))
                .rounded(px(10.0))
                .border_1()
                .border_color(component_theme.border)
                .p(px(12.0))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .child("Devices"),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(component_theme.muted_foreground)
                        .child(
                            "Click a device to show its workspaces in the switcher. Connect another machine over SSH.",
                        ),
                )
                .child(device_rows)
                .child(
                    h_flex()
                        .w_full()
                        .h(px(34.0))
                        .px(px(10.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(component_theme.border)
                        .items_center()
                        .child(ssh_control),
                )
                .child(
                    div()
                        .id("device-settings-connect")
                        .w_full()
                        .h(px(32.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(component_theme.border)
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.0))
                        .text_color(component_theme.foreground)
                        .cursor_pointer()
                        .hover(|s| {
                            s.bg(component_theme.foreground.opacity(crate::theme::WASH_HOVER))
                        })
                        .on_click(move |_, window, app| {
                            connect_herdr.update(app, |this, cx| {
                                this.add_ssh_machine(window, cx);
                            });
                        })
                        .child("Connect over SSH"),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(component_theme.muted_foreground)
                        .child(
                            "SSH must be non-interactive: run ssh-copy-id user@host, or use Tailscale SSH. Password login is not supported.",
                        ),
                )
                .child(
                    div()
                        .id("device-settings-close")
                        .w_full()
                        .h(px(30.0))
                        .rounded(px(6.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.0))
                        .text_color(component_theme.muted_foreground)
                        .cursor_pointer()
                        .hover(|s| {
                            s.bg(component_theme.foreground.opacity(crate::theme::WASH_HOVER))
                        })
                        .on_click(move |_, _window, app| {
                            cancel_herdr.update(app, |this, cx| this.close_settings_page(cx));
                        })
                        .child("Close"),
                ),
        )
        .into_any_element()
}
