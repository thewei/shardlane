//! [INPUT]: Depends on the ShardlaneApp type from the crate root (super) and existing types/imports (use super::*); no independent external dependencies.
//! [OUTPUT]: Exposes ShardlaneApp's terminal scrolling and find: local scroll state machine + ⌘F search/highlight/cycle + client search entries (inherent impl split).
//! [POS]: The `crates/herdr-gui` shell scroll_search responsibility domain, mechanically split out of main.rs; together with sibling shell_* modules it forms ShardlaneApp's method surface.
use super::*;

impl ShardlaneApp {
    pub(super) fn client_search_items(&self) -> Vec<ClientSearchItem> {
        let project_index = build_project_index(&self.state, &self.scripts);
        let mut items = Vec::new();

        for runtime_workspace in &self.state.workspaces {
            let project = project_index.for_runtime_id(&runtime_workspace.workspace_id);
            let title = project
                .map(|project| project.label.clone())
                .unwrap_or_else(|| runtime_workspace.workspace_id.clone());
            let project_path = project.and_then(|project| project.project_path.clone());
            let detail = project_path
                .clone()
                .unwrap_or_else(|| "Project".to_string());
            let ids = project
                .map(|project| project.search_context())
                .unwrap_or_else(|| runtime_workspace.workspace_id.clone());
            items.push(
                ClientSearchItem::new(
                    title,
                    detail,
                    &ids,
                    ComponentIconName::FolderOpen,
                    ClientSearchTarget::Project {
                        workspace_id: runtime_workspace.workspace_id.clone(),
                    },
                )
                .with_history_project_paths(project_path.into_iter().collect()),
            );
        }

        for tab in &self.state.tabs {
            let title = self.tab_title(tab);
            let project = tab
                .workspace_id
                .as_deref()
                .and_then(|workspace_id| project_index.for_runtime_id(workspace_id));
            let project_label = project
                .map(|project| project.label.as_str())
                .unwrap_or("Unknown project");
            let project_path = project
                .and_then(|project| project.project_path.as_deref())
                .unwrap_or_default();
            let detail = if project_path.is_empty() {
                format!("Tab · {project_label}")
            } else {
                format!("Tab · {project_label} · {project_path}")
            };
            let ids = format!(
                "{} {}",
                tab.tab_id,
                project
                    .map(|project| project.search_context())
                    .unwrap_or_default()
            );
            items.push(ClientSearchItem::new(
                title,
                detail,
                &ids,
                ComponentIconName::SquareTerminal,
                ClientSearchTarget::Tab {
                    tab_id: tab.tab_id.clone(),
                },
            ));
        }

        // Search only the authoritative Pane projection already loaded in memory. Hidden-tab
        // panes are not preloaded merely to make Global Search broader.
        for pane in &self.state.panes {
            let title = pane
                .terminal_title
                .as_deref()
                .or(pane.title.as_deref())
                .or(pane.label.as_deref())
                .unwrap_or(pane.pane_id.as_str())
                .to_string();
            let tab_label = pane
                .tab_id
                .as_deref()
                .and_then(|tab_id| self.state.tabs.iter().find(|tab| tab.tab_id == tab_id))
                .map(|tab| self.tab_title(tab))
                .or_else(|| pane.tab_id.clone())
                .unwrap_or_else(|| "Current tab".to_string());
            let project = pane
                .workspace_id
                .as_deref()
                .and_then(|workspace_id| project_index.for_runtime_id(workspace_id));
            let project_label = project
                .map(|project| project.label.clone())
                .or_else(|| pane.workspace_id.clone())
                .unwrap_or_else(|| "Project".to_string());
            let detail = format!("Pane · {tab_label} · {project_label}");
            let ids = [
                Some(pane.pane_id.as_str()),
                pane.terminal_id.as_deref(),
                pane.workspace_id.as_deref(),
                pane.tab_id.as_deref(),
                pane.cwd.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
            items.push(ClientSearchItem::new(
                title,
                detail,
                &ids,
                ComponentIconName::SquareTerminal,
                ClientSearchTarget::Pane {
                    workspace_id: pane.workspace_id.clone(),
                    tab_id: pane.tab_id.clone(),
                    pane_id: pane.pane_id.clone(),
                },
            ));
        }

        for script in &self.scripts.scripts {
            let project = project_index.for_project_path(&script.project_path);
            let project_label = project
                .map(|project| project.label.as_str())
                .unwrap_or(script.project_path.as_str());
            let project_path = project
                .and_then(|project| project.project_path.as_deref())
                .unwrap_or(script.project_path.as_str());
            let project_context = project
                .map(|project| project.search_context())
                .unwrap_or_else(|| project_path.to_string());
            let ports = script
                .runtime
                .ports
                .iter()
                .map(|port| format!(":{port}"))
                .collect::<Vec<_>>()
                .join(", ");
            let detail = if ports.is_empty() {
                format!(
                    "BackgroundJob · {} · {} · {} · {}",
                    script.kind.label(),
                    script.runtime.status.label(),
                    project_label,
                    script.command_summary()
                )
            } else {
                format!(
                    "BackgroundJob · {} · {} · {} · {} · {}",
                    script.kind.label(),
                    script.runtime.status.label(),
                    ports,
                    project_label,
                    script.command_summary()
                )
            };
            items.push(ClientSearchItem::new(
                script.name.clone(),
                detail,
                &format!("{} {} {}", script.id, project_context, project_path),
                ComponentIconName::SquareTerminal,
                ClientSearchTarget::Script {
                    script_id: script.id.clone(),
                },
            ));
        }

        for service in &self.observed_services {
            let project_label = project_index
                .for_runtime_id(&service.workspace_id)
                .map(|project| project.label.as_str())
                .unwrap_or(service.workspace_id.as_str());
            let ports = service
                .ports
                .iter()
                .map(|port| format!(":{port}"))
                .collect::<Vec<_>>()
                .join(", ");
            items.push(ClientSearchItem::new(
                service.pane_name.clone(),
                format!("Service · {ports} · {project_label} · {}", service.command),
                &format!(
                    "{} {} {} {} {} {}",
                    service.workspace_id,
                    service.tab_id,
                    service.pane_id,
                    service.pid,
                    service.pane_name,
                    service.command
                ),
                ComponentIconName::SquareTerminal,
                ClientSearchTarget::DetectedService {
                    workspace_id: service.workspace_id.clone(),
                    tab_id: service.tab_id.clone(),
                    pane_id: service.pane_id.clone(),
                },
            ));
        }

        for agent in &self.state.agents {
            let title = agent
                .title
                .clone()
                .or_else(|| agent.name.clone())
                .or_else(|| agent.display_agent.clone())
                .or_else(|| agent.agent.clone())
                .unwrap_or_else(|| "Agent".to_string());
            let project = agent
                .workspace_id
                .as_deref()
                .and_then(|workspace_id| project_index.for_runtime_id(workspace_id))
                .or_else(|| {
                    agent
                        .foreground_cwd
                        .as_deref()
                        .or(agent.cwd.as_deref())
                        .and_then(|path| project_index.for_project_path(path))
                });
            let project_path = project
                .and_then(|project| project.project_path.as_deref())
                .or(agent.foreground_cwd.as_deref())
                .or(agent.cwd.as_deref())
                .unwrap_or_default();
            let project_label = project
                .map(|project| project.label.as_str())
                .unwrap_or_default();
            let detail = match (project_label.is_empty(), project_path.is_empty()) {
                (true, true) => "Agent".to_string(),
                (false, true) => format!("Agent · {project_label}"),
                (true, false) => format!("Agent · {project_path}"),
                (false, false) => format!("Agent · {project_label} · {project_path}"),
            };
            let mut ids = vec![agent.terminal_id.clone()];
            ids.extend(
                [
                    agent.workspace_id.as_deref(),
                    agent.tab_id.as_deref(),
                    agent.pane_id.as_deref(),
                    agent.agent.as_deref(),
                    agent.display_agent.as_deref(),
                    agent.agent_status.as_deref(),
                ]
                .into_iter()
                .flatten()
                .map(str::to_string),
            );
            if let Some(session) = &agent.agent_session {
                ids.extend([
                    session.agent.clone(),
                    session.kind.clone(),
                    session.source.clone(),
                    session.value.clone(),
                ]);
            }
            if let Some(project) = project {
                ids.push(project.search_context());
            }
            items.push(ClientSearchItem::new(
                title,
                detail,
                &ids.join(" "),
                ComponentIconName::Bot,
                ClientSearchTarget::Agent {
                    workspace_id: agent.workspace_id.clone(),
                    tab_id: agent.tab_id.clone(),
                    pane_id: agent.pane_id.clone(),
                    terminal_id: Some(agent.terminal_id.clone()),
                },
            ));
        }

        items
    }

    pub(crate) fn open_search(
        &mut self,
        _: &OpenSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.search_open {
            self.client_picker = None;
            self.search_open = false;
            self.sync_terminal_application_focus(cx);
            cx.notify();
            return;
        }
        let items = self.client_search_items();
        self.open_client_picker(
            "Search everything…",
            items,
            Some(history::history_db_path()),
            window,
            cx,
        );
    }

    /// Clear the terminal wheel-gesture state (pixel residual). The old local-scrollback
    /// halves (`scroll_active`/`pending_scroll_rows`) died with the Embedded scroll chain
    /// (audit B02); only the wheel residual survives as terminal-local scroll state.
    pub(super) fn reset_terminal_scroll_state(&mut self) {
        self.terminal_scroll_residual_px = 0.0;
    }
}
