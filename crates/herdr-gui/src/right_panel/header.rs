//! [INPUT]: The import surface and types of the right_panel module root (`use super::*`).
//! [OUTPUT]: render_right_panel_header — the right panel's tab bar + add/close buttons.
//! [POS]: The header responsibility slice of the right_panel directory.
use super::*;

impl ShardlaneApp {
    pub(super) fn render_right_panel_header(
        &self,
        theme: ContentSurfaceTheme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let herdr = cx.entity();
        // P1-6: the current default profile is captured before building the UI;
        // it is frozen at surface creation.
        let frozen_profile_id = self.config.browser.default_profile().id.clone();
        // notate 2026-08-29 G2: snapshot the profile list before building the menu
        // (Fn closures cannot borrow self).
        let frozen_menu_profiles = self.config.browser.profiles.clone();
        let active_surface = self.right_panel.active_surface;

        let mut tabs = div()
            .id("right-panel-tabs")
            .h_full()
            .min_w_0()
            .flex_1()
            .flex()
            .items_center()
            .gap(px(4.0))
            .overflow_x_scroll();

        for (index, surface) in self.right_panel.surfaces.iter().enumerate() {
            let active = active_surface == Some(index);
            let label = surface.label();
            let icon_path = surface.icon_path();
            let activate_herdr = herdr.clone();
            let close_herdr = herdr.clone();

            tabs = tabs.child(
                div()
                    .id(("right-panel-tab", index))
                    .h(px(28.0))
                    .min_w(px(100.0))
                    .max_w(px(176.0))
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(SPACE_ICON)
                    .cursor_pointer()
                    .when(active, |el| el.bg(theme.foreground.opacity(0.10)))
                    .when(!active, |el| {
                        el.hover(|e| e.bg(theme.foreground.opacity(0.05)))
                    })
                    .child(
                        Icon::empty()
                            .path(icon_path)
                            .with_size(px(13.0))
                            .text_color(if active {
                                theme.foreground
                            } else {
                                theme.muted
                            }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(crate::theme::FONT_BODY)
                            .text_color(if active {
                                theme.foreground
                            } else {
                                theme.muted
                            })
                            .child(label),
                    )
                    .child(
                        div()
                            .id(("close-right-panel-tab", index))
                            .size(px(14.0))
                            .rounded(px(3.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|e| e.bg(theme.foreground.opacity(0.12)))
                            .child(
                                Icon::empty()
                                    .path("icons/x.svg")
                                    .with_size(px(9.0))
                                    .text_color(theme.muted),
                            )
                            .on_mouse_down(MouseButton::Left, move |_, _, app| {
                                close_herdr.update(app, |this, cx| {
                                    this.close_right_panel_surface(index, cx);
                                });
                            }),
                    )
                    .on_mouse_down(MouseButton::Left, move |_, _, app| {
                        activate_herdr.update(app, |this, cx| {
                            this.activate_right_panel_surface(index, cx);
                        });
                    }),
            );
        }

        // BROWSER-04: browser.enabled gating (the bool is copied out of the
        // closure's scope).

        let browser_menu_enabled = self.config.browser.enabled;

        let add_herdr = herdr.clone();
        let toggle_herdr = herdr.clone();

        div()
            .id("right-panel-header")
            .h(px(40.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(SPACE_ICON)
            .px(px(8.0))
            .border_b_1()
            .border_color(theme.border)
            .child(tabs)
            .child(
                Button::new("add-right-panel-tab-btn")
                    .ghost()
                    .xsmall()
                    .icon(Icon::empty().path("icons/plus.svg").with_size(px(12.0)))
                    .dropdown_menu_with_anchor(gpui::Corner::TopRight, move |menu, _, _| {
                        let h1 = add_herdr.clone();
                        let h2 = add_herdr.clone();
                        let h3 = add_herdr.clone();
                        let h4 = add_herdr.clone();
                        menu.item(
                            PopupMenuItem::new("Services")
                                .icon(
                                    Icon::empty()
                                        .path("icons/square-terminal.svg")
                                        .with_size(px(13.0)),
                                )
                                .on_click(move |_, _, app| {
                                    h4.update(app, |this, cx| {
                                        this.open_right_panel_surface(
                                            RightPanelSurface::Services,
                                            cx,
                                        );
                                    });
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Files")
                                .icon(Icon::empty().path("icons/folder.svg").with_size(px(13.0)))
                                .on_click(move |_, _, app| {
                                    h1.update(app, |this, cx| {
                                        this.open_right_panel_surface(RightPanelSurface::Files, cx);
                                    });
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Lazygit")
                                .icon(
                                    Icon::empty()
                                        .path("icons/git-branch.svg")
                                        .with_size(px(13.0)),
                                )
                                .on_click(move |_, _, app| {
                                    h2.update(app, |this, cx| {
                                        this.open_right_panel_surface(
                                            RightPanelSurface::Lazygit,
                                            cx,
                                        );
                                    });
                                }),
                        )
                        // BROWSER-04: browser.enabled gating (the bool is taken
                        // out first; closures cannot borrow self). notate
                        // 2026-08-29 G2: created profiles become truly usable
                        // here — one entry per profile, and the surface creation
                        // freezes that profile's identity.
                        .when(browser_menu_enabled, |menu| {
                            let menu_profiles = frozen_menu_profiles.clone();
                            let default_id = frozen_profile_id.clone();
                            let default_label = if menu_profiles.is_empty() {
                                "Browser"
                            } else {
                                "Browser · Default"
                            };
                            let item = |menu: gpui_component::menu::PopupMenu,
                                        label: String,
                                        profile_id: String,
                                        herdr: crate::Entity<ShardlaneApp>| {
                                menu.item(
                                    PopupMenuItem::new(label)
                                        .icon(
                                            Icon::empty()
                                                .path("icons/globe.svg")
                                                .with_size(px(13.0)),
                                        )
                                        .on_click(move |_, _, app| {
                                            herdr.update(app, |this, cx| {
                                                let surface = RightPanelSurface::Browser {
                                                    url: crate::right_panel::BROWSER_DEFAULT_URL
                                                        .into(),
                                                    profile_id: profile_id.clone(),
                                                };
                                                this.open_right_panel_surface(surface, cx);
                                            });
                                        }),
                                )
                            };
                            let mut menu =
                                item(menu, default_label.to_string(), default_id, h3.clone());
                            for profile in &menu_profiles {
                                let profile_id = profile.id.clone();
                                let label = format!("Browser · {}", profile.name);
                                menu = item(menu, label, profile_id, h3.clone());
                            }
                            menu
                        })
                    }),
            )
            .child(
                div()
                    .id("close-right-panel-btn")
                    .size(px(24.0))
                    .rounded(px(5.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|e| e.bg(theme.foreground.opacity(crate::theme::WASH_HOVER)))
                    .child(
                        Icon::empty()
                            .path("icons/panel-right.svg")
                            .with_size(px(13.0))
                            .text_color(theme.muted),
                    )
                    .on_mouse_down(MouseButton::Left, move |_, _, app| {
                        toggle_herdr.update(app, |this, cx| {
                            this.toggle_right_panel(cx);
                        });
                    }),
            )
    }
}
