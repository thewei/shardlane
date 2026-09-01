/**
 * [INPUT]: settings::LazygitConfig, the active Project path, herdr::resolve_user_cli,
 *          ManagedTerminal::host_process, and the hosted-PTY capabilities of Ghostty/TerminalPane.
 * [OUTPUT]: Provides Lazygit CLI detection and version judgment, Git root resolution, overlay
 *           serialization, and ShardlaneApp's single Lazygit auxiliary session attach/stop/poll methods.
 * [POS]: The Lazygit runtime boundary of right_panel; the process lifecycle is independent of the
 *        Herdr global terminal slot and owns at most one short-lived child process while a visible
 *        Lazygit surface exists.
 */
use super::*;
use crate::terminal_interact::TerminalMouseReport;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Auxiliary target name used by the shared terminal input/IME machinery.
pub(crate) const LAZYGIT_TARGET: &str = "lazygit";
const MIN_SUPPORTED_LAZYGIT: LazygitVersion = LazygitVersion(0, 64, 0);
const LAZYGIT_DRAIN_BUDGET_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct LazygitVersion(pub(crate) u32, pub(crate) u32, pub(crate) u32);

impl std::fmt::Display for LazygitVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LazygitCompatibility {
    Missing,
    Outdated,
    Supported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LazygitLaunchMode {
    Missing,
    WithoutOverlay,
    WithOverlay,
}

fn launch_mode(compatibility: LazygitCompatibility) -> LazygitLaunchMode {
    match compatibility {
        LazygitCompatibility::Missing => LazygitLaunchMode::Missing,
        // The CLI startup contract is stable on the versions we can discover, but the
        // Shardlane overlay is only certified against the minimum supported release.
        LazygitCompatibility::Outdated => LazygitLaunchMode::WithoutOverlay,
        LazygitCompatibility::Supported => LazygitLaunchMode::WithOverlay,
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LazygitDetection {
    pub executable: Option<PathBuf>,
    pub version: Option<LazygitVersion>,
    pub compatibility: LazygitCompatibility,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub(crate) enum LazygitRepoError {
    ProjectPathUnresolved,
    NotGitRepository(String),
    GitUnavailable(String),
}

impl std::fmt::Display for LazygitRepoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProjectPathUnresolved => write!(f, "Project path is unavailable"),
            Self::NotGitRepository(detail) => write!(f, "Not a Git repository{detail}"),
            Self::GitUnavailable(detail) => write!(f, "Git is unavailable{detail}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LazygitHostStatus {
    Stopped,
    Starting,
    Running,
    RunningOutdated,
    Missing,
    Outdated,
    ProjectUnresolved,
    NotGitRepository,
    Exited,
    Failed,
}

impl LazygitHostStatus {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Starting => "Starting",
            Self::Running => "Running",
            Self::RunningOutdated => "Running — update recommended",
            Self::Missing => "Lazygit not found",
            Self::Outdated => "Lazygit needs an update",
            Self::ProjectUnresolved => "Project unavailable",
            Self::NotGitRepository => "Not a Git repository",
            Self::Exited => "Exited",
            Self::Failed => "Failed",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LazygitHostState {
    pub status: LazygitHostStatus,
    pub detail: Option<String>,
    pub executable: Option<PathBuf>,
    pub version: Option<LazygitVersion>,
    pub repo_root: Option<PathBuf>,
}

impl Default for LazygitHostState {
    fn default() -> Self {
        Self {
            status: LazygitHostStatus::Stopped,
            detail: None,
            executable: None,
            version: None,
            repo_root: None,
        }
    }
}

/// Runtime-only auxiliary state. It deliberately does not live in RightPanelState or
/// Project snapshots: a Project remembers that Lazygit was selected, never a child process.
pub(crate) struct LazygitSession {
    pub pane: Entity<TerminalPane>,
    pub terminal: Option<Arc<Mutex<ManagedTerminal>>>,
    pub key_encoder: Option<Arc<Mutex<GhosttyKeyEncoderState>>>,
    pub input: Option<TerminalControlInput>,
    pub frame: Arc<TerminalFrame>,
    pub target_path: Option<PathBuf>,
    pub requested_path: Option<PathBuf>,
    pub size: Option<TerminalSize>,
    pub generation: u64,
    pub frame_pending: bool,
    pub last_frame_at: Option<Instant>,
    pub scroll_residual_px: f64,
    pub attach_in_flight: bool,
    pub host: LazygitHostState,
}

impl LazygitSession {
    pub(crate) fn new(cx: &mut Context<ShardlaneApp>) -> Self {
        Self {
            pane: cx.new(TerminalPane::new),
            terminal: None,
            key_encoder: None,
            input: None,
            frame: Arc::new(TerminalFrame::default()),
            target_path: None,
            requested_path: None,
            size: None,
            generation: 0,
            frame_pending: false,
            last_frame_at: None,
            scroll_residual_px: 0.0,
            attach_in_flight: false,
            host: LazygitHostState::default(),
        }
    }

    pub(crate) fn is_running_for(&self, path: &Path) -> bool {
        self.target_path.as_deref() == Some(path) && self.terminal.is_some()
    }
}

pub(crate) fn minimum_supported_version() -> LazygitVersion {
    MIN_SUPPORTED_LAZYGIT
}

/// Parse the version token from both `lazygit --version` formats and Homebrew wrappers.
pub(crate) fn parse_lazygit_version(output: &str) -> Option<LazygitVersion> {
    output.split_whitespace().find_map(|token| {
        let token = token.trim_matches(|ch: char| !ch.is_ascii_digit() && ch != '.');
        let mut parts = token.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        Some(LazygitVersion(major, minor, patch))
    })
}

fn executable_from_path(path: &Path) -> Option<PathBuf> {
    path.is_file().then(|| path.to_path_buf())
}

fn resolve_executable(configured: &str) -> Option<PathBuf> {
    if !configured.trim().is_empty() {
        return executable_from_path(Path::new(configured.trim()));
    }
    // Reuse the host's login-shell/fallback discovery so a Finder-launched .app
    // sees the same Homebrew/wax/user-bin executable as the terminal launch.
    crate::herdr::resolve_user_cli("lazygit")
}

pub(crate) fn detect_lazygit(config: &settings::LazygitConfig) -> LazygitDetection {
    let Some(executable) = resolve_executable(&config.executable_path) else {
        return LazygitDetection {
            executable: None,
            version: None,
            compatibility: LazygitCompatibility::Missing,
            detail: format!(
                "Install Lazygit {} or newer, then refresh detection.",
                minimum_supported_version()
            ),
        };
    };
    let output = Command::new(&executable).arg("--version").output();
    let Ok(output) = output else {
        return LazygitDetection {
            executable: Some(executable),
            version: None,
            compatibility: LazygitCompatibility::Outdated,
            detail: "The configured executable could not report a version.".to_string(),
        };
    };
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let version = parse_lazygit_version(&text);
    let compatibility = match version {
        Some(version) if version >= MIN_SUPPORTED_LAZYGIT => LazygitCompatibility::Supported,
        _ => LazygitCompatibility::Outdated,
    };
    let detail = match version {
        Some(version) => format!("{version} at {}", executable.display()),
        None => format!("Could not parse a version from {}", executable.display()),
    };
    LazygitDetection {
        executable: Some(executable),
        version,
        compatibility,
        detail,
    }
}

pub(crate) fn resolve_git_root(path: &Path) -> Result<PathBuf, LazygitRepoError> {
    if !path.is_dir() {
        return Err(LazygitRepoError::ProjectPathUnresolved);
    }
    let git = crate::herdr::resolve_user_cli("git")
        .ok_or_else(|| LazygitRepoError::GitUnavailable(": git executable not found".into()))?;
    let output = Command::new(git)
        .args([
            "-C",
            path.to_string_lossy().as_ref(),
            "rev-parse",
            "--show-toplevel",
        ])
        .output()
        .map_err(|error| LazygitRepoError::GitUnavailable(format!(": {error}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(LazygitRepoError::NotGitRepository(if detail.is_empty() {
            String::new()
        } else {
            format!(": {detail}")
        }));
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        return Err(LazygitRepoError::NotGitRepository(String::new()));
    }
    Ok(PathBuf::from(root))
}

pub(crate) fn lazygit_overlay_path() -> PathBuf {
    settings::app_data_dir()
        .join("runtime")
        .join("lazygit-overlay.yml")
}

/// Serialize only the small set of Shardlane-owned keys. Lazygit still reads the user's
/// global/repository configuration for every other key; this file is disposable runtime data.
pub(crate) fn serialize_lazygit_overlay(config: &settings::LazygitConfig) -> String {
    let side_panel = f32::from(config.side_panel_width.clamp(15, 60)) / 100.0;
    format!(
        "gui:\n  mouseEvents: {}\n  sidePanelWidth: {side_panel:.4}\n  screenMode: {}\ngit:\n  autoRefresh: {}\n",
        config.mouse_events,
        config.screen_mode.cli_value(),
        config.auto_refresh
    )
}

pub(crate) fn write_lazygit_overlay(
    config: &settings::LazygitConfig,
) -> Result<Option<PathBuf>, String> {
    if !config.integration_enabled {
        return Ok(None);
    }
    let path = lazygit_overlay_path();
    let Some(parent) = path.parent() else {
        return Err("Lazygit overlay has no parent directory".to_string());
    };
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create overlay directory: {error}"))?;
    let temp = path.with_extension("yml.tmp");
    std::fs::write(&temp, serialize_lazygit_overlay(config))
        .map_err(|error| format!("write Lazygit overlay: {error}"))?;
    std::fs::rename(&temp, &path).map_err(|error| format!("install Lazygit overlay: {error}"))?;
    Ok(Some(path))
}

fn apply_lazygit_spawn_environment(command: &mut Command, executable: &Path) {
    // `ManagedTerminal::spawn_pty` deliberately clears the child environment before
    // applying explicit entries. Lazygit starts Git subprocesses by name, so preserve
    // the sanitized login-shell PATH (and editor/locale settings) just like the primary
    // Herdr host.
    let env: std::collections::HashMap<String, String> = std::env::vars().collect();
    for (key, value) in crate::herdr_tui::sanitized_spawn_env(&env) {
        command.env(key, value);
    }
    let mut path_entries = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    let mut tool_parents = Vec::new();
    if let Some(parent) = executable
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tool_parents.push(parent.to_path_buf());
    }
    if let Some(parent) = crate::herdr::resolve_user_cli("git").and_then(|git| {
        git.parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
    }) {
        tool_parents.push(parent);
    }
    for parent in tool_parents.into_iter().rev() {
        if !path_entries.iter().any(|entry| entry == &parent) {
            path_entries.insert(0, parent);
        }
    }
    if let Ok(path) = std::env::join_paths(path_entries) {
        command.env("PATH", path);
    }
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
}

fn discover_default_lazygit_config_file(executable: &Path) -> Option<PathBuf> {
    let output = Command::new(executable)
        .arg("--print-config-dir")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let directory = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if directory.is_empty() {
        return None;
    }
    let path = PathBuf::from(directory).join("config.yml");
    path.is_file().then_some(path)
}

fn merged_lazygit_config_files(
    existing: Option<&str>,
    default_global: Option<&Path>,
    overlay: &Path,
) -> String {
    let mut paths = existing
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if paths.is_empty() {
        if let Some(global) = default_global {
            paths.push(global.to_string_lossy().into_owned());
        }
    }
    let overlay = overlay.to_string_lossy().into_owned();
    if !paths.iter().any(|path| path == &overlay) {
        paths.push(overlay);
    }
    paths.join(",")
}

struct LazygitAttachResult {
    detection: LazygitDetection,
    repo_root: PathBuf,
    managed: ManagedTerminal,
    wake: TerminalWakeReceiver,
    input: TerminalControlInput,
    key_encoder: Arc<Mutex<GhosttyKeyEncoderState>>,
    frame: TerminalFrame,
}

impl ShardlaneApp {
    pub(crate) fn ensure_lazygit_detection(&mut self, cx: &mut Context<Self>) {
        if self.lazygit_detection.is_some() || self.lazygit_detection_requested.get() {
            return;
        }
        self.lazygit_detection_requested.set(true);
        let config = self.config.lazygit.clone();
        self._lazygit_detection_task = cx.spawn(async move |this, cx| {
            let detection = cx
                .background_executor()
                .spawn(async move { detect_lazygit(&config) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.lazygit_detection = Some(detection);
                view.lazygit_detection_requested.set(false);
                cx.notify();
            });
        });
    }

    pub(crate) fn refresh_lazygit_detection(&mut self, cx: &mut Context<Self>) {
        self.lazygit_detection = None;
        self.lazygit_detection_requested.set(false);
        self.ensure_lazygit_detection(cx);
    }

    pub(crate) fn ensure_lazygit_executable_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = &self.lazygit_executable_input {
            let value = self.config.lazygit.executable_path.clone();
            if input.read(cx).value() != value {
                input.update(cx, |state, cx| state.set_value(value, window, cx));
            }
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Auto-detect from PATH"));
        input.update(cx, |state, cx| {
            state.set_value(self.config.lazygit.executable_path.clone(), window, cx);
        });
        let herdr = cx.entity();
        let input_for_commit = input.clone();
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |_, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    let raw = input_for_commit.read(cx).value().to_string();
                    let herdr = herdr.clone();
                    window.defer(cx, move |window, cx| {
                        herdr.update(cx, |this, cx| {
                            this.commit_lazygit_executable(&raw, window, cx)
                        });
                    });
                }
                _ => {}
            },
        );
        self.lazygit_executable_input = Some(input);
        self.lazygit_executable_subscription = Some(subscription);
    }

    pub(crate) fn commit_lazygit_executable(
        &mut self,
        raw: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = raw.trim().to_string();
        if value == self.config.lazygit.executable_path {
            return;
        }
        self.config.lazygit.executable_path = value.clone();
        self.save_config();
        if let Some(input) = &self.lazygit_executable_input {
            input.update(cx, |state, cx| state.set_value(value, window, cx));
        }
        self.refresh_lazygit_detection(cx);
        if self.is_lazygit_surface_active() {
            self.restart_lazygit_session(cx);
        }
        cx.notify();
    }

    pub(crate) fn open_lazygit_action(
        &mut self,
        _: &OpenLazygit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_right_panel_surface(RightPanelSurface::Lazygit, cx);
    }

    pub(crate) fn is_lazygit_surface_active(&self) -> bool {
        self.right_panel.open
            && self
                .right_panel
                .active_surface
                .and_then(|index| self.right_panel.surfaces.get(index))
                .is_some_and(|surface| matches!(surface, RightPanelSurface::Lazygit))
    }

    pub(crate) fn ensure_lazygit_session(&mut self, cx: &mut Context<Self>) {
        if !self.is_lazygit_surface_active() {
            self.stop_lazygit_session(cx);
            return;
        }
        let Some(project_path) = self.active_project_path_for_right_panel() else {
            self.stop_lazygit_session(cx);
            self.lazygit_session.host.status = LazygitHostStatus::ProjectUnresolved;
            self.lazygit_session.host.detail =
                Some("Select a Project before opening Lazygit.".into());
            cx.notify();
            return;
        };
        if self.lazygit_session.is_running_for(&project_path)
            || self.lazygit_session.attach_in_flight
                && self.lazygit_session.requested_path.as_deref() == Some(project_path.as_path())
        {
            return;
        }
        self.stop_lazygit_session(cx);
        self.lazygit_session.host.status = LazygitHostStatus::Starting;
        self.lazygit_session.host.detail = Some("Resolving Git root and starting Lazygit…".into());
        self.lazygit_session.requested_path = Some(project_path.clone());
        self.lazygit_session.attach_in_flight = true;
        self.lazygit_session.generation = self.lazygit_session.generation.wrapping_add(1);
        let generation = self.lazygit_session.generation;
        let config = self.config.lazygit.clone();
        let project_path_for_task = project_path.clone();
        let visible = self.lazygit_session.size.unwrap_or((80, 24, 1, 1));
        let colors = self.hosted_terminal_colors;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let detection = detect_lazygit(&config);
                    let launch_mode = launch_mode(detection.compatibility);
                    if launch_mode == LazygitLaunchMode::Missing {
                        return Err::<LazygitAttachResult, String>(detection.detail.clone());
                    }
                    let repo_root = resolve_git_root(&project_path_for_task)
                        .map_err(|error| error.to_string())?;
                    let overlay = match launch_mode {
                        LazygitLaunchMode::WithOverlay => write_lazygit_overlay(&config)?,
                        LazygitLaunchMode::WithoutOverlay => None,
                        LazygitLaunchMode::Missing => None,
                    };
                    let executable = detection.executable.clone().ok_or_else(|| {
                        "Lazygit executable disappeared during startup".to_string()
                    })?;
                    let mut command = Command::new(&executable);
                    command
                        .current_dir(&repo_root)
                        .arg("--path")
                        .arg(&repo_root)
                        .arg("--screen-mode")
                        .arg(config.screen_mode.cli_value());
                    if let Some(overlay) = overlay {
                        let configured_files = std::env::var("LG_CONFIG_FILE")
                            .ok()
                            .filter(|value| !value.trim().is_empty());
                        let default_global = if configured_files.is_none() {
                            discover_default_lazygit_config_file(&executable)
                        } else {
                            None
                        };
                        let config_files = merged_lazygit_config_files(
                            configured_files.as_deref(),
                            default_global.as_deref(),
                            &overlay,
                        );
                        command
                            .arg("--use-config-file")
                            .arg(config_files);
                    }
                    command.arg(config.startup_panel.cli_value());
                    apply_lazygit_spawn_environment(&mut command, &executable);
                    let mut managed = ManagedTerminal::host_process(
                        &command,
                        visible.0.max(2),
                        visible.1.max(2),
                        crate::ghostty::LOCAL_SCROLLBACK_LINES as usize,
                    )?;
                    if let Some((foreground, background)) = colors {
                        managed.set_dynamic_colors(foreground, background);
                    }
                    let wake = managed.wake_receiver();
                    let input = managed.input_handle();
                    let key_encoder = managed.key_encoder_handle();
                    let frame = managed.frame_reusing(None).unwrap_or_default();
                    Ok(LazygitAttachResult {
                        detection,
                        repo_root,
                        managed,
                        wake,
                        input,
                        key_encoder,
                        frame,
                    })
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.lazygit_session.generation != generation
                    || !view.is_lazygit_surface_active()
                    || view.lazygit_session.requested_path.as_ref() != Some(&project_path)
                {
                    return;
                }
                view.lazygit_session.attach_in_flight = false;
                match result {
                    Ok(result) => {
                        let detection = result.detection.clone();
                        let outdated =
                            detection.compatibility == LazygitCompatibility::Outdated;
                        view.lazygit_session.host.status = if outdated {
                            LazygitHostStatus::RunningOutdated
                        } else {
                            LazygitHostStatus::Running
                        };
                        view.lazygit_session.host.detail = Some(if outdated {
                            format!(
                                "{}; running without the Shardlane overlay. Update to {} for the certified integration.",
                                detection.detail,
                                minimum_supported_version()
                            )
                        } else {
                            detection.detail.clone()
                        });
                        view.lazygit_detection = Some(detection.clone());
                        view.lazygit_session.host.executable = detection.executable;
                        view.lazygit_session.host.version = detection.version;
                        view.lazygit_session.host.repo_root = Some(result.repo_root.clone());
                        view.lazygit_session.target_path = Some(project_path.clone());
                        view.lazygit_session.terminal = Some(Arc::new(Mutex::new(result.managed)));
                        view.lazygit_session.key_encoder = Some(result.key_encoder);
                        view.lazygit_session.input = Some(result.input);
                        view.lazygit_session.frame = Arc::new(result.frame);
                        view.lazygit_session.frame_pending = false;
                        view.lazygit_session.last_frame_at = Some(Instant::now());
                        view.lazygit_session.pane.update(cx, |pane, cx| {
                            // Attach bootstrap frame: no row plan.
                            pane.set_frame(
                                view.lazygit_session.frame.clone(),
                                TerminalFramePlan::Unknown,
                                cx,
                            )
                        });
                        poll_lazygit_terminal(generation, result.wake, cx);
                    }
                    Err(error) => {
                        let detection = view
                            .lazygit_detection
                            .clone()
                            .unwrap_or_else(|| detect_lazygit(&view.config.lazygit));
                        view.lazygit_detection = Some(detection.clone());
                        view.lazygit_session.host.status = match detection.compatibility {
                            LazygitCompatibility::Missing => LazygitHostStatus::Missing,
                            LazygitCompatibility::Outdated => LazygitHostStatus::Outdated,
                            LazygitCompatibility::Supported => {
                                if error.contains("Not a Git repository") {
                                    LazygitHostStatus::NotGitRepository
                                } else if error.contains("Project path") {
                                    LazygitHostStatus::ProjectUnresolved
                                } else {
                                    LazygitHostStatus::Failed
                                }
                            }
                        };
                        view.lazygit_session.host.detail = Some(error);
                        view.lazygit_session.host.executable = detection.executable;
                        view.lazygit_session.host.version = detection.version;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn stop_lazygit_session(&mut self, cx: &mut Context<Self>) {
        let session = &mut self.lazygit_session;
        session.generation = session.generation.wrapping_add(1);
        session.attach_in_flight = false;
        session.requested_path = None;
        session.target_path = None;
        session.terminal = None;
        session.key_encoder = None;
        session.input = None;
        session.size = None;
        session.frame_pending = false;
        session.last_frame_at = None;
        session.scroll_residual_px = 0.0;
        session.frame = Arc::new(TerminalFrame::default());
        session.host = LazygitHostState::default();
        session.pane.update(cx, |pane, cx| {
            pane.set_frame(session.frame.clone(), TerminalFramePlan::Unknown, cx)
        });
    }

    pub(crate) fn restart_lazygit_session(&mut self, cx: &mut Context<Self>) {
        self.stop_lazygit_session(cx);
        self.ensure_lazygit_session(cx);
    }

    pub(crate) fn lazygit_key_encoder(&self) -> Option<Arc<Mutex<GhosttyKeyEncoderState>>> {
        self.lazygit_session.key_encoder.clone()
    }

    pub(crate) fn lazygit_terminal(&self) -> Option<Arc<Mutex<ManagedTerminal>>> {
        self.lazygit_session.terminal.clone()
    }

    pub(crate) fn lazygit_control(&self) -> Option<TerminalControlInput> {
        self.lazygit_session.input.clone()
    }

    pub(crate) fn lazygit_frame(&self) -> &TerminalFrame {
        self.lazygit_session.frame.as_ref()
    }

    pub(crate) fn lazygit_size(&self) -> Option<TerminalSize> {
        self.lazygit_session.size
    }

    pub(crate) fn set_lazygit_size(&mut self, size: TerminalSize, cx: &mut Context<Self>) {
        if self.lazygit_session.size == Some(size) {
            return;
        }
        self.lazygit_session.size = Some(size);
        let Some(terminal) = self.lazygit_session.terminal.clone() else {
            return;
        };
        if let Ok(mut managed) = terminal.try_lock() {
            if let Ok(frame) = managed.resize_local(size.0.max(2), size.1.max(2), size.2, size.3) {
                let frame = Arc::new(frame);
                self.lazygit_session.frame = frame.clone();
                self.lazygit_session.frame_pending = false;
                self.lazygit_session.pane.update(cx, |pane, cx| {
                    // Resize reflows the whole grid: no row plan.
                    pane.set_frame(frame, TerminalFramePlan::Unknown, cx)
                });
            }
        };
    }

    pub(crate) fn lazygit_selection_geometry(&self, window: &Window) -> SelectionGeometry {
        let panel_left =
            (window.bounds().size.width.to_f64() - self.right_panel.width as f64).max(0.0);
        let panel_header = 40.0;
        let padding = f64::from(self.terminal_content_padding());
        SelectionGeometry {
            origin_x: panel_left + padding,
            origin_y: self.chrome_height() + panel_header + padding,
        }
    }

    pub(crate) fn sync_lazygit_surface_size(
        &mut self,
        width: f64,
        height: f64,
        cx: &mut Context<Self>,
    ) {
        let padding = f64::from(self.terminal_content_padding()) * 2.0;
        let cell_width = self.terminal_cell_width().max(1.0);
        let cell_height = self.terminal_cell_height().max(1.0);
        let content_width = (width - padding).max(1.0);
        let content_height = (height - padding).max(1.0);
        let cols = (content_width / cell_width)
            .floor()
            .clamp(f64::from(PANE_MIN_COLS), TERMINAL_MAX_COLS) as u16;
        let rows = (content_height / cell_height)
            .floor()
            .clamp(f64::from(PANE_MIN_ROWS), TERMINAL_MAX_ROWS) as u16;
        self.set_lazygit_size(
            (cols, rows, width.round() as u16, height.round() as u16),
            cx,
        );
    }

    pub(crate) fn handle_lazygit_keyboard(
        &mut self,
        key: &Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.is_lazygit_surface_active() || self.lazygit_session.terminal.is_none() {
            return false;
        }
        if !crate::shell_tui::tui_key_encodes_to_pty(key) {
            return false;
        }
        self.prepare_terminal_for_input(LAZYGIT_TARGET, cx);
        let Some(bytes) = self.encode_terminal_key_for(LAZYGIT_TARGET, key) else {
            return false;
        };
        if bytes.is_empty() {
            // Legacy Ghostty mode can intentionally produce no bytes for an
            // option/alt printable chord (the encoder has no option-as-alt
            // policy). Do not report the event as handled when nothing reached
            // Lazygit; the global route may then apply its normal fallback.
            return false;
        }
        self.queue_terminal_raw_bytes_traced(LAZYGIT_TARGET.to_string(), bytes, "lazygit_key", cx);
        true
    }

    pub(crate) fn handle_lazygit_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_lazygit_surface_active() {
            return;
        }
        let cell_height = self.terminal_cell_height();
        let steps = Self::scroll_rows_for_event(
            &mut self.lazygit_session.scroll_residual_px,
            event,
            cell_height,
        );
        if steps == 0 {
            // Own the gesture even while its fractional residual is below one
            // row; otherwise a parent panel can consume the first touchpad
            // pixels and selection remains visibly stale until the next tick.
            if !event.modifiers.shift {
                self.prepare_terminal_for_scroll_input(LAZYGIT_TARGET, cx);
                cx.stop_propagation();
            }
            return;
        }
        self.prepare_terminal_for_scroll_input(LAZYGIT_TARGET, cx);
        let geometry = self.lazygit_selection_geometry(window);
        if self.try_report_terminal_wheel(LAZYGIT_TARGET, steps, event, geometry, cx) {
            cx.stop_propagation();
        }
    }

    pub(crate) fn handle_lazygit_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_lazygit_surface_active() {
            return;
        }
        window.focus(&self.focus_handle);
        self.prepare_terminal_for_input(LAZYGIT_TARGET, cx);
        let geometry = self.lazygit_selection_geometry(window);
        let button = Self::terminal_mouse_button(&event.button);
        let report = TerminalMouseReport {
            action: TerminalMouseAction::Press,
            button,
            modifiers: &event.modifiers,
            position: event.position,
            selection_geometry: geometry,
            any_button_pressed: true,
        };
        if self.try_report_terminal_mouse(LAZYGIT_TARGET, report, cx) {
            cx.stop_propagation();
        }
    }
}

fn poll_lazygit_terminal(
    generation: u64,
    wake: TerminalWakeReceiver,
    cx: &mut Context<ShardlaneApp>,
) {
    cx.spawn(async move |this, cx| {
        let mut poll_interval = Duration::from_millis(16);
        loop {
            let trigger = super::super::wait_for_terminal_poll(
                &wake,
                cx.background_executor().timer(poll_interval),
            )
            .await;
            let mut disconnected = trigger == super::super::TerminalPollTrigger::Closed;
            let mut active = false;
            let mut stale = false;
            let _ = this.update(cx, |view, cx| {
                if view.lazygit_session.generation != generation
                    || !view.is_lazygit_surface_active()
                {
                    stale = true;
                    return;
                }
                let Some(terminal) = view.lazygit_session.terminal.clone() else {
                    stale = true;
                    return;
                };
                if let Ok(mut managed) = terminal.try_lock() {
                    let drained = managed.drain_frames_budgeted(LAZYGIT_DRAIN_BUDGET_BYTES);
                    if drained.consumed {
                        active = true;
                        view.lazygit_session.frame_pending = true;
                    }
                    disconnected |= drained.disconnected;
                    let color_queries = managed.take_pending_color_queries();
                    if color_queries != 0 {
                        if let Some((foreground, background)) = view.hosted_terminal_colors {
                            let _ =
                                managed.answer_color_queries(color_queries, foreground, background);
                            active = true;
                        }
                    }
                }
                if view.lazygit_session.frame_pending {
                    if let Ok(mut managed) = terminal.try_lock() {
                        let previous = view.lazygit_session.frame.as_ref();
                        if let Ok(frame) = managed.frame_reusing(Some(previous)) {
                            let frame = Arc::new(frame);
                            view.lazygit_session.frame = frame.clone();
                            view.lazygit_session.frame_pending = false;
                            view.lazygit_session.last_frame_at = Some(Instant::now());
                            view.lazygit_session.pane.update(cx, |pane, cx| {
                                // The lazygit pane rides its own raw (unprojected) frames;
                                // keeping the conservative full re-sync is fine for the
                                // bounded auxiliary surface (B16 scope: primary TUI).
                                pane.set_frame(frame, TerminalFramePlan::Unknown, cx)
                            });
                            active = true;
                        }
                    }
                }
                if disconnected {
                    view.lazygit_session.terminal = None;
                    view.lazygit_session.input = None;
                    view.lazygit_session.key_encoder = None;
                    view.lazygit_session.host.status = LazygitHostStatus::Exited;
                    view.lazygit_session.host.detail =
                        Some("Lazygit exited. Restart to try again.".into());
                    cx.notify();
                }
            });
            if stale {
                break;
            }
            if disconnected {
                break;
            }
            poll_interval = if active {
                Duration::from_millis(16)
            } else {
                Duration::from_millis(100)
            };
        }
    })
    .detach();
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_version_output() {
        assert_eq!(
            parse_lazygit_version("lazygit version 0.64.1"),
            Some(LazygitVersion(0, 64, 1))
        );
        assert_eq!(
            parse_lazygit_version("commit=abc version=v1.2.3"),
            Some(LazygitVersion(1, 2, 3))
        );
        assert_eq!(parse_lazygit_version("unknown"), None);
    }

    #[test]
    fn overlay_contains_only_shardlane_owned_keys() {
        let config = settings::LazygitConfig::default();
        let yaml = serialize_lazygit_overlay(&config);
        assert!(yaml.contains("mouseEvents: true"));
        assert!(yaml.contains("sidePanelWidth: 0.2500"));
        assert!(yaml.contains("screenMode: normal"));
        assert!(yaml.contains("autoRefresh: true"));
        assert!(!yaml.contains("keybinding:"));
    }

    #[test]
    fn version_gate_is_explicit() {
        assert!(LazygitVersion(0, 64, 0) >= minimum_supported_version());
        assert!(LazygitVersion(0, 63, 9) < minimum_supported_version());
    }

    #[test]
    fn outdated_cli_can_be_hosted_without_overlay() {
        assert_eq!(
            launch_mode(LazygitCompatibility::Outdated),
            LazygitLaunchMode::WithoutOverlay
        );
    }

    #[test]
    fn overlay_config_keeps_global_and_custom_layers_before_runtime_layer() {
        let global = Path::new("/tmp/lazygit/config.yml");
        let overlay = Path::new("/tmp/shardlane/lazygit-overlay.yml");
        assert_eq!(
            merged_lazygit_config_files(None, Some(global), overlay),
            "/tmp/lazygit/config.yml,/tmp/shardlane/lazygit-overlay.yml"
        );
        assert_eq!(
            merged_lazygit_config_files(
                Some("/dotfiles/lazygit.yml, /themes/dark.yml"),
                Some(global),
                overlay,
            ),
            "/dotfiles/lazygit.yml,/themes/dark.yml,/tmp/shardlane/lazygit-overlay.yml"
        );
    }

    #[test]
    fn auxiliary_spawn_keeps_git_path_after_pty_environment_is_sanitized() {
        let mut command = Command::new("lazygit");
        apply_lazygit_spawn_environment(&mut command, Path::new("/opt/homebrew/bin/lazygit"));
        let path = command
            .get_envs()
            .find(|(key, _)| key.to_string_lossy() == "PATH")
            .and_then(|(_, value)| value);
        assert!(
            path.is_some(),
            "Lazygit must inherit PATH so Git is resolvable"
        );
        assert!(command
            .get_envs()
            .any(|(key, value)| key.to_string_lossy() == "TERM"
                && value == Some(std::ffi::OsStr::new("xterm-256color"))));
    }
}
