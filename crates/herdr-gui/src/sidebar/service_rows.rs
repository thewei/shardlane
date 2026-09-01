//! [INPUT]: Constants, types, and root-level imports from the sidebar module root (`super`); full inheritance via `use super::*`.
//! [OUTPUT]: Provides the free-form service_card renderer, observed-service cards, service
//! script cards (one-shot scripts stay out of the Services section), agent rows, and the
//! Agents section's history backfill rows (chronological fill up to 10 + a fixed "View more history sessions" trailing row).
//! [POS]: Service/agent card rendering layer of `crates/herdr-gui::sidebar`; consumed by shell; mechanically split out of sidebar.rs and sharing the module-root namespace with its sibling submodules.
use super::*;
use crate::ui::menus::{menu_action, menu_action_cx};

fn project_dir_name(agent: &Agent) -> Option<String> {
    agent
        .foreground_cwd
        .as_deref()
        .or(agent.cwd.as_deref())
        .and_then(|cwd| std::path::Path::new(cwd).file_name())
        .and_then(|name| name.to_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
}

pub(super) fn agent_row_label(
    identity: &str,
    has_brand: bool,
    title: Option<&str>,
    project: Option<&str>,
) -> (String, Option<String>) {
    let label = if let Some(t) = title {
        crate::ui_metrics::single_line_label(t)
    } else if has_brand {
        project
            .map(String::from)
            .unwrap_or_else(|| identity.to_string())
    } else {
        match project {
            Some(p) => format!("{identity} · {p}"),
            None => identity.to_string(),
        }
    };
    let meta = if has_brand && title.is_some() {
        project.map(String::from)
    } else {
        None
    };
    (label, meta)
}

#[allow(clippy::too_many_arguments)]
fn service_card(
    id: impl Into<ElementId>,
    project_label: String,
    pane_name: String,
    command: String,
    ports: String,
    status: Option<String>,
    active: bool,
    on_activate: impl Fn(&mut Window, &mut App) + 'static,
    cx: &Context<ShardlaneApp>,
) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id(id)
        .w_full()
        .min_w_0()
        .min_h(px(48.0))
        .px(SPACE_SM)
        .py(px(6.0))
        .rounded(theme.radius)
        .cursor_pointer()
        .when(active, |card| {
            card.bg(theme.foreground.opacity(crate::theme::WASH_ACTIVE))
                .text_color(theme.sidebar_accent_foreground)
        })
        .when(!active, |card| {
            card.text_color(theme.sidebar_foreground)
                .hover(|style| style.bg(theme.foreground.opacity(crate::theme::WASH_HOVER)))
        })
        .shardlane_interactive(
            theme.primary.opacity(INTERACTIVE_FOCUS_OPACITY),
            on_activate,
        )
        .child(
            v_flex()
                .w_full()
                .min_w_0()
                .gap(px(2.0))
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap(SPACE_XS)
                        .child(
                            Icon::new(ComponentIconName::SquareTerminal)
                                .xsmall()
                                .text_color(theme.muted_foreground),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(FONT_CAPTION)
                                .font_weight(FontWeight::MEDIUM)
                                .child(format!("{project_label} · {pane_name}")),
                        )
                        .when(!ports.is_empty(), |row| {
                            row.child(sidebar_meta_pill(
                                ports,
                                theme.success.opacity(0.14),
                                theme.success,
                            ))
                        }),
                )
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap(SPACE_XS)
                        .pl(px(18.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(FONT_LABEL)
                                .text_color(theme.muted_foreground)
                                .child(command),
                        )
                        .when_some(status, |row, status| {
                            row.child(
                                div()
                                    .flex_none()
                                    .text_size(FONT_LABEL)
                                    .text_color(theme.muted_foreground)
                                    .child(status),
                            )
                        }),
                ),
        )
}

impl ShardlaneApp {
    pub(super) fn sidebar_observed_service_card(
        &self,
        service: &ObservedService,
        project_label: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workspace_id = service.workspace_id.clone();
        let tab_id = service.tab_id.clone();
        let pane_id = service.pane_id.clone();
        let active = self.state.focused_pane_id.as_deref() == Some(service.pane_id.as_str());
        let herdr = cx.entity();

        service_card(
            ElementId::Name(format!("shardlane-detected-service-{}", service.pane_id).into()),
            project_label.to_string(),
            service.pane_name.clone(),
            crate::ui_metrics::single_line_label(&service.command),
            service.ports_label(),
            Some(format!("PID {}", service.pid)),
            active,
            move |window, app| {
                herdr.update(app, |this, cx| {
                    // FocusIntent seam: sidebar entries go through apply_focus_intent.
                    this.apply_focus_intent(
                        FocusIntent::pane(
                            Some(workspace_id.clone()),
                            Some(tab_id.clone()),
                            pane_id.clone(),
                        ),
                        window,
                        cx,
                    );
                });
            },
            cx,
        )
        .into_any_element()
    }

    pub(super) fn sidebar_service_script_card(
        &self,
        script: &ScriptRecord,
        project_label: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let script_id = script.id.clone();
        let focus_id = script.id.clone();
        let stop_id = script.id.clone();
        let restart_id = script.id.clone();
        let delete_id = script.id.clone();
        let status = script.runtime.status.label().to_string();
        let active = script
            .pane_id
            .as_deref()
            .zip(self.state.focused_pane_id.as_deref())
            .is_some_and(|(script_pane, focused)| script_pane == focused);
        let ports = match script.runtime.ports.as_slice() {
            [] => String::new(),
            [port] => format!(":{port}"),
            ports => format!(":{} +{}", ports[0], ports.len() - 1),
        };
        let herdr = cx.entity();
        let focus_herdr = herdr.clone();
        let running = matches!(
            script.runtime.status,
            ScriptStatus::Starting | ScriptStatus::Running
        );

        service_card(
            ElementId::Name(format!("shardlane-service-{script_id}").into()),
            project_label.to_string(),
            script.name.clone(),
            crate::ui_metrics::single_line_label(&script.command_summary()),
            ports,
            Some(status),
            active,
            move |window, app| {
                focus_herdr.update(app, |this, cx| {
                    this.focus_script_id(focus_id.clone(), window, cx)
                });
            },
            cx,
        )
        .context_menu(move |menu, _, _| {
            let herdr = herdr.clone();
            let script_id = script_id.clone();

            let menu = if running {
                menu.item({
                    let id = stop_id.clone();
                    menu_action_cx("Stop Service", &herdr, move |this, cx| {
                        this.stop_script_id(id.clone(), cx)
                    })
                })
            } else {
                menu.item({
                    let id = stop_id.clone();
                    menu_action_cx("Start Service", &herdr, move |this, cx| {
                        this.start_script_id(id.clone(), cx)
                    })
                })
            };
            menu.item({
                let id = restart_id.clone();
                menu_action_cx("Restart Service", &herdr, move |this, cx| {
                    this.restart_script_id(id.clone(), cx)
                })
            })
            .item(PopupMenuItem::new("Edit Script…").on_click({
                let herdr = herdr.clone();
                let id = script_id.clone();
                move |_, window, app| {
                    let herdr = herdr.clone();
                    let id = id.clone();
                    window.defer(app, move |window, app| {
                        herdr.update(app, |this, cx| {
                            this.open_edit_script_dialog(id, window, cx);
                        });
                    });
                }
            }))
            .item(PopupMenuItem::separator())
            .item({
                // P12-1: destructive deletes go through a confirmation dialog first
                // (the menu closes on click, leaving no place for two-click arming).
                let herdr = herdr.clone();
                let id = delete_id.clone();
                PopupMenuItem::new("Delete Service").on_click(move |_, window, app| {
                    let herdr = herdr.clone();
                    let id = id.clone();
                    window.defer(app, move |window, app| {
                        herdr.update(app, |this, cx| {
                            this.confirm_delete_script(id, window, cx);
                        });
                    });
                })
            })
        })
        .into_any_element()
    }

    /// History session row in the Sidebar Agents section (notate 2026-08-29):
    /// backfills history sessions chronologically when fewer than 10; clicking
    /// jumps to the matching entry in the History secondary page.
    pub(super) fn sidebar_history_session_row(
        &self,
        session: &ConversationMeta,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let lead = agent_brand_icon(session.agent.as_str(), dark)
            .map(RowLead::Brand)
            .unwrap_or(RowLead::Icon("icons/layers.svg"));
        // notate 08-29 round two: meta shows last-active time (short format);
        // long titles/project names take too much room in a 26px row.
        let meta_text: Option<SharedString> = (session.updated_at > 0).then(|| {
            SharedString::from(crate::history::history_last_active_short(
                session.updated_at,
            ))
        });
        let key = session.key.clone();
        let herdr = cx.entity();
        sidebar_row(
            &self.sidebar_roving_agents,
            format!("shardlane-agent-history-{key}"),
            lead,
            session.title.clone(),
            None,
            meta_text,
            None,
            false,
            None,
            false,
            RowLevel::Primary,
            move |window, app| {
                herdr.update(app, |this, cx| {
                    this.open_history_session_by_key(key.clone(), window, cx);
                });
            },
            cx,
        )
        .into_any_element()
    }

    /// Trailing row of the Sidebar Agents section (notate 2026-08-29): a fixed
    /// "view more history sessions" row that opens the History secondary surface.
    pub(super) fn sidebar_history_more_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let herdr = cx.entity();
        sidebar_row(
            &self.sidebar_roving_agents,
            "shardlane-agent-history-more",
            RowLead::Icon("icons/layers.svg"),
            "View more history sessions",
            None,
            None,
            None,
            false,
            None,
            false,
            RowLevel::Primary,
            move |window, app| {
                herdr.update(app, |this, cx| {
                    this.open_history_surface_from_sidebar(cx);
                    let _ = window;
                });
            },
            cx,
        )
        .into_any_element()
    }

    pub(super) fn sidebar_agent_row(
        &self,
        agent: &Agent,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workspace_id = agent.workspace_id.clone();
        let tab_id = agent.tab_id.clone();
        let pane_id = agent.pane_id.clone();
        let identity = agent_identity(agent).unwrap_or("Agent");
        let has_brand = agent_brand_icon(identity, dark).is_some();
        let project_name = project_dir_name(agent);
        let (label, meta) = agent_row_label(
            identity,
            has_brand,
            agent.title.as_deref(),
            project_name.as_deref(),
        );
        let meta_text: Option<SharedString> = meta.map(SharedString::from);
        let lead = agent_brand_icon(identity, dark)
            .map(RowLead::Brand)
            .unwrap_or(RowLead::Icon("icons/terminal.svg"));
        let id = agent
            .pane_id
            .as_deref()
            .unwrap_or(agent.terminal_id.as_str())
            .to_string();
        let terminal_id_for_focus = agent.terminal_id.clone();
        let herdr = cx.entity();
        let deny_herdr = herdr.clone();
        let deny_pane = agent.pane_id.clone();

        let status = agent
            .agent_status
            .as_deref()
            .or(agent.custom_status.as_deref())
            .map(crate::status::attention_for_raw_status);
        // Agent row highlight follows the client-local selection (focused_pane_id);
        // it does not switch on Herdr runtime focus events.
        let active = agent
            .pane_id
            .as_deref()
            .is_some_and(|pane_id| self.state.focused_pane_id.as_deref() == Some(pane_id));

        sidebar_row(
            &self.sidebar_roving_agents,
            format!("shardlane-agent-{id}"),
            lead,
            label,
            None,
            meta_text,
            None,
            false,
            status,
            active,
            RowLevel::Primary,
            move |window, app| {
                herdr.update(app, |this, cx| {
                    // FocusIntent::Agent (audit P2-6): terminal_id is the stable primary
                    // identity; a missing pane_id no longer degrades into a lost focus;
                    // when `agent.focus` fails the focus chain automatically falls back
                    // to workspace→tab→pane.
                    let intent = FocusIntent::agent(
                        terminal_id_for_focus.clone(),
                        workspace_id.clone(),
                        tab_id.clone(),
                        pane_id.clone(),
                    );
                    this.apply_focus_intent(intent, window, cx);
                });
            },
            cx,
        )
        .context_menu(move |menu, _, _| {
            let Some(deny_pane) = deny_pane.clone() else {
                return menu;
            };
            menu.item(menu_action(
                "Not an Agent",
                &deny_herdr,
                move |this, window, cx| {
                    this.deny_pane_agent_by_id(deny_pane.clone(), window, cx);
                },
            ))
        })
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::agent_row_label;

    #[test]
    fn brand_agent_with_title_shows_title_and_project_meta() {
        let (label, meta) =
            agent_row_label("Claude Code", true, Some("Fix login bug"), Some("my-app"));
        assert_eq!(label, "Fix login bug");
        assert_eq!(meta.as_deref(), Some("my-app"));
    }

    #[test]
    fn brand_agent_without_title_shows_project_as_label() {
        let (label, meta) = agent_row_label("Claude Code", true, None, Some("my-app"));
        assert_eq!(label, "my-app");
        assert!(meta.is_none());
    }

    #[test]
    fn brand_agent_without_title_or_project_shows_identity() {
        let (label, meta) = agent_row_label("Claude Code", true, None, None);
        assert_eq!(label, "Claude Code");
        assert!(meta.is_none());
    }

    #[test]
    fn non_brand_agent_with_project_shows_identity_dot_project() {
        let (label, meta) = agent_row_label("unknown-cli", false, None, Some("my-app"));
        assert_eq!(label, "unknown-cli · my-app");
        assert!(meta.is_none());
    }

    #[test]
    fn non_brand_agent_without_project_shows_identity() {
        let (label, meta) = agent_row_label("unknown-cli", false, None, None);
        assert_eq!(label, "unknown-cli");
        assert!(meta.is_none());
    }

    #[test]
    fn title_always_wins_regardless_of_brand() {
        let (label, _meta) = agent_row_label(
            "unknown-cli",
            false,
            Some("Implementing API"),
            Some("backend"),
        );
        assert_eq!(label, "Implementing API");
    }

    #[test]
    fn brand_agent_title_without_project_has_no_meta() {
        let (label, meta) = agent_row_label("Codex", true, Some("Review PR"), None);
        assert_eq!(label, "Review PR");
        assert!(meta.is_none());
    }
}
