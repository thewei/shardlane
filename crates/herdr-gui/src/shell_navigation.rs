//! [INPUT]: Depends on the ShardlaneApp type from the crate root (super) and existing types/imports (use super::*); no independent external dependencies.
//! [OUTPUT]: Exposes ShardlaneApp's navigation and event projection: workspace/tab/pane focus RPCs (`run_navigation_rpc` scaffold), Herdr event subscription, and status patch application (inherent impl shard).
//! [POS]: The `crates/herdr-gui` shell navigation responsibility domain, mechanically split out of main.rs; together with sibling shell_* modules it forms ShardlaneApp's method surface.
use super::*;

/// Shell-level unified navigation intent (FocusIntent seam): Sidebar, Global
/// Search, History Continue/live targets, New Agent success, Activity, system notification clicks,
/// and status/menu jumps all construct this intent and execute it via [`ShardlaneApp::apply_focus_intent`].
/// No entry point may implement its own focus chain, nor treat terminal attach/render initialization
/// as a navigation side effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FocusIntent {
    Project {
        workspace_id: String,
    },
    Tab {
        tab_id: String,
    },
    /// Pane target; default fields are filled from the local projection by the existing resolution chain,
    /// and when it hits an Agent, protocol-20 `agent.focus` goes direct (plan §12.5), falling back to the pane chain on failure.
    Pane {
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: String,
    },
    /// Agent target (audit P2-6): `terminal_id` is the stable primary identity (`pane_id` may be absent
    /// — see shardlane-host's `Agent` model). The TUI focus chain prefers protocol-20
    /// `agent.focus(terminal_id)`, falling back to workspace→tab→pane on failure.
    Agent {
        terminal_id: String,
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: Option<String>,
    },
}

impl FocusIntent {
    pub(super) fn project(workspace_id: impl Into<String>) -> Self {
        Self::Project {
            workspace_id: workspace_id.into(),
        }
    }

    pub(super) fn tab(tab_id: impl Into<String>) -> Self {
        Self::Tab {
            tab_id: tab_id.into(),
        }
    }

    pub(super) fn pane(
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: impl Into<String>,
    ) -> Self {
        Self::Pane {
            workspace_id,
            tab_id,
            pane_id: pane_id.into(),
        }
    }

    /// Resolve an intent from any known subset of ids (status/menu, Header summary jumps, etc. — cases that
    /// only know part of the attribution): the most specific Pane first, then Tab, then Project; all-empty returns None.
    pub(super) fn from_targets(
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: Option<String>,
    ) -> Option<Self> {
        if let Some(pane_id) = pane_id {
            return Some(Self::Pane {
                workspace_id,
                tab_id,
                pane_id,
            });
        }
        if let Some(tab_id) = tab_id {
            return Some(Self::Tab { tab_id });
        }
        workspace_id.map(|workspace_id| Self::Project { workspace_id })
    }

    /// Agent intent with a known stable terminal_id (audit P2-6): terminal_id is no longer lost
    /// just because pane_id is absent.
    pub(super) fn agent(
        terminal_id: impl Into<String>,
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: Option<String>,
    ) -> Self {
        Self::Agent {
            terminal_id: terminal_id.into(),
            workspace_id,
            tab_id,
            pane_id,
        }
    }
}

impl ShardlaneApp {
    pub(super) fn sync_pane_event_subscription(&mut self, cx: &mut Context<Self>) {
        let mut pane_ids = self
            .state
            .panes
            .iter()
            .map(|pane| pane.pane_id.clone())
            .collect::<Vec<_>>();
        pane_ids.sort();
        pane_ids.dedup();
        if pane_ids == self.pane_event_subscription_ids {
            return;
        }
        self.pane_event_subscription_ids = pane_ids.clone();
        self._pane_event_subscription_script = BackgroundJob::ready(());
        let Some(client) = self.client.clone() else {
            return;
        };
        if pane_ids.is_empty() {
            return;
        }
        // The handshake is a blocking connect+ack socket round trip and must leave the UI thread;
        // the handle is held by a field, and an old handshake is cancelled when the pane set changes again.
        self._pane_event_subscription_handshake = cx.spawn(async move |this, cx| {
            let expected = pane_ids.clone();
            let result = cx
                .background_executor()
                .spawn(async move { client.subscribe_pane_events(&pane_ids) })
                .await;
            let _ = this.update(cx, |view, cx| {
                // Race guard: the pane set advanced again during the handshake (a new sync updated the ids);
                // this result is void.
                if view.pane_event_subscription_ids != expected {
                    return;
                }
                match result {
                    Ok(events) => {
                        // F19: subscription succeeded, reset the retry counter.
                        view.pane_subscription_retry_attempts = 0;
                        view._pane_event_subscription_script = cx.spawn(async move |this, cx| {
                            while let Ok(event) = events.recv().await {
                                // TUI-only doesn't consume Herdr's pane scroll projection. This used to
                                // write pane.scroll_changed into old Embedded state and notify the whole
                                // Root/Sidebar, letting real scrolling steal the terminal's frame budget with a
                                // 6–7ms root render. Only Agent status patches still need to enter UI projection.
                                let Some(patch) = event.agent_status_patch() else {
                                    continue;
                                };
                                let _ = this.update(cx, |view, cx| {
                                    let agent_changed =
                                        view.apply_agent_status_patch(&patch).unwrap_or(false);
                                    if agent_changed {
                                        view.notify_sidebar(cx);
                                        cx.notify();
                                    }
                                });
                            }
                        });
                    }
                    Err(error) => {
                        lag_log(format_args!("pane event subscription failed: {error}"));
                        // A backend without push events (events_push=false, e.g.
                        // tmux) is a permanent degradation, not a transient
                        // failure: retrying would spin the backoff loop forever.
                        if !matches!(
                            error,
                            shardlane_host::mux::MuxError::Unsupported("events_push")
                        ) {
                            // Clear the recorded set; the next surface application retries the subscription;
                            // F19: also do bounded backoff retries instead of waiting for the user to navigate.
                            view.pane_event_subscription_ids.clear();
                            view.schedule_pane_subscription_retry(cx);
                        }
                    }
                }
            });
        });
    }

    pub(super) fn start_event_subscription(
        client: std::sync::Arc<dyn shardlane_host::mux::MultiplexerConnection>,
        events: async_channel::Receiver<HerdrEvent>,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            // Multi-instance binding guard: a Project rebind bumps binding_generation;
            // this loop (subscribed to the PREVIOUS instance's socket) must exit instead
            // of writing the old instance's state into the newly bound window.
            let mut last_agent_refresh = Instant::now();
            let mut last_navigation_dirty_at = None;
            let mut agent_refresh_pending = false;
            let mut agent_dirty_started_at: Option<Instant> = None;
            let mut last_agent_dirty_at: Option<Instant> = None;
            loop {
                let binding_current = this
                    .update(cx, |view, _| view.binding_generation == generation)
                    .unwrap_or(false);
                if !binding_current {
                    return;
                }
                let needs_timer = agent_refresh_pending
                    || last_navigation_dirty_at.is_some()
                    || this
                        .update(cx, |view, _| {
                            view.initializing || view.navigation_reconcile_pending
                        })
                        .unwrap_or(false);
                let mut events_closed = false;
                let first_event = if needs_timer {
                    let event_wait = events.recv();
                    let timer = cx.background_executor().timer(Duration::from_millis(80));
                    futures::pin_mut!(event_wait, timer);
                    match select(event_wait, timer).await {
                        Either::Left((Ok(event), _)) => Some(event),
                        Either::Left((Err(_), _)) => {
                            events_closed = true;
                            None
                        }
                        Either::Right(_) => None,
                    }
                } else {
                    match events.recv().await {
                        Ok(event) => Some(event),
                        Err(_) => {
                            events_closed = true;
                            None
                        }
                    }
                };

                let mut navigation_dirty = false;
                let mut navigation_focus_dirty = false;
                let mut surface_dirty_workspaces = HashSet::new();
                let mut surface_dirty_unknown = false;
                let mut agents_dirty = false;
                let mut agent_status_patches = Vec::new();
                let mut updated_layout = None;
                {
                    let mut absorb_event = |event: HerdrEvent| {
                        navigation_dirty |= event.refreshes_navigation_projection();
                        navigation_focus_dirty |= event.refreshes_navigation_focus_projection();
                        if event.refreshes_tab_surface_projection() {
                            if let Some(workspace_id) = event.affected_workspace_id() {
                                surface_dirty_workspaces.insert(workspace_id);
                            } else {
                                surface_dirty_unknown = true;
                            }
                        }
                        if let Some(patch) = event.agent_status_patch() {
                            agent_status_patches.push(patch);
                        } else if event.refreshes_agents() {
                            agents_dirty = true;
                        }
                        if let Some(layout) = event.updated_layout() {
                            updated_layout = Some(layout);
                        }
                    };
                    if let Some(event) = first_event {
                        absorb_event(event);
                    }
                    loop {
                        match events.try_recv() {
                            Ok(event) => absorb_event(event),
                            Err(async_channel::TryRecvError::Empty) => break,
                            Err(async_channel::TryRecvError::Closed) => {
                                events_closed = true;
                                break;
                            }
                        }
                    }
                }

                if !agent_status_patches.is_empty() {
                    let unresolved = this
                        .update(cx, |view, cx| {
                            let mut unresolved = false;
                            let mut changed = false;
                            for patch in &agent_status_patches {
                                match view.apply_agent_status_patch(patch) {
                                    Some(patch_changed) => changed |= patch_changed,
                                    None => unresolved = true,
                                }
                            }
                            if changed {
                                view.notify_sidebar(cx);
                                cx.notify();
                            }
                            unresolved
                        })
                        .unwrap_or(true);
                    agents_dirty |= unresolved;
                }

                for workspace_id in &surface_dirty_workspaces {
                    let workspace_id = workspace_id.clone();
                    let _ = this.update(cx, |view, cx| {
                        let should_reload = view
                            .sidebar_pane
                            .update(cx, |pane, _| pane.invalidate_project_panes(&workspace_id));
                        if should_reload {
                            view.load_sidebar_project_panes(workspace_id.clone(), cx);
                        }
                    });
                }

                if navigation_dirty {
                    last_navigation_dirty_at = Some(Instant::now());
                }

                if let Some(layout) = updated_layout {
                    let _ = this.update(cx, |view, cx| {
                        if view.navigation_loading
                            || view.state.focused_tab_id.as_deref() != Some(layout.tab_id.as_str())
                        {
                            return;
                        }
                        view.apply_current_layout(layout, cx);
                    });
                }

                let navigation_dirty_age = last_navigation_dirty_at.map(|at: Instant| at.elapsed());
                let should_reconcile_navigation = this
                    .update(cx, |view, _| {
                        view.navigation_reconcile_pending |=
                            navigation_dirty || navigation_focus_dirty;
                        if navigation_reconcile_ready(
                            view.navigation_reconcile_pending,
                            view.navigation_loading,
                            navigation_dirty_age,
                        ) {
                            std::mem::take(&mut view.navigation_reconcile_pending)
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if should_reconcile_navigation {
                    last_navigation_dirty_at = None;
                }

                let mut navigation_focus_changed = false;
                if should_reconcile_navigation {
                    let refresh_client = client.clone();
                    let refreshed = cx
                        .background_executor()
                        .spawn(async move { refresh_client.navigation_state() })
                        .await;
                    navigation_focus_changed = this
                        .update(cx, |view, cx| {
                            if view.navigation_loading {
                                view.navigation_reconcile_pending = true;
                                return false;
                            }
                            match refreshed {
                                Ok(navigation) => {
                                    let previous_focus = (
                                        view.state.focused_workspace_id.clone(),
                                        view.state.focused_tab_id.clone(),
                                    );
                                    let changed = view.apply_navigation_state(navigation);
                                    let status_changed = !view.status.is_connected();
                                    view.status = ConnectionStatus::Connected;
                                    let focus_changed = previous_focus
                                        != (
                                            view.state.focused_workspace_id.clone(),
                                            view.state.focused_tab_id.clone(),
                                        );
                                    if changed || status_changed {
                                        if view.initializing {
                                            view.last_startup_change_at = Instant::now();
                                        }
                                        view.notify_sidebar(cx);
                                        cx.notify();
                                    }
                                    focus_changed
                                }
                                Err(err) => {
                                    view.status = ConnectionStatus::Offline(err.to_string());
                                    false
                                }
                            }
                        })
                        .unwrap_or(false);
                }

                let surface_target = this
                    .update(cx, |view, _| {
                        if view.navigation_loading {
                            return None;
                        }
                        let workspace_id = view.state.focused_workspace_id.clone()?;
                        let tab_id = view.state.focused_tab_id.clone()?;
                        let affected = surface_dirty_unknown
                            || surface_dirty_workspaces.contains(&workspace_id);
                        (navigation_focus_changed || affected || navigation_focus_dirty)
                            .then_some((workspace_id, tab_id))
                    })
                    .ok()
                    .flatten();
                if let Some((workspace_id, tab_id)) = surface_target {
                    let refresh_client = client.clone();
                    let refreshed = cx
                        .background_executor()
                        .spawn(
                            async move { refresh_client.tab_surface_state(&workspace_id, &tab_id) },
                        )
                        .await;
                    let window_handle = this
                        .update(cx, |view, cx| {
                            if view.navigation_loading {
                                return None;
                            }
                            match refreshed {
                                Ok(surface) => {
                                    view.apply_tab_surface_state(surface, cx);
                                    view.status = ConnectionStatus::Connected;
                                    view.sync_terminal_application_focus(cx);
                                    view.notify_sidebar(cx);
                                    cx.notify();
                                    view.window_handle
                                }
                                Err(err) => {
                                    view.status = ConnectionStatus::Offline(err.to_string());
                                    None
                                }
                            }
                        })
                        .ok()
                        .flatten();
                    if let Some(window_handle) = window_handle {
                        let _ = cx.update_window(window_handle, |_, window, cx| {
                            let _ = this.update(cx, |view, cx| {
                                view.attach_focused_terminal(window, cx);
                            });
                        });
                    }
                }

                if agents_dirty {
                    let now = Instant::now();
                    if !agent_refresh_pending {
                        agent_dirty_started_at = Some(now);
                    }
                    last_agent_dirty_at = Some(now);
                    agent_refresh_pending = true;
                }
                if agent_reconcile_ready(
                    agent_refresh_pending,
                    last_agent_dirty_at.map(|at| at.elapsed()),
                    agent_dirty_started_at.map(|at| at.elapsed()),
                    last_agent_refresh.elapsed(),
                ) {
                    agent_refresh_pending = false;
                    agent_dirty_started_at = None;
                    last_agent_dirty_at = None;
                    last_agent_refresh = Instant::now();
                    let refresh_client = client.clone();
                    let refreshed = cx
                        .background_executor()
                        .spawn(async move { refresh_client.agents() })
                        .await;
                    let _ = this.update(cx, |view, cx| {
                        if let Ok(agents) = refreshed {
                            if view.state.agents != agents {
                                view.state.agents = agents;
                                if view.initializing {
                                    view.last_startup_change_at = Instant::now();
                                }
                                view.notify_sidebar(cx);
                            }
                        }
                    });
                }

                let _ = this.update(cx, |view, cx| {
                    if !view.initializing {
                        return;
                    }
                    let now = Instant::now();
                    let minimum_loading =
                        now.duration_since(view.startup_started_at) >= Duration::from_millis(350);
                    let event_stream_is_quiet = now.duration_since(view.last_startup_change_at)
                        >= Duration::from_millis(120);
                    let terminal_attach_is_settled = view.terminal_attach_target.is_none();
                    let loading_timeout =
                        now.duration_since(view.startup_started_at) >= Duration::from_millis(2500);
                    if (minimum_loading && event_stream_is_quiet && terminal_attach_is_settled)
                        || loading_timeout
                    {
                        view.initializing = false;
                        view.notify_sidebar(cx);
                        cx.notify();
                    }
                });

                if events_closed {
                    lag_log(format_args!("herdr.events disconnected"));
                    let _ = this.update(cx, |view, cx| {
                        if view.initializing {
                            view.initializing = false;
                        }
                        view.status =
                            ConnectionStatus::Offline("herdr events disconnected".to_string());
                        view.notify_sidebar(cx);
                        cx.notify();
                        // F47: no longer waiting for the user to click reconnect — rebuild automatically in the
                        // background with exponential backoff (bootstrap + resubscribe); once restored, the pane
                        // subscription and projection are rebuilt.
                        view.schedule_events_reconnect(cx);
                    });
                    return;
                }
            }
        })
        .detach();
    }

    pub(super) fn refresh_agents_snapshot(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let refreshed = cx
                .background_executor()
                .spawn(async move { client.agents() })
                .await;
            let _ = this.update(cx, |view, cx| {
                if let Ok(agents) = refreshed {
                    if view.state.agents != agents {
                        view.state.agents = agents;
                        view.notify_sidebar(cx);
                    }
                }
            });
        })
        .detach();
    }

    pub(super) fn previous_tab(
        &mut self,
        _: &PreviousTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_tab_offset(-1, window, cx);
    }

    pub(super) fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_tab_offset(1, window, cx);
    }

    pub(super) fn previous_project(
        &mut self,
        _: &PreviousProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_project_offset(-1, window, cx);
    }

    pub(super) fn next_project(
        &mut self,
        _: &NextProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_project_offset(1, window, cx);
    }

    /// The FocusIntent seam's sole executor: all cross-domain navigation entries (Sidebar/Search/History/
    /// New Agent/Activity/notification/status) construct an intent and land focus here uniformly. It records
    /// history, then dispatches on the target's most specific id to the existing focus family implementations
    /// (local ShellSelection update + TUI host focus chain); entries no longer carry their own chains.
    pub(super) fn apply_focus_intent(
        &mut self,
        intent: FocusIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Re-selecting the already-active target is not navigation: pushing the current position
        // onto the back stack and clearing the forward stack would corrupt history for a no-op
        // (audit A14). The secondary-surface exits still apply, mirroring focus_workspace_id's
        // F24 ordering — clicking the selected object while Settings/Help is open must exit it.
        if self.focus_intent_is_current_selection(&intent) {
            self.new_agent_open = false;
            self.pending_close_tab = None;
            self.leave_history_surface(cx);
            self.exit_blocked_secondary_surfaces(cx);
            window.focus(&self.focus_handle);
            return;
        }
        // Record the current position onto the back stack
        if let Some(current) = self.current_focus_intent() {
            if self.nav_back_stack.last() != Some(&current) {
                self.nav_back_stack.push(current);
                // Bound the history depth to prevent unbounded memory growth
                if self.nav_back_stack.len() > 64 {
                    self.nav_back_stack.remove(0);
                }
                self.nav_forward_stack.clear();
            }
        }
        // Only direct (forward) agent jumps count as Switcher MRU accesses; history replays
        // dispatch through the same executor without re-ranking the MRU list.
        if let FocusIntent::Agent { terminal_id, .. } = &intent {
            self.agent_switcher.record_access(terminal_id);
        }
        self.execute_focus_intent(intent, window, cx);
    }

    /// Audit A14: does this intent already describe the current selection? Resolves the intent's
    /// most specific target id and compares it with the live selection.
    fn focus_intent_is_current_selection(&self, intent: &FocusIntent) -> bool {
        match intent {
            FocusIntent::Project { workspace_id } => {
                self.state.focused_workspace_id.as_deref() == Some(workspace_id.as_str())
            }
            FocusIntent::Tab { tab_id } => {
                self.state.focused_tab_id.as_deref() == Some(tab_id.as_str())
            }
            FocusIntent::Pane { pane_id, .. } => {
                self.state.focused_pane_id.as_deref() == Some(pane_id.as_str())
            }
            FocusIntent::Agent {
                terminal_id,
                pane_id,
                ..
            } => match pane_id.as_deref() {
                Some(pane_id) => self.state.focused_pane_id.as_deref() == Some(pane_id),
                None => self
                    .state
                    .focused_pane_id
                    .as_deref()
                    .and_then(|focused_pane| {
                        self.state
                            .agents
                            .iter()
                            .find(|agent| agent.pane_id.as_deref() == Some(focused_pane))
                    })
                    .is_some_and(|agent| agent.terminal_id == terminal_id.as_str()),
            },
        }
    }

    /// Audit A05: the shared `match intent {…}` dispatch used by apply_focus_intent and the
    /// back/forward executors. This is also the single focus-restoration point for intent-driven
    /// navigation (audit A04, F35): after a jump the window focus may linger on a removed input
    /// handle, and the next registry chord would be swallowed as a dead key — explicitly return
    /// root focus like focus_pane_id does.
    fn execute_focus_intent(
        &mut self,
        intent: FocusIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        match intent {
            FocusIntent::Project { workspace_id } => {
                self.focus_workspace_id(workspace_id, window, cx);
            }
            FocusIntent::Tab { tab_id } => self.focus_tab_id(tab_id, window, cx),
            FocusIntent::Pane {
                workspace_id,
                tab_id,
                pane_id,
            } => self.focus_target(workspace_id, tab_id, Some(pane_id), window, cx),
            FocusIntent::Agent {
                terminal_id,
                workspace_id,
                tab_id,
                pane_id,
            } => self.focus_agent_target(terminal_id, workspace_id, tab_id, pane_id, window, cx),
        }
    }

    /// Build a FocusIntent snapshot from the current client selection state (for back/forward history).
    fn current_focus_intent(&self) -> Option<FocusIntent> {
        if let Some(pane_id) = self.state.focused_pane_id.clone() {
            return Some(FocusIntent::Pane {
                workspace_id: self.active_workspace_id().map(String::from),
                tab_id: self.active_tab().map(|t| t.tab_id.clone()),
                pane_id,
            });
        }
        if let Some(tab) = self.active_tab() {
            return Some(FocusIntent::Tab {
                tab_id: tab.tab_id.clone(),
            });
        }
        if let Some(ws_id) = self.active_workspace_id() {
            return Some(FocusIntent::Project {
                workspace_id: ws_id.to_string(),
            });
        }
        None
    }

    /// Go back to the previous navigation position.
    pub(super) fn navigate_back(
        &mut self,
        _: &NavigateBack,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prev) = self.nav_back_stack.pop() else {
            return;
        };
        // Push the current position onto the forward stack
        if let Some(current) = self.current_focus_intent() {
            self.nav_forward_stack.push(current);
        }
        self.execute_focus_intent(prev, window, cx);
    }

    /// Go forward to the next navigation position.
    pub(super) fn navigate_forward(
        &mut self,
        _: &NavigateForward,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(next) = self.nav_forward_stack.pop() else {
            return;
        };
        // Push the current position onto the back stack
        if let Some(current) = self.current_focus_intent() {
            self.nav_back_stack.push(current);
        }
        self.execute_focus_intent(next, window, cx);
    }

    /// Shared focus chain driver for the TUI host mode: after ensuring the host is alive, drive the
    /// protocol-20 direct focus chain (workspace.focus → tab.focus → pane.focus; when a pane hits an
    /// Agent, agent.focus goes direct) so the host TUI follows. Navigation never respawns the host and
    /// never uses zoom workarounds; no-op outside TUI mode (Embedded surfaces own their completion paths).
    pub(super) fn drive_tui_focus_chain(
        &mut self,
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: Option<String>,
        agent_terminal: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ensure_tui_surface(window, cx, true);
        // Explicit terminal_id takes priority (FocusIntent::Agent); otherwise resolve by pane hit.
        let agent_terminal = agent_terminal.or_else(|| {
            pane_id
                .as_deref()
                .and_then(|pane_id| self.tui_agent_terminal_for(Some(pane_id)))
        });
        self.dispatch_tui_focus(workspace_id, tab_id, pane_id, agent_terminal, cx);
    }

    /// Unified navigation convergence after a structural command has consumed the authoritative payload
    /// and written ShellSelection (New Agent created / History Continue / new Project): this only completes
    /// the terminal part — TUI mode drives the shared focus chain, and Embedded kept attach during the
    /// transition (deleted at T4); it never re-pulls surface snapshots, honoring the "structural commands
    /// consume the authoritative return" discipline.
    pub(super) fn complete_navigation_after_authoritative_selection(
        &mut self,
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // TUI-only: structural command convergence = driving the shared focus chain; Embedded attach was
        // deleted with the per-Pane runtime.
        // Focus return (measured 2026-08-28): after a New Agent / New Terminal submit, removing the secondary
        // surface's composer doesn't trigger blur, so window focus lingers on a dead handle — the first
        // registry chord (e.g. ⌘V) is intercepted by the swallow guard and the action can't dispatch, becoming
        // a dead key (the typing path self-heals via the Terminal route; paste doesn't). All structural
        // commands converge here, explicitly returning root focus like focus_pane_id does.
        window.focus(&self.focus_handle);
        self.drive_tui_focus_chain(workspace_id, tab_id, pane_id, None, window, cx);
        self.sync_terminal_application_focus(cx);
    }

    pub(super) fn focus_workspace_id(
        &mut self,
        workspace_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // F24: surface exit precedes the same-id early return — clicking the "already selected"
        // object while Settings/Help is open must also exit the blocking surface, aligned with History's behavior.
        self.new_agent_open = false;
        self.exit_blocked_secondary_surfaces(cx);
        self.leave_history_surface(cx);
        self.focus_workspace_id_inner(workspace_id, window, cx);
    }

    /// Close secondary surfaces that block the terminal (Settings/Help); the History surface exits via its dedicated path.
    pub(super) fn exit_blocked_secondary_surfaces(&mut self, cx: &mut Context<Self>) {
        // The blocked-surface set is whatever stands between the user and the
        // work surface they just navigated to. The New Agent page is the most
        // frequent occupant (it is the startup landing surface), so a sidebar
        // / picker click that skips it looked like a dead button until the
        // page joined this exit list.
        let mut changed = false;
        if self.show_settings || self.show_help {
            self.show_settings = false;
            self.show_help = false;
            changed = true;
        }
        if self.new_agent_open {
            self.new_agent_open = false;
            changed = true;
        }
        if changed {
            self.sync_terminal_application_focus(cx);
            cx.notify();
        }
    }

    pub(super) fn focus_workspace_id_inner(
        &mut self,
        workspace_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.focused_workspace_id.as_deref() == Some(workspace_id.as_str()) {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        // TUI host mode: Project switching drives the shared focus chain (never respawns the host).
        self.drive_tui_focus_chain(Some(workspace_id.clone()), None, None, None, window, cx);
        self.show_settings = false;
        self.terminal_fade_start = Some(Instant::now());
        let previous_workspace = self.state.focused_workspace_id.clone();
        self.clear_ime_state();
        self.state.focused_workspace_id = Some(workspace_id.clone());
        self.new_agent_context_workspace_id = Some(workspace_id.clone());
        self.state.focused_tab_id = None;
        self.state.focused_pane_id = None;
        derive_selection_flags(&mut self.state);
        self.sync_terminal_application_focus(cx);
        self.notify_sidebar(cx);
        if previous_workspace.as_deref() != self.state.focused_workspace_id.as_deref() {
            self.sync_right_panel_for_project_context(cx);
        }
        cx.notify();
        let window_handle = window.window_handle();
        // F36: resolve the target Tab locally first — the event subscription continuously maintains
        // tabs/active_tab_id metadata, so the normal path doesn't pull a full navigation_state (saving
        // workspace.list+tab.list round trips); only when local has no Tabs for this Project at all
        // (early startup/after reconnect) does it fall back to the old Navigation pull.
        // F20: resolve the target Tab locally — client memory → Herdr active_tab_id → first local Tab.
        let local_tab_id = resolve_workspace_tab_locally(
            &self.state.workspaces,
            &self.state.tabs,
            &workspace_id,
            self.workspace_tab_selection_memory
                .get(&workspace_id)
                .map(String::as_str),
        );
        self.run_navigation_rpc(
            client,
            cx,
            Some(window_handle),
            true,
            move |client| {
                let fallback = |client: &dyn shardlane_host::mux::MultiplexerConnection| -> Result<
                    FocusNavigationProjection,
                    shardlane_host::mux::MuxError,
                > {
                    let navigation = client.navigation_state()?;
                    let (workspace_id, tab_id) =
                        resolve_navigation_selection(Some(&workspace_id), None, &navigation);
                    let surface = match (workspace_id.as_deref(), tab_id.as_deref()) {
                        (Some(workspace_id), Some(tab_id)) => {
                            Some(client.tab_surface_state(workspace_id, tab_id)?)
                        }
                        _ => None,
                    };
                    Ok(FocusNavigationProjection::Navigation {
                        navigation,
                        surface,
                    })
                };
                match local_tab_id {
                    Some(tab_id) => client
                        .tab_surface_state(&workspace_id, &tab_id)
                        .map(FocusNavigationProjection::Surface)
                        // Local Tab metadata lagging (closed remotely): fall back to full navigation resolution.
                        .or_else(|_| fallback(client)),
                    None => fallback(client),
                }
            },
            move |view, projection, cx| {
                match projection {
                    FocusNavigationProjection::Navigation {
                        navigation,
                        surface,
                    } => {
                        view.apply_navigation_state(navigation);
                        if let Some(surface) = surface {
                            view.apply_tab_surface_state(surface, cx);
                        } else {
                            // F25: when a Project has no usable surface, clear the workspace selection too;
                            // otherwise the same-id early return would stay permanently stuck on a blank
                            // surface (you'd have to visit another Project first).
                            view.state.focused_workspace_id = None;
                            view.state.focused_pane_id = None;
                            view.state.panes.clear();
                            view.state.layouts.clear();
                            derive_selection_flags(&mut view.state);
                            view.clear_terminal_surface(cx);
                        }
                    }
                    FocusNavigationProjection::Surface(surface) => {
                        view.apply_tab_surface_state(surface, cx);
                    }
                    FocusNavigationProjection::Full(state) => {
                        view.state = state;
                        // F34: after the whole-domain replacement, derive the selection flags uniformly.
                        derive_selection_flags(&mut view.state);
                        view.prune_steering_drafts();
                    }
                }
                view.reset_terminal_scroll_state();
                true
            },
        );
    }

    pub(super) fn focus_tab_id(
        &mut self,
        tab_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_agent_open = false;
        self.pending_close_tab = None;
        self.leave_history_surface(cx);
        self.exit_blocked_secondary_surfaces(cx);
        if self.state.focused_tab_id.as_deref() == Some(tab_id.as_str()) {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        let workspace_id = self
            .state
            .tabs
            .iter()
            .find(|tab| tab.tab_id == tab_id)
            .and_then(|tab| tab.workspace_id.clone())
            .or_else(|| self.state.focused_workspace_id.clone());
        // TUI host mode: Tab switching drives the shared focus chain (never respawns the host).
        self.drive_tui_focus_chain(
            workspace_id.clone(),
            Some(tab_id.clone()),
            None,
            None,
            window,
            cx,
        );
        self.show_settings = false;
        self.clear_ime_state();
        self.state.focused_tab_id = Some(tab_id.clone());
        self.state.focused_pane_id = None;
        // F20: record the client's local Tab selection memory; when switching back to a Project it takes priority over Herdr's active_tab_id.
        if let Some(workspace_id) = workspace_id.as_deref() {
            self.workspace_tab_selection_memory
                .insert(workspace_id.to_string(), tab_id.clone());
        }
        self.persist_current_workspace_state();
        derive_selection_flags(&mut self.state);
        self.sync_terminal_application_focus(cx);
        self.notify_sidebar(cx);
        cx.notify();
        let window_handle = window.window_handle();
        self.run_navigation_rpc(
            client,
            cx,
            Some(window_handle),
            true,
            // No more tab.focus sends: Tab selection is client-local state, and surfaces pull by the
            // locally resolved (workspace, tab); unknown Tabs fall back to visible_state as an exception recovery.
            move |client| {
                if let Some(workspace_id) = workspace_id.as_deref() {
                    client
                        .tab_surface_state(workspace_id, &tab_id)
                        .map(FocusNavigationProjection::Surface)
                } else {
                    client.visible_state().map(FocusNavigationProjection::Full)
                }
            },
            move |view, projection, cx| {
                match projection {
                    FocusNavigationProjection::Surface(surface) => {
                        view.apply_tab_surface_state(surface, cx);
                    }
                    FocusNavigationProjection::Navigation { .. } => {
                        // Unreachable from this projection closure; Navigation is produced only by
                        // the workspace-level resolvers that need a full navigation_state pull.
                    }
                    FocusNavigationProjection::Full(state) => {
                        view.state = state;
                        // F34: after the whole-domain replacement, derive the selection flags uniformly.
                        derive_selection_flags(&mut view.state);
                        view.prune_steering_drafts();
                    }
                }
                view.reset_terminal_scroll_state();
                true
            },
        );
    }

    pub(super) fn focus_tab_offset(
        &mut self,
        offset: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tabs = self.visible_tabs();
        if tabs.is_empty() {
            return;
        }
        let active_id = self
            .state
            .focused_tab_id
            .as_deref()
            .or_else(|| self.active_tab().map(|tab| tab.tab_id.as_str()));
        let active_index = active_id
            .and_then(|id| tabs.iter().position(|tab| tab.tab_id == id))
            .unwrap_or(0);
        let next_index = (active_index as isize + offset).rem_euclid(tabs.len() as isize) as usize;
        self.focus_tab_id(tabs[next_index].tab_id.clone(), window, cx);
    }

    pub(super) fn focus_project_offset(
        &mut self,
        offset: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.workspaces.is_empty() {
            return;
        }
        let active_id = self.state.focused_workspace_id.as_deref().or_else(|| {
            self.active_workspace()
                .map(|workspace| workspace.workspace_id.as_str())
        });
        let active_index = active_id
            .and_then(|id| {
                self.state
                    .workspaces
                    .iter()
                    .position(|workspace| workspace.workspace_id == id)
            })
            .unwrap_or(0);
        let next_index = (active_index as isize + offset)
            .rem_euclid(self.state.workspaces.len() as isize) as usize;
        self.focus_workspace_id(
            self.state.workspaces[next_index].workspace_id.clone(),
            window,
            cx,
        );
    }

    /// Pane selection is client-local navigation state: it sends no focus to Herdr and never rewrites
    /// layout.focused_pane_id (that's Herdr's zoom runtime fact driving the zoom rendering).
    /// F2/F21: when the current Tab is zoomed and a non-zoom-target pane is selected, the selection
    /// semantics become "switch the view target" — aligned via Herdr's `pane.zoom` structural command
    /// (probe verified: with A zoomed, toggling B exits zoom and focuses B), and the reply's
    /// apply_current_layout keeps the local selection so rendering/input/controllers all converge on the
    /// authoritative layout, eliminating "looking at A while typing into B" and B's controller-less
    /// degraded input.
    pub(super) fn focus_pane_id(
        &mut self,
        pane_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.exit_blocked_secondary_surfaces(cx);
        if self.state.focused_pane_id.as_deref() == Some(pane_id.as_str()) {
            // Re-focus of the same pane: steering state still needs its idempotent guarantees (first creation/index rescan).
            self.ensure_steering(window, cx);
            return;
        }

        // P2-8 (audit 2026-08-28): plain focus only focuses. The old Embedded-era implicit layout
        // mutation of "auto toggle_pane_zoom on zoom-target mismatch" was deleted;
        // zoom now changes only via the user's explicit Toggle Zoom action (plan §12.4 / TUI-05).
        let selected_tab_id = self
            .state
            .panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .and_then(|pane| pane.tab_id.clone())
            .or_else(|| self.state.focused_tab_id.clone());

        self.reset_terminal_scroll_state();
        self.clear_ime_state();
        self.state.focused_pane_id = Some(pane_id.clone());
        derive_selection_flags(&mut self.state);
        self.sync_terminal_application_focus(cx);
        self.notify_sidebar(cx);
        cx.notify();

        // TUI host mode: Pane selection drives the shared focus chain (pane.focus; agent.focus direct
        // when an agent is hit), the host TUI follows, never respawning and never using zoom workarounds.
        let tui_workspace_id = selected_tab_id.as_deref().and_then(|tab_id| {
            self.state
                .tabs
                .iter()
                .find(|tab| tab.tab_id == tab_id)
                .and_then(|tab| tab.workspace_id.clone())
        });
        self.drive_tui_focus_chain(
            tui_workspace_id,
            selected_tab_id.clone(),
            Some(pane_id.clone()),
            None,
            window,
            cx,
        );
    }

    /// Unified scaffolding for navigation socket RPCs: token invalidation protection, loading lifecycle,
    /// status governance, and optional attach follow-up. The projection runs on the background executor
    /// (never the UI thread); apply runs on the UI thread only while the token is still valid and returns
    /// whether an attach is needed.
    /// The Err arm uniformly does status + notify_sidebar + notify (conservatively one extra sidebar refresh);
    /// `attach_on_error` preserves the focus family's status quo of "attaching the optimistic selection even after an error".
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_navigation_rpc<P, Projection, Apply>(
        &mut self,
        client: std::sync::Arc<dyn shardlane_host::mux::MultiplexerConnection>,
        cx: &mut Context<Self>,
        attach_window: Option<AnyWindowHandle>,
        attach_on_error: bool,
        projection: Projection,
        apply: Apply,
    ) where
        P: Send + 'static,
        Projection: FnOnce(
                &dyn shardlane_host::mux::MultiplexerConnection,
            ) -> Result<P, shardlane_host::mux::MuxError>
            + Send
            + 'static,
        Apply: FnOnce(&mut Self, P, &mut Context<Self>) -> bool + 'static,
    {
        self.navigation_token = self.navigation_token.wrapping_add(1);
        let token = self.navigation_token;
        self.navigation_loading = true;
        // The handle is stored in a field instead of detaching: when a new navigation replaces an old
        // task, drop cancels it immediately, so superseded socket round trips no longer run to no effect (GPUI BackgroundJob drop = cancel).
        self._navigation_script = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { projection(client.as_ref()) })
                .await;
            let applied = this
                .update(cx, |view, cx| {
                    if view.navigation_token != token {
                        return false;
                    }
                    view.navigation_loading = false;
                    let attach = match result {
                        Ok(projection) => {
                            let attach = apply(view, projection, cx);
                            view.status = ConnectionStatus::Connected;
                            view.notify_sidebar(cx);
                            cx.notify();
                            attach
                        }
                        Err(err) => {
                            view.status = ConnectionStatus::Offline(err.to_string());
                            view.notify_sidebar(cx);
                            cx.notify();
                            attach_on_error
                        }
                    };
                    attach_window.is_some() && attach
                })
                .unwrap_or(false);
            if let (true, Some(window)) = (applied, attach_window) {
                let _ = cx.update_window(window, |_, window, cx| {
                    let _ = this.update(cx, |view, cx| view.attach_focused_terminal(window, cx));
                });
            }
        });
    }

    pub(super) fn focus_target(
        &mut self,
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_target_inner(workspace_id, tab_id, pane_id, None, window, cx);
    }

    /// FocusIntent::Agent executor (audit P2-6): terminal_id is the stable primary identity, passed
    /// explicitly to the focus chain so protocol-20 `agent.focus` goes direct; the local ShellSelection
    /// still lands on the pane (or Tab/Project when absent), keeping the sidebar highlight semantics unchanged.
    pub(super) fn focus_agent_target(
        &mut self,
        terminal_id: String,
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_target_inner(workspace_id, tab_id, pane_id, Some(terminal_id), window, cx);
    }

    fn focus_target_inner(
        &mut self,
        workspace_id: Option<String>,
        tab_id: Option<String>,
        pane_id: Option<String>,
        agent_terminal_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_agent_open = false;
        // Audits A02/A13: every search/notification/status/sidebar jump lands here, so the
        // secondary-surface exits (Settings/Help, with their terminal focus sync) and the
        // armed two-click Tab close must reset on the shared path — an armed close surviving
        // a jump would close a Tab on the next single click. This replaces the old ad-hoc
        // `show_settings = false`, which skipped the companion focus sync.
        self.pending_close_tab = None;
        self.exit_blocked_secondary_surfaces(cx);
        self.leave_history_surface(cx);
        let Some(client) = self.client.clone() else {
            return;
        };
        // Audit A12: an Agent intent whose pane_id is absent still carries attribution on its
        // runtime row (terminal_id is the stable identity, audit P2-6); without this fallback
        // the resolution chain below would see all-empty ids and silently no-op the jump.
        let agent_attribution = agent_terminal_id.as_deref().and_then(|terminal_id| {
            self.state
                .agents
                .iter()
                .find(|agent| agent.terminal_id == terminal_id)
                .map(|agent| {
                    (
                        agent.pane_id.clone(),
                        agent.tab_id.clone(),
                        agent.workspace_id.clone(),
                    )
                })
        });
        let pane_id = pane_id.or_else(|| {
            agent_attribution
                .as_ref()
                .and_then(|(pane, _, _)| pane.clone())
        });
        let tab_id = tab_id.or_else(|| {
            agent_attribution
                .as_ref()
                .and_then(|(_, tab, _)| tab.clone())
        });
        let workspace_id = workspace_id.or_else(|| {
            agent_attribution
                .as_ref()
                .and_then(|(_, _, workspace)| workspace.clone())
        });
        let tab_id = tab_id.or_else(|| {
            pane_id.as_deref().and_then(|pane_id| {
                self.state
                    .panes
                    .iter()
                    .find(|pane| pane.pane_id == pane_id)
                    .and_then(|pane| pane.tab_id.clone())
                    .or_else(|| {
                        self.state
                            .agents
                            .iter()
                            .find(|agent| agent.pane_id.as_deref() == Some(pane_id))
                            .and_then(|agent| agent.tab_id.clone())
                    })
            })
        });
        let workspace_id = workspace_id
            .or_else(|| {
                tab_id.as_deref().and_then(|tab_id| {
                    self.state
                        .tabs
                        .iter()
                        .find(|tab| tab.tab_id == tab_id)
                        .and_then(|tab| tab.workspace_id.clone())
                })
            })
            .or_else(|| {
                pane_id.as_deref().and_then(|pane_id| {
                    self.state
                        .panes
                        .iter()
                        .find(|pane| pane.pane_id == pane_id)
                        .and_then(|pane| pane.workspace_id.clone())
                        .or_else(|| {
                            self.state
                                .agents
                                .iter()
                                .find(|agent| agent.pane_id.as_deref() == Some(pane_id))
                                .and_then(|agent| agent.workspace_id.clone())
                        })
                })
            });
        if workspace_id.is_none() && tab_id.is_none() && pane_id.is_none() {
            return;
        }

        // TUI host mode: navigation drives the shared focus chain in sync (workspace.focus → tab.focus →
        // pane.focus / agent.focus); the host TUI follows server state refreshes and is never respawned.
        self.drive_tui_focus_chain(
            workspace_id.clone(),
            tab_id.clone(),
            pane_id.clone(),
            agent_terminal_id,
            window,
            cx,
        );

        if let Some(workspace_id) = workspace_id.as_deref() {
            self.state.focused_workspace_id = Some(workspace_id.to_string());
        }
        if let Some(tab_id) = tab_id.as_deref() {
            self.state.focused_tab_id = Some(tab_id.to_string());
            if let Some(workspace_id) = workspace_id.as_deref() {
                self.workspace_tab_selection_memory
                    .insert(workspace_id.to_string(), tab_id.to_string());
            }
        }
        if let Some(pane_id) = pane_id.as_deref() {
            self.state.focused_pane_id = Some(pane_id.to_string());
        }
        derive_selection_flags(&mut self.state);
        self.sync_terminal_application_focus(cx);
        self.notify_sidebar(cx);
        cx.notify();
        let window_handle = window.window_handle();
        self.run_navigation_rpc(
            client,
            cx,
            Some(window_handle),
            true,
            // Navigation doesn't sync Herdr: no workspace/tab/pane focus sent; surfaces pull by the locally resolved target.
            move |client| {
                if let (Some(workspace_id), Some(tab_id)) =
                    (workspace_id.as_deref(), tab_id.as_deref())
                {
                    return client
                        .tab_surface_state(workspace_id, tab_id)
                        .map(FocusNavigationProjection::Surface);
                }
                if workspace_id.is_some() {
                    let navigation = client.navigation_state()?;
                    let (workspace_id, tab_id) =
                        resolve_navigation_selection(workspace_id.as_deref(), None, &navigation);
                    let surface = match (workspace_id.as_deref(), tab_id.as_deref()) {
                        (Some(workspace_id), Some(tab_id)) => {
                            Some(client.tab_surface_state(workspace_id, tab_id)?)
                        }
                        _ => None,
                    };
                    return Ok(FocusNavigationProjection::Navigation {
                        navigation,
                        surface,
                    });
                }
                client.visible_state().map(FocusNavigationProjection::Full)
            },
            move |view, projection, cx| {
                match projection {
                    FocusNavigationProjection::Surface(surface) => {
                        view.apply_tab_surface_state(surface, cx);
                    }
                    FocusNavigationProjection::Navigation {
                        navigation,
                        surface,
                    } => {
                        view.apply_navigation_state(navigation);
                        if let Some(surface) = surface {
                            view.apply_tab_surface_state(surface, cx);
                        } else {
                            view.state.focused_pane_id = None;
                            view.state.panes.clear();
                            view.state.layouts.clear();
                            view.clear_terminal_surface(cx);
                        }
                    }
                    FocusNavigationProjection::Full(state) => {
                        view.state = state;
                        // F34: after the whole-domain replacement, derive the selection flags uniformly.
                        derive_selection_flags(&mut view.state);
                        view.prune_steering_drafts();
                    }
                }
                view.reset_terminal_scroll_state();
                true
            },
        );
    }

    pub(super) fn apply_agent_status_patch(&mut self, patch: &AgentStatusPatch) -> Option<bool> {
        // P2-2 closure: the Settings toggle is the outer gate and window unfocus the inner gate (aligned
        // with the BEL path's handle_terminal_bells window_active semantics — when the user is watching
        // the app, status transitions are in-band info and shouldn't disturb).
        let notifications_enabled = self.config.behavior.agent_notifications && !self.window_active;
        let (changed, notification) = {
            let agent = self
                .state
                .agents
                .iter_mut()
                .find(|agent| agent.pane_id.as_deref() == Some(patch.pane_id.as_str()))?;
            let previous_status = agent.agent_status.clone();
            let mut changed = apply_agent_projection_patch(agent, patch);
            let notification = notifications_enabled
                .then(|| {
                    agent_notification_kind(
                        previous_status.as_deref(),
                        agent.agent_status.as_deref(),
                    )
                    .map(|kind| {
                        // Any notifiable transition while unfocused requests dock attention
                        //(AppKit coalesces on its own), consistent with the BEL path.
                        notifications::request_dock_attention();
                        (agent_notification_copy(agent, kind), agent.pane_id.clone())
                    })
                })
                .flatten();
            if let Some(pane) = self
                .state
                .panes
                .iter_mut()
                .find(|pane| pane.pane_id == patch.pane_id)
            {
                changed |= apply_pane_agent_projection_patch(pane, patch);
            }
            (changed, notification)
        };
        if let Some(((title, body), pane_id)) = notification {
            // With pane_id: clicking the notification jumps to the pane (the full P2-2 loop).
            if let Some(pane_id) = pane_id {
                notifications::show_with_pane(&title, &body, &pane_id);
            } else {
                notifications::show(&title, &body);
            }
        }
        Some(changed)
    }

    pub(super) fn apply_navigation_state(&mut self, mut navigation: NavigationState) -> bool {
        // TUI-only: the host TUI's runtime focus is the authority (the sidebar is a projection of
        // the same `herdr` client), so the incoming projection's focused ids are accepted as-is;
        // keeping an older local selection here would desynchronize left-side highlighting from
        // the TUI tab/pane. (The retired `resolve_navigation_selection_for_surface` wrapper only
        // cloned these two fields back onto themselves.)
        if let (Some(workspace_id), Some(tab_id)) = (
            navigation.focused_workspace_id.as_deref(),
            navigation.focused_tab_id.as_deref(),
        ) {
            for workspace in &mut navigation.workspaces {
                if workspace.workspace_id == workspace_id {
                    workspace.active_tab_id = Some(tab_id.to_string());
                }
            }
        }
        // F34: derive the selection flags uniformly on the incoming projection before comparing, so server-side focus facts don't create false diffs.
        derive_navigation_selection_flags(&mut navigation);

        let changed = self.state.focused_workspace_id != navigation.focused_workspace_id
            || self.state.focused_tab_id != navigation.focused_tab_id
            || self.state.workspaces != navigation.workspaces
            || self.state.tabs != navigation.tabs;
        if changed {
            self.state.focused_workspace_id = navigation.focused_workspace_id;
            self.state.focused_tab_id = navigation.focused_tab_id;
            self.state.workspaces = navigation.workspaces;
            self.state.tabs = navigation.tabs;
            // F20: the memory table is pruned with navigation reconcile, reclaiming stale entries (closed Projects).
            self.workspace_tab_selection_memory
                .retain(|workspace_id, _| {
                    self.state
                        .workspaces
                        .iter()
                        .any(|workspace| workspace.workspace_id == *workspace_id)
                });
            // The pane universe is pruned with reconcile: steering drafts of closed panes are reclaimed in step
            //(same cause and same mechanism as the F4/F20 memory table pruning).
            self.prune_steering_drafts();
        }
        changed
    }

    pub(super) fn apply_tab_surface_state(
        &mut self,
        surface: TabSurfaceState,
        cx: &mut Context<Self>,
    ) {
        let previous_workspace = self.state.focused_workspace_id.clone();
        let tui_area = surface.layouts.first().map(|layout| layout.area);
        // TUI-only: the sidebar faithfully mirrors the host TUI's runtime focus (Tab/Pane switches
        // inside the TUI also reverse-sync to the left-side highlight via events).
        let focused_pane_id = surface.focused_pane_id.clone();
        // surface.layouts's focused_pane_id keeps Herdr's runtime fact (zoom rendering depends on it)
        // and is not rewritten to the client's selection; leaf pane flags derive uniformly via derive_selection_flags.
        self.state.focused_workspace_id = Some(surface.workspace_id.clone());
        self.state.focused_tab_id = Some(surface.tab_id.clone());
        self.state.focused_pane_id = focused_pane_id;
        for workspace in &mut self.state.workspaces {
            if workspace.workspace_id == surface.workspace_id {
                workspace.active_tab_id = Some(surface.tab_id.clone());
            }
        }
        self.state.panes = surface.panes;
        self.state.layouts = surface.layouts;
        if let Some(area) = tui_area {
            self.apply_tui_chrome_area(area, false, cx);
        }
        // Applying a surface prunes drafts immediately: closing a pane inside the focused tab doesn't wait for reconcile.
        self.prune_steering_drafts();
        derive_selection_flags(&mut self.state);
        self.sync_pane_event_subscription(cx);
        // Tab/project switching: with no sidebar context the right panel follows runtime focus; with context, only the current context's path refreshes.
        if previous_workspace.as_deref() != self.state.focused_workspace_id.as_deref() {
            if self.new_agent_context_workspace_id.is_none() {
                self.sync_right_panel_for_project_context(cx);
            } else if self.right_panel.open {
                self.refresh_right_panel_state(cx);
            }
        }
        // F35: the orphan sweep (a full ProjectIndex rebuild) doesn't run on every Tab/Pane switch;
        // it is instead triggered by the event-driven navigation reconcile (see start_event_subscription).
    }

    /// Apply an authoritative layout update (structural command reply / layout.updated event): geometry
    /// and zoom facts are accepted as-is, but selection belongs to the client — the local focused_pane_id
    /// is preserved when it still exists, falling back to Herdr's focused_pane_id only when the local
    /// selection is no longer in the new layout.
    pub(super) fn apply_current_layout(&mut self, layout: PaneLayout, cx: &mut Context<Self>) {
        let tui_area = layout.area;
        // TUI-only: the host TUI's runtime focus IS the sidebar selection.
        self.state.focused_pane_id = layout.focused_pane_id.clone();
        self.state.layouts.clear();
        self.state.layouts.push(layout);
        self.apply_tui_chrome_area(tui_area, false, cx);
        derive_selection_flags(&mut self.state);
        self.sync_terminal_application_focus(cx);
        self.notify_sidebar(cx);
        cx.notify();
    }

    pub(super) fn apply_created_tab(
        &mut self,
        mut created: TabCreatedResult,
        layout: PaneLayout,
        fallback_workspace_id: Option<String>,
        adjust_workspace_counts: bool,
    ) {
        let workspace_id = created
            .tab
            .workspace_id
            .clone()
            .or(fallback_workspace_id)
            .unwrap_or_else(|| layout.workspace_id.clone().unwrap_or_default());
        created.tab.workspace_id = Some(workspace_id.clone());
        created.root_pane.workspace_id = Some(workspace_id.clone());
        created.root_pane.tab_id = Some(created.tab.tab_id.clone());

        let tab_already_projected = self
            .state
            .tabs
            .iter()
            .any(|tab| tab.tab_id == created.tab.tab_id);
        if let Some(existing) = self
            .state
            .tabs
            .iter_mut()
            .find(|tab| tab.tab_id == created.tab.tab_id)
        {
            *existing = created.tab.clone();
        } else {
            self.state.tabs.push(created.tab.clone());
        }
        for workspace in &mut self.state.workspaces {
            if workspace.workspace_id == workspace_id {
                workspace.active_tab_id = Some(created.tab.tab_id.clone());
                if adjust_workspace_counts && !tab_already_projected {
                    workspace.tab_count = workspace.tab_count.map(|count| count.saturating_add(1));
                    workspace.pane_count =
                        workspace.pane_count.map(|count| count.saturating_add(1));
                }
            }
        }
        self.state.focused_workspace_id = Some(workspace_id);
        self.state.focused_tab_id = Some(created.tab.tab_id);
        self.state.focused_pane_id = Some(created.root_pane.pane_id.clone());
        self.state.panes = vec![created.root_pane];
        self.state.layouts = vec![layout];
        derive_selection_flags(&mut self.state);
    }

    pub(super) fn notify_sidebar(&self, cx: &mut Context<Self>) {
        self.sidebar_pane.update(cx, |_, cx| cx.notify());
        self.notify_status_bar();
    }

    pub(super) fn status_bar_snapshot(&self) -> status_bar::StatusBarSnapshot {
        let summary = operational_summary(&self.state.agents, &self.scripts);
        let connected = self.status.is_connected();
        let active_ws_id = self.active_workspace_id();

        let agents = self
            .state
            .agents
            .iter()
            .map(|agent| {
                let identity = sidebar::agent_identity(agent).unwrap_or("Agent");
                let project_name = agent
                    .foreground_cwd
                    .as_deref()
                    .or(agent.cwd.as_deref())
                    .and_then(|cwd| std::path::Path::new(cwd).file_name())
                    .and_then(|name| name.to_str())
                    .unwrap_or(agent.workspace_id.as_deref().unwrap_or("Project"))
                    .to_string();

                // Audit A01: the menu grouping derives from the same single runtime→product
                // mapping as operational_summary (failed → NeedsAttention, starting/running →
                // Working); the retired raw `== "blocked"` / `== "working"` comparisons drifted
                // from the summary counts.
                let attention = crate::status::agent_effective_status(agent)
                    .map(crate::status::attention_for_raw_status)
                    .unwrap_or(crate::status::AttentionLevel::Idle);

                status_bar::StatusBarAgentItem {
                    name: identity.to_string(),
                    project_name,
                    status: attention.label().to_string(),
                    is_blocked: attention == crate::status::AttentionLevel::NeedsAttention,
                    is_working: attention == crate::status::AttentionLevel::Working,
                    terminal_id: agent.terminal_id.clone(),
                    workspace_id: agent.workspace_id.clone(),
                    tab_id: agent.tab_id.clone(),
                    pane_id: agent.pane_id.clone(),
                }
            })
            .collect();

        let projects = self
            .state
            .workspaces
            .iter()
            .map(|ws| {
                let name = ws
                    .label
                    .as_deref()
                    .or(ws.cwd.as_deref())
                    .and_then(|path| std::path::Path::new(path).file_name())
                    .and_then(|name| name.to_str())
                    .unwrap_or(&ws.workspace_id)
                    .to_string();
                let is_active = active_ws_id == Some(&ws.workspace_id);
                status_bar::StatusBarProjectItem {
                    workspace_id: ws.workspace_id.clone(),
                    name,
                    is_active,
                }
            })
            .collect();

        status_bar::StatusBarSnapshot {
            connected,
            working_agents_count: summary.working_agents,
            blocked_agents_count: summary.blocked_agents,
            active_scripts_count: summary.active_scripts,
            failed_scripts_count: summary.failed_scripts,
            projects,
            agents,
        }
    }

    pub(super) fn notify_status_bar(&self) {
        if let Some(status_bar) = self.shared.status_bar.0.as_ref() {
            status_bar.update(&self.status_bar_snapshot());
        }
    }

    pub(super) fn handle_notification_action(
        &mut self,
        action: notifications::NotificationAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            // P2-2 click-to-jump: activation + landing focus both reuse StatusBar FocusTarget semantics
            //(pane → tab → workspace resolved level by level, see focus_target).
            notifications::NotificationAction::FocusPane(pane_id) => {
                self.handle_status_bar_action(
                    status_bar::StatusBarAction::FocusTarget {
                        workspace_id: None,
                        tab_id: None,
                        pane_id: Some(pane_id),
                    },
                    window,
                    cx,
                );
            }
        }
    }

    pub(super) fn handle_status_bar_action(
        &mut self,
        action: status_bar::StatusBarAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(target_os = "macos")]
        {
            if let Some(mtm) = objc2::MainThreadMarker::new() {
                objc2_app_kit::NSApplication::sharedApplication(mtm).activate();
            }
        }
        window.activate_window();
        cx.activate(true);

        match action {
            status_bar::StatusBarAction::FocusTarget {
                workspace_id,
                tab_id,
                pane_id,
            } => {
                // FocusIntent seam: status/menu jumps share the Sidebar/Search/History landing;
                // partial attribution degrades to the most specific target, and all-empty means nothing to focus.
                if let Some(intent) = FocusIntent::from_targets(workspace_id, tab_id, pane_id) {
                    self.apply_focus_intent(intent, window, cx);
                }
            }
            status_bar::StatusBarAction::FocusAgent {
                terminal_id,
                workspace_id,
                tab_id,
                pane_id,
            } => {
                // Audit A12: agent jumps keep the stable terminal_id even when the runtime row
                // has no pane yet, so the click always lands on the agent-focused chain instead
                // of degrading into a silent no-op.
                self.apply_focus_intent(
                    FocusIntent::agent(terminal_id, workspace_id, tab_id, pane_id),
                    window,
                    cx,
                );
            }
            status_bar::StatusBarAction::OpenNewAgent => {
                self.open_fresh_new_agent_surface(window, cx);
            }
            status_bar::StatusBarAction::OpenHistory => {
                self.toggle_history(&OpenHistory, window, cx);
            }
            status_bar::StatusBarAction::OpenSettings => {
                self.show_settings = true;
                self.new_agent_open = false;
                self.leave_history_surface(cx);
                cx.notify();
            }
            status_bar::StatusBarAction::OpenSearch => {
                self.open_search(&OpenSearch, window, cx);
            }
            status_bar::StatusBarAction::ShowMainWindow => {
                // Window activated and focused above
            }
            status_bar::StatusBarAction::Quit => {
                cx.quit();
            }
        }
    }

    pub(crate) fn find_sidebar_git_status(
        &self,
        path: &str,
    ) -> Option<&git_status::GitStatusSnapshot> {
        let trimmed = path.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return None;
        }
        if let Some(snapshot) = self.sidebar_git_status.get(trimmed) {
            return Some(snapshot);
        }
        if let Some(snapshot) = self.sidebar_git_status.get(path) {
            return Some(snapshot);
        }
        self.sidebar_git_status.values().find(|snapshot| {
            let snap_path = snapshot.path.trim_end_matches('/');
            trimmed == snap_path
                || (trimmed.starts_with(snap_path)
                    && trimmed.as_bytes().get(snap_path.len()) == Some(&b'/'))
        })
    }

    /// Git snapshot refresh for the active project (Header pill) and every
    /// visible Sidebar Project: skip while fresh, otherwise collect all stale
    /// paths in ONE sequential background task and write back per path (safe
    /// to call every frame; bounded by the instance's runtime workspace count,
    /// the 12s freshness window, and the in-flight guard — never a per-frame
    /// git subprocess storm).
    pub(super) fn refresh_git_status(&self, cx: &mut Context<Self>) {
        let active_path = self
            .active_workspace()
            .and_then(|workspace| workspace.cwd.clone())
            .unwrap_or_default();
        let mut candidates: Vec<String> = self
            .state
            .workspaces
            .iter()
            .filter_map(|workspace| workspace.cwd.clone())
            .collect();
        for pane in &self.state.panes {
            if let Some(cwd) = pane.cwd.as_deref() {
                if !cwd.is_empty() {
                    candidates.push(cwd.to_string());
                }
            }
        }
        for panes in self.panes_by_project.values() {
            for pane in panes {
                if let Some(cwd) = pane.cwd.as_deref() {
                    if !cwd.is_empty() {
                        candidates.push(cwd.to_string());
                    }
                }
            }
        }
        for agent in &self.state.agents {
            if let Some(cwd) = agent.foreground_cwd.as_deref().or(agent.cwd.as_deref()) {
                if !cwd.is_empty() {
                    candidates.push(cwd.to_string());
                }
            }
        }
        let project_index = crate::workspace_model::build_project_index(&self.state, &self.scripts);
        let visible_projects = crate::workspace_model::visible_sidebar_projects_with_index(
            &self.state,
            &project_index,
        );
        for project in visible_projects {
            if let Some(path) = project.project_path {
                if !path.is_empty() {
                    candidates.push(path);
                }
            }
        }
        if !active_path.is_empty() {
            candidates.push(active_path.clone());
        }
        candidates.sort();
        candidates.dedup();
        let is_fresh = |snapshot: Option<&git_status::GitStatusSnapshot>, path: &str| {
            snapshot.is_some_and(|snapshot| snapshot.is_fresh_for(path))
        };
        let mut stale: Vec<String> = Vec::new();
        for path in candidates {
            if path.is_empty() {
                continue;
            }
            let fresh = if path == active_path {
                is_fresh(self.git_status.as_ref(), &path)
                    || is_fresh(self.sidebar_git_status.get(&path), &path)
            } else {
                is_fresh(self.sidebar_git_status.get(&path), &path)
            };
            let mut inflight = self
                .git_inflight
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if fresh || inflight.contains(&path) {
                continue;
            }
            inflight.insert(path.clone());
            stale.push(path);
        }
        if stale.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            // Sequential collection: one background worker for the whole
            // batch keeps the git subprocess count at one-at-a-time.
            let snapshots: Vec<_> = cx
                .background_executor()
                .spawn(async move {
                    stale
                        .iter()
                        .map(|path| (path.clone(), git_status::git_status_snapshot(path)))
                        .collect()
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                let mut changed = false;
                for (path, snapshot_opt) in snapshots {
                    view.git_inflight
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .remove(&path);
                    if let Some(snapshot) = snapshot_opt {
                        view.sidebar_git_status
                            .insert(path.clone(), snapshot.clone());
                        if Some(path.as_str())
                            == view
                                .active_workspace()
                                .and_then(|workspace| workspace.cwd.as_deref())
                        {
                            view.git_status = Some(snapshot);
                        }
                        changed = true;
                    }
                }
                if changed {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
