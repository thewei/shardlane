//! [INPUT]: Constants, types, and root-level imports from the sidebar module root (`super`); full inheritance via `use super::*`.
//! [OUTPUT]: Provides ShardlaneApp's history rows, workspace agent status aggregation, and project/tab row rendering (including drag initiation and drop targets: tabs map through the authoritative order, projects reorder via Herdr, load more). Project rows project a git identity trailing (branch + working-tree +/- counts) from the per-project snapshot map; Tab leads use the square-terminal glyph with a success service badge when an observed service runs inside the Tab.
//! [POS]: Tree-row rendering layer of `crates/herdr-gui::sidebar`; consumed by shell; calls the rows primitives and the projection; mechanically split out of sidebar.rs and sharing the module-root namespace with its sibling submodules.
use super::*;
use crate::ui::menus::{menu_action, menu_action_cx};

impl ShardlaneApp {
    pub(super) fn workspace_agent_status(
        &self,
        workspace_id: &str,
    ) -> Option<crate::status::AttentionLevel> {
        let agent_status = self
            .state
            .agents
            .iter()
            .filter(|agent| agent.workspace_id.as_deref() == Some(workspace_id))
            .filter_map(|agent| agent.agent_status.as_deref())
            .find(|status| *status == "blocked")
            .or_else(|| {
                self.state
                    .agents
                    .iter()
                    .filter(|agent| agent.workspace_id.as_deref() == Some(workspace_id))
                    .filter_map(|agent| agent.agent_status.as_deref())
                    .find(|status| *status == "working")
            });
        agent_status.map(crate::status::attention_for_raw_status)
    }

    pub(super) fn sidebar_project_row(
        &self,
        workspace: &Workspace,
        resolved_project_path: Option<&str>,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workspace_id = workspace.workspace_id.clone();
        let focus_id = workspace.workspace_id.clone();
        let menu_new_tab_id = workspace.workspace_id.clone();
        let menu_rename_id = workspace.workspace_id.clone();
        let menu_close_id = workspace.workspace_id.clone();
        let title = workspace
            .label
            .as_deref()
            .unwrap_or(&workspace.workspace_id)
            .to_string();
        let active = workspace.focused
            || self
                .active_workspace_id()
                .is_some_and(|focused| focused == workspace.workspace_id);
        let herdr = cx.entity();
        let history_action_herdr = herdr.clone();
        let rename_title = title.clone();
        let project_cwd = resolved_project_path
            .filter(|p| !p.trim().is_empty())
            .map(|p| p.to_string())
            .or_else(|| {
                workspace
                    .cwd
                    .as_deref()
                    .filter(|p| !p.trim().is_empty())
                    .map(|p| p.to_string())
            })
            .or_else(|| {
                self.panes_by_project
                    .get(&workspace.workspace_id)
                    .and_then(|panes| {
                        panes.iter().find_map(|pane| {
                            pane.cwd
                                .as_deref()
                                .filter(|p| !p.trim().is_empty())
                                .map(|p| p.to_string())
                        })
                    })
            })
            .or_else(|| {
                self.state
                    .panes
                    .iter()
                    .filter(|pane| pane.workspace_id.as_deref() == Some(workspace_id.as_str()))
                    .find_map(|pane| {
                        pane.cwd
                            .as_deref()
                            .filter(|p| !p.trim().is_empty())
                            .map(|p| p.to_string())
                    })
            })
            .or_else(|| {
                self.state
                    .agents
                    .iter()
                    .filter(|agent| agent.workspace_id.as_deref() == Some(workspace_id.as_str()))
                    .find_map(|agent| {
                        agent
                            .foreground_cwd
                            .as_deref()
                            .or(agent.cwd.as_deref())
                            .filter(|p| !p.trim().is_empty())
                            .map(|p| p.to_string())
                    })
            })
            .unwrap_or_default();
        let drag = SidebarProjectDrag {
            workspace_id: workspace.workspace_id.clone(),
            label: title.clone(),
            position: Point::default(),
        };
        let drag_over_workspace_id = workspace.workspace_id.clone();
        let drop_target_workspace_id = workspace.workspace_id.clone();
        let drop_herdr = herdr.clone();
        let drop_color = cx.theme().primary;
        let focus_herdr = herdr.clone();
        let status = self.workspace_agent_status(&workspace.workspace_id);
        let history_action_project = project_cwd.clone();
        let history_action_group = format!("shardlane-project-history-{workspace_id}");
        let component_theme = cx.theme().clone();

        let git_snapshot = self
            .find_sidebar_git_status(&project_cwd)
            .or_else(|| {
                self.panes_by_project
                    .get(&workspace.workspace_id)
                    .and_then(|panes| {
                        panes.iter().find_map(|pane| {
                            pane.cwd
                                .as_deref()
                                .and_then(|cwd| self.find_sidebar_git_status(cwd))
                        })
                    })
            })
            .or_else(|| {
                self.state
                    .panes
                    .iter()
                    .filter(|pane| pane.workspace_id.as_deref() == Some(workspace_id.as_str()))
                    .find_map(|pane| {
                        pane.cwd
                            .as_deref()
                            .and_then(|cwd| self.find_sidebar_git_status(cwd))
                    })
            });

        // Git identity displayed directly to the right of the project item title:
        // branch icon + branch name + file change +/- counts.
        let git_element: Option<AnyElement> = git_snapshot.map(|snapshot| {
            let theme = component_theme.clone();
            h_flex()
                .min_w_0()
                .flex_shrink_0()
                .items_center()
                .gap(px(3.0))
                .child(
                    icon("icons/git-branch.svg")
                        .with_size(px(10.5))
                        .text_color(theme.muted_foreground.opacity(0.7)),
                )
                .child(
                    div()
                        .max_w(px(72.0))
                        .text_size(crate::theme::FONT_META)
                        .text_color(theme.muted_foreground.opacity(0.85))
                        .truncate()
                        .child(snapshot.branch.clone()),
                )
                .when(snapshot.additions > 0 || snapshot.deletions > 0, |row| {
                    row.child(
                        h_flex()
                            .gap(px(2.0))
                            .items_center()
                            .when(snapshot.additions > 0, |r| {
                                r.child(
                                    div()
                                        .text_size(crate::theme::FONT_META)
                                        .text_color(theme.success)
                                        .child(format!("+{}", snapshot.additions)),
                                )
                            })
                            .when(snapshot.deletions > 0, |r| {
                                r.child(
                                    div()
                                        .text_size(crate::theme::FONT_META)
                                        .text_color(theme.danger)
                                        .child(format!("-{}", snapshot.deletions)),
                                )
                            }),
                    )
                })
                .into_any_element()
        });

        let row_element = sidebar_row(
            &self.sidebar_roving_workspaces,
            format!("shardlane-workspace-{workspace_id}"),
            RowLead::Project {
                expanded: !collapsed,
            },
            title,
            git_element,
            None,
            None,
            None,
            false,
            None,
            active,
            RowLevel::Primary,
            move |window, app| {
                focus_herdr.update(app, |this, cx| {
                    if this.native_tabs_enabled() {
                        // Native-tab placement: Projects don't expand a Tab subtree, so a
                        // click focuses the Project (the strip then shows its Tabs).
                        this.focus_project_from_sidebar(focus_id.clone(), window, cx);
                    } else {
                        this.toggle_project_folder(focus_id.clone(), window, cx);
                    }
                });
            },
            cx,
        )
        .on_drag(drag, |drag, position, _, cx| {
            let drag = drag.clone().position(position);
            cx.new(|_| drag)
        })
        .can_drop(|value, _, _| value.downcast_ref::<SidebarProjectDrag>().is_some())
        .drag_over::<SidebarProjectDrag>(move |style, drag, _, _| {
            if drag.workspace_id != drag_over_workspace_id {
                style.border_t_1().border_color(drop_color)
            } else {
                style
            }
        })
        .on_drop(move |drag: &SidebarProjectDrag, _, app| {
            if drag.workspace_id == drop_target_workspace_id {
                return;
            }
            let dragged_workspace_id = drag.workspace_id.clone();
            let target_workspace_id = drop_target_workspace_id.clone();
            drop_herdr.update(app, |this, cx| {
                this.reorder_sidebar_project(dragged_workspace_id, target_workspace_id, cx)
            });
        })
        .context_menu(move |menu, _, _| {
            let herdr = herdr.clone();
            let workspace_id = menu_new_tab_id.clone();
            let rename_label = rename_title.clone();

            let m = menu
                .item({
                    let ws = workspace_id.clone();
                    menu_action("New Tab", &herdr, move |this, window, cx| {
                        this.create_tab_in_workspace(Some(ws.clone()), window, cx)
                    })
                })
                .item({
                    let ws = menu_rename_id.clone();
                    let label = rename_label.clone();
                    menu_action("Rename Project…", &herdr, move |this, window, cx| {
                        this.open_project_rename(ws.clone(), label.clone(), window, cx)
                    })
                });
            m.item(PopupMenuItem::separator()).item({
                let ws = menu_close_id.clone();
                menu_action("Close Project", &herdr, move |this, window, cx| {
                    this.close_workspace_id(ws.clone(), window, cx)
                })
            })
        })
        .into_any_element();

        let status_element: Option<AnyElement> = status.map(|level| {
            div()
                .absolute()
                .right(SIDEBAR_EDGE)
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .group_hover(history_action_group.clone(), |s| s.opacity(0.0))
                .child(crate::status::status_glyph_container(
                    SharedString::from(format!("project-status-{workspace_id}")),
                    level,
                    cx,
                ))
                .into_any_element()
        });
        div()
            .relative()
            .w_full()
            .group(history_action_group.clone())
            .child(row_element)
            .when_some(status_element, |wrapper, el| wrapper.child(el))
            .child(
                div()
                    .id(SharedString::from(format!(
                        "shardlane-project-history-action-{workspace_id}"
                    )))
                    .group_hover(history_action_group, |style| style.opacity(1.0))
                    .opacity(0.0)
                    .absolute()
                    .right(SIDEBAR_EDGE)
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(22.0))
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(component_theme.foreground.opacity(0.08)))
                    .tooltip(crate::ui::tooltip::tooltip_fn("Project History"))
                    .child(
                        icon("icons/layers.svg")
                            .with_size(px(13.0))
                            .text_color(component_theme.muted_foreground),
                    )
                    .on_click(move |_, _, app| {
                        app.stop_propagation();
                        let project = history_action_project.clone();
                        history_action_herdr
                            .update(app, |this, cx| this.open_project_history(project, cx));
                    }),
            )
            .into_any_element()
    }

    pub(super) fn sidebar_tab_row(
        &self,
        tab: Tab,
        workspace_id: String,
        insert_index: usize,
        pane_count: usize,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tab_id = tab.tab_id.clone();
        let wrapper_tab_id = tab.tab_id.clone();
        let focus_id = tab.tab_id.clone();
        let rename_id = tab.tab_id.clone();
        let close_id = tab.tab_id.clone();
        let active = tab.focused
            || self
                .state
                .focused_tab_id
                .as_deref()
                .is_some_and(|focused| focused == tab.tab_id);
        let title = self.tab_title(&tab);
        let rename_title = title.clone();
        let drag = SidebarTabDrag {
            tab_id: tab.tab_id.clone(),
            workspace_id: workspace_id.clone(),
            label: title.clone(),
            position: Point::default(),
        };
        let can_drop_workspace_id = workspace_id.clone();
        let drag_over_workspace_id = workspace_id.clone();
        let drop_target_workspace_id = workspace_id.clone();
        let drag_over_tab_id = tab.tab_id.clone();
        let drop_target_tab_id = tab.tab_id.clone();
        let drop_herdr = cx.entity();
        let drop_color = cx.theme().primary;
        let tab_agent = self
            .state
            .agents
            .iter()
            .find(|agent| agent.tab_id.as_deref() == Some(tab.tab_id.as_str()));
        // Service badge join: an observed service (process listening on a port)
        // carries its owning Tab id, so a Terminal Tab's glyph gets a success
        // dot exactly when one of this Tab's panes hosts a running service.
        let has_running_service = self
            .observed_services
            .iter()
            .any(|service| service.tab_id == tab.tab_id)
            || self.scripts.scripts.iter().any(|script| {
                script.tab_id.as_deref() == Some(tab.tab_id.as_str())
                    && script.runtime.status == crate::scripts::ScriptStatus::Running
            });
        let lead = tab_agent
            .and_then(agent_identity)
            .and_then(|identity| agent_brand_icon(identity, dark))
            .map(RowLead::Brand)
            .unwrap_or({
                // Terminal-specific glyph (square-terminal): the generic
                // terminal.svg read as a placeholder, not as a product icon.
                if has_running_service {
                    RowLead::IconService("icons/square-terminal.svg")
                } else {
                    RowLead::Icon("icons/square-terminal.svg")
                }
            });
        let tab_agent_status = tab_agent
            .and_then(|agent| {
                agent
                    .agent_status
                    .as_deref()
                    .or(agent.custom_status.as_deref())
            })
            .filter(|s| *s != "unknown")
            .map(crate::status::attention_for_raw_status);
        let herdr = cx.entity();
        let focus_herdr = herdr.clone();
        let hover_close_herdr = herdr.clone();
        let hover_close_id = tab.tab_id.clone();
        let is_pinned = self.config.ui.sidebar.pinned_tabs.contains(&tab.tab_id);
        let component_theme_tab = cx.theme().clone();
        let close_confirming = self.pending_close_tab.as_deref() == Some(tab.tab_id.as_str());

        let tab_trailing_elements: Vec<AnyElement> = {
            let mut els = Vec::new();
            if pane_count > 1 {
                els.push(
                    div()
                        .flex_shrink_0()
                        .text_size(FONT_LABEL)
                        .text_color(component_theme_tab.muted_foreground)
                        .child(pane_count.to_string())
                        .into_any_element(),
                );
            }
            if let Some(level) = tab_agent_status {
                els.push(crate::status::status_glyph_container(
                    SharedString::from(format!("tab-status-{tab_id}")),
                    level,
                    cx,
                ));
            }
            els
        };
        let has_trailing = !tab_trailing_elements.is_empty();

        let close_button = div()
            .id(ElementId::Name(
                format!("shardlane-tab-close-{hover_close_id}").into(),
            ))
            .size(px(16.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(3.0))
            .cursor_pointer()
            .when(close_confirming, |s| {
                s.bg(component_theme_tab.danger.opacity(0.18))
            })
            .when(!close_confirming, |s| {
                s.hover(|s| s.bg(component_theme_tab.foreground.opacity(0.12)))
            })
            .active(|s| s.bg(component_theme_tab.danger.opacity(0.24)))
            .on_click(move |_, window, app| {
                app.stop_propagation();
                hover_close_herdr.update(app, |this, cx| {
                    this.confirm_close_tab(hover_close_id.clone(), window, cx)
                });
            })
            .tooltip(move |_, cx| {
                cx.new(|_| {
                    Tooltip::new(if close_confirming {
                        "Click again to close"
                    } else {
                        "Close Tab"
                    })
                })
                .into()
            })
            .child(
                icon("icons/x.svg")
                    .with_size(px(12.0))
                    .text_color(if close_confirming {
                        component_theme_tab.danger
                    } else {
                        component_theme_tab.muted_foreground
                    }),
            );

        let tab_group = format!("shardlane-tab-group-{tab_id}");

        let row_element = sidebar_row(
            &self.sidebar_roving_workspaces,
            format!("shardlane-tab-{tab_id}"),
            lead,
            title,
            None,
            None,
            None,
            None,
            false,
            None,
            active,
            RowLevel::Sub,
            move |window, app| {
                focus_herdr.update(app, |this, cx| {
                    this.apply_focus_intent(FocusIntent::tab(focus_id.clone()), window, cx)
                });
            },
            cx,
        )
        .into_any_element();

        div()
            .id(SharedString::from(format!(
                "shardlane-tab-wrapper-{wrapper_tab_id}"
            )))
            .relative()
            .w_full()
            .group(tab_group.clone())
            .child(row_element)
            .when(has_trailing, {
                let group = tab_group.clone();
                move |wrapper| {
                    wrapper.child(
                        h_flex()
                            .absolute()
                            .right(SIDEBAR_EDGE)
                            .top_0()
                            .bottom_0()
                            .items_center()
                            .gap(SPACE_XS)
                            .group_hover(group, |s| s.opacity(0.0))
                            .children(tab_trailing_elements),
                    )
                }
            })
            .child(
                div()
                    .absolute()
                    .right(SIDEBAR_EDGE)
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .when(!close_confirming, |el| el.opacity(0.0))
                    .group_hover(tab_group, |s| s.opacity(1.0))
                    .child(close_button),
            )
            .on_drag(drag, |drag, position, _, cx| {
                let drag = drag.clone().position(position);
                cx.new(|_| drag)
            })
            .can_drop(move |value, _, _| {
                value
                    .downcast_ref::<SidebarTabDrag>()
                    .is_some_and(|drag| drag.workspace_id == can_drop_workspace_id)
            })
            .drag_over::<SidebarTabDrag>(move |style, drag, _, _| {
                if drag.workspace_id == drag_over_workspace_id && drag.tab_id != drag_over_tab_id {
                    style.border_t_1().border_color(drop_color)
                } else {
                    style
                }
            })
            .on_drop(move |drag: &SidebarTabDrag, _, app| {
                if drag.workspace_id != drop_target_workspace_id
                    || drag.tab_id == drop_target_tab_id
                {
                    return;
                }
                let dragged_tab_id = drag.tab_id.clone();
                let workspace_id = drop_target_workspace_id.clone();
                drop_herdr.update(app, |this, cx| {
                    this.move_tab_to_index(dragged_tab_id, workspace_id, insert_index, cx)
                });
            })
            .context_menu(move |menu, _, _| {
                let herdr = herdr.clone();
                let rename_label = rename_title.clone();
                let pin_label = if is_pinned { "Unpin Tab" } else { "Pin Tab" };

                menu.item({
                    let tab_id = tab_id.clone();
                    menu_action_cx(pin_label, &herdr, move |this, cx| {
                        this.toggle_pin_tab(tab_id.clone(), cx);
                    })
                })
                .item({
                    let tab_id = rename_id.clone();
                    let label = rename_label.clone();
                    menu_action("Rename Tab…", &herdr, move |this, window, cx| {
                        this.open_tab_rename(tab_id.clone(), label.clone(), window, cx)
                    })
                })
                .item({
                    let tab_id = rename_id.clone();
                    menu_action(
                        crate::i18n::t("shell.copy_tab_id"),
                        &herdr,
                        move |this, window, cx| this.copy_runtime_id(tab_id.clone(), window, cx),
                    )
                })
                .item(PopupMenuItem::separator())
                .item({
                    let tab_id = close_id.clone();
                    menu_action("Close Tab", &herdr, move |this, window, cx| {
                        this.close_tab_by_id(tab_id.clone(), window, cx)
                    })
                })
            })
            .into_any_element()
    }
}
