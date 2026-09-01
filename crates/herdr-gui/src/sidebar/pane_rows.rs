//! [INPUT]: Constants, types, and root-level imports from the sidebar module root (`super`); full inheritance via `use super::*`.
//! [OUTPUT]: Provides ShardlaneApp::sidebar_pane_row — pane row rendering (agent claim/disown menu, process info entry, focus state).
//! [POS]: Pane row rendering layer of `crates/herdr-gui::sidebar`; consumed by shell; calls the rows primitives and the projection; mechanically split out of sidebar.rs and sharing the module-root namespace with its sibling submodules.
use super::*;
use crate::ui::menus::{menu_action, menu_action_cx};

impl ShardlaneApp {
    pub(super) fn sidebar_pane_row(
        &self,
        pane: &Pane,
        workspace_id: &str,
        tab_id: &str,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane_id = pane.pane_id.clone();
        let focus_pane_id = pane_id.clone();
        let focus_workspace_id = workspace_id.to_string();
        let focus_tab_id = tab_id.to_string();
        let move_workspace_id = workspace_id.to_string();
        let move_targets = self
            .state
            .tabs
            .iter()
            .filter(|tab| {
                tab.workspace_id.as_deref() == Some(workspace_id) && tab.tab_id.as_str() != tab_id
            })
            .map(|tab| (tab.tab_id.clone(), self.tab_title(tab)))
            .collect::<Vec<_>>();
        let close_workspace_id = workspace_id.to_string();
        let label = pane
            .label
            .as_deref()
            .filter(|label| !label.trim().is_empty())
            .or_else(|| {
                pane.title
                    .as_deref()
                    .filter(|title| !title.trim().is_empty())
            })
            .or_else(|| {
                pane.terminal_title
                    .as_deref()
                    .filter(|title| !title.trim().is_empty())
            })
            .or_else(|| {
                pane.cwd
                    .as_deref()
                    .and_then(|cwd| std::path::Path::new(cwd).file_name())
                    .and_then(|name| name.to_str())
            })
            .unwrap_or(&pane.pane_id)
            .to_string();
        let rename_label = label.clone();
        let role_badge = if pane.agent.as_deref().is_some() {
            Some("Agent")
        } else {
            Some("Terminal")
        };
        let lead = pane
            .agent
            .as_deref()
            .and_then(|agent| agent_brand_icon(agent, dark))
            .map(RowLead::Brand)
            .unwrap_or(RowLead::Icon("icons/terminal.svg"));
        let zoomed = self.state.layout_for_tab(tab_id).is_some_and(|layout| {
            layout.zoomed && layout.focused_pane_id.as_deref() == Some(pane.pane_id.as_str())
        });
        let status: Option<crate::status::AttentionLevel> = pane
            .agent_status
            .as_deref()
            .filter(|status| *status != "unknown")
            .map(crate::status::attention_for_raw_status)
            .or(if zoomed {
                Some(crate::status::AttentionLevel::Idle)
            } else {
                None
            });
        let active = self.state.focused_pane_id.as_deref() == Some(pane.pane_id.as_str());
        let herdr = cx.entity();
        let focus_herdr = herdr.clone();

        sidebar_row(
            &self.sidebar_roving_workspaces,
            format!("shardlane-pane-{pane_id}"),
            lead,
            label,
            role_badge,
            None,
            None,
            false,
            status,
            active,
            RowLevel::Pane,
            move |window, app| {
                focus_herdr.update(app, |this, cx| {
                    // FocusIntent seam: sidebar entries go through apply_focus_intent.
                    this.apply_focus_intent(
                        FocusIntent::pane(
                            Some(focus_workspace_id.clone()),
                            Some(focus_tab_id.clone()),
                            focus_pane_id.clone(),
                        ),
                        window,
                        cx,
                    );
                });
            },
            cx,
        )
        .context_menu(move |menu, window, cx| {
            let herdr = herdr.clone();
            let pane_id = pane_id.clone();
            let rename_label = rename_label.clone();
            let move_workspace_id = move_workspace_id.clone();
            let move_targets = move_targets.clone();
            let close_workspace_id = close_workspace_id.clone();

            let mut menu = menu
                .item({
                    let rename_label = rename_label.clone();
                    let pane_id = pane_id.clone();
                    menu_action("Rename Pane…", &herdr, move |this, window, cx| {
                        this.open_pane_rename(pane_id.clone(), rename_label.clone(), window, cx)
                    })
                })
                .submenu("Mark as Agent…", window, cx, {
                    let herdr = herdr.clone();
                    let pane_id = pane_id.clone();
                    move |submenu, _, _| {
                        AgentId::ALL.into_iter().fold(submenu, |submenu, agent| {
                            let pane_id = pane_id.clone();
                            submenu.item(menu_action(
                                agent.display_name(),
                                &herdr,
                                move |this, window, cx| {
                                    this.mark_pane_as_agent_by_id(
                                        pane_id.clone(),
                                        agent,
                                        window,
                                        cx,
                                    );
                                },
                            ))
                        })
                    }
                })
                .item({
                    let pane_id = pane_id.clone();
                    let ws = move_workspace_id.clone();
                    menu_action("Move to New Tab", &herdr, move |this, window, cx| {
                        this.move_pane_to_new_tab_by_id(pane_id.clone(), ws.clone(), window, cx)
                    })
                });

            if !move_targets.is_empty() {
                menu = menu.submenu("Move to Tab", window, cx, {
                    let herdr = herdr.clone();
                    let pane_id = pane_id.clone();
                    move |submenu, _, _| {
                        move_targets
                            .iter()
                            .fold(submenu, |submenu, (tab_id, title)| {
                                let pane_id = pane_id.clone();
                                let tab_id = tab_id.clone();
                                submenu.item(menu_action(
                                    title.clone(),
                                    &herdr,
                                    move |this, window, cx| {
                                        this.move_pane_to_tab_by_id(
                                            pane_id.clone(),
                                            tab_id.clone(),
                                            window,
                                            cx,
                                        )
                                    },
                                ))
                            })
                    }
                });
            }

            menu = menu
                .item(PopupMenuItem::separator())
                .submenu("Focus", window, cx, {
                    let herdr = herdr.clone();
                    let pane_id = pane_id.clone();
                    move |submenu, _, _| {
                        [
                            ("Left", "left"),
                            ("Right", "right"),
                            ("Up", "up"),
                            ("Down", "down"),
                        ]
                        .into_iter()
                        .fold(submenu, |submenu, (label, direction)| {
                            let pane_id = pane_id.clone();
                            submenu.item(menu_action(label, &herdr, move |this, window, cx| {
                                this.focus_pane_direction_by_id(
                                    pane_id.clone(),
                                    direction,
                                    window,
                                    cx,
                                )
                            }))
                        })
                    }
                });

            menu = menu.submenu("Resize", window, cx, {
                let herdr = herdr.clone();
                let pane_id = pane_id.clone();
                move |submenu, _, _| {
                    [
                        ("Left", "left"),
                        ("Right", "right"),
                        ("Up", "up"),
                        ("Down", "down"),
                    ]
                    .into_iter()
                    .fold(submenu, |submenu, (label, direction)| {
                        let pane_id = pane_id.clone();
                        submenu.item(menu_action_cx(label, &herdr, move |this, cx| {
                            this.resize_pane_direction_by_id(pane_id.clone(), direction, cx)
                        }))
                    })
                }
            });

            menu = menu.submenu("Swap With", window, cx, {
                let herdr = herdr.clone();
                let pane_id = pane_id.clone();
                move |submenu, _, _| {
                    [
                        ("Left", "left"),
                        ("Right", "right"),
                        ("Up", "up"),
                        ("Down", "down"),
                    ]
                    .into_iter()
                    .fold(submenu, |submenu, (label, direction)| {
                        let pane_id = pane_id.clone();
                        submenu.item(menu_action_cx(label, &herdr, move |this, cx| {
                            this.swap_pane_direction_by_id(pane_id.clone(), direction, cx)
                        }))
                    })
                }
            });

            menu.item(PopupMenuItem::separator())
                .item({
                    let pane_id = pane_id.clone();
                    menu_action_cx("Split Right", &herdr, move |this, cx| {
                        this.split_pane_right_by_id(pane_id.clone(), cx)
                    })
                })
                .item({
                    let pane_id = pane_id.clone();
                    menu_action_cx("Split Down", &herdr, move |this, cx| {
                        this.split_pane_down_by_id(pane_id.clone(), cx)
                    })
                })
                .item(PopupMenuItem::separator())
                .item({
                    let pane_id = pane_id.clone();
                    menu_action_cx("Toggle Pane Zoom", &herdr, move |this, cx| {
                        this.toggle_pane_zoom_by_id(pane_id.clone(), cx)
                    })
                })
                .item({
                    let pane_id = pane_id.clone();
                    menu_action("Process Info…", &herdr, move |this, window, cx| {
                        this.show_pane_process_info_by_id(pane_id.clone(), window, cx)
                    })
                })
                .item(PopupMenuItem::separator())
                .item({
                    let pane_id = pane_id.clone();
                    menu_action_cx("Close Pane", &herdr, move |this, cx| {
                        this.close_pane_by_id(pane_id.clone(), close_workspace_id.clone(), cx)
                    })
                })
        })
        .into_any_element()
    }
}
