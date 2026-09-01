//! [INPUT]: Constants, types, and root-level imports from the sidebar module root (`super`); full inheritance via `use super::*`.
//! [OUTPUT]: Provides ShardlaneApp::sidebar() — the full assembly of the Sidebar's single navigation surface (Projects/Tabs/Agents/Scripts/History sections, collapsing, drag and drop, projection consumption).
//! [POS]: Main assembly layer of `crates/herdr-gui::sidebar`; consumes the output of rows/tree_rows/pane_rows/service_rows/projection/section_layout; mechanically split out of sidebar.rs and sharing the module-root namespace with its sibling submodules.
use super::*;
use crate::composer_chip::ComposerChip;
use gpui_component::input::{Input, InputState};
use gpui_component::popover::{Popover, PopoverState};

/// One workspace-switcher machine row: (display name, is-local, its instances).
/// Each instance row is (jump key, label, running, bound).
type PickerMachine = (String, bool, Vec<(String, String, bool, bool)>);

impl ShardlaneApp {
    pub(crate) fn sidebar(
        &self,
        projection: SidebarProjection<'_>,
        theme: UiTheme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let SidebarProjection {
            expanded_projects,
            panes_by_project,
            project_pane_loads_in_flight,
            project_pane_errors,
        } = projection;
        let component_theme = cx.theme().clone();
        let herdr = cx.entity();
        let search_herdr = herdr.clone();
        let reconnect_herdr = herdr.clone();
        let history_herdr = herdr.clone();
        let new_agent_herdr = herdr.clone();
        let new_agent_header_herdr = herdr.clone();
        let new_project_herdr = herdr.clone();
        let agents_herdr = herdr.clone();
        let settings_herdr = herdr.clone();
        // Multi-instance footer switcher (expected design, 2026-09): the chip
        // shows the MACHINE of the current binding; the panel groups each
        // connected machine's Herdr instances (= workspaces) under a machine
        // header with live running status, plus an inline SSH quick-connect.
        // (key, label, running, bound) per instance; (name, local?, instances).
        let bound_key = self
            .bound_project()
            .map(|binding| binding.project_id.clone());
        let local_machine_name = crate::remote_display_host_name();
        let bridged_devices: HashSet<String> = self
            .shared
            .ssh_bridges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|bridge| bridge.device_id.clone())
            .collect();
        let picker_instance = |key: String,
                               raw: &str,
                               override_label: String,
                               fallback: String,
                               running: bool,
                               bound: bool|
         -> (String, String, bool, bool) {
            let label = if override_label == fallback {
                format!("herdr:{raw}")
            } else {
                override_label
            };
            (key, label, running, bound)
        };
        let mut picker_machines: Vec<PickerMachine> = Vec::new();
        let local_instances = self
            .shared
            .instance_list()
            .iter()
            .map(|instance| {
                let key = if instance.is_default {
                    "default".to_string()
                } else {
                    instance.name.clone()
                };
                let fallback = if instance.is_default {
                    "Default".to_string()
                } else {
                    instance.name.clone()
                };
                let override_label = self.shared.display_name(if instance.is_default {
                    None
                } else {
                    Some(instance.name.as_str())
                });
                let bound = bound_key.as_deref() == Some(key.as_str());
                picker_instance(
                    key,
                    &instance.name,
                    override_label,
                    fallback,
                    instance.running,
                    bound,
                )
            })
            .collect::<Vec<_>>();
        picker_machines.push((local_machine_name.clone(), true, local_instances));
        for device in &self.config.devices {
            let Some(target) = device.ssh_target.clone() else {
                continue;
            };
            // Disconnected machines are managed in Settings → Machines; the
            // switcher only lists live bridges.
            if !bridged_devices.contains(&device.id) {
                continue;
            }
            let instances = self
                .shared
                .remote_sessions_for(&device.id)
                .iter()
                .map(|session| {
                    let key = format!("ssh:{}:{}", device.id, session.name);
                    let override_label = self.shared.display_name(Some(key.as_str()));
                    let bound = bound_key.as_deref() == Some(key.as_str());
                    picker_instance(
                        key.clone(),
                        &session.name,
                        override_label,
                        key,
                        session.running,
                        bound,
                    )
                })
                .collect::<Vec<_>>();
            picker_machines.push((format!("{target} · {}", device.name), false, instances));
        }
        // The chip mirrors the machine of the current binding (local unless a
        // remote instance is bound); its dot reflects that machine's liveness.
        let bound_device_id = bound_key
            .as_deref()
            .and_then(|key| key.strip_prefix("ssh:"))
            .and_then(|rest| rest.split_once(':'))
            .map(|(device_id, _)| device_id.to_string());
        let (chip_machine, chip_connected) = match &bound_device_id {
            Some(device_id) => {
                let device = self
                    .config
                    .devices
                    .iter()
                    .find(|device| &device.id == device_id);
                (
                    device
                        .map(|device| device.name.clone())
                        .unwrap_or_else(|| local_machine_name.clone()),
                    bridged_devices.contains(device_id),
                )
            }
            None => (local_machine_name.clone(), true),
        };
        let workspace_picker_herdr = herdr.clone();
        let dark = theme.bg <= 0x808080;
        // Sidebar render owns one ProjectIndex snapshot. Previously the visible-project
        // projection and the Sidebar itself each rebuilt the same index, paying the
        // path/script projection cost twice on every Sidebar repaint.
        let project_index = build_project_index(&self.state, &self.scripts);
        let visible_projects = visible_sidebar_projects_with_index(&self.state, &project_index);
        let visible_project_runtime_ids: HashSet<String> = visible_projects
            .iter()
            .map(|project| project.runtime_workspace_id.clone())
            .collect();
        // Multi-instance model (2026-09): the Projects section lists ONLY the
        // bound instance's runtime workspaces (herdr's own projects). Other
        // instances (= workspaces) live exclusively in the footer workspace
        // switcher — mixing them here made every click jump windows or rebind
        // and churned the whole list (projects + footer) on each press.
        let mut sidebar_project_entries: Vec<(Workspace, Option<String>)> = Vec::new();
        for workspace in &self.state.workspaces {
            let path = visible_projects
                .iter()
                .find(|visible| visible.runtime_workspace_id == workspace.workspace_id)
                .and_then(|visible| visible.project_path.clone());
            sidebar_project_entries.push((workspace.clone(), path));
        }
        let visible_project_count = sidebar_project_entries.len();

        // Top action area: New Script / Search / History as three full-width
        // action rows (same language as before: 32 tall, px4, rounded7, gap10,
        // 16px icon + 13px label, both text_secondary; hover 6%, active 9%).
        let new_agent_action = sidebar_action_row(
            "shardlane-sidebar-new-agent",
            icon("icons/square-pen.svg"),
            "New Task",
            &component_theme,
        )
        .child(
            div()
                .flex_none()
                .text_size(FONT_LABEL)
                .text_color(component_theme.muted_foreground.opacity(0.75))
                .child(
                    crate::shortcuts::display_chord_for_id("app.new-task", &self.config.shortcuts)
                        .unwrap_or_else(|| "⌘N".to_string()),
                ),
        )
        .shardlane_interactive(
            component_theme.primary.opacity(INTERACTIVE_FOCUS_OPACITY),
            move |window, app| {
                new_agent_herdr.update(app, |this, cx| {
                    this.open_fresh_new_agent_surface(window, cx)
                });
            },
        );

        let search_action = sidebar_action_row(
            "shardlane-sidebar-search",
            icon("icons/search.svg"),
            "Search",
            &component_theme,
        )
        .child(
            div()
                .flex_none()
                .text_size(FONT_LABEL)
                .text_color(component_theme.muted_foreground.opacity(0.75))
                .child(
                    crate::shortcuts::display_chord_for_id("app.search", &self.config.shortcuts)
                        .unwrap_or_else(|| "⌘K".to_string()),
                ),
        )
        .shardlane_interactive(
            component_theme.primary.opacity(INTERACTIVE_FOCUS_OPACITY),
            move |window, app| {
                search_herdr.update(app, |this, cx| this.open_search(&OpenSearch, window, cx));
            },
        );

        let history_tooltip = if self.history.loading {
            "History indexing in progress".to_string()
        } else if let Some(count) = self.history.total_sessions {
            if count == 1 {
                "History · 1 conversation".to_string()
            } else {
                format!("History · {count} conversations")
            }
        } else {
            "History".to_string()
        };
        let history_action = sidebar_action_row(
            "shardlane-sidebar-history",
            icon("icons/layers.svg"),
            "History",
            &component_theme,
        )
        .tooltip(crate::ui::tooltip::tooltip_fn(history_tooltip))
        .shardlane_interactive(
            component_theme.primary.opacity(INTERACTIVE_FOCUS_OPACITY),
            move |window, app| {
                history_herdr.update(app, |this, cx| {
                    this.toggle_history(&OpenHistory, window, cx)
                });
            },
        );

        // List roving focus: the container is this section's only Tab stop;
        // ↑/↓ jump to the first/last row.
        self.sidebar_roving_workspaces.begin_frame();
        let workspace_container_handle = self.sidebar_roving_workspaces.container_handle(cx);
        let workspace_container_down = self.sidebar_roving_workspaces.clone();
        let workspace_container_up = self.sidebar_roving_workspaces.clone();
        let mut workspace_scrolling = v_flex()
            .id("shardlane-workspaces-scroll")
            .track_focus(&workspace_container_handle)
            .focus(|style| style.bg(component_theme.primary.opacity(0.06)))
            .on_key_down(move |event, window, _app| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "down" => {
                        _app.stop_propagation();
                        if let Some(first) = workspace_container_down.first_row_handle() {
                            window.focus(&first);
                        }
                    }
                    "up" => {
                        _app.stop_propagation();
                        if let Some(last) = workspace_container_up.last_row_handle() {
                            window.focus(&last);
                        }
                    }
                    _ => {}
                }
            })
            .w_full()
            .px(SIDEBAR_EDGE)
            .pb(SPACE_XS)
            .gap(SPACE_XS);
        if visible_project_count == 0 {
            let empty_herdr = herdr.clone();
            workspace_scrolling = workspace_scrolling.child(
                div()
                    .id("shardlane-no-projects-hint")
                    .w_full()
                    .flex_shrink_0()
                    .py(px(12.0))
                    .px(SIDEBAR_EDGE)
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(FONT_LABEL)
                            .text_color(component_theme.muted_foreground)
                            .child("No projects in this workspace"),
                    )
                    .child(
                        h_flex()
                            .id("shardlane-empty-add-project")
                            .h(ROW_HEIGHT)
                            .px(px(10.0))
                            .gap(SPACE_ICON)
                            .items_center()
                            .rounded(component_theme.radius)
                            .text_size(FONT_LABEL)
                            .text_color(component_theme.muted_foreground)
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(component_theme.foreground.opacity(crate::theme::WASH_HOVER))
                            })
                            .active(|s| {
                                s.bg(component_theme
                                    .foreground
                                    .opacity(crate::theme::WASH_ACTIVE))
                            })
                            .on_click(move |_, window, app| {
                                empty_herdr.update(app, |this, cx| {
                                    this.new_project(&NewProject, window, cx)
                                });
                            })
                            .child(
                                icon("icons/folder-new.svg")
                                    .with_size(px(14.0))
                                    .text_color(component_theme.muted_foreground),
                            )
                            .child("New Project"),
                    ),
            );
        }

        for (workspace, project_path) in sidebar_project_entries.iter() {
            let expanded = expanded_projects.contains(&workspace.workspace_id)
                && !workspace.workspace_id.starts_with("project:");
            workspace_scrolling = workspace_scrolling.child(self.sidebar_project_row(
                workspace,
                project_path.as_deref(),
                !expanded,
                cx,
            ));
            if expanded && self.native_tabs_enabled() {
                // Native-tab placement: the per-Tab subtree is presented by the content-area
                // Tab strip; the Sidebar keeps the Project row (one presentation owner per
                // the `terminal.tab_bar_placement` setting).
                workspace_scrolling =
                    workspace_scrolling.child(sidebar_hint_row("Tabs live in the tab bar", cx));
            } else if expanded {
                let tabs = self.tabs_for_workspace(&workspace.workspace_id);
                if let Some(error) = project_pane_errors.get(&workspace.workspace_id) {
                    // Audit A11: a failed workspace_panes load must not render as a silently
                    // empty section — show the error and offer the same load as the expand click.
                    let retry_herdr = herdr.clone();
                    let retry_workspace_id = workspace.workspace_id.clone();
                    workspace_scrolling = workspace_scrolling.child(
                        div()
                            .id(ElementId::Name(
                                format!("shardlane-project-pane-retry-{retry_workspace_id}").into(),
                            ))
                            .h(ROW_HEIGHT_SUB)
                            .flex_shrink_0()
                            .pl(LEAD_INSET + SUB_INDENT)
                            .pr(SIDEBAR_EDGE)
                            .flex()
                            .items_center()
                            .gap(SPACE_XS)
                            .text_size(FONT_LABEL)
                            .text_color(component_theme.danger)
                            .cursor_pointer()
                            .hover(|style| {
                                style.bg(component_theme
                                    .foreground
                                    .opacity(crate::theme::WASH_HOVER))
                            })
                            .on_click(move |_, _window, app| {
                                retry_herdr.update(app, |this, cx| {
                                    this.load_sidebar_project_panes(retry_workspace_id.clone(), cx)
                                });
                            })
                            .child(Icon::new(ComponentIconName::TriangleAlert).xsmall())
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .child("Couldn't load panes — click to retry"),
                            )
                            .tooltip(crate::ui::tooltip::tooltip_fn(error.clone())),
                    );
                } else if tabs.is_empty() {
                    workspace_scrolling =
                        workspace_scrolling.child(sidebar_hint_row("No live tabs", cx));
                } else {
                    for (insert_index, tab) in tabs.into_iter().enumerate() {
                        let tab_id = tab.tab_id.clone();
                        let pane_source = if self.state.focused_tab_id.as_deref()
                            == Some(tab_id.as_str())
                            && !self.state.panes.is_empty()
                        {
                            Some(self.state.panes.as_slice())
                        } else {
                            panes_by_project
                                .get(&workspace.workspace_id)
                                .map(Vec::as_slice)
                        };
                        let panes = sidebar_panes_for_tab(
                            pane_source.unwrap_or_default(),
                            &tab_id,
                            self.state.layout_for_tab(&tab_id),
                        );
                        let reported_pane_count =
                            usize::try_from(tab.pane_count.unwrap_or_default())
                                .unwrap_or(usize::MAX);
                        let effective_pane_count = reported_pane_count.max(panes.len());
                        workspace_scrolling = workspace_scrolling.child(self.sidebar_tab_row(
                            tab,
                            workspace.workspace_id.clone(),
                            insert_index,
                            effective_pane_count,
                            dark,
                            cx,
                        ));
                        if effective_pane_count > 1 {
                            for pane in &panes {
                                workspace_scrolling =
                                    workspace_scrolling.child(self.sidebar_pane_row(
                                        pane,
                                        &workspace.workspace_id,
                                        &tab_id,
                                        dark,
                                        cx,
                                    ));
                            }
                            if panes.len() < effective_pane_count {
                                let hint = if project_pane_loads_in_flight
                                    .contains(&workspace.workspace_id)
                                {
                                    "Loading panes…"
                                } else {
                                    "Pane details unavailable"
                                };
                                workspace_scrolling =
                                    workspace_scrolling.child(sidebar_hint_row(hint, cx));
                            }
                        }
                    }
                }
            }
        }

        let visible_agents = self.state.agents.clone();
        // notate 2026-08-29: when the Agents section has fewer than 10 entries,
        // backfill recent history sessions in chronological order up to 10, with
        // a fixed trailing row "view more history sessions" → History.
        let sidebar_history_sessions = self.sidebar_agent_history_sessions(visible_agents.len());
        // notate 2026-08-29: one-shot tasks (e.g. Commit) are not resident
        // Services — the Services section lists only long-running service scripts.
        // With no client-side grouping, every resident service script of the bound
        // instance is visible (per-project cards resolve through the ProjectIndex).
        let visible_scripts = self
            .scripts
            .scripts
            .iter()
            .filter(|script| script.kind == ScriptKind::Service && !script.one_shot)
            .collect::<Vec<_>>();
        let visible_observed_services = self
            .observed_services
            .iter()
            .filter(|service| visible_project_runtime_ids.contains(&service.workspace_id))
            .collect::<Vec<_>>();

        // Agents list roving focus: same interaction contract as Workspaces.
        self.sidebar_roving_agents.begin_frame();
        let agent_container_handle = self.sidebar_roving_agents.container_handle(cx);
        let agent_container_down = self.sidebar_roving_agents.clone();
        let agent_container_up = self.sidebar_roving_agents.clone();
        let mut agent_scrolling = v_flex()
            .id("shardlane-agents-scroll")
            .track_focus(&agent_container_handle)
            .focus(|style| style.bg(component_theme.primary.opacity(0.06)))
            .on_key_down(move |event, window, _app| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "down" => {
                        _app.stop_propagation();
                        if let Some(first) = agent_container_down.first_row_handle() {
                            window.focus(&first);
                        }
                    }
                    "up" => {
                        _app.stop_propagation();
                        if let Some(last) = agent_container_up.last_row_handle() {
                            window.focus(&last);
                        }
                    }
                    _ => {}
                }
            })
            .w_full()
            .px(SIDEBAR_EDGE)
            .pb(px(16.0))
            .gap(SPACE_XS);
        if !self.agents_collapsed {
            if visible_agents.is_empty() && sidebar_history_sessions.is_empty() {
                agent_scrolling = agent_scrolling.child(sidebar_hint_row("No active agents", cx));
            } else {
                for agent in &visible_agents {
                    agent_scrolling =
                        agent_scrolling.child(self.sidebar_agent_row(agent, dark, cx));
                }
                for session in &sidebar_history_sessions {
                    agent_scrolling =
                        agent_scrolling.child(self.sidebar_history_session_row(session, dark, cx));
                }
            }
            agent_scrolling = agent_scrolling.child(self.sidebar_history_more_row(cx));
        }

        let mut services_scrolling = v_flex()
            .id("shardlane-services-scroll")
            .w_full()
            .px(SIDEBAR_EDGE)
            .pb(SPACE_SM)
            .gap(SPACE_XS);
        if !self.services_collapsed {
            if visible_scripts.is_empty() && visible_observed_services.is_empty() {
                services_scrolling =
                    services_scrolling.child(sidebar_hint_row("No running services", cx));
            }
            let scripts_by_project = self.scripts.grouped_by_project();
            for visible in &visible_projects {
                let Some(workspace) = self
                    .state
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == visible.runtime_workspace_id)
                else {
                    continue;
                };
                let project_scripts = project_index
                    .for_runtime_id(&workspace.workspace_id)
                    .and_then(|project| scripts_by_project.get(&project.key))
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .copied()
                    .filter(|script| script.kind == ScriptKind::Service && !script.one_shot)
                    .collect::<Vec<_>>();
                let project_services = visible_observed_services
                    .iter()
                    .copied()
                    .filter(|service| service.workspace_id == workspace.workspace_id)
                    .collect::<Vec<_>>();
                if project_scripts.is_empty() && project_services.is_empty() {
                    continue;
                }
                let project_label = visible.label.clone();
                for script in project_scripts {
                    services_scrolling = services_scrolling
                        .child(self.sidebar_service_script_card(script, &project_label, cx));
                }
                for service in project_services {
                    services_scrolling = services_scrolling
                        .child(self.sidebar_observed_service_card(service, &project_label, cx));
                }
            }
        }

        let summary = OperationalSummary {
            blocked_agents: visible_agents
                .iter()
                .filter(|agent| agent.agent_status.as_deref() == Some("blocked"))
                .count(),
            working_agents: visible_agents
                .iter()
                .filter(|agent| agent.agent_status.as_deref() == Some("working"))
                .count(),
            failed_scripts: visible_scripts
                .iter()
                .filter(|script| script.runtime.status == ScriptStatus::Failed)
                .count(),
            active_scripts: visible_scripts
                .iter()
                .filter(|script| {
                    matches!(
                        script.runtime.status,
                        ScriptStatus::Starting | ScriptStatus::Running
                    )
                })
                .count()
                + visible_observed_services.len(),
        };
        // SBX-07: collapsed summaries use the shared glyph (status_glyph_container),
        // same state language as the Activity/bell rows.
        let agent_header_summary = if self.agents_collapsed && summary.blocked_agents > 0 {
            Some(group_header_glyph(
                crepuscularity_gpui::ElementId::Name("agents-collapse-glyph".into()),
                crate::status::AttentionLevel::NeedsAttention,
                summary.blocked_agents,
                component_theme.danger,
                cx,
            ))
        } else if self.agents_collapsed && summary.working_agents > 0 {
            Some(group_header_glyph(
                crepuscularity_gpui::ElementId::Name("agents-collapse-glyph-working".into()),
                crate::status::AttentionLevel::Working,
                summary.working_agents,
                component_theme.primary,
                cx,
            ))
        } else {
            None
        };
        let service_header_summary = if self.services_collapsed && summary.failed_scripts > 0 {
            Some(group_header_glyph(
                crepuscularity_gpui::ElementId::Name("services-collapse-glyph".into()),
                crate::status::AttentionLevel::NeedsAttention,
                summary.failed_scripts,
                component_theme.danger,
                cx,
            ))
        } else if self.services_collapsed && summary.active_scripts > 0 {
            Some(group_header_glyph(
                crepuscularity_gpui::ElementId::Name("services-collapse-glyph-working".into()),
                crate::status::AttentionLevel::Working,
                summary.active_scripts,
                component_theme.success,
                cx,
            ))
        } else {
            None
        };

        let project_header_herdr = herdr.clone();
        let project_header = group_header(
            "shardlane-projects-header",
            "Projects",
            self.projects_collapsed,
            Some(
                div()
                    .id("shardlane-sidebar-new-project")
                    .flex_none()
                    .size(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .rounded(px(4.0))
                    .hover(|s| s.bg(component_theme.foreground.opacity(crate::theme::WASH_HOVER)))
                    .active(|s| {
                        s.bg(component_theme
                            .foreground
                            .opacity(crate::theme::WASH_ACTIVE))
                    })
                    .on_click(move |_, window, app| {
                        app.stop_propagation();
                        new_project_herdr
                            .update(app, |this, cx| this.new_project(&NewProject, window, cx));
                    })
                    .tooltip(crate::ui::tooltip::tooltip_fn("New Project"))
                    .child(
                        icon("icons/folder-new.svg")
                            .with_size(px(14.0))
                            .text_color(component_theme.muted_foreground),
                    )
                    .into_any_element(),
            ),
            move |_, _, app| {
                project_header_herdr.update(app, |this, cx| this.toggle_projects(cx));
            },
            cx,
        );
        let agents_header = group_header(
            "shardlane-agents-header",
            "Agents",
            self.agents_collapsed,
            Some(
                h_flex()
                    .flex_none()
                    .gap(px(4.0))
                    .when_some(agent_header_summary, |row, summary| row.child(summary))
                    .child(
                        div()
                            .id("shardlane-sidebar-new-agent-shortcut")
                            .flex_none()
                            .size(px(20.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .rounded(px(4.0))
                            .hover(|s| {
                                s.bg(component_theme.foreground.opacity(crate::theme::WASH_HOVER))
                            })
                            .active(|s| {
                                s.bg(component_theme
                                    .foreground
                                    .opacity(crate::theme::WASH_ACTIVE))
                            })
                            .on_click(move |_, window, app| {
                                app.stop_propagation();
                                new_agent_header_herdr.update(app, |this, cx| {
                                    this.open_fresh_new_agent_surface(window, cx)
                                });
                            })
                            .tooltip(crate::ui::tooltip::tooltip_fn("New Task"))
                            .child(
                                icon("icons/sparkle.svg")
                                    .with_size(px(14.0))
                                    .text_color(component_theme.muted_foreground),
                            ),
                    )
                    .into_any_element(),
            ),
            move |_, window, app| {
                agents_herdr.update(app, |this, cx| {
                    this.toggle_agents(&ToggleAgents, window, cx)
                });
            },
            cx,
        );
        let services_header_herdr = herdr.clone();
        let services_header = group_header(
            "shardlane-services-header",
            "Services",
            self.services_collapsed,
            service_header_summary,
            move |_, _, app| {
                services_header_herdr.update(app, |this, cx| this.toggle_services_section(cx));
            },
            cx,
        );
        // Three sidebar sections: content sizes naturally, the whole stack
        // scrolls. No fixed heights and no drag-to-resize anymore.
        let sections = div()
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .child(
                v_flex()
                    .w_full()
                    // Slot 0: Agents (R5 agent-first).
                    .child(
                        v_flex()
                            .w_full()
                            .child(agents_header)
                            .when(!self.agents_collapsed, |section| {
                                section.child(agent_scrolling)
                            }),
                    )
                    // Slot 1: Projects.
                    .child(
                        v_flex()
                            .w_full()
                            .child(project_header)
                            .when(!self.projects_collapsed, |section| {
                                section.child(workspace_scrolling)
                            }),
                    )
                    // Slot 2: Services.
                    .child(
                        v_flex()
                            .w_full()
                            .child(services_header)
                            .when(!self.services_collapsed, |section| {
                                section.child(services_scrolling)
                            }),
                    ),
            )
            .into_any_element();

        v_flex()
            .w_full()
            .h_full()
            .flex_shrink_0()
            .bg(component_theme.sidebar)
            .child(
                v_flex()
                    .w_full()
                    .flex_shrink_0()
                    .px(SIDEBAR_EDGE)
                    .pt(SPACE_MD)
                    .pb(SPACE_SM)
                    .gap(SPACE_XS)
                    .child(new_agent_action)
                    .child(search_action)
                    .child(history_action),
            )
            .child(sections)
            .when(!self.status.is_connected(), |this| {
                this.child(
                    h_flex()
                        .flex_shrink_0()
                        .px(SIDEBAR_EDGE)
                        .py(SPACE_SM)
                        .gap(SPACE_SM)
                        .text_size(FONT_LABEL)
                        .text_color(component_theme.muted_foreground)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(self.status.detail().to_string()),
                        )
                        .child(
                            Button::new("shardlane-sidebar-reconnect")
                                .ghost()
                                .xsmall()
                                .label("Reconnect")
                                .on_click(move |_, window, app| {
                                    reconnect_herdr
                                        .update(app, |this, cx| this.refresh(&Refresh, window, cx));
                                }),
                        ),
                )
            })
            .child({
                let footer_herdr = herdr.clone();
                h_flex()
                    .w_full()
                    .h(ROW_HEIGHT)
                    .flex_shrink_0()
                    .mt(SPACE_MD)
                    .px(SIDEBAR_EDGE)
                    .pt(SPACE_MD)
                    .pb(SPACE_SM)
                    .border_t_1()
                    .border_color(component_theme.border)
                    .child(div().flex_1())
                    .child({
                        // The expected workspace switcher: a machine chip that
                        // opens a panel of machines → their Herdr instances
                        // (= workspaces) with running status, New Workspace,
                        // and an inline SSH quick-connect row (mock 2026-09).
                        let picker_herdr = workspace_picker_herdr.clone();
                        let picker_machines = picker_machines.clone();
                        let picker_theme = component_theme.clone();
                        let mut chip = ComposerChip::new("shardlane-sidebar-workspace-picker")
                            .icon(
                                div()
                                    .size(px(7.0))
                                    .rounded_full()
                                    .flex_shrink_0()
                                    .bg(if chip_connected {
                                        component_theme.success
                                    } else {
                                        component_theme.muted_foreground.opacity(0.45)
                                    })
                                    .into_any_element(),
                            )
                            .label(chip_machine)
                            .cursor_pointer();
                        let chip_style = chip.style().clone();
                        Popover::new("shardlane-sidebar-workspace-popover")
                            .anchor(gpui::Corner::TopLeft)
                            .trigger(chip)
                            .trigger_style(chip_style)
                            .content(
                                move |_popover_state: &mut PopoverState,
                                      window: &mut Window,
                                      cx: &mut Context<PopoverState>| {
                                    let popover = cx.entity();
                                    let picker_herdr = picker_herdr.clone();
                                    // Quick-connect input lives in the popover's
                                    // keyed state (entities must not be created
                                    // during ShardlaneApp's own render pass).
                                    let ssh_input_holder = window.use_keyed_state(
                                        "shardlane-footer-ssh-input",
                                        cx,
                                        |window, cx| {
                                            cx.new(|cx| {
                                                InputState::new(window, cx).placeholder(
                                                    crate::i18n::t(
                                                        "workspace.ssh_placeholder",
                                                    ),
                                                )
                                            })
                                        },
                                    );
                                    let ssh_input = ssh_input_holder.read(cx).clone();
                                    let mut sections = v_flex().w_full().gap(px(2.0));
                                    for (machine_name, is_local, instances) in &picker_machines {
                                        sections = sections
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .px(px(10.0))
                                                    .pt(px(6.0))
                                                    .pb(px(2.0))
                                                    .gap(px(6.0))
                                                    .items_center()
                                                    .child(
                                                        div()
                                                            .size(px(7.0))
                                                            .rounded_full()
                                                            .flex_shrink_0()
                                                            .bg(picker_theme.success),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(13.0))
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(picker_theme.foreground)
                                                            .min_w_0()
                                                            .truncate()
                                                            .child(SharedString::from(
                                                                if *is_local {
                                                                    format!(
                                                                        "{} {}",
                                                                        machine_name,
                                                                        crate::i18n::t(
                                                                            "workspace.local"
                                                                        )
                                                                    )
                                                                } else {
                                                                    machine_name.clone()
                                                                },
                                                            )),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .w_full()
                                                    .px(px(10.0))
                                                    .text_size(px(11.0))
                                                    .text_color(picker_theme.muted_foreground)
                                                    .child(crate::i18n::t(
                                                        "workspace.multiplexers",
                                                    )),
                                            );
                                        for (key, label, running, bound) in instances {
                                            let row_herdr = picker_herdr.clone();
                                            let row_popover = popover.clone();
                                            let key = key.clone();
                                            sections = sections.child(
                                                h_flex()
                                                    .id(SharedString::from(format!(
                                                        "ws-picker-{key}"
                                                    )))
                                                    .w_full()
                                                    .h(px(30.0))
                                                    .px(px(10.0))
                                                    .rounded(px(6.0))
                                                    .gap(px(8.0))
                                                    .items_center()
                                                    .cursor_pointer()
                                                    .hover(|s| {
                                                        s.bg(picker_theme.foreground.opacity(
                                                            crate::theme::WASH_HOVER,
                                                        ))
                                                    })
                                                    .on_click(move |_, window, app| {
                                                        row_popover.update(app, |state, cx| {
                                                            state.dismiss(window, cx)
                                                        });
                                                        row_herdr.update(app, |this, cx| {
                                                            this.open_or_jump_project(
                                                                &key, window, cx,
                                                            )
                                                        });
                                                    })
                                                    .child(
                                                        div()
                                                            .size(px(7.0))
                                                            .rounded_full()
                                                            .flex_shrink_0()
                                                            .bg(if *running {
                                                                picker_theme.success
                                                            } else {
                                                                picker_theme
                                                                    .muted_foreground
                                                                    .opacity(0.45)
                                                            }),
                                                    )
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .truncate()
                                                            .text_size(px(13.0))
                                                            .text_color(if *bound {
                                                                picker_theme.foreground
                                                            } else {
                                                                picker_theme.muted_foreground
                                                            })
                                                            .child(label.clone()),
                                                    )
                                                    .child(if *bound {
                                                        Icon::empty()
                                                            .path("icons/check.svg")
                                                            .with_size(px(12.0))
                                                            .text_color(picker_theme.success)
                                                            .flex_shrink_0()
                                                            .into_any_element()
                                                    } else if !*running {
                                                        div()
                                                            .flex_shrink_0()
                                                            .text_size(px(11.0))
                                                            .text_color(
                                                                picker_theme.muted_foreground,
                                                            )
                                                            .child(crate::i18n::t(
                                                                "workspace.stopped",
                                                            ))
                                                            .into_any_element()
                                                    } else {
                                                        div().into_any_element()
                                                    }),
                                            );
                                        }
                                    }
                                    let new_ws_herdr = picker_herdr.clone();
                                    let new_ws_popover = popover.clone();
                                    let connect_herdr = picker_herdr.clone();
                                    let connect_input = ssh_input.clone();
                                    v_flex()
                                        .w(px(320.0))
                                        .py(px(4.0))
                                        .child(
                                            div()
                                                .id("shardlane-ws-picker-scroll")
                                                .w_full()
                                                .max_h(px(420.0))
                                                .overflow_y_scroll()
                                                .child(sections),
                                        )
                                        .child(
                                            h_flex()
                                                .id("shardlane-ws-picker-new-workspace")
                                                .w_full()
                                                .h(px(30.0))
                                                .px(px(10.0))
                                                .rounded(px(6.0))
                                                .gap(px(8.0))
                                                .items_center()
                                                .cursor_pointer()
                                                .hover(|s| {
                                                    s.bg(picker_theme.foreground.opacity(
                                                        crate::theme::WASH_HOVER,
                                                    ))
                                                })
                                                .on_click(move |_, window, app| {
                                                    new_ws_popover.update(app, |state, cx| {
                                                        state.dismiss(window, cx)
                                                    });
                                                    new_ws_herdr.update(app, |this, cx| {
                                                        this.run_new_project_flow(window, cx)
                                                    });
                                                })
                                                .child(
                                                    Icon::empty()
                                                        .path("icons/plus.svg")
                                                        .with_size(px(12.0))
                                                        .text_color(
                                                            picker_theme.muted_foreground,
                                                        )
                                                        .flex_shrink_0(),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(12.0))
                                                        .text_color(
                                                            picker_theme.muted_foreground,
                                                        )
                                                        .child(crate::i18n::t(
                                                            "workspace.new",
                                                        )),
                                                ),
                                        )
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .h(px(30.0))
                                                .mt(px(4.0))
                                                .mx(px(6.0))
                                                .px(px(6.0))
                                                .rounded(px(6.0))
                                                .border_1()
                                                .border_color(picker_theme.border)
                                                .gap(px(4.0))
                                                .items_center()
                                                .child(
                                                    Input::new(&connect_input)
                                                        .small()
                                                        .appearance(false)
                                                        .w_full()
                                                        .text_size(px(12.0)),
                                                )
                                                .child(
                                                    div()
                                                        .id("shardlane-ws-picker-ssh-connect")
                                                        .size(px(20.0))
                                                        .flex_shrink_0()
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .rounded(px(4.0))
                                                        .cursor_pointer()
                                                        .hover(|s| {
                                                            s.bg(picker_theme.foreground
                                                                .opacity(
                                                                    crate::theme::WASH_HOVER,
                                                                ))
                                                        })
                                                        .on_click(move |_, window, app| {
                                                            let target = connect_input
                                                                .read(app)
                                                                .value()
                                                                .trim()
                                                                .to_string();
                                                            let input = connect_input.clone();
                                                            connect_herdr.update(
                                                                app,
                                                                |this, cx| {
                                                                    this.start_ssh_machine_connect(
                                                                        target,
                                                                        Some(input),
                                                                        window,
                                                                        cx,
                                                                    )
                                                                },
                                                            );
                                                        })
                                                        .child(
                                                            Icon::empty()
                                                                .path("icons/arrow-right.svg")
                                                                .with_size(px(12.0))
                                                                .text_color(
                                                                    picker_theme
                                                                        .muted_foreground,
                                                                ),
                                                        ),
                                                ),
                                        )
                                },
                            )
                    })
                    .child(
                        div()
                            .id("shardlane-sidebar-mobile-icon")
                            .size(ROW_HEIGHT)
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(component_theme.radius)
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(component_theme
                                    .sidebar_accent
                                    .opacity(INTERACTIVE_HOVER_OPACITY))
                            })
                            .active(|s| s.bg(component_theme.sidebar_accent))
                            .tooltip(crate::ui::tooltip::tooltip_fn("Mobile"))
                            .shardlane_interactive(
                                component_theme.primary.opacity(INTERACTIVE_FOCUS_OPACITY),
                                move |window, app| {
                                    footer_herdr.update(app, |this, cx| {
                                        this.open_mobile_surface(cx);
                                        let _ = window;
                                    });
                                },
                            )
                            .child(
                                Icon::empty()
                                    .path("icons/smartphone.svg")
                                    .with_size(px(15.0))
                                    .text_color(component_theme.muted_foreground),
                            ),
                    )
                    .child(
                        div()
                            .id("shardlane-sidebar-settings-icon")
                            .size(ROW_HEIGHT)
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(component_theme.radius)
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(component_theme
                                    .sidebar_accent
                                    .opacity(INTERACTIVE_HOVER_OPACITY))
                            })
                            .active(|s| s.bg(component_theme.sidebar_accent))
                            .tooltip(crate::ui::tooltip::tooltip_fn("Settings"))
                            .shardlane_interactive(
                                component_theme.primary.opacity(INTERACTIVE_FOCUS_OPACITY),
                                move |window, app| {
                                    settings_herdr.update(app, |this, cx| {
                                        this.toggle_settings(&ToggleSettings, window, cx)
                                    });
                                },
                            )
                            .child(
                                Icon::new(ComponentIconName::Settings)
                                    .with_size(px(15.0))
                                    .text_color(component_theme.muted_foreground),
                            ),
                    )
            })
    }
}
