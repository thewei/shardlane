//! [INPUT]: The import surface and types of the right_panel module root (`use super::*`).
//! [OUTPUT]: render_right_panel_chooser / render_chooser_card — the tool chooser for the empty panel.
//! [POS]: The chooser responsibility slice of the right_panel directory.
use super::*;

impl ShardlaneApp {
    pub(super) fn render_right_panel_chooser(
        &self,
        theme: ContentSurfaceTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // P1-6: the current default profile is captured before building the UI;
        // it is frozen at surface creation.
        let frozen_profile_id = self.config.browser.default_profile().id.clone();
        let herdr = cx.entity();

        let h_files = herdr.clone();
        let h_lazygit = herdr.clone();
        let h_browser = herdr.clone();

        div()
            .id("right-panel-chooser")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px(px(24.0))
            .child(
                div()
                    .w_full()
                    .max_w(px(380.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_size(crate::theme::FONT_SECTION_TITLE)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.foreground)
                            .child("Open Panel"),
                    )
                    .child(
                        div()
                            .text_size(crate::theme::FONT_BODY)
                            .text_color(theme.muted)
                            .child("Choose a tool to display in the right sidebar:"),
                    )
                    .child(
                        div()
                            .mt(px(8.0))
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .child(self.render_chooser_card(
                                "Files",
                                "icons/folder.svg",
                                "Browse and view project files",
                                theme,
                                move |app| {
                                    h_files.update(app, |this, cx| {
                                        this.open_right_panel_surface(RightPanelSurface::Files, cx);
                                    });
                                },
                            ))
                            .child(self.render_chooser_card(
                                "Lazygit",
                                "icons/git-branch.svg",
                                "Review branches, commits, and working tree changes",
                                theme,
                                move |app| {
                                    h_lazygit.update(app, |this, cx| {
                                        this.open_right_panel_surface(
                                            RightPanelSurface::Lazygit,
                                            cx,
                                        );
                                    });
                                },
                            ))
                            // BROWSER-04: browser.enabled gating (reviving a dead config).
                            .when(self.config.browser.enabled, |row| {
                                row.child(self.render_chooser_card(
                                    "Browser",
                                    "icons/globe.svg",
                                    "Preview local web apps and documentation",
                                    theme,
                                    move |app| {
                                        h_browser.update(app, |this, cx| {
                                            // P1-6: freeze the current default profile
                                            // at creation.
                                            let surface = RightPanelSurface::Browser {
                                                url: crate::right_panel::BROWSER_DEFAULT_URL.into(),
                                                profile_id: frozen_profile_id.clone(),
                                            };
                                            this.open_right_panel_surface(surface, cx);
                                        });
                                    },
                                ))
                            }),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_chooser_card(
        &self,
        title: &'static str,
        icon_path: &'static str,
        description: &'static str,
        theme: ContentSurfaceTheme,
        on_click: impl Fn(&mut App) + 'static,
    ) -> impl IntoElement {
        div()
            .h(px(64.0))
            .p(px(10.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.foreground.opacity(0.02))
            .flex()
            .items_center()
            .gap(px(12.0))
            .cursor_pointer()
            .hover(|e| {
                e.bg(theme.foreground.opacity(0.05))
                    .border_color(theme.foreground.opacity(0.15))
            })
            .child(
                div()
                    .size(px(34.0))
                    .rounded(px(6.0))
                    .bg(theme.foreground.opacity(0.04))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::empty()
                            .path(icon_path)
                            .with_size(px(18.0))
                            .text_color(theme.foreground),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_size(crate::theme::FONT_LIST_TITLE)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.foreground)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(crate::theme::FONT_META)
                            .text_color(theme.muted)
                            .truncate()
                            .child(description),
                    ),
            )
            .on_mouse_down(MouseButton::Left, move |_, _, app| {
                on_click(app);
            })
    }
}
