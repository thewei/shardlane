//! [INPUT]: Depends on the ShardlaneApp type from the crate root (super) and existing types/imports (use super::*); no independent external dependencies.
//! [OUTPUT]: Exposes ShardlaneApp's settings persistence and window preferences: config read/write/reload/font size/always-on-top
//!           (including applying `ConfigDiff::language` to the i18n locale on reload),
//!           plus parking the Lazygit auxiliary session while Settings is open and restoring it when the visible right panel returns.
//! [POS]: The `crates/herdr-gui` shell settings responsibility domain, mechanically split out of main.rs; together with sibling shell_* modules it forms ShardlaneApp's method surface.
use super::*;

impl ShardlaneApp {
    pub(super) fn toggle_settings(
        &mut self,
        _: &ToggleSettings,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_settings = !self.show_settings;
        if self.show_settings {
            self.new_agent_open = false;
            self.leave_history_surface(cx);
            self.stop_lazygit_session(cx);
            self.clear_ime_state();
            self.refresh_herdr_user_config_snapshot(cx);
        } else if self.is_lazygit_surface_active() {
            // Settings intentionally parks the auxiliary child; returning to the
            // visible right-panel surface must bind a fresh session immediately.
            self.ensure_lazygit_session(cx);
        }
        if !self.show_settings {
            self.settings_provider_detail = None;
        }
        self.show_help = false;
        self.sync_terminal_application_focus(cx);
        self.notify_sidebar(cx);
        cx.notify();
    }

    pub(super) fn set_settings_section(
        &mut self,
        section: SettingsSection,
        cx: &mut Context<Self>,
    ) {
        // The Theme page hosts the visual editor for the Herdr theme config; on entry it
        // reconciles with externally hand-edited configuration.
        if section == SettingsSection::Appearance {
            self.refresh_herdr_user_config_snapshot(cx);
        }
        if section != SettingsSection::Providers {
            self.settings_provider_detail = None;
        }
        if self.settings_section == section {
            return;
        }
        self.settings_section = section;
        cx.notify();
    }

    fn refresh_herdr_user_config_snapshot(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async { herdr_tui::load_herdr_user_config() })
                .await;
            let _ = this.update(cx, |view, cx| match loaded {
                Ok(snapshot) => {
                    if view.herdr_user_config != snapshot {
                        view.herdr_user_config = snapshot;
                        view.sync_app_theme_from_herdr(cx);
                        view.save_config();
                        cx.notify();
                    }
                }
                Err(error) => lag_log(format_args!("herdr.config refresh failed: {error}")),
            });
        })
        .detach();
    }

    pub(super) fn reload_herdr_config(
        &mut self,
        _: &ReloadHerdrConfig,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self.client.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    client.reload_config()?;
                    let snapshot = herdr_tui::load_herdr_user_config().map_err(|error| {
                        herdr::HerdrError::Api(format!("read Herdr config after reload: {error}"))
                    })?;
                    let state = client.visible_state()?;
                    Ok::<_, herdr::HerdrError>((snapshot, state))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok((snapshot, state)) => {
                        view.herdr_user_config = snapshot;
                        view.sync_app_theme_from_herdr(cx);
                        view.save_config();
                        view.state = state;
                        // F34: after the whole-domain replacement, derive the selection flags uniformly.
                        derive_selection_flags(&mut view.state);
                        view.prune_steering_drafts();
                        view.status = ConnectionStatus::Connected;
                        view.sync_pane_event_subscription(cx);
                    }
                    Err(err) => view.status = ConnectionStatus::Offline(err.to_string()),
                }
                view.notify_sidebar(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn start_config_reload(
        &mut self,
        changed_rx: async_channel::Receiver<()>,
        cx: &mut Context<Self>,
    ) -> BackgroundJob<()> {
        cx.spawn(async move |this, cx| loop {
            if changed_rx.recv().await.is_err() {
                break;
            }
            cx.background_executor()
                .timer(Duration::from_millis(75))
                .await;
            while changed_rx.try_recv().is_ok() {}
            let loaded = cx
                .background_executor()
                .spawn(async {
                    settings::ApplicationConfig::load_strict().map_err(|error| error.to_string())
                })
                .await;
            match loaded {
                Ok(config) => {
                    if this
                        .update(cx, |view, cx| view.apply_reloaded_config(config, cx))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => lag_log(format_args!("config.reload ignored error={error}")),
            }
        })
    }

    pub(super) fn apply_reloaded_config(
        &mut self,
        next: settings::ApplicationConfig,
        cx: &mut Context<Self>,
    ) {
        let next = next.normalized();
        let diff = settings::ConfigDiff::between(&self.config, &next);
        if diff.is_empty() {
            return;
        }
        // The Tab-bar placement swaps the Sidebar Tab subtree for the content-area strip, so a
        // change (including an external config edit) must invalidate the cached Sidebar too.
        let tab_bar_placement_changed =
            self.config.terminal.tab_bar_placement != next.terminal.tab_bar_placement;
        self.config = next;
        // SCT-03: shortcut changes (including external config.json edits) → immediate runtime rebinding,
        // no silent drops and no waiting for a restart.
        if diff.shortcuts {
            self.rebind_shortcuts(cx);
        }
        if diff.theme {
            self.theme_mode = theme_mode_from_config(&self.config.ui.appearance);
        }
        if diff.language {
            // The i18n locale is process-global and read at render time; applying
            // it plus the trailing full cx.notify() reskins every surface,
            // including gpui-component's own strings.
            i18n::apply_language(self.config.ui.language);
        }
        if diff.right_panel {
            self.right_panel.width = self.config.ui.right_panel.width.clamp(280.0, 1000.0) as f32;
            cx.notify();
        }
        if diff.sidebar {
            self.shell_sidebar_width = self
                .config
                .ui
                .sidebar
                .width
                .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
            self.sidebar_collapsed = self.config.ui.sidebar.collapsed;
            self.projects_collapsed = self.config.ui.sidebar.projects_collapsed;
            self.agents_collapsed = self.config.ui.sidebar.agents_collapsed;
            self.services_collapsed = self.config.ui.sidebar.services_collapsed;
            self.notify_sidebar(cx);
        }
        if diff.terminal {
            self.apply_terminal_render_settings(cx);
            if tab_bar_placement_changed {
                self.notify_sidebar(cx);
            }
        }
        if diff.lazygit {
            self.refresh_lazygit_detection(cx);
            if self.is_lazygit_surface_active() {
                self.restart_lazygit_session(cx);
            }
        }
        if diff.history_sources {
            // Source policy is user-owned config; rebuild one roster generation and
            // replace the watcher before rescanning. Existing scan results become
            // stale through the history generation guard, while the disposable
            // catalog is reconciled against the newly active roots in the background.
            let policy = self.config.history_sources.clone();
            self.rebuild_history_roster(&policy, cx);
            self.refresh_history(true, cx);
        }
        if diff.theme || diff.window {
            self.apply_native_window_preferences(cx);
        }
        if diff.terminal || diff.window {
            self.sync_config_sliders(cx);
        }
        lag_log(format_args!(
            "config.reload language={} theme={} window={} sidebar={} terminal={} behavior={}",
            diff.language, diff.theme, diff.window, diff.sidebar, diff.terminal, diff.behavior
        ));
        cx.notify();
    }

    pub(super) fn sync_config_sliders(&self, cx: &mut Context<Self>) {
        let Some(window_handle) = self.window_handle else {
            return;
        };
        let font_size_slider = self.font_size_slider.clone();
        let line_height_slider = self.line_height_slider.clone();
        let opacity_slider = self.opacity_slider.clone();
        let font_size = self.config.terminal.font_size;
        let line_height = self.config.terminal.line_height;
        let opacity = self.config.ui.window.opacity;
        let _ = cx.update_window(window_handle, |_, window, cx| {
            font_size_slider.update(cx, |state, cx| state.set_value(font_size, window, cx));
            line_height_slider.update(cx, |state, cx| state.set_value(line_height, window, cx));
            opacity_slider.update(cx, |state, cx| state.set_value(opacity, window, cx));
        });
    }

    pub(super) fn schedule_config_save(&mut self, cx: &mut Context<Self>) {
        self._config_save_script = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let _ = this.update(cx, |view, _| view.save_config());
        });
    }

    pub(super) fn save_config(&mut self) {
        self.config = self.config.clone().normalized();
        self.config.save();
    }

    /// R6 B6: synchronize the loopback Remote API service with the current config (spawn on enable, stop on disable).
    /// The token never enters logs; the SHARDLANE_REMOTE_TOKEN/SHARDLANE_REMOTE_PORT env vars apply only to
    /// the spawned copy and are never persisted.
    pub(super) fn apply_remote_settings(&mut self, cx: &mut Context<Self>) {
        if self.config.remote.ensure_identity() {
            // First enable fills in host_id/access_token: persist before deciding whether to spawn.
            self.save_config();
        }
        let mut spawn_config = self.config.remote.clone();
        spawn_config.overlay_env();
        if spawn_config.is_loopback_ready() {
            let mut remote_slot = self
                .shared
                .remote_server
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(handle) = remote_slot.take() {
                handle.stop();
                lag_log(format_args!("remote.server stopped for restart"));
            }
            let options = shardlane_remote::RemoteServerOptions {
                // M3: Remote live queries share the same Host session owner as the GUI Chat worker.
                conversation_sessions: Some(self.conversation_sessions.clone()),
                // A01: the Remote TUI viewer attaches to the same Host-owned shared child
                // process as the Desktop hosted TUI (one child + one PTY per Host process).
                shared_tui: Some(self.shared.tui_registry.clone()),
                // AC-05: Desktop and Remote share the same process-level follow-up queue truth.
                delivery_coordinator: Some(self.delivery.clone()),
                config: spawn_config,
                host_name: remote_display_host_name(),
                host_version: env!("CARGO_PKG_VERSION").to_string(),
                settings_path: settings::config_path(),
                herdr_socket_override: None,
                web_bundle_path: crate::web_bundle_path(),
            };
            match shardlane_remote::spawn_remote_server(options) {
                Ok(handle) => {
                    lag_log(format_args!(
                        "remote.server listening on {} ({:?})",
                        handle.addr, self.config.remote.listener_mode,
                    ));
                    *remote_slot = Some(handle);
                }
                Err(error) => {
                    lag_log(format_args!("remote.server failed to start: {error}"));
                    *remote_slot = None;
                }
            }
        } else {
            let mut remote_slot = self
                .shared
                .remote_server
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(handle) = remote_slot.take() {
                handle.stop();
                lag_log(format_args!("remote.server stopped"));
            }
        }
        cx.notify();
    }

    pub(super) fn terminal_font_size(&self) -> f64 {
        f64::from(clamped_terminal_font_size(self.config.terminal.font_size))
    }

    /// Shared entry for ⌘+/⌘-/⌘0 font-size adjustments: unified clamp, dedup, re-ranking, and persistence.
    pub(super) fn apply_terminal_font_size(
        &mut self,
        next: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = clamped_terminal_font_size(next);
        if (next - self.config.terminal.font_size).abs() < f32::EPSILON {
            return;
        }
        self.config.terminal.font_size = next;
        self.apply_terminal_render_settings(cx);
        self.sync_config_sliders(cx);
        self.save_config();
        cx.notify();
        window.push_notification(format!("Terminal font {next:.0} pt"), cx);
    }

    pub(super) fn increase_terminal_font_size(
        &mut self,
        _: &IncreaseTerminalFontSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_terminal_font_size(self.config.terminal.font_size + 1.0, window, cx);
    }

    pub(super) fn decrease_terminal_font_size(
        &mut self,
        _: &DecreaseTerminalFontSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_terminal_font_size(self.config.terminal.font_size - 1.0, window, cx);
    }

    pub(super) fn reset_terminal_font_size(
        &mut self,
        _: &ResetTerminalFontSize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let default_size = settings::TerminalConfig::default().font_size;
        self.apply_terminal_font_size(default_size, window, cx);
    }

    pub(super) fn window_opacity(&self) -> f32 {
        self.config.ui.window.opacity.clamp(0.55, 1.0)
    }

    pub(super) fn native_forced_dark(&self) -> Option<bool> {
        match self.theme_mode {
            ThemeMode::System => None,
            ThemeMode::Light => Some(false),
            ThemeMode::Dark => Some(true),
        }
    }

    pub(super) fn apply_native_window_preferences(&self, cx: &mut Context<Self>) {
        let Some(window_handle) = self.window_handle else {
            return;
        };
        let opacity = self.window_opacity();
        let always_on_top = self.config.ui.window.always_on_top;
        let _ = cx.update_window(window_handle, |_, window, _| {
            if let Err(error) =
                macos_window::apply(window, opacity, always_on_top, self.native_forced_dark())
            {
                lag_log(format_args!("window.native_preferences error={error}"));
            }
        });
    }

    pub(super) fn toggle_window_always_on_top(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.config.ui.window.always_on_top = !self.config.ui.window.always_on_top;
        if let Err(error) = macos_window::apply(
            window,
            self.window_opacity(),
            self.config.ui.window.always_on_top,
            self.native_forced_dark(),
        ) {
            lag_log(format_args!("window.always_on_top error={error}"));
        }
        self.save_config();
        cx.notify();
    }

    pub(super) fn toggle_always_on_top(
        &mut self,
        _: &ToggleAlwaysOnTop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_window_always_on_top(window, cx);
    }
}
