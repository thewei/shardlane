//! Shardlane application configuration model and JSON persistence.
//!
//! `~/.shardlane/config.json` is the single source of truth for user-owned
//! application configuration. Runtime/UI fields may cache projections of this
//! model for rendering, but Settings UI and external file edits both mutate or
//! reload this same typed model.
//!
//! [INPUT]: Depends on serde/notify/async-channel for persistence and file watching
//! [OUTPUT]: Exposes `ApplicationConfig` (including `TerminalConfig`'s font/padding/theme/
//!           cursor shape and blink preferences, the `Language` i18n preference,
//!           `BehaviorConfig` and other typed enums,
//!           plus the Lazygit auxiliary tool config), load/save/normalize, and the config-change channel
//! [POS]: The client configuration layer; settings_view.rs reads/writes it, main.rs consumes it to drive rendering

use crate::sidebar::SIDEBAR_DEFAULT_WIDTH;
use async_channel::Receiver;
use notify::{RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const CONFIG_VERSION: u32 = 5;
const CONFIG_FILE_NAME: &str = "config.json";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to create config directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to serialize config: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("failed to write config {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// A machine that can host Herdr instances. Today only the local machine is
/// seeded; Herdr's `--remote <ssh-target>` supports remote devices server-side,
/// and SSH socket forwarding lands with the machine-connect page.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DeviceEntry {
    /// Stable id ("local" for this machine).
    pub id: String,
    /// Display name (hostname once known).
    pub name: String,
    /// `None` = this machine; `Some(target)` = `user@host` SSH target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_target: Option<String>,
}

impl DeviceEntry {
    pub fn local() -> Self {
        Self {
            id: "local".to_string(),
            name: "This Mac".to_string(),
            ssh_target: None,
        }
    }
}

/// B4/C4: one currently-open workspace window (session + frame), snapshotted
/// for launch restore. Rebuilt from live windows on every persist.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OpenWorkspaceRecord {
    /// Herdr session name; None = the default instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
}

/// Workspaces are Herdr instances (multi-instance model): Shardlane keeps NO
/// workspace registry — instances are enumerated from `herdr session list`.
/// The only Shardlane-side data is cosmetic: per-session display-name
/// overrides so the workspace switcher can show semantic names (herdr has no
/// session rename), plus the machine list above.

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct ApplicationConfig {
    pub version: u32,
    /// Machines that can host Herdr instances (local seeded; SSH targets are
    /// added from the machine panel — socket forwarding is the next step).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<DeviceEntry>,
    /// B4/C4: the workspace windows currently open (restored at launch).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open_workspaces: Vec<OpenWorkspaceRecord>,
    pub ui: UiConfig,
    pub terminal: TerminalConfig,
    pub behavior: BehaviorConfig,
    /// Remote Access (loopback Remote API). The only writer is the Settings UI;
    /// the shardlane-remote service is read-only. Default is everything off (the zero idle-cost invariant).
    #[serde(default)]
    pub remote: shardlane_remote::RemoteConfig,
    /// Keyboard shortcut overrides and disabled commands.
    #[serde(
        default,
        skip_serializing_if = "crate::shortcuts::ShortcutConfig::is_empty"
    )]
    pub shortcuts: crate::shortcuts::ShortcutConfig,
    /// Lazygit auxiliary tool configuration. The executable is hosted only while the
    /// visible right-panel surface is active; it is never a Herdr runtime terminal.
    #[serde(default)]
    pub lazygit: LazygitConfig,
    /// Browser profile configuration (session isolation, profiles).
    #[serde(
        default,
        skip_serializing_if = "crate::browser_profile::BrowserConfig::is_empty"
    )]
    pub browser: crate::browser_profile::BrowserConfig,
    /// R4: the user's per-provider enable set (keyed by slug). Default = the default-enabled set
    /// (Stable + Preview-opt-in); unknown/removed slugs are normalized away on load.
    #[serde(default)]
    pub providers: ProvidersConfig,
    /// History source policy is user-owned configuration. The SQLite history
    /// catalog remains disposable and never stores these choices.
    #[serde(default)]
    pub history_sources: shardlane_history::HistorySourcePolicy,
}

impl Default for ApplicationConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            devices: vec![DeviceEntry::local()],
            open_workspaces: Vec::new(),
            ui: UiConfig::default(),
            terminal: TerminalConfig::default(),
            behavior: BehaviorConfig::default(),
            remote: shardlane_remote::RemoteConfig::default(),
            shortcuts: crate::shortcuts::ShortcutConfig::default(),
            lazygit: LazygitConfig::default(),
            browser: crate::browser_profile::BrowserConfig::default(),
            providers: ProvidersConfig::default(),
            history_sources: shardlane_history::HistorySourcePolicy::default(),
        }
    }
}

/// Per-provider user enablement config (R4 / audit CS-08). slug = AgentId::as_str.
/// Technical capability/exposure lives in the `shardlane-history` registry; this only stores user intent.
/// Semantics: the `enabled` set IS the truth — present = enabled, absent = not enabled; the default
/// instance = the default-enabled set (Stable: ClaudeCode, Codex, Cursor, Pi, Omp); Hidden never enters the default.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct ProvidersConfig {
    pub enabled: std::collections::HashSet<String>,
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        let default_agents: [shardlane_history::AgentId; 5] = [
            shardlane_history::AgentId::ClaudeCode,
            shardlane_history::AgentId::Codex,
            shardlane_history::AgentId::Cursor,
            shardlane_history::AgentId::Pi,
            shardlane_history::AgentId::Omp,
        ];
        Self {
            enabled: default_agents
                .into_iter()
                .map(|agent| agent.as_str().to_string())
                .collect(),
        }
    }
}

impl ProvidersConfig {
    pub fn is_enabled(&self, agent: shardlane_history::AgentId) -> bool {
        self.enabled.contains(agent.as_str())
    }

    pub fn set_enabled(&mut self, agent: shardlane_history::AgentId, enabled: bool) {
        let slug = agent.as_str();
        if enabled {
            self.enabled.insert(slug.to_string());
        } else {
            self.enabled.remove(slug);
        }
    }

    /// PP5 unified Picker projection: the list of providers that are product-visible (not Hidden) and user-enabled.
    /// The New Agent Picker, History Continue-as, and Chat entry all share this single source.
    pub fn available_choices(&self) -> Vec<shardlane_history::AgentId> {
        shardlane_history::exposed_agents()
            .into_iter()
            .filter(|agent| self.enabled.contains(agent.as_str()))
            .collect()
    }
}

// The Shardlane Workspace config data model (WorkspacesConfig/WorkspaceConfig/palette/parsing)
// lives in shardlane-host::workspace_config; the legacy client-side grouping feature that
// consumed it was deleted (2026-09-01 multi-instance cleanup), so the GUI no longer re-exports it.

/// Interface language of Shardlane's native shell (i18n). Only English ships
/// today; adding a language = a new variant + `locales/<code>.yml` in
/// `crates/herdr-gui`. The catalog and gpui-component's own strings share one
/// process-global locale owned by `crate::i18n`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Language {
    #[default]
    En,
}

impl serde::Serialize for Language {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.code())
    }
}

impl Language {
    /// Every supported language (Settings menu order).
    pub const ALL: [Language; 1] = [Language::En];

    /// Locale code matching the `locales/<code>.yml` file stem.
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
        }
    }

    /// Native display name: a language is always listed in its own language.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::En => "English",
        }
    }

    /// Lenient config-value parse: unknown values (a locale from a newer
    /// release, a typo) fall back to the default instead of failing the whole
    /// config load — the next canonical save writes the supported spelling.
    pub fn from_config_value(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "en" => Self::En,
            _ => Self::default(),
        }
    }
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::from_config_value(&raw))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct UiConfig {
    /// Interface language (i18n) of the native shell. See `crate::i18n`.
    pub language: Language,
    /// Global light/dark policy.
    pub appearance: String,
    /// Application color scheme independent from light/dark policy.
    pub color_scheme: String,
    pub window: WindowConfig,
    pub sidebar: SidebarConfig,
    /// Right panel (Files/Lazygit/Browser) layout. Same semantics as a right_panel_width.
    pub right_panel: RightPanelConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            language: Language::default(),
            appearance: "system".to_string(),
            color_scheme: "shardlane-native".to_string(),
            window: WindowConfig::default(),
            sidebar: SidebarConfig::default(),
            right_panel: RightPanelConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct RightPanelConfig {
    /// Right panel pixel width, updated while dragging the divider (clamped 280–1000).
    pub width: f64,
}

impl Default for RightPanelConfig {
    fn default() -> Self {
        Self { width: 440.0 }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct WindowConfig {
    pub opacity: f32,
    pub always_on_top: bool,
    /// Last window frame width. 0.0 means never recorded; built-in defaults are used then.
    pub width: f64,
    /// Last window frame height. 0.0 means never recorded.
    pub height: f64,
    /// Last window frame x coordinate (logical pixels). 0.0 means never recorded.
    pub x: f64,
    /// Last window frame y coordinate (logical pixels). 0.0 means never recorded.
    pub y: f64,
}

impl WindowConfig {
    /// Whether a real window frame was ever recorded (credible only when all are non-zero).
    pub fn has_bounds(&self) -> bool {
        self.width > 0.0 && self.height > 0.0
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            always_on_top: false,
            width: 0.0,
            height: 0.0,
            x: 0.0,
            y: 0.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct SidebarConfig {
    pub width: f64,
    pub collapsed: bool,
    pub project_section_height: f64,
    pub agent_section_height: f64,
    pub service_section_height: f64,
    pub projects_collapsed: bool,
    pub agents_collapsed: bool,
    pub services_collapsed: bool,
    #[serde(default)]
    pub pinned_tabs: Vec<String>,
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            width: SIDEBAR_DEFAULT_WIDTH,
            collapsed: false,
            project_section_height: 0.0,
            agent_section_height: 0.0,
            service_section_height: 0.0,
            projects_collapsed: false,
            agents_collapsed: false,
            services_collapsed: false,
            pinned_tabs: Vec::new(),
        }
    }
}

/// Normal terminal presentation is exclusively the hosted Herdr TUI
/// The former `TerminalSurfaceMode`/`surface_mode` Embedded/TUI
/// selector was removed at the TUI-only cutover. Stale `surface_mode` keys in old
/// config files are ignored by serde's unknown-field behavior and disappear on the
/// next canonical save; no permanent migration machinery exists.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub font_family: String,
    pub font_size: f32,
    pub line_height: f32,
    /// Inset between the Terminal surface edge and the character grid, in logical pixels.
    pub padding: f32,
    /// Forces the caret shape when not `follow-terminal` (the running program's
    /// DECSCUSR/defaults remain authoritative in `follow-terminal` mode).
    pub cursor_style: TerminalCursorStylePreference,
    /// Forces caret blinking on/off; `follow-terminal` defers to the program's
    /// blink mode (DECTCEM-adjacent `?12` semantics projected by the VT model).
    pub cursor_blink: TerminalCursorBlinkPreference,
    /// Where the active Project's Tab list is presented: the canonical Sidebar
    /// tree, or a native Tab strip at the top of the content area (both remain
    /// pure presentations of Herdr's authoritative Tab order).
    pub tab_bar_placement: TabBarPlacement,
}

/// Presentation target for the Project Tab list (client-owned display preference;
/// Tab runtime state stays Herdr-authoritative either way).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TabBarPlacement {
    /// Tabs render as Sidebar rows under their Project (current default).
    #[default]
    Sidebar,
    /// Tabs render as a native Tab strip above the hosted terminal content;
    /// the Sidebar keeps Projects without the per-Tab subtree.
    Native,
}

/// Lazygit startup and Shardlane-owned overlay preferences. User/repository Lazygit
/// configuration remains authoritative for every key not explicitly represented here.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct LazygitConfig {
    /// Empty means resolve `lazygit` through PATH.
    pub executable_path: String,
    pub startup_panel: LazygitStartupPanel,
    pub screen_mode: LazygitScreenMode,
    /// Enable the minimal generated Shardlane overlay (never edits user/repository files).
    pub integration_enabled: bool,
    /// Mirror Lazygit's native mouse event support in the hosted PTY.
    pub mouse_events: bool,
    /// Keep Lazygit refreshing status while the surface is visible.
    pub auto_refresh: bool,
    /// Keep the generated overlay's side panel readable in a narrow right panel.
    pub side_panel_width: u8,
}

impl Default for LazygitConfig {
    fn default() -> Self {
        Self {
            executable_path: String::new(),
            startup_panel: LazygitStartupPanel::Status,
            screen_mode: LazygitScreenMode::Normal,
            integration_enabled: true,
            mouse_events: true,
            auto_refresh: true,
            side_panel_width: 25,
        }
    }
}

impl LazygitConfig {
    pub fn normalized(mut self) -> Self {
        self.executable_path = self.executable_path.trim().to_string();
        self.side_panel_width = self.side_panel_width.clamp(15, 60);
        self
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LazygitStartupPanel {
    #[default]
    Status,
    Branch,
    Log,
    Stash,
}

impl LazygitStartupPanel {
    pub fn cli_value(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Branch => "branch",
            Self::Log => "log",
            Self::Stash => "stash",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LazygitScreenMode {
    #[default]
    Normal,
    Half,
    Full,
}

impl LazygitScreenMode {
    pub fn cli_value(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Half => "half",
            Self::Full => "full",
        }
    }
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            font_family: "Menlo".to_string(),
            font_size: 12.0,
            line_height: 18.0,
            padding: 8.0,
            cursor_style: TerminalCursorStylePreference::FollowTerminal,
            cursor_blink: TerminalCursorBlinkPreference::FollowTerminal,
            tab_bar_placement: TabBarPlacement::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct BehaviorConfig {
    pub agent_notifications: bool,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            agent_notifications: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalCursorStylePreference {
    #[default]
    FollowTerminal,
    Block,
    Bar,
    Underline,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalCursorBlinkPreference {
    #[default]
    FollowTerminal,
    On,
    Off,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConfigDiff {
    pub language: bool,
    pub theme: bool,
    pub window: bool,
    pub sidebar: bool,
    pub right_panel: bool,
    pub terminal: bool,
    pub behavior: bool,
    /// SCT-03: any new domain must join the diff or hot reload silently drops it.
    pub shortcuts: bool,
    pub browser: bool,
    pub lazygit: bool,
    pub remote: bool,
    pub providers: bool,
    pub history_sources: bool,
}

impl ConfigDiff {
    pub fn between(current: &ApplicationConfig, next: &ApplicationConfig) -> Self {
        Self {
            language: current.ui.language != next.ui.language,
            theme: current.ui.appearance != next.ui.appearance
                || current.ui.color_scheme != next.ui.color_scheme,
            window: current.ui.window != next.ui.window,
            sidebar: current.ui.sidebar != next.ui.sidebar,
            right_panel: current.ui.right_panel != next.ui.right_panel,
            terminal: current.terminal != next.terminal,
            behavior: current.behavior != next.behavior,
            shortcuts: current.shortcuts != next.shortcuts,
            browser: current.browser != next.browser,
            lazygit: current.lazygit != next.lazygit,
            remote: current.remote != next.remote,
            providers: current.providers != next.providers,
            history_sources: current.history_sources != next.history_sources,
        }
    }

    pub fn is_empty(self) -> bool {
        self == Self::default()
    }
}

impl ApplicationConfig {
    pub fn load() -> Self {
        match Self::load_or_default() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("{error}");
                Self::default()
            }
        }
    }

    pub fn load_strict() -> Result<Self, ConfigError> {
        Self::load_from_path(&config_path())
    }

    /// Machine-list persistence (machine panel: local + added SSH targets).
    pub(crate) fn persist_machines(devices: Vec<DeviceEntry>) {
        let Ok(mut config) = Self::load_strict() else {
            return;
        };
        config.devices = devices;
        config.save();
    }

    /// Open-window snapshot persistence (B4/C4): load-modify-save so a restore
    /// snapshot never clobbers other settings another window just wrote.
    pub(crate) fn persist_open_workspaces(records: Vec<OpenWorkspaceRecord>) {
        let Ok(mut config) = Self::load_strict() else {
            return;
        };
        config.open_workspaces = records;
        config.save();
    }

    fn load_or_default() -> Result<Self, ConfigError> {
        let path = config_path();
        if path.exists() {
            return Self::load_from_path(&path);
        }
        let config = Self::default();
        config.save_result()?;
        Ok(config)
    }

    pub fn load_from_path(path: &Path) -> Result<Self, ConfigError> {
        let json = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let config = serde_json::from_str::<Self>(&json).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(config.normalized())
    }

    pub fn save(&self) {
        if let Err(error) = self.save_result() {
            eprintln!("{error}");
        }
    }

    pub fn save_result(&self) -> Result<(), ConfigError> {
        self.save_to_path(&config_path())
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), ConfigError> {
        let normalized = self.clone().normalized();
        let json = serde_json::to_string_pretty(&normalized)?;
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(dir).map_err(|source| ConfigError::CreateDir {
            path: dir.to_path_buf(),
            source,
        })?;
        let temp_path = path.with_extension("json.tmp");
        std::fs::write(&temp_path, format!("{json}\n")).map_err(|source| ConfigError::Write {
            path: temp_path.clone(),
            source,
        })?;
        std::fs::rename(&temp_path, path).map_err(|source| ConfigError::Write {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(())
    }

    pub fn normalized(mut self) -> Self {
        self.version = CONFIG_VERSION;
        // Machines: seed the local machine entry for configs without one.
        if self.devices.is_empty() {
            self.devices.push(DeviceEntry::local());
        }
        let appearance = self.ui.appearance.trim().to_string();
        self.ui.appearance = match appearance.as_str() {
            "" | "system" => "system".to_string(),
            "dark" | "system-dark" => "dark".to_string(),
            "light" | "system-light" => "light".to_string(),
            _ => "system".to_string(),
        };
        if self.ui.color_scheme.trim().is_empty() {
            self.ui.color_scheme = "shardlane-native".to_string();
        }
        self.ui.sidebar.width = self.ui.sidebar.width.clamp(176.0, 520.0);
        self.ui.right_panel.width = self.ui.right_panel.width.clamp(280.0, 1000.0);
        for height in [
            &mut self.ui.sidebar.project_section_height,
            &mut self.ui.sidebar.agent_section_height,
            &mut self.ui.sidebar.service_section_height,
        ] {
            if *height > 0.0 {
                *height = height.clamp(72.0, 1_200.0);
            }
        }
        // Audit D02: an explicitly configured ListenerMode::Loopback is honored as-is
        // (binds 127.0.0.1 only). The historical silent widening to LocalNetwork is gone;
        // fresh installs still start disabled/off (RemoteConfig::default()).
        // ApplicationConfig's whole-document deserialization doesn't go through from_settings_value;
        // here port==0 (missing field/old config) is likewise normalized to the default port so a
        // random ephemeral port can't break pairing.
        if self.remote.port == 0 {
            self.remote.port = shardlane_remote::DEFAULT_REMOTE_PORT;
        }
        self.terminal.padding = self.terminal.padding.clamp(0.0, 32.0);
        self.ui.window.opacity = self.ui.window.opacity.clamp(0.55, 1.0);
        if self.terminal.font_family.trim().is_empty() {
            self.terminal.font_family = "Menlo".to_string();
        }
        self.terminal.font_size = self.terminal.font_size.clamp(10.0, 24.0);
        self.terminal.line_height = self.terminal.line_height.clamp(14.0, 34.0);
        self.lazygit = self.lazygit.normalized();
        self.history_sources = self.history_sources.normalized();
        self
    }
}

pub struct ConfigWatcher {
    _watcher: notify::RecommendedWatcher,
    changed_rx: Receiver<()>,
}

impl ConfigWatcher {
    pub fn start() -> Option<Self> {
        Self::start_path(config_path())
    }

    fn start_path(path: PathBuf) -> Option<Self> {
        let parent = path.parent()?.to_path_buf();
        std::fs::create_dir_all(&parent).ok()?;
        let file_name = path.file_name()?.to_owned();
        let (changed_tx, changed_rx) = async_channel::bounded(1);
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else {
                    return;
                };
                if event
                    .paths
                    .iter()
                    .any(|event_path| event_path.file_name() == Some(file_name.as_os_str()))
                {
                    let _ = changed_tx.try_send(());
                }
            })
            .ok()?;
        watcher.watch(&parent, RecursiveMode::NonRecursive).ok()?;
        Some(Self {
            _watcher: watcher,
            changed_rx,
        })
    }

    pub fn changed_receiver(&self) -> Receiver<()> {
        self.changed_rx.clone()
    }
}

pub fn app_data_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".shardlane")
}

pub fn config_path() -> PathBuf {
    app_data_dir().join(CONFIG_FILE_NAME)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn normalized_keeps_remote_port_default_and_explicit() {
        // Default config port=8757 (RemoteConfig::default); explicit config is preserved.
        let mut config = ApplicationConfig::default();
        assert_eq!(config.remote.port, shardlane_remote::DEFAULT_REMOTE_PORT);
        config = config.normalized();
        assert_eq!(config.remote.port, shardlane_remote::DEFAULT_REMOTE_PORT);

        // Old configs explicitly wrote 0: normalized normalizes to the default port so a random ephemeral port can't break pairing.
        config.remote.port = 0;
        config = config.normalized();
        assert_eq!(config.remote.port, shardlane_remote::DEFAULT_REMOTE_PORT);

        // An explicitly configured port must be preserved.
        config.remote.port = 9100;
        config = config.normalized();
        assert_eq!(config.remote.port, 9100);
    }

    #[test]
    fn normalized_honors_explicit_loopback_listener() {
        // Audit D02: an explicitly configured Loopback must survive load/normalize/save
        // instead of being silently widened to LocalNetwork (0.0.0.0). Fresh installs
        // still start Off/disabled (RemoteConfig::default()).
        let mut config = ApplicationConfig::default();
        assert_eq!(
            config.remote.listener_mode,
            shardlane_remote::ListenerMode::Off
        );
        config.remote.listener_mode = shardlane_remote::ListenerMode::Loopback;
        config = config.normalized();
        assert_eq!(
            config.remote.listener_mode,
            shardlane_remote::ListenerMode::Loopback
        );
        let json = serde_json::to_string(&config).unwrap_or_else(|error| panic!("{error}"));
        let restored: ApplicationConfig =
            serde_json::from_str(&json).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            restored.remote.listener_mode,
            shardlane_remote::ListenerMode::Loopback
        );
    }

    #[test]
    fn window_bounds_round_trip() {
        let mut config = ApplicationConfig::default();
        config.ui.window.width = 1440.0;
        config.ui.window.height = 900.0;
        config.ui.window.x = 120.0;
        config.ui.window.y = 64.0;
        assert!(config.ui.window.has_bounds());
        let json = serde_json::to_string(&config).unwrap_or_else(|error| panic!("{error}"));
        let restored: ApplicationConfig =
            serde_json::from_str(&json).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(restored.ui.window.width, 1440.0);
        assert_eq!(restored.ui.window.height, 900.0);
        assert_eq!(restored.ui.window.x, 120.0);
        assert_eq!(restored.ui.window.y, 64.0);
        assert!(restored.ui.window.has_bounds());
    }

    #[test]
    fn window_bounds_legacy_defaults_to_unrecorded() {
        // v5 configs were written before the bounds fields existed; serde(default) must fall back to "never recorded".
        let legacy = r#"{
            "version": 5,
            "ui": { "appearance": "system", "color_scheme": "shardlane-native" }
        }"#;
        let config: ApplicationConfig =
            serde_json::from_str(legacy).unwrap_or_else(|error| panic!("{error}"));
        assert!(!config.ui.window.has_bounds());
        assert_eq!(config.ui.window.width, 0.0);
        assert_eq!(config.ui.window.height, 0.0);
        assert_eq!(config.ui.window.opacity, 1.0);
    }

    #[test]
    fn terminal_cursor_preferences_round_trip_and_default() {
        let config: ApplicationConfig = serde_json::from_str::<ApplicationConfig>(
            r#"{"terminal": {"cursor_style": "underline", "cursor_blink": "off"}}"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            config.terminal.cursor_style,
            TerminalCursorStylePreference::Underline
        );
        assert_eq!(
            config.terminal.cursor_blink,
            TerminalCursorBlinkPreference::Off
        );

        let mut config = ApplicationConfig::default();
        assert_eq!(
            config.terminal.cursor_style,
            TerminalCursorStylePreference::FollowTerminal
        );
        assert_eq!(
            config.terminal.cursor_blink,
            TerminalCursorBlinkPreference::FollowTerminal
        );
        config.terminal.cursor_style = TerminalCursorStylePreference::Bar;
        config.terminal.cursor_blink = TerminalCursorBlinkPreference::On;
        let value = serde_json::to_value(&config).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(value["terminal"]["cursor_style"], "bar");
        assert_eq!(value["terminal"]["cursor_blink"], "on");
    }

    #[test]
    fn lazygit_config_round_trips_and_normalizes() {
        let config: ApplicationConfig = serde_json::from_str::<ApplicationConfig>(
            r#"{
                "lazygit": {
                    "executable_path": "  /opt/homebrew/bin/lazygit  ",
                    "startup_panel": "branch",
                    "screen_mode": "full",
                    "integration_enabled": false,
                    "mouse_events": false,
                    "auto_refresh": true,
                    "side_panel_width": 99
                }
            }"#,
        )
        .unwrap_or_else(|error| panic!("{error}"))
        .normalized();
        assert_eq!(config.lazygit.executable_path, "/opt/homebrew/bin/lazygit");
        assert_eq!(config.lazygit.startup_panel, LazygitStartupPanel::Branch);
        assert_eq!(config.lazygit.screen_mode, LazygitScreenMode::Full);
        assert!(!config.lazygit.integration_enabled);
        assert!(!config.lazygit.mouse_events);
        assert_eq!(config.lazygit.side_panel_width, 60);

        let value = serde_json::to_value(&config).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(value["lazygit"]["startup_panel"], "branch");
        assert_eq!(value["lazygit"]["screen_mode"], "full");
    }

    #[test]
    fn application_config_serializes_as_domain_owned_json() {
        let mut config = ApplicationConfig::default();
        config.ui.appearance = "dark".into();
        config.ui.color_scheme = "catppuccin".into();
        config.ui.sidebar.project_section_height = 512.0;
        config.ui.window.always_on_top = true;
        config.terminal.font_family = "Berkeley Mono".into();
        let value = serde_json::to_value(&config).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(value["version"], CONFIG_VERSION);
        assert_eq!(value["ui"]["appearance"], "dark");
        assert_eq!(value["ui"]["color_scheme"], "catppuccin");
        assert!(value["ui"].get("theme").is_none());
        assert_eq!(value["ui"]["sidebar"]["project_section_height"], 512.0);
        assert_eq!(value["terminal"]["font_family"], "Berkeley Mono");
        assert_eq!(value["terminal"]["padding"], 8.0);
        assert!(value.get("sidebar_width").is_none());
    }

    #[test]
    fn legacy_workspace_grouping_keys_are_ignored_and_dropped_on_save() {
        // 2026-09-01 cleanup: the client-side Workspace grouping keys are gone from
        // the model. Old config files may still contain them; serde ignores unknown
        // fields, the app never reads them back, and the next canonical save omits them.
        let legacy = serde_json::json!({
            "workspaces": {
                "active_id": "ws-1",
                "items": [{
                    "id": "ws-1",
                    "name": "Default",
                    "color": "#7C8CFF",
                    "project_paths": ["/work/demo"]
                }]
            },
            "project_path_overrides": { "w1": "/work/demo" },
            "project_path_last_seen_ms": { "/work/demo": 1234 },
            "workspace_dormant_project_paths": { "ws-1": ["/work/demo"] },
            "behavior": { "workspace_restore_policy": "always" }
        });
        let restored: ApplicationConfig =
            serde_json::from_value(legacy).unwrap_or_else(|error| panic!("{error}"));
        let value = serde_json::to_value(&restored).unwrap_or_else(|error| panic!("{error}"));
        assert!(value.get("workspaces").is_none());
        assert!(value.get("project_path_overrides").is_none());
        assert!(value.get("project_path_last_seen_ms").is_none());
        assert!(value.get("workspace_dormant_project_paths").is_none());
        assert!(value["behavior"].get("workspace_restore_policy").is_none());
    }

    #[test]
    fn stale_surface_mode_key_is_ignored_and_dropped_on_save() {
        // TUI-only cutover: the removed Embedded/TUI selector leaves a stale
        // `surface_mode` key in old config files. Serde ignores unknown fields,
        // the app never reads it back, and the next canonical save omits it.
        let stale = serde_json::json!({
            "terminal": {
                "surface_mode": "tui",
                "font_size": 13.0,
            }
        });
        let restored: ApplicationConfig = serde_json::from_value(stale).unwrap();
        assert_eq!(restored.terminal.font_size, 13.0);
        let value = serde_json::to_value(&restored).unwrap();
        assert!(value["terminal"].get("surface_mode").is_none());
        // Fresh defaults likewise carry no mode key at all.
        let fresh = serde_json::to_value(ApplicationConfig::default()).unwrap();
        assert!(fresh["terminal"].get("surface_mode").is_none());
    }

    #[test]
    fn invalid_external_values_are_normalized_before_application() {
        let config = ApplicationConfig {
            ui: UiConfig {
                language: Language::En,
                appearance: String::new(),
                color_scheme: String::new(),
                window: WindowConfig {
                    opacity: 0.1,
                    always_on_top: false,
                    ..WindowConfig::default()
                },
                sidebar: SidebarConfig {
                    width: 10_000.0,
                    project_section_height: 5.0,
                    ..SidebarConfig::default()
                },
                right_panel: RightPanelConfig { width: 5_000.0 },
            },
            terminal: TerminalConfig {
                font_family: String::new(),
                font_size: 99.0,
                line_height: 1.0,
                padding: 99.0,
                ..TerminalConfig::default()
            },
            ..ApplicationConfig::default()
        }
        .normalized();
        assert_eq!(config.ui.appearance, "system");
        assert_eq!(config.ui.color_scheme, "shardlane-native");
        assert_eq!(config.ui.sidebar.width, 520.0);
        assert_eq!(config.ui.sidebar.project_section_height, 72.0);
        assert_eq!(config.ui.window.opacity, 0.55);
        assert_eq!(config.terminal.font_family, "Menlo");
        assert_eq!(config.terminal.font_size, 24.0);
        assert_eq!(config.terminal.line_height, 14.0);
        assert_eq!(config.terminal.padding, 32.0);
    }

    #[test]
    fn config_diff_marks_only_changed_domains() {
        let current = ApplicationConfig::default();
        let mut next = current.clone();
        next.terminal.font_size = 16.0;
        next.behavior.agent_notifications = false;
        let diff = ConfigDiff::between(&current, &next);
        assert!(!diff.theme);
        assert!(!diff.window);
        assert!(!diff.sidebar);
        assert!(diff.terminal);
        assert!(diff.behavior);
        assert!(!diff.lazygit);

        next.lazygit.auto_refresh = false;
        assert!(ConfigDiff::between(&current, &next).lazygit);

        next = current.clone();
        next.providers
            .set_enabled(shardlane_history::AgentId::ClaudeCode, false);
        assert!(ConfigDiff::between(&current, &next).providers);

        next = current.clone();
        next.history_sources
            .custom_roots
            .push(shardlane_history::CustomHistoryRoot {
                agent: shardlane_history::AgentId::Codex,
                path: std::path::PathBuf::from("/tmp/codex-history"),
            });
        assert!(ConfigDiff::between(&current, &next).history_sources);
    }

    #[test]
    fn tab_bar_placement_kebab_round_trip_and_diff() {
        // The user-facing config spelling is kebab-case; the Sidebar default keeps old
        // configs (without the key) unchanged.
        assert_eq!(
            serde_json::to_value(TabBarPlacement::Sidebar).unwrap(),
            serde_json::json!("sidebar")
        );
        assert_eq!(
            serde_json::to_value(TabBarPlacement::Native).unwrap(),
            serde_json::json!("native")
        );
        let legacy: TerminalConfig = serde_json::from_str(r#"{"font_family":"Menlo"}"#)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(legacy.tab_bar_placement, TabBarPlacement::Sidebar);

        let current = ApplicationConfig::default();
        let mut next = current.clone();
        next.terminal.tab_bar_placement = TabBarPlacement::Native;
        assert!(ConfigDiff::between(&current, &next).terminal);
    }

    #[test]
    fn language_round_trips_and_unknown_values_fall_back_to_english() {
        // The supported spelling round-trips through the canonical config JSON.
        let config: ApplicationConfig = serde_json::from_str(r#"{"ui": {"language": "en"}}"#)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(config.ui.language, Language::En);
        assert_eq!(
            serde_json::to_value(config.ui.language).unwrap_or_else(|error| panic!("{error}")),
            serde_json::json!("en")
        );

        // A locale from a newer release (or a typo) must not fail the whole
        // config load: it normalizes to the default and the next canonical
        // save writes the supported spelling.
        let lenient: ApplicationConfig = serde_json::from_str(r#"{"ui": {"language": "zh-CN"}}"#)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(lenient.ui.language, Language::En);
        assert_eq!(
            serde_json::to_value(lenient.ui.language).unwrap_or_else(|error| panic!("{error}")),
            serde_json::json!("en")
        );
    }

    #[test]
    fn config_diff_tracks_language_changes() {
        // With a single variant both sides are En, so the diff must be quiet;
        // the language that lands next extends this test with a true case.
        let current = ApplicationConfig::default();
        let mut next = current.clone();
        next.ui.language = Language::En;
        assert!(!ConfigDiff::between(&current, &next).language);
        assert!(!ConfigDiff::between(&current, &next).theme);
    }

    #[test]
    fn history_source_policy_round_trips_outside_disposable_catalog() {
        let mut config = ApplicationConfig::default();
        config
            .history_sources
            .custom_roots
            .push(shardlane_history::CustomHistoryRoot {
                agent: shardlane_history::AgentId::ClaudeCode,
                path: std::path::PathBuf::from("/Volumes/archive/.claude"),
            });
        let disabled = shardlane_history::HistorySourceKey::new(
            shardlane_history::AgentId::ClaudeCode,
            std::path::PathBuf::from("/Users/demo/.claude/projects"),
        );
        config.history_sources.set_enabled(
            shardlane_history::HistorySourceKind::Default,
            disabled.clone(),
            false,
        );
        let encoded = serde_json::to_string(&config).unwrap_or_else(|error| panic!("{error}"));
        assert!(encoded.contains("history_sources"));
        let restored: ApplicationConfig =
            serde_json::from_str(&encoded).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            restored.history_sources.custom_roots,
            config.history_sources.custom_roots
        );
        assert!(restored
            .history_sources
            .disabled_defaults
            .contains(&disabled));
    }

    #[test]
    fn atomic_config_roundtrip_uses_one_canonical_file() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let path = temp.path().join(CONFIG_FILE_NAME);
        let mut config = ApplicationConfig::default();
        config.terminal.font_size = 17.0;
        config
            .save_to_path(&path)
            .unwrap_or_else(|error| panic!("{error}"));
        let loaded =
            ApplicationConfig::load_from_path(&path).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(loaded, config);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn config_watcher_coalesces_external_file_changes() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let path = temp.path().join(CONFIG_FILE_NAME);
        ApplicationConfig::default()
            .save_to_path(&path)
            .unwrap_or_else(|error| panic!("{error}"));
        let watcher = ConfigWatcher::start_path(path.clone())
            .unwrap_or_else(|| panic!("config watcher unavailable"));
        let mut changed = ApplicationConfig::default();
        changed.ui.color_scheme = "one-dark".into();
        changed
            .save_to_path(&path)
            .unwrap_or_else(|error| panic!("{error}"));
        changed.terminal.font_size = 16.0;
        changed
            .save_to_path(&path)
            .unwrap_or_else(|error| panic!("{error}"));

        let receiver = watcher.changed_receiver();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && receiver.try_recv().is_err() {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            Instant::now() < deadline,
            "watcher did not observe config update"
        );
        assert!(
            receiver.try_recv().is_err(),
            "config changes should coalesce"
        );
    }
}

#[cfg(test)]
mod providers_tests {
    use super::*;

    /// R4 contract: the default instance enables the Stable set (ClaudeCode, Codex, Cursor,
    /// Pi, Omp); Hidden/Preview never enter the default.
    #[test]
    fn default_enabled_covers_the_stable_set() {
        let config = ProvidersConfig::default();
        assert!(config.is_enabled(shardlane_history::AgentId::ClaudeCode));
        assert!(config.is_enabled(shardlane_history::AgentId::Codex));
        assert!(config.is_enabled(shardlane_history::AgentId::Cursor));
        assert!(config.is_enabled(shardlane_history::AgentId::Pi));
        assert!(config.is_enabled(shardlane_history::AgentId::Omp));
        // Hidden providers never enter the default even when they implement live decoding.
        assert!(!config.is_enabled(shardlane_history::AgentId::Kimi));
        assert!(!config.is_enabled(shardlane_history::AgentId::Grok));
        // Preview is opt-in by default.
        assert!(!config.is_enabled(shardlane_history::AgentId::CommandCode));
        assert!(!config.is_enabled(shardlane_history::AgentId::Qoder));
    }

    /// set_enabled toggle round trip + serde roundtrip preserve the set.
    #[test]
    fn providers_config_roundtrip_and_toggle() {
        let mut config = ProvidersConfig::default();
        config.set_enabled(shardlane_history::AgentId::ClaudeCode, false);
        config.set_enabled(shardlane_history::AgentId::CommandCode, true);
        assert!(!config.is_enabled(shardlane_history::AgentId::ClaudeCode));
        assert!(config.is_enabled(shardlane_history::AgentId::CommandCode));

        let json = serde_json::to_string(&config).unwrap_or_else(|e| panic!("{e}"));
        let parsed: ProvidersConfig = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(parsed, config);

        // Old configs (no providers field) load = the default-enabled set.
        let legacy: ApplicationConfig =
            serde_json::from_str(r#"{"version":5}"#).unwrap_or_else(|e| panic!("{e}"));
        assert!(legacy
            .providers
            .is_enabled(shardlane_history::AgentId::ClaudeCode));
        assert!(!legacy
            .providers
            .is_enabled(shardlane_history::AgentId::Kimi));
    }

    /// PP5: available_choices = exposed_agents ∩ enabled (the sole Picker projection).
    #[test]
    fn available_choices_is_exposed_intersect_enabled() {
        let config = ProvidersConfig::default();
        let choices = config.available_choices();
        // Must be non-empty (at least ClaudeCode).
        assert!(!choices.is_empty());
        // Every choice must be both exposed and enabled.
        for &agent in &choices {
            assert!(
                shardlane_history::provider_exposed(agent),
                "{agent:?} must be exposed"
            );
            assert!(config.is_enabled(agent), "{agent:?} must be enabled");
        }
        // A Hidden provider is not in choices even when enabled.
        let mut custom = ProvidersConfig::default();
        custom.set_enabled(shardlane_history::AgentId::Kimi, true);
        assert!(
            !custom
                .available_choices()
                .contains(&shardlane_history::AgentId::Kimi),
            "Hidden provider must not appear in available_choices"
        );
        // A disabled exposed provider is not in choices.
        let mut limited = ProvidersConfig::default();
        limited.set_enabled(shardlane_history::AgentId::ClaudeCode, false);
        assert!(
            !limited
                .available_choices()
                .contains(&shardlane_history::AgentId::ClaudeCode),
            "disabled provider must not appear in available_choices"
        );
    }
}
