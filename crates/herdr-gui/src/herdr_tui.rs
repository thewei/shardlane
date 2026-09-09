//! Herdr TUI host mode: the Shardlane content area runs the user's everyday `herdr` TUI (singleton PTY).
//!
//! Architecture ruling (2026-08-29): Herdr TUI mode = running an ordinary
//! `herdr` inside the Shardlane content area. There is exactly one Terminal/PTY/Herdr client
//! throughout. The Herdr user config (default `~/.config/herdr/config.toml`, or the path
//! given by `HERDR_CONFIG_PATH`) is the single configuration source of truth; Shardlane
//! Settings is only a visual editor for that config, validated with `herdr config check`
//! before writing and applied via `server.reload_config` after writing. A private TUI
//! config mirror must never be generated again.
//! Shardlane does not reimplement the Herdr TUI and keeps driving focus through the API.
//!
//! [INPUT]: `crate::herdr::HerdrClient` focus wrappers (workspace_focus/tab_focus/
//!          pane_focus/agent_focus), Herdr `pane.layout.area`,
//!          the user's Herdr config.toml
//! [OUTPUT]: `TUI_TARGET` (host surface identifier), user Herdr config read/write/validation,
//!           atomic `ThemeScheme` theme writes (`ThemeAppearanceMode`/`theme_appearance_mode`/
//!           `effective_theme_selections`/`theme_scheme_update`, the Theme page's only theme path),
//!           `execute_focus_plan` (navigation chain), `TuiChromeProjection`, the `HerdrTuiHostState`
//!           lifecycle model, `protocol_supported` (cached metadata check, no RPC)
//! [POS]: The strategy layer for the TUI presentation mode (spawn environment/navigation chain/chrome projection); PTY transport belongs to
//!          `terminal_stream.rs`, rendering to the existing Ghostty/GPUI stack, and all terminal semantics to Herdr

use crate::ghostty::TerminalFrame;
use crate::herdr::LayoutRect;
use shardlane_host::diagnostics::lag_log;
use std::path::{Path, PathBuf};
use std::process::Command;
use toml_edit::{value, DocumentMut};

#[derive(Clone, Debug, PartialEq)]
pub struct HerdrUserConfigSnapshot {
    pub theme_name: String,
    pub theme_auto_switch: bool,
    pub theme_light_name: String,
    pub theme_dark_name: String,
    pub copy_on_select: bool,
    pub mouse_capture: bool,
    pub mouse_scroll_lines: i64,
    pub sidebar_start_collapsed: bool,
    pub sidebar_collapsed_mode: String,
    pub pane_borders: bool,
    pub pane_outer_borders: bool,
    pub pane_scrollbars: bool,
    pub pane_gaps: bool,
    pub hide_tab_bar_when_single_tab: bool,
    pub tab_bar_position: String,
    pub status_indicators: String,
}

impl Default for HerdrUserConfigSnapshot {
    fn default() -> Self {
        Self {
            theme_name: "catppuccin".to_string(),
            theme_auto_switch: false,
            theme_light_name: String::new(),
            theme_dark_name: String::new(),
            copy_on_select: true,
            mouse_capture: true,
            mouse_scroll_lines: 3,
            sidebar_start_collapsed: false,
            sidebar_collapsed_mode: "compact".to_string(),
            pane_borders: true,
            pane_outer_borders: true,
            pane_scrollbars: true,
            pane_gaps: true,
            hide_tab_bar_when_single_tab: false,
            tab_bar_position: "top".to_string(),
            status_indicators: "dots".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum HerdrUserConfigUpdate {
    ThemeName(String),
    /// One atomic Theme-page write: appearance mode plus the selected light/dark
    /// Herdr built-ins. `name` carries the active single theme for manual modes and
    /// a valid fallback theme for auto mode; Herdr's own auto-switch reads
    /// `light_name`/`dark_name` directly.
    ThemeScheme {
        auto_switch: bool,
        name: String,
        light_name: String,
        dark_name: String,
    },
    CopyOnSelect(bool),
    MouseCapture(bool),
    MouseScrollLines(i64),
    SidebarStartCollapsed(bool),
    SidebarCollapsedMode(String),
    PaneBorders(bool),
    PaneOuterBorders(bool),
    PaneScrollbars(bool),
    PaneGaps(bool),
    HideTabBarWhenSingleTab(bool),
    TabBarPosition(String),
    StatusIndicators(String),
}

impl HerdrUserConfigUpdate {
    pub fn is_theme_change(&self) -> bool {
        matches!(self, Self::ThemeName(_) | Self::ThemeScheme { .. })
    }
}

/// The Theme page's appearance mode: manual light, manual dark, or Herdr's
/// light/dark auto-switch following the host appearance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeAppearanceMode {
    Light,
    Dark,
    Auto,
}

/// Derive the Theme page appearance mode from a Herdr config snapshot. Manual mode
/// follows the category of the active built-in; a custom (non-preset) theme falls
/// back to Shardlane's synced appearance preference.
pub fn theme_appearance_mode(
    config: &HerdrUserConfigSnapshot,
    fallback_appearance: &str,
) -> ThemeAppearanceMode {
    if config.theme_auto_switch {
        return ThemeAppearanceMode::Auto;
    }
    match crate::theme::preset_for_herdr_theme(&config.theme_name) {
        Some(preset) if preset.category == crate::theme::ThemePresetCategory::Light => {
            ThemeAppearanceMode::Light
        }
        Some(_) => ThemeAppearanceMode::Dark,
        None => {
            if fallback_appearance == "light" {
                ThemeAppearanceMode::Light
            } else {
                ThemeAppearanceMode::Dark
            }
        }
    }
}

/// Effective Theme-page selections: empty Herdr `light_name`/`dark_name` fields fall
/// back to the active manual theme when it matches the category, then to Herdr's
/// stock light/dark built-ins. Returns `(light_name, dark_name)`.
pub fn effective_theme_selections(config: &HerdrUserConfigSnapshot) -> (String, String) {
    let current = crate::theme::preset_for_herdr_theme(&config.theme_name);
    let light = if !config.theme_light_name.is_empty() {
        config.theme_light_name.clone()
    } else if current
        .is_some_and(|preset| preset.category == crate::theme::ThemePresetCategory::Light)
    {
        config.theme_name.clone()
    } else {
        "catppuccin-latte".to_string()
    };
    let dark = if !config.theme_dark_name.is_empty() {
        config.theme_dark_name.clone()
    } else if current
        .is_some_and(|preset| preset.category == crate::theme::ThemePresetCategory::Dark)
    {
        config.theme_name.clone()
    } else {
        "catppuccin".to_string()
    };
    (light, dark)
}

/// Resolve a Theme-page intent (mode + light/dark selections) into one atomic config
/// write. Manual modes make the picked appearance's selection the single active
/// theme (`name`); auto mode keeps the current built-in as `name` fallback when it
/// is one of the official presets.
pub fn theme_scheme_update(
    mode: ThemeAppearanceMode,
    light_name: &str,
    dark_name: &str,
    current_name: &str,
) -> HerdrUserConfigUpdate {
    let name = match mode {
        ThemeAppearanceMode::Light => light_name.to_string(),
        ThemeAppearanceMode::Dark => dark_name.to_string(),
        ThemeAppearanceMode::Auto => {
            if crate::theme::preset_for_herdr_theme(current_name).is_some() {
                current_name.to_string()
            } else {
                light_name.to_string()
            }
        }
    };
    HerdrUserConfigUpdate::ThemeScheme {
        auto_switch: mode == ThemeAppearanceMode::Auto,
        name,
        light_name: light_name.to_string(),
        dark_name: dark_name.to_string(),
    }
}

/// TUI host surface identifier (terminal_target value; namespace-isolated from controllers).
pub const TUI_TARGET: &str = "herdr-tui";

/// Herdr-owned navigation chrome surrounding the authoritative Pane area inside the
/// hosted TUI. Shardlane never redraws that chrome; it projects only the Pane rectangle
/// while the full `herdr` process keeps running off-screen behind the same PTY.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TuiChromeProjection {
    pub left: u16,
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
}

impl TuiChromeProjection {
    pub fn from_layout(outer_cols: u16, outer_rows: u16, area: LayoutRect) -> Option<Self> {
        let left = u16::try_from(area.x).ok()?;
        let raw_top = u16::try_from(area.y).ok()?;
        let width = u16::try_from(area.width).ok()?;
        let height = u16::try_from(area.height).ok()?;
        if width == 0
            || height == 0
            || left.saturating_add(width) > outer_cols
            || raw_top.saturating_add(height) > outer_rows
        {
            return None;
        }
        // In Herdr 0.8, the top Tab bar was reflected in `area.y = 1`. In Herdr 0.9+, `area.y`
        // is reported relative to the pane area (`area.y = 0`), while `area.height` remains
        // `outer_rows - top_chrome` (e.g. 39 rows for a 40-row terminal with a 1-row tab bar).
        // When `raw_top == 0` and `height < outer_rows`, the missing vertical rows belong to
        // the top chrome (Herdr desktop tab bar at row 0), never the bottom.
        let top = if raw_top > 0 {
            raw_top
        } else {
            outer_rows.saturating_sub(height)
        };
        let bottom = outer_rows.saturating_sub(top.saturating_add(height));
        Some(Self {
            left,
            top,
            right: outer_cols.saturating_sub(left.saturating_add(width)),
            bottom,
        })
    }

    pub fn is_empty(self) -> bool {
        self.left == 0 && self.top == 0 && self.right == 0 && self.bottom == 0
    }

    pub fn outer_grid(self, visible_cols: u16, visible_rows: u16) -> (u16, u16) {
        (
            visible_cols
                .saturating_add(self.left)
                .saturating_add(self.right),
            visible_rows
                .saturating_add(self.top)
                .saturating_add(self.bottom),
        )
    }

    pub fn project_frame(self, frame: &TerminalFrame) -> TerminalFrame {
        frame.project_rect(self.left, self.top, self.right, self.bottom)
    }

    pub fn visible_to_raw_cell(self, cell: (u16, u16)) -> (u16, u16) {
        (
            cell.0.saturating_add(self.left),
            cell.1.saturating_add(self.top),
        )
    }

    pub fn raw_to_visible_cell(self, cell: (u16, u16)) -> Option<(u16, u16)> {
        (cell.0 >= self.left && cell.1 >= self.top).then_some((
            cell.0.saturating_sub(self.left),
            cell.1.saturating_sub(self.top),
        ))
    }

    pub fn visible_to_raw_selection(
        self,
        selection: ((u16, u16), (u16, u16)),
    ) -> ((u16, u16), (u16, u16)) {
        (
            self.visible_to_raw_cell(selection.0),
            self.visible_to_raw_cell(selection.1),
        )
    }

    pub fn raw_to_visible_selection(
        self,
        selection: ((u16, u16), (u16, u16)),
    ) -> Option<((u16, u16), (u16, u16))> {
        Some((
            self.raw_to_visible_cell(selection.0)?,
            self.raw_to_visible_cell(selection.1)?,
        ))
    }
}

/// Build a spawn environment with herdr identity variables stripped. Pure function.
pub fn sanitized_spawn_env(
    source: &std::collections::HashMap<String, String>,
) -> Vec<(String, String)> {
    shardlane_host::herdr::sanitized_tui_env(source)
}

pub fn herdr_user_config_path() -> PathBuf {
    shardlane_host::herdr::herdr_user_config_path()
}

fn load_system_config_document(path: &Path) -> Result<DocumentMut, String> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    let source = std::fs::read_to_string(path)
        .map_err(|error| format!("read Herdr config {}: {error}", path.display()))?;
    source
        .parse::<DocumentMut>()
        .map_err(|error| format!("parse Herdr config {}: {error}", path.display()))
}

fn config_item<'a>(
    document: &'a DocumentMut,
    table_name: &str,
    key: &str,
) -> Option<&'a toml_edit::Item> {
    document
        .as_table()
        .get(table_name)
        .and_then(|item| item.as_table_like())
        .and_then(|table| table.get(key))
}

fn herdr_user_config_snapshot_from_document(document: &DocumentMut) -> HerdrUserConfigSnapshot {
    HerdrUserConfigSnapshot {
        theme_name: config_item(document, "theme", "name")
            .and_then(|item| item.as_str())
            .unwrap_or("catppuccin")
            .to_string(),
        theme_auto_switch: config_item(document, "theme", "auto_switch")
            .and_then(|item| item.as_bool())
            .unwrap_or(false),
        theme_light_name: config_item(document, "theme", "light_name")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string(),
        theme_dark_name: config_item(document, "theme", "dark_name")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .to_string(),
        copy_on_select: config_item(document, "ui", "copy_on_select")
            .and_then(|item| item.as_bool())
            .unwrap_or(true),
        mouse_capture: config_item(document, "ui", "mouse_capture")
            .and_then(|item| item.as_bool())
            .unwrap_or(true),
        mouse_scroll_lines: config_item(document, "ui", "mouse_scroll_lines")
            .and_then(|item| item.as_integer())
            .unwrap_or(3),
        sidebar_start_collapsed: config_item(document, "ui", "sidebar_start_collapsed")
            .and_then(|item| item.as_bool())
            .unwrap_or(false),
        sidebar_collapsed_mode: config_item(document, "ui", "sidebar_collapsed_mode")
            .and_then(|item| item.as_str())
            .unwrap_or("compact")
            .to_string(),
        pane_borders: config_item(document, "ui", "pane_borders")
            .and_then(|item| item.as_bool())
            .unwrap_or(true),
        pane_outer_borders: config_item(document, "ui", "pane_outer_borders")
            .and_then(|item| item.as_bool())
            .unwrap_or(true),
        pane_scrollbars: config_item(document, "ui", "pane_scrollbars")
            .and_then(|item| item.as_bool())
            .unwrap_or(true),
        pane_gaps: config_item(document, "ui", "pane_gaps")
            .and_then(|item| item.as_bool())
            .unwrap_or(true),
        hide_tab_bar_when_single_tab: config_item(document, "ui", "hide_tab_bar_when_single_tab")
            .and_then(|item| item.as_bool())
            .unwrap_or(false),
        tab_bar_position: config_item(document, "ui", "tab_bar_position")
            .and_then(|item| item.as_str())
            .unwrap_or("top")
            .to_string(),
        status_indicators: config_item(document, "ui", "status_indicators")
            .and_then(|item| item.as_str())
            .unwrap_or("dots")
            .to_string(),
    }
}

pub fn load_herdr_user_config() -> Result<HerdrUserConfigSnapshot, String> {
    let path = herdr_user_config_path();
    let document = load_system_config_document(&path)?;
    Ok(herdr_user_config_snapshot_from_document(&document))
}

fn apply_herdr_user_config_update(
    document: &mut DocumentMut,
    update: &HerdrUserConfigUpdate,
) -> Result<(), String> {
    match update {
        HerdrUserConfigUpdate::ThemeName(name) => {
            document["theme"]["name"] = value(name.clone());
            document["theme"]["auto_switch"] = value(false);
            if let Some(preset) = crate::theme::preset_for_herdr_theme(name) {
                match preset.category {
                    crate::theme::ThemePresetCategory::Light => {
                        document["theme"]["light_name"] = value(name.clone());
                    }
                    crate::theme::ThemePresetCategory::Dark => {
                        document["theme"]["dark_name"] = value(name.clone());
                    }
                }
            }
        }
        HerdrUserConfigUpdate::ThemeScheme {
            auto_switch,
            name,
            light_name,
            dark_name,
        } => {
            document["theme"]["name"] = value(name.clone());
            document["theme"]["auto_switch"] = value(*auto_switch);
            document["theme"]["light_name"] = value(light_name.clone());
            document["theme"]["dark_name"] = value(dark_name.clone());
        }
        HerdrUserConfigUpdate::CopyOnSelect(enabled) => {
            document["ui"]["copy_on_select"] = value(*enabled);
        }
        HerdrUserConfigUpdate::MouseCapture(enabled) => {
            document["ui"]["mouse_capture"] = value(*enabled);
        }
        HerdrUserConfigUpdate::MouseScrollLines(lines) => {
            if !(1..=100).contains(lines) {
                return Err(format!("mouse_scroll_lines out of range: {lines}"));
            }
            document["ui"]["mouse_scroll_lines"] = value(*lines);
        }
        HerdrUserConfigUpdate::SidebarStartCollapsed(enabled) => {
            document["ui"]["sidebar_start_collapsed"] = value(*enabled);
        }
        HerdrUserConfigUpdate::SidebarCollapsedMode(mode) => {
            if mode != "compact" && mode != "hidden" {
                return Err(format!("invalid sidebar_collapsed_mode: {mode}"));
            }
            document["ui"]["sidebar_collapsed_mode"] = value(mode.clone());
        }
        HerdrUserConfigUpdate::PaneBorders(enabled) => {
            document["ui"]["pane_borders"] = value(*enabled);
        }
        HerdrUserConfigUpdate::PaneOuterBorders(enabled) => {
            document["ui"]["pane_outer_borders"] = value(*enabled);
        }
        HerdrUserConfigUpdate::PaneScrollbars(enabled) => {
            document["ui"]["pane_scrollbars"] = value(*enabled);
        }
        HerdrUserConfigUpdate::PaneGaps(enabled) => {
            document["ui"]["pane_gaps"] = value(*enabled);
        }
        HerdrUserConfigUpdate::HideTabBarWhenSingleTab(enabled) => {
            document["ui"]["hide_tab_bar_when_single_tab"] = value(*enabled);
        }
        HerdrUserConfigUpdate::TabBarPosition(position) => {
            if position != "top" && position != "bottom" {
                return Err(format!("invalid tab_bar_position: {position}"));
            }
            document["ui"]["tab_bar_position"] = value(position.clone());
        }
        HerdrUserConfigUpdate::StatusIndicators(style) => {
            if style != "dots" && style != "symbols" {
                return Err(format!("invalid status_indicators: {style}"));
            }
            document["ui"]["status_indicators"] = value(style.clone());
        }
    }
    Ok(())
}

pub fn update_herdr_user_config(
    update: HerdrUserConfigUpdate,
) -> Result<HerdrUserConfigSnapshot, String> {
    let path = herdr_user_config_path();
    let mut document = load_system_config_document(&path)?;
    apply_herdr_user_config_update(&mut document, &update)?;
    let parent = path
        .parent()
        .ok_or_else(|| "Herdr config has no parent directory".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create {}: {error}", parent.display()))?;
    let temporary = path.with_extension(format!("tmp-shardlane-{}", std::process::id()));
    std::fs::write(&temporary, document.to_string())
        .map_err(|error| format!("write {}: {error}", temporary.display()))?;
    if let Ok(metadata) = std::fs::metadata(&path) {
        let _ = std::fs::set_permissions(&temporary, metadata.permissions());
    }
    validate_herdr_tui_config(&temporary)?;
    std::fs::rename(&temporary, &path)
        .map_err(|error| format!("install {}: {error}", path.display()))?;
    load_herdr_user_config()
}

fn validate_herdr_tui_config(path: &Path) -> Result<(), String> {
    let herdr_cli = crate::herdr::herdr_cli_path()
        .ok_or_else(|| "herdr CLI not found; cannot validate hosted TUI config".to_string())?;
    let output = Command::new(herdr_cli)
        .args(["config", "check"])
        .env("HERDR_CONFIG_PATH", path)
        .output()
        .map_err(|error| format!("run herdr config check: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(format!(
        "Herdr config rejected: {}{}{}",
        stdout.trim(),
        if stdout.trim().is_empty() || stderr.trim().is_empty() {
            ""
        } else {
            "; "
        },
        stderr.trim()
    ))
}

/// TUI mode navigation chain: drive the hosted TUI to follow Shardlane sidebar navigation.
///
/// Order (protocol 20 direct focus; the zoom-toggle workaround must not return —
/// 2026-08-26 audit TUI-05):
/// 1. `workspace.focus` — abort on failure (server unreachable makes entire chain pointless).
/// 2. `tab.focus` — abort on failure (tab closed etc).
/// 3. `pane.focus` — direct PaneTarget focus.
///
/// Returns first failure step description (caller logs only, does not mutate UI state).
pub fn execute_focus_plan(
    client: &dyn shardlane_host::mux::MultiplexerConnection,
    workspace_id: Option<&str>,
    tab_id: Option<&str>,
    pane_id: Option<&str>,
    agent_terminal_id: Option<&str>,
) -> Result<(), String> {
    if let Some(target) = agent_terminal_id {
        let runtime = match client.agent_runtime() {
            Some(runtime) => runtime,
            None => return Err("agents unsupported by this backend".to_string()),
        };
        return match runtime.agent_focus(target) {
            Ok(()) => Ok(()),
            Err(error) => {
                lag_log(format_args!(
                    "tui.agent_focus {target} failed, falling back to pane chain: {error}"
                ));
                execute_pane_chain(client, workspace_id, tab_id, pane_id)
            }
        };
    }
    execute_pane_chain(client, workspace_id, tab_id, pane_id)
}

fn execute_pane_chain(
    client: &dyn shardlane_host::mux::MultiplexerConnection,
    workspace_id: Option<&str>,
    tab_id: Option<&str>,
    pane_id: Option<&str>,
) -> Result<(), String> {
    let Some(workspace_id) = workspace_id else {
        return Ok(());
    };
    if let Err(error) = client.workspace_focus(workspace_id) {
        return Err(format!("workspace.focus {workspace_id}: {error}"));
    }
    let Some(tab_id) = tab_id else {
        return Ok(());
    };
    if let Err(error) = client.tab_focus(tab_id) {
        return Err(format!("tab.focus {tab_id}: {error}"));
    }
    let Some(pane_id) = pane_id else {
        return Ok(());
    };
    client
        .pane_focus(pane_id)
        .map_err(|error| format!("pane.focus {pane_id}: {error}"))
}

// ---------------------------------------------------------------------------
// Host state model for Settings UI display
// ---------------------------------------------------------------------------

/// Runtime state of the Herdr TUI host process.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HerdrTuiHostStatus {
    #[default]
    Stopped,
    Starting,
    Running,
    Failed,
}

impl HerdrTuiHostStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Starting => "Starting…",
            Self::Running => "Running",
            Self::Failed => "Failed",
        }
    }
}

/// Observable state for the TUI host (surfaced in Settings and the content-area
/// failure placeholder). Deliberately minimal: status + last error.
#[derive(Clone, Debug, Default)]
pub struct HerdrTuiHostState {
    pub status: HerdrTuiHostStatus,
    pub last_error: Option<String>,
}

/// Whether the current Herdr server supports protocol 20 (required for TUI focus wrappers).
/// Cached connection metadata only — never performs socket RPC (audit TUI-08:
/// compatibility checks must not run from render paths).
/// Protocol gate stays a Herdr-protocol-self concern: resolved through the
/// adapter escape hatch (docs/multiplexer-api.md §5 whitelist).
pub fn protocol_supported(client: &dyn shardlane_host::mux::MultiplexerConnection) -> bool {
    match client.as_herdr() {
        // Protocol 20 is required for the Herdr focus chain (TUI-only cutover).
        Some(herdr) => herdr.protocol().unwrap_or(0) >= 20,
        // The gate is a Herdr-protocol concern; other backends are not bound
        // by it (their focus chains degrade per capabilities).
        None => true,
    }
}

/// Reveal the generated TUI config file in Finder. No-op if path does not exist.
impl HerdrTuiHostState {
    /// Record a host failure with error message.
    pub fn set_failed(&mut self, error: String) {
        self.status = HerdrTuiHostStatus::Failed;
        self.last_error = Some(error);
    }

    /// Record successful host start.
    pub fn set_running(&mut self) {
        self.status = HerdrTuiHostStatus::Running;
        self.last_error = None;
    }

    /// Mark a restart in progress (status only; the actual relaunch is driven by
    /// the app's TUI surface lifecycle, not by this model).
    pub fn begin_restart(&mut self) {
        self.status = HerdrTuiHostStatus::Starting;
        self.last_error = None;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::herdr::HerdrClient;

    #[test]
    fn user_config_snapshot_accepts_empty_document_without_panicking() {
        let document = DocumentMut::new();
        let snapshot = herdr_user_config_snapshot_from_document(&document);
        assert_eq!(snapshot, HerdrUserConfigSnapshot::default());
    }

    #[test]
    fn user_config_snapshot_accepts_partial_sections_without_panicking() {
        let document = r#"
[theme]
name = "one-light"

[ui]
mouse_capture = false
"#
        .parse::<DocumentMut>()
        .unwrap();
        let snapshot = herdr_user_config_snapshot_from_document(&document);
        assert_eq!(snapshot.theme_name, "one-light");
        assert!(!snapshot.mouse_capture);
        assert!(!snapshot.theme_auto_switch);
        assert_eq!(snapshot.sidebar_collapsed_mode, "compact");
        assert!(snapshot.pane_borders);
    }

    #[test]
    fn spawn_env_strips_herdr_pane_identity_and_terminal_overrides() {
        let strip_keys = shardlane_host::herdr::HERDR_TUI_STRIP_ENV_KEYS;
        let mut source = std::collections::HashMap::new();
        for key in strip_keys {
            source.insert(key.to_string(), "leak".to_string());
        }
        source.insert("HOME".to_string(), "/Users/test".to_string());
        source.insert("HERDR_SOCKET_PATH".to_string(), "/tmp/x.sock".to_string());
        source.insert(
            "HERDR_CONFIG_PATH".to_string(),
            "/Users/test/.config/herdr/config.toml".to_string(),
        );
        let env = sanitized_spawn_env(&source);
        let keys: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        for stripped in strip_keys {
            assert!(!keys.contains(stripped), "{stripped} must be stripped");
        }
        assert!(keys.contains(&"HOME"));
        assert!(keys.contains(&"HERDR_SOCKET_PATH"));
        assert!(keys.contains(&"HERDR_CONFIG_PATH"));
    }

    #[test]
    fn visual_config_updates_preserve_unrelated_herdr_settings() {
        let mut document = r#"
onboarding = true

[theme]
name = "nord"
auto_switch = true

[ui]
sidebar_width = 26

[[keys]]
key = "prefix+a"
type = "plugin_action"
command = "example.action"
"#
        .parse::<DocumentMut>()
        .unwrap();
        apply_herdr_user_config_update(
            &mut document,
            &HerdrUserConfigUpdate::SidebarCollapsedMode("hidden".to_string()),
        )
        .unwrap();
        apply_herdr_user_config_update(&mut document, &HerdrUserConfigUpdate::MouseCapture(false))
            .unwrap();
        let rendered = document.to_string();

        assert_eq!(document["theme"]["name"].as_str(), Some("nord"));
        assert_eq!(document["theme"]["auto_switch"].as_bool(), Some(true));
        assert_eq!(document["ui"]["sidebar_width"].as_integer(), Some(26));
        assert_eq!(
            document["ui"]["sidebar_collapsed_mode"].as_str(),
            Some("hidden")
        );
        assert_eq!(document["ui"]["mouse_capture"].as_bool(), Some(false));
        assert!(rendered.contains("command = \"example.action\""));
        assert!(rendered.contains("key = \"prefix+a\""));
    }

    #[test]
    fn user_sidebar_preferences_are_real_herdr_config_keys() {
        let mut document = DocumentMut::new();
        apply_herdr_user_config_update(
            &mut document,
            &HerdrUserConfigUpdate::SidebarStartCollapsed(true),
        )
        .unwrap();
        apply_herdr_user_config_update(
            &mut document,
            &HerdrUserConfigUpdate::SidebarCollapsedMode("hidden".to_string()),
        )
        .unwrap();
        assert_eq!(
            document["ui"]["sidebar_start_collapsed"].as_bool(),
            Some(true)
        );
        assert_eq!(
            document["ui"]["sidebar_collapsed_mode"].as_str(),
            Some("hidden")
        );
    }

    #[test]
    fn manual_theme_update_writes_real_herdr_theme_and_disables_auto_switch() {
        let mut document = r#"
[theme]
name = "nord"
auto_switch = true
"#
        .parse::<DocumentMut>()
        .unwrap();
        apply_herdr_user_config_update(
            &mut document,
            &HerdrUserConfigUpdate::ThemeName("one-light".to_string()),
        )
        .unwrap();
        assert_eq!(document["theme"]["name"].as_str(), Some("one-light"));
        assert_eq!(document["theme"]["auto_switch"].as_bool(), Some(false));
        assert_eq!(document["theme"]["light_name"].as_str(), Some("one-light"));
    }

    #[test]
    fn theme_appearance_mode_follows_auto_switch_and_theme_category() {
        let config = HerdrUserConfigSnapshot {
            theme_name: "catppuccin-latte".to_string(),
            ..HerdrUserConfigSnapshot::default()
        };
        assert_eq!(
            theme_appearance_mode(&config, "dark"),
            ThemeAppearanceMode::Light
        );
        let config = HerdrUserConfigSnapshot {
            theme_name: "nord".to_string(),
            ..HerdrUserConfigSnapshot::default()
        };
        assert_eq!(
            theme_appearance_mode(&config, "light"),
            ThemeAppearanceMode::Dark
        );
        let config = HerdrUserConfigSnapshot {
            theme_auto_switch: true,
            ..HerdrUserConfigSnapshot::default()
        };
        assert_eq!(
            theme_appearance_mode(&config, "light"),
            ThemeAppearanceMode::Auto
        );
        // Custom (non-preset) themes fall back to Shardlane's synced appearance.
        let custom = HerdrUserConfigSnapshot {
            theme_name: "my-custom-theme".to_string(),
            ..HerdrUserConfigSnapshot::default()
        };
        assert_eq!(
            theme_appearance_mode(&custom, "light"),
            ThemeAppearanceMode::Light
        );
        assert_eq!(
            theme_appearance_mode(&custom, "dark"),
            ThemeAppearanceMode::Dark
        );
    }

    #[test]
    fn effective_theme_selections_fall_back_to_current_then_stock_builtins() {
        let latte = HerdrUserConfigSnapshot {
            theme_name: "catppuccin-latte".to_string(),
            ..HerdrUserConfigSnapshot::default()
        };
        // Empty light/dark names: the light manual theme seeds light; dark stays stock.
        assert_eq!(
            effective_theme_selections(&latte),
            ("catppuccin-latte".to_string(), "catppuccin".to_string())
        );
        let nord = HerdrUserConfigSnapshot {
            theme_name: "nord".to_string(),
            ..HerdrUserConfigSnapshot::default()
        };
        assert_eq!(
            effective_theme_selections(&nord),
            ("catppuccin-latte".to_string(), "nord".to_string())
        );
        // Explicit fields always win.
        let explicit = HerdrUserConfigSnapshot {
            theme_light_name: "one-light".to_string(),
            theme_dark_name: "dracula".to_string(),
            ..HerdrUserConfigSnapshot::default()
        };
        assert_eq!(
            effective_theme_selections(&explicit),
            ("one-light".to_string(), "dracula".to_string())
        );
    }

    #[test]
    fn theme_scheme_update_writes_mode_and_both_selections_atomically() {
        // Manual light: the light selection becomes the active theme.
        let update = theme_scheme_update(
            ThemeAppearanceMode::Light,
            "catppuccin-latte",
            "nord",
            "nord",
        );
        assert_eq!(
            update,
            HerdrUserConfigUpdate::ThemeScheme {
                auto_switch: false,
                name: "catppuccin-latte".to_string(),
                light_name: "catppuccin-latte".to_string(),
                dark_name: "nord".to_string(),
            }
        );
        assert!(update.is_theme_change());

        // Manual dark: the dark selection becomes the active theme.
        let update = theme_scheme_update(
            ThemeAppearanceMode::Dark,
            "catppuccin-latte",
            "dracula",
            "catppuccin-latte",
        );
        assert_eq!(
            update,
            HerdrUserConfigUpdate::ThemeScheme {
                auto_switch: false,
                name: "dracula".to_string(),
                light_name: "catppuccin-latte".to_string(),
                dark_name: "dracula".to_string(),
            }
        );

        // Auto: the current built-in stays as `name` fallback; both selections required.
        let update =
            theme_scheme_update(ThemeAppearanceMode::Auto, "one-light", "one-dark", "vesper");
        assert_eq!(
            update,
            HerdrUserConfigUpdate::ThemeScheme {
                auto_switch: true,
                name: "vesper".to_string(),
                light_name: "one-light".to_string(),
                dark_name: "one-dark".to_string(),
            }
        );

        // Auto with a custom current theme falls back to the light selection as `name`.
        let update = theme_scheme_update(
            ThemeAppearanceMode::Auto,
            "one-light",
            "one-dark",
            "my-custom-theme",
        );
        assert_eq!(
            update,
            HerdrUserConfigUpdate::ThemeScheme {
                auto_switch: true,
                name: "one-light".to_string(),
                light_name: "one-light".to_string(),
                dark_name: "one-dark".to_string(),
            }
        );
    }

    #[test]
    fn theme_scheme_update_writes_all_theme_keys() {
        let mut document = DocumentMut::new();
        apply_herdr_user_config_update(
            &mut document,
            &theme_scheme_update(ThemeAppearanceMode::Auto, "one-light", "one-dark", "vesper"),
        )
        .unwrap();
        assert_eq!(document["theme"]["name"].as_str(), Some("vesper"));
        assert_eq!(document["theme"]["auto_switch"].as_bool(), Some(true));
        assert_eq!(document["theme"]["light_name"].as_str(), Some("one-light"));
        assert_eq!(document["theme"]["dark_name"].as_str(), Some("one-dark"));

        let mut document = r#"
[theme]
name = "vesper"
auto_switch = true
light_name = "one-light"
dark_name = "one-dark"
"#
        .parse::<DocumentMut>()
        .unwrap();
        apply_herdr_user_config_update(
            &mut document,
            &theme_scheme_update(
                ThemeAppearanceMode::Light,
                "catppuccin-latte",
                "one-dark",
                "vesper",
            ),
        )
        .unwrap();
        assert_eq!(document["theme"]["name"].as_str(), Some("catppuccin-latte"));
        assert_eq!(document["theme"]["auto_switch"].as_bool(), Some(false));
        assert_eq!(
            document["theme"]["light_name"].as_str(),
            Some("catppuccin-latte")
        );
        assert_eq!(document["theme"]["dark_name"].as_str(), Some("one-dark"));
    }

    #[test]
    fn every_settings_theme_is_accepted_by_installed_herdr() {
        if crate::herdr::herdr_cli_path().is_none() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        for preset in crate::theme::THEME_PRESETS {
            let mut document = DocumentMut::new();
            apply_herdr_user_config_update(
                &mut document,
                &HerdrUserConfigUpdate::ThemeName(preset.herdr_theme.to_string()),
            )
            .unwrap();
            let path = temp.path().join(format!("{}.toml", preset.id));
            std::fs::write(&path, document.to_string()).unwrap();
            if let Err(error) = validate_herdr_tui_config(&path) {
                panic!(
                    "Settings theme {} / Herdr {} was rejected: {error}",
                    preset.id, preset.herdr_theme
                );
            }
        }
    }

    #[test]
    #[allow(clippy::expect_used)] // Test assertion: the layout constant is constructed within this test, always Some.
    fn chrome_projection_tracks_sidebar_and_tab_bar_insets() {
        let projection = TuiChromeProjection::from_layout(
            120,
            40,
            LayoutRect {
                x: 26,
                y: 1,
                width: 94,
                height: 38,
            },
        )
        .unwrap_or_default();
        assert_eq!(
            projection,
            TuiChromeProjection {
                left: 26,
                top: 1,
                right: 0,
                bottom: 1,
            }
        );
        assert_eq!(projection.outer_grid(94, 38), (120, 40));
        assert_eq!(projection.visible_to_raw_cell((0, 0)), (26, 1));
        assert_eq!(projection.raw_to_visible_cell((26, 1)), Some((0, 0)));
        assert_eq!(projection.raw_to_visible_cell((25, 1)), None);
    }

    #[test]
    fn chrome_projection_handles_herdr_0_9_relative_layout() {
        // Herdr 0.9 reports pane-local area (y: 0, height: rows - 1). The 1 missing row
        // represents the top Tab bar (row 0), never the bottom.
        let projection = TuiChromeProjection::from_layout(
            120,
            40,
            LayoutRect {
                x: 0,
                y: 0,
                width: 120,
                height: 39,
            },
        )
        .unwrap_or_default();
        assert_eq!(
            projection,
            TuiChromeProjection {
                left: 0,
                top: 1,
                right: 0,
                bottom: 0,
            }
        );
        assert_eq!(projection.outer_grid(120, 39), (120, 40));
        assert_eq!(projection.visible_to_raw_cell((0, 0)), (0, 1));
        assert_eq!(projection.raw_to_visible_cell((0, 1)), Some((0, 0)));
        assert_eq!(projection.raw_to_visible_cell((0, 0)), None);
    }

    #[test]
    fn chrome_projection_projects_frame_and_cursor_to_pane_origin() {
        let line = |cells: &[&str]| crate::ghostty::TerminalLine {
            cells: cells.iter().map(|cell| (*cell).to_string()).collect(),
            ..Default::default()
        };
        let frame = TerminalFrame {
            lines: vec![
                line(&["t", "a", "b", "s", "!"]),
                line(&["s", "A", "B", "C", "x"]),
                line(&["b", "o", "t", "t", "m"]),
            ],
            cursor: Some((2, 1)),
            ..Default::default()
        };
        let projection = TuiChromeProjection {
            left: 1,
            top: 1,
            right: 1,
            bottom: 1,
        };
        let projected = projection.project_frame(&frame);
        assert_eq!(projected.lines.len(), 1);
        assert_eq!(projected.lines[0].cells, vec!["A", "B", "C"]);
        assert_eq!(projected.cursor, Some((1, 0)));
    }

    #[test]
    fn host_state_lifecycle_transitions() {
        let mut state = HerdrTuiHostState::default();
        assert_eq!(state.status, HerdrTuiHostStatus::Stopped);

        state.begin_restart();
        assert_eq!(state.status, HerdrTuiHostStatus::Starting);
        assert!(state.last_error.is_none());

        state.set_running();
        assert_eq!(state.status, HerdrTuiHostStatus::Running);

        state.set_failed("connection lost".to_string());
        assert_eq!(state.status, HerdrTuiHostStatus::Failed);
        assert_eq!(state.last_error.as_deref(), Some("connection lost"));

        state.begin_restart();
        assert_eq!(state.status, HerdrTuiHostStatus::Starting);
        assert!(state.last_error.is_none());
    }

    #[test]
    fn host_status_labels_are_nonempty() {
        for status in [
            HerdrTuiHostStatus::Stopped,
            HerdrTuiHostStatus::Starting,
            HerdrTuiHostStatus::Running,
            HerdrTuiHostStatus::Failed,
        ] {
            assert!(!status.label().is_empty(), "{status:?} label is empty");
        }
    }

    #[test]
    fn focus_plan_executes_direct_pane_focus_chain() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        let temp = tempfile::tempdir().unwrap();
        let socket_path = temp.path().join("focus-chain.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let server = std::thread::spawn(move || {
            let mut lines = Vec::new();
            for _ in 0..7 {
                let (stream, _) = listener.accept().unwrap();
                let mut stream = stream;
                let mut line = String::new();
                BufReader::new(&mut stream).read_line(&mut line).unwrap();
                // pane.focus has no dedicated result type (same family as workspace.focus/tab.focus).
                let response = r#"{"result":{"type":"ok"},"error":null}"#;
                writeln!(stream, "{response}").unwrap();
                lines.push(line);
            }
            lines
        });

        let client = HerdrClient::for_test_socket(socket_path);
        execute_focus_plan(&client, Some("w1"), Some("w1:t1"), Some("w1:p1"), None).unwrap();
        execute_focus_plan(&client, Some("w1"), Some("w1:t1"), Some("w1:p2"), None).unwrap();
        // Agent direct path: a single agent.focus; on success the pane chain is skipped.
        execute_focus_plan(&client, None, None, None, Some("t-agent")).unwrap();

        let lines = server.join().unwrap();
        assert_eq!(lines.len(), 7, "unexpected request count: {}", lines.len());
        assert!(lines[0].contains("workspace.focus"), "{}", lines[0]);
        assert!(lines[1].contains("tab.focus"), "{}", lines[1]);
        assert!(lines[2].contains("pane.focus"), "{}", lines[2]);
        assert!(lines[2].contains("\"pane_id\":\"w1:p1\""), "{}", lines[2]);
        assert!(lines[3].contains("workspace.focus"), "{}", lines[3]);
        assert!(lines[4].contains("tab.focus"), "{}", lines[4]);
        assert!(lines[5].contains("pane.focus"), "{}", lines[5]);
        assert!(lines[5].contains("\"pane_id\":\"w1:p2\""), "{}", lines[5]);
        assert!(lines[6].contains("agent.focus"), "{}", lines[6]);
        assert!(lines[6].contains("\"target\":\"t-agent\""), "{}", lines[6]);
        let all = lines.join("");
        assert!(
            !all.contains("pane.zoom"),
            "zoom workaround must not appear: {all}"
        );
        assert!(
            !all.contains("pane.layout"),
            "layout probe must not appear: {all}"
        );
    }

    #[test]
    fn agent_focus_failure_falls_back_to_pane_chain() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        let temp = tempfile::tempdir().unwrap();
        let socket_path = temp.path().join("agent-fallback.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let server = std::thread::spawn(move || {
            let mut lines = Vec::new();
            for _ in 0..4 {
                let (stream, _) = listener.accept().unwrap();
                let mut stream = stream;
                let mut line = String::new();
                BufReader::new(&mut stream).read_line(&mut line).unwrap();
                let response = if line.contains("agent.focus") {
                    r#"{"result":null,"error":{"code":"not_found","message":"agent gone"}}"#
                } else {
                    r#"{"result":{"type":"ok"},"error":null}"#
                };
                writeln!(stream, "{response}").unwrap();
                lines.push(line);
            }
            lines
        });

        let client = HerdrClient::for_test_socket(socket_path);
        execute_focus_plan(
            &client,
            Some("w2"),
            Some("w2:t1"),
            Some("w2:p1"),
            Some("t-gone"),
        )
        .unwrap();

        let lines = server.join().unwrap();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("agent.focus"), "{}", lines[0]);
        assert!(lines[1].contains("workspace.focus"), "{}", lines[1]);
        assert!(lines[2].contains("tab.focus"), "{}", lines[2]);
        assert!(lines[3].contains("pane.focus"), "{}", lines[3]);
    }
}
