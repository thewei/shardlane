//! [INPUT]: The main-crate namespace and sibling-module public surface forwarded by the new_agent module root (super).
//! [OUTPUT]: Provides New Agent's page rendering tree: the composer card (project/branch/agent pickers, attachments, control rows, send button), the three-tab layout with drag chips, and Script rows (menu delete confirmation + inline two-click arming).
//! [POS]: The `crates/herdr-gui` new_agent submodule (mechanically split out of new_agent.rs), cooperating isomorphically with sibling submodules, exported via the root re-export.
use super::*;
use crate::agent_ui::{composer_send_state, AgentComposer};

#[derive(Clone)]
struct WorkflowDrag {
    script_id: String,
    label: String,
    position: Point<Pixels>,
}

impl WorkflowDrag {
    fn position(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }
}

impl Render for WorkflowDrag {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(6.0))
            .bg(cx.theme().secondary)
            .border_1()
            .border_color(cx.theme().border)
            .text_size(theme::FONT_BODY)
            .text_color(cx.theme().foreground)
            .child(self.label.clone())
    }
}

/// Menu construction shared by the title-sentence picker and the project chip
/// under the card (a single SelectState source of truth, zero duplication
/// across the two entries).
fn new_agent_project_menu(
    choices: Vec<NewAgentProjectChoice>,
    selected: Option<String>,
    app: Entity<ShardlaneApp>,
) -> impl Fn(
    gpui_component::menu::PopupMenu,
    &mut Window,
    &mut Context<gpui_component::menu::PopupMenu>,
) -> gpui_component::menu::PopupMenu {
    move |mut menu, _, _| {
        for project in &choices {
            let workspace_id = project.runtime_workspace_id.clone();
            let picker_herdr = app.clone();
            let checked = selected.as_deref() == Some(workspace_id.as_str());
            let label = project.label.clone();
            let path = project.project_path.clone();
            let path_label = if path.is_empty() {
                "—".to_string()
            } else {
                path.clone()
            };
            menu = menu.item(
                PopupMenuItem::element(move |_, cx| {
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .items_center()
                        .gap_2()
                        .child(div().child(label.clone()))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(theme::FONT_META)
                                .text_color(cx.theme().muted_foreground)
                                .child(path_label.clone()),
                        )
                })
                .checked(checked)
                .on_click(move |_, window, app| {
                    picker_herdr.update(app, |this, cx| {
                        this.select_new_agent_project(workspace_id.clone(), window, cx)
                    });
                }),
            );
        }
        // Trailing action, same as before: New Project — system directory picker →
        // create library → assign → select.
        let flow_herdr = app.clone();
        menu = menu.separator().item(
            PopupMenuItem::element(move |_, _| {
                h_flex()
                    .w_full()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .child(
                        Icon::new(ComponentIconName::FolderOpen)
                            .with_size(px(15.0))
                            .into_any_element(),
                    )
                    .child(div().child("New Project…"))
            })
            .on_click(move |_, window, app_cx| {
                flow_herdr.update(app_cx, |view, cx| {
                    view.begin_workspace_creation(window, cx);
                });
            }),
        );
        menu
    }
}

/// The ProjectNameSelector language: an in-sentence picker label — no padding,
/// no solid border,
/// a 1px dotted underline (dash [1,2], canvas-drawn at the text box's bottom)
/// marks the replaceable slot;
/// resting color muted; the selected/open state darkens in the original
/// (a static resting color is used here).
fn headline_project_label(
    name: String,
    foreground: gpui::Hsla,
    underline: gpui::Hsla,
) -> impl IntoElement {
    div()
        .relative()
        .flex_none()
        .text_size(theme::FONT_HEADLINE)
        .font_weight(FontWeight::MEDIUM)
        .text_color(foreground)
        .child(name)
        .child(
            ::gpui::canvas(
                move |_, _, _| {},
                move |bounds, _, window, _| {
                    let y = bounds.origin.y + bounds.size.height - px(0.5);
                    let mut builder =
                        ::gpui::PathBuilder::stroke(px(1.0)).dash_array(&[px(1.0), px(2.0)]);
                    builder.move_to(::gpui::point(bounds.origin.x, y));
                    builder.line_to(::gpui::point(bounds.origin.x + bounds.size.width, y));
                    if let Ok(path) = builder.build() {
                        window.paint_path(path, underline);
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
}

/// Codex-style dynamic question placeholder: changes with the selected agent
/// ("Ask Codex to…").
pub(super) fn agent_prompt_placeholder(agent: AgentId) -> SharedString {
    SharedString::from(format!("Ask {} to…", agent.display_name()))
}

fn shell_history_suggestions(query: &str, limit: usize) -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let hist_path = PathBuf::from(&home).join(".zsh_history");
    let content = std::fs::read_to_string(&hist_path).unwrap_or_default();
    let query_lower = query.to_lowercase();
    let mut seen = std::collections::HashSet::new();
    let mut results = Vec::new();
    for line in content.lines().rev() {
        let cmd = if let Some(pos) = line.find(';') {
            &line[pos + 1..]
        } else {
            line
        };
        let cmd = cmd.trim();
        if cmd.is_empty() {
            continue;
        }
        if !query_lower.is_empty() && !cmd.to_lowercase().contains(&query_lower) {
            continue;
        }
        if seen.insert(cmd.to_string()) {
            results.push(cmd.to_string());
            if results.len() >= limit {
                break;
            }
        }
    }
    results
}

impl ShardlaneApp {
    pub(crate) fn new_agent_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_new_agent_ui(window, cx);
        let Some(ui) = self.new_agent_ui.as_ref() else {
            return div().size_full().into_any_element();
        };
        let visible_projects = self.composer_visible_projects();
        if visible_projects.is_empty() {
            return self.new_agent_empty_state(cx);
        }
        let project_choices = composer_project_choices_from_visible(&visible_projects);
        let selected_project_id = ui.project.read(cx).selected_value().cloned();
        let context_project_id = self.new_agent_context_workspace_id.clone();
        let display_project_id = context_project_id
            .as_deref()
            .or(selected_project_id.as_deref());
        let project_name = display_project_id
            .and_then(|workspace_id| {
                visible_sidebar_project_by_runtime_id(&visible_projects, workspace_id)
                    .map(|project| project.label.clone())
            })
            .unwrap_or_else(|| "Select Project".to_string());
        let tab_kind = ui.tab;
        let project_path_for_commands =
            sidebar_project_path_for_context(&visible_projects, display_project_id, None);
        let command_scripts: Vec<ScriptRecord> = project_path_for_commands
            .as_deref()
            .map(|project_path| {
                self.scripts
                    .scripts
                    .iter()
                    .filter(|script| {
                        crate::workspace_model::project_paths_match(
                            &script.project_path,
                            project_path,
                        )
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let prompt = ui.prompt.clone();
        let terminal_input = ui.terminal_input.clone();
        let terminal_query = ui.terminal_input.read(cx).value().trim().to_string();
        let terminal_empty = terminal_query.is_empty();
        let branch_state = ui.branch.clone();
        let branch_choices = ui.branch_choices.clone();
        let branch_selected = ui.branch.read(cx).selected_value().cloned();
        let mode = ui.mode;
        let permission = ui.permission;
        let agent = ui.agent;
        let agent_availability = ui.agent_availability.clone();
        let attachments = ui.attachments.clone();
        let branch_loading = ui.branch_loading;
        let submitting = ui.submitting;
        // P12-1: inline delete two-click arming mirror (first click arms →
        // "Delete?", second click executes).
        let script_delete_armed_id = ui.script_delete_armed_id.clone();
        let prompt_empty = ui.prompt.read(cx).value().trim().is_empty();
        let dark = cx.theme().mode.is_dark();
        let attach_herdr = cx.entity();
        let submit_herdr = attach_herdr.clone();
        let project_menu_herdr = attach_herdr.clone();
        let agent_menu_herdr = attach_herdr.clone();
        let available_providers = self.config.providers.available_choices();
        let branch_menu_herdr = attach_herdr.clone();
        let mode_menu_herdr = attach_herdr.clone();
        let permission_menu_herdr = attach_herdr.clone();
        let tab_agent_herdr = attach_herdr.clone();
        let tab_terminal_herdr = attach_herdr.clone();
        let tab_command_herdr = attach_herdr.clone();
        let terminal_submit_herdr = attach_herdr.clone();
        let command_run_herdr = attach_herdr.clone();
        let command_add_herdr = attach_herdr.clone();
        let selected_for_menu = context_project_id.or(selected_project_id);
        // The ProjectNameSelector language: the in-sentence picker has no solid
        // border or padding,
        // a dotted underline (canvas dash [1,2], 1px) marks the replaceable
        // {project} slot;
        // DropdownButton's built-in caret segment hints "click to open" (a manual
        // arrow would double the arrows).
        let headline_foreground = cx.theme().foreground;
        let headline_underline = cx.theme().muted_foreground.opacity(0.55);
        let project_picker = ComposerChip::new("new-agent-project-picker")
            .bare()
            .hover_tint(false)
            .label_element(
                headline_project_label(
                    project_name.clone(),
                    headline_foreground,
                    headline_underline,
                )
                .into_any_element(),
            )
            .dropdown_menu_with_anchor(
                gpui::Corner::BottomLeft,
                new_agent_project_menu(
                    project_choices.clone(),
                    selected_for_menu.clone(),
                    project_menu_herdr.clone(),
                ),
            );

        // The workspace footer's project chip (MenuChip: folder icon + name,
        // max_w190 truncation); shares the same SelectState source of truth as
        // the title-sentence picker, and the menu is shared.
        let project_chip = ComposerChip::new("new-agent-project-chip")
            .max_w(px(190.0))
            .min_w_0()
            .icon(
                Icon::new(ComponentIconName::Folder)
                    .with_size(px(12.0))
                    .text_color(cx.theme().muted_foreground)
                    .into_any_element(),
            )
            .label(project_name.clone())
            .dropdown_menu_with_anchor(
                gpui::Corner::BottomLeft,
                new_agent_project_menu(project_choices, selected_for_menu, project_menu_herdr),
            );

        // Branch pill (text-only ghost): with the title sentence carrying the
        // Project context, Branch recedes as the secondary "where it runs"
        // context into the card's action-area left cluster, on the same layer
        // as attach/mode.
        let branch_label = if branch_loading {
            "Branch…".to_string()
        } else {
            branch_choices
                .iter()
                .find(|choice| Some(choice.name.as_str()) == branch_selected.as_deref())
                .map(|choice| choice.label.clone())
                .or_else(|| branch_selected.clone())
                .unwrap_or_else(|| "Branch".to_string())
        };
        let branch_picker = ComposerChip::new("new-agent-branch-picker")
            .icon(
                Icon::empty()
                    .path("icons/git-branch.svg")
                    .with_size(px(13.0))
                    .text_color(cx.theme().muted_foreground)
                    .into_any_element(),
            )
            .label(branch_label)
            .dropdown_menu_with_anchor(gpui::Corner::BottomLeft, move |mut menu, _, _| {
                for (index, choice) in branch_choices.iter().enumerate() {
                    let state = branch_state.clone();
                    let notify_herdr = branch_menu_herdr.clone();
                    let checked = branch_selected.as_deref() == Some(choice.name.as_str());
                    let label = choice.label.clone();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| div().child(label.clone()))
                            .checked(checked)
                            .on_click(move |_, window, app| {
                                state.update(app, |state, cx| {
                                    state.set_selected_index(
                                        Some(IndexPath::default().row(index)),
                                        window,
                                        cx,
                                    );
                                });
                                notify_herdr.update(app, |_, cx| cx.notify());
                            }),
                    );
                }
                menu
            });

        // MenuChip: a single quiet chip with a brand image + name (no caret
        // segment); the menu's checked state carries selection.
        let agent_picker = ComposerChip::new("new-agent-agent-picker")
            .icon(
                agent_brand_icon(agent.as_str(), dark)
                    .map(|path| img(path).size(px(14.0)).into_any_element())
                    .unwrap_or_else(|| {
                        Icon::new(ComponentIconName::Bot)
                            .with_size(px(14.0))
                            .text_color(cx.theme().muted_foreground)
                            .into_any_element()
                    }),
            )
            .label(agent.display_name())
            .dropdown_menu_with_anchor(gpui::Corner::BottomLeft, move |mut menu, _, _| {
                for candidate in available_providers.iter().copied() {
                    let menu_herdr = agent_menu_herdr.clone();
                    let selected = candidate == agent;
                    let brand_path = agent_brand_icon(candidate.as_str(), dark).map(str::to_string);
                    let unavailable = agent_availability
                        .as_ref()
                        .is_some_and(|set| !set.contains(&candidate));
                    let name = candidate.display_name();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, cx| {
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .items_center()
                                .gap_2()
                                .child(
                                    brand_path
                                        .as_deref()
                                        .map(|path| img(path).size(px(15.0)).into_any_element())
                                        .unwrap_or_else(|| {
                                            Icon::new(ComponentIconName::Bot)
                                                .xsmall()
                                                .into_any_element()
                                        }),
                                )
                                .child(div().child(name))
                                .when(unavailable, |row| {
                                    row.child(
                                        div()
                                            .text_size(theme::FONT_META)
                                            .text_color(cx.theme().muted_foreground)
                                            .child("not installed"),
                                    )
                                })
                        })
                        .checked(selected)
                        .on_click(move |_, window, app| {
                            menu_herdr.update(app, |this, cx| {
                                let Some(ui) = this.new_agent_ui.as_mut() else {
                                    return;
                                };
                                if ui.agent == candidate {
                                    return;
                                }
                                ui.agent = candidate;
                                let prompt = ui.prompt.clone();
                                prompt.update(cx, |state, input_cx| {
                                    state.set_placeholder(
                                        agent_prompt_placeholder(candidate),
                                        window,
                                        input_cx,
                                    );
                                });
                                // Agent switch → rebuild the command catalog for the
                                // new dialect.
                                this.schedule_new_agent_reference_scan(cx);
                                cx.notify();
                            });
                        }),
                    );
                }
                menu
            });

        // Plan/Build selector (operator feedback: chip menus in the same language
        // as agent/branch);
        // the chip icon follows the current mode, selection carried by the
        // menu's checked state.
        let mode_icon = if mode == NewAgentMode::Plan {
            ComponentIconName::BookOpen
        } else {
            ComponentIconName::SquareTerminal
        };
        let mode_picker = ComposerChip::new("new-agent-mode-picker")
            .icon(
                Icon::new(mode_icon)
                    .with_size(px(12.0))
                    .text_color(cx.theme().muted_foreground)
                    .into_any_element(),
            )
            .label(mode.label())
            .dropdown_menu_with_anchor(gpui::Corner::BottomLeft, move |mut menu, _, _| {
                for candidate in [NewAgentMode::Plan, NewAgentMode::Build] {
                    let menu_herdr = mode_menu_herdr.clone();
                    let checked = candidate == mode;
                    let label = candidate.label();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| div().child(label))
                            .checked(checked)
                            .on_click(move |_, _, app| {
                                menu_herdr.update(app, |this, cx| {
                                    let Some(ui) = this.new_agent_ui.as_mut() else {
                                        return;
                                    };
                                    if ui.mode != candidate {
                                        ui.mode = candidate;
                                        cx.notify();
                                    }
                                });
                            }),
                    );
                }
                menu
            });

        let permission_icon_path = match permission {
            NewAgentPermission::AskApproval => "icons/lock.svg",
            NewAgentPermission::AutoApprove | NewAgentPermission::FullAccess => {
                "icons/lock-open.svg"
            }
        };
        let permission_picker = ComposerChip::new("new-agent-permission-picker")
            .icon(
                Icon::empty()
                    .path(permission_icon_path)
                    .with_size(px(12.0))
                    .text_color(cx.theme().muted_foreground)
                    .into_any_element(),
            )
            .label(permission.label())
            .dropdown_menu_with_anchor(gpui::Corner::BottomLeft, move |mut menu, _, _| {
                for candidate in [
                    NewAgentPermission::AskApproval,
                    NewAgentPermission::AutoApprove,
                    NewAgentPermission::FullAccess,
                ] {
                    let menu_herdr = permission_menu_herdr.clone();
                    let checked = candidate == permission;
                    let label = candidate.label();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| div().child(label))
                            .checked(checked)
                            .on_click(move |_, _, app| {
                                menu_herdr.update(app, |this, cx| {
                                    let Some(ui) = this.new_agent_ui.as_mut() else {
                                        return;
                                    };
                                    if ui.permission != candidate {
                                        ui.permission = candidate;
                                        cx.notify();
                                    }
                                });
                            }),
                    );
                }
                menu
            });

        let tab_group = {
            let active_bg = cx.theme().foreground.opacity(0.08);
            let tab_text_active = cx.theme().foreground;
            let tab_text_inactive = cx.theme().muted_foreground;
            let tab_item = |id: &'static str,
                            label: &'static str,
                            is_active: bool,
                            herdr: Entity<ShardlaneApp>,
                            target: NewTabKind| {
                div()
                    .id(id)
                    .px(px(16.0))
                    .py(px(6.0))
                    .rounded(px(7.0))
                    .cursor_pointer()
                    .text_size(crate::theme::FONT_LIST_TITLE)
                    .font_weight(FontWeight::MEDIUM)
                    .when(is_active, |el| el.bg(active_bg).text_color(tab_text_active))
                    .when(!is_active, |el| {
                        el.text_color(tab_text_inactive)
                            .hover(|s| s.text_color(tab_text_active))
                    })
                    .on_click(move |_, _, app| {
                        herdr.update(app, |this, cx| {
                            if let Some(ui) = this.new_agent_ui.as_mut() {
                                ui.tab = target;
                                cx.notify();
                            }
                        });
                    })
                    .child(label)
            };
            h_flex()
                .gap(px(2.0))
                .px(px(4.0))
                .py(px(4.0))
                .rounded(px(10.0))
                .bg(cx.theme().foreground.opacity(0.04))
                .child(tab_item(
                    "new-script-tab-agent",
                    "Agent",
                    tab_kind == NewTabKind::Agent,
                    tab_agent_herdr,
                    NewTabKind::Agent,
                ))
                .child(tab_item(
                    "new-script-tab-terminal",
                    "Terminal",
                    tab_kind == NewTabKind::Terminal,
                    tab_terminal_herdr,
                    NewTabKind::Terminal,
                ))
                .child(tab_item(
                    "new-script-tab-command",
                    "Script",
                    tab_kind == NewTabKind::Command,
                    tab_command_herdr,
                    NewTabKind::Command,
                ))
        };

        let headline_text = match tab_kind {
            NewTabKind::Agent => "What should we do in ",
            NewTabKind::Terminal => "Run command in ",
            NewTabKind::Command => "Workflows in ",
        };

        let tab_key_herdr = attach_herdr.clone();
        div()
            .id("new-script-page")
            .size_full()
            .bg(cx.theme().background)
            .px(px(28.0))
            .flex()
            .flex_col()
            .on_key_down(move |event, _window, app| {
                if !event.keystroke.modifiers.platform {
                    return;
                }
                let target = match event.keystroke.key.as_str() {
                    "1" => Some(NewTabKind::Agent),
                    "2" => Some(NewTabKind::Terminal),
                    "3" => Some(NewTabKind::Command),
                    _ => None,
                };
                if let Some(target) = target {
                    app.stop_propagation();
                    tab_key_herdr.update(app, |this, cx| {
                        if let Some(ui) = this.new_agent_ui.as_mut() {
                            ui.tab = target;
                            cx.notify();
                        }
                    });
                }
            })
            .child(
                h_flex()
                    .w_full()
                    .justify_center()
                    .pt(px(24.0))
                    .child(tab_group),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .w_full()
                    .justify_center()
                    .items_baseline()
                    .child(
                        div()
                            .text_size(theme::FONT_HEADLINE)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().foreground)
                            .child(headline_text),
                    )
                    .child(project_picker)
                    .when(tab_kind != NewTabKind::Command, |row| {
                        row.child(
                            div()
                                .text_size(theme::FONT_HEADLINE)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().foreground)
                                .child("?"),
                        )
                    }),
            )
            .when(tab_kind != NewTabKind::Command, |page| {
                page.child(div().flex_1())
            })
            .when(tab_kind == NewTabKind::Terminal, |page| {
                page.child(
                    h_flex().w_full().justify_center().pb(px(12.0)).child(
                        v_flex()
                            .w_full()
                            .max_w(px(720.0))
                            .child(
                                h_flex()
                                    .w_full()
                                    .overflow_hidden()
                                    .rounded(px(13.0))
                                    .border_1()
                                    .border_color(cx.theme().border.opacity(0.58))
                                    .bg(if dark {
                                        cx.theme().foreground.opacity(0.035)
                                    } else {
                                        gpui::rgb(0xFFFFFF).into()
                                    })
                                    .py(px(10.0))
                                    .px(px(14.0))
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .child(
                                                Input::new(&terminal_input)
                                                    .w_full()
                                                    .appearance(false)
                                                    .p_0(),
                                            ),
                                    )
                                    .when(submitting, |row| {
                                        row.child(
                                            div()
                                                .size(px(26.0))
                                                .rounded_full()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .bg(cx.theme().secondary)
                                                .child(Spinner::new().xsmall()),
                                        )
                                    })
                                    .when(!submitting, |row| {
                                        row.child(
                                            div()
                                                .id("new-terminal-send")
                                                .size(px(26.0))
                                                .rounded_full()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .bg(if terminal_empty {
                                                    cx.theme().secondary.opacity(0.7)
                                                } else {
                                                    cx.theme().foreground
                                                })
                                                .when(!terminal_empty, |button| {
                                                    button
                                                        .cursor_default()
                                                        .hover(|style| style.opacity(0.9))
                                                        .active(|style| style.opacity(0.8))
                                                        .on_click(move |_, window, app| {
                                                            terminal_submit_herdr.update(
                                                                app,
                                                                |this, cx| {
                                                                    this.submit_new_terminal_command(
                                                                        window, cx,
                                                                    )
                                                                },
                                                            )
                                                        })
                                                })
                                                .child(
                                                    Icon::new(ComponentIconName::ArrowUp)
                                                        .with_size(px(16.0))
                                                        .text_color(if terminal_empty {
                                                            cx.theme().muted_foreground
                                                        } else {
                                                            cx.theme().background
                                                        }),
                                                ),
                                        )
                                    }),
                            )
                            .when(!terminal_empty, |col| {
                                let suggestions = shell_history_suggestions(&terminal_query, 5);
                                if suggestions.is_empty() {
                                    col
                                } else {
                                    col.child(
                                        v_flex()
                                            .w_full()
                                            .mt(px(4.0))
                                            .overflow_hidden()
                                            .rounded(px(8.0))
                                            .border_1()
                                            .border_color(cx.theme().border.opacity(0.4))
                                            .bg(if dark {
                                                cx.theme().foreground.opacity(0.025)
                                            } else {
                                                gpui::rgb(0xFFFFFF).into()
                                            })
                                            .children(suggestions.into_iter().enumerate().map(
                                                |(i, cmd)| {
                                                    let fill_input = terminal_input.clone();
                                                    let cmd_clone = cmd.clone();
                                                    div()
                                                        .id(SharedString::from(format!(
                                                            "hist-{i}"
                                                        )))
                                                        .w_full()
                                                        .px(px(12.0))
                                                        .py(px(6.0))
                                                        .text_size(theme::FONT_META)
                                                        .text_color(cx.theme().foreground)
                                                        .truncate()
                                                        .cursor_pointer()
                                                        .hover(|s| {
                                                            s.bg(cx.theme().foreground.opacity(0.05))
                                                        })
                                                        .when(i > 0, |row| {
                                                            row.border_t_1().border_color(
                                                                cx.theme().border.opacity(0.2),
                                                            )
                                                        })
                                                        .on_click(move |_, window, app| {
                                                            fill_input.update(app, |input, cx| {
                                                                input.set_value(
                                                                    cmd_clone.clone(),
                                                                    window,
                                                                    cx,
                                                                );
                                                            });
                                                        })
                                                        .child(cmd)
                                                },
                                            )),
                                    )
                                }
                            })
                            .child(
                                h_flex()
                                    .w_full()
                                    .h(px(28.0))
                                    .mt(px(4.0))
                                    .pl(px(10.0))
                                    .pr(px(10.0))
                                    .flex()
                                    .items_center()
                                    .gap(px(4.0))
                                    .text_size(theme::FONT_BODY)
                                    .child(
                                        Icon::new(ComponentIconName::Folder)
                                            .with_size(px(12.0))
                                            .text_color(cx.theme().muted_foreground),
                                    )
                                    .child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(project_name.clone()),
                                    ),
                            ),
                    ),
                )
            })
            .when(tab_kind == NewTabKind::Command, |page| {
                page.child(
                    v_flex()
                        .id("workflow-list-scroll")
                        .flex_1()
                        .w_full()
                        .min_h_0()
                        .items_center()
                        .pt(px(20.0))
                        .overflow_y_scroll()
                        .child(if command_scripts.is_empty() {
                            v_flex()
                                .w_full()
                                .max_w(px(480.0))
                                .items_center()
                                .gap(px(12.0))
                                .child(
                                    div()
                                        .text_size(theme::FONT_BODY)
                                        .text_color(cx.theme().muted_foreground)
                                        .child("No workflows for this project"),
                                )
                                .child(
                                    Button::new("new-script-add-workflow")
                                        .ghost()
                                        .small()
                                        .icon(ComponentIconName::Plus)
                                        .label("New Script")
                                        .on_click(move |_, window, app| {
                                            command_add_herdr.update(app, |this, cx| {
                                                this.open_new_script_dialog(window, cx);
                                            });
                                        }),
                                )
                                .into_any_element()
                        } else {
                            v_flex()
                                .w_full()
                                .max_w(px(560.0))
                                .overflow_hidden()
                                .rounded(px(12.0))
                                .border_1()
                                .border_color(cx.theme().border.opacity(0.5))
                                .bg(if dark {
                                    cx.theme().foreground.opacity(0.025)
                                } else {
                                    gpui::rgb(0xFFFFFF).into()
                                })
                                .children(command_scripts.iter().enumerate().map(
                                    |(index, script)| {
                                        let script_id = script.definition.id.clone();
                                        let script_name = script.definition.name.clone();
                                        let summary = if script.definition.description.is_empty()
                                        {
                                            script.definition.command_summary()
                                        } else {
                                            script.definition.description.clone()
                                        };
                                        let item_tags = script.definition.tags.clone();
                                        let run_herdr = command_run_herdr.clone();
                                        let edit_herdr = command_run_herdr.clone();
                                        let delete_herdr = command_run_herdr.clone();
                                        let edit_id = script.definition.id.clone();
                                        let delete_id = script.definition.id.clone();
                                        let row_delete_armed = script_delete_armed_id
                                            .as_deref()
                                            .is_some_and(|armed| armed == script.definition.id);
                                        let drag_id = script.definition.id.clone();
                                        let ctx_edit_herdr = command_run_herdr.clone();
                                        let ctx_delete_herdr = command_run_herdr.clone();
                                        let ctx_run_herdr = command_run_herdr.clone();
                                        let ctx_edit_id = script.definition.id.clone();
                                        let ctx_delete_id = script.definition.id.clone();
                                        let ctx_run_id = script.definition.id.clone();
                                        let drop_index = index;
                                        let drop_herdr = command_run_herdr.clone();
                                        let drag_over_bg = cx.theme().foreground.opacity(crate::theme::WASH_HOVER);
                                        let drag = WorkflowDrag {
                                            script_id: drag_id,
                                            label: script_name.clone(),
                                            position: Point::default(),
                                        };
                                        h_flex()
                                            .id(SharedString::from(format!(
                                                "wf-row-{index}"
                                            )))
                                            .group("workflow-row")
                                            .w_full()
                                            .py(px(10.0))
                                            .px(px(16.0))
                                            .items_center()
                                            .gap(px(10.0))
                                            .cursor_pointer()
                                            .hover(|s| {
                                                s.bg(cx.theme().foreground.opacity(0.04))
                                            })
                                            .active(|s| {
                                                s.bg(cx.theme().foreground.opacity(0.07))
                                            })
                                            .when(index > 0, |row| {
                                                row.border_t_1().border_color(
                                                    cx.theme().border.opacity(0.3),
                                                )
                                            })
                                            .on_drag(drag, |drag, position, _, cx| {
                                                let drag = drag.clone().position(position);
                                                cx.new(|_| drag)
                                            })
                                            .drag_over::<WorkflowDrag>(move |style, _, _, _| {
                                                style.bg(drag_over_bg)
                                            })
                                            .on_drop(move |drag: &WorkflowDrag, _, app| {
                                                drop_herdr.update(app, |this, cx| {
                                                    this.move_script_to_index(
                                                        drag.script_id.clone(),
                                                        drop_index,
                                                        cx,
                                                    );
                                                });
                                            })
                                            .on_click(move |_, window, app| {
                                                run_herdr.update(app, |this, cx| {
                                                    this.run_script_id(
                                                        script_id.clone(), window, cx,
                                                    );
                                                });
                                            })
                                            .context_menu(move |menu, _, _| {
                                                let e_herdr = ctx_edit_herdr.clone();
                                                let e_id = ctx_edit_id.clone();
                                                let r_herdr = ctx_run_herdr.clone();
                                                let r_id = ctx_run_id.clone();
                                                let d_herdr = ctx_delete_herdr.clone();
                                                let d_id = ctx_delete_id.clone();
                                                menu.item(
                                                    PopupMenuItem::new("Run").on_click(
                                                        move |_, window, app| {
                                                            r_herdr.update(
                                                                app,
                                                                |this, cx| {
                                                                    this.run_script_id(
                                                                        r_id.clone(),
                                                                        window,
                                                                        cx,
                                                                    );
                                                                },
                                                            );
                                                        },
                                                    ),
                                                )
                                                .item(
                                                    PopupMenuItem::new("Edit…").on_click(
                                                        move |_, window, app| {
                                                            e_herdr.update(
                                                                app,
                                                                |this, cx| {
                                                                    this.open_edit_script_dialog(
                                                                        e_id.clone(),
                                                                        window,
                                                                        cx,
                                                                    );
                                                                },
                                                            );
                                                        },
                                                    ),
                                                )
                                                .item(PopupMenuItem::separator())
                                                .item(
                                                    // P12-1: menu deletes go through a
                                                    // confirmation dialog first.
                                                    PopupMenuItem::new("Delete").on_click(
                                                            move |_, window, app| {
                                                                d_herdr.update(
                                                                    app,
                                                                    |this, cx| {
                                                                        this.confirm_delete_script(
                                                                            d_id.clone(),
                                                                            window,
                                                                            cx,
                                                                        );
                                                                    },
                                                                );
                                                            },
                                                        ),
                                                )
                                            })
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(1.0))
                                                    .child(
                                                        div()
                                                            .text_size(theme::FONT_BODY)
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(
                                                                cx.theme().foreground,
                                                            )
                                                            .truncate()
                                                            .child(script_name),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(theme::FONT_META)
                                                            .text_color(
                                                                cx.theme().muted_foreground,
                                                            )
                                                            .truncate()
                                                            .child(summary),
                                                    )
                                                    .when(!item_tags.is_empty(), |col| {
                                                        col.child(
                                                            h_flex()
                                                                .mt(px(3.0))
                                                                .gap(px(4.0))
                                                                .children(
                                                                    item_tags.iter().map(|tag| {
                                                                        div()
                                                                            .px(px(6.0))
                                                                            .py(px(1.0))
                                                                            .rounded(px(4.0))
                                                                            .bg(cx.theme().accent.opacity(0.1))
                                                                            .text_size(crate::theme::FONT_DECORATIVE)
                                                                            .text_color(cx.theme().accent)
                                                                            .child(tag.clone())
                                                                    }),
                                                                ),
                                                        )
                                                    }),
                                            )
                                            .child(
                                                h_flex()
                                                    .flex_shrink_0()
                                                    .gap(px(2.0))
                                                    .items_center()
                                                    .child(
                                                        div()
                                                            .id(SharedString::from(format!(
                                                                "wf-edit-{index}"
                                                            )))
                                                            .size(px(24.0))
                                                            .rounded(px(4.0))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .invisible()
                                                            .group_hover("workflow-row", |s| {
                                                                s.visible()
                                                            })
                                                            .hover(|s| {
                                                                s.bg(cx.theme().foreground.opacity(0.08))
                                                            })
                                                            .active(|s| {
                                                                s.bg(cx.theme().foreground.opacity(0.12))
                                                            })
                                                            .on_click(move |_, window, app| {
                                                                app.stop_propagation();
                                                                edit_herdr.update(
                                                                    app,
                                                                    |this, cx| {
                                                                        this.open_edit_script_dialog(
                                                                            edit_id.clone(),
                                                                            window,
                                                                            cx,
                                                                        );
                                                                    },
                                                                );
                                                            })
                                                            .child(
                                                                Icon::empty()
                                                                    .path("icons/pencil.svg")
                                                                    .with_size(px(12.0))
                                                                    .text_color(
                                                                        cx.theme().muted_foreground,
                                                                    ),
                                                            ),
                                                    )
                                                    .child(
                                                        // P12-1: two-click armed confirmation — first click turns
                                                        // "Delete?" danger state, second click deletes.
                                                        div()
                                                            .id(SharedString::from(format!(
                                                                "wf-del-{index}"
                                                            )))
                                                            .when(row_delete_armed, |btn| {
                                                                btn.visible()
                                                                    .h(px(24.0))
                                                                    .px(px(8.0))
                                                                    .bg(cx.theme().danger.opacity(0.15))
                                                            })
                                                            .when(!row_delete_armed, |btn| {
                                                                btn.size(px(24.0)).invisible().group_hover(
                                                                    "workflow-row",
                                                                    |s| s.visible(),
                                                                )
                                                            })
                                                            .rounded(px(4.0))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .hover(|s| {
                                                                s.bg(cx.theme().danger.opacity(0.1))
                                                            })
                                                            .active(|s| {
                                                                s.bg(cx.theme().danger.opacity(0.18))
                                                            })
                                                            .on_click(move |_, _, app| {
                                                                app.stop_propagation();
                                                                delete_herdr.update(
                                                                    app,
                                                                    |this, cx| {
                                                                        let already_armed = this
                                                                            .new_agent_ui
                                                                            .as_ref()
                                                                            .and_then(|ui| {
                                                                                ui.script_delete_armed_id
                                                                                    .as_deref()
                                                                            })
                                                                            .is_some_and(
                                                                                |armed| {
                                                                                    armed
                                                                                        == delete_id
                                                                                            .as_str()
                                                                                },
                                                                            );
                                                                        if !already_armed {
                                                                            if let Some(ui) =
                                                                                this.new_agent_ui
                                                                                    .as_mut()
                                                                            {
                                                                                ui.script_delete_armed_id =
                                                                                    Some(
                                                                                        delete_id
                                                                                            .clone(),
                                                                                    );
                                                                            }
                                                                            cx.notify();
                                                                            return;
                                                                        }
                                                                        if let Some(ui) =
                                                                            this.new_agent_ui.as_mut()
                                                                        {
                                                                            ui.script_delete_armed_id =
                                                                                None;
                                                                        }
                                                                        this.delete_script_id(
                                                                            delete_id.clone(),
                                                                            cx,
                                                                        );
                                                                    },
                                                                );
                                                            })
                                                            .when(row_delete_armed, |btn| {
                                                                btn.text_size(theme::FONT_META)
                                                                    .text_color(
                                                                        cx.theme().danger,
                                                                    )
                                                                    .child("Delete?")
                                                            })
                                                            .when(!row_delete_armed, |btn| {
                                                                btn.child(
                                                                    Icon::new(
                                                                        ComponentIconName::Close,
                                                                    )
                                                                    .with_size(px(12.0))
                                                                    .text_color(
                                                                        cx.theme().danger,
                                                                    ),
                                                                )
                                                            }),
                                                    )
                                                    .child(
                                                        Icon::new(ComponentIconName::ArrowRight)
                                                            .with_size(px(14.0))
                                                            .text_color(
                                                                cx.theme()
                                                                    .muted_foreground
                                                                    .opacity(0.6),
                                                            ),
                                                    ),
                                            )
                                    },
                                ))
                                .into_any_element()
                        }),
                )
            })
            .when(tab_kind == NewTabKind::Agent, |page| {
                // Shared AgentComposer: the single implementation of card geometry,
                // input, attachment bar, and send button.
                // New Agent only injects its own chips, the @ reference entry, and
                // submit/remove callbacks.
                let mut composer = AgentComposer::new("new-agent-composer", prompt.clone())
                    .attachments(attachments.clone())
                    .left_control(agent_picker);
                if agent_supports_native_plan_mode(agent) {
                    composer = composer.left_control(mode_picker);
                }
                if agent_supports_permission_flags(agent) {
                    composer = composer.left_control(permission_picker);
                }
                if !submitting {
                    // @ file reference entry: focus the prompt and insert @ at the
                    // cursor;
                    // subsequent keystrokes are handled by the component's native
                    // completion menu (fuzzy file list).
                    let reference_herdr = attach_herdr.clone();
                    composer = composer.trailing(
                        div()
                            .id("new-agent-reference")
                            .size(px(26.0))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .mr(px(6.0))
                            .bg(cx.theme().secondary.opacity(0.7))
                            .cursor_default()
                            .hover(|style| style.opacity(0.9))
                            .active(|style| style.opacity(0.8))
                            .tooltip(crate::ui::tooltip::tooltip_fn(
                                "Reference a project file (@)",
                            ))
                            .on_click(move |_, window, app| {
                                reference_herdr.update(app, |this, cx| {
                                    let Some(ui) = this.new_agent_ui.as_ref() else {
                                        return;
                                    };
                                    let prompt = ui.prompt.clone();
                                    prompt.update(cx, |state, input_cx| {
                                        state.focus(window, input_cx);
                                        state.insert("@", window, input_cx);
                                    });
                                });
                            })
                            .child(
                                Icon::empty()
                                    .path("icons/paperclip.svg")
                                    .with_size(px(14.0))
                                    .text_color(cx.theme().muted_foreground),
                            ),
                    );
                }
                page.child(
                    h_flex().w_full().justify_center().pb(px(12.0)).child(
                        composer
                            .send_state(composer_send_state(submitting, !prompt_empty))
                            .on_send(move |window, app| {
                                submit_herdr.update(app, |this, cx| {
                                    this.submit_new_agent(window, cx);
                                });
                            })
                            .on_remove_attachment(move |index, app| {
                                attach_herdr.update(app, |this, cx| {
                                    this.remove_new_agent_attachment(index, cx);
                                });
                            })
                            .footer(vec![
                                project_chip.into_any_element(),
                                branch_picker.into_any_element(),
                                div().flex_1().into_any_element(),
                            ]),
                    ),
                )
            })
            .into_any_element()
    }
}
