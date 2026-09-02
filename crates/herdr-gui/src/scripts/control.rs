//! [INPUT]: Main-crate imports and sibling-module shared items passed through the scripts module root (super) via the `use super::*` chain
//! [OUTPUT]: Provides script lifecycle control (start/stop/restart/delete/move/focus) and the menu-entry delete confirmation dialog (confirm_delete_script); delete also clears new_agent-domain inline-arming leftovers (P12-1 hardening)
//! [POS]: The lifecycle-control slice of the scripts module, mechanically split out of scripts.rs; deleting no longer kills the pane along with it (P12-1)
use super::launch::{launch_script_runtime, resolve_script_project_launch_target};
use super::observe::now_ms;
use super::*;

impl ShardlaneApp {
    pub(crate) fn start_script_id(&mut self, script_id: String, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(script) = self
            .scripts
            .scripts
            .iter()
            .find(|script| script.id == script_id)
            .cloned()
        else {
            return;
        };
        let project_index = build_project_index(&self.state, &self.scripts);
        let launch_target = match resolve_script_project_launch_target(&script, &project_index) {
            Ok(target) => target,
            Err(error) => {
                if let Some(record) = self.scripts.get_mut(&script_id) {
                    record.runtime.status = ScriptStatus::Failed;
                    record.runtime.last_error = Some(error.clone());
                }
                self.status = ConnectionStatus::Offline(error);
                self.notify_sidebar(cx);
                return;
            }
        };
        let mut persistence_changed = false;
        if let Some(record) = self.scripts.get_mut(&script_id) {
            record.last_run_at_ms = Some(now_ms());
            persistence_changed = true;
            record.runtime.status = ScriptStatus::Starting;
            record.runtime.last_error = None;
            record.runtime.ports.clear();
            record.runtime.pid = None;
            record.runtime.started_at_ms = Some(now_ms());
        }
        if persistence_changed {
            self.scripts.save();
        }
        self.notify_sidebar(cx);

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(
                    async move { launch_script_runtime(client.as_ref(), &script, launch_target) },
                )
                .await;
            let _ = this.update(cx, |view, cx| {
                let mut runtime_materialized = false;
                let mut persistence_changed = false;
                if let Some(record) = view.scripts.get_mut(&script_id) {
                    match result {
                        Ok((workspace_id, tab_id, pane_id)) => {
                            persistence_changed = record.workspace_id != workspace_id
                                || record.tab_id.as_deref() != Some(tab_id.as_str())
                                || record.pane_id.as_deref() != Some(pane_id.as_str());
                            record.workspace_id = workspace_id;
                            record.tab_id = Some(tab_id);
                            record.pane_id = Some(pane_id);
                            record.runtime.status = ScriptStatus::Starting;
                            record.runtime.last_error = None;
                            runtime_materialized = true;
                        }
                        Err(error) => {
                            record.runtime.status = ScriptStatus::Failed;
                            record.runtime.last_error = Some(error.clone());
                            view.status = ConnectionStatus::Offline(error);
                        }
                    }
                }
                if runtime_materialized {
                    let _ = view.script_monitor_wake.try_send(());
                }
                if persistence_changed {
                    view.scripts.save();
                }
                view.schedule_recovery_refresh(cx);
                view.notify_sidebar(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn stop_script_id(&mut self, script_id: String, cx: &mut Context<Self>) {
        let pane_id = self
            .scripts
            .scripts
            .iter()
            .find(|script| script.id == script_id)
            .and_then(|script| script.pane_id.clone());
        let mut persistence_changed = false;
        if let Some(record) = self.scripts.get_mut(&script_id) {
            persistence_changed = record.tab_id.is_some() || record.pane_id.is_some();
            record.runtime = ScriptRuntimeProjection::default();
            record.tab_id = None;
            record.pane_id = None;
        }
        if persistence_changed {
            self.scripts.save();
        }
        self.notify_sidebar(cx);
        let Some(client) = self.client.clone() else {
            return;
        };
        if let Some(pane_id) = pane_id {
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        client
                            .close_pane(&pane_id)
                            .map_err(|error| error.to_string())
                    })
                    .await;
                let _ = this.update(cx, |view, cx| {
                    if let Err(error) = result {
                        view.status = ConnectionStatus::Offline(error);
                    }
                    view.schedule_recovery_refresh(cx);
                    view.notify_sidebar(cx);
                });
            })
            .detach();
        }
    }

    pub(crate) fn restart_script_id(&mut self, script_id: String, cx: &mut Context<Self>) {
        let old_pane = self
            .scripts
            .scripts
            .iter()
            .find(|script| script.id == script_id)
            .and_then(|script| script.pane_id.clone());
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(script) = self
            .scripts
            .scripts
            .iter()
            .find(|script| script.id == script_id)
            .cloned()
        else {
            return;
        };
        let project_index = build_project_index(&self.state, &self.scripts);
        let launch_target = match resolve_script_project_launch_target(&script, &project_index) {
            Ok(target) => target,
            Err(error) => {
                if let Some(record) = self.scripts.get_mut(&script_id) {
                    record.runtime.status = ScriptStatus::Failed;
                    record.runtime.last_error = Some(error.clone());
                }
                self.status = ConnectionStatus::Offline(error);
                self.notify_sidebar(cx);
                return;
            }
        };
        let mut persistence_changed = false;
        if let Some(record) = self.scripts.get_mut(&script_id) {
            record.last_run_at_ms = Some(now_ms());
            persistence_changed = true;
            record.runtime.status = ScriptStatus::Starting;
            record.runtime.pid = None;
            record.runtime.ports.clear();
            record.runtime.started_at_ms = Some(now_ms());
            record.runtime.last_error = None;
        }
        if persistence_changed {
            self.scripts.save();
        }
        self.notify_sidebar(cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if let Some(pane_id) = old_pane {
                        let _ = client.close_pane(&pane_id);
                    }
                    launch_script_runtime(client.as_ref(), &script, launch_target)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                let mut runtime_materialized = false;
                let mut persistence_changed = false;
                if let Some(record) = view.scripts.get_mut(&script_id) {
                    match result {
                        Ok((workspace_id, tab_id, pane_id)) => {
                            persistence_changed = record.workspace_id != workspace_id
                                || record.tab_id.as_deref() != Some(tab_id.as_str())
                                || record.pane_id.as_deref() != Some(pane_id.as_str());
                            record.workspace_id = workspace_id;
                            record.tab_id = Some(tab_id);
                            record.pane_id = Some(pane_id);
                            record.runtime.status = ScriptStatus::Starting;
                            record.runtime.last_error = None;
                            runtime_materialized = true;
                        }
                        Err(error) => {
                            persistence_changed =
                                record.tab_id.is_some() || record.pane_id.is_some();
                            record.tab_id = None;
                            record.pane_id = None;
                            record.runtime.status = ScriptStatus::Failed;
                            record.runtime.last_error = Some(error.clone());
                            view.status = ConnectionStatus::Offline(error);
                        }
                    }
                }
                if runtime_materialized {
                    let _ = view.script_monitor_wake.try_send(());
                }
                if persistence_changed {
                    view.scripts.save();
                }
                view.schedule_recovery_refresh(cx);
                view.notify_sidebar(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Delete the Script definition (P12-1: no longer closes the pane along with
    /// it — the runtime belongs to Herdr, and hours of scrollback must not be
    /// destroyed by a single click; the pane remains a normal pane for the user
    /// to close).
    /// Destructive entries must go through confirm_delete_script first
    /// (two-click / dialog confirmation).
    pub(crate) fn delete_script_id(&mut self, script_id: String, cx: &mut Context<Self>) {
        self.scripts.scripts.retain(|script| script.id != script_id);
        self.scripts.save();
        // Self-clearing of the inline two-click arming state: deleting via the
        // menu confirmation path without clearing would leave Some(deleted id)
        // behind (no risk of wrong deletion, just stale state; clear it
        // uniformly).
        self.clear_script_delete_arming();
        self.notify_sidebar(cx);
        cx.notify();
    }

    /// Delete-confirmation dialog for the menu entry (P12-1): popup menus close
    /// on click, leaving no place for two-click arming, so this uses a
    /// confirmation dialog. Degrades
    /// safely when a dialog is already open (no delete, user notified), avoiding
    /// stacked dialogs.
    pub(crate) fn confirm_delete_script(
        &mut self,
        script_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            window.push_notification("Close the open dialog before deleting a Script", cx);
            return;
        }
        let Some(script) = self
            .scripts
            .scripts
            .iter()
            .find(|script| script.id == script_id)
        else {
            return;
        };
        let script_name = script.name.clone();
        let had_pane = script.pane_id.is_some();
        let app = cx.entity();
        let dialog_width =
            responsive_dialog_width(window.bounds().size.width.to_f64(), 0.84, 300.0, 420.0);
        window.open_dialog(cx, move |dialog, _window, cx| {
            let confirm_app = app.clone();
            let confirm_id = script_id.clone();
            dialog
                .title("Delete Script")
                .w(px(dialog_width))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("Delete")
                        .cancel_text("Cancel"),
                )
                .footer(|ok, cancel, window, cx| vec![cancel(window, cx), ok(window, cx)])
                .child(
                    v_flex()
                        .gap(DIALOG_CONTENT_GAP)
                        .child(
                            div()
                                .text_size(theme::FONT_BODY)
                                .child(format!(
                                    "Delete the Script \"{script_name}\" from this Project?"
                                )),
                        )
                        .child(
                            div()
                                .text_size(theme::FONT_META)
                                .text_color(cx.theme().muted_foreground)
                                .child(if had_pane {
                                    "The definition is removed. The running pane stays open — close it yourself when done."
                                } else {
                                    "The definition is removed."
                                }),
                        ),
                )
                .on_ok(move |_, _, app| {
                    confirm_app.update(app, |view, cx| {
                        view.delete_script_id(confirm_id.clone(), cx);
                    });
                    true
                })
        });
    }

    /// Unified drop-on-row semantics: the drop lands before the target row
    /// (consistent with the border_t insertion indicator); an adjacent
    /// forward-move (same order) is a no-op and triggers no persistence/notify.
    pub(crate) fn move_script_to_index(
        &mut self,
        script_id: String,
        target_index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(src_index) = self.scripts.scripts.iter().position(|t| t.id == script_id) else {
            return;
        };
        let insert_at = if src_index < target_index {
            target_index.saturating_sub(1)
        } else {
            target_index.min(self.scripts.scripts.len())
        };
        if insert_at == src_index {
            return;
        }
        let script = self.scripts.scripts.remove(src_index);
        self.scripts.scripts.insert(insert_at, script);
        self.scripts.save();
        self.notify_sidebar(cx);
    }

    pub(crate) fn focus_script_id(
        &mut self,
        script_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(script) = self
            .scripts
            .scripts
            .iter()
            .find(|script| script.id == script_id)
            .cloned()
        else {
            return;
        };
        let runtime_project_matches = build_project_index(&self.state, &self.scripts)
            .for_runtime_id(&script.workspace_id)
            .and_then(|project| project.project_path.as_deref())
            .is_some_and(|runtime_path| project_paths_match(runtime_path, &script.project_path));
        if !runtime_project_matches || script.tab_id.is_none() || script.pane_id.is_none() {
            self.start_script_id(script_id, cx);
            return;
        }
        // FocusIntent seam: focusing a Script's running surface lands at the same
        // place as the other entries (the guard guarantees the tab/pane are
        // materialized; default values would only fall into an invalid target
        // and early-exit under an anomalous projection).
        self.apply_focus_intent(
            FocusIntent::Pane {
                workspace_id: Some(script.workspace_id),
                tab_id: script.tab_id,
                pane_id: script.pane_id.unwrap_or_default(),
            },
            window,
            cx,
        );
    }
}
