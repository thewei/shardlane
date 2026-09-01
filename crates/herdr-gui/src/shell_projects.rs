//! [INPUT]: Depends on the ShardlaneApp type from the crate root (super) and existing types/imports (use super::*); no independent external dependencies.
//! [OUTPUT]: Exposes ShardlaneApp's project/tab management: create/close/pin; Tab drag's conversion to Herdr's authoritative order with a before-target and `tab.move`; Project drag's `workspace.move_block` write-back (ordering authority always belongs to Herdr; the Shardlane Workspace only filters membership); sidebar history loading (inherent impl shard).
//! [POS]: The `crates/herdr-gui` shell projects responsibility domain, mechanically split out of main.rs; together with sibling shell_* modules it forms ShardlaneApp's method surface.
use super::*;

impl ShardlaneApp {
    pub(super) fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        let workspace_id = self.active_workspace_id().map(str::to_string);
        self.create_tab_in_workspace(workspace_id, window, cx);
    }

    pub(super) fn open_new_agent(
        &mut self,
        _: &OpenNewAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_fresh_new_agent_surface(window, cx);
    }

    pub(super) fn create_tab_in_workspace(
        &mut self,
        workspace_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_agent_open = false;
        let Some(client) = self.client.clone() else {
            return;
        };
        let attach_window = window.window_handle();
        let create_workspace_id = workspace_id.clone();
        self.run_navigation_rpc(
            client,
            cx,
            Some(attach_window),
            false,
            move |client| {
                let created = client.create_tab(create_workspace_id.as_deref())?;
                let layout = client.pane_layout(&created.root_pane.pane_id)?;
                Ok((created, layout))
            },
            move |view, (created, layout), cx| {
                view.apply_created_tab(created, layout, workspace_id, true);
                view.reset_terminal_scroll_state();
                view.sync_terminal_application_focus(cx);
                true
            },
        );
    }

    pub(super) fn move_tab_to_index(
        &mut self,
        tab_id: String,
        workspace_id: String,
        display_insert_index: usize,
        cx: &mut Context<Self>,
    ) {
        let belongs_to_workspace = self.state.tabs.iter().any(|tab| {
            tab.tab_id == tab_id && tab.workspace_id.as_deref() == Some(workspace_id.as_str())
        });
        if !belongs_to_workspace {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        // The sidebar Tab order IS Herdr's authoritative order; Pinned is only a client-side
        // marker and must not create a second display order. Drop-on-row semantics = move
        // before the target, so only Herdr's remove+insert forward-index offset needs handling.
        let authoritative_ids: Vec<String> = self
            .state
            .tabs
            .iter()
            .filter(|tab| tab.workspace_id.as_deref() == Some(workspace_id.as_str()))
            .map(|tab| tab.tab_id.clone())
            .collect();
        let insert_index = before_target_authoritative_insert(
            &authoritative_ids,
            tab_id.as_str(),
            display_insert_index,
        )
        .unwrap_or(display_insert_index);
        self.run_navigation_rpc(
            client,
            cx,
            None,
            false,
            move |client| {
                client.move_tab(&tab_id, insert_index)?;
                Ok(())
            },
            // Authoritative correction flows through the already-subscribed tab.moved event (which
            // carries the whole workspace's TabInfo list; the event pump's 80ms debounce refreshes the
            // navigation projection); drag-and-drop no longer trails a full navigation_state() snapshot.
            move |_view, (), _cx| false,
        );
    }

    /// The visible Project list shared by the sidebar Projects section, the New
    /// Agent composer, and the right panel's Files: the bound instance's Herdr
    /// runtime workspaces in Herdr's authoritative order (no client-side grouping).
    pub(crate) fn visible_sidebar_projects(&self) -> Vec<VisibleSidebarProject> {
        crate::workspace_model::visible_sidebar_projects(&self.state, &self.scripts)
    }

    /// Sidebar Project drag ordering: the ordering authority is always the Herdr
    /// runtime. Uses the 0.8.x schema's `workspace.move_block` before-target
    /// semantics to write back to Herdr directly; afterwards it only waits for the
    /// workspace.moved/reordered events to refresh the projection.
    pub(super) fn reorder_sidebar_project(
        &mut self,
        dragged_runtime_workspace_id: String,
        target_runtime_workspace_id: String,
        cx: &mut Context<Self>,
    ) {
        if dragged_runtime_workspace_id == target_runtime_workspace_id {
            return;
        }
        let visible_ids = self
            .visible_sidebar_projects()
            .into_iter()
            .map(|project| project.runtime_workspace_id)
            .collect::<std::collections::HashSet<_>>();
        if !visible_ids.contains(&dragged_runtime_workspace_id)
            || !visible_ids.contains(&target_runtime_workspace_id)
        {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        self.run_navigation_rpc(
            client,
            cx,
            None,
            false,
            move |client| {
                client.move_workspace_before(
                    &dragged_runtime_workspace_id,
                    &target_runtime_workspace_id,
                )?;
                Ok(())
            },
            move |_view, (), _cx| false,
        );
    }

    pub(super) fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab_id) = self.active_tab().map(|tab| tab.tab_id.clone()) else {
            return;
        };
        self.close_tab_by_id(tab_id, window, cx);
    }

    pub(super) fn new_project(
        &mut self,
        _: &NewProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_new_project_flow(window, cx);
    }

    /// Multi-instance New Project: creates a Project registry entry backed by a
    /// dedicated Herdr session (`shardlane-project-<n>`; names are checked
    /// against the registry AND existing CLI sessions so a foreign instance is
    /// never hijacked), then rebinds THIS window to it. Projects carry no
    /// working directory of their own — per-Tab cwds belong to Herdr, and the
    /// instance's first workspace is created on first bind.
    /// Multi-instance New Workspace: creates a new Herdr instance (unique
    /// `shardlane-project-<n>` session; collisions checked against live
    /// sessions so CLI-created instances are never hijacked) and rebinds this
    /// window to it. There is no Shardlane-side workspace registry, and no
    /// per-Project directory — per-Tab cwds belong to Herdr.
    pub(super) fn run_new_project_flow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_history_surface(cx);
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let session = cx
                .background_executor()
                .spawn(async move {
                    let existing = shardlane_host::herdr::list_sessions().unwrap_or_default();
                    let ordinal = existing.iter().filter(|s| !s.is_default).count() + 1;
                    let mut suffix = ordinal;
                    loop {
                        let candidate = format!("shardlane-project-{suffix}");
                        if !existing.iter().any(|s| s.name == candidate) {
                            break candidate;
                        }
                        suffix += 1;
                    }
                })
                .await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |view, cx| {
                    view.bind_instance(Some(session), window, cx);
                    window.push_notification("Workspace created", cx);
                });
            });
        })
        .detach();
    }

    pub(super) fn close_project(
        &mut self,
        _: &CloseProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace_id) = self.active_workspace_id().map(str::to_string) else {
            return;
        };
        self.close_workspace_id(workspace_id, window, cx);
    }

    /// Audit A06: the shared close-RPC scaffolding for `close_workspace_id`/`close_tab_by_id` —
    /// run the destructive close, then pull the authoritative navigation projection (plus the
    /// follow-up surface when the closed item was focused) and apply it.
    fn run_close_rpc<F>(
        &mut self,
        close: F,
        closing_active: bool,
        attach_window: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce(&HerdrClient) -> Result<(), herdr::HerdrError> + Send + 'static,
    {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.run_navigation_rpc(
            client,
            cx,
            Some(attach_window),
            false,
            move |client| {
                close(client)?;
                let navigation = client.navigation_state()?;
                let surface = if closing_active {
                    match (
                        navigation.focused_workspace_id.as_deref(),
                        navigation.focused_tab_id.as_deref(),
                    ) {
                        (Some(workspace_id), Some(tab_id)) => {
                            Some(client.tab_surface_state(workspace_id, tab_id)?)
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                Ok((navigation, surface))
            },
            move |view, (navigation, surface), cx| {
                view.apply_navigation_state(navigation);
                if let Some(surface) = surface {
                    view.apply_tab_surface_state(surface, cx);
                } else if closing_active && view.state.focused_tab_id.is_none() {
                    view.clear_terminal_surface(cx);
                }
                closing_active && view.state.focused_tab_id.is_some()
            },
        );
    }

    pub(super) fn close_workspace_id(
        &mut self,
        workspace_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let closing_active = self.state.focused_workspace_id.as_deref() == Some(&workspace_id);
        let attach_window = window.window_handle();
        self.run_close_rpc(
            move |client| client.close_workspace(&workspace_id),
            closing_active,
            attach_window,
            cx,
        );
    }

    pub(super) fn toggle_project_folder(
        &mut self,
        workspace_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_close_tab = None;
        let expanded = self
            .sidebar_pane
            .update(cx, |pane, cx| pane.toggle_project(&workspace_id, cx));
        if expanded {
            self.load_sidebar_project_panes(workspace_id.clone(), cx);
        }
        self.new_agent_context_workspace_id = Some(workspace_id.clone());
        if self.new_agent_open {
            self.select_new_agent_project(workspace_id, window, cx);
        }
        self.sync_right_panel_for_project_context(cx);
    }

    /// Native-tab placement (`terminal.tab_bar_placement`): a Sidebar Project click focuses the
    /// Project instead of expanding a Tab subtree — the Tabs are presented by the content-area
    /// Tab strip. Keeps the same Project-context side effects as `toggle_project_folder` so the
    /// New Agent composer and the right panel follow the clicked Project.
    pub(super) fn focus_project_from_sidebar(
        &mut self,
        workspace_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_close_tab = None;
        // Multi-instance model: rows synthesized from the Project registry carry a
        // "project:<id>" id. Clicking one jumps to the Project's window when one
        // exists, otherwise rebinds THIS window to that instance. Runtime workspace
        // ids (the bound instance's own workspaces) keep the legacy focus path.
        if let Some(project_id) = workspace_id.strip_prefix("project:") {
            self.open_or_jump_project(project_id, window, cx);
            return;
        }
        self.new_agent_context_workspace_id = Some(workspace_id.clone());
        if self.new_agent_open {
            self.select_new_agent_project(workspace_id.clone(), window, cx);
        }
        self.sync_right_panel_for_project_context(cx);
        self.apply_focus_intent(FocusIntent::project(workspace_id), window, cx);
    }

    pub(super) fn load_sidebar_project_panes(
        &mut self,
        workspace_id: String,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let should_load = self
            .sidebar_pane
            .update(cx, |pane, _| pane.begin_project_pane_load(&workspace_id));
        if !should_load {
            return;
        }
        let sidebar = self.sidebar_pane.clone();
        cx.spawn(async move |_this, cx| {
            let load_workspace_id = workspace_id.clone();
            let panes = cx
                .background_executor()
                .spawn(async move { client.workspace_panes(&load_workspace_id) })
                .await;
            let sidebar_workspace_id = workspace_id.clone();
            let _ = sidebar.update(cx, |pane, cx| {
                pane.finish_project_pane_load(sidebar_workspace_id, panes, cx);
            });
        })
        .detach();
    }

    /// Display title for a Tab: Herdr's own title fields first, then the focused pane's.
    pub(super) fn tab_title(&self, tab: &Tab) -> String {
        tab.terminal_title
            .as_deref()
            .or(tab.title.as_deref())
            .or(tab.label.as_deref())
            .or_else(|| {
                self.state
                    .panes
                    .iter()
                    .find(|pane| pane.tab_id.as_deref() == Some(tab.tab_id.as_str()))
                    .and_then(|pane| {
                        pane.terminal_title
                            .as_deref()
                            .or(pane.title.as_deref())
                            .or(pane.label.as_deref())
                    })
            })
            .unwrap_or(&tab.tab_id)
            .to_string()
    }

    pub(crate) fn confirm_close_tab(
        &mut self,
        tab_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending_close_tab.as_deref() == Some(&tab_id) {
            self.pending_close_tab = None;
            self.close_tab_by_id(tab_id, window, cx);
        } else {
            self.pending_close_tab = Some(tab_id);
            cx.notify();
        }
    }

    pub(super) fn toggle_pin_tab(&mut self, tab_id: String, cx: &mut Context<Self>) {
        if let Some(pos) = self
            .config
            .ui
            .sidebar
            .pinned_tabs
            .iter()
            .position(|id| id == &tab_id)
        {
            self.config.ui.sidebar.pinned_tabs.remove(pos);
        } else {
            self.config.ui.sidebar.pinned_tabs.push(tab_id);
        }
        self.config.save();
        cx.notify();
    }

    /// Stale pinned_tabs sweep (alongside navigation reconciliation): removes
    /// runtime tab IDs absent from the current Herdr snapshot; persists only when
    /// something was removed (no-op contract). Pinned persistence stores runtime
    /// tab IDs (which change on Herdr restart), so they must converge through
    /// navigation reconciliation to avoid the config accumulating dead IDs forever.
    pub(crate) fn prune_stale_pinned_tabs(&mut self, cx: &mut Context<Self>) -> usize {
        if !self.status.is_connected() {
            return 0;
        }
        let pinned = &mut self.config.ui.sidebar.pinned_tabs;
        let before = pinned.len();
        pinned.retain(|tab_id| self.state.tabs.iter().any(|tab| &tab.tab_id == tab_id));
        let removed = before.saturating_sub(pinned.len());
        if removed > 0 {
            lag_log(format_args!("sidebar.pinned_tabs pruned removed={removed}"));
            self.save_config();
            self.notify_sidebar(cx);
            cx.notify();
        }
        removed
    }

    pub(super) fn close_tab_by_id(
        &mut self,
        tab_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let closing_active = self.state.focused_tab_id.as_deref() == Some(&tab_id);
        let attach_window = window.window_handle();
        self.run_close_rpc(
            move |client| client.close_tab(&tab_id),
            closing_active,
            attach_window,
            cx,
        );
    }
}

/// Drop-on-row conversion to Herdr's authoritative order: the dragged item lands before the target row.
/// Herdr `tab.move`'s insert_index is interpreted against the post-remove list, so forward drags subtract one.
/// Returns None when the dragged item is not in the authoritative list.
pub(crate) fn before_target_authoritative_insert(
    authoritative_ids: &[String],
    dragged: &str,
    target_index: usize,
) -> Option<usize> {
    let dragged_index = authoritative_ids.iter().position(|id| id == dragged)?;
    Some(if dragged_index < target_index {
        target_index.saturating_sub(1)
    } else {
        target_index
    })
}
