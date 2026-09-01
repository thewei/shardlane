//! [INPUT]: Depends on the `herdr` runtime, the `ghostty` terminal engine, the `gpui`/`gpui_component` component libraries, the `rust_i18n` i18n backend (crate-root `i18n!`), and the submodules
//! [OUTPUT]: Exposes the Shardlane macOS client entry point, the `ShardlaneApp` root view model, and action dispatch
//! [POS]: crates/herdr-gui's core assembly and application entry, coordinating global state, menus, and panel routing

// i18n backend embedding: this must stay at the crate root because
// `rust_i18n::t!` expands to `crate::_rust_i18n_t!`. Catalog and helpers live
// in `i18n.rs` / `crates/herdr-gui/locales/` (settings::Language drives it).
rust_i18n::i18n!("crates/herdr-gui/locales", fallback = "en");

mod agent_cli;
mod agent_switcher;
mod agent_ui;
mod assets;
mod browser_profile;
mod browser_profile_view;
mod chat;
mod composer_chip;
mod font_catalog;
mod ghostty;
mod git_status;
mod header_view;
mod help;
mod herdr;
mod herdr_tui;
mod history;
mod i18n;
mod input;
mod interaction;
mod macos_window;
mod mobile_view;
mod new_agent;
mod notifications;
mod pane_layout;
mod providers_view;
mod rename;
mod right_panel;
mod scripts;
mod search_model;
mod search_view;
mod settings;
mod settings_view;
mod shell_input;
mod shell_navigation;
mod shell_overlays;
mod shell_panes;
mod shell_projects;
mod shell_render;
mod shell_scroll_search;
mod shell_settings;
mod shell_tabs;
mod shell_terminal_stream;
mod shell_theme;
mod shell_tui;
mod shortcuts;
mod shortcuts_view;
mod sidebar;
mod ssh_bridge;
mod status;
mod status_bar;
mod steering;
mod terminal_interact;
mod terminal_stream;
mod terminal_trace;
mod terminal_view;
mod theme;
mod ui;
mod ui_metrics;
mod workspace_model;

// FocusIntent seam: the type belongs to the navigation domain, shell_navigation.rs,
// and is re-exported at the crate root so sibling modules (Sidebar/Search/Header) can reference it by root path name.
use shell_navigation::FocusIntent;

use crepuscularity_gpui as gpui;
use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{
    actions, bounds, canvas, div, gpui_window_options, point, px, rgb, size, AnyElement, AnyView,
    AnyWindowHandle, App, Application, Bounds, Context, Entity, FocusHandle, Focusable,
    InputHandler, IntoElement, KeyBinding, Keystroke, Menu, MenuItem, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PathPromptOptions, Pixels, Render, ScrollHandle,
    ScrollWheelEvent, SharedString, StyleRefinement, Subscription, SystemMenuType,
    Task as BackgroundJob, TouchPhase, UTF16Selection, WeakEntity, Window, WindowAppearance,
    WindowBackgroundAppearance, WindowBounds, WindowId,
};
use futures::future::{select, Either};
use ghostty::{
    GhosttyKeyEncoderState, TerminalFrame, TerminalFramePlan, TerminalModifiers,
    TerminalMouseAction, TerminalMouseButton, TerminalMouseGeometry,
};
use gpui_component::{
    button::{Button, ButtonVariants as _, DropdownButton},
    h_flex,
    input::{Input, InputEvent, InputState},
    label::Label,
    list::{List, ListDelegate, ListItem, ListState},
    menu::{ContextMenuExt, PopupMenuItem},
    scroll::ScrollableElement as _,
    slider::{Slider, SliderEvent, SliderState, SliderValue},
    spinner::Spinner,
    v_flex, ActiveTheme as _, Icon, IconName as ComponentIconName, IndexPath, Root,
    Selectable as _, Sizable as _, TitleBar, WindowExt as _,
};
use herdr::{
    herdr_cli_path, installed_cli_version, Agent, AgentStatusPatch, DeviceEndpoint, HerdrClient,
    HerdrEvent, HerdrState, LayoutRect, NavigationState, Pane, PaneLayout, PaneLayoutActionResult,
    PaneMoveResult, PaneProcessInfo, Tab, TabCreatedResult, TabSurfaceState, Workspace,
};
use input::{ghostty_terminal_key, key_name};
use interaction::InteractiveSurfaceExt as _;
use pane_layout::neighbor_pane_in_direction;
use scripts::{ScriptRegistry, ScriptStatus};
pub(crate) use search_model::{
    ClientPickerOverlay, ClientSearchDelegate, ClientSearchItem, ClientSearchTarget,
};
use search_view::{client_search_result_item, render_client_picker_overlay};
use shardlane_history::{
    AgentId, ConversationMeta, HistoryAdapterRoster, HistoryCatalog, SearchHit,
};
use shardlane_host::diagnostics::lag_log;
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use terminal_interact::{PendingTerminalCopy, SelectionGeometry};
use terminal_stream::{
    ManagedTerminal, TerminalControlInput, TerminalWakeReceiver, TerminalWakeSender,
};
use terminal_view::{
    cached_terminal, TerminalGeometry, TerminalPane, TerminalSelection, TERMINAL_SELECTION_BG,
};
use theme::UiTheme;
use ui_metrics::{INTERACTIVE_FOCUS_OPACITY, SPACE_ICON};
use workspace_model::{build_project_index, VisibleSidebarProject};

actions!(
    shardlane,
    [
        ToggleHelp,
        ToggleSettings,
        OpenHistory,
        OpenSearch,
        FindInConversation,
        OpenNewAgent,
        OpenAbout,
        Refresh,
        PickerSelectUp,
        PickerSelectDown,
        PickerCancel,
        PickerAcceptCompletion,
        Paste,
        Copy,
        SelectAll,
        IncreaseTerminalFontSize,
        DecreaseTerminalFontSize,
        ResetTerminalFontSize,
        SplitRight,
        SplitDown,
        TogglePaneZoom,
        FocusLeft,
        FocusRight,
        FocusUp,
        FocusDown,
        ResizeLeft,
        ResizeRight,
        ResizeUp,
        ResizeDown,
        ClosePane,
        PreviousTab,
        NextTab,
        NewTab,
        RenameTab,
        CloseTab,
        NewProject,
        NewScript,
        RenameProject,
        CloseProject,
        PreviousProject,
        NextProject,
        NewWindow,
        MergeAllWindows,
        ToggleSidebar,
        ToggleAgents,
        ToggleServices,
        ThemeCatppuccin,
        ThemeCatppuccinLatte,
        ThemeTokyoNight,
        ThemeTokyoNightDay,
        ThemeDracula,
        ThemeNord,
        ThemeGruvbox,
        ThemeGruvboxLight,
        ThemeOneDark,
        ThemeOneLight,
        ThemeSolarized,
        ThemeSolarizedLight,
        ThemeKanagawa,
        ThemeKanagawaLotus,
        ThemeRosePine,
        ThemeRosePineDawn,
        ThemeVesper,
        ReloadHerdrConfig,
        ToggleAlwaysOnTop,
        ToggleRightPanel,
        OpenLazygit,
        NavigateBack,
        NavigateForward,
        SwitchAgentNext,
        SwitchAgentPrev,
        ConfirmAgentSwitch,
        CancelAgentSwitch,
        Quit
    ]
);

macro_rules! set_theme {
    ($name:ident, $action:ty, $theme:literal) => {
        pub(super) fn $name(&mut self, _: &$action, window: &mut Window, cx: &mut Context<Self>) {
            self.set_theme($theme.to_string(), window, cx);
        }
    };
}
pub(crate) use set_theme;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum TerminalSelectionMode {
    #[default]
    Cell,
    Word,
    Line,
}

/// The single authoritative type for connection state: the string sentinel ("connected") has been
/// replaced by the type system, so no mis-typed copy anywhere can silently masquerade as offline/online.
#[derive(Clone, Debug, Eq, PartialEq)]
enum ConnectionStatus {
    Connected,
    Offline(String),
}

impl ConnectionStatus {
    fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }

    /// For pure-copy display surfaces like empty_state; never used in state decisions.
    fn detail(&self) -> &str {
        match self {
            Self::Connected => "connected",
            Self::Offline(reason) => reason,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum SettingsSection {
    #[default]
    Appearance,
    Terminal,
    Lazygit,
    Shortcuts,
    Behavior,
    Window,
    Machines,
    Providers,
    Mobile,
    Browser,
}

impl SettingsSection {
    // Audit E24: Providers sits before Mobile/Browser — it decides whether New Task/History
    // can work at all, so it outranks the secondary mobile/browser surfaces in the sidebar.
    const ALL: [Self; 10] = [
        Self::Appearance,
        Self::Terminal,
        Self::Lazygit,
        Self::Shortcuts,
        Self::Behavior,
        Self::Window,
        Self::Machines,
        Self::Providers,
        Self::Mobile,
        Self::Browser,
    ];

    fn label(self) -> SharedString {
        match self {
            Self::Appearance => i18n::t("settings.section.appearance"),
            Self::Terminal => i18n::t("settings.section.terminal"),
            Self::Lazygit => i18n::t("settings.section.lazygit"),
            Self::Shortcuts => i18n::t("settings.section.shortcuts"),
            Self::Behavior => i18n::t("settings.section.behavior"),
            Self::Window => i18n::t("settings.section.window"),
            Self::Machines => i18n::t("settings.section.machines"),
            Self::Providers => i18n::t("settings.section.providers"),
            Self::Mobile => i18n::t("settings.section.mobile"),
            Self::Browser => i18n::t("settings.section.browser"),
        }
    }

    /// Audit A28: returns a ready-to-render Icon so Mobile can use the local smartphone glyph
    /// (gpui-component 0.5.1's IconName set has no device-class icon and Mobile/Browser would
    /// otherwise both show Globe).
    fn icon(self) -> gpui_component::Icon {
        match self {
            Self::Appearance => gpui_component::Icon::new(ComponentIconName::Palette),
            Self::Terminal => gpui_component::Icon::new(ComponentIconName::SquareTerminal),
            Self::Lazygit => gpui_component::Icon::new(ComponentIconName::GitHub),
            Self::Shortcuts => gpui_component::Icon::new(ComponentIconName::Settings),
            Self::Behavior => gpui_component::Icon::new(ComponentIconName::Settings2),
            Self::Window => gpui_component::Icon::new(ComponentIconName::Frame),
            Self::Machines => gpui_component::Icon::empty().path("icons/layers.svg"),
            Self::Providers => gpui_component::Icon::new(ComponentIconName::Bot),
            Self::Mobile => gpui_component::Icon::empty().path("icons/smartphone.svg"),
            Self::Browser => gpui_component::Icon::new(ComponentIconName::Globe),
        }
    }
}

/// R6: Host name for hello/bootstrap display (the machine name; cached once per process).
fn remote_display_host_name() -> String {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        if let Some(name) = std::env::var("SHARDLANE_HOST_NAME")
            .ok()
            .filter(|name| !name.trim().is_empty())
        {
            return name;
        }
        std::process::Command::new("scutil")
            .args(["--get", "ComputerName"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Shardlane Host".to_string())
    })
    .clone()
}

#[derive(Clone, Copy, Default)]
struct ContentSurfaceTheme {
    background: gpui::Hsla,
    foreground: gpui::Hsla,
    muted: gpui::Hsla,
    border: gpui::Hsla,
    hover: gpui::Hsla,
    active: gpui::Hsla,
    primary: gpui::Hsla,
    danger: gpui::Hsla,
    success: gpui::Hsla,
    is_dark: bool,
}

impl ContentSurfaceTheme {
    fn button_variant(&self, cx: &App) -> gpui_component::button::ButtonCustomVariant {
        gpui_component::button::ButtonCustomVariant::new(cx)
            .foreground(self.foreground)
            .hover(self.hover)
            .active(self.active)
    }
}

enum TerminalInputCommand {
    Text {
        pane_id: String,
        target: String,
        text: String,
    },
    Paste {
        pane_id: String,
        target: String,
        text: String,
    },
    Focus {
        pane_id: String,
        target: String,
        terminal: Arc<Mutex<ManagedTerminal>>,
        focused: bool,
    },
    RawBytes {
        pane_id: String,
        target: String,
        bytes: Vec<u8>,
    },
    Keys {
        pane_id: String,
        keys: Vec<String>,
    },
}

/// Terminal input failure classification: only socket/RPC-layer failures may poison the connection
/// state. API-level key rejections (invalid_key), local encoding failures, and dead controller
/// sessions concern only that single input; they are logged and the queue keeps draining, preventing
/// one keystroke from flipping the whole App to Offline.
enum TerminalInputFailure {
    Connection(String),
    Input(String),
}

impl TerminalInputFailure {
    fn input(reason: impl std::fmt::Display) -> Self {
        Self::Input(reason.to_string())
    }
}

impl From<String> for TerminalInputFailure {
    fn from(reason: String) -> Self {
        Self::Input(reason)
    }
}

/// Herdr RPC error → input failure classification: API rejections are Input; socket/IO/serialization
/// failures are Connection (only evidence of genuinely degrading connectivity enters the connection state).
fn classify_input_failure(err: herdr::HerdrError) -> TerminalInputFailure {
    match err {
        herdr::HerdrError::Api(reason) => TerminalInputFailure::Input(reason),
        other => TerminalInputFailure::Connection(other.to_string()),
    }
}

/// Pure decision for application-level terminal focus reporting (consumed by `sync_terminal_application_focus`):
/// inject Focus In/Out only on window-active flips; `reported_live` is the reported state after the
/// controller-liveness self-check. With no pane to report, stay unreported (the F18 regression pin).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalFocusReportAction {
    None,
    FocusIn,
    FocusOut,
}

fn terminal_focus_report_action(
    surface_active: bool,
    reported_live: Option<&(String, String)>,
) -> TerminalFocusReportAction {
    if surface_active == reported_live.is_some() {
        TerminalFocusReportAction::None
    } else if surface_active {
        TerminalFocusReportAction::FocusIn
    } else {
        TerminalFocusReportAction::FocusOut
    }
}

fn coalesce_terminal_input(
    queue: &mut VecDeque<TerminalInputCommand>,
    command: TerminalInputCommand,
) {
    match (queue.back_mut(), command) {
        (
            Some(TerminalInputCommand::Text {
                pane_id: queued_pane,
                target: queued_target,
                text: queued_text,
            }),
            TerminalInputCommand::Text {
                pane_id,
                target,
                text,
            },
        ) if queued_pane == &pane_id && queued_target == &target => queued_text.push_str(&text),
        (
            Some(TerminalInputCommand::Keys {
                pane_id: queued_pane,
                keys: queued_keys,
            }),
            TerminalInputCommand::Keys { pane_id, keys },
        ) if queued_pane == &pane_id => queued_keys.extend(keys),
        (_, command) => queue.push_back(command),
    }
}

#[derive(Clone)]
struct TerminalInputHandler {
    herdr: Entity<ShardlaneApp>,
    pane_id: String,
    target: String,
    bounds: Bounds<Pixels>,
    cell_width: f32,
    cell_height: f32,
}

impl TerminalInputHandler {
    fn clear_marked_text(&self, cx: &mut App) {
        let trace_started = terminal_trace::enabled().then(Instant::now);
        self.herdr.update(cx, |view, cx| {
            let changed = view.ime_target.is_some()
                || !view.ime_marked_text.is_empty()
                || view.ime_selected_range.is_some();
            view.ime_target = None;
            view.ime_marked_text.clear();
            view.ime_selected_range = None;
            if changed {
                cx.notify();
            }
            if let Some(started) = trace_started {
                terminal_trace::event(format_args!(
                    "stage=ui.ime_clear changed={changed} root_notify={changed} elapsed_us={}",
                    terminal_trace::elapsed_us(started),
                ));
            }
        });
    }
}

impl InputHandler for TerminalInputHandler {
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<UTF16Selection> {
        let view = self.herdr.read(cx);
        let range = if view.ime_target.as_deref() == Some(self.target.as_str()) {
            view.ime_selected_range.clone().unwrap_or_else(|| {
                let len = view.ime_marked_text.encode_utf16().count();
                len..len
            })
        } else {
            0..0
        };
        Some(UTF16Selection {
            range,
            reversed: false,
        })
    }

    fn marked_text_range(&mut self, _window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        let view = self.herdr.read(cx);
        if view.ime_target.as_deref() != Some(self.target.as_str())
            || view.ime_marked_text.is_empty()
        {
            return None;
        }
        Some(0..view.ime_marked_text.encode_utf16().count())
    }

    fn text_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<String> {
        let view = self.herdr.read(cx);
        if view.ime_target.as_deref() != Some(self.target.as_str()) {
            return None;
        }
        let len = view.ime_marked_text.encode_utf16().count();
        *adjusted_range = Some(0..len);
        Some(view.ime_marked_text.clone())
    }

    fn replace_text_in_range(
        &mut self,
        _replacement_range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut App,
    ) {
        // GPUI reports key_char "\n"/"\t" for Enter/Tab, and AppKit may still route those keys
        // through insertText on top of the Ghostty encoder that already queued the bytes
        // (see `tui_key_encodes_to_pty`). Control-only payloads are therefore dropped; IME
        // commits are natural language and never control-only.
        if crate::shell_tui::text_is_terminal_control_payload(text) {
            lag_log(format_args!(
                "tui.text dropped control payload len={}",
                text.len()
            ));
            // A duplicate AppKit control event must not leave an IME preedit
            // overlay alive after Enter/Tab has already been encoded by the
            // Ghostty key path.  Clear the composition even though no PTY
            // bytes are queued for the control payload.
            self.clear_marked_text(cx);
            return;
        }
        let utf8_bytes = text.len();
        let utf16_units = text.encode_utf16().count();
        let text = text.to_string();
        let pane_id = self.pane_id.clone();
        let target = self.target.clone();
        let trace_started = terminal_trace::enabled().then(Instant::now);
        self.herdr.update(cx, |view, cx| {
            let had_preedit = view.ime_target.as_deref() == Some(target.as_str())
                || !view.ime_marked_text.is_empty()
                || view.ime_selected_range.is_some();
            view.ime_target = None;
            view.ime_marked_text.clear();
            view.ime_selected_range = None;
            // Text insertion is authoritative here for both direct printable input and IME
            // commits. Hosted TUI no longer pre-encodes printable keys in the global observer,
            // so there is no duplicate-ASCII path to suppress. Direct text does not dirty the
            // root shell: the only visible result is authoritative PTY echo → TerminalPane.
            // IME commit does notify once to remove the preedit overlay.
            view.prepare_terminal_for_input(&target, cx);
            let trace_kind = if had_preedit { "ime_commit" } else { "text" };
            view.queue_terminal_text_traced(pane_id, target, text, trace_kind, cx);
            if had_preedit {
                cx.notify();
            }
            if let Some(started) = trace_started {
                terminal_trace::event(format_args!(
                    "stage=ui.text_submit kind={trace_kind} utf8_bytes={utf8_bytes} utf16_units={utf16_units} root_notify={had_preedit} elapsed_us={}",
                    terminal_trace::elapsed_us(started),
                ));
            }
        });
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut App,
    ) {
        let utf8_bytes = new_text.len();
        let utf16_units = new_text.encode_utf16().count();
        let target = self.target.clone();
        let new_text = new_text.to_string();
        let trace_started = terminal_trace::enabled().then(Instant::now);
        self.herdr.update(cx, |view, cx| {
            view.ime_target = Some(target);
            view.ime_marked_text = new_text;
            view.ime_selected_range = new_selected_range;
            cx.notify();
            if let Some(started) = trace_started {
                terminal_trace::event(format_args!(
                    "stage=ui.ime_mark utf8_bytes={utf8_bytes} utf16_units={utf16_units} root_notify=true elapsed_us={}",
                    terminal_trace::elapsed_us(started),
                ));
            }
        });
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut App) {
        self.clear_marked_text(cx);
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let trace_started = terminal_trace::enabled().then(Instant::now);
        let cursor = self
            .herdr
            .read(cx)
            .terminal_frame_for_target()
            .cursor
            .unwrap_or((0, 0));
        let origin = point(
            self.bounds.origin.x + px(cursor.0 as f32 * self.cell_width),
            self.bounds.origin.y + px(cursor.1 as f32 * self.cell_height),
        );
        let bounds = Bounds::new(
            origin,
            size(px(self.cell_width.max(1.0)), px(self.cell_height.max(1.0))),
        );
        if let Some(started) = trace_started {
            terminal_trace::event(format_args!(
                "stage=ui.ime_bounds elapsed_us={}",
                terminal_trace::elapsed_us(started),
            ));
        }
        Some(bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<usize> {
        Some(0)
    }

    fn apple_press_and_hold_enabled(&mut self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ThemeMode {
    System,
    Dark,
    Light,
}

/// The single clamp boundary for terminal font size (shared by the Settings slider and ⌘+/⌘-/⌘0).
const TERMINAL_FONT_SIZE_RANGE: (f32, f32) = (10.0, 24.0);

fn clamped_terminal_font_size(next: f32) -> f32 {
    next.clamp(TERMINAL_FONT_SIZE_RANGE.0, TERMINAL_FONT_SIZE_RANGE.1)
}

fn theme_mode_from_config(value: &str) -> ThemeMode {
    match value {
        "dark" => ThemeMode::Dark,
        "light" => ThemeMode::Light,
        _ => ThemeMode::System,
    }
}

/// Audit A18: the single Herdr-user-config → Shardlane chrome preset resolution. Startup
/// bootstrap, `sync_app_theme_from_herdr`, and `theme()` each used to keep their own partially
/// drifted variant of this rule (the dark-name + auto_switch combination was honored in only
/// one of them); they now all call this.
///
/// Rule (the most complete, auto-switch-aware variant): in manual mode `theme_name` is
/// authoritative; with `theme_auto_switch` the per-appearance slot (`theme_dark_name` /
/// `theme_light_name`, selected by `dark`) is preferred, and an empty or unknown slot falls
/// back to the legacy `theme_name`. `dark` only matters for the live paint path — the config
/// paths pass `true` (the dark slot is the scheme anchor, matching sync_app_theme_from_herdr's
/// original rule).
fn resolved_theme_preset(
    herdr: &herdr_tui::HerdrUserConfigSnapshot,
    dark: bool,
) -> Option<theme::ThemePreset> {
    let configured = if herdr.theme_auto_switch {
        let slot = if dark {
            &herdr.theme_dark_name
        } else {
            &herdr.theme_light_name
        };
        if slot.is_empty() {
            &herdr.theme_name
        } else {
            slot
        }
    } else {
        &herdr.theme_name
    };
    theme::preset_for_herdr_theme(configured)
        .or_else(|| theme::preset_for_herdr_theme(&herdr.theme_name))
}

/// Audit A19: the single navigation projection type — the former TabNavigationProjection
/// (Surface|Full) was a strict subset and was collapsed into this enum.
enum FocusNavigationProjection {
    Surface(TabSurfaceState),
    Navigation {
        navigation: NavigationState,
        surface: Option<TabSurfaceState>,
    },
    Full(HerdrState),
}

fn resolve_navigation_selection(
    current_workspace_id: Option<&str>,
    current_tab_id: Option<&str>,
    navigation: &NavigationState,
) -> (Option<String>, Option<String>) {
    let selected_workspace_id = current_workspace_id
        .filter(|workspace_id| {
            navigation
                .workspaces
                .iter()
                .any(|workspace| workspace.workspace_id == *workspace_id)
        })
        .map(str::to_string)
        .or_else(|| navigation.focused_workspace_id.clone());

    let selected_tab_id = current_tab_id
        .filter(|tab_id| {
            navigation.tabs.iter().any(|tab| {
                tab.tab_id == *tab_id
                    && selected_workspace_id.as_deref().is_none_or(|workspace_id| {
                        tab.workspace_id.as_deref() == Some(workspace_id)
                    })
            })
        })
        .map(str::to_string)
        .or_else(|| {
            selected_workspace_id.as_deref().and_then(|workspace_id| {
                navigation
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == workspace_id)
                    .and_then(|workspace| workspace.active_tab_id.clone())
            })
        })
        .or_else(|| navigation.focused_tab_id.clone())
        .or_else(|| {
            selected_workspace_id.as_deref().and_then(|workspace_id| {
                navigation
                    .tabs
                    .iter()
                    .find(|tab| tab.workspace_id.as_deref() == Some(workspace_id))
                    .map(|tab| tab.tab_id.clone())
            })
        });

    (selected_workspace_id, selected_tab_id)
}

/// F34's single derivation point for selection flags: the three focused ids are the SSOT, and entity
/// booleans derive only from them. Call when a projection enters state (before compare/store) and after
/// any navigation change; never write `.focused` directly elsewhere (server reply fields stored with
/// their payload are exempt).
fn derive_selection_flags(state: &mut HerdrState) {
    let workspace_id = state.focused_workspace_id.as_deref();
    let tab_id = state.focused_tab_id.as_deref();
    let pane_id = state.focused_pane_id.as_deref();
    derive_workspace_tab_flags(&mut state.workspaces, &mut state.tabs, workspace_id, tab_id);
    for pane in &mut state.panes {
        pane.focused = Some(pane.pane_id.as_str()) == pane_id;
    }
    for layout in &mut state.layouts {
        // layout.focused_pane_id is Herdr's zoom runtime fact (rendering depends on it) and is not rewritten;
        // only the leaf pane flags sync, so stale booleans aren't misread as selection.
        let focused = layout.focused_pane_id.as_deref();
        for pane in &mut layout.panes {
            pane.focused = Some(pane.pane_id.as_str()) == pane_id.or(focused);
        }
    }
}

/// The navigation-domain subset of `derive_selection_flags` (NavigationState has no panes/layouts).
fn derive_navigation_selection_flags(navigation: &mut NavigationState) {
    let workspace_id = navigation.focused_workspace_id.as_deref();
    let tab_id = navigation.focused_tab_id.as_deref();
    derive_workspace_tab_flags(
        &mut navigation.workspaces,
        &mut navigation.tabs,
        workspace_id,
        tab_id,
    );
}

fn derive_workspace_tab_flags(
    workspaces: &mut [Workspace],
    tabs: &mut [Tab],
    workspace_id: Option<&str>,
    tab_id: Option<&str>,
) {
    for workspace in workspaces {
        workspace.focused = Some(workspace.workspace_id.as_str()) == workspace_id;
    }
    for tab in tabs {
        tab.focused = Some(tab.tab_id.as_str()) == tab_id;
    }
}

/// F20: local Tab resolution priority when switching Projects — client memory → Herdr active_tab_id
/// → the Project's first local Tab. Memory must pass the ownership check (the tab still belongs to the workspace).
/// Pure function, consumed by `focus_workspace_id_inner`.
fn resolve_workspace_tab_locally(
    workspaces: &[Workspace],
    tabs: &[Tab],
    workspace_id: &str,
    memory: Option<&str>,
) -> Option<String> {
    let tab_belongs = |tab_id: &str| {
        tabs.iter()
            .any(|tab| tab.tab_id == tab_id && tab.workspace_id.as_deref() == Some(workspace_id))
    };
    memory
        .filter(|tab_id| tab_belongs(tab_id))
        .map(str::to_string)
        .or_else(|| {
            workspaces
                .iter()
                .find(|workspace| workspace.workspace_id == workspace_id)
                .and_then(|workspace| workspace.active_tab_id.clone())
                .filter(|tab_id| tab_belongs(tab_id))
        })
        .or_else(|| {
            tabs.iter()
                .find(|tab| tab.workspace_id.as_deref() == Some(workspace_id))
                .map(|tab| tab.tab_id.clone())
        })
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct OperationalSummary {
    blocked_agents: usize,
    working_agents: usize,
    failed_scripts: usize,
    active_scripts: usize,
}

fn operational_summary(agents: &[Agent], scripts: &ScriptRegistry) -> OperationalSummary {
    // M8: single aggregation semantics — raw string interpretation happens only at the runtime→product
    // mapping boundary (attention_for_raw_status); badge counts no longer maintain a second string-based decision.
    let mut summary = OperationalSummary::default();
    for agent in agents {
        let attention = crate::status::agent_effective_status(agent)
            .map(crate::status::attention_for_raw_status)
            .unwrap_or(crate::status::AttentionLevel::Idle);
        match attention {
            crate::status::AttentionLevel::NeedsAttention => {
                summary.blocked_agents += 1;
            }
            crate::status::AttentionLevel::Working => {
                summary.working_agents += 1;
            }
            _ => {}
        }
    }
    OperationalSummary {
        blocked_agents: summary.blocked_agents,
        working_agents: summary.working_agents,
        failed_scripts: scripts
            .scripts
            .iter()
            .filter(|script| script.runtime.status == ScriptStatus::Failed)
            .count(),
        active_scripts: scripts
            .scripts
            .iter()
            .filter(|script| {
                matches!(
                    script.runtime.status,
                    ScriptStatus::Starting | ScriptStatus::Running
                )
            })
            .count(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentNotificationKind {
    Finished,
    NeedsAttention,
    Ready,
}

fn agent_notification_kind(
    previous: Option<&str>,
    next: Option<&str>,
) -> Option<AgentNotificationKind> {
    if previous.is_none() || previous == next {
        return None;
    }
    match (previous, next) {
        (Some("working"), Some("done")) | (Some("blocked"), Some("done")) => {
            Some(AgentNotificationKind::Finished)
        }
        (Some("working"), Some("blocked")) => Some(AgentNotificationKind::NeedsAttention),
        (Some("working"), Some("idle")) => Some(AgentNotificationKind::Ready),
        _ => None,
    }
}

fn agent_notification_copy(agent: &Agent, kind: AgentNotificationKind) -> (String, String) {
    // Audit A17: same identity fallback chain as the sidebar/status bar — one chain, not two.
    let identity = sidebar::agent_identity(agent).unwrap_or("Agent");
    let title = match kind {
        AgentNotificationKind::Finished => format!("{identity} finished"),
        AgentNotificationKind::NeedsAttention => format!("{identity} needs your attention"),
        AgentNotificationKind::Ready => format!("{identity} is ready"),
    };
    let body = agent
        .title
        .as_deref()
        .or(agent.custom_status.as_deref())
        .unwrap_or("Shardlane Agent status changed")
        .to_string();
    (title, body)
}

fn apply_agent_projection_patch(agent: &mut Agent, patch: &AgentStatusPatch) -> bool {
    let mut changed = false;
    if let Some(value) = &patch.agent_status {
        if &agent.agent_status != value {
            agent.agent_status = value.clone();
            changed = true;
        }
    }
    if let Some(value) = &patch.agent_session {
        if &agent.agent_session != value {
            agent.agent_session = value.clone();
            changed = true;
        }
    }
    if let Some(value) = &patch.name {
        if &agent.name != value {
            agent.name = value.clone();
            changed = true;
        }
    }
    if let Some(value) = &patch.agent {
        if &agent.agent != value {
            agent.agent = value.clone();
            changed = true;
        }
    }
    if let Some(value) = &patch.display_agent {
        if &agent.display_agent != value {
            agent.display_agent = value.clone();
            changed = true;
        }
    }
    if let Some(value) = &patch.title {
        if &agent.title != value {
            agent.title = value.clone();
            changed = true;
        }
    }
    if let Some(value) = &patch.custom_status {
        if &agent.custom_status != value {
            agent.custom_status = value.clone();
            changed = true;
        }
    }
    if let Some(value) = &patch.tab_id {
        if &agent.tab_id != value {
            agent.tab_id = value.clone();
            changed = true;
        }
    }
    if let Some(value) = &patch.cwd {
        if &agent.cwd != value {
            agent.cwd = value.clone();
            changed = true;
        }
    }
    if let Some(value) = &patch.foreground_cwd {
        if &agent.foreground_cwd != value {
            agent.foreground_cwd = value.clone();
            changed = true;
        }
    }
    // patch.focused is a Herdr runtime focus fact and is not written into the client selection projection:
    // navigation selection is owned locally by the client; events must not flip it back.
    let workspace_id = Some(patch.workspace_id.clone());
    if agent.workspace_id != workspace_id {
        agent.workspace_id = workspace_id;
        changed = true;
    }
    changed
}

fn apply_pane_agent_projection_patch(pane: &mut Pane, patch: &AgentStatusPatch) -> bool {
    let mut changed = false;
    if let Some(value) = &patch.agent_status {
        if &pane.agent_status != value {
            pane.agent_status = value.clone();
            changed = true;
        }
    }
    if let Some(value) = &patch.agent {
        if &pane.agent != value {
            pane.agent = value.clone();
            changed = true;
        }
    }
    if let Some(value) = &patch.title {
        if &pane.title != value {
            pane.title = value.clone();
            changed = true;
        }
    }
    if let Some(value) = &patch.tab_id {
        if &pane.tab_id != value {
            pane.tab_id = value.clone();
            changed = true;
        }
    }
    if let Some(value) = &patch.cwd {
        if &pane.cwd != value {
            pane.cwd = value.clone();
            changed = true;
        }
    }
    // Same as above: patch.focused doesn't drive client selection; Pane highlight follows only the local focused_pane_id.
    let workspace_id = Some(patch.workspace_id.clone());
    if pane.workspace_id != workspace_id {
        pane.workspace_id = workspace_id;
        changed = true;
    }
    changed
}

/// Sidebar drag session: Shell edge resize or right panel resize (consumed by root-level move/up listeners).
#[derive(Clone, Copy)]
enum SidebarDrag {
    Shell(f64, f64),
    /// Right panel resize (press x, starting width); dragging left widens it, same semantics as a RightPanel.
    RightPanel(f64, f64),
}

/// PANEL_SLIDE: sidebar slide in/out duration.
const PANEL_SLIDE: Duration = Duration::from_millis(200);

/// Minimal port of motion::WidthTween: a one-shot width slide starting from the current rendered width
/// (switching mid-slide reverses from the edge's actual position instead of jumping back to the far
/// end); render advances it every frame, and after the timeout width_toward returns None so the caller
/// settles on the target — which is exactly when a closing panel leaves the element tree.
struct WidthTween {
    from: f64,
    started: Instant,
}

impl WidthTween {
    fn new(from: f64) -> Self {
        Self {
            from,
            started: Instant::now(),
        }
    }

    fn width_toward(&self, target: f64) -> Option<f64> {
        let progress = self.started.elapsed().as_secs_f32() / PANEL_SLIDE.as_secs_f32();
        (progress < 1.0)
            .then(|| self.from + (target - self.from) * ease_out_quint(progress.max(0.0)) as f64)
    }
}

fn ease_out_quint(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(5)
}

/// Advance one slide step: returns this frame's rendered width; when finished, retires the tween and settles on the target.
fn slide_width(slide: &mut Option<WidthTween>, target: f64) -> f64 {
    match slide.as_ref().and_then(|s| s.width_toward(target)) {
        Some(width) => width,
        None => {
            *slide = None;
            target
        }
    }
}

/// This window's bound Project (= one Herdr instance under the multi-instance model).
/// `session: None` is the user's default Herdr instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProjectBinding {
    pub(crate) project_id: String,
    pub(crate) project_name: String,
    pub(crate) session: Option<String>,
    /// B1: SSH bridge socket for remote-machine instances (None = local).
    pub(crate) socket_override: Option<std::path::PathBuf>,
}

/// Process-wide services and cross-window bookkeeping shared by every Shardlane window.
/// Multi-instance model (2026-09): one Herdr instance per Project and one Project
/// displayed per window, so each window owns its client, runtime state, and TUI
/// session and every window can be live simultaneously. Only truly process-scoped
/// things live here.
pub(crate) struct ShellSharedRuntime {
    /// Per-instance TUI managers: at most one hosted TUI child per Herdr
    /// instance, shared by every viewer — the desktop window bound to that
    /// Project AND the Remote/mobile clients.
    pub(crate) tui_registry: std::sync::Arc<shardlane_host::shared_tui::TuiManagerRegistry>,
    /// Host-owned shared live Conversation session owner (M3).
    pub(crate) conversation_sessions: std::sync::Arc<shardlane_host::ConversationSessionManager>,
    /// Host-owned semantic follow-up queue (M4).
    pub(crate) follow_up_queue: std::sync::Arc<shardlane_host::ConversationFollowUpQueue>,
    /// Host-owned queued delivery coordinator (R2-03/CR-01).
    pub(crate) delivery: std::sync::Arc<shardlane_host::ConversationDeliveryCoordinator>,
    /// The one NSStatusItem per process. `StatusBarController` is inherently process-global
    /// (module statics hold the action sender and the last snapshot); constructing one per
    /// window would duplicate the menu-bar item.
    pub(crate) status_bar: SendStatusBar,
    /// R6: the loopback Remote API service handle (spawned/stopped by the settings toggle).
    /// Process-scoped and bound to the default instance: a second window toggling Settings
    /// must not spawn a second listener.
    pub(crate) remote_server: std::sync::Mutex<Option<shardlane_remote::RemoteServerHandle>>,
    /// Per-session display-name overrides (cosmetic; persisted in settings).
    pub(crate) display_names: std::sync::Mutex<std::collections::HashMap<String, String>>,
    /// Cached Herdr instance list (CLI `session list`); refreshed in the
    /// background and on picker/New-Workspace actions — never per frame.
    pub(crate) instances: std::sync::Mutex<Vec<shardlane_host::herdr::HerdrSessionListing>>,
    /// Live SSH socket bridges (B1): the remote machine's herdr socket is
    /// forwarded to a local unix socket; instances behind it are addressed by
    /// that socket path on bind.
    pub(crate) ssh_bridges: std::sync::Mutex<Vec<crate::ssh_bridge::SshBridge>>,
    /// Remote instance listings per device id (fetched over SSH at bridge
    /// bring-up and picker refresh).
    pub(crate) remote_sessions: std::sync::Mutex<
        std::collections::HashMap<String, Vec<shardlane_host::herdr::HerdrSessionListing>>,
    >,
    /// Which window currently displays which Project (project id → WindowId). Sidebar
    /// clicks and ⌘N use this to jump to the existing window instead of duplicating.
    pub(crate) project_windows: std::sync::Mutex<std::collections::HashMap<String, WindowId>>,
    /// Live window registry (weak views) for app-level routing: keystrokes, status-bar and
    /// notification clicks must land in the entity that owns the receiving window.
    pub(crate) windows: std::sync::Mutex<Vec<ShellWindowRecord>>,
}

/// `StatusBarController` owns a `Retained<NSStatusItem>` (main-thread AppKit),
/// which is not `Send`/`Sync` by default. Every Shardlane touch point runs on
/// the UI thread, and its click path crosses threads only through the
/// controller's own internal `Sender` static, so storing it in the
/// process-wide runtime can assert `Send`/`Sync` safely.
pub(crate) struct SendStatusBar(pub(crate) Option<status_bar::StatusBarController>);
unsafe impl Send for SendStatusBar {}
unsafe impl Sync for SendStatusBar {}

#[derive(Clone)]
pub(crate) struct ShellWindowRecord {
    pub(crate) id: WindowId,
    pub(crate) handle: AnyWindowHandle,
    pub(crate) view: WeakEntity<ShardlaneApp>,
}

impl ShellSharedRuntime {
    /// Constructs the process-wide services once. Returns the status-bar and notification
    /// action receivers for the app-level consumers (`spawn_global_action_consumer`).
    pub(crate) fn new(
        display_names: std::collections::HashMap<String, String>,
    ) -> (
        std::sync::Arc<Self>,
        async_channel::Receiver<status_bar::StatusBarAction>,
        async_channel::Receiver<notifications::NotificationAction>,
    ) {
        let (status_bar_tx, status_bar_rx) = async_channel::unbounded();
        let status_bar = status_bar::StatusBarController::new(status_bar_tx);
        let (notification_tx, notification_rx) = async_channel::unbounded();
        notifications::attach_action_sender(notification_tx);
        // The queue view reads the same queue truth the coordinator delivers from.
        let delivery_queue = shardlane_host::ConversationDeliveryCoordinator::new(
            std::sync::Arc::new(shardlane_host::ConversationFollowUpQueue::new()),
            None,
        );
        let follow_up_queue = std::sync::Arc::clone(delivery_queue.queue());
        let runtime = std::sync::Arc::new(Self {
            tui_registry: std::sync::Arc::new(
                shardlane_host::shared_tui::TuiManagerRegistry::default(),
            ),
            conversation_sessions: {
                let manager = std::sync::Arc::new(shardlane_host::ConversationSessionManager::new(
                    crate::history::transcript_source::history_db_path(),
                ));
                // CR-15: idle shared-only LiveSessions are reclaimed by a real timed reaper,
                // no longer relying on later manager operations to trigger it.
                manager.spawn_shared_reaper();
                manager
            },
            follow_up_queue,
            delivery: delivery_queue,
            status_bar: SendStatusBar(status_bar),
            remote_server: std::sync::Mutex::new(None),
            display_names: std::sync::Mutex::new(display_names),
            instances: std::sync::Mutex::new(
                shardlane_host::herdr::list_sessions().unwrap_or_default(),
            ),
            ssh_bridges: std::sync::Mutex::new(Vec::new()),
            remote_sessions: std::sync::Mutex::new(std::collections::HashMap::new()),
            project_windows: std::sync::Mutex::new(std::collections::HashMap::new()),
            windows: std::sync::Mutex::new(Vec::new()),
        });
        (runtime, status_bar_rx, notification_rx)
    }

    pub(crate) fn register_window(&self, handle: AnyWindowHandle, view: WeakEntity<ShardlaneApp>) {
        let id = handle.window_id();
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        windows.retain(|record| record.id != id);
        windows.push(ShellWindowRecord { id, handle, view });
    }

    /// Registry lookup with stale-entry pruning: a closed window's weak view fails to
    /// upgrade, and its record is reclaimed on the next pass.
    fn window_record(&self, id: WindowId) -> Option<ShellWindowRecord> {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        windows.retain(|record| record.view.upgrade().is_some());
        windows.iter().find(|record| record.id == id).cloned()
    }

    pub(crate) fn window_view(&self, id: WindowId) -> Option<Entity<ShardlaneApp>> {
        self.window_record(id)
            .and_then(|record| record.view.upgrade())
    }

    /// The window currently displaying `project_id`, if that window still lives.
    pub(crate) fn window_handle_for_project(&self, project_id: &str) -> Option<AnyWindowHandle> {
        let id = self
            .project_windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(project_id)
            .copied()?;
        self.window_record(id).map(|record| record.handle)
    }

    /// Records the window displaying a Project.
    pub(crate) fn set_project_window(&self, project_id: &str, window_id: WindowId) {
        self.project_windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(project_id.to_string(), window_id);
    }

    /// Unregisters a window from `project_id` — only while the map still points
    /// at THAT window. Called on rebind: the window is alive but abandoning the
    /// project, so an aliveness check alone would leave a stale entry that made
    /// later switches to the old project self-activate instead of rebinding.
    pub(crate) fn clear_project_window(&self, project_id: &str, window_id: WindowId) {
        let mut map = self
            .project_windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if map
            .get(project_id)
            .is_some_and(|mapped| *mapped == window_id)
        {
            map.remove(project_id);
        }
    }

    pub(crate) fn instance_list(&self) -> Vec<shardlane_host::herdr::HerdrSessionListing> {
        self.instances
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Refreshes the cached instance list (CLI round trip; call off the hot path).
    pub(crate) fn refresh_instances(&self) {
        *self
            .instances
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            shardlane_host::herdr::list_sessions().unwrap_or_default();
    }

    /// Registers a live SSH bridge (B1) and caches the machine's instance
    /// list so the workspace picker can offer them.
    pub(crate) fn register_bridge(&self, bridge: crate::ssh_bridge::SshBridge) {
        let listing = crate::ssh_bridge::list_remote_sessions(&bridge.target);
        let device_id = bridge.device_id.clone();
        self.ssh_bridges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(bridge);
        if let Ok(sessions) = listing {
            self.remote_sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(device_id, sessions);
        }
    }

    pub(crate) fn remote_sessions_for(
        &self,
        device_id: &str,
    ) -> Vec<shardlane_host::herdr::HerdrSessionListing> {
        self.remote_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(device_id)
            .cloned()
            .unwrap_or_default()
    }

    /// The bridge serving a remote instance key (`ssh:<device>:<session>`).
    pub(crate) fn bridge_for_key(&self, key: &str) -> Option<crate::ssh_bridge::SshBridge> {
        let device_id = key.split(':').nth(1)?.to_string();
        self.ssh_bridges
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|bridge| bridge.device_id == device_id)
            .cloned()
    }

    /// The cosmetic display name for an instance (settings override or the
    /// session name itself; "default" reads as "Default").
    pub(crate) fn display_name(&self, session: Option<&str>) -> String {
        let key = session.unwrap_or("default");
        let overrides = self
            .display_names
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        overrides.get(key).cloned().unwrap_or_else(|| {
            if key == "default" {
                "Default".to_string()
            } else {
                key.to_string()
            }
        })
    }

    /// Sets a display-name override and persists it (rename = cosmetic only;
    /// the Herdr instance itself is untouched).
    pub(crate) fn set_display_name(&self, session: Option<&str>, name: String) {
        let key = session.unwrap_or("default").to_string();
        let default_display = session.is_none() && name == "Default";
        {
            let mut overrides = self
                .display_names
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if default_display || overrides.get(&key).is_some_and(|old| *old == name) {
                overrides.remove(&key);
            } else {
                overrides.insert(key.clone(), name);
            }
        }
        settings::ApplicationConfig::persist_instance_display_names(
            self.display_names
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
        );
    }
}

struct ShardlaneApp {
    device: DeviceEndpoint,
    /// Process-wide services + cross-window bookkeeping (TUI live arbitration, window
    /// registry, status bar, Remote server slot, Lazygit slot).
    shared: std::sync::Arc<ShellSharedRuntime>,
    client: Option<HerdrClient>,
    herdr_user_config: herdr_tui::HerdrUserConfigSnapshot,
    terminal: Option<Arc<Mutex<ManagedTerminal>>>,
    /// Small Ghostty key encoder snapshot kept separate from the VT/frame mutex so repeated
    /// named/modifier keys never wait for a full-screen extraction.
    terminal_key_encoder: Option<Arc<Mutex<GhosttyKeyEncoderState>>>,
    terminal_input: Option<TerminalControlInput>,
    terminal_target: Option<String>,
    /// P1-3: all dynamic (chord, scope) pairs of the currently effective generation; rebinding adds NoAction for retired chords.
    dyn_keymap_generation: Vec<(String, shortcuts::ShortcutScope)>,
    /// UX (2026-08-27 review): the in-progress state of "click a shortcut to record a new combination" on the
    /// Shortcuts settings page. Some(id) means the next keystroke is being captured for that id; during
    /// recording, all registered chords are temporarily suppressed by NoAction (later entries win), and
    /// exit restores everything at once via rebind_shortcuts().
    pub(crate) shortcut_recording: Option<shortcuts::ShortcutId>,
    /// Active project git status snapshot (consumed by the header's +/- pill and Info popover; 12s freshness).
    git_status: Option<git_status::GitStatusSnapshot>,
    terminal_attach_target: Option<String>,
    terminal_size: Option<TerminalSize>,
    terminal_surface_size: Option<TerminalSize>,
    terminal_token: u64,
    /// Theme-derived dynamic default colors (foreground, background) that Shardlane —
    /// as the hosted terminal emulator — seeds into its Ghostty model and reports to
    /// the Herdr TUI child's OSC 10/11 queries. Both derive from the active Herdr
    /// official theme; `None` until the first hosted attach/theme application.
    hosted_terminal_colors: Option<(u32, u32)>,
    /// The most recent raw Ghostty frame before the presentation-only Herdr chrome projection. The projected
    /// `terminal_frame` has fewer rows/cols and cannot serve as the signature-reuse baseline for the hosted TUI.
    terminal_raw_frame: Arc<TerminalFrame>,
    /// The projection deriving `terminal_frame` from `terminal_raw_frame`; when the raw frame's allocation is
    /// unchanged, the presentation path can use it to skip a second deep projection.
    terminal_frame_projection: crate::herdr_tui::TuiChromeProjection,
    terminal_frame: Arc<TerminalFrame>,
    terminal_pane: Entity<TerminalPane>,
    /// Horizontal scroll of the native content-area Tab strip (shell_tabs.rs); the strip's
    /// page arrows and edge clamping read/mutate this handle.
    native_tab_scroll: ScrollHandle,
    /// Last scroll max seen by the strip's measure canvas, so the page arrows re-render once
    /// when the strip starts/stops overflowing (max_offset is only written during paint).
    native_tab_scroll_max: std::cell::Cell<f64>,
    pending_copy_selection: Option<PendingTerminalCopy>,
    /// Sender side of the hosted terminal's poll wake channel (audit B20): a pended
    /// copy-on-select wakes the poll immediately instead of waiting out the backoff.
    terminal_poll_wake: Option<TerminalWakeSender>,
    last_terminal_frame_at: Option<Instant>,
    last_terminal_input_at: Option<Instant>,
    terminal_pending_frame: bool,
    /// Resize RPC debounce for the single-pane path (same shape as PaneTerminalSlot.rpc_debounce_*).
    /// VT bytes waiting while ghostty frame() holds the session lock.
    pending_vt: Vec<u8>,
    applied_ui_theme: Option<UiTheme>,
    terminal_geometry: TerminalGeometry,
    /// F47: event stream auto-reconnect state. `attempts` counts consecutive failures (the backoff input);
    /// the `_reconnect_script` handle prevents double chains (the old chain is dropped/cancelled before a new one spawns).
    events_reconnect_attempts: u32,
    _events_reconnect_script: BackgroundJob<()>,
    /// F19: pane subscription retry state (same backoff accumulator).
    pane_subscription_retry_attempts: u32,
    _pane_subscription_retry_script: BackgroundJob<()>,
    /// F4: client-local per-Tab Pane selection memory (tab_id → pane_id). When switching back to a Tab it
    /// takes priority over Herdr-side focus memory to restore the user's last pane; a failed existence check voids it.
    /// F20: client-local per-Project Tab selection memory (workspace_id → tab_id).
    /// Recorded on tab switch; used on project switch with priority over Herdr's active_tab_id.
    workspace_tab_selection_memory: HashMap<String, String>,
    shell_sidebar_width: f64,
    /// Sidebar drag in progress: Shell = edge resize (press x, starting width); Sections = section height resize
    /// (handle index, press y, starting heights of the three segments).
    sidebar_drag: Option<SidebarDrag>,
    /// Page navigation history stacks (back/forward, like a Cursor/Codex browser history).
    nav_back_stack: Vec<FocusIntent>,
    nav_forward_stack: Vec<FocusIntent>,
    /// Plan 060 Phase 1: Ctrl-Tab Agent Switcher overlay。
    agent_switcher: crate::agent_switcher::AgentSwitcherState,
    /// Sidebar/right panel mid-slide (WidthTween): while Some, render advances each frame
    /// and requests the next; *_rendered_width is this frame's actual rendered width (the target width
    /// after dragging/settling).
    sidebar_slide: Option<WidthTween>,
    right_panel_slide: Option<WidthTween>,
    /// Tab switch fade-in for terminal content.
    terminal_fade_start: Option<Instant>,
    sidebar_rendered_width: f64,
    right_panel_rendered_width: f64,
    state: HerdrState,
    /// This view's own entity id: cross-entity aggregation (`sync_open_workspaces`)
    /// must never re-enter the entity whose update lease it already holds.
    entity_id: gpui::EntityId,
    /// This window's bound Project (= one Herdr instance). `None` renders the Project
    /// picker: a brand-new window that has not yet claimed a Project.
    binding: Option<ProjectBinding>,
    /// Bumped on every bind/unbind so superseded bootstrap tasks and event loops
    /// from a previous Project self-terminate instead of writing foreign state.
    binding_generation: u64,
    /// The Project picker overlay (⌘N windows start here; also reachable while bound
    /// to switch/jump without a sidebar round trip).
    show_project_picker: bool,
    /// Picker filter input (lazily created — InputState needs a live Window).
    project_picker_filter: Option<Entity<InputState>>,
    /// Machine panel SSH-target input (lazily created, same reason).
    machine_ssh_input: Option<Entity<InputState>>,
    /// Picker refresh-in-flight flag (instance list lives on the shared runtime).
    instances_refresh_requested: std::cell::Cell<bool>,
    status: ConnectionStatus,
    show_help: bool,
    show_settings: bool,
    tui_host: crate::herdr_tui::HerdrTuiHostState,
    /// The Hosted Herdr TUI's presentation-only chrome projection. Herdr/Ghostty still hold the
    /// full screen; Shardlane shows only the Pane region matching `pane.layout.area`.
    tui_chrome_projection: crate::herdr_tui::TuiChromeProjection,
    /// The viewer-local model's actual grid after adopting a remote viewer's shared-session
    /// resize. Chrome margins for the adoption path must be derived from this grid, not from
    /// the desktop's compensated target grid.
    tui_adopted_grid: Option<(u16, u16)>,
    /// TUI host respawn cooldown deadline (short-term loop prevention after process exit/spawn failure);
    /// navigation/restart actions can punch through the cooldown (ensure_tui_surface's force).
    tui_respawn_blocked_until: Option<Instant>,
    /// The TUI mode's most recent focus chain task: the last click wins (same semantics as _navigation_task).
    _tui_focus_task: BackgroundJob<()>,
    /// Hosted chrome geometry probe; runs only briefly after attach/focus, never periodic polling.
    _tui_projection_task: BackgroundJob<()>,
    /// Applying hosted-TUI presentation config is serialized by replacement: a newer Settings
    /// change cancels the previous prepare/reload/restart task so rapid theme clicks cannot race.
    _tui_config_apply_task: BackgroundJob<()>,
    settings_section: SettingsSection,
    /// Settings > Providers second-level destination. `None` is the provider list.
    settings_provider_detail: Option<AgentId>,
    /// R6: the loopback Remote API service handle lives on the process-wide
    /// `shared` runtime (`ShellSharedRuntime::remote_server`), never per-window.
    /// Mobile port input state (Settings → Mobile → Port; lazily created, ensured by render every frame).
    mobile_port_input: Option<Entity<InputState>>,
    /// Port commit subscription (Enter/Blur commits; dropping it unsubscribes).
    mobile_port_subscription: Option<Subscription>,
    /// Settings sidebar titlebar drag armed state (window_drag_region semantics).
    settings_titlebar_drag_armed: bool,
    /// Settings sidebar search field (settings-sidebar search filtering navigation by label);
    /// InputState needs a window to construct, so it's lazily created at first render.
    settings_sidebar_search: Option<Entity<InputState>>,
    history: history::HistoryUiState,
    /// Chat semantic sidecar presentation (same Herdr Agent; disposable).
    chat: chat::ChatUi,
    /// Host-owned shared live Conversation session owner (M3): the Chat worker
    /// holds leases here and Remote live reads are served from the same state.
    conversation_sessions: std::sync::Arc<shardlane_host::ConversationSessionManager>,
    /// Host-owned semantic follow-up queue (M4): one `Send after turn` item per
    /// live Conversation; delivery uses exact-target waits, never a watcher.
    follow_up_queue: std::sync::Arc<shardlane_host::ConversationFollowUpQueue>,
    /// R2-03/CR-01: Host-owned queued delivery coordinator (shared by Chat/History/Remote;
    /// the queue view still reads `follow_up_queue`).
    delivery: std::sync::Arc<shardlane_host::ConversationDeliveryCoordinator>,
    /// This window's Herdr TUI session manager. For the Default Project this is the
    /// process-level manager shared with the Remote server's viewers; for a
    /// named-session Project it is a window-owned manager whose TUI child lives
    /// and dies with the binding.
    tui_manager: std::sync::Arc<shardlane_host::shared_tui::TuiManager>,
    right_panel: right_panel::RightPanelState,
    /// Optional visible Lazygit child. This auxiliary slot is independent from the
    /// primary hosted Herdr PTY and is destroyed when the surface is hidden/closed.
    lazygit_session: right_panel::lazygit::LazygitSession,
    /// Settings-only CLI discovery cache; render never shells out synchronously.
    lazygit_detection: Option<right_panel::lazygit::LazygitDetection>,
    lazygit_detection_requested: std::cell::Cell<bool>,
    _lazygit_detection_task: BackgroundJob<()>,
    /// Lazygit executable override input (Settings → Lazygit); lazily created because
    /// InputState needs a live Window and must not be constructed during bootstrap.
    lazygit_executable_input: Option<Entity<InputState>>,
    lazygit_executable_subscription: Option<Subscription>,
    /// Per-Project (Herdr runtime workspace) right panel Tab and content snapshots.
    right_panel_projects: HashMap<String, right_panel::RightPanelProjectContent>,
    /// The runtime workspace id the current `right_panel` content is bound to.
    right_panel_runtime_id: Option<String>,
    /// Native WKWebView pool for Browser surfaces (keyed by BrowserSessionId; floats above the GPUI
    /// compositing layer and must be explicitly hidden when the panel closes — see right_panel::webview).
    browser_webviews:
        HashMap<crate::browser_profile::BrowserSessionId, right_panel::webview::BrowserWebview>,
    /// Browser address bar input states (keyed by URL; mirroring Browser.address, Submit
    /// navigates immediately, and address_dirty prevents page write-backs from overwriting mid-typing content).
    browser_addresses: HashMap<String, Entity<InputState>>,
    /// UX (review #12): the most recent failed navigation URL (shown in the GPUI-level error card).
    pub(crate) browser_load_failure: Option<String>,
    /// Submit subscriptions for address inputs (held per URL key; dropping unsubscribes).
    browser_address_subscriptions: HashMap<String, Subscription>,
    new_agent_open: bool,
    new_agent_ui: Option<new_agent::NewAgentUiState>,
    /// New Agent composer / right panel Files Project context: updated on sidebar Project clicks or
    /// runtime focus; the composer prefers this field when opening, falling back to focused_workspace_id.
    new_agent_context_workspace_id: Option<String>,
    scripts: ScriptRegistry,
    observed_services: Vec<scripts::ObservedService>,
    search_open: bool,
    client_picker: Option<ClientPickerOverlay>,
    about_open: bool,
    rename_open: bool,
    script_dialog_open: bool,
    navigation_loading: bool,
    navigation_token: u64,
    navigation_reconcile_pending: bool,
    font_size_slider: Entity<SliderState>,
    line_height_slider: Entity<SliderState>,
    opacity_slider: Entity<SliderState>,
    /// Full sidebar chrome as own entity — spaces toggle must not dirty root/terminal.
    sidebar_pane: Entity<SidebarPane>,
    sidebar_collapsed: bool,
    sidebar_auto_collapsed: bool,
    projects_collapsed: bool,
    agents_collapsed: bool,
    services_collapsed: bool,
    theme_mode: ThemeMode,
    /// Unconsumed precise wheel/trackpad distance in pixels. Consumed rows are subtracted
    /// immediately so a continuous gesture cannot resend its cumulative distance.
    terminal_scroll_residual_px: f64,
    input_queue: VecDeque<TerminalInputCommand>,
    input_in_flight: bool,
    window_active: bool,
    /// Per-target notification throttle for terminal BELs (system notifications only while the window is unfocused).
    terminal_bell_last_notify: HashMap<String, Instant>,
    /// Sidebar Tab close confirmation: the first X click enters the red confirm state, the second performs the close.
    pending_close_tab: Option<String>,
    /// Browser profile destructive-action arm (two-click confirm: first click arms, the
    /// second within the arm window executes — audit E03). State lives on the app so the
    /// armed row re-renders and unrelated actions can disarm.
    browser_confirm: Option<(
        crate::browser_profile::BrowserProfileConfirmAction,
        String,
        Instant,
    )>,
    /// History-source Remove arm (two-click confirm — audit E22).
    history_remove_confirm: Option<(AgentId, std::path::PathBuf, Instant)>,
    /// Sidebar list roving focus registries: rows are not Tab stops, the container is the sole stop.
    sidebar_roving_workspaces: Rc<interaction::RovingList>,
    sidebar_roving_agents: Rc<interaction::RovingList>,
    /// Whether the window frame changed again since the last persist; deactivation triggers one save_config.
    window_bounds_dirty: bool,
    window_handle: Option<AnyWindowHandle>,
    reported_terminal_focus: Option<(String, String)>,
    ime_target: Option<String>,
    ime_marked_text: String,
    ime_selected_range: Option<Range<usize>>,
    /// Terminal text selection is client-local and independent per terminal target.
    selections: HashMap<String, TerminalSelection>,
    selection_target: Option<String>,
    selection_geometry: Option<SelectionGeometry>,
    selection_anchor_cell: Option<(u16, u16)>,
    selection_mode: TerminalSelectionMode,
    selection_dragged: bool,
    selecting: bool,
    focus_handle: FocusHandle,
    config: settings::ApplicationConfig,
    initializing: bool,
    startup_started_at: Instant,
    last_startup_change_at: Instant,
    render_seq: u64,
    script_monitor_wake: async_channel::Sender<()>,
    _script_monitor: BackgroundJob<()>,
    _config_save_script: BackgroundJob<()>,
    /// R4: provider CLI availability cache (None = not yet probed; probed in the background when entering Settings).
    provider_availability: Option<std::collections::HashMap<shardlane_history::AgentId, String>>,
    provider_availability_requested: std::cell::Cell<bool>,
    /// Herdr integration health cache; status probes never run from render.
    provider_integration_health: Option<Vec<shardlane_host::agent_integrations::IntegrationHealth>>,
    provider_integration_health_requested: std::cell::Cell<bool>,
    /// Per-roster source existence probe used only for Settings status text.
    history_source_probe:
        Option<std::collections::HashMap<shardlane_history::HistorySourceKey, bool>>,
    history_source_probe_requested: std::cell::Cell<bool>,
    /// Derived index metadata (path/size); the catalog itself remains disposable.
    history_index_snapshot: Option<providers_view::HistoryIndexSnapshot>,
    history_index_snapshot_requested: std::cell::Cell<bool>,
    _config_reload_script: BackgroundJob<()>,
    pane_event_subscription_ids: Vec<String>,
    _pane_event_subscription_script: BackgroundJob<()>,
    _pane_event_subscription_handshake: BackgroundJob<()>,
    /// The most recent navigation RPC's task handle: when a new navigation replaces the old task the handle
    /// is dropped and GPUI cancels it immediately (executor.rs:54), so superseded socket round trips stop running.
    _navigation_script: BackgroundJob<()>,
    /// P2-1 steering composer persistent state (created when the first agent pane is focused).
    steering: Option<steering::SteeringState>,
    _config_watcher: Option<settings::ConfigWatcher>,
    _config_subscriptions: Vec<Subscription>,
}

/// Owns Sidebar-only disclosure state so folder toggles never re-render the terminal tree.
struct SidebarPane {
    app: Entity<ShardlaneApp>,
    expanded_projects: HashSet<String>,
    panes_by_project: HashMap<String, Vec<Pane>>,
    project_pane_loads_in_flight: HashSet<String>,
    /// Audit A11: the last workspace_panes error per Project — an expanded Project renders a
    /// "Couldn't load panes — click to retry" row instead of silently showing an empty list.
    project_pane_errors: HashMap<String, String>,
}

impl SidebarPane {
    fn toggle_project(&mut self, workspace_id: &str, cx: &mut Context<Self>) -> bool {
        let expanded = if self.expanded_projects.remove(workspace_id) {
            false
        } else {
            self.expanded_projects.insert(workspace_id.to_string());
            true
        };
        cx.notify();
        expanded
    }

    fn begin_project_pane_load(&mut self, workspace_id: &str) -> bool {
        !self.panes_by_project.contains_key(workspace_id)
            && self
                .project_pane_loads_in_flight
                .insert(workspace_id.to_string())
    }

    fn invalidate_project_panes(&mut self, workspace_id: &str) -> bool {
        self.panes_by_project.remove(workspace_id);
        self.expanded_projects.contains(workspace_id)
    }

    fn finish_project_pane_load(
        &mut self,
        workspace_id: String,
        panes: Result<Vec<Pane>, herdr::HerdrError>,
        cx: &mut Context<Self>,
    ) {
        self.project_pane_loads_in_flight.remove(&workspace_id);
        match panes {
            Ok(panes) => {
                self.project_pane_errors.remove(&workspace_id);
                self.panes_by_project.insert(workspace_id, panes);
            }
            // Audit A11: don't silently swallow the RPC failure — the expanded Project shows a
            // retry row carrying the error instead of a blank pane list.
            Err(error) => {
                self.project_pane_errors
                    .insert(workspace_id, error.to_string());
            }
        }
        cx.notify();
    }

    fn update_cached_pane_label(&mut self, pane_id: &str, label: &str, cx: &mut Context<Self>) {
        let mut changed = false;
        for panes in self.panes_by_project.values_mut() {
            if let Some(pane) = panes.iter_mut().find(|pane| pane.pane_id == pane_id) {
                if pane.label.as_deref() != Some(label) {
                    pane.label = Some(label.to_string());
                    changed = true;
                }
            }
        }
        if changed {
            cx.notify();
        }
    }

    fn update_cached_pane_location(
        &mut self,
        pane_id: &str,
        workspace_id: &str,
        tab_id: &str,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        for panes in self.panes_by_project.values_mut() {
            if let Some(pane) = panes.iter_mut().find(|pane| pane.pane_id == pane_id) {
                if pane.workspace_id.as_deref() != Some(workspace_id)
                    || pane.tab_id.as_deref() != Some(tab_id)
                {
                    pane.workspace_id = Some(workspace_id.to_string());
                    pane.tab_id = Some(tab_id.to_string());
                    changed = true;
                }
            }
        }
        if changed {
            cx.notify();
        }
    }
}

impl Render for SidebarPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Entity::update borrows the handle: the closure's immutable captures of other fields don't
        // conflict with self.app, so there's no need to clone the whole projection across the entity
        // boundary (with its dozen-plus heap Strings per session).
        // Safe: ShardlaneApp is not mid-update when only SidebarPane is dirty.
        self.app.update(cx, |app, app_cx| {
            app.build_sidebar(
                sidebar::SidebarProjection {
                    expanded_projects: &self.expanded_projects,
                    panes_by_project: &self.panes_by_project,
                    project_pane_loads_in_flight: &self.project_pane_loads_in_flight,
                    project_pane_errors: &self.project_pane_errors,
                },
                window,
                app_cx,
            )
        })
    }
}

const SIDEBAR_MIN_WIDTH: f64 = 220.0;
const SIDEBAR_MAX_WIDTH: f64 = 420.0;
/// Audit A26: the single compact-shell breakpoint — Sidebar auto-collapse, the Header's
/// compact layout, and the Settings page layout all switch at this one width (the former
/// HEADER_COMPACT_WINDOW_WIDTH alias and its duplicate predicate were deleted).
const SIDEBAR_AUTO_COLLAPSE_WINDOW_WIDTH: f64 = 980.0;
const TERMINAL_MIN_WIDTH: f64 = 320.0;

fn sidebar_should_auto_collapse(window_width: f64) -> bool {
    window_width < SIDEBAR_AUTO_COLLAPSE_WINDOW_WIDTH
}

fn is_secondary_surface(show_settings: bool, history_open: bool, new_agent_open: bool) -> bool {
    show_settings || history_open || new_agent_open
}

/// Audit A30 (multi-window update): the shared action-channel consumer (status bar and
/// notification clicks route their async_channel of actions onto the UI thread). With
/// multiple windows the consumer is process-level and routes to the window that most
/// recently claimed its Project binding — the natural click target.
fn spawn_global_action_consumer<A>(
    cx: &mut App,
    shared: std::sync::Arc<ShellSharedRuntime>,
    rx: async_channel::Receiver<A>,
    handle: fn(&mut ShardlaneApp, A, &mut Window, &mut Context<ShardlaneApp>),
    owns: fn(&ShardlaneApp, &A) -> bool,
) where
    A: Send + 'static,
{
    cx.spawn(async move |cx| {
        while let Ok(action) = rx.recv().await {
            // Candidate windows, newest registration first.
            let candidates = {
                let windows = shared
                    .windows
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                windows
                    .iter()
                    .rev()
                    .filter_map(|record| record.view.upgrade().map(|view| (record.handle, view)))
                    .collect::<Vec<_>>()
            };
            // A2: prefer the window whose bound instance actually owns the
            // action's target (e.g. the pane a notification points at); fall
            // back to the most recently registered window for process-scoped
            // actions (menu-bar clicks).
            let mut target = None;
            for candidate in &candidates {
                let owned = candidate
                    .1
                    .update(cx, |view, _| owns(view, &action))
                    .unwrap_or(false);
                if owned {
                    target = Some(candidate.clone());
                    break;
                }
            }
            let target = target.or_else(|| candidates.first().cloned());
            let Some((window_handle, view)) = target else {
                continue;
            };
            let _ = cx.update_window(window_handle, |_, window, cx| {
                view.update(cx, |view, cx| {
                    handle(view, action, window, cx);
                });
            });
        }
    })
    .detach();
}

/// A2: a notification's FocusPane belongs to the window whose instance hosts
/// that pane.
fn notification_targets_view(
    view: &ShardlaneApp,
    action: &notifications::NotificationAction,
) -> bool {
    match action {
        notifications::NotificationAction::FocusPane(pane_id) => {
            view.state.panes.iter().any(|pane| pane.pane_id == *pane_id)
        }
    }
}

/// Status-bar actions are process-scoped: every window can serve them.
fn status_bar_targets_view(_: &ShardlaneApp, _: &status_bar::StatusBarAction) -> bool {
    true
}

fn terminal_background_is_dark(color: u32) -> bool {
    let r = ((color >> 16) & 0xff) as u64;
    let g = ((color >> 8) & 0xff) as u64;
    let b = (color & 0xff) as u64;
    (299 * r + 587 * g + 114 * b) < 128_000
}

const TERMINAL_MAX_COLS: f64 = 500.0;
const TERMINAL_MAX_ROWS: f64 = 180.0;
// The authoritative terminal grid must be allowed to shrink to the cells that physically fit.
// Larger artificial minima make Ghostty/Herdr render columns or rows that GPUI cannot paint.
const PANE_MIN_COLS: u16 = 1;
const PANE_MIN_ROWS: u16 = 1;
/// Custom gpui-component titlebar height. The platform traffic lights remain native.
const APP_TITLEBAR_HEIGHT: f64 = 34.0;
/// Mirrors gpui-component 0.5.1's macOS `TITLE_BAR_LEFT_PADDING` so Shardlane can align
/// the Sidebar/content surface split without replacing the component's native drag behavior.
const APP_TITLEBAR_LEFT_INSET: f32 = 80.0;
const TERMINAL_PAINT_FUDGE: f64 = 2.0;
const TERMINAL_POLL_ACTIVE_MS: u64 = 16;
const TERMINAL_POLL_FOCUSED_IDLE_MAX_MS: u64 = 48;
const TERMINAL_POLL_BACKGROUND_IDLE_MAX_MS: u64 = 120;
const AGENT_RECONCILE_QUIET_MS: u64 = 250;
const AGENT_RECONCILE_MAX_DELAY_MS: u64 = 1_500;
const AGENT_RECONCILE_MIN_INTERVAL_MS: u64 = 750;
const NAVIGATION_RECONCILE_QUIET_MS: u64 = 180;
/// F48: the per-poll-iteration VT ingestion byte budget on the UI thread. Large Agent output dumps are
/// digested across multiple rounds (with a backlog, the active polling cadence is preserved), so a
/// multi-MB single frame can't block the UI thread.
const TERMINAL_DRAIN_BUDGET_BYTES: usize = 256 * 1024;
const TERMINAL_INTERACTIVE_DRAIN_BUDGET_BYTES: usize = 64 * 1024;
/// F19/F47's shared reconnect backoff: consecutive failure count → wait duration, exponential growth capped at 15 seconds.
fn reconnect_backoff(attempts: u32) -> Duration {
    const BASE_MS: u64 = 500;
    const MAX: Duration = Duration::from_secs(15);
    let exp = attempts.min(5);
    Duration::from_millis(BASE_MS.saturating_mul(1_u64 << exp)).min(MAX)
}

fn agent_reconcile_ready(
    pending: bool,
    last_dirty_age: Option<Duration>,
    pending_age: Option<Duration>,
    last_refresh_age: Duration,
) -> bool {
    pending
        && last_refresh_age >= Duration::from_millis(AGENT_RECONCILE_MIN_INTERVAL_MS)
        && (last_dirty_age
            .is_some_and(|age| age >= Duration::from_millis(AGENT_RECONCILE_QUIET_MS))
            || pending_age
                .is_some_and(|age| age >= Duration::from_millis(AGENT_RECONCILE_MAX_DELAY_MS)))
}

fn navigation_reconcile_ready(pending: bool, loading: bool, dirty_age: Option<Duration>) -> bool {
    pending
        && !loading
        && dirty_age.is_none_or(|age| age >= Duration::from_millis(NAVIGATION_RECONCILE_QUIET_MS))
}

fn should_project_terminal_frame(pending_frame: bool, render_blocked: bool) -> bool {
    pending_frame && !render_blocked
}

fn next_terminal_poll_interval(current: Duration, active: bool, focused: bool) -> Duration {
    if active {
        return Duration::from_millis(TERMINAL_POLL_ACTIVE_MS);
    }
    let max_ms = if focused {
        TERMINAL_POLL_FOCUSED_IDLE_MAX_MS
    } else {
        TERMINAL_POLL_BACKGROUND_IDLE_MAX_MS
    };
    let next_ms = (current.as_millis() as u64)
        .saturating_mul(2)
        .clamp(TERMINAL_POLL_ACTIVE_MS, max_ms);
    Duration::from_millis(next_ms)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalPollTrigger {
    Output,
    Timer,
    Closed,
}

fn terminal_drain_budget_bytes(recent_user_input: bool) -> usize {
    if recent_user_input {
        TERMINAL_INTERACTIVE_DRAIN_BUDGET_BYTES
    } else {
        TERMINAL_DRAIN_BUDGET_BYTES
    }
}

fn terminal_frame_min_interval_for_activity(
    recent_user_input: bool,
    output_pending: bool,
) -> Duration {
    if recent_user_input {
        // Input echo and trackpad-driven TUI redraws are already naturally bounded by PTY
        // output + GPUI's display-link paint. A second fixed 16 ms software gate can turn a
        // 60 Hz input stream into ~30 Hz when output arrives just before the gate becomes due.
        Duration::ZERO
    } else if output_pending {
        Duration::from_millis(TERMINAL_POLL_ACTIVE_MS)
    } else {
        Duration::from_millis(100)
    }
}

fn terminal_frame_retry_interval(
    last_frame_age: Option<Duration>,
    frame_min_interval: Duration,
) -> Duration {
    // If a frame arrived before its presentation budget was due, wait only the *remaining*
    // budget. Waiting another full active poll interval is the 16 ms -> ~33 ms cadence bug.
    last_frame_age
        .map(|age| frame_min_interval.saturating_sub(age))
        .unwrap_or(Duration::ZERO)
        .max(Duration::from_millis(1))
}

async fn wait_for_terminal_poll<F>(wake: &TerminalWakeReceiver, timer: F) -> TerminalPollTrigger
where
    F: Future<Output = ()>,
{
    let wake_wait = wake.recv();
    futures::pin_mut!(wake_wait, timer);
    match select(wake_wait, timer).await {
        Either::Left((Ok(()), _)) => TerminalPollTrigger::Output,
        Either::Left((Err(_), _)) => TerminalPollTrigger::Closed,
        Either::Right((_, _)) => TerminalPollTrigger::Timer,
    }
}

type TerminalSize = (u16, u16, u16, u16);

/// The single grid-size convention (audit B19): cols/rows derive from content-exact pixels
/// (padding excluded) and the size's pixel fields report those same content-exact pixels.
/// The window-derived attach fallback and the steady-state canvas measurement previously
/// disagreed on the pixel half (padding-inclusive vs content-exact), forcing one extra
/// SIGWINCH/re-layout after every attach; with one helper that drift cannot return.
fn grid_size_for(
    content_width: f64,
    content_height: f64,
    cell_width: f64,
    cell_height: f64,
) -> TerminalSize {
    (
        (content_width / cell_width)
            .floor()
            .clamp(f64::from(PANE_MIN_COLS), TERMINAL_MAX_COLS) as u16,
        (content_height / cell_height)
            .floor()
            .clamp(f64::from(PANE_MIN_ROWS), TERMINAL_MAX_ROWS) as u16,
        content_width.round().clamp(1.0, f64::from(u16::MAX)) as u16,
        content_height.round().clamp(1.0, f64::from(u16::MAX)) as u16,
    )
}

impl ShardlaneApp {
    /// Audit A31: the startup config is loaded once in `main` and handed in, instead of the
    /// constructor performing its own third disk read.
    fn with_config(
        config: settings::ApplicationConfig,
        shared: std::sync::Arc<ShellSharedRuntime>,
        cx: &mut Context<Self>,
    ) -> Self {
        // Herdr's normal user config is the only Herdr configuration source. Shardlane reads it
        // to mirror the app chrome and exposes visual editors for selected keys, but bootstrap
        // itself uses the same environment/default config resolution as the Herdr CLI.
        let mut config = config;
        let herdr_user_config = herdr_tui::load_herdr_user_config().unwrap_or_else(|error| {
            lag_log(format_args!(
                "herdr.config read failed; using defaults in Settings: {error}"
            ));
            herdr_tui::HerdrUserConfigSnapshot::default()
        });
        // Audit A18: the same auto-switch-aware resolution as sync_app_theme_from_herdr/theme()
        // (the bootstrap variant previously only looked at theme_name).
        if let Some(preset) = resolved_theme_preset(&herdr_user_config, true) {
            config.ui.appearance = if herdr_user_config.theme_auto_switch {
                "system".to_string()
            } else {
                preset.appearance.to_string()
            };
            config.ui.color_scheme = preset.scheme.to_string();
        }
        // Multi-instance model: a new window starts UNBOUND. `bind_project` (called by
        // `open_shell_window` for the startup window, or from the Project picker) owns
        // the per-Project bootstrap: client, state, event subscription, and TUI attach.
        let client: Option<HerdrClient> = None;
        let state = HerdrState::default();
        let status = ConnectionStatus::Offline("No Project".to_string());
        let startup_now = Instant::now();
        let theme_mode = theme_mode_from_config(&config.ui.appearance);
        let sidebar_width = config
            .ui
            .sidebar
            .width
            .clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
        let mut right_panel = right_panel::RightPanelState::default();
        right_panel.width = config.ui.right_panel.width.clamp(280.0, 1000.0) as f32;
        let font_size_slider = cx.new(|_| {
            SliderState::new()
                .min(TERMINAL_FONT_SIZE_RANGE.0)
                .max(TERMINAL_FONT_SIZE_RANGE.1)
                .step(1.0)
                .default_value(clamped_terminal_font_size(config.terminal.font_size))
        });
        let line_height_slider = cx.new(|_| {
            SliderState::new()
                .min(14.0)
                .max(34.0)
                .step(1.0)
                .default_value(config.terminal.line_height.clamp(14.0, 34.0))
        });
        let opacity_slider = cx.new(|_| {
            SliderState::new()
                .min(0.55)
                .max(1.0)
                .step(0.05)
                .default_value(config.ui.window.opacity.clamp(0.55, 1.0))
        });
        let mut config_subscriptions = Vec::new();
        config_subscriptions.push(cx.subscribe(
            &font_size_slider,
            |this, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(SliderValue::Single(value)) = event else {
                    return;
                };
                this.config.terminal.font_size = value.clamp(10.0, 24.0);
                this.apply_terminal_render_settings(cx);
                this.schedule_config_save(cx);
                cx.notify();
            },
        ));
        config_subscriptions.push(cx.subscribe(
            &line_height_slider,
            |this, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(SliderValue::Single(value)) = event else {
                    return;
                };
                this.config.terminal.line_height = value.clamp(14.0, 34.0);
                this.apply_terminal_render_settings(cx);
                this.schedule_config_save(cx);
                cx.notify();
            },
        ));
        config_subscriptions.push(cx.subscribe(
            &opacity_slider,
            |this, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(SliderValue::Single(value)) = event else {
                    return;
                };
                this.config.ui.window.opacity = value.clamp(0.55, 1.0);
                this.apply_native_window_preferences(cx);
                this.schedule_config_save(cx);
                cx.notify();
            },
        ));
        config_subscriptions.push(cx.on_app_quit(|this, cx| {
            this.sync_open_workspaces(cx);
            this.save_config();
            async {}
        }));
        let config_watcher = settings::ConfigWatcher::start();
        let config_changed_rx = config_watcher
            .as_ref()
            .map(settings::ConfigWatcher::changed_receiver);
        let (script_monitor_wake, script_monitor_wake_rx) = async_channel::bounded(1);
        let terminal_pane = cx.new(TerminalPane::new);
        let lazygit_session = right_panel::lazygit::LazygitSession::new(cx);
        let mut history = history::HistoryUiState::default();
        history.roster = Arc::new(HistoryAdapterRoster::new(&config.history_sources));
        let initially_expanded_project = state.focused_workspace_id.clone();
        let default_tui = shared.tui_registry.clone().get_or_create(None);
        let conversation_sessions = shared.conversation_sessions.clone();
        let follow_up_queue = shared.follow_up_queue.clone();
        let delivery = shared.delivery.clone();
        let mut view = Self {
            device: DeviceEndpoint::default(),
            shared,
            client,
            herdr_user_config,
            provider_availability: None,
            provider_availability_requested: std::cell::Cell::new(false),
            provider_integration_health: None,
            provider_integration_health_requested: std::cell::Cell::new(false),
            history_source_probe: None,
            history_source_probe_requested: std::cell::Cell::new(false),
            history_index_snapshot: None,
            history_index_snapshot_requested: std::cell::Cell::new(false),
            terminal: None,
            terminal_key_encoder: None,
            terminal_input: None,
            terminal_target: None,
            dyn_keymap_generation: Vec::new(),
            shortcut_recording: None,
            git_status: None,
            terminal_attach_target: None,
            terminal_size: None,
            terminal_surface_size: None,
            terminal_token: 0,
            hosted_terminal_colors: None,
            terminal_raw_frame: Arc::new(TerminalFrame::default()),
            terminal_frame_projection: crate::herdr_tui::TuiChromeProjection::default(),
            terminal_frame: Arc::new(TerminalFrame::default()),
            terminal_pane,
            native_tab_scroll: ScrollHandle::new(),
            native_tab_scroll_max: std::cell::Cell::new(-1.0),
            pending_copy_selection: None,
            terminal_poll_wake: None,
            last_terminal_frame_at: None,
            last_terminal_input_at: None,
            terminal_pending_frame: false,
            pending_vt: Vec::new(),
            applied_ui_theme: None,
            terminal_geometry: TerminalGeometry::default(),
            events_reconnect_attempts: 0,
            _events_reconnect_script: BackgroundJob::ready(()),
            pane_subscription_retry_attempts: 0,
            _pane_subscription_retry_script: BackgroundJob::ready(()),
            workspace_tab_selection_memory: HashMap::new(),
            shell_sidebar_width: sidebar_width,
            sidebar_drag: None,
            nav_back_stack: Vec::new(),
            nav_forward_stack: Vec::new(),
            agent_switcher: crate::agent_switcher::AgentSwitcherState::new(cx.focus_handle()),
            sidebar_slide: None,
            right_panel_slide: None,
            terminal_fade_start: None,
            sidebar_rendered_width: if config.ui.sidebar.collapsed {
                0.0
            } else {
                sidebar_width
            },
            right_panel_rendered_width: 0.0,
            state,
            entity_id: cx.entity().entity_id(),
            binding: None,
            binding_generation: 0,
            show_project_picker: false,
            project_picker_filter: None,
            machine_ssh_input: None,
            instances_refresh_requested: std::cell::Cell::new(false),
            status,
            show_help: false,
            show_settings: false,
            tui_host: crate::herdr_tui::HerdrTuiHostState::default(),
            tui_chrome_projection: crate::herdr_tui::TuiChromeProjection::default(),
            tui_adopted_grid: None,
            tui_respawn_blocked_until: None,
            _tui_focus_task: BackgroundJob::ready(()),
            _tui_projection_task: BackgroundJob::ready(()),
            _tui_config_apply_task: BackgroundJob::ready(()),
            settings_section: SettingsSection::default(),
            settings_provider_detail: None,
            mobile_port_input: None,
            mobile_port_subscription: None,
            browser_confirm: None,
            history_remove_confirm: None,
            settings_titlebar_drag_armed: false,
            settings_sidebar_search: None,
            history,
            chat: chat::ChatUi::default(),
            conversation_sessions,
            follow_up_queue,
            delivery,
            // Per-binding TUI manager: the Default Project shares the process-level
            // manager (with the Remote viewers); named-session Projects get their own
            // child, replaced on every (re)bind.
            tui_manager: default_tui,
            right_panel,
            lazygit_session,
            lazygit_detection: None,
            lazygit_detection_requested: std::cell::Cell::new(false),
            _lazygit_detection_task: BackgroundJob::ready(()),
            lazygit_executable_input: None,
            lazygit_executable_subscription: None,
            right_panel_projects: HashMap::new(),
            right_panel_runtime_id: None,
            browser_webviews: HashMap::new(),
            browser_addresses: HashMap::new(),
            browser_load_failure: None,
            browser_address_subscriptions: HashMap::new(),
            new_agent_open: true,
            new_agent_ui: None,
            new_agent_context_workspace_id: None,
            scripts: ScriptRegistry::load(),
            observed_services: Vec::new(),
            search_open: false,
            client_picker: None,
            about_open: false,
            rename_open: false,
            script_dialog_open: false,
            navigation_loading: false,
            navigation_token: 0,
            navigation_reconcile_pending: false,
            font_size_slider,
            line_height_slider,
            opacity_slider,
            sidebar_pane: {
                let app = cx.entity();
                cx.new(|_| SidebarPane {
                    app,
                    expanded_projects: initially_expanded_project.clone().into_iter().collect(),
                    panes_by_project: HashMap::new(),
                    project_pane_loads_in_flight: HashSet::new(),
                    project_pane_errors: HashMap::new(),
                })
            },
            sidebar_collapsed: config.ui.sidebar.collapsed,
            sidebar_auto_collapsed: false,
            projects_collapsed: config.ui.sidebar.projects_collapsed,
            agents_collapsed: config.ui.sidebar.agents_collapsed,
            services_collapsed: config.ui.sidebar.services_collapsed,
            theme_mode,
            terminal_scroll_residual_px: 0.0,
            input_queue: VecDeque::new(),
            input_in_flight: false,
            window_active: false,
            terminal_bell_last_notify: HashMap::new(),
            pending_close_tab: None,
            sidebar_roving_workspaces: Rc::new(interaction::RovingList::default()),
            sidebar_roving_agents: Rc::new(interaction::RovingList::default()),
            window_bounds_dirty: false,
            window_handle: None,
            reported_terminal_focus: None,
            ime_target: None,
            ime_marked_text: String::new(),
            ime_selected_range: None,
            selections: HashMap::new(),
            selection_target: None,
            selection_geometry: None,
            selection_anchor_cell: None,
            selection_mode: TerminalSelectionMode::Cell,
            selection_dragged: false,
            selecting: false,
            focus_handle: cx.focus_handle(),
            config,
            // A new window is never "initializing" — the per-Project bootstrap runs in
            // `bind_project`, which sets this flag for the duration of its connect.
            initializing: false,
            startup_started_at: startup_now,
            last_startup_change_at: startup_now,
            render_seq: 0,
            script_monitor_wake,
            _script_monitor: BackgroundJob::ready(()),
            _config_save_script: BackgroundJob::ready(()),
            _config_reload_script: BackgroundJob::ready(()),
            pane_event_subscription_ids: Vec::new(),
            _pane_event_subscription_script: BackgroundJob::ready(()),
            _pane_event_subscription_handshake: BackgroundJob::ready(()),
            _navigation_script: BackgroundJob::ready(()),
            steering: None,
            _config_watcher: config_watcher,
            _config_subscriptions: config_subscriptions,
        };
        view.prune_stale_pinned_tabs(cx);
        view._script_monitor = view.start_script_monitor(script_monitor_wake_rx, cx);
        if let Some(config_changed_rx) = config_changed_rx {
            view._config_reload_script = view.start_config_reload(config_changed_rx, cx);
        }
        view.apply_terminal_render_settings(cx);
        view.apply_remote_settings(cx);
        view.sync_pane_event_subscription(cx);
        view.notify_status_bar();
        view
    }

    // ------------------------------------------------------------------
    // Multi-instance Project binding (2026-09): one Herdr instance per
    // Project, one Project displayed per window. A window owns its client,
    // runtime state, and TUI session; switching Projects rebinds the window
    // (or jumps to the Project's existing window).
    // ------------------------------------------------------------------

    /// The Project this window currently displays.
    pub(crate) fn bound_project(&self) -> Option<&ProjectBinding> {
        self.binding.as_ref()
    }

    /// Sidebar/picker semantics: jump to the Project's existing window when one
    /// exists, otherwise bind (or rebind) THIS window to it.
    pub(crate) fn open_or_jump_project(
        &mut self,
        project_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_project_picker = false;
        self.pending_close_tab = None;
        if self
            .bound_project()
            .is_some_and(|binding| binding.project_id == project_id)
        {
            // Already displayed here: mirror the old same-project click semantics.
            self.exit_blocked_secondary_surfaces(cx);
            cx.notify();
            return;
        }
        if let Some(handle) = self.shared.window_handle_for_project(project_id) {
            let this_window = self.window_handle.as_ref().map(|handle| handle.window_id());
            if this_window == Some(handle.window_id()) {
                // Stale map pointing at THIS window while bound elsewhere:
                // fall through to a rebind instead of self-activating forever.
            } else {
                let _ = cx.update_window(handle, |_, target_window, _| {
                    target_window.activate_window();
                });
                return;
            }
        }
        // B1: remote-machine instance (`ssh:<device>:<session>`) — bind over
        // the device's live SSH bridge socket.
        if let Some(remote) = project_id.strip_prefix("ssh:") {
            let Some((device_id, session)) = remote.split_once(':') else {
                return;
            };
            if self.shared.bridge_for_key(project_id).is_none() {
                window.push_notification("Machine bridge is down — reconnect it", cx);
                return;
            }
            let _ = device_id;
            self.bind_instance_on_socket(
                Some(session.to_string()),
                self.shared
                    .bridge_for_key(project_id)
                    .map(|bridge| bridge.local_socket),
                project_id.to_string(),
                window,
                cx,
            );
            return;
        }
        let session = (project_id != "default").then(|| project_id.to_string());
        // Unknown named instances can appear between refreshes (e.g. created
        // by the CLI): refresh once before giving up.
        if session.as_deref().is_some_and(|name| {
            !self
                .shared
                .instance_list()
                .iter()
                .any(|instance| instance.name == name)
        }) {
            self.shared.refresh_instances();
        }
        self.bind_instance(session, window, cx);
    }

    /// B4/C4: this window's snapshot row (session + frame). `None` while
    /// unbound (the picker window is not restorable).
    fn open_workspace_record(&self) -> Option<settings::OpenWorkspaceRecord> {
        let binding = self.bound_project()?;
        let window = &self.config.ui.window;
        Some(settings::OpenWorkspaceRecord {
            session: if binding.project_id == "default" {
                None
            } else {
                Some(binding.project_id.clone())
            },
            x: window.x,
            y: window.y,
            width: window.width,
            height: window.height,
        })
    }

    /// B4/C4: rebuilds the open-workspace snapshot (session + frame per live
    /// window) and persists it. Closed windows drop out naturally because the
    /// set is recomputed from live registrations. Call from a window context.
    ///
    /// Re-entrancy: this runs INSIDE this entity's update lease, so the window
    /// that owns `self` contributes its row directly (no `view.update` on
    /// self); only *other* live windows are read across the entity boundary.
    pub(crate) fn sync_open_workspaces(&self, cx: &mut App) {
        let mut records = Vec::new();
        if let Some(record) = self.open_workspace_record() {
            records.push(record);
        }
        {
            let windows = self
                .shared
                .windows
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for record in windows.iter() {
                let Some(view) = record.view.upgrade() else {
                    continue;
                };
                if view.entity_id() == self.entity_id {
                    continue;
                }
                if let Some(row) = view.update(cx, |view, _| view.open_workspace_record()) {
                    records.push(row);
                }
            }
        }
        settings::ApplicationConfig::persist_open_workspaces(records);
    }

    /// (Re)binds this window to a Herdr instance (= workspace): tears down the
    /// previous instance's per-window resources, then bootstraps client +
    /// state + events + TUI for the new one. `session == None` is the default
    /// instance. Safe to call while unbound (⌘N windows).
    pub(crate) fn bind_instance(
        &mut self,
        session: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project_key = session.clone().unwrap_or_else(|| "default".to_string());
        self.bind_instance_on_socket(session, None, project_key, window, cx);
    }

    /// Bind with an explicit socket — the B1 SSH-bridge path for
    /// remote-machine instances (`socket_override` = the forwarded unix
    /// socket). Remote instances never bootstrap a local server: the bridge
    /// must be alive.
    pub(crate) fn bind_instance_on_socket(
        &mut self,
        session: Option<String>,
        socket_override: Option<std::path::PathBuf>,
        project_key: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.teardown_binding(cx);
        // One TUI child per Herdr instance, process-wide: this window and any
        // Remote/mobile viewers share the same manager (broadcast subscribers).
        // The registry key is device-scoped for bridges so two machines can
        // host same-named sessions.
        let registry_key = match &socket_override {
            Some(socket) => format!("bridge:{}", socket.display()),
            None => session.clone().unwrap_or_else(|| "default".to_string()),
        };
        self.tui_manager = self.shared.tui_registry.get_or_create_keyed(&registry_key);
        self.binding_generation = self.binding_generation.wrapping_add(1);
        let generation = self.binding_generation;
        self.initializing = true;
        self.update_window_title(window);
        self.notify_sidebar(cx);
        cx.notify();

        let bind_session = session;
        // Local named instances live on their own socket; the default instance
        // uses standard discovery (env override / ~/.config/herdr/herdr.sock).
        // Bridge (remote-machine) instances are pinned to the forwarded socket
        // and never bootstrap a local server.
        let fresh_named_instance = socket_override.is_none() && bind_session.is_some();
        let socket = socket_override.clone().unwrap_or_else(|| {
            shardlane_host::herdr::session_socket_path_for(bind_session.as_deref())
        });
        let project_name = self.shared.display_name(bind_session.as_deref());
        self.binding = Some(ProjectBinding {
            project_id: project_key.clone(),
            project_name,
            session: bind_session.clone(),
            socket_override: socket_override.clone(),
        });
        if let Some(id) = self.window_handle.as_ref().map(|handle| handle.window_id()) {
            self.shared.set_project_window(&project_key, id);
        }
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let bootstrapped = cx
                .background_executor()
                .spawn(async move {
                    // The server is persistent: an already-running instance is
                    // adopted as-is; only a dead socket starts a new server.
                    // Bridge (remote) instances must answer through the tunnel —
                    // never start a local server on their behalf.
                    let client = match &socket_override {
                        Some(_) => HerdrClient::connect_to(&socket)?,
                        None => HerdrClient::connect_to(&socket).or_else(|_| {
                            HerdrClient::bootstrap_for_session(bind_session.as_deref())
                        })?,
                    };
                    let mut state = client.visible_state()?;
                    // A freshly created named instance has no Workspace yet: create the
                    // Project's one workspace so the hosted TUI has a surface. Per the
                    // per-Tab cwd model the workspace carries no Project-level directory
                    // — Herdr seeds each Tab's own cwd.
                    if fresh_named_instance && state.workspaces.is_empty() {
                        client.create_workspace_at(None)?;
                        state = client.visible_state()?;
                    }
                    let events = client.subscribe_events().ok();
                    Ok::<_, herdr::HerdrError>((client, state, events))
                })
                .await;
            let _ = cx.update_window(window_handle, move |_, window, cx| {
                let _ = this.update(cx, |view, cx| {
                    if view.binding_generation != generation {
                        return; // superseded by a newer bind
                    }
                    view.initializing = false;
                    match bootstrapped {
                        Ok((client, state, events)) => {
                            view.client = Some(client.clone());
                            view.status = ConnectionStatus::Connected;
                            view.state = state;
                            // F34: after the whole-domain replacement, derive the selection flags uniformly.
                            derive_selection_flags(&mut view.state);
                            if let Some(events) = events {
                                Self::start_event_subscription(client, events, generation, cx);
                            }
                            view.sync_pane_event_subscription(cx);
                            view.bootstrap_sidebar_history(cx);
                            if let Some(workspace_id) = view.state.focused_workspace_id.clone() {
                                view.load_sidebar_project_panes(workspace_id, cx);
                            }
                            view.ensure_tui_surface(window, cx, true);
                            view.notify_sidebar(cx);
                            view.notify_status_bar();
                            view.sync_open_workspaces(cx);
                            // Refresh the shared instance cache off the UI thread
                            // so the workspace switcher's running/停止 states
                            // reflect the instance this window just (re)started.
                            let shared = view.shared.clone();
                            cx.spawn(async move |_, cx| {
                                cx.background_executor()
                                    .spawn(async move { shared.refresh_instances() })
                                    .await;
                            })
                            .detach();
                        }
                        Err(error) => {
                            view.status = ConnectionStatus::Offline(error.to_string());
                            view.notify_sidebar(cx);
                            view.notify_status_bar();
                        }
                    }
                    view.update_window_title(window);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    /// Drops every per-binding resource. The default instance's TUI child is left
    /// running (Remote viewers may be attached); a named session's child dies with
    /// this window's manager Arc on the next bind.
    fn teardown_binding(&mut self, cx: &mut Context<Self>) {
        if let Some(binding) = self.binding.take() {
            if let Some(id) = self.window_handle.as_ref().map(|handle| handle.window_id()) {
                self.shared.clear_project_window(&binding.project_id, id);
            }
        }
        self.binding_generation = self.binding_generation.wrapping_add(1);
        self.detach_hosted_tui(cx);
        self.client = None;
        self.state = HerdrState::default();
        self.git_status = None;
        self.workspace_tab_selection_memory.clear();
        self.right_panel_projects.clear();
        self.right_panel_runtime_id = None;
        self.events_reconnect_attempts = 0;
        self.pane_subscription_retry_attempts = 0;
        self.pane_event_subscription_ids.clear();
        // Replacing the handshake job with a fresh no-op cancels any in-flight
        // pane subscription connect from the previous binding.
        self._pane_event_subscription_script = BackgroundJob::ready(());
        self._pane_event_subscription_handshake = BackgroundJob::ready(());
        self.clear_ime_state();
    }

    /// Native window title = the bound Project name (drives the native tab
    /// strip, Mission Control, and the Window menu's window list).
    pub(crate) fn update_window_title(&mut self, window: &mut Window) {
        let title = match self.binding.as_ref() {
            Some(binding) => binding.project_name.clone(),
            None => "New Window".to_string(),
        };
        window.set_window_title(&title);
    }

    /// The Project picker page (see `project_picker_page_impl`).
    pub(crate) fn project_picker_page(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        crate::shell_render::project_picker_page_impl(self, window, cx)
    }

    /// Machine panel: adds an SSH machine and brings up its herdr socket
    /// bridge — `ssh -N -L <local-unix-sock>:<remote-herdr-sock> <target>` —
    /// then probes the forwarded socket. Requires non-interactive SSH
    /// (ssh-copy-id / Tailscale). The bridge process is detached and owned by
    /// the app; instances hosted on that machine become bindable afterwards.
    pub(crate) fn add_ssh_machine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.machine_ssh_input.clone() else {
            return;
        };
        let target = input.read(cx).value().trim().to_string();
        self.start_ssh_machine_connect(target, Some(input), window, cx);
    }

    /// Quick-connect from the workspace switcher's inline SSH row uses the
    /// popover's own keyed input (see sidebar::shell); the Machines settings
    /// page uses `add_ssh_machine` below.
    fn start_ssh_machine_connect(
        &mut self,
        target: String,
        input: Option<Entity<InputState>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if target.is_empty() {
            return;
        }
        if self
            .config
            .devices
            .iter()
            .any(|device| device.ssh_target.as_deref() == Some(target.as_str()))
        {
            window.push_notification("Machine already added", cx);
            return;
        }
        let device_id = format!("ssh-{}", uuid::Uuid::new_v4().simple());
        let device = settings::DeviceEntry {
            id: device_id.clone(),
            name: target.clone(),
            ssh_target: Some(target.clone()),
        };
        let mut devices = self.config.devices.clone();
        devices.push(device.clone());
        settings::ApplicationConfig::persist_machines(devices.clone());
        self.config.devices = devices;
        if let Some(input) = input.as_ref() {
            input.update(cx, |state, cx| {
                state.set_value("", window, cx);
            });
        }
        window.push_notification(format!("Connecting to {target}…"), cx);

        // Bridge bring-up (background): resolve remote HOME → spawn the unix
        // socket forward → probe with a herdr ping.
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { ssh_bridge::bring_up(&device_id, &target) })
                .await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |view, cx| {
                    match outcome {
                        Ok(bridge) => {
                            view.shared.register_bridge(bridge);
                            window.push_notification("Machine connected", cx);
                        }
                        Err(error) => {
                            window.push_notification(format!("SSH connect failed: {error}"), cx);
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    /// Dismissing the picker: an unbound window has nothing to show — close it.
    pub(crate) fn close_project_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.show_project_picker {
            self.show_project_picker = false;
        }
        if self.binding.is_none() {
            window.remove_window();
            return;
        }
        cx.notify();
    }

    /// ⌘N: a new window starts unbound and shows the Project picker. Picking a
    /// Project that already has a window jumps there instead (the unbound
    /// window then simply closes).
    fn new_window(&mut self, _: &NewWindow, window: &mut Window, cx: &mut Context<Self>) {
        let shared = self.shared.clone();
        let config = self.config.clone();
        let bounds = cascaded_window_bounds(window.bounds());
        let _ = open_shell_window(cx, shared, config, bounds, None);
    }

    /// Window → Merge All Windows: native macOS window tabbing via gpui's
    /// `mergeAllWindows:` support (vendored gpui implements the selector).
    fn merge_all_windows(
        &mut self,
        _: &MergeAllWindows,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if let Some(handle) = self.window_handle {
            let _ = handle.update(_cx, |_, window, _| window.merge_all_windows());
        }
    }

    fn quit(&mut self, _: &Quit, _window: &mut Window, cx: &mut Context<Self>) {
        cx.quit();
    }

    fn refresh(&mut self, _: &Refresh, window: &mut Window, cx: &mut Context<Self>) {
        if self.navigation_loading {
            window.push_notification("Refresh already in progress", cx);
            return;
        }
        self.navigation_token = self.navigation_token.wrapping_add(1);
        let token = self.navigation_token;
        self.navigation_loading = true;
        let existing_client = self.client.clone();
        let reconnect_events = !self.status.is_connected();
        // Refresh re-adopts THIS window's bound instance (never the default one blindly).
        let generation = self.binding_generation;
        let refresh_session = self
            .binding
            .as_ref()
            .and_then(|binding| binding.session.clone());
        let window_handle = window.window_handle();
        window.push_notification("Refreshing Shardlane…", cx);
        self.notify_sidebar(cx);
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut reconnected = false;
                    let client = match existing_client {
                        Some(client) if client.ping().is_ok() => client,
                        _ => {
                            reconnected = true;
                            HerdrClient::bootstrap_for_session(refresh_session.as_deref())?
                        }
                    };
                    let state = client.visible_state()?;
                    let events = if reconnect_events || reconnected {
                        client.subscribe_events().ok()
                    } else {
                        None
                    };
                    Ok::<_, herdr::HerdrError>((client, state, events, reconnected))
                })
                .await;

            let mut should_attach = false;
            let mut message = "Refreshed".to_string();
            let _ = this.update(cx, |view, cx| {
                if view.navigation_token != token || view.binding_generation != generation {
                    return;
                }
                view.navigation_loading = false;
                match result {
                    Ok((client, state, events, reconnected)) => {
                        view.client = Some(client.clone());
                        view.state = state;
                        // F34: after the whole-domain replacement, derive the selection flags uniformly.
                        derive_selection_flags(&mut view.state);
                        view.prune_steering_drafts();
                        view.status = ConnectionStatus::Connected;
                        view.prune_stale_pinned_tabs(cx);
                        view.notify_sidebar(cx);
                        if let Some(events) = events {
                            Self::start_event_subscription(client, events, generation, cx);
                        }
                        if reconnected || reconnect_events {
                            // After the global event stream is rebuilt, the pane-scoped subscription must be rebuilt
                            // too: the old subscription's socket died with the disconnect while the pane set may
                            // have been restored as-is, and a "no change" diff would leave the pane event stream dead.
                            view.sync_pane_event_subscription(cx);
                        }
                        if !view.history.loading {
                            view.refresh_history(false, cx);
                        }
                        should_attach = true;
                        if reconnected {
                            message = "Reconnected to Herdr".to_string();
                        }
                    }
                    Err(err) => {
                        view.status = ConnectionStatus::Offline(err.to_string());
                        view.notify_sidebar(cx);
                        message = format!("Refresh failed: {err}");
                    }
                }
                cx.notify();
            });
            let _ = cx.update_window(window_handle, |_, window, cx| {
                window.push_notification(message.clone(), cx);
                if should_attach {
                    let _ = this.update(cx, |view, cx| view.attach_focused_terminal(window, cx));
                }
            });
        })
        .detach();
    }

    fn content_surface_theme(&self, window: &Window) -> ContentSurfaceTheme {
        let app_theme = self.theme(window);
        // The content/Terminal surface follows the colors actually resolved by the hosted Herdr
        // TUI. Before the first Herdr frame arrives, fall back to Shardlane's UI theme only for
        // bootstrap paint; no named Shardlane Terminal palette participates at runtime.
        let background_rgb = self.hosted_terminal_background_rgb_from_theme(app_theme);
        let foreground_rgb = self
            .terminal_frame
            .default_foreground
            .unwrap_or(app_theme.text);
        let background: gpui::Hsla = rgb(background_rgb).into();
        let foreground: gpui::Hsla = rgb(foreground_rgb).into();
        let is_dark = terminal_background_is_dark(background_rgb);
        ContentSurfaceTheme {
            background,
            foreground,
            muted: foreground.opacity(0.62),
            // This is intentionally much softer than the normal app border. The content Header
            // should read as one Terminal surface, not as a separate toolbar stacked above it.
            border: foreground.opacity(0.035),
            hover: foreground.opacity(0.07),
            active: foreground.opacity(0.12),
            // These are Shardlane application semantics, not Terminal palette entries.
            primary: rgb(if is_dark { 0x0a84ff } else { 0x0064d2 }).into(),
            danger: rgb(if is_dark { 0xff453a } else { 0xd70015 }).into(),
            success: rgb(if is_dark { 0x30d158 } else { 0x248a3d }).into(),
            is_dark,
        }
    }
}

impl Render for ShardlaneApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = Instant::now();
        self.render_seq = self.render_seq.wrapping_add(1);
        let seq = self.render_seq;
        let theme = self.theme(window);
        if self.applied_ui_theme != Some(theme) {
            theme::sync_gpui_component_theme(theme, cx);
            self.applied_ui_theme = Some(theme);
            self.notify_sidebar(cx);
        }
        // Multi-instance: an unbound window (⌘N) — or any window with the picker
        // open — renders only the Project picker; the full shell below assumes a
        // bound Herdr instance.
        if self.binding.is_none() || self.show_project_picker {
            return self.project_picker_page(window, cx);
        }
        self.sidebar_auto_collapsed =
            sidebar_should_auto_collapse(window.bounds().size.width.to_f64());
        // settle_panel_slides semantics: advance both sides' slides before reading widths — terminal
        // geometry, dividers, and header segments all use this frame's rendered width (measured every
        // frame mid-slide, the same path as dragging, delivering on coalescing).
        let sidebar_target = if self.shell_sidebar_visible() {
            self.shell_sidebar_width
        } else {
            0.0
        };
        self.sidebar_rendered_width = slide_width(&mut self.sidebar_slide, sidebar_target);
        let right_panel_target = if self.right_panel.open {
            self.right_panel.width as f64
        } else {
            0.0
        };
        self.right_panel_rendered_width =
            slide_width(&mut self.right_panel_slide, right_panel_target);
        if self.terminal_fade_start.is_some() && self.terminal_fade_opacity() >= 1.0 {
            self.terminal_fade_start = None;
        }
        if self.sidebar_slide.is_some()
            || self.right_panel_slide.is_some()
            || self.terminal_fade_start.is_some()
        {
            window.request_animation_frame();
        }
        self.sync_terminal_geometry(window, cx);
        // Vsync-aligned frame extraction (measured 2026-08-28): the free-cadence poll's presentation phase
        // aliased with the display refresh, giving host TUI scrolling a ~33ms stair-step; after moving
        // extraction into the render frame, new content lands on screen in the same frame, locking the
        // presentation beat to the display refresh (same model as Ghostty). The min-interval guard is
        // unchanged, still falling back to the 100ms idle cadence.
        if self.terminal_target.as_deref() == Some(herdr_tui::TUI_TARGET)
            && !self.terminal_surface_blocked()
        {
            self.maybe_refresh_terminal_frame(cx, false);
        }
        self.sync_terminal_selection(cx);
        let initializing = self.initializing;
        // The Settings page has no shared Header (the settings layout): the sidebar carries its own
        // titlebar (traffic-light clearance + drag strip); History/NewAgent keep the shared Header.
        let show_header = !self.show_settings;
        let after_state = render_started.elapsed();

        let root = view_file!("ui/ui.crepus")
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground);
        let after_tree = render_started.elapsed();
        let total_ms = after_tree.as_secs_f64() * 1000.0;
        if terminal_trace::enabled() {
            let (frame_input_id, frame_read_id) = terminal_trace::last_frame_source();
            terminal_trace::event(format_args!(
                "stage=root.render seq={seq} frame_input_id={frame_input_id} frame_read_id={frame_read_id} total_us={} state_us={} tree_us={} since_input_us={} pending_frame={} blocked={}",
                after_tree.as_micros(),
                after_state.as_micros(),
                (after_tree - after_state).as_micros(),
                self.last_terminal_input_at
                    .map(|at| at.elapsed().as_micros())
                    .unwrap_or(u128::MAX),
                self.terminal_pending_frame,
                self.terminal_surface_blocked(),
            ));
        }
        if seq <= 3 || total_ms > 2.0 {
            lag_log(format_args!(
                "root render seq={seq} {total_ms:.2}ms state={:.2}ms tree={:.2}ms workspaces={} tabs={} panes={} agents={} term_lines={}",
                after_state.as_secs_f64() * 1000.0,
                (after_tree - after_state).as_secs_f64() * 1000.0,
                self.state.workspaces.len(),
                self.state.tabs.len(),
                self.state.panes.len(),
                self.state.agents.len(),
                self.terminal_frame.lines.len(),
            ));
        }

        // Lazily create the Settings sidebar search field (InputState needs a window; render ensures every frame).
        if self.settings_sidebar_search.is_none() {
            self.settings_sidebar_search =
                Some(cx.new(|cx| InputState::new(window, cx).placeholder("Search Settings")));
        }
        // Mobile port input is lazily created in the same pattern (defined in mobile_view.rs).
        self.ensure_mobile_port_input(window, cx);
        // UX fix (2026-08-27 review): the root context must always be "ShardlaneApp".
        // It was once switched wholesale to "HerdrTui" in TUI mode, silently killing every registered
        // shortcut with scope=App (cmd-k/cmd-n/cmd-w/cmd-shift-x/...) in TUI mode — clicks worked,
        // shortcuts all died. The "HerdrTui"-specific bindings moved onto the TUI surface's child
        // element context (see tui_surface_view's .key_context), keeping the ancestor-stack matching semantics.
        root.key_context("ShardlaneApp")
            .track_focus(&self.focus_handle)
            // Sidebar resize / section height move/up listeners mount at root level: the #sidebar-divider
            // overlay and client_shell are sibling subtrees, and only root listeners can cover both.
            .on_mouse_move(cx.listener(Self::shell_sidebar_drag_move))
            .on_mouse_move(cx.listener(Self::right_panel_drag_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::shell_sidebar_drag_end))
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
            .children(self.render_agent_switcher(window, cx))
            .on_action(cx.listener(Self::toggle_help))
            .on_action(cx.listener(Self::toggle_settings))
            .on_action(cx.listener(Self::toggle_history))
            .on_action(cx.listener(Self::open_search))
            .on_action(cx.listener(Self::toggle_conversation_find))
            .on_action(cx.listener(Self::open_about))
            .on_action(cx.listener(Self::refresh))
            .on_action(cx.listener(Self::split_right))
            .on_action(cx.listener(Self::split_down))
            .on_action(cx.listener(Self::toggle_pane_zoom))
            .on_action(cx.listener(Self::focus_left))
            .on_action(cx.listener(Self::focus_right))
            .on_action(cx.listener(Self::focus_up))
            .on_action(cx.listener(Self::focus_down))
            .on_action(cx.listener(Self::resize_left))
            .on_action(cx.listener(Self::resize_right))
            .on_action(cx.listener(Self::resize_up))
            .on_action(cx.listener(Self::resize_down))
            .on_action(cx.listener(Self::close_pane))
            .on_action(cx.listener(Self::previous_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::open_new_agent))
            .on_action(cx.listener(Self::rename_active_tab))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::new_project))
            .on_action(cx.listener(Self::new_window))
            .on_action(cx.listener(Self::merge_all_windows))
            .on_action(cx.listener(Self::new_script))
            .on_action(cx.listener(Self::rename_active_project))
            .on_action(cx.listener(Self::close_project))
            .on_action(cx.listener(Self::previous_project))
            .on_action(cx.listener(Self::next_project))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_agents))
            .on_action(cx.listener(Self::toggle_services))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::increase_terminal_font_size))
            .on_action(cx.listener(Self::decrease_terminal_font_size))
            .on_action(cx.listener(Self::reset_terminal_font_size))
            .on_action(cx.listener(Self::theme_catppuccin))
            .on_action(cx.listener(Self::theme_catppuccin_latte))
            .on_action(cx.listener(Self::theme_tokyo_night))
            .on_action(cx.listener(Self::theme_tokyo_night_day))
            .on_action(cx.listener(Self::theme_dracula))
            .on_action(cx.listener(Self::theme_nord))
            .on_action(cx.listener(Self::theme_gruvbox))
            .on_action(cx.listener(Self::theme_gruvbox_light))
            .on_action(cx.listener(Self::theme_one_dark))
            .on_action(cx.listener(Self::theme_one_light))
            .on_action(cx.listener(Self::theme_solarized))
            .on_action(cx.listener(Self::theme_solarized_light))
            .on_action(cx.listener(Self::theme_kanagawa))
            .on_action(cx.listener(Self::theme_kanagawa_lotus))
            .on_action(cx.listener(Self::theme_rose_pine))
            .on_action(cx.listener(Self::theme_rose_pine_dawn))
            .on_action(cx.listener(Self::theme_vesper))
            .on_action(cx.listener(Self::reload_herdr_config))
            .on_action(cx.listener(Self::toggle_always_on_top))
            .on_action(cx.listener(Self::toggle_right_panel_action))
            .on_action(cx.listener(Self::open_lazygit_action))
            .on_action(cx.listener(Self::picker_accept_completion))
            .on_action(cx.listener(Self::navigate_back))
            .on_action(cx.listener(Self::navigate_forward))
            .on_action(cx.listener(Self::switch_agent_next))
            .on_action(cx.listener(Self::switch_agent_prev))
            .on_action(cx.listener(Self::confirm_agent_switch))
            .on_action(cx.listener(Self::cancel_agent_switch))
            .on_modifiers_changed(cx.listener(Self::agent_switcher_modifiers_changed))
            .on_action(cx.listener(Self::quit))
            .into_any_element()
    }
}

/// Scroll presentation diagnostics (plan: smoothness quantification). When SHARDLANE_SCROLL_DEBUG=1, lag_log
/// records wheel send and frame present times to compute wheel→present latency and jitter distribution.
pub(crate) fn scroll_debug_enabled() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("SHARDLANE_SCROLL_DEBUG").is_ok())
}

fn consume_terminal_scroll_rows(
    residual_px: &mut f64,
    delta_px: f64,
    cell_height: f64,
    gesture_ended: bool,
) -> isize {
    let cell_height = cell_height.max(1.0);
    if delta_px.abs() < f64::EPSILON {
        if gesture_ended {
            *residual_px = 0.0;
        }
        return 0;
    }

    if residual_px.abs() > f64::EPSILON && residual_px.signum() != delta_px.signum() {
        *residual_px = 0.0;
    }
    *residual_px += delta_px;

    // Terminal viewports are row-addressed. Preserve sub-row trackpad distance as a residual,
    // and only emit newly crossed rows. Never resend the cumulative gesture distance.
    let rows = (*residual_px / cell_height).trunc() as isize;
    *residual_px -= rows as f64 * cell_height;
    if gesture_ended {
        *residual_px = 0.0;
    }
    rows
}

fn normalize_cell_selection(start: (u16, u16), end: (u16, u16)) -> ((u16, u16), (u16, u16)) {
    if start.1 < end.1 || (start.1 == end.1 && start.0 <= end.0) {
        (start, end)
    } else {
        (end, start)
    }
}

fn responsive_dialog_width(
    window_width: f64,
    fraction: f64,
    min_width: f64,
    max_width: f64,
) -> f32 {
    (window_width * fraction).clamp(min_width, max_width) as f32
}

fn theme_preset_card(
    ix: usize,
    preset: theme::ThemePreset,
    active: bool,
    content_theme: ContentSurfaceTheme,
    herdr: Entity<ShardlaneApp>,
) -> AnyElement {
    let preview_dark = preset.category == theme::ThemePresetCategory::Dark;
    let preview = theme::theme_for_scheme(preset.scheme, preview_dark);
    // App-theme preview only. Terminal colors are selected separately from Herdr's official
    // theme list and must not be previewed through Shardlane's retired Terminal palettes.
    let bars = [
        (0.94, preview.active),
        (0.72, preview.hover),
        (0.56, preview.border),
        (0.36, preview.label),
    ];
    let preview_background = rgb(preview.bg);
    let preview_foreground = rgb(preview.text);
    let preview_panel = rgb(preview.panel);
    let click_herdr = herdr.clone();

    div()
        .id(("settings-theme-preset", ix))
        .w_full()
        .min_h(px(104.0))
        .p_3()
        .rounded(px(9.0))
        .border_1()
        .border_color(if active {
            content_theme.primary.opacity(0.95)
        } else {
            content_theme.border
        })
        .bg(preview_background)
        .text_color(preview_foreground)
        .cursor_pointer()
        .overflow_hidden()
        .hover(move |style| style.border_color(content_theme.primary.opacity(0.55)))
        .active(|style| style.opacity(0.86))
        .shardlane_interactive(content_theme.primary.opacity(0.92), move |window, app| {
            click_herdr.update(app, |this, cx| {
                this.apply_theme_card_selection(preset, window, cx);
            });
        })
        .child(
            v_flex()
                .w_full()
                .min_w_0()
                .gap_3()
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .items_start()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .flex_1()
                                .whitespace_normal()
                                .text_size(theme::FONT_BODY)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(preview_foreground)
                                .child(preset.label),
                        )
                        .when(active, |row| {
                            row.child(
                                div()
                                    .flex_none()
                                    .size(px(20.0))
                                    .rounded(px(10.0))
                                    .bg(preview_panel)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(preview_foreground)
                                    .child(Icon::new(ComponentIconName::CircleCheck).xsmall()),
                            )
                        }),
                )
                .child(
                    v_flex()
                        .w_full()
                        .gap(px(5.0))
                        .children(bars.into_iter().map(|(width, color)| {
                            div()
                                .w(relative(width))
                                .h(px(7.0))
                                .rounded(px(3.5))
                                .bg(rgb(color))
                        })),
                ),
        )
        .into_any_element()
}

/// SCT-01: the sole mapping of registry id → GPUI action (runtime bindings are generated from it).
fn registry_key_binding(id: &str, chord: &str, context: Option<&str>) -> Option<KeyBinding> {
    match id {
        "app.quit" => Some(KeyBinding::new(chord, Quit, context)),
        "app.settings" => Some(KeyBinding::new(chord, ToggleSettings, context)),
        "app.search" => Some(KeyBinding::new(chord, OpenSearch, context)),
        "app.find" => Some(KeyBinding::new(chord, FindInConversation, context)),
        "app.new-window" => Some(KeyBinding::new(chord, NewWindow, context)),
        "app.new-task" => Some(KeyBinding::new(chord, OpenNewAgent, context)),
        "app.refresh" => Some(KeyBinding::new(chord, Refresh, context)),
        "app.help" => Some(KeyBinding::new(chord, ToggleHelp, context)),
        "app.reload-config" => Some(KeyBinding::new(chord, ReloadHerdrConfig, context)),
        "lazygit.open" => Some(KeyBinding::new(chord, OpenLazygit, context)),
        "sidebar.toggle" => Some(KeyBinding::new(chord, ToggleSidebar, context)),
        "sidebar.agents" => Some(KeyBinding::new(chord, ToggleAgents, context)),
        "terminal.paste" => Some(KeyBinding::new(chord, Paste, context)),
        "terminal.copy" => Some(KeyBinding::new(chord, Copy, context)),
        "terminal.select-all" => Some(KeyBinding::new(chord, SelectAll, context)),
        "terminal.font-increase" => Some(KeyBinding::new(chord, IncreaseTerminalFontSize, context)),
        "terminal.font-decrease" => Some(KeyBinding::new(chord, DecreaseTerminalFontSize, context)),
        "terminal.font-reset" => Some(KeyBinding::new(chord, ResetTerminalFontSize, context)),
        "pane.split-right" => Some(KeyBinding::new(chord, SplitRight, context)),
        "pane.split-down" => Some(KeyBinding::new(chord, SplitDown, context)),
        "pane.zoom" => Some(KeyBinding::new(chord, TogglePaneZoom, context)),
        "pane.focus-left" => Some(KeyBinding::new(chord, FocusLeft, context)),
        "pane.focus-right" => Some(KeyBinding::new(chord, FocusRight, context)),
        "pane.focus-up" => Some(KeyBinding::new(chord, FocusUp, context)),
        "pane.focus-down" => Some(KeyBinding::new(chord, FocusDown, context)),
        "pane.resize-left" => Some(KeyBinding::new(chord, ResizeLeft, context)),
        "pane.resize-right" => Some(KeyBinding::new(chord, ResizeRight, context)),
        "pane.resize-up" => Some(KeyBinding::new(chord, ResizeUp, context)),
        "pane.resize-down" => Some(KeyBinding::new(chord, ResizeDown, context)),
        "pane.close" => Some(KeyBinding::new(chord, ClosePane, context)),
        "tab.new" => Some(KeyBinding::new(chord, NewTab, context)),
        "tab.close" => Some(KeyBinding::new(chord, CloseTab, context)),
        "tab.previous" => Some(KeyBinding::new(chord, PreviousTab, context)),
        "tab.next" => Some(KeyBinding::new(chord, NextTab, context)),
        "project.previous" => Some(KeyBinding::new(chord, PreviousProject, context)),
        "project.next" => Some(KeyBinding::new(chord, NextProject, context)),
        // Audit E07: Agent Switcher chords moved from hardcoded internal bindings into the
        // registry, so they are configurable, listed in help/Settings, and visible to
        // chord_is_bound's terminal key-swallow decision.
        "agent.switcher-next" => Some(KeyBinding::new(chord, SwitchAgentNext, context)),
        "agent.switcher-prev" => Some(KeyBinding::new(chord, SwitchAgentPrev, context)),
        "agent.switcher-confirm" => Some(KeyBinding::new(chord, ConfirmAgentSwitch, context)),
        "agent.switcher-cancel" => Some(KeyBinding::new(chord, CancelAgentSwitch, context)),
        _ => None,
    }
}

/// SCT-01: generate all runtime bindings from REGISTRY + config; default chords that are
/// overridden/disabled are shadowed with NoAction (the GPUI keymap gives later entries priority). Internal bindings
/// (picker/HerdrTui/Input) are not part of the user-configurable surface and
/// are appended hardcoded.
///
/// P1-3 (audit 2026-08-27): also returns every dynamic `(chord, scope)` pair this
/// generation installs — active action chords AND NoAction masks. Runtime rebinding
/// needs the full previous generation so chords that a later generation retires can be
/// tombstoned (GPUI keymap is append-only; later bindings win).
fn dyn_shortcut_install(
    config: &settings::ApplicationConfig,
) -> (
    Vec<KeyBinding>,
    Vec<(String, crate::shortcuts::ShortcutScope)>,
) {
    let shortcut_config = &config.shortcuts;
    let mut bindings = Vec::new();
    let mut installed_pairs = Vec::new();

    for resolved in shortcuts::resolved_bindings(shortcut_config) {
        if let Some(binding) =
            registry_key_binding(resolved.id, &resolved.chord, resolved.scope.gpui_context())
        {
            installed_pairs.push((resolved.chord.clone(), resolved.scope));
            bindings.push(binding);
            // Terminal-domain actions are mirrored into the host surface's sub-context "HerdrTui" (GPUI's
            // ancestor-stack coexistence semantics). Bindings are generated from the same resolved registry —
            // user overrides / disables take effect live; there is no second hardcoded chord table (audit P1-1).
            if shortcuts::find_entry(resolved.id)
                .is_some_and(|entry| entry.category == shortcuts::ShortcutCategory::Terminal)
            {
                if let Some(binding) =
                    registry_key_binding(resolved.id, &resolved.chord, Some("HerdrTui"))
                {
                    bindings.push(binding);
                }
            }
        }
    }
    for (_id, chord, scope) in shortcuts::shadowed_default_chords(shortcut_config) {
        installed_pairs.push((chord.clone(), scope));
        bindings.push(KeyBinding::new(
            chord.as_str(),
            gpui::NoAction,
            scope.gpui_context(),
        ));
    }

    // --- Internal bindings (not user-configurable) ---
    // Audit E07: the Agent Switcher chords (previously here) now come from the registry above —
    // removing this duplicate kept them from double-installing and unregisters them from the
    // hardcoded surface.
    bindings.extend([
        // Picker
        KeyBinding::new("up", PickerSelectUp, Some("ClientPicker")),
        KeyBinding::new("down", PickerSelectDown, Some("ClientPicker")),
        KeyBinding::new("escape", PickerCancel, Some("ClientPicker")),
        KeyBinding::new("tab", PickerAcceptCompletion, Some("ClientPicker")),
        KeyBinding::new("right", PickerAcceptCompletion, Some("ClientPicker")),
        // Input's Shift+Enter for newline
        KeyBinding::new(
            "shift-enter",
            gpui_component::input::Enter { secondary: true },
            Some("Input"),
        ),
    ]);

    (bindings, installed_pairs)
}

/// SCT-01 entry check: startup replaces hardcoded bindings with registry-driven ones.
/// (startup_self_check below)
fn startup_self_check() {
    lag_log(format_args!("=== shardlane startup self-check ==="));

    match (herdr_cli_path(), installed_cli_version()) {
        (Some(path), Some(version)) => {
            lag_log(format_args!("  herdr: {version} ({})", path.display()))
        }
        (Some(path), None) => lag_log(format_args!(
            "  herdr: found at {} (--version failed)",
            path.display()
        )),
        (None, _) => lag_log(format_args!("  herdr: NOT FOUND")),
    }

    let ghostty_lib = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("vendor/ghostty-vt/lib/libghostty-vt.a");
    lag_log(format_args!(
        "  libghostty-vt: {}",
        if ghostty_lib.exists() {
            "present"
        } else {
            "MISSING"
        }
    ));

    // Audit A32: no hardcoded version claim — the number silently lied after dependency
    // upgrades; the Cargo.lock pins the actual gpui-component version.
    lag_log(format_args!("  gpui-component: initialized"));

    lag_log(format_args!("=== self-check complete ==="));
}

/// W4: resolve the Mobile Web bundle path. In a packaged `.app`, the bundle is at
/// `Shardlane.app/Contents/Resources/mobile-web/`; during development it falls back
/// to `$SHARDLANE_WEB_BUNDLE` env, then to a sibling `herdr-mobile/dist` export
/// (`pnpm export:web` output), so the dev Remote server serves the Mobile Web SPA
/// without extra configuration. Returns None when no candidate exists.
fn web_bundle_path() -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var("SHARDLANE_WEB_BUNDLE") {
        let p = std::path::PathBuf::from(path);
        if p.is_dir() {
            return Some(p);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let exe = std::env::current_exe().ok();
        if let Some(resources) = exe
            .as_deref()
            .and_then(|exe| exe.parent())
            .and_then(|macos| macos.parent())
            .map(|contents| contents.join("Resources").join("mobile-web"))
            .filter(|resources| resources.is_dir())
        {
            return Some(resources);
        }
    }
    // Development fallback: `crates/herdr-gui` → workspace root → sibling export.
    // Requiring `index.html` keeps a half-finished export from being mounted.
    let dev_bundle =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../herdr-mobile/dist");
    if dev_bundle.join("index.html").is_file() {
        return Some(dev_bundle);
    }
    None
}

fn main() {
    std::env::set_var("OS_ACTIVITY_MODE", "disable");

    let app = Application::new().with_assets(assets::Assets);
    app.run(|cx: &mut App| {
        gpui_component::init(cx);
        startup_self_check();

        cx.set_menus(vec![
            Menu {
                name: "Shardlane".into(),
                items: vec![
                    MenuItem::action("About Shardlane…", OpenAbout),
                    MenuItem::separator(),
                    MenuItem::os_submenu("Services", SystemMenuType::Services),
                    MenuItem::separator(),
                    MenuItem::action("Settings…", ToggleSettings),
                    MenuItem::action("New Task", OpenNewAgent),
                    MenuItem::action("History", OpenHistory),
                    MenuItem::action("Go to…", OpenSearch),
                    MenuItem::separator(),
                    MenuItem::action("Reload Config", ReloadHerdrConfig),
                    MenuItem::action("Refresh", Refresh),
                    MenuItem::separator(),
                    MenuItem::action("Quit Shardlane", Quit),
                ],
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::separator(),
                    MenuItem::action("Copy", Copy),
                    MenuItem::action("Paste", Paste),
                    MenuItem::action("Select All", SelectAll),
                ],
            },
            Menu {
                name: "Terminal".into(),
                items: vec![
                    MenuItem::action("New Tab", NewTab),
                    MenuItem::action("Rename Tab…", RenameTab),
                    MenuItem::action("Close Tab", CloseTab),
                    MenuItem::action("Previous Tab", PreviousTab),
                    MenuItem::action("Next Tab", NextTab),
                    MenuItem::separator(),
                    MenuItem::action("Split Right", SplitRight),
                    MenuItem::action("Split Down", SplitDown),
                    MenuItem::action("Toggle Pane Zoom", TogglePaneZoom),
                    MenuItem::separator(),
                    MenuItem::action("Focus Pane Left", FocusLeft),
                    MenuItem::action("Focus Pane Right", FocusRight),
                    MenuItem::action("Focus Pane Up", FocusUp),
                    MenuItem::action("Focus Pane Down", FocusDown),
                    MenuItem::separator(),
                    MenuItem::action("Resize Pane Left", ResizeLeft),
                    MenuItem::action("Resize Pane Right", ResizeRight),
                    MenuItem::action("Resize Pane Up", ResizeUp),
                    MenuItem::action("Resize Pane Down", ResizeDown),
                    MenuItem::separator(),
                    MenuItem::action("Increase Font Size", IncreaseTerminalFontSize),
                    MenuItem::action("Decrease Font Size", DecreaseTerminalFontSize),
                    MenuItem::action("Reset Font Size", ResetTerminalFontSize),
                    MenuItem::separator(),
                    MenuItem::action("Close Pane", ClosePane),
                ],
            },
            Menu {
                name: "Window".into(),
                items: vec![
                    MenuItem::action("New Window", NewWindow),
                    MenuItem::action("Merge All Windows", MergeAllWindows),
                    MenuItem::separator(),
                    MenuItem::action("New Project", NewProject),
                    MenuItem::action("New Script…", NewScript),
                    MenuItem::action("Rename Project…", RenameProject),
                    MenuItem::action("Close Project", CloseProject),
                    MenuItem::separator(),
                    MenuItem::action("Previous Project", PreviousProject),
                    MenuItem::action("Next Project", NextProject),
                    MenuItem::separator(),
                    MenuItem::action("Always on Top", ToggleAlwaysOnTop),
                ],
            },
            Menu {
                name: "View".into(),
                items: vec![
                    MenuItem::action("Toggle Sidebar", ToggleSidebar),
                    MenuItem::action("Toggle Right Panel", ToggleRightPanel),
                    MenuItem::action("Toggle Agents", ToggleAgents),
                    MenuItem::action("Toggle Services", ToggleServices),
                    MenuItem::action("Toggle Help", ToggleHelp),
                    MenuItem::separator(),
                    MenuItem::submenu(Menu {
                        name: "Themes".into(),
                        items: vec![
                            MenuItem::action("Catppuccin", ThemeCatppuccin),
                            MenuItem::action("Catppuccin Latte", ThemeCatppuccinLatte),
                            MenuItem::action("Tokyo Night", ThemeTokyoNight),
                            MenuItem::action("Tokyo Night Day", ThemeTokyoNightDay),
                            MenuItem::action("Dracula", ThemeDracula),
                            MenuItem::action("Nord", ThemeNord),
                            MenuItem::action("Gruvbox", ThemeGruvbox),
                            MenuItem::action("Gruvbox Light", ThemeGruvboxLight),
                            MenuItem::action("One Dark", ThemeOneDark),
                            MenuItem::action("One Light", ThemeOneLight),
                            MenuItem::action("Solarized", ThemeSolarized),
                            MenuItem::action("Solarized Light", ThemeSolarizedLight),
                            MenuItem::action("Kanagawa", ThemeKanagawa),
                            MenuItem::action("Kanagawa Lotus", ThemeKanagawaLotus),
                            MenuItem::action("Rose Pine", ThemeRosePine),
                            MenuItem::action("Rose Pine Dawn", ThemeRosePineDawn),
                            MenuItem::action("Vesper", ThemeVesper),
                        ],
                    }),
                ],
            },
        ]);

        // Audit A31: one disk read + parse per launch — this load feeds the keybindings, the
        // window-restore frame, the Project registry, and ShardlaneApp's per-window bootstrap.
        let startup_config = settings::ApplicationConfig::load();
        // The i18n locale is process-global and read at render time; applying it
        // before the first window renders means every surface starts localized
        // (gpui-component's own strings share the same global).
        i18n::apply_language(startup_config.ui.language);
        let (startup_bindings, _startup_pairs) = dyn_shortcut_install(&startup_config);
        cx.bind_keys(startup_bindings);

        // Process-wide services: one status-bar item, one notification action
        // channel, one per-instance TUI manager registry, one display-name map.
        let (shared, status_bar_rx, notification_rx) =
            ShellSharedRuntime::new(startup_config.instance_display_names.clone());
        spawn_global_action_consumer(
            cx,
            shared.clone(),
            status_bar_rx,
            ShardlaneApp::handle_status_bar_action,
            status_bar_targets_view,
        );
        spawn_global_action_consumer(
            cx,
            shared.clone(),
            notification_rx,
            ShardlaneApp::handle_notification_action,
            notification_targets_view,
        );

        // A1: reap unwatched per-instance TUI children. `reap_idle` only stops
        // a child with ZERO broadcast subscribers that has also been idle for
        // its lease, so a bound window's surface (and any remote viewer's
        // stream) always keeps its child alive. Reopening an instance re-spawns
        // lazily via `open()`'s idempotent slot.
        {
            let reaper_registry = shared.tui_registry.clone();
            cx.spawn(async move |cx| loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(30))
                    .await;
                reaper_registry.reap_idle_all();
            })
            .detach();
        }

        // Workspace switcher freshness: poll `herdr session list` on the
        // background executor so running/停止 dots stay current without any
        // UI-thread CLI round trip.
        {
            let refresh_shared = shared.clone();
            cx.spawn(async move |cx| loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(30))
                    .await;
                let shared = refresh_shared.clone();
                cx.background_executor()
                    .spawn(async move { shared.refresh_instances() })
                    .await;
            })
            .detach();
        }

        // App-level keystroke routing: GPUI hands the observer the window that
        // received the key, and the registry maps it to THAT window's entity —
        // with multiple windows, window 1 must never swallow window 2's keys.
        {
            let shared = shared.clone();
            cx.observe_keystrokes(move |event, window, cx| {
                let Some(view) = shared.window_view(window.window_handle().window_id()) else {
                    return;
                };
                view.update(cx, |view, view_cx| {
                    view.handle_keystroke(&event.keystroke, window, view_cx);
                });
            })
            .detach();
        }

        // Prefer restoring the last window frame at startup; fall back to built-in defaults if never recorded. Minimum size is unchanged.
        let startup_window = startup_config.ui.window.clone();
        let initial_bounds = if startup_window.has_bounds() {
            bounds(
                point(px(startup_window.x as f32), px(startup_window.y as f32)),
                size(
                    px(startup_window.width as f32),
                    px(startup_window.height as f32),
                ),
            )
        } else {
            bounds(point(px(80.0), px(80.0)), size(px(1280.0), px(820.0)))
        };
        // The startup window binds the default Herdr instance (the user's
        // existing default session, whatever workspaces it holds).
        let initial_session = Some("default".to_string());
        let _ = open_shell_window(
            cx,
            shared.clone(),
            startup_config.clone(),
            initial_bounds,
            initial_session,
        );
        // B4: restore the remaining workspace windows (one per instance) with
        // their last frames. Unknown/dead sessions fail their bind softly and
        // fall back to the picker.
        for record in &startup_config.open_workspaces {
            let Some(session) = record.session.clone() else {
                continue; // the default instance's window is already open
            };
            if session == "default" || session.starts_with("ssh:") {
                continue; // default already open; remote windows need their bridge first
            }
            let restored_bounds = bounds(
                point(px(record.x as f32), px(record.y as f32)),
                size(px(record.width as f32), px(record.height as f32)),
            );
            let _ = open_shell_window(
                cx,
                shared.clone(),
                startup_config.clone(),
                restored_bounds,
                Some(session),
            );
        }
        cx.activate(true);
    });
}

/// Opens one shell window and performs its per-window wiring (window
/// registration, native window preferences, focus, activation/bounds
/// observers). `initial_session` binds the window immediately (`Some("default")`
/// on startup binds the default Herdr instance); `None` leaves it unbound on
/// the workspace picker (⌘N).
fn open_shell_window(
    cx: &mut App,
    shared: std::sync::Arc<ShellSharedRuntime>,
    config: settings::ApplicationConfig,
    window_bounds: Bounds<Pixels>,
    initial_session: Option<String>,
) -> gpui::Result<AnyWindowHandle> {
    let mut options = gpui_window_options(
        "dev.shardlane.app",
        "",
        Some(WindowBounds::Windowed(window_bounds)),
        Some(size(px(360.0), px(420.0))),
    );
    let mut titlebar = TitleBar::title_bar_options();
    titlebar.traffic_light_position = Some(point(px(20.0), px(11.0)));
    options.titlebar = Some(titlebar);
    options.window_background = WindowBackgroundAppearance::Transparent;
    // Native window tabbing (vendored gpui implements tabbingIdentifier +
    // mergeAllWindows:): the user's macOS tabbing preference applies to shell
    // windows, and Window → Merge All Windows merges them into native tabs.
    options.tabbing_identifier = Some("dev.shardlane.window".to_string());
    let window = cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| ShardlaneApp::with_config(config, shared.clone(), cx));
        cx.new(|cx| Root::new(view, window, cx))
    })?;
    let handle = AnyWindowHandle::from(window);
    if let Err(error) = window.update(cx, |root, window, cx| {
        let view = root.view().clone().downcast::<ShardlaneApp>().ok();
        if let Some(view) = view {
            view.update(cx, |view, view_cx| {
                view.window_handle = Some(window.window_handle());
                view.shared
                    .register_window(window.window_handle(), view_cx.entity().downgrade());
                if let Err(error) = macos_window::apply(
                    window,
                    view.window_opacity(),
                    view.config.ui.window.always_on_top,
                    view.native_forced_dark(),
                ) {
                    lag_log(format_args!("window.native_preferences error={error}"));
                }
                window.focus(&view.focus_handle);
                view_cx
                    .observe_window_activation(window, |view, window, cx| {
                        view.window_active = window.is_window_active();
                        // Diagnostic gated like its siblings (audit A03): an unconditional
                        // log per activation flip spams /tmp/shardlane-lag.log in normal use.
                        if terminal_trace::enabled() {
                            lag_log(format_args!(
                                "tui.activation event active={} target={:?}",
                                view.window_active, view.terminal_target,
                            ));
                        }
                        view.sync_terminal_application_focus(cx);
                        if view.window_active {
                            // Each window owns its Herdr instance, so activation only
                            // re-adopts this window's own grid width.
                            view.restore_tui_grid_on_activation(cx);
                            // F26: retry attach when the user returns and the backoff has expired.
                            if view.terminal_target.is_none()
                                && view.terminal_attach_target.is_none()
                            {
                                view.attach_focused_terminal(window, cx);
                            }
                        }
                        // Window frame changes happen frequently during resize/drag; persist once on deactivation only.
                        if !view.window_active && view.window_bounds_dirty {
                            view.save_config();
                            view.window_bounds_dirty = false;
                        }
                        if !view.window_active {
                            view.sync_open_workspaces(cx);
                        }
                    })
                    .detach();
                let mut display_id = window.display(view_cx).map(|display| display.id());
                view_cx
                    .observe_window_bounds(window, move |view, window, cx| {
                        let next_display_id = window.display(cx).map(|display| display.id());
                        if next_display_id != display_id {
                            lag_log(format_args!(
                                "window.display_changed {:?} -> {:?} scale={:.2}",
                                display_id,
                                next_display_id,
                                window.scale_factor()
                            ));
                            display_id = next_display_id;
                        }
                        let current = window.bounds();
                        view.config.ui.window.x = f64::from(current.origin.x);
                        view.config.ui.window.y = f64::from(current.origin.y);
                        view.config.ui.window.width = f64::from(current.size.width);
                        view.config.ui.window.height = f64::from(current.size.height);
                        view.window_bounds_dirty = true;
                        view.sync_terminal_geometry(window, cx);
                    })
                    .detach();
                view.update_window_title(window);
                match initial_session {
                    Some(session) => view.bind_instance(
                        (session != "default").then_some(session),
                        window,
                        view_cx,
                    ),
                    None => {
                        view.show_project_picker = true;
                        view_cx.notify();
                    }
                }
            });
        }
    }) {
        lag_log(format_args!("window.wiring failed: {error}"));
    }
    Ok(handle)
}

/// New-window placement: cascade from the activating window so overlapping
/// windows are visibly stacked (macOS-style 28pt steps).
fn cascaded_window_bounds(base: Bounds<Pixels>) -> Bounds<Pixels> {
    const CASCADE_STEP: f32 = 28.0;
    bounds(
        point(
            base.origin.x + px(CASCADE_STEP),
            base.origin.y + px(CASCADE_STEP),
        ),
        base.size,
    )
}

/// B20: a pended copy carries the terminal generation it was made against; a poll tick
/// after a host restart/token bump must not copy the old session's coordinates into the
/// clipboard.
fn pending_copy_is_stale(copy: &PendingTerminalCopy, terminal_token: u64) -> bool {
    copy.generation != terminal_token
}

/// Copy-on-select flush: the single implementation shared by the single-terminal and multi-pane
/// polling loops, so clipboard semantics fixes no longer need editing two places. Returns whether any
/// real work happened (drives poll liveness).
fn flush_pending_terminal_copy(
    pending: &mut Option<PendingTerminalCopy>,
    terminal_token: u64,
    managed: &mut ManagedTerminal,
    cx: &mut Context<ShardlaneApp>,
) -> bool {
    let Some(copy) = pending.take() else {
        return false;
    };
    if pending_copy_is_stale(&copy, terminal_token) {
        if terminal_trace::enabled() {
            terminal_trace::event(format_args!(
                "stage=ui.copy_flush discarded=stale_generation copy_generation={} current_generation={terminal_token}",
                copy.generation,
            ));
        }
        return false;
    }
    let trace_started = terminal_trace::enabled().then(Instant::now);
    let semantic_started = terminal_trace::enabled().then(Instant::now);
    let text = managed
        .selection_text(copy.selection.0, copy.selection.1)
        .unwrap_or(copy.fallback_text);
    let semantic_us = semantic_started
        .map(terminal_trace::elapsed_us)
        .unwrap_or(0);
    let bytes = text.len();
    if !text.is_empty() {
        cx.write_to_clipboard(crepuscularity_gpui::ClipboardItem::new_string(text));
    }
    if let Some(started) = trace_started {
        terminal_trace::event(format_args!(
            "stage=ui.copy_flush bytes={bytes} semantic_us={semantic_us} total_us={}",
            terminal_trace::elapsed_us(started),
        ));
    }
    true
}

fn poll_managed_terminal(
    token: u64,
    target: String,
    wake: TerminalWakeReceiver,
    cx: &mut Context<ShardlaneApp>,
) {
    let mut last_summary = Instant::now();
    let mut drains_window: u64 = 0;
    let mut frames_window: u64 = 0;
    let mut poll_interval = Duration::from_millis(TERMINAL_POLL_ACTIVE_MS);
    cx.spawn(async move |this, cx| loop {
        let trace_wait_started = terminal_trace::enabled().then(Instant::now);
        let trigger =
            wait_for_terminal_poll(&wake, cx.background_executor().timer(poll_interval)).await;
        if let Some(started) = trace_wait_started {
            terminal_trace::event(format_args!(
                "stage=poll.wake trigger={trigger:?} wait_us={} requested_interval_us={}",
                terminal_trace::elapsed_us(started),
                poll_interval.as_micros(),
            ));
        }
        let mut disconnected = trigger == TerminalPollTrigger::Closed;
        let mut stale = false;
        let mut active = false;
        let mut render_blocked = false;
        let mut pending_bells = 0_u64;
        let mut frame_retry_after = None;
        if this
            .update(cx, |view, cx| {
                if view.terminal_token != token {
                    stale = true;
                    return;
                }
                render_blocked = view.terminal_surface_blocked();
                let recent_user_input = !render_blocked
                    && view
                        .last_terminal_input_at
                        .is_some_and(|at| at.elapsed() < Duration::from_millis(300));
                active |= recent_user_input;
                if let Some(terminal) = view.terminal.clone() {
                    match terminal.try_lock() {
                        Ok(mut managed) => {
                            // Another viewer may have resized the shared Herdr TUI
                            // session (resize applies globally). Adopt its
                            // authoritative grid: the child repaints only its own
                            // grid, so a stale larger local model would project
                            // residual glyphs into the visible pane.
                            match managed.adopt_shared_geometry(
                                view.terminal_cell_width(),
                                view.terminal_cell_height(),
                            ) {
                                Ok(Some((frame, cols, rows))) => {
                                    lag_log(format_args!(
                                        "tui.shared_geometry adopted cols={cols} rows={rows}"
                                    ));
                                    view.tui_adopted_grid = Some((cols, rows));
                                    // Root re-render so the header "Restore Width" fallback
                                    // appears/disappears with the adopted grid promptly.
                                    cx.notify();
                                    // Shared-geometry adoption reflows the grid: no row plan.
                                    view.set_terminal_frame(Arc::new(frame), None, cx);
                                    view.schedule_tui_chrome_probe(
                                        view.state.focused_pane_id.clone(),
                                        false,
                                        cx,
                                    );
                                    active = true;
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    lag_log(format_args!(
                                        "tui.shared_geometry adopt failed: {error}"
                                    ));
                                }
                            }
                            active |= flush_pending_terminal_copy(
                                &mut view.pending_copy_selection,
                                view.terminal_token,
                                &mut managed,
                                cx,
                            );
                            if !view.pending_vt.is_empty() {
                                active = true;
                                managed.write_bytes(&view.pending_vt);
                                pending_bells += managed.take_terminal_bells();
                                view.pending_vt.clear();
                            }
                            let drained = managed.drain_frames_budgeted(
                                terminal_drain_budget_bytes(recent_user_input),
                            );
                            if drained.consumed {
                                active = true;
                                drains_window = drains_window.wrapping_add(1);
                                view.terminal_pending_frame = true;
                                if scroll_debug_enabled() {
                                    lag_log(format_args!(
                                        "scroll_debug drain t={}",
                                        std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .map(|d| d.as_millis())
                                            .unwrap_or(0),
                                    ));
                                }
                            }
                            disconnected |= drained.disconnected;
                            let bells = pending_bells + drained.bells;
                            if bells > 0 {
                                view.handle_terminal_bells(&target, bells);
                            }
                            // Shardlane is the hosted TUI's terminal emulator: answer its
                            // OSC 10/11 "what are your default colors?" queries with the
                            // theme-derived dynamic colors, like any native terminal. The
                            // Herdr TUI paints its surface from these answers; without
                            // them it falls back to a dark built-in palette regardless of
                            // the configured Herdr theme.
                            let color_queries = managed.take_pending_color_queries();
                            if color_queries != 0 {
                                if let Some((foreground, background)) =
                                    view.hosted_terminal_colors
                                {
                                    active = true;
                                    if let Err(error) = managed
                                        .answer_color_queries(color_queries, foreground, background)
                                    {
                                        lag_log(format_args!(
                                            "tui.osc color report failed: {error}"
                                        ));
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            active = true;
                            lag_log(format_args!(
                                "poll_managed try_lock busy (pending_vt={})",
                                view.pending_vt.len(),
                            ));
                        }
                    }
                }
                if should_project_terminal_frame(view.terminal_pending_frame, render_blocked) {
                    active = true;
                    if view.maybe_refresh_terminal_frame(cx, false) {
                        frames_window = frames_window.wrapping_add(1);
                    }
                }
                if view.terminal_pending_frame && !render_blocked {
                    frame_retry_after = Some(terminal_frame_retry_interval(
                        view.last_terminal_frame_at.map(|at| at.elapsed()),
                        view.terminal_frame_min_interval(true),
                    ));
                }
            })
            .is_err()
        {
            break;
        }
        if stale {
            break;
        }
        poll_interval = next_terminal_poll_interval(poll_interval, active, !render_blocked);
        if let Some(retry_after) = frame_retry_after {
            poll_interval = poll_interval.min(retry_after);
        }
        if terminal_trace::enabled() && (active || frame_retry_after.is_some()) {
            terminal_trace::event(format_args!(
                "stage=poll.schedule active={active} render_blocked={render_blocked} retry_us={} next_interval_us={}",
                frame_retry_after
                    .map(|duration| duration.as_micros())
                    .unwrap_or(0),
                poll_interval.as_micros(),
            ));
        }
        if last_summary.elapsed() >= Duration::from_secs(2) {
            if drains_window > 0 || frames_window > 0 {
                lag_log(format_args!(
                    "poll_managed 2s drains={drains_window} frames_sched={frames_window}"
                ));
            }
            last_summary = Instant::now();
            drains_window = 0;
            frames_window = 0;
        }
        if disconnected {
            let _ = this.update(cx, |view, cx| {
                if view.terminal_token == token {
                    // TUI host child process exit: the socket connection is unrelated to the host process
                    //(audit TUI-SMOKE-3) and must not touch ConnectionStatus;
                    // enter cooldown + Failed and show the restart placeholder in the content area.
                    // The shared reset list keeps the last frame visible behind the
                    // placeholder (reset_surface=false, audit B04).
                    view.reset_hosted_tui_state(false, cx);
                    view.tui_respawn_blocked_until = Some(Instant::now() + Duration::from_secs(3));
                    view.tui_host
                        .set_failed("Herdr TUI host exited".to_string());
                }
                cx.notify();
            });
            break;
        }
    })
    .detach();
}

#[cfg(test)]
mod main_tests;
