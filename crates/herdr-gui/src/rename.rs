//! Presentation layer for the Project/Tab/Pane rename Dialog.
//!
//! [INPUT]: Depends on HerdrClient (workspace/tab/pane rename) and gpui-component (Input/Dialog controls)
//! [OUTPUT]: Exposes rename actions such as rename_active_project, rename_active_tab, rename_focused_pane
//! [POS]: Rename interaction dialog split out of main.rs

use super::*;
use gpui_component::{
    dialog::DialogButtonProps,
    input::{Input, InputState},
};

#[derive(Clone)]
enum RenameTarget {
    /// A runtime workspace inside the bound instance (legacy in-instance row).
    Project(String),
    /// A Project REGISTRY entry (the bound workspace itself). Renaming an
    /// adopted entry promotes it to a Shardlane-maintained workspace.
    RegistryProject(String),
    Tab(String),
    Pane(String),
}

impl ShardlaneApp {
    pub(super) fn rename_active_project(
        &mut self,
        _: &RenameProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Renames the BOUND Project registry entry (the workspace itself, not a
        // runtime row inside it). Renaming an adopted workspace promotes it to
        // a Shardlane-maintained one (adopted=false).
        if let Some(binding) = self.bound_project() {
            let project_id = binding.project_id.clone();
            let name = binding.project_name.clone();
            self.open_rename_dialog(
                RenameTarget::RegistryProject(project_id),
                "Rename Workspace",
                name,
                window,
                cx,
            );
        }
    }

    pub(super) fn rename_active_tab(
        &mut self,
        _: &RenameTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let tab_id = tab.tab_id.clone();
        let label = self.tab_title(tab);
        self.open_tab_rename(tab_id, label, window, cx);
    }

    pub(super) fn open_project_rename(
        &mut self,
        workspace_id: String,
        current_label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_rename_dialog(
            RenameTarget::Project(workspace_id),
            "Rename Project",
            current_label,
            window,
            cx,
        );
    }

    pub(super) fn open_tab_rename(
        &mut self,
        tab_id: String,
        current_label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_rename_dialog(
            RenameTarget::Tab(tab_id),
            "Rename Tab",
            current_label,
            window,
            cx,
        );
    }

    pub(super) fn open_pane_rename(
        &mut self,
        pane_id: String,
        current_label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_rename_dialog(
            RenameTarget::Pane(pane_id),
            "Rename Pane",
            current_label,
            window,
            cx,
        );
    }

    fn open_rename_dialog(
        &mut self,
        target: RenameTarget,
        title: &'static str,
        current_label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.rename_open || window.has_active_dialog(cx) {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Name"));
        input.update(cx, |state, input_cx| {
            state.set_value(current_label, window, input_cx);
            state.focus(window, input_cx);
        });

        self.rename_open = true;
        self.clear_ime_state();
        self.sync_terminal_application_focus(cx);
        cx.notify();

        let herdr = cx.entity();
        let dialog_input = input.clone();
        let close_herdr = herdr.clone();
        let dialog_width =
            responsive_dialog_width(window.bounds().size.width.to_f64(), 0.84, 300.0, 420.0);
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let submit_herdr = herdr.clone();
            let submit_input = dialog_input.clone();
            let submit_target = target.clone();
            let close_herdr = close_herdr.clone();
            dialog
                .title(title)
                .w(px(dialog_width))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Save")
                        .cancel_text("Cancel"),
                )
                .footer(|ok, cancel, window, cx| vec![cancel(window, cx), ok(window, cx)])
                .child(Input::new(&dialog_input).w_full())
                .on_ok(move |_, window, app| {
                    let label = submit_input.read(app).value().trim().to_string();
                    if label.is_empty() {
                        return false;
                    }
                    submit_herdr.update(app, |view, cx| {
                        view.apply_rename(submit_target.clone(), label.clone(), window, cx);
                    });
                    true
                })
                .on_close(move |_, _, app| {
                    close_herdr.update(app, |view, cx| {
                        view.rename_open = false;
                        view.sync_terminal_application_focus(cx);
                        cx.notify();
                    });
                })
        });
    }

    fn apply_rename(
        &mut self,
        target: RenameTarget,
        label: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Registry-level rename: the BOUND workspace entry itself. Renaming an
        // adopted workspace promotes it to a Shardlane-maintained one
        // (adopted=false), and best-effort renames the instance's focused
        // runtime workspace so the in-TUI label matches.
        if let RenameTarget::RegistryProject(project_id) = &target {
            // Rename = a cosmetic display-name override (herdr sessions have no
            // rename); the Herdr instance itself is untouched.
            let key_session = (project_id.as_str() != "default").then(|| project_id.clone());
            self.shared
                .set_display_name(key_session.as_deref(), label.clone());
            let mut renamed_bound = false;
            if let Some(binding) = self.binding.as_mut() {
                if binding.project_id == *project_id {
                    binding.project_name = label.clone();
                    renamed_bound = true;
                }
            }
            if renamed_bound {
                self.update_window_title(_window);
            }
            let rename = self
                .client
                .as_ref()
                .zip(self.state.focused_workspace_id.clone())
                .map(|(client, workspace_id)| client.rename_workspace(&workspace_id, &label));
            let _ = rename;
            cx.notify();
            return;
        }
        let result = match &target {
            RenameTarget::RegistryProject(_) => unreachable!("handled above"),
            RenameTarget::Project(workspace_id) => self
                .client
                .as_ref()
                .ok_or_else(|| "Herdr runtime is unavailable".to_string())
                .and_then(|client| {
                    client
                        .rename_workspace(workspace_id, &label)
                        .map_err(|error| error.to_string())
                }),
            RenameTarget::Tab(tab_id) => self
                .client
                .as_ref()
                .ok_or_else(|| "Herdr runtime is unavailable".to_string())
                .and_then(|client| {
                    client
                        .rename_tab(tab_id, &label)
                        .map_err(|error| error.to_string())
                }),
            RenameTarget::Pane(pane_id) => self
                .client
                .as_ref()
                .ok_or_else(|| "Herdr runtime is unavailable".to_string())
                .and_then(|client| {
                    client
                        .rename_pane(pane_id, &label)
                        .map_err(|error| error.to_string())
                }),
        };
        if let Err(error) = result {
            self.status = ConnectionStatus::Offline(error);
        } else {
            match target {
                RenameTarget::RegistryProject(_) => {}
                RenameTarget::Project(workspace_id) => {
                    if let Some(workspace) = self
                        .state
                        .workspaces
                        .iter_mut()
                        .find(|workspace| workspace.workspace_id == workspace_id)
                    {
                        workspace.label = Some(label.clone());
                    }
                }
                RenameTarget::Tab(tab_id) => {
                    if let Some(tab) = self.state.tabs.iter_mut().find(|tab| tab.tab_id == tab_id) {
                        tab.label = Some(label.clone());
                    }
                }
                RenameTarget::Pane(pane_id) => {
                    if let Some(pane) = self
                        .state
                        .panes
                        .iter_mut()
                        .find(|pane| pane.pane_id == pane_id)
                    {
                        pane.label = Some(label.clone());
                    }
                    self.sidebar_pane.update(cx, |sidebar, cx| {
                        sidebar.update_cached_pane_label(&pane_id, &label, cx)
                    });
                }
            }
            self.notify_sidebar(cx);
        }
        cx.notify();
    }
}
