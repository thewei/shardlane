//! [INPUT]: Depends on the ShardlaneApp type from the crate root (super) and existing types/imports (use super::*); no independent external dependencies.
//! [OUTPUT]: Exposes ShardlaneApp's pane operation surface: the run_pane_rpc shared scaffold (B27), split/zoom/resize/swap/close/move, process info, agent claiming, and the runtime-ID clipboard helper (copy_runtime_id); includes the focus/resize direction-key macro family (inherent impl shard).
//! [POS]: The `crates/herdr-gui` shell panes responsibility domain, mechanically split out of main.rs; together with sibling shell_* modules it forms ShardlaneApp's method surface.
use super::*;

/// Focus direction keys (⌥⌘arrows etc.): the only difference between action types is the
/// direction literal; the macro family eliminates four copies of identical boilerplate.
macro_rules! focus_direction {
    ($name:ident, $action:ty, $dir:literal) => {
        pub(super) fn $name(&mut self, _: &$action, window: &mut Window, cx: &mut Context<Self>) {
            let Some(pane_id) = self.focused_pane().map(|pane| pane.pane_id.clone()) else {
                return;
            };
            self.focus_pane_direction_by_id(pane_id, $dir, window, cx);
        }
    };
}

/// Resize direction keys: same as above, but without needing a window.
macro_rules! resize_direction {
    ($name:ident, $action:ty, $dir:literal) => {
        pub(super) fn $name(&mut self, _: &$action, _window: &mut Window, cx: &mut Context<Self>) {
            let Some(pane_id) = self.focused_pane().map(|pane| pane.pane_id.clone()) else {
                return;
            };
            self.resize_pane_direction_by_id(pane_id, $dir, cx);
        }
    };
}

impl ShardlaneApp {
    /// Shared scaffolding for the pane-operation RPC family (B27: split / zoom / resize /
    /// swap / close / agent claim-deny / process info). Owns the clone-client → background
    /// projection → UI-thread apply sequence and the connection-status surface so the seven
    /// former hand-rolled copies cannot drift again (the move family keeps its richer
    /// `run_navigation_rpc` sibling).
    ///
    /// Notification policy (the one deliberate behavior normalization): the SUCCESS path
    /// always ends with `ConnectionStatus::Connected` + `notify_sidebar` + root `cx.notify()`
    /// — the explicit-notify majority pattern (split already did it directly; zoom and
    /// resize/swap inherited it from `apply_current_layout`; the move family from
    /// `run_navigation_rpc`). Close, agent claim/deny, and process info previously relied on
    /// incidental notifications from their result handlers and now behave identically to the
    /// rest of the family. The FAILURE path is unchanged everywhere: `ConnectionStatus::Offline`
    /// + `cx.notify()` (the sidebar stays event/reconcile-driven on errors).
    ///
    /// Unlike `run_navigation_rpc` there is deliberately no navigation token/loading lifecycle
    /// and no attach follow-up: pane operations must neither block the terminal surface
    /// (`navigation_loading`) nor cancel superseded operations — each completes independently.
    /// `window` (when supplied) wraps the apply phase in the window context so `apply` can
    /// open dialogs with a real `&mut Window`; all other callers pass `None`.
    pub(super) fn run_pane_rpc<P, Projection, Apply>(
        &mut self,
        cx: &mut Context<Self>,
        window: Option<AnyWindowHandle>,
        projection: Projection,
        apply: Apply,
    ) where
        P: Send + 'static,
        Projection: FnOnce(
                &dyn shardlane_host::mux::MultiplexerConnection,
            ) -> Result<P, shardlane_host::mux::MuxError>
            + Send
            + 'static,
        Apply: FnOnce(&mut Self, P, Option<&mut Window>, &mut Context<Self>) + 'static,
    {
        let Some(client) = self.client.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { projection(client.as_ref()) })
                .await;
            let finish = move |view: &mut Self,
                               result: Result<P, shardlane_host::mux::MuxError>,
                               window: Option<&mut Window>,
                               cx: &mut Context<Self>| {
                match result {
                    Ok(value) => {
                        apply(view, value, window, cx);
                        view.status = ConnectionStatus::Connected;
                        view.notify_sidebar(cx);
                        cx.notify();
                    }
                    Err(err) => {
                        view.status = ConnectionStatus::Offline(err.to_string());
                        cx.notify();
                    }
                }
            };
            match window {
                Some(window_handle) => {
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        let _ = this.update(cx, |view, cx| finish(view, result, Some(window), cx));
                    });
                }
                None => {
                    let _ = this.update(cx, |view, cx| finish(view, result, None, cx));
                }
            }
        })
        .detach();
    }

    focus_direction!(focus_left, FocusLeft, "left");
    focus_direction!(focus_right, FocusRight, "right");
    focus_direction!(focus_up, FocusUp, "up");
    focus_direction!(focus_down, FocusDown, "down");
    resize_direction!(resize_left, ResizeLeft, "left");
    resize_direction!(resize_right, ResizeRight, "right");
    resize_direction!(resize_up, ResizeUp, "up");
    resize_direction!(resize_down, ResizeDown, "down");
    pub(super) fn run_pane_split_by_id(
        &mut self,
        pane_id: String,
        operation: fn(
            &dyn shardlane_host::mux::MultiplexerConnection,
            &str,
        ) -> Result<Pane, shardlane_host::mux::MuxError>,
        cx: &mut Context<Self>,
    ) {
        self.run_pane_rpc(
            cx,
            None,
            move |client| {
                let created = operation(client, &pane_id)?;
                let workspace_id = created.workspace_id.clone().ok_or_else(|| {
                    shardlane_host::mux::MuxError::Api(
                        "pane.split response missing required workspace_id".to_string(),
                    )
                })?;
                let tab_id = created.tab_id.clone().ok_or_else(|| {
                    shardlane_host::mux::MuxError::Api(
                        "pane.split response missing required tab_id".to_string(),
                    )
                })?;
                client.tab_surface_state(&workspace_id, &tab_id)
            },
            |view, surface, _, cx| {
                view.apply_tab_surface_state(surface, cx);
            },
        );
    }

    pub(super) fn split_pane_right_by_id(&mut self, pane_id: String, cx: &mut Context<Self>) {
        self.run_pane_split_by_id(pane_id, mux_split_right, cx);
    }

    pub(super) fn split_pane_down_by_id(&mut self, pane_id: String, cx: &mut Context<Self>) {
        self.run_pane_split_by_id(pane_id, mux_split_down, cx);
    }

    pub(super) fn toggle_pane_zoom_by_id(&mut self, pane_id: String, cx: &mut Context<Self>) {
        self.run_pane_rpc(
            cx,
            None,
            move |client| client.toggle_pane_zoom(&pane_id),
            |view, result, _, cx| {
                view.apply_current_layout(result.layout, cx);
            },
        );
    }

    pub(super) fn run_pane_layout_action_by_id(
        &mut self,
        pane_id: String,
        direction: &'static str,
        operation: fn(
            &dyn shardlane_host::mux::MultiplexerConnection,
            &str,
            shardlane_host::mux::MuxDirection,
        ) -> Result<PaneLayoutActionResult, shardlane_host::mux::MuxError>,
        cx: &mut Context<Self>,
    ) {
        let Ok(parsed_direction) = direction.parse::<shardlane_host::mux::MuxDirection>() else {
            return;
        };
        self.run_pane_rpc(
            cx,
            None,
            move |client| operation(client, &pane_id, parsed_direction),
            |view, result, _, cx| {
                view.apply_current_layout(result.layout, cx);
            },
        );
    }

    /// F3: directional navigation shares one implementation with click navigation —
    /// find the neighbor locally, then delegate to focus_pane_id so keyboard focus
    /// fallback/scroll reset/IME cleanup/attach behave exactly like a click.
    pub(super) fn focus_pane_direction_by_id(
        &mut self,
        pane_id: String,
        direction: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layout) = self
            .state
            .layouts
            .iter()
            .find(|layout| layout.panes.iter().any(|pane| pane.pane_id == pane_id))
        else {
            return;
        };
        let Some(neighbor) = neighbor_pane_in_direction(layout, &pane_id, direction) else {
            return;
        };
        self.focus_pane_id(neighbor, window, cx);
    }

    pub(super) fn resize_pane_direction_by_id(
        &mut self,
        pane_id: String,
        direction: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.run_pane_layout_action_by_id(pane_id, direction, mux_resize_pane, cx);
    }

    pub(super) fn swap_pane_direction_by_id(
        &mut self,
        pane_id: String,
        direction: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.run_pane_layout_action_by_id(pane_id, direction, mux_swap_pane, cx);
    }

    pub(super) fn close_pane_by_id(
        &mut self,
        pane_id: String,
        workspace_id: String,
        cx: &mut Context<Self>,
    ) {
        let closes_workspace = self
            .state
            .workspaces
            .iter()
            .find(|workspace| workspace.workspace_id == workspace_id)
            .and_then(|workspace| workspace.pane_count)
            .is_some_and(|count| count <= 1);
        let operation_workspace_id = workspace_id.clone();
        self.run_pane_rpc(
            cx,
            None,
            move |client| {
                if closes_workspace {
                    client.close_workspace(&operation_workspace_id)
                } else {
                    client.close_pane(&pane_id)
                }
            },
            move |view, (), _, cx| {
                if !closes_workspace {
                    view.sidebar_pane.update(cx, |sidebar, cx| {
                        if sidebar.invalidate_project_panes(&workspace_id) {
                            cx.notify();
                        }
                    });
                }
            },
        );
    }

    pub(super) fn move_pane_to_tab_by_id(
        &mut self,
        pane_id: String,
        target_tab_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let attach_window = window.window_handle();
        self.run_navigation_rpc(
            client,
            cx,
            Some(attach_window),
            false,
            move |client| {
                let moved = client.move_pane_to_tab(&pane_id, &target_tab_id)?;
                let target_workspace_id = moved
                    .target_layout
                    .workspace_id
                    .clone()
                    .or_else(|| moved.pane.workspace_id.clone())
                    .ok_or_else(|| {
                        shardlane_host::mux::MuxError::Api(
                            "pane.move result omitted target workspace".to_string(),
                        )
                    })?;
                let target_tab_id = moved.target_layout.tab_id.clone();
                let surface = client.tab_surface_state(&target_workspace_id, &target_tab_id)?;
                let navigation = client.navigation_state()?;
                Ok((navigation, surface, moved.pane.pane_id))
            },
            move |view, (navigation, surface, moved_pane_id), cx| {
                let workspace_id = surface.workspace_id.clone();
                let tab_id = surface.tab_id.clone();
                view.apply_navigation_state(navigation);
                view.apply_tab_surface_state(surface, cx);
                view.sidebar_pane.update(cx, |sidebar, cx| {
                    sidebar.update_cached_pane_location(&moved_pane_id, &workspace_id, &tab_id, cx)
                });
                view.reset_terminal_scroll_state();
                true
            },
        );
    }

    /// Agent correction — manual claim: declare that a pane runs a specific agent.
    /// On success, immediately re-pull agent.list (the manual kick bottoms out the
    /// event-reconciliation loop, covering event silence when no state changed).
    pub(super) fn mark_pane_as_agent_by_id(
        &mut self,
        pane_id: String,
        agent: AgentId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_pane_rpc(
            cx,
            None,
            move |client| {
                client
                    .agent_runtime()
                    .ok_or(shardlane_host::mux::MuxError::Unsupported("agents"))?
                    .report_pane_agent(&pane_id, agent_cli::herdr_agent_id(agent))
            },
            |view, (), _, cx| {
                view.refresh_agents_snapshot(cx);
            },
        );
    }

    /// Agent correction — manual denial: clear all agent attribution on the pane (including Herdr's own inference).
    pub(super) fn deny_pane_agent_by_id(
        &mut self,
        pane_id: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_pane_rpc(
            cx,
            None,
            move |client| {
                client
                    .agent_runtime()
                    .ok_or(shardlane_host::mux::MuxError::Unsupported("agents"))?
                    .clear_pane_agent_authority(&pane_id)
            },
            |view, (), _, cx| {
                view.refresh_agents_snapshot(cx);
            },
        );
    }

    /// Copies one runtime identifier (Pane/Tab/Session ID) to the clipboard.
    /// The toast echoes the exact value so the user can verify what was
    /// captured before pasting it into a CLI/API call.
    pub(crate) fn copy_runtime_id(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.write_to_clipboard(crepuscularity_gpui::ClipboardItem::new_string(id.clone()));
        window.push_notification(crate::i18n::t_with("shell.id_copied", &[("id", id)]), cx);
    }

    pub(super) fn show_pane_process_info_by_id(
        &mut self,
        pane_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let window_handle = window.window_handle();
        self.run_pane_rpc(
            cx,
            Some(window_handle),
            move |client| client.pane_process_info(&pane_id),
            |view, info, window, cx| {
                if let Some(window) = window {
                    view.open_pane_process_info_dialog(info, window, cx);
                }
            },
        );
    }

    pub(super) fn open_pane_process_info_dialog(
        &mut self,
        info: PaneProcessInfo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane_id = info.pane_id.clone();
        let shell_pid = info
            .shell_pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "—".to_string());
        let tty = info.tty.clone().unwrap_or_else(|| "—".to_string());
        let process_group = info
            .foreground_process_group_id
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "—".to_string());
        let processes = info.foreground_processes.clone();
        let dialog_width =
            responsive_dialog_width(window.bounds().size.width.to_f64(), 0.78, 300.0, 620.0);
        window.open_dialog(cx, move |dialog, _window, cx| {
            let mut process_list = v_flex().w_full().gap_2();
            for process in &processes {
                let command = process
                    .cmdline
                    .clone()
                    .or_else(|| process.argv.as_ref().map(|argv| argv.join(" ")))
                    .or_else(|| process.argv0.clone())
                    .unwrap_or_else(|| process.name.clone());
                let cwd = process.cwd.clone().unwrap_or_else(|| "—".to_string());
                process_list = process_list.child(
                    v_flex()
                        .w_full()
                        .gap_1()
                        .px_3()
                        .py_2()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().secondary)
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .gap_3()
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(process.name.clone()),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(theme::FONT_META)
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("PID {}", process.pid)),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(theme::FONT_META)
                                .text_color(cx.theme().muted_foreground)
                                .child(command),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(theme::FONT_META)
                                .text_color(cx.theme().muted_foreground)
                                .child(cwd),
                        ),
                );
            }
            if processes.is_empty() {
                process_list = process_list.child(
                    div()
                        .text_size(theme::FONT_DESCRIPTION)
                        .text_color(cx.theme().muted_foreground)
                        .child("No foreground processes reported by Herdr."),
                );
            }
            dialog
                .w(px(dialog_width))
                .margin_top(px(80.0))
                .close_button(true)
                .child(
                    v_flex()
                        .w_full()
                        .gap_3()
                        .child(
                            div()
                                .text_size(theme::FONT_SECTION_TITLE)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Process Info"),
                        )
                        .child(
                            div()
                                .text_size(theme::FONT_DESCRIPTION)
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("Pane {pane_id}")),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .flex_wrap()
                                .gap_4()
                                .text_size(theme::FONT_DESCRIPTION)
                                .child(format!("Shell PID  {shell_pid}"))
                                .child(format!("TTY  {tty}"))
                                .child(format!("PGRP  {process_group}")),
                        )
                        .child(process_list),
                )
        });
    }

    pub(super) fn move_pane_to_new_tab_by_id(
        &mut self,
        pane_id: String,
        workspace_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let attach_window = window.window_handle();
        self.run_navigation_rpc(
            client,
            cx,
            Some(attach_window),
            false,
            move |client| {
                let moved: PaneMoveResult = client.move_pane_to_new_tab(&pane_id, &workspace_id)?;
                let target_workspace_id = moved
                    .target_layout
                    .workspace_id
                    .clone()
                    .or_else(|| moved.pane.workspace_id.clone())
                    .unwrap_or(workspace_id);
                let target_tab_id = moved.target_layout.tab_id.clone();
                let surface = TabSurfaceState {
                    workspace_id: target_workspace_id,
                    tab_id: target_tab_id,
                    focused_pane_id: moved.target_layout.focused_pane_id.clone(),
                    panes: vec![moved.pane],
                    layouts: vec![moved.target_layout],
                };
                let navigation = client.navigation_state()?;
                Ok((navigation, surface))
            },
            move |view, (navigation, surface), cx| {
                let workspace_id = surface.workspace_id.clone();
                let tab_id = surface.tab_id.clone();
                let moved_pane_id = surface.panes.first().map(|pane| pane.pane_id.clone());
                view.apply_navigation_state(navigation);
                view.apply_tab_surface_state(surface, cx);
                if let Some(pane_id) = moved_pane_id {
                    view.sidebar_pane.update(cx, |sidebar, cx| {
                        sidebar.update_cached_pane_location(&pane_id, &workspace_id, &tab_id, cx)
                    });
                }
                view.reset_terminal_scroll_state();
                true
            },
        );
    }

    pub(super) fn split_right(
        &mut self,
        _: &SplitRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane_id) = self.focused_pane().map(|pane| pane.pane_id.clone()) else {
            return;
        };
        self.split_pane_right_by_id(pane_id, cx);
    }

    pub(super) fn split_down(
        &mut self,
        _: &SplitDown,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane_id) = self.focused_pane().map(|pane| pane.pane_id.clone()) else {
            return;
        };
        self.split_pane_down_by_id(pane_id, cx);
    }

    pub(super) fn toggle_pane_zoom(
        &mut self,
        _: &TogglePaneZoom,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane_id) = self.focused_pane().map(|pane| pane.pane_id.clone()) else {
            return;
        };
        self.toggle_pane_zoom_by_id(pane_id, cx);
    }

    pub(super) fn close_pane(
        &mut self,
        _: &ClosePane,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane_id) = self.focused_pane().map(|pane| pane.pane_id.clone()) else {
            return;
        };
        let Some(workspace_id) = self.active_workspace_id().map(str::to_string) else {
            return;
        };
        self.close_pane_by_id(pane_id, workspace_id, cx);
    }
}

/// Pane-operation adapters over the neutral seam: the by_id family keeps its
/// `fn`-pointer shape, so the four Herdr method pointers become free functions.
fn mux_split_right(
    client: &dyn shardlane_host::mux::MultiplexerConnection,
    pane_id: &str,
) -> Result<Pane, shardlane_host::mux::MuxError> {
    client.split_pane(pane_id, shardlane_host::mux::SplitDirection::Right)
}

fn mux_split_down(
    client: &dyn shardlane_host::mux::MultiplexerConnection,
    pane_id: &str,
) -> Result<Pane, shardlane_host::mux::MuxError> {
    client.split_pane(pane_id, shardlane_host::mux::SplitDirection::Down)
}

fn mux_resize_pane(
    client: &dyn shardlane_host::mux::MultiplexerConnection,
    pane_id: &str,
    direction: shardlane_host::mux::MuxDirection,
) -> Result<PaneLayoutActionResult, shardlane_host::mux::MuxError> {
    client.resize_pane(pane_id, direction)
}

fn mux_swap_pane(
    client: &dyn shardlane_host::mux::MultiplexerConnection,
    pane_id: &str,
    direction: shardlane_host::mux::MuxDirection,
) -> Result<PaneLayoutActionResult, shardlane_host::mux::MuxError> {
    client.swap_pane(pane_id, direction)
}
