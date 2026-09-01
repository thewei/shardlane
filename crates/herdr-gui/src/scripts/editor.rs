//! [INPUT]: Main-crate imports and sibling-module shared items passed through the scripts module root (super) via the `use super::*` chain
//! [OUTPUT]: Provides the script editor and surface entries (Script dialog, keybinding parsing, run/open entries, duplicate-name soft warning)
//! [POS]: The editor-surface slice of the scripts module, mechanically split out of scripts.rs
use super::model::{
    default_script_icon, next_script_id, normalize_script_keybinding, script_icon_slug,
};
use super::*;

fn script_dialog_field(label: &'static str, control: impl IntoElement, cx: &App) -> AnyElement {
    v_flex()
        .w_full()
        .gap(SPACE_ICON)
        .child(
            div()
                .text_size(theme::FONT_META)
                .font_weight(FontWeight::MEDIUM)
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(control)
        .into_any_element()
}

impl ShardlaneApp {
    pub(crate) fn active_project_scripts(&self) -> Vec<ScriptRecord> {
        let Some(runtime_workspace_id) = self.active_workspace_id() else {
            return Vec::new();
        };
        let project_index = build_project_index(&self.state, &self.scripts);
        let project_path = project_index
            .for_runtime_id(runtime_workspace_id)
            .and_then(|project| project.project_path.as_deref());
        let mut scripts = self
            .scripts
            .scripts
            .iter()
            .filter(|script| {
                project_path.is_some_and(|project_path| {
                    project_paths_match(&script.project_path, project_path)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        scripts.sort_by(|left, right| {
            right
                .last_run_at_ms
                .cmp(&left.last_run_at_ms)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        scripts
    }

    pub(crate) fn script_id_for_keybinding(&self, key: &Keystroke) -> Option<String> {
        let keybinding = key_name(key);
        self.active_project_scripts()
            .into_iter()
            .find(|script| script.keybinding.as_deref() == Some(keybinding.as_str()))
            .map(|script| script.id.clone())
    }

    pub(crate) fn run_script_id(
        &mut self,
        script_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let running = self
            .scripts
            .scripts
            .iter()
            .find(|script| script.id == script_id)
            .is_some_and(|script| {
                matches!(
                    script.runtime.status,
                    ScriptStatus::Starting | ScriptStatus::Running
                ) && script.pane_id.is_some()
            });
        if running {
            self.focus_script_id(script_id, window, cx);
        } else {
            self.start_script_id(script_id, cx);
        }
    }

    pub(crate) fn toggle_services_section(&mut self, cx: &mut Context<Self>) {
        self.services_collapsed = !self.services_collapsed;
        self.config.ui.sidebar.services_collapsed = self.services_collapsed;
        self.save_config();
        // Disclosure is presentation-only. Service discovery owns its own low-frequency cache;
        // expanding/collapsing this section must never force pane.process_info/lsof work.
        self.notify_sidebar(cx);
        cx.notify();
    }

    pub(crate) fn open_new_script_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_script_editor(None, window, cx);
    }

    pub(crate) fn open_edit_script_dialog(
        &mut self,
        script_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_script_editor(Some(script_id), window, cx);
    }

    fn open_script_editor(
        &mut self,
        editing_script_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root_has_dialog = window.has_active_dialog(cx);
        if self.script_dialog_open || root_has_dialog {
            return;
        }

        let existing_script = editing_script_id
            .as_ref()
            .and_then(|id| self.scripts.scripts.iter().find(|t| &t.id == id).cloned());

        let (workspace_id, project_path) = if let Some(ref existing) = existing_script {
            (existing.workspace_id.clone(), existing.project_path.clone())
        } else {
            let workspace = self.active_workspace().cloned();
            let Some(workspace) = workspace else {
                window.push_notification("Add or select a Project before adding a Script", cx);
                return;
            };
            let Some(project_path) = build_project_index(&self.state, &self.scripts)
                .for_runtime_id(&workspace.workspace_id)
                .and_then(|scope| scope.project_path.clone())
            else {
                window.push_notification("Selected Project has no usable path", cx);
                return;
            };
            (workspace.workspace_id.clone(), project_path)
        };

        let initial_name = existing_script
            .as_ref()
            .map(|t| t.name.clone())
            .unwrap_or_default();
        let initial_keybinding = existing_script
            .as_ref()
            .and_then(|t| t.keybinding.clone())
            .unwrap_or_default();
        let initial_command = existing_script
            .as_ref()
            .map(|t| t.command.clone())
            .unwrap_or_default();
        let initial_icon = existing_script
            .as_ref()
            .map(|t| t.icon.clone())
            .unwrap_or_else(default_script_icon);
        let initial_kind = existing_script.as_ref().map(|t| t.kind).unwrap_or_default();
        let initial_one_shot = existing_script.as_ref().map(|t| t.one_shot).unwrap_or(true);
        let initial_close_on_complete = existing_script
            .as_ref()
            .map(|t| t.close_on_complete)
            .unwrap_or(true);

        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Script name"));
        let keybinding =
            cx.new(|cx| InputState::new(window, cx).placeholder("Optional, e.g. cmd+shift+r"));
        let command = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("One command per line, e.g. pnpm install")
                .multi_line(true)
                .soft_wrap(true)
        });

        if !initial_name.is_empty() {
            name.update(cx, |state, input_cx| {
                state.set_value(initial_name, window, input_cx);
            });
        }
        if !initial_keybinding.is_empty() {
            keybinding.update(cx, |state, input_cx| {
                state.set_value(initial_keybinding, window, input_cx);
            });
        }
        if !initial_command.is_empty() {
            command.update(cx, |state, input_cx| {
                state.set_value(initial_command, window, input_cx);
            });
        }

        let icon_row = match script_icon_slug(&initial_icon).as_str() {
            "asterisk" => 1,
            "bot" => 2,
            "star" => 3,
            _ => 0,
        };
        let icon = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(vec![
                    "Terminal".to_string(),
                    "Asterisk".to_string(),
                    "Bot".to_string(),
                    "Star".to_string(),
                ]),
                Some(IndexPath::default().row(icon_row)),
                window,
                cx,
            )
        });

        let kind_row = match initial_kind {
            ScriptKind::Command => 0,
            ScriptKind::Service => 1,
            ScriptKind::Debugger => 2,
        };
        let kind = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(vec![
                    "Command".to_string(),
                    "Service".to_string(),
                    "Debugger".to_string(),
                ]),
                Some(IndexPath::default().row(kind_row)),
                window,
                cx,
            )
        });

        let initial_tags = existing_script
            .as_ref()
            .map(|t| t.tags.join(", "))
            .unwrap_or_default();
        let tags = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. dev, build, deploy"));
        if !initial_tags.is_empty() {
            tags.update(cx, |state, input_cx| {
                state.set_value(initial_tags, window, input_cx);
            });
        }

        let one_shot = Rc::new(Cell::new(initial_one_shot));
        let close_on_complete = Rc::new(Cell::new(initial_close_on_complete));

        let app = cx.entity();
        let close_app = app.clone();
        let name_input = name.clone();
        let keybinding_input = keybinding.clone();
        let tags_input = tags.clone();
        let command_input = command.clone();
        let icon_input = icon.clone();
        let kind_input = kind.clone();
        let editing_id = editing_script_id.clone();
        let dialog_title = if editing_script_id.is_some() {
            "Edit Script"
        } else {
            "New Script"
        };

        name.update(cx, |state, input_cx| state.focus(window, input_cx));
        self.script_dialog_open = true;
        self.clear_ime_state();
        self.sync_terminal_application_focus(cx);
        cx.notify();

        let dialog_width =
            responsive_dialog_width(window.bounds().size.width.to_f64(), 0.88, 360.0, 620.0);
        window.open_dialog(cx, move |dialog, _window, cx| {
            let submit_app = app.clone();
            let submit_name = name_input.clone();
            let submit_keybinding = keybinding_input.clone();
            let submit_tags = tags_input.clone();
            let submit_command = command_input.clone();
            let submit_icon = icon_input.clone();
            let submit_kind = kind_input.clone();
            let submit_workspace = workspace_id.clone();
            let submit_project_path = project_path.clone();
            let submit_one_shot = one_shot.clone();
            let submit_close_on_complete = close_on_complete.clone();
            let submit_editing_id = editing_id.clone();
            let one_shot_toggle = one_shot.clone();
            let close_toggle = close_on_complete.clone();
            let close_app = close_app.clone();
            dialog
                .title(dialog_title)
                .w(px(dialog_width))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Save")
                        .cancel_text("Cancel"),
                )
                .footer(|ok, cancel, window, cx| vec![cancel(window, cx), ok(window, cx)])
                .child(
                    v_flex()
                        .w_full()
                        .gap(DIALOG_CONTENT_GAP)
                        .child(
                            h_flex()
                                .w_full()
                                .gap_3()
                                .child(script_dialog_field(
                                    "Icon",
                                    Select::new(&icon_input).w(px(132.0)),
                                    cx,
                                ))
                                .child(script_dialog_field(
                                    "Type",
                                    Select::new(&kind_input).w(px(150.0)),
                                    cx,
                                ))
                                .child(script_dialog_field(
                                    "Name",
                                    Input::new(&name_input).w_full(),
                                    cx,
                                )),
                        )
                        .child(script_dialog_field(
                            "Keybinding",
                            Input::new(&keybinding_input).w_full(),
                            cx,
                        ))
                        .child(script_dialog_field(
                            "Tags · comma-separated",
                            Input::new(&tags_input).w_full(),
                            cx,
                        ))
                        .child(script_dialog_field(
                            "Commands · one command per line",
                            Input::new(&command_input).w_full().h(px(132.0)),
                            cx,
                        ))
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .gap_4()
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child("One-time script")
                                        .child(
                                            div()
                                                .text_size(theme::FONT_META)
                                                .text_color(cx.theme().muted_foreground)
                                                .child("Treat completion as the end of this run."),
                                        ),
                                )
                                .child(
                                    Toggle::new(
                                        "script-one-shot",
                                        ControlSurface::from_app_theme(cx),
                                    )
                                    .checked(one_shot_toggle.get())
                                    .on_change(move |checked, _, _| {
                                        one_shot_toggle.set(checked)
                                    }),
                                ),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .gap_4()
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child("Close Pane when complete")
                                        .child(
                                            div()
                                                .text_size(theme::FONT_META)
                                                .text_color(cx.theme().muted_foreground)
                                                .child("For one-time runs, close the Script Pane after the command returns."),
                                        ),
                                )
                                .child(
                                    Toggle::new(
                                        "script-close-on-complete",
                                        ControlSurface::from_app_theme(cx),
                                    )
                                    .checked(close_toggle.get())
                                    .on_change(move |checked, _, _| {
                                        close_toggle.set(checked)
                                    }),
                                ),
                        ),
                )
                .on_ok(move |_, window, app| {
                    let name = submit_name.read(app).value().trim().to_string();
                    let command = submit_command.read(app).value().trim().to_string();
                    let raw_keybinding = submit_keybinding.read(app).value().trim().to_string();
                    let keybinding = match normalize_script_keybinding(&raw_keybinding) {
                        Ok(value) => value,
                        Err(error) => {
                            window.push_notification(error, app);
                            return false;
                        }
                    };
                    let icon = submit_icon
                        .read(app)
                        .selected_value()
                        .cloned()
                        .map(|value| script_icon_slug(&value))
                        .unwrap_or_else(default_script_icon);
                    let Some(kind) = submit_kind
                        .read(app)
                        .selected_value()
                        .and_then(|value| ScriptKind::parse(value))
                    else {
                        window.push_notification("Select a Script type", app);
                        return false;
                    };
                    if name.is_empty() || command.lines().all(|line| line.trim().is_empty()) {
                        window.push_notification("Script name and at least one command are required", app);
                        return false;
                    }
                    if let Some(keybinding) = keybinding.as_deref() {
                        let conflict = submit_app.read(app).scripts.scripts.iter().any(|script| {
                            Some(&script.id) != submit_editing_id.as_ref()
                                && script.keybinding.as_deref() == Some(keybinding)
                                && project_paths_match(&script.project_path, &submit_project_path)
                        });
                        if conflict {
                            window.push_notification(
                                format!("Keybinding {keybinding} is already used by a Script in this Project"),
                                app,
                            );
                            return false;
                        }
                    }
                    let tags: Vec<String> = submit_tags
                        .read(app)
                        .value()
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    // P12-5: duplicate-name soft warning within the same Project
                    // (does not block saving; duplicates only hurt visual
                    // distinction).
                    let duplicate_name = submit_app
                        .read(app)
                        .scripts
                        .scripts
                        .iter()
                        .any(|script| {
                            Some(&script.id) != submit_editing_id.as_ref()
                                && script.name == name
                                && project_paths_match(
                                    &script.project_path,
                                    &submit_project_path,
                                )
                        });
                    let saved_name = name.clone();
                    submit_app.update(app, |view, cx| {
                        if let Some(ref script_id) = submit_editing_id {
                            if let Some(record) = view.scripts.get_mut(script_id) {
                                record.name = name;
                                record.icon = icon;
                                record.keybinding = keybinding;
                                record.command = command;
                                record.kind = kind;
                                record.one_shot = submit_one_shot.get();
                                record.close_on_complete = submit_close_on_complete.get();
                                record.tags = tags;
                            }
                        } else {
                            let record = ScriptRecord {
                                definition: ScriptDefinition {
                                    id: next_script_id(),
                                    project_path: submit_project_path.clone(),
                                    name,
                                    description: String::new(),
                                    icon,
                                    keybinding,
                                    command,
                                    kind,
                                    one_shot: submit_one_shot.get(),
                                    close_on_complete: submit_close_on_complete.get(),
                                    last_run_at_ms: None,
                                    tags,
                                },
                                workspace_id: submit_workspace.clone(),
                                ..ScriptRecord::default()
                            };
                            view.scripts.scripts.push(record);
                        }
                        view.scripts.save();
                        view.notify_sidebar(cx);
                        cx.notify();
                    });
                    if duplicate_name {
                        window.push_notification(
                            format!(
                                "Saved. Note: another Script in this Project is also named \"{saved_name}\""
                            ),
                            app,
                        );
                    }
                    true
                })
                .on_close(move |_, _, app| {
                    close_app.update(app, |view, cx| {
                        view.script_dialog_open = false;
                        view.sync_terminal_application_focus(cx);
                        cx.notify();
                    });
                })
        });
    }
}
