//! List/detail projections for Provider Settings and History source actions.
//!
//! [INPUT]: Depends on `ApplicationConfig`'s provider/source policy, the
//! `HistoryAdapterRoster` generation held by HistoryUiState, background
//! availability/integration/index caches, and gpui-component Settings controls.
//! [OUTPUT]: Exposes `ShardlaneApp::providers_settings_content` and source configuration actions;
//! all filesystem/CLI/Herdr status reads happen in the background — render only consumes caches.
//! [POS]: The sole presentation layer for Settings → Providers; owns no Provider runtime and
//! never turns `history.sqlite3` into user configuration.

use super::*;
use crate::assets::agent_brand_icon;
use crate::settings_view::{settings_card, settings_card_row};
use crate::ui::controls::{ControlSurface, Toggle};
use ::gpui::img;
use gpui_component::button::ButtonVariants as _;
use shardlane_history::{
    HistorySourceKey, HistorySourceKind, HistorySourceLocation, HistorySourcePolicy,
    LiveCapability, ProviderCapabilities, ProviderExposure,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How long a history-source Remove stays armed before the confirmation lapses
/// (E22 two-click arm, mirroring the workspace dialog's arm pattern).
const REMOVE_ARM_WINDOW: std::time::Duration = std::time::Duration::from_secs(5);

/// Cached metadata for the disposable local History index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HistoryIndexSnapshot {
    pub(crate) path: PathBuf,
    pub(crate) size_bytes: u64,
}

fn display_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    static HOME: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    if let Some(home) = HOME
        .get_or_init(|| std::env::var_os("HOME").map(PathBuf::from))
        .as_ref()
    {
        let home = home.to_string_lossy();
        let home = home.trim_end_matches('/');
        if path == home {
            return "~".to_string();
        }
        if let Some(rest) = path.strip_prefix(home) {
            if rest.starts_with('/') {
                return format!("~{rest}");
            }
        }
    }
    path.into_owned()
}

fn format_bytes(size: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = size as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{size} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn provider_icon(agent: AgentId, dark: bool, size: f32) -> AnyElement {
    agent_brand_icon(agent.as_str(), dark)
        .map(|path| img(path).size(px(size)).into_any_element())
        .unwrap_or_else(|| {
            Icon::new(ComponentIconName::Bot)
                .with_size(px(size))
                .into_any_element()
        })
}

fn source_session_count(app: &ShardlaneApp, location: &HistorySourceLocation) -> usize {
    let root = location.path().to_string_lossy();
    app.history
        .sessions
        .iter()
        .filter(|session| session.agent == location.agent())
        .filter(|session| shardlane_history::path_owns(&root, &session.file_path))
        .count()
}

fn source_status(
    location: &HistorySourceLocation,
    probe: Option<&HashMap<HistorySourceKey, bool>>,
) -> &'static str {
    match probe.and_then(|probe| probe.get(&location.key)).copied() {
        None => "Checking…",
        Some(true) => "Available",
        Some(false) => "Unavailable",
    }
}

fn provider_capability_text(
    app: &ShardlaneApp,
    agent: AgentId,
    caps: &ProviderCapabilities,
) -> String {
    let installed = match app.provider_availability.as_ref() {
        None => "Detecting…".to_string(),
        Some(available) => available
            .get(&agent)
            .map(|path| format!("Installed · {}", display_path(Path::new(path))))
            .unwrap_or_else(|| "Not found".to_string()),
    };
    let mut parts = vec![installed, "History".to_string()];
    if caps.live == LiveCapability::AppendLog {
        parts.push("Live Chat".to_string());
    }
    if caps.exposure == ProviderExposure::Preview {
        parts.push("Preview".to_string());
    }
    parts.join(" · ")
}

fn history_status_text(
    app: &ShardlaneApp,
    agent: AgentId,
    probe: Option<&HashMap<HistorySourceKey, bool>>,
) -> String {
    let locations = app
        .history
        .roster
        .locations
        .iter()
        .filter(|location| location.agent() == agent)
        .collect::<Vec<_>>();
    if locations.is_empty() {
        return "Unavailable".to_string();
    }
    let checked = locations
        .iter()
        .filter_map(|location| probe.and_then(|probe| probe.get(&location.key)))
        .copied()
        .collect::<Vec<_>>();
    if checked.len() != locations.len() {
        return "Checking…".to_string();
    }
    let available = checked.iter().filter(|available| **available).count();
    if available == 0 {
        return "Unavailable".to_string();
    }
    let sessions = locations
        .iter()
        .filter(|location| probe.is_some_and(|probe| probe.get(&location.key) == Some(&true)))
        .map(|location| source_session_count(app, location))
        .sum::<usize>();
    format!(
        "Available · {available} source{} · {sessions} indexed session{}",
        plural(available),
        plural(sessions)
    )
}

fn integration_status_text(app: &ShardlaneApp, agent: AgentId) -> String {
    let Some(health) = app
        .provider_integration_health
        .as_ref()
        .and_then(|all| all.iter().find(|health| health.provider == agent))
    else {
        return "Checking…".to_string();
    };
    if matches!(
        health.strategy,
        shardlane_host::agent_integrations::AgentIntegrationStrategy::Deferred
    ) {
        return "Deferred · Herdr hook/session binding pending".to_string();
    }
    if !health.herdr_cli_present {
        return "Herdr CLI not found".to_string();
    }
    if let Some(official) = &health.official {
        return if official.is_current {
            format!("Installed · {}", official.raw_state)
        } else {
            format!("Not installed · {}", official.raw_state)
        };
    }
    match health.strategy {
        shardlane_host::agent_integrations::AgentIntegrationStrategy::HerdrScreenOnly
        | shardlane_host::agent_integrations::AgentIntegrationStrategy::HerdrScreenWithManagedSessionBridge
        | shardlane_host::agent_integrations::AgentIntegrationStrategy::ManagedLifecycleBridge => {
            "Herdr status unavailable".to_string()
        }
        shardlane_host::agent_integrations::AgentIntegrationStrategy::HerdrOfficial { .. }
        | shardlane_host::agent_integrations::AgentIntegrationStrategy::Deferred => {
            "Herdr status unavailable".to_string()
        }
    }
}

fn plural(value: usize) -> &'static str {
    if value == 1 {
        ""
    } else {
        "s"
    }
}

fn index_size(path: &Path) -> u64 {
    [
        path.to_path_buf(),
        path.with_extension("sqlite3-wal"),
        path.with_extension("sqlite3-shm"),
    ]
    .into_iter()
    .filter_map(|path| std::fs::metadata(path).ok())
    .map(|metadata| metadata.len())
    .sum()
}

impl ShardlaneApp {
    pub(crate) fn ensure_provider_availability(&self, cx: &mut Context<Self>) {
        if self.provider_availability.is_some() || self.provider_availability_requested.get() {
            return;
        }
        self.provider_availability_requested.set(true);
        cx.spawn(async move |this, cx| {
            let available = cx
                .background_executor()
                .spawn(async move {
                    shardlane_history::exposed_agents()
                        .into_iter()
                        .filter_map(|agent| {
                            crate::agent_cli::resolve_agent_launch(agent)
                                .ok()
                                .map(|launch| (agent, launch.executable))
                        })
                        .collect::<HashMap<_, _>>()
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.provider_availability = Some(available);
                view.provider_availability_requested.set(false);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn ensure_provider_integration_health(&self, cx: &mut Context<Self>) {
        if self.provider_integration_health.is_some()
            || self.provider_integration_health_requested.get()
        {
            return;
        }
        self.provider_integration_health_requested.set(true);
        cx.spawn(async move |this, cx| {
            let health = cx
                .background_executor()
                .spawn(async { shardlane_host::agent_integrations::audit_integrations() })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.provider_integration_health = Some(health);
                view.provider_integration_health_requested.set(false);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn ensure_history_source_probe(&self, agent: AgentId, cx: &mut Context<Self>) {
        if self.history_source_probe_requested.get() {
            return;
        }
        let generation = self.history.generation;
        let locations = self
            .history
            .roster
            .locations
            .iter()
            .filter(|location| location.agent() == agent)
            .map(|location| location.key.clone())
            .collect::<Vec<_>>();
        if locations.is_empty() {
            return;
        }
        if self
            .history_source_probe
            .as_ref()
            .is_some_and(|probe| locations.iter().all(|key| probe.contains_key(key)))
        {
            return;
        }
        self.history_source_probe_requested.set(true);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    locations
                        .into_iter()
                        .map(|key| {
                            let exists = key.path.exists();
                            (key, exists)
                        })
                        .collect::<HashMap<_, _>>()
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.history_source_probe_requested.set(false);
                if view.history.generation != generation {
                    view.history_source_probe = None;
                    return;
                }
                view.history_source_probe = Some(result);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn ensure_history_index_snapshot(&self, cx: &mut Context<Self>) {
        if self.history_index_snapshot.is_some() || self.history_index_snapshot_requested.get() {
            return;
        }
        self.history_index_snapshot_requested.set(true);
        let path = crate::history::history_db_path();
        cx.spawn(async move |this, cx| {
            let snapshot = cx
                .background_executor()
                .spawn(async move {
                    HistoryIndexSnapshot {
                        size_bytes: index_size(&path),
                        path,
                    }
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.history_index_snapshot = Some(snapshot);
                view.history_index_snapshot_requested.set(false);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn open_provider_detail(&mut self, agent: AgentId, cx: &mut Context<Self>) {
        self.settings_section = SettingsSection::Providers;
        self.settings_provider_detail = Some(agent);
        self.ensure_history_source_probe(agent, cx);
        cx.notify();
    }

    pub(crate) fn return_to_provider_list(&mut self, cx: &mut Context<Self>) {
        self.settings_provider_detail = None;
        self.settings_section = SettingsSection::Providers;
        cx.notify();
    }

    pub(crate) fn set_provider_enabled(
        &mut self,
        agent: AgentId,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        self.config.providers.set_enabled(agent, enabled);
        self.save_config();
        cx.notify();
    }

    pub(crate) fn set_history_source_enabled(
        &mut self,
        location: HistorySourceLocation,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        let mut policy = self.config.history_sources.clone();
        policy.set_enabled(location.kind, location.key, enabled);
        self.config.history_sources = policy.normalized();
        self.save_config();
        let policy = self.config.history_sources.clone();
        self.rebuild_history_roster(&policy, cx);
        self.refresh_history(true, cx);
        self.history_source_probe = None;
        self.history_source_probe_requested.set(false);
        cx.notify();
    }

    pub(crate) fn remove_history_source(
        &mut self,
        agent: AgentId,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.history_remove_confirm = None;
        let mut policy = self.config.history_sources.clone();
        policy.remove_custom_root(agent, &path);
        self.config.history_sources = policy.normalized();
        self.save_config();
        let policy = self.config.history_sources.clone();
        self.rebuild_history_roster(&policy, cx);
        self.refresh_history(true, cx);
        self.history_source_probe = None;
        self.history_source_probe_requested.set(false);
        cx.notify();
    }

    /// E22 (audit 2026-09-01): "Remove" deletes persisted config, so it uses the app's
    /// light two-click arm (first click arms + notifies; the second click within the arm
    /// window executes). Arming a different source, or letting the arm lapse, disarms.
    pub(crate) fn remove_history_source_confirmed(
        &mut self,
        agent: AgentId,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((armed_agent, armed_path, armed_at)) = &self.history_remove_confirm {
            if *armed_agent == agent
                && *armed_path == path
                && armed_at.elapsed() <= REMOVE_ARM_WINDOW
            {
                self.remove_history_source(agent, path, cx);
                return;
            }
        }
        self.history_remove_confirm = Some((agent, path, std::time::Instant::now()));
        window.push_notification(
            "Click Remove again to confirm removing this history source",
            cx,
        );
    }

    pub(crate) fn restore_provider_sources(&mut self, agent: AgentId, cx: &mut Context<Self>) {
        let mut policy = self.config.history_sources.clone();
        policy.restore_defaults(agent);
        self.config.history_sources = policy.normalized();
        self.save_config();
        let policy = self.config.history_sources.clone();
        self.rebuild_history_roster(&policy, cx);
        self.refresh_history(true, cx);
        self.history_source_probe = None;
        self.history_source_probe_requested.set(false);
        cx.notify();
    }

    pub(crate) fn add_history_source(
        &mut self,
        agent: AgentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add History Source".into()),
        });
        let window_handle = window.window_handle();
        let locations = self.history.roster.locations.clone();
        cx.spawn(async move |this, cx| {
            let selected = match picker.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    let message = format!("History source picker failed: {error}");
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        window.push_notification(message.clone(), cx);
                    });
                    None
                }
            };
            let Some(path) = selected else { return };
            let _ = this.update(cx, |view, cx| {
                let policy = view.config.history_sources.clone();
                if let Err(error) = policy.validate_custom_root(agent, &path, &locations) {
                    window_notification(window_handle, cx, error);
                    return;
                }
                let root = shardlane_history::normalize_custom_root(agent, path);
                view.config
                    .history_sources
                    .custom_roots
                    .push(shardlane_history::CustomHistoryRoot { agent, path: root });
                view.config.history_sources = view.config.history_sources.clone().normalized();
                view.save_config();
                let policy = view.config.history_sources.clone();
                view.rebuild_history_roster(&policy, cx);
                view.refresh_history(true, cx);
                view.history_source_probe = None;
                view.history_source_probe_requested.set(false);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn edit_history_source(
        &mut self,
        agent: AgentId,
        old_root: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Edit History Source".into()),
        });
        let window_handle = window.window_handle();
        let locations = self
            .history
            .roster
            .locations
            .iter()
            .filter(|location| {
                !(location.agent() == agent
                    && location.kind == HistorySourceKind::Custom
                    && view_custom_root_for_location(&self.config.history_sources, agent, location)
                        .as_ref()
                        .is_some_and(|root| *root == old_root))
            })
            .cloned()
            .collect::<Vec<_>>();
        cx.spawn(async move |this, cx| {
            let selected = match picker.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    let message = format!("History source picker failed: {error}");
                    let _ = cx.update_window(window_handle, |_, window, cx| {
                        window.push_notification(message.clone(), cx);
                    });
                    None
                }
            };
            let Some(path) = selected else { return };
            let _ = this.update(cx, |view, cx| {
                let policy = view.config.history_sources.clone();
                if let Err(error) = policy.validate_custom_root(agent, &path, &locations) {
                    window_notification(window_handle, cx, error);
                    return;
                }
                let root = shardlane_history::normalize_custom_root(agent, path);
                view.config
                    .history_sources
                    .replace_custom_root(agent, &old_root, root);
                view.config.history_sources = view.config.history_sources.clone().normalized();
                view.save_config();
                let policy = view.config.history_sources.clone();
                view.rebuild_history_roster(&policy, cx);
                view.refresh_history(true, cx);
                view.history_source_probe = None;
                view.history_source_probe_requested.set(false);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn providers_settings_content(
        &mut self,
        surface: ControlSurface,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        if self.settings_section != SettingsSection::Providers {
            return div();
        }
        self.ensure_provider_availability(cx);
        self.ensure_provider_integration_health(cx);
        self.ensure_history_index_snapshot(cx);
        let dark = self.content_surface_theme(window).is_dark;
        match self.settings_provider_detail {
            Some(agent) => {
                self.ensure_history_source_probe(agent, cx);
                self.provider_detail_content(agent, surface, dark, window, cx)
            }
            None => self.provider_list_content(surface, dark, cx),
        }
    }

    fn provider_list_content(
        &self,
        surface: ControlSurface,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let herdr = cx.entity();
        let rows = shardlane_history::exposed_agents()
            .into_iter()
            .filter_map(|agent| {
                let caps = shardlane_history::provider_capabilities(agent)?;
                let enabled = self.config.providers.is_enabled(agent);
                let detail = provider_capability_text(self, agent, caps);
                let open_herdr = herdr.clone();
                let toggle_herdr = herdr.clone();
                let body = h_flex()
                    .w_full()
                    .min_w_0()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .items_center()
                    .gap_3()
                    .child(provider_icon(agent, dark, 18.0))
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(theme::FONT_LIST_TITLE)
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(agent.display_name()),
                            )
                            .child(
                                div()
                                    .text_size(theme::FONT_META)
                                    .text_color(surface.foreground.opacity(0.7))
                                    .truncate()
                                    .child(detail),
                            ),
                    )
                    .child(
                        Toggle::new(
                            SharedString::from(format!("provider-enabled-{}", agent.as_str())),
                            surface,
                        )
                        .checked(enabled)
                        .on_change(move |checked, _, app| {
                            toggle_herdr.update(app, |view, cx| {
                                view.set_provider_enabled(agent, checked, cx);
                            });
                        }),
                    )
                    .child(
                        Button::new(SharedString::from(format!(
                            "provider-open-{}",
                            agent.as_str()
                        )))
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(ComponentIconName::ChevronRight).xsmall())
                        .on_click(move |_, _, app| {
                            open_herdr.update(app, |view, cx| {
                                view.open_provider_detail(agent, cx);
                            });
                        }),
                    );
                Some(body.into_any_element())
            })
            .collect::<Vec<_>>();
        let index_row = self.history_index_snapshot.as_ref().map(|snapshot| {
            settings_card_row(
                "Local History Index",
                &format!(
                    "{} · {}",
                    display_path(&snapshot.path),
                    format_bytes(snapshot.size_bytes)
                ),
                Button::new("history-index-reveal")
                    .ghost()
                    .xsmall()
                    .label("Reveal in Finder")
                    .on_click({
                        let path = snapshot.path.clone();
                        move |_, _, _| {
                            let _ = reveal_in_finder(&path);
                        }
                    })
                    .into_any_element(),
            )
        });
        let mut content = div()
            .w_full()
            .min_w_0()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .mt(px(4.0))
                    .text_size(theme::FONT_BODY)
                    .line_height(px(18.0))
                    .opacity(0.72)
                    .child("Choose which providers appear in New Agent and History continuation."),
            )
            .child(settings_card(surface, rows));
        if let Some(row) = index_row {
            content = content.child(settings_card(surface, vec![row]));
        } else {
            content = content.child(settings_card(
                surface,
                vec![settings_card_row(
                    "Local History Index",
                    "Index metadata is being loaded in the background.",
                    Spinner::new().xsmall().into_any_element(),
                )],
            ));
        }
        content
    }

    fn provider_detail_content(
        &self,
        agent: AgentId,
        surface: ControlSurface,
        dark: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let herdr = cx.entity();
        let caps = shardlane_history::provider_capabilities(agent);
        let capability = caps
            .map(|caps| provider_capability_text(self, agent, caps))
            .unwrap_or_else(|| "History".to_string());
        let back = Button::new("providers-back")
            .ghost()
            .xsmall()
            .icon(Icon::new(ComponentIconName::ArrowLeft).xsmall())
            .tooltip("Back to Providers")
            .on_click({
                let herdr = herdr.clone();
                move |_, _, app| {
                    herdr.update(app, |view, cx| view.return_to_provider_list(cx));
                }
            });
        let cli_path = self
            .provider_availability
            .as_ref()
            .and_then(|available| available.get(&agent))
            .map(|path| display_path(Path::new(path)))
            .unwrap_or_else(|| "Not found".to_string());
        let availability_rows = vec![
            settings_card_row(
                "CLI",
                &cli_path,
                div()
                    .text_size(theme::FONT_META)
                    .text_color(surface.foreground.opacity(0.72))
                    .child(match self.provider_availability.as_ref() {
                        None => "Detecting…",
                        Some(available) if available.contains_key(&agent) => "Installed",
                        Some(_) => "Not found",
                    })
                    .into_any_element(),
            ),
            settings_card_row(
                "History",
                &history_status_text(self, agent, self.history_source_probe.as_ref()),
                div().text_size(theme::FONT_META).into_any_element(),
            ),
            settings_card_row(
                "Live Chat",
                if caps.is_some_and(|caps| caps.live == LiveCapability::AppendLog) {
                    "Available"
                } else {
                    "Not available"
                },
                div().text_size(theme::FONT_META).into_any_element(),
            ),
            settings_card_row(
                "Herdr integration",
                &integration_status_text(self, agent),
                div().text_size(theme::FONT_META).into_any_element(),
            ),
        ];

        let mut source_rows = Vec::new();
        let locations = self
            .history
            .roster
            .locations
            .iter()
            .filter(|location| location.agent() == agent)
            .cloned()
            .collect::<Vec<_>>();
        for location in locations {
            let toggle_herdr = herdr.clone();
            let enabled = location.enabled;
            let source = location.clone();
            let display = display_path(location.path());
            let kind = match location.kind {
                HistorySourceKind::Default => "Default",
                HistorySourceKind::Custom => "Custom",
            };
            let count = source_session_count(self, &location);
            let status = source_status(&location, self.history_source_probe.as_ref());
            let mut menu = DropdownButton::new(SharedString::from(format!(
                "history-source-menu-{}",
                display
            )));
            let menu_herdr = herdr.clone();
            let path_for_reveal = location.path().clone();
            let custom_root = self
                .config
                .history_sources
                .custom_root_for_location(agent, location.path());
            let edit_root = custom_root.clone();
            let remove_root = custom_root;
            menu = menu
                .ghost()
                .xsmall()
                .button(
                    Button::new(SharedString::from(format!(
                        "history-source-more-{}",
                        display
                    )))
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(ComponentIconName::Ellipsis).xsmall()),
                )
                .dropdown_menu(move |mut popup, _, cx| {
                    let reveal = path_for_reveal.clone();
                    popup = popup.item(PopupMenuItem::new("Reveal in Finder").on_click(
                        move |_, _, _| {
                            let _ = reveal_in_finder(&reveal);
                        },
                    ));
                    if let Some(edit_root) = edit_root.clone() {
                        let edit_herdr = menu_herdr.clone();
                        popup = popup.item(PopupMenuItem::new("Edit…").on_click(
                            move |_, window, app| {
                                edit_herdr.update(app, |view, cx| {
                                    view.edit_history_source(agent, edit_root.clone(), window, cx);
                                });
                            },
                        ));
                    }
                    if let Some(remove_root) = remove_root.clone() {
                        let remove_herdr = menu_herdr.clone();
                        // E22 (audit 2026-09-01): destructive Remove arms on the first click
                        // (with a notification); reopening the menu shows the armed label and
                        // the second click executes.
                        let armed = remove_herdr
                            .read(cx)
                            .history_remove_confirm
                            .as_ref()
                            .is_some_and(|(armed_agent, armed_path, armed_at)| {
                                *armed_agent == agent
                                    && *armed_path == remove_root
                                    && armed_at.elapsed() <= REMOVE_ARM_WINDOW
                            });
                        popup = popup.item(
                            PopupMenuItem::new(if armed {
                                "Click again to Remove"
                            } else {
                                "Remove"
                            })
                            .on_click(move |_, window, app| {
                                remove_herdr.update(app, |view, cx| {
                                    view.remove_history_source_confirmed(
                                        agent,
                                        remove_root.clone(),
                                        window,
                                        cx,
                                    );
                                });
                            }),
                        );
                    }
                    popup
                });
            let source_row = h_flex()
                .w_full()
                .min_w_0()
                .min_h(px(60.0))
                .px(px(20.0))
                .py(px(12.0))
                .items_center()
                .gap_2()
                .child(Icon::new(ComponentIconName::Folder).xsmall())
                .child(
                    v_flex()
                        .min_w_0()
                        .flex_1()
                        .gap(px(2.0))
                        .child(div().text_size(theme::FONT_META).truncate().child(display))
                        .child(
                            div()
                                .text_size(theme::FONT_META)
                                .text_color(surface.foreground.opacity(0.68))
                                .child(format!(
                                    "{kind} · {status} · {count} session{}",
                                    plural(count)
                                )),
                        ),
                )
                .child(
                    Toggle::new(
                        SharedString::from(format!(
                            "history-source-enabled-{}",
                            location.path().display()
                        )),
                        surface,
                    )
                    .checked(enabled)
                    .on_change(move |checked, _, app| {
                        toggle_herdr.update(app, |view, cx| {
                            view.set_history_source_enabled(source.clone(), checked, cx);
                        });
                    }),
                )
                .child(menu);
            source_rows.push(source_row.into_any_element());
        }

        let add_herdr = herdr.clone();
        let restore_herdr = herdr.clone();
        let add_button = Button::new("history-source-add")
            .ghost()
            .xsmall()
            .label("Add Location")
            .on_click(move |_, window, app| {
                add_herdr.update(app, |view, cx| view.add_history_source(agent, window, cx));
            });
        let restore_button = Button::new("history-source-restore")
            .ghost()
            .xsmall()
            .label("Restore Defaults")
            .on_click(move |_, window, app| {
                confirm_restore_provider_sources(agent, window, app, restore_herdr.clone());
            });
        let source_card = settings_card(
            surface,
                vec![
                h_flex()
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .items_center()
                    .justify_between()
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap(px(2.0))
                            .child(div().font_weight(FontWeight::MEDIUM).child("History Sources"))
                            .child(
                                div()
                                    .text_size(theme::FONT_META)
                                    .text_color(surface.foreground.opacity(0.68))
                                    .child("Built-in roots can be disabled; custom roots can be edited or removed."),
                            ),
                    )
                    .child(add_button)
                    .into_any_element(),
            ]
            .into_iter()
            .chain(source_rows)
            .chain(std::iter::once(
                h_flex()
                    .w_full()
                    .min_h(px(60.0))
                    .px(px(20.0))
                    .py(px(12.0))
                    .justify_end()
                    .child(restore_button)
                    .into_any_element(),
            ))
            .collect(),
        );

        div()
            .w_full()
            .min_w_0()
            .child(
                h_flex()
                    .w_full()
                    .mt_3()
                    .items_center()
                    .gap_3()
                    .child(back)
                    .child(provider_icon(agent, dark, 24.0))
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(theme::FONT_SECTION_TITLE)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(agent.display_name()),
                            )
                            .child(
                                div()
                                    .text_size(theme::FONT_META)
                                    .text_color(surface.foreground.opacity(0.72))
                                    .child(capability),
                            ),
                    ),
            )
            .child(settings_card(surface, availability_rows))
            .child(source_card)
    }
}

fn view_custom_root_for_location(
    policy: &HistorySourcePolicy,
    agent: AgentId,
    location: &HistorySourceLocation,
) -> Option<PathBuf> {
    policy.custom_root_for_location(agent, location.path())
}

fn confirm_restore_provider_sources(
    agent: AgentId,
    window: &mut Window,
    cx: &mut App,
    herdr: Entity<ShardlaneApp>,
) {
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let herdr = herdr.clone();
        dialog
            .title(format!("Restore {} History Sources", agent.display_name()))
            .button_props(
                gpui_component::dialog::DialogButtonProps::default()
                    .ok_text("Restore")
                    .cancel_text("Cancel"),
            )
            .footer(|ok, cancel, window, cx| vec![cancel(window, cx), ok(window, cx)])
            .child(
                div()
                    .text_size(theme::FONT_BODY)
                    .child("Remove custom locations and re-enable built-in sources? Your local History index will not be deleted."),
            )
            .on_ok(move |_, _, app| {
                herdr.update(app, |view, cx| view.restore_provider_sources(agent, cx));
                true
            })
    });
}

/// E21: the single "Reveal in Finder" implementation shared by the index card
/// and the history-source menu.
fn reveal_in_finder(path: &std::path::Path) -> std::io::Result<std::process::Child> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
}

/// E02 (audit 2026-09-01): a rejected source add/edit must be user-visible. The name
/// promised a window notification but the body only logged — push a real notification
/// on the same channel as the picker-failure paths above.
fn window_notification(
    window_handle: AnyWindowHandle,
    cx: &mut Context<ShardlaneApp>,
    message: String,
) {
    lag_log(format_args!("history source action rejected: {message}"));
    let _ = cx.update_window(window_handle, |_, window, cx| {
        window.push_notification(message, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_path_collapses_home_without_touching_filesystem() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/test".to_string());
        assert_eq!(display_path(Path::new(&home)), "~");
        assert_eq!(
            display_path(&PathBuf::from(home).join(".qoder")),
            "~/.qoder"
        );
    }

    #[test]
    fn format_bytes_uses_human_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn plural_is_stable_for_source_and_session_labels() {
        assert_eq!(format!("source{}", plural(1)), "source");
        assert_eq!(format!("source{}", plural(2)), "sources");
    }
}
