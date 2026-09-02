//! [INPUT]: The main-crate namespace and sibling-module public surface forwarded by the new_agent module root (super).
//! [OUTPUT]: Provides New Agent's surface lifecycle: open/reset/ensure, project and branch sync, attachments,
//! submission (agent and terminal-command, including slash-command dialect encoding), background reference-catalog
//! rebuild (schedule_new_agent_reference_scan), inline delete arming cleanup (clear_script_delete_arming),
//! and empty-state rendering.
//! [POS]: The `crates/herdr-gui` new_agent submodule (mechanically split out of new_agent.rs), cooperating isomorphically with sibling submodules, exported via the root re-export.
use super::page::agent_prompt_placeholder;
use super::reference::expand_command_submission;
use super::reference_index::{build_file_index, scan_agent_commands};
use super::reference_provider::{ComposerReferenceProvider, ComposerReferenceSource};
use super::*;

impl ShardlaneApp {
    pub(crate) fn open_new_agent_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_settings = false;
        self.leave_history_surface(cx);
        self.new_agent_open = true;
        let had_ui = self.new_agent_ui.is_some();
        self.ensure_new_agent_ui(window, cx);
        if had_ui {
            if let Some(workspace_id) = self.new_agent_context_runtime_id().map(str::to_string) {
                self.select_new_agent_project(workspace_id, window, cx);
            }
        }
        self.clear_ime_state();
        self.sync_terminal_application_focus(cx);
        if self.right_panel.open {
            self.sync_right_panel_for_project_context(cx);
            self.refresh_right_panel_state(cx);
        }
        cx.notify();
    }

    pub(crate) fn open_fresh_new_agent_surface(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_agent_ui = None;
        self.open_new_agent_surface(window, cx);
    }

    /// P12-1 hardening: clear stale inline script-delete arming. When deleting
    /// via the menu confirmation path, the inline self-clearing in page.rs is
    /// bypassed and the armed id lingers (no wrong-deletion risk — the id is
    /// bound and never reused, just stale state); clear it uniformly here.
    /// Called from the scripts domain (the field itself is pub(super) and
    /// cannot be written cross-module directly).
    pub(crate) fn clear_script_delete_arming(&mut self) {
        if let Some(ui) = self.new_agent_ui.as_mut() {
            ui.script_delete_armed_id = None;
        }
    }

    fn new_agent_context_runtime_id(&self) -> Option<&str> {
        self.new_agent_context_workspace_id
            .as_deref()
            .or(self.state.focused_workspace_id.as_deref())
    }

    fn new_agent_project_choices(&self) -> Vec<NewAgentProjectChoice> {
        composer_project_choices_from_visible(&self.visible_sidebar_projects())
    }

    pub(super) fn composer_visible_projects(&self) -> Vec<VisibleSidebarProject> {
        self.visible_sidebar_projects()
    }

    pub(super) fn ensure_new_agent_ui(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.new_agent_ui.is_some() {
            self.sync_new_agent_project_list(window, cx);
            return;
        }

        let projects = self.new_agent_project_choices();
        let context_runtime_id = self.new_agent_context_runtime_id();
        let selected_project_index = focused_new_agent_project_index(&projects, context_runtime_id);
        let prompt = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(agent_prompt_placeholder(AgentId::ALL[0]))
                // auto_grow mode (composer semantics from before): rows grow with
                // content and scroll internally once min/max rows are capped;
                // multi_line (PlainText mode) keeps a constant rows height that
                // never grows — the root cause of the pre-050 stuck height.
                .auto_grow(3, 17)
                .soft_wrap(true)
        });
        let terminal_input = cx.new(|cx| InputState::new(window, cx).placeholder("Enter command…"));
        let project = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(projects.clone()),
                selected_project_index.map(|index| IndexPath::default().row(index)),
                window,
                cx,
            )
            .searchable(true)
        });
        let branch = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<NewAgentBranchChoice>::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });

        let project_subscription = cx.subscribe_in(
            &project,
            window,
            |this, _, event: &SelectEvent<SearchableVec<NewAgentProjectChoice>>, window, cx| {
                let SelectEvent::Confirm(Some(workspace_id)) = event else {
                    return;
                };
                this.load_new_agent_branches(workspace_id.clone(), window, cx);
                cx.notify();
            },
        );

        let prompt_subscription = cx.subscribe_in(
            &prompt,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { secondary: false } = event {
                    let Some(ui) = this.new_agent_ui.as_ref() else {
                        return;
                    };
                    if ui.submitting || ui.prompt.read(cx).value().trim().is_empty() {
                        return;
                    }
                    this.submit_new_agent(window, cx);
                }
            },
        );

        let terminal_input_subscription = cx.subscribe_in(
            &terminal_input,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { secondary: false } = event {
                    let Some(ui) = this.new_agent_ui.as_ref() else {
                        return;
                    };
                    if ui.submitting || ui.terminal_input.read(cx).value().trim().is_empty() {
                        return;
                    }
                    this.submit_new_terminal_command(window, cx);
                }
            },
        );

        let initial_workspace_id = selected_project_index
            .and_then(|index| projects.get(index))
            .map(|project| project.runtime_workspace_id.clone());
        // Precheck each agent's CLI availability in the background (resolving one
        // by one through the login shell): results only annotate the menu,
        // never block the composer; the task handle is stored in state and
        // cancelled when the composer closes.
        let agent_scan = cx.spawn(async move |this, cx| {
            let available = cx
                .background_executor()
                .spawn(async move {
                    // R4: only probe product-visible providers (Hidden neither
                    // enters the menu nor gets scanned).
                    shardlane_history::exposed_agents()
                        .into_iter()
                        .filter(|agent| resolve_agent_launch(*agent).is_ok())
                        .collect::<HashSet<_>>()
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if let Some(ui) = view.new_agent_ui.as_mut() {
                    ui.agent_availability = Some(available);
                    cx.notify();
                }
            });
        });
        self.new_agent_ui = Some(NewAgentUiState {
            tab: NewTabKind::default(),
            prompt: prompt.clone(),
            terminal_input: terminal_input.clone(),
            project,
            branch,
            mode: NewAgentMode::default(),
            permission: NewAgentPermission::default(),
            agent: AgentId::ALL[0],
            agent_availability: None,
            attachments: Vec::new(),
            file_index: None,
            command_catalog: None,
            _reference_scan: None,
            branch_project_id: None,
            branch_loading: false,
            branch_choices: Vec::new(),
            submitting: false,
            script_delete_armed_id: None,
            _agent_scan: Some(agent_scan),
            _subscriptions: vec![
                project_subscription,
                prompt_subscription,
                terminal_input_subscription,
            ],
        });
        prompt.update(cx, |state, input_cx| state.focus(window, input_cx));
        // Mount reference completion: @ file / slash-command catalog → the
        // component's native CompletionMenu.
        let app_handle = cx.entity().downgrade();
        prompt.update(cx, |state, _input_cx| {
            state.lsp.completion_provider = Some(ComposerReferenceProvider::new(
                app_handle,
                ComposerReferenceSource::NewAgent,
            ));
        });
        self.schedule_new_agent_reference_scan(cx);
        if let Some(workspace_id) = initial_workspace_id {
            self.load_new_agent_branches(workspace_id, window, cx);
        }
    }

    /// Rebuild the reference catalog in the background (the selected project's
    /// file index + the current agent's command catalog).
    /// Skips when the snapshot is still fresh and its ownership matches; the
    /// task handle is stored in ui for cancellation when the composer closes.
    pub(super) fn schedule_new_agent_reference_scan(&mut self, cx: &mut Context<Self>) {
        let Some(ui) = self.new_agent_ui.as_ref() else {
            return;
        };
        let agent = ui.agent;
        let Some(workspace_id) = ui.project.read(cx).selected_value().cloned() else {
            return;
        };
        let Some(project_path) = sidebar_project_path_for_context(
            &self.visible_sidebar_projects(),
            Some(workspace_id.as_str()),
            None,
        )
        .filter(|path| !path.trim().is_empty()) else {
            return;
        };
        let index_fresh = ui
            .file_index
            .as_ref()
            .is_some_and(|index| index.project_path == project_path);
        let catalog_fresh = ui
            .command_catalog
            .as_ref()
            .is_some_and(|catalog| catalog.agent == agent);
        if index_fresh && catalog_fresh {
            return;
        }
        let scan = cx.spawn(async move |this, cx| {
            let index_root = project_path.clone();
            let catalog_root = project_path.clone();
            let index = cx
                .background_executor()
                .spawn(async move { build_file_index(std::path::Path::new(&index_root)) })
                .await;
            let catalog = cx
                .background_executor()
                .spawn(
                    async move { scan_agent_commands(agent, std::path::Path::new(&catalog_root)) },
                )
                .await;
            let _ = this.update(cx, |view, cx| {
                let Some(ui) = view.new_agent_ui.as_mut() else {
                    return;
                };
                ui.file_index = Some(std::sync::Arc::new(index));
                ui.command_catalog = Some(std::sync::Arc::new(catalog));
                cx.notify();
            });
        });
        if let Some(ui) = self.new_agent_ui.as_mut() {
            ui._reference_scan = Some(scan);
        }
    }

    /// Keep the Project list and `new_agent_context_workspace_id` aligned every
    /// frame while the composer is open.
    fn sync_new_agent_project_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ui) = self.new_agent_ui.as_ref() else {
            return;
        };
        let projects = self.new_agent_project_choices();
        let workspace_id = self.new_agent_context_runtime_id().and_then(|focused| {
            projects
                .iter()
                .find(|project| project.runtime_workspace_id == focused)
                .map(|project| project.runtime_workspace_id.clone())
        });
        let project_state = ui.project.clone();
        let index = workspace_id.as_deref().and_then(|workspace_id| {
            projects
                .iter()
                .position(|project| project.runtime_workspace_id == workspace_id)
        });
        project_state.update(cx, |state, select_cx| {
            state.set_items(SearchableVec::new(projects), window, select_cx);
            if let Some(index) = index {
                state.set_selected_index(Some(IndexPath::default().row(index)), window, select_cx);
            }
        });
        if let Some(workspace_id) = workspace_id {
            self.load_new_agent_branches(workspace_id, window, cx);
        }
    }

    pub(crate) fn select_new_agent_project(
        &mut self,
        workspace_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_agent_context_workspace_id = Some(workspace_id.clone());
        self.ensure_new_agent_ui(window, cx);
        let projects = self.new_agent_project_choices();
        let Some(ui) = self.new_agent_ui.as_ref() else {
            return;
        };
        let project_state = ui.project.clone();
        let index = projects
            .iter()
            .position(|project| project.runtime_workspace_id == workspace_id);
        if index.is_none() {
            lag_log(format_args!(
                "new_agent: project {workspace_id} missing from composer choices (count={})",
                projects.len()
            ));
            return;
        }
        project_state.update(cx, |state, select_cx| {
            state.set_items(SearchableVec::new(projects), window, select_cx);
            if let Some(index) = index {
                state.set_selected_index(Some(IndexPath::default().row(index)), window, select_cx);
            }
        });
        self.load_new_agent_branches(workspace_id, window, cx);
        cx.notify();
    }

    fn load_new_agent_branches(
        &mut self,
        workspace_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project_path) = sidebar_project_path_for_context(
            &self.visible_sidebar_projects(),
            Some(workspace_id.as_str()),
            None,
        ) else {
            return;
        };
        let Some(ui) = self.new_agent_ui.as_mut() else {
            return;
        };
        if ui.branch_project_id.as_deref() == Some(workspace_id.as_str()) {
            return;
        }
        ui.branch_loading = true;
        ui.branch_project_id = Some(workspace_id.clone());
        let branch_state = ui.branch.clone();
        // Project switch → rebuild the reference catalog for the new project.
        self.schedule_new_agent_reference_scan(cx);
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let snapshot = cx
                .background_executor()
                .spawn(async move { git_branch_snapshot(&project_path) })
                .await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |view, cx| {
                    let Some(ui) = view.new_agent_ui.as_mut() else {
                        return;
                    };
                    if ui.branch_project_id.as_deref() != Some(workspace_id.as_str()) {
                        return;
                    }
                    ui.branch_loading = false;
                    let snapshot = snapshot.unwrap_or_else(|_| BranchSnapshot {
                        choices: vec![NewAgentBranchChoice {
                            name: String::new(),
                            label: "Current working tree".to_string(),
                        }],
                        selected_index: 0,
                    });
                    ui.branch_choices = snapshot.choices.clone();
                    branch_state.update(cx, |state, select_cx| {
                        state.set_items(SearchableVec::new(snapshot.choices), window, select_cx);
                        state.set_selected_index(
                            Some(IndexPath::default().row(snapshot.selected_index)),
                            window,
                            select_cx,
                        );
                    });
                    cx.notify();
                });
            });
        })
        .detach();
    }

    pub(crate) fn remove_new_agent_attachment(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(ui) = self.new_agent_ui.as_mut() else {
            return;
        };
        if index < ui.attachments.len() {
            ui.attachments.remove(index);
            cx.notify();
        }
    }

    pub(crate) fn submit_new_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ensure_new_agent_ui(window, cx);
        let Some(ui) = self.new_agent_ui.as_ref() else {
            return;
        };
        if ui.submitting {
            return;
        }
        let prompt = ui.prompt.read(cx).value().trim().to_string();
        let workspace_id = ui.project.read(cx).selected_value().cloned();
        let branch = ui
            .branch
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default();
        let mode = ui.mode;
        let permission = ui.permission;
        let agent = ui.agent;
        let attachments = ui.attachments.clone();
        // Slash-command/skill submission dialect (codex `$name`, pi `/skill:name`,
        // template expansion);
        // no match (plain prompt / Builtin pass-through) keeps the original text.
        let prompt = ui
            .command_catalog
            .as_ref()
            .filter(|catalog| catalog.agent == agent)
            .and_then(|catalog| expand_command_submission(agent, &prompt, &catalog.commands))
            .unwrap_or(prompt);

        if prompt.is_empty() {
            window.push_notification("Describe what the Agent should do", cx);
            return;
        }
        let Some(workspace_id) = workspace_id else {
            window.push_notification("Select a Project", cx);
            return;
        };
        let visible_projects = self.visible_sidebar_projects();
        let Some(project_path) =
            sidebar_project_path_for_context(&visible_projects, Some(workspace_id.as_str()), None)
                .filter(|path| !path.trim().is_empty())
        else {
            window.push_notification("Selected Project has no usable path", cx);
            return;
        };
        let Some(client) = self.client.clone() else {
            window.push_notification("Herdr is not connected", cx);
            return;
        };

        let Some(ui) = self.new_agent_ui.as_mut() else {
            return;
        };
        ui.submitting = true;
        cx.notify();
        let delivery = std::sync::Arc::clone(&self.delivery);
        let intent = shardlane_host::AgentLaunchIntent {
            operation_id: format!(
                "new-agent-{}-{}",
                agent.as_str(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default()
            ),
            workspace_id: Some(workspace_id.clone()),
            project_path,
            branch,
            mode: mode.host_mode(),
            permission: permission.host_permission(),
            agent,
            prompt,
            attachments,
            extra_args: Vec::new(),
            skip_initial_prompt: false,
        };
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let launch_client = match client.as_herdr() {
                        Some(herdr) => herdr.clone(),
                        None => {
                            return Err(shardlane_host::AgentLaunchFailure::RuntimeCreateFailed(
                                "this instance does not support agents".to_string(),
                            ))
                        }
                    };
                    let preparation = shardlane_host::GitWorktreePreparation::new(
                        crate::settings::app_data_dir().join("worktrees"),
                    );
                    shardlane_host::run_agent_launch_with_ledger(
                        &launch_client,
                        &preparation,
                        &intent,
                        delivery.launch_ledger(),
                    )
                })
                .await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |view, cx| {
                    if let Some(ui) = view.new_agent_ui.as_mut() {
                        ui.submitting = false;
                    }
                    match result {
                        Ok(outcome) => {
                            view.new_agent_open = false;
                            view.new_agent_ui = None;
                            view.show_settings = false;
                            // FocusIntent seam: structural commands consume the authoritative payload first, then funnel through one place.
                            let created_tab_id = outcome.created.tab.tab_id.clone();
                            let created_pane_id = outcome.created.root_pane.pane_id.clone();
                            let created_workspace_id = outcome
                                .created
                                .tab
                                .workspace_id
                                .clone()
                                .unwrap_or_else(|| outcome.workspace_id.clone());
                            view.apply_created_tab(
                                outcome.created,
                                outcome.layout,
                                Some(outcome.workspace_id),
                                true,
                            );
                            view.status = ConnectionStatus::Connected;
                            view.reset_terminal_scroll_state();
                            view.sync_terminal_application_focus(cx);
                            view.notify_sidebar(cx);
                            cx.notify();
                            view.complete_navigation_after_authoritative_selection(
                                Some(created_workspace_id),
                                Some(created_tab_id),
                                Some(created_pane_id),
                                window,
                                cx,
                            );
                            // Agent-first: land in Chat when the exact semantic
                            // Conversation bound; Terminal stays the fallback.
                            if outcome.identity.is_some() {
                                view.chat.model.mode = crate::chat::WorkSurfaceMode::Chat;
                                cx.notify();
                            }
                        }
                        Err(shardlane_host::AgentLaunchFailure::AgentCreated {
                            agent_ref,
                            tab_id,
                            pane_id,
                            detail,
                            ..
                        }) => {
                            // The runtime mutation committed.  Close the
                            // launch form, focus the created target, and keep
                            // the setup detail visible; never reopen a blind
                            // retry that could create a second Agent.
                            view.new_agent_open = false;
                            view.new_agent_ui = None;
                            view.status = ConnectionStatus::Connected;
                            if let Some(client) = view.client.as_ref() {
                                if let Ok(state) = client.visible_state() {
                                    view.state = state;
                                }
                            }
                            view.chat.model.last_error = Some(format!(
                                "Agent created in pane {} but needs attention: {}",
                                pane_id, detail
                            ));
                            view.state.focused_tab_id = Some(tab_id.clone());
                            view.state.focused_pane_id = Some(pane_id.clone());
                            crate::derive_selection_flags(&mut view.state);
                            view.chat.model.mode = crate::chat::WorkSurfaceMode::Terminal;
                            let workspace_id = view.state.focused_workspace_id.clone();
                            view.complete_navigation_after_authoritative_selection(
                                workspace_id,
                                Some(tab_id),
                                Some(pane_id),
                                window,
                                cx,
                            );
                            let _ = agent_ref;
                            cx.notify();
                        }
                        Err(shardlane_host::AgentLaunchFailure::AgentStartUncertain {
                            agent_ref,
                            tab_id,
                            pane_id,
                            detail,
                        }) => {
                            // The start write may already have committed. Keep
                            // this target visible and close the launch form;
                            // an explicit reconcile is safer than retrying the
                            // same provider in a new pane.
                            view.new_agent_open = false;
                            view.new_agent_ui = None;
                            view.status = ConnectionStatus::Connected;
                            if let Some(client) = view.client.as_ref() {
                                if let Ok(state) = client.visible_state() {
                                    view.state = state;
                                }
                            }
                            view.chat.model.last_error = Some(format!(
                                "Agent start is uncertain in pane {}: {}",
                                pane_id, detail
                            ));
                            view.state.focused_tab_id = Some(tab_id);
                            view.state.focused_pane_id = Some(pane_id);
                            crate::derive_selection_flags(&mut view.state);
                            view.chat.model.mode = crate::chat::WorkSurfaceMode::Terminal;
                            let workspace_id = view.state.focused_workspace_id.clone();
                            let tab_id = view.state.focused_tab_id.clone();
                            let pane_id = view.state.focused_pane_id.clone();
                            view.complete_navigation_after_authoritative_selection(
                                workspace_id,
                                tab_id,
                                pane_id,
                                window,
                                cx,
                            );
                            let _ = agent_ref;
                            cx.notify();
                        }
                        Err(error) => {
                            let message = error.to_string();
                            view.status = ConnectionStatus::Offline(message.clone());
                            window.push_notification(message, cx);
                            cx.notify();
                        }
                    }
                });
            });
        })
        .detach();
    }

    pub(crate) fn submit_new_terminal_command(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_new_agent_ui(window, cx);
        let Some(ui) = self.new_agent_ui.as_ref() else {
            return;
        };
        if ui.submitting {
            return;
        }
        let command = ui.terminal_input.read(cx).value().trim().to_string();
        let workspace_id = ui.project.read(cx).selected_value().cloned();

        if command.is_empty() {
            window.push_notification("Enter a command to run", cx);
            return;
        }
        let Some(workspace_id) = workspace_id else {
            window.push_notification("Select a Project", cx);
            return;
        };
        let visible_projects = self.visible_sidebar_projects();
        let Some(project_path) =
            sidebar_project_path_for_context(&visible_projects, Some(workspace_id.as_str()), None)
                .filter(|path| !path.trim().is_empty())
        else {
            window.push_notification("Selected Project has no usable path", cx);
            return;
        };
        let Some(client) = self.client.clone() else {
            window.push_notification("Herdr is not connected", cx);
            return;
        };

        let Some(ui) = self.new_agent_ui.as_mut() else {
            return;
        };
        ui.submitting = true;
        cx.notify();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let launch_client = client.clone();
            let ws_id = workspace_id.clone();
            let cmd = command.clone();
            let cwd = project_path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    launch_terminal_command(launch_client.as_ref(), &ws_id, &cwd, &cmd)
                })
                .await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |view, cx| {
                    if let Some(ui) = view.new_agent_ui.as_mut() {
                        ui.submitting = false;
                    }
                    match result {
                        Ok((created, layout, workspace_id)) => {
                            view.new_agent_open = false;
                            view.new_agent_ui = None;
                            view.show_settings = false;
                            // FocusIntent seam: structural commands consume the authoritative payload first, then funnel through one place.
                            let created_tab_id = created.tab.tab_id.clone();
                            let created_pane_id = created.root_pane.pane_id.clone();
                            let created_workspace_id = created
                                .tab
                                .workspace_id
                                .clone()
                                .unwrap_or(workspace_id.clone());
                            view.apply_created_tab(created, layout, Some(workspace_id), true);
                            view.status = ConnectionStatus::Connected;
                            view.reset_terminal_scroll_state();
                            view.sync_terminal_application_focus(cx);
                            view.notify_sidebar(cx);
                            cx.notify();
                            view.complete_navigation_after_authoritative_selection(
                                Some(created_workspace_id),
                                Some(created_tab_id),
                                Some(created_pane_id),
                                window,
                                cx,
                            );
                        }
                        Err(error) => {
                            view.status = ConnectionStatus::Offline(error.clone());
                            window.push_notification(error, cx);
                            cx.notify();
                        }
                    }
                });
            });
        })
        .detach();
    }

    /// Empty-project onboarding state (the original NoProjectState language):
    /// sparkle icon + large title + a one-line guide + a round primary CTA — the
    /// first-run user's "create your first project" entry.
    pub(super) fn new_agent_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let herdr = cx.entity();
        div()
            .size_full()
            .bg(cx.theme().background)
            .px(px(28.0))
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(384.0))
                    .items_center()
                    .gap(SPACE_ICON)
                    .child(
                        div()
                            .mt(px(6.0))
                            .text_size(px(20.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().foreground)
                            .child("Add a Project to begin"),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .text_size(theme::FONT_DESCRIPTION)
                            .line_height(px(19.0))
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                "Projects point Shardlane at a working directory. Add one to start your first Agent.",
                            ),
                    )
                    .child(
                        // UX fix: a button promising "Add Project" must go through
                        // the real project-creation flow.
                        // The old implementation jumped to Settings→Appearance,
                        // a page with no Workspaces section at all — the user got
                        // dumped into theme settings, breaking the creation loop.
                        Button::new("new-agent-add-project")
                            .primary()
                            .small()
                            .rounded_full()
                            .icon(ComponentIconName::Plus)
                            .label("Add Project")
                            .tooltip("Choose a folder to create a project")
                            .on_click(move |_, window, app| {
                                herdr.update(app, |this, cx| {
                                    this.run_new_project_flow(window, cx);
                                });
                            }),
                    ),
            )
            .into_any_element()
    }
}
