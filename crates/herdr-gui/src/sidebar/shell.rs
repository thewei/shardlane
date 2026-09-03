//! [INPUT]: Constants, types, and root-level imports from the sidebar module root (`super`); full inheritance via `use super::*`.
//! [OUTPUT]: Provides ShardlaneApp::sidebar() — the full assembly of the Sidebar's single navigation surface (Projects/Tabs/Agents/Scripts/History sections, collapsing, drag and drop, projection consumption).
//! [POS]: Main assembly layer of `crates/herdr-gui::sidebar`; consumes the output of rows/tree_rows/pane_rows/service_rows/projection/section_layout; mechanically split out of sidebar.rs and sharing the module-root namespace with its sibling submodules.
use super::*;
use crate::composer_chip::ComposerChip;

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
        // Footer workspace switcher: the shared panel (also wrapped around the
        // header breadcrumbs) — the chip shows the CURRENT WORKSPACE's name
        // with its running dot.
        let dark = theme.bg <= 0x808080;
        // Sidebar render owns one ProjectIndex snapshot. Previously the visible-project
        // projection and the Sidebar itself each rebuilt the same index, paying the
        // path/script projection cost twice on every Sidebar repaint.
        let project_index = build_project_index(&self.state, &self.scripts);
        let visible_projects = visible_sidebar_projects_with_index(&self.state, &project_index);
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

        let blocked_agents = visible_agents
            .iter()
            .filter(|agent| agent.agent_status.as_deref() == Some("blocked"))
            .count();
        let working_agents = visible_agents
            .iter()
            .filter(|agent| agent.agent_status.as_deref() == Some("working"))
            .count();
        // SBX-07: collapsed summaries use the shared glyph (status_glyph_container),
        // same state language as the Activity/bell rows.
        let agent_header_summary = if self.agents_collapsed && blocked_agents > 0 {
            Some(group_header_glyph(
                crepuscularity_gpui::ElementId::Name("agents-collapse-glyph".into()),
                crate::status::AttentionLevel::NeedsAttention,
                blocked_agents,
                component_theme.danger,
                cx,
            ))
        } else if self.agents_collapsed && working_agents > 0 {
            Some(group_header_glyph(
                crepuscularity_gpui::ElementId::Name("agents-collapse-glyph-working".into()),
                crate::status::AttentionLevel::Working,
                working_agents,
                component_theme.primary,
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
        // Two sidebar sections (Services moved to the right panel, 2026-09-03):
        // content sizes naturally, the whole stack
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
                    .pb(SPACE_MD)
                    .border_t_1()
                    .border_color(component_theme.border)
                    .child({
                        // Footer workspace switcher: shared panel (also used by
                        // the header breadcrumbs) — chip = current workspace.
                        let workspace_picker_herdr = herdr.clone();
                        let picker_machines = crate::switcher_panel::build_picker_machines(self);
                        let selected_device = crate::switcher_panel::selected_panel_device(self);
                        let (chip_label, chip_running) = match self.bound_project() {
                            Some(binding) => {
                                let running = self
                                    .shared
                                    .instance_list()
                                    .iter()
                                    .find(|instance| instance.name == binding.project_id)
                                    .map(|instance| instance.running)
                                    .unwrap_or(true);
                                (binding.project_name.clone(), running)
                            }
                            None => (crate::remote_display_host_name(), true),
                        };
                        let chip = ComposerChip::new("shardlane-sidebar-workspace-picker")
                            .icon(
                                div()
                                    .size(px(7.0))
                                    .rounded_full()
                                    .flex_shrink_0()
                                    .bg(if chip_running {
                                        component_theme.success
                                    } else {
                                        component_theme.muted_foreground.opacity(0.45)
                                    })
                                    .into_any_element(),
                            )
                            .label(chip_label)
                            .cursor_pointer();
                        crate::switcher_panel::workspace_switcher_panel(
                            workspace_picker_herdr.clone(),
                            picker_machines,
                            selected_device,
                            gpui::Corner::BottomLeft,
                            "shardlane-sidebar-workspace-popover",
                            "shardlane-ws-picker-filter",
                            chip,
                        )
                    })
                    .child(div().flex_1())
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
