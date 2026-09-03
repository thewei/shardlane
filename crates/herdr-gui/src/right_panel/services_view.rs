//! [INPUT]: The import surface and types of the right_panel module root (`use super::*`);
//! crate::scripts service types (ScriptKind/ScriptRecord/ScriptStatus/ObservedService);
//! shell_navigation's FocusIntent seam.
//! [OUTPUT]: render_right_panel_services — the Services surface listing resident
//! service scripts and observed listening processes, with terminal jump and
//! localhost external links.
//! [POS]: The services_view responsibility slice of the right_panel directory.
use super::*;
use crate::scripts::{ObservedService, ScriptKind, ScriptRecord, ScriptStatus};

impl ShardlaneApp {
    pub(super) fn render_right_panel_services(
        &self,
        theme: ContentSurfaceTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let herdr = cx.entity();
        // Same resident-service semantics the former Sidebar section used:
        // only long-running (Service kind, non-one-shot) scripts are services.
        let scripts: Vec<&ScriptRecord> = self
            .scripts
            .scripts
            .iter()
            .filter(|script| script.kind == ScriptKind::Service && !script.one_shot)
            .collect();
        let observed: Vec<&ObservedService> = self.observed_services.iter().collect();

        // Project grouping follows the bound instance's runtime workspaces;
        // entries whose workspace vanished group under a muted fallback.
        let project_label = |workspace_id: &str| -> String {
            self.state
                .workspaces
                .iter()
                .find(|workspace| workspace.workspace_id == workspace_id)
                .and_then(|workspace| {
                    workspace.label.clone().or_else(|| {
                        workspace.cwd.as_deref().and_then(|cwd| {
                            std::path::Path::new(cwd)
                                .file_name()
                                .and_then(|name| name.to_str())
                                .map(str::to_string)
                        })
                    })
                })
                .unwrap_or_else(|| "Other".to_string())
        };

        let status_color = |status: ScriptStatus| match status {
            ScriptStatus::Running => theme.success,
            ScriptStatus::Starting => theme.primary,
            ScriptStatus::Failed => theme.danger,
            ScriptStatus::Stopped => theme.muted,
        };

        let mut groups: Vec<(String, Vec<AnyElement>)> = Vec::new();
        let push_row =
            |groups: &mut Vec<(String, Vec<AnyElement>)>, label: String, row: AnyElement| {
                if let Some(group) = groups.iter_mut().find(|(key, _)| *key == label) {
                    group.1.push(row);
                } else {
                    groups.push((label, vec![row]));
                }
            };

        for script in &scripts {
            let label = project_label(&script.workspace_id);
            let script_id = script.id.clone();
            let name = script.name.clone();
            let status = script.runtime.status;
            let ports = script.runtime.ports.clone();
            let color = status_color(status);
            let status_text = status.label();
            let row_herdr = herdr.clone();
            let click_script_id = script_id.clone();
            let ports_for_row = ports.clone();
            let active_or_starting =
                matches!(status, ScriptStatus::Running | ScriptStatus::Starting);
            let row = div()
                .id(SharedString::from(format!("rp-service-script-{script_id}")))
                .h(px(52.0))
                .px(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.border)
                .flex()
                .items_center()
                .gap(SPACE_ICON)
                .cursor_pointer()
                .hover(|s| s.bg(theme.foreground.opacity(crate::theme::WASH_HOVER)))
                .on_click(move |_, window, app| {
                    row_herdr.update(app, |this, cx| {
                        // Click = jump to the service's terminal; a stopped
                        // script starts instead (the former Sidebar card's
                        // primary action).
                        if matches!(status, ScriptStatus::Running | ScriptStatus::Starting) {
                            this.focus_script_id(click_script_id.clone(), window, cx);
                        } else {
                            this.start_script_id(click_script_id.clone(), cx);
                        }
                    });
                })
                .child(div().size(px(8.0)).rounded_full().flex_shrink_0().bg(color))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(crate::theme::FONT_BODY)
                                .text_color(theme.foreground)
                                .child(name),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(crate::theme::FONT_META)
                                .text_color(theme.muted)
                                .child(SharedString::from(format!(
                                    "{status_text}{}",
                                    match ports.as_slice() {
                                        [] => String::new(),
                                        [port] => format!(" · :{port}"),
                                        [first, rest @ ..] =>
                                            format!(" · :{first} +{}", rest.len()),
                                    }
                                ))),
                        ),
                )
                .children(
                    ports_for_row
                        .iter()
                        .map(|port| port_link_button(&herdr, script_id.clone(), *port, &theme)),
                )
                .when(active_or_starting, |row| {
                    row.child(script_action_button(
                        &herdr,
                        script_id.clone(),
                        ScriptRowAction::Stop,
                        &theme,
                    ))
                    .child(script_action_button(
                        &herdr,
                        script_id.clone(),
                        ScriptRowAction::Restart,
                        &theme,
                    ))
                })
                .into_any_element();
            push_row(&mut groups, label, row);
        }

        for service in &observed {
            let label = project_label(&service.workspace_id);
            let name = if service.pane_name.is_empty() {
                crate::ui_metrics::single_line_label(&service.command)
            } else {
                service.pane_name.clone()
            };
            let ports = service.ports.clone();
            let row_herdr = herdr.clone();
            let ports_for_row = ports.clone();
            let intent = FocusIntent::from_targets(
                Some(service.workspace_id.clone()),
                Some(service.tab_id.clone()),
                Some(service.pane_id.clone()),
            );
            let row =
                div()
                    .id(SharedString::from(format!(
                        "rp-service-observed-{}-{}",
                        service.workspace_id, service.pane_id
                    )))
                    .h(px(52.0))
                    .px(px(10.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(theme.border)
                    .flex()
                    .items_center()
                    .gap(SPACE_ICON)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.foreground.opacity(crate::theme::WASH_HOVER)))
                    .on_click(move |_, window, app| {
                        if let Some(intent) = intent.clone() {
                            row_herdr.update(app, |this, cx| {
                                this.apply_focus_intent(intent, window, cx);
                            });
                        }
                    })
                    .child(
                        div()
                            .size(px(8.0))
                            .rounded_full()
                            .flex_shrink_0()
                            .bg(theme.success),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(crate::theme::FONT_BODY)
                                    .text_color(theme.foreground)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(crate::theme::FONT_META)
                                    .text_color(theme.muted)
                                    .child(SharedString::from(format!("running{}", {
                                        let label = service.ports_label();
                                        if label.is_empty() {
                                            String::new()
                                        } else {
                                            format!(" · {label}")
                                        }
                                    }))),
                            ),
                    )
                    .children(ports_for_row.iter().map(|port| {
                        port_link_button(&herdr, service.pane_id.clone(), *port, &theme)
                    }))
                    .into_any_element();
            push_row(&mut groups, label, row);
        }

        if groups.is_empty() {
            return crate::ui::empty_state::empty_state("No running services", theme.muted)
                .into_any_element();
        }

        let mut list = div().flex().flex_col().gap(px(6.0)).py(px(4.0));
        for (label, rows) in groups {
            list = list
                .child(
                    div()
                        .px(px(4.0))
                        .pt(px(4.0))
                        .text_size(crate::theme::FONT_META)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted)
                        .child(label),
                )
                .children(rows);
        }

        div()
            .id("right-panel-services-scroll")
            .size_full()
            .overflow_y_scroll()
            .px(px(8.0))
            .child(list)
            .into_any_element()
    }
}

/// One per-port external-link button. Owns every value its click closure
/// needs, so no outer render borrow can be moved into the `Fn` closure.
fn port_link_button(
    herdr: &Entity<ShardlaneApp>,
    key: String,
    port: u16,
    theme: &ContentSurfaceTheme,
) -> AnyElement {
    let url = std::rc::Rc::new(format!("http://localhost:{port}"));
    let url_for_click = url.clone();
    let link_herdr = herdr.clone();
    div()
        .id(SharedString::from(format!("rp-service-link-{key}-{port}")))
        .size(px(22.0))
        .rounded(px(5.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .cursor_pointer()
        .hover(|s| s.bg(theme.foreground.opacity(0.10)))
        .on_click(move |_, _, app| {
            app.stop_propagation();
            let url = (*url_for_click).clone();
            link_herdr.update(app, |this: &mut ShardlaneApp, cx| {
                // localhost dev servers are the sanctioned right-panel Browser
                // scope; a disabled browser falls back to the system handler.
                if this.config.browser.enabled {
                    let profile_id = this.config.browser.default_profile().id.clone();
                    this.open_right_panel_surface(
                        RightPanelSurface::Browser { url, profile_id },
                        cx,
                    );
                } else {
                    let _ = std::process::Command::new("open").arg(&url).spawn();
                }
            });
        })
        .tooltip(crate::ui::tooltip::tooltip_fn(format!("Open {url}")))
        .child(
            Icon::empty()
                .path("icons/globe.svg")
                .with_size(px(12.0))
                .text_color(theme.muted),
        )
        .into_any_element()
}

enum ScriptRowAction {
    Stop,
    Restart,
}

/// Hover-equivalent control for service scripts: stop / restart stay reachable
/// now that the Sidebar card (which carried these actions) is gone.
fn script_action_button(
    herdr: &Entity<ShardlaneApp>,
    script_id: String,
    action: ScriptRowAction,
    theme: &ContentSurfaceTheme,
) -> AnyElement {
    let (icon_path, label) = match action {
        ScriptRowAction::Stop => ("icons/x.svg", "Stop service"),
        ScriptRowAction::Restart => ("icons/rotate-cw.svg", "Restart service"),
    };
    let action_herdr = herdr.clone();
    div()
        .id(SharedString::from(format!(
            "rp-service-action-{script_id}-{label}"
        )))
        .size(px(22.0))
        .rounded(px(5.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .cursor_pointer()
        .hover(|s| s.bg(theme.foreground.opacity(0.10)))
        .on_click(move |_, _, app| {
            app.stop_propagation();
            let script_id = script_id.clone();
            action_herdr.update(app, |this: &mut ShardlaneApp, cx| match action {
                ScriptRowAction::Stop => this.stop_script_id(script_id, cx),
                ScriptRowAction::Restart => this.restart_script_id(script_id, cx),
            });
        })
        .tooltip(crate::ui::tooltip::tooltip_fn(label))
        .child(
            Icon::empty()
                .path(icon_path)
                .with_size(px(11.0))
                .text_color(theme.muted),
        )
        .into_any_element()
}
