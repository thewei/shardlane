/**
 * [INPUT]: The runtime state of right_panel::lazygit, the shared TerminalPane/ManagedTerminal
 *          input bridge, and the Right Panel chrome/theme.
 * [OUTPUT]: render_right_panel_lazygit_view — the Lazygit hosted auxiliary terminal and the
 *           native status card, wiring left/middle/right mouse press and release.
 * [POS]: The presentation layer of right_panel; owns no processes, only consumes the visible
 *        LazygitSession snapshot and wires up input.
 */
use super::*;

impl ShardlaneApp {
    pub(super) fn render_right_panel_lazygit_view(
        &mut self,
        theme: ContentSurfaceTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let status = self.lazygit_session.host.status;
        if self.lazygit_session.terminal.is_none() {
            return self.render_lazygit_status_card(theme, status, cx);
        }

        let frame = self.lazygit_session.frame.clone();
        let pane = self.lazygit_session.pane.clone();
        let measure_herdr = cx.entity();
        let input_bridge = self.terminal_input_bridge(
            lazygit::LAZYGIT_TARGET.to_string(),
            lazygit::LAZYGIT_TARGET.to_string(),
            cx,
        );
        let preedit = self.ime_preedit_overlay(lazygit::LAZYGIT_TARGET, &frame, cx);
        let color = frame
            .surface_background
            .or(frame.default_background)
            .unwrap_or(self.theme(window).terminal);
        let target = lazygit::LAZYGIT_TARGET.to_string();

        let terminal_surface = div()
            .id("right-panel-lazygit")
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(rgb(color))
            .cursor_text()
            .key_context("Lazygit")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, _| {
                    window.focus(&this.focus_handle);
                }),
            )
            .on_scroll_wheel(cx.listener(Self::handle_lazygit_scroll_wheel))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(Self::handle_lazygit_mouse_down),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(Self::handle_lazygit_mouse_down),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(Self::handle_lazygit_mouse_down),
            )
            .on_mouse_move(
                cx.listener(move |this, event: &MouseMoveEvent, window, cx| {
                    let geometry = this.lazygit_selection_geometry(window);
                    this.handle_terminal_mouse_move_target(&target, geometry, event, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, event, window, cx| {
                    let geometry = this.lazygit_selection_geometry(window);
                    this.handle_terminal_mouse_up_target(
                        lazygit::LAZYGIT_TARGET,
                        geometry,
                        event,
                        cx,
                    );
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |this, event, window, cx| {
                    let geometry = this.lazygit_selection_geometry(window);
                    this.handle_terminal_mouse_up_target(
                        lazygit::LAZYGIT_TARGET,
                        geometry,
                        event,
                        cx,
                    );
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(move |this, event, window, cx| {
                    let geometry = this.lazygit_selection_geometry(window);
                    this.handle_terminal_mouse_up_target(
                        lazygit::LAZYGIT_TARGET,
                        geometry,
                        event,
                        cx,
                    );
                }),
            )
            .child(
                canvas(
                    move |bounds, _, app| {
                        let width = bounds.size.width.to_f64();
                        let height = bounds.size.height.to_f64();
                        measure_herdr.update(app, |this, cx| {
                            this.sync_lazygit_surface_size(width, height, cx);
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(cached_terminal(pane))
            .child(input_bridge)
            .when_some(preedit, |element, preedit| element.child(preedit))
            .into_any_element();

        div()
            .id("right-panel-lazygit-shell")
            .size_full()
            .flex()
            .flex_col()
            .when(status == lazygit::LazygitHostStatus::RunningOutdated, |element| {
                element.child(
                    div()
                        .flex_none()
                        .h(px(28.0))
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .text_size(crate::theme::FONT_META)
                        .text_color(theme.danger)
                        .bg(theme.danger.opacity(0.08))
                        .child(format!(
                            "Lazygit {} is running without the Shardlane overlay; update to {} for certified integration.",
                            self.lazygit_session.host.version.map(|v| v.to_string()).unwrap_or_else(|| "outdated".to_string()),
                            lazygit::minimum_supported_version()
                        )),
                )
            })
            .child(terminal_surface)
            .into_any_element()
    }

    fn render_lazygit_status_card(
        &self,
        theme: ContentSurfaceTheme,
        status: lazygit::LazygitHostStatus,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let detail = self
            .lazygit_session
            .host
            .detail
            .clone()
            .unwrap_or_else(|| match status {
                lazygit::LazygitHostStatus::Starting => {
                    "Resolving Git root and launching the auxiliary tool…".into()
                }
                lazygit::LazygitHostStatus::Stopped => {
                    "Lazygit is stopped while this surface is hidden.".into()
                }
                lazygit::LazygitHostStatus::Exited => {
                    "Lazygit exited. Restart to open a fresh session.".into()
                }
                _ => "Open Lazygit from a Git Project to review its working tree.".into(),
            });
        let herdr = cx.entity();
        div()
            .id("right-panel-lazygit-status")
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.0))
            .px(px(28.0))
            .text_color(theme.foreground)
            .child(
                Icon::empty()
                    .path("icons/git-branch.svg")
                    .with_size(px(26.0))
                    .text_color(match status {
                        lazygit::LazygitHostStatus::Running
                        | lazygit::LazygitHostStatus::RunningOutdated => theme.success,
                        lazygit::LazygitHostStatus::Missing
                        | lazygit::LazygitHostStatus::Outdated
                        | lazygit::LazygitHostStatus::NotGitRepository
                        | lazygit::LazygitHostStatus::Failed => theme.danger,
                        _ => theme.muted,
                    }),
            )
            .child(
                div()
                    .text_size(crate::theme::FONT_SECTION_TITLE)
                    .font_weight(FontWeight::MEDIUM)
                    .child(status.label()),
            )
            .child(
                div()
                    .max_w(px(420.0))
                    .text_size(crate::theme::FONT_BODY)
                    .text_color(theme.muted)
                    .whitespace_normal()
                    .text_center()
                    .child(detail),
            )
            .when(
                matches!(
                    status,
                    lazygit::LazygitHostStatus::Exited
                        | lazygit::LazygitHostStatus::Failed
                        | lazygit::LazygitHostStatus::NotGitRepository
                        | lazygit::LazygitHostStatus::Missing
                        | lazygit::LazygitHostStatus::Outdated
                ),
                |element| {
                    element.child(
                        Button::new("lazygit-restart")
                            .ghost()
                            .small()
                            .label("Restart")
                            .on_click(move |_, _, app| {
                                herdr.update(app, |this, cx| this.restart_lazygit_session(cx));
                            }),
                    )
                },
            )
            .into_any_element()
    }
}
