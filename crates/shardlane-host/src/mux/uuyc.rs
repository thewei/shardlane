//! NetEase UU Remote Desktop CLI (`uuyc-cli lterm`) backend adapter.
//!
//! UU Remote's `uuyc-cli lterm` manages local terminal sessions (`ls`, `new`,
//! `attach`, `has`, `kill`, `rename`) backed by an internal tmux daemon
//! (`uuyc-mux`). This adapter integrates `uuyc-cli lterm` into the
//! backend-neutral Multiplexer API.
//!
//! Terminal multiplexing only (`agents = false`, `server_admin = false`,
//! `events_push = false`). Gracefully degrades when `uuyc-cli` is not
//! installed.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Deserialize;
use tokio::sync::broadcast;

use super::{
    CreateTab, CreateWorkspace, InstanceListing, InstanceRef, InstanceTarget, Multiplexer,
    MultiplexerConnection, MultiplexerServerAdmin, MultiplexerStream, MuxAgentRuntime,
    MuxCapabilities, MuxDirection, MuxError, SplitDirection,
};
use crate::dto::{HerdrTuiMode, HerdrTuiSessionStatus};
use crate::herdr::{
    Agent, NavigationState, Pane, PaneLayout, PaneLayoutActionResult, PaneMoveResult,
    PaneProcessInfo, PaneProcessInfoProcess, Tab, TabCreatedResult, TabSurfaceState, Workspace,
    WorkspaceCreatedResult,
};
use crate::shared_tui::TuiError;

const UUYC_CLI_BIN: &str = "uuyc-cli";
const KNOWN_PATHS: &[&str] = &[
    "/usr/local/bin/uuyc-cli",
    "/Applications/UURemote.app/Contents/Helpers/uuyc-cli",
];
const OUTPUT_QUEUE_CAPACITY: usize = 4096;

/// Resolve the `uuyc-cli` binary path.
/// Priority: `SHARDLANE_UUYC_CLI_PATH` env override -> `PATH` -> known macOS paths.
pub fn uuyc_cli_path() -> Option<PathBuf> {
    if let Some(custom) = std::env::var_os("SHARDLANE_UUYC_CLI_PATH").map(PathBuf::from) {
        if custom.as_os_str().is_empty() || !custom.is_file() {
            return None;
        }
        return Some(custom);
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join(UUYC_CLI_BIN);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for path in KNOWN_PATHS {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Display-name override store (Shardlane-owned, mirrors Herdr/tmux patterns).
fn instance_metadata_path(name: &str) -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config/shardlane/uuyc-instances")
        .join(format!("{name}.json"))
}

fn read_display_name(name: &str) -> Option<String> {
    let text = std::fs::read_to_string(instance_metadata_path(name)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("display_name")?
        .as_str()
        .map(str::to_string)
        .filter(|name| !name.trim().is_empty())
}

fn write_display_name(name: &str, display_name: &str) -> Result<(), MuxError> {
    let trimmed = display_name.trim();
    if trimmed.is_empty() {
        return Err(MuxError::Api("display name is empty".to_string()));
    }
    let path = instance_metadata_path(name);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(MuxError::Io)?;
    }
    let body = serde_json::json!({ "version": 1, "display_name": trimmed });
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&body).map_err(MuxError::Json)?,
    )
    .map_err(MuxError::Io)?;
    Ok(())
}

// --- CLI JSON contracts ---

#[derive(Debug, Deserialize)]
struct LtermLsResponse {
    #[serde(default)]
    data: Option<LtermLsData>,
    #[serde(default)]
    success: bool,
    #[serde(default)]
    error: Option<LtermErrorDetail>,
}

#[derive(Debug, Deserialize)]
struct LtermLsData {
    #[serde(default)]
    sessions: Vec<LtermSessionEntry>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LtermSessionEntry {
    pub name: String,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub created_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LtermErrorDetail {
    #[serde(default)]
    pub code: Option<i64>,
    #[serde(default)]
    pub message: Option<String>,
}

fn run_uuyc_cli(args: &[&str]) -> Result<String, MuxError> {
    let cli = uuyc_cli_path()
        .ok_or_else(|| MuxError::InstallFailed("uuyc-cli is not installed".to_string()))?;
    let output = Command::new(&cli)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(MuxError::Io)?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        if !stdout.is_empty() {
            if let Ok(resp) = serde_json::from_str::<LtermLsResponse>(&stdout) {
                if let Some(err) = resp.error {
                    if let Some(msg) = err.message {
                        return Err(MuxError::Api(msg));
                    }
                }
            }
        }
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("uuyc-cli exited with status {:?}", output.status.code())
        };
        Err(MuxError::Api(detail))
    }
}

/// Fast check whether a session exists using `uuyc-cli lterm has <name>`.
fn has_session(name: &str) -> bool {
    let Some(cli) = uuyc_cli_path() else {
        return false;
    };
    Command::new(&cli)
        .args(["lterm", "has", name])
        .stdin(Stdio::null())
        .output()
        .ok()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn list_sessions_from_cli() -> Option<Vec<LtermSessionEntry>> {
    let text = run_uuyc_cli(&["lterm", "ls"]).ok()?;
    let resp: LtermLsResponse = serde_json::from_str(&text).ok()?;
    if !resp.success {
        return None;
    }
    resp.data.map(|d| d.sessions)
}

/// Create a session non-interactively if it does not already exist.
/// `uuyc-cli lterm new` exits with code 1 in non-TTY environments after
/// the session is created by XPC; we verify liveness with `has_session`.
fn ensure_session(name: &str) -> Result<(), MuxError> {
    if has_session(name) {
        return Ok(());
    }
    let _ = run_uuyc_cli(&["lterm", "new", name]);
    if has_session(name) {
        Ok(())
    } else {
        Err(MuxError::Api(format!(
            "failed to create uuyc session '{name}'"
        )))
    }
}

fn rename_session(old_name: &str, new_name: &str) -> Result<(), MuxError> {
    run_uuyc_cli(&["lterm", "rename", old_name, new_name]).map(|_| ())
}

fn kill_session(name: &str) -> Result<(), MuxError> {
    if !has_session(name) {
        return Ok(());
    }
    run_uuyc_cli(&["lterm", "kill", name]).map(|_| ())
}

// --- Backend facade ---

#[derive(Clone, Copy, Debug, Default)]
pub struct UuycBackend;

impl Multiplexer for UuycBackend {
    fn id(&self) -> &'static str {
        "uuyc"
    }

    fn capabilities(&self) -> MuxCapabilities {
        MuxCapabilities {
            agents: false,
            server_admin: false,
            shared_tui: true,
            pane_history_read: false,
            cross_workspace_tab_move: false,
            events_push: false,
        }
    }

    fn list_instances(&self) -> Option<Vec<InstanceListing>> {
        let _cli = uuyc_cli_path()?;
        let sessions = list_sessions_from_cli()?;
        let listings = sessions
            .into_iter()
            .map(|entry| InstanceListing {
                backend: self.id().to_string(),
                name: entry.name.clone(),
                display_name: read_display_name(&entry.name),
                running: entry.state.as_deref() == Some("running"),
                is_default: false,
            })
            .collect();
        Some(listings)
    }

    fn rename_instance(&self, instance: &str, display_name: &str) -> Result<(), MuxError> {
        write_display_name(instance, display_name)
    }

    fn stop_instance(&self, instance: &str) -> Result<(), MuxError> {
        kill_session(instance)
    }

    fn delete_instance(&self, instance: &str) -> Result<(), MuxError> {
        kill_session(instance)?;
        let _ = std::fs::remove_file(instance_metadata_path(instance));
        Ok(())
    }

    fn open_instance(
        &self,
        reference: &InstanceRef,
    ) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
        let name = match &reference.target {
            InstanceTarget::Named(name) => {
                ensure_session(name)?;
                name.clone()
            }
            InstanceTarget::Default => {
                let existing = self.list_instances().unwrap_or_default();
                if let Some(first) = existing.first() {
                    first.name.clone()
                } else {
                    let default_name = "session1";
                    ensure_session(default_name)?;
                    default_name.to_string()
                }
            }
            InstanceTarget::Socket(_) => {
                return Err(MuxError::Unsupported("socket targeting for uuyc"));
            }
        };

        Ok(Arc::new(UuycConnection::new(name)))
    }

    fn connect_instance(
        &self,
        reference: &InstanceRef,
    ) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
        let name = match &reference.target {
            InstanceTarget::Named(name) => {
                if !has_session(name) {
                    return Err(MuxError::NotFound(format!(
                        "uuyc session '{name}' not found"
                    )));
                }
                name.clone()
            }
            InstanceTarget::Default => {
                let existing = self.list_instances().unwrap_or_default();
                existing
                    .into_iter()
                    .next()
                    .map(|l| l.name)
                    .ok_or_else(|| MuxError::NotFound("no uuyc sessions exist".to_string()))?
            }
            InstanceTarget::Socket(_) => {
                return Err(MuxError::Unsupported("socket targeting for uuyc"));
            }
        };

        Ok(Arc::new(UuycConnection::new(name)))
    }

    fn server_admin(&self) -> Option<&dyn MultiplexerServerAdmin> {
        None
    }
}

// --- Per-instance connection ---

pub struct UuycConnection {
    session: String,
    stream: Mutex<Option<Arc<UuycAttachStream>>>,
}

impl UuycConnection {
    pub fn new(session: String) -> Self {
        Self {
            session,
            stream: Mutex::new(None),
        }
    }

    fn open_stream(
        &self,
        session: &str,
        cols: u16,
        rows: u16,
    ) -> Result<Arc<UuycAttachStream>, TuiError> {
        let mut guard = self
            .stream
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(existing) = guard.as_ref() {
            if existing.is_running() {
                return Ok(existing.clone());
            }
        }
        let stream = UuycAttachStream::spawn(session, cols, rows)?;
        *guard = Some(stream.clone());
        Ok(stream)
    }

    fn default_workspace(&self) -> Workspace {
        Workspace {
            workspace_id: self.session.clone(),
            label: Some(self.session.clone()),
            cwd: None,
            agent_status: None,
            active_tab_id: Some(format!("{}-tab-0", self.session)),
            focused: true,
            tab_count: Some(1),
            pane_count: Some(1),
            number: None,
        }
    }

    fn default_tab(&self) -> Tab {
        Tab {
            tab_id: format!("{}-tab-0", self.session),
            workspace_id: Some(self.session.clone()),
            label: Some(self.session.clone()),
            title: None,
            terminal_title: None,
            agent_status: None,
            pane_count: Some(1),
            focused: true,
        }
    }

    fn default_pane(&self) -> Pane {
        Pane {
            pane_id: format!("{}-pane-0", self.session),
            terminal_id: None,
            workspace_id: Some(self.session.clone()),
            tab_id: Some(format!("{}-tab-0", self.session)),
            label: None,
            title: Some(self.session.clone()),
            terminal_title: None,
            cwd: None,
            agent_status: None,
            agent: None,
            focused: true,
            scroll: None,
        }
    }

    fn default_layout(&self) -> PaneLayout {
        let rect = crate::herdr::LayoutRect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        PaneLayout {
            tab_id: format!("{}-tab-0", self.session),
            workspace_id: Some(self.session.clone()),
            area: rect,
            panes: vec![crate::herdr::LayoutPane {
                pane_id: format!("{}-pane-0", self.session),
                rect,
                focused: true,
            }],
            splits: Vec::new(),
            focused_pane_id: Some(format!("{}-pane-0", self.session)),
            zoomed: false,
        }
    }
}

impl MultiplexerConnection for UuycConnection {
    fn capabilities(&self) -> MuxCapabilities {
        UuycBackend.capabilities()
    }

    fn ping(&self) -> Result<(), MuxError> {
        if has_session(&self.session) {
            Ok(())
        } else {
            Err(MuxError::NotFound(format!(
                "uuyc session '{}' not found",
                self.session
            )))
        }
    }

    fn protocol(&self) -> Option<u32> {
        None
    }

    fn server_started_with_supplied_config(&self) -> bool {
        false
    }

    fn navigation_state(&self) -> Result<NavigationState, MuxError> {
        Ok(NavigationState {
            focused_workspace_id: Some(self.session.clone()),
            focused_tab_id: Some(format!("{}-tab-0", self.session)),
            workspaces: vec![self.default_workspace()],
            tabs: vec![self.default_tab()],
        })
    }

    fn visible_state(&self) -> Result<crate::herdr::HerdrState, MuxError> {
        Ok(crate::herdr::HerdrState {
            focused_workspace_id: Some(self.session.clone()),
            focused_tab_id: Some(format!("{}-tab-0", self.session)),
            focused_pane_id: Some(format!("{}-pane-0", self.session)),
            workspaces: vec![self.default_workspace()],
            tabs: vec![self.default_tab()],
            panes: vec![self.default_pane()],
            agents: Vec::new(),
            layouts: vec![self.default_layout()],
            protocol: None,
            version: None,
        })
    }

    fn host_bootstrap_state(&self) -> Result<crate::herdr::HerdrState, MuxError> {
        self.visible_state()
    }

    fn workspace_state(&self) -> Result<crate::herdr::HerdrState, MuxError> {
        self.visible_state()
    }

    fn workspace_panes(&self, _workspace_id: &str) -> Result<Vec<Pane>, MuxError> {
        Ok(vec![self.default_pane()])
    }

    fn tab_surface_state(
        &self,
        workspace_id: &str,
        tab_id: &str,
    ) -> Result<TabSurfaceState, MuxError> {
        Ok(TabSurfaceState {
            workspace_id: workspace_id.to_string(),
            tab_id: tab_id.to_string(),
            focused_pane_id: Some(format!("{}-pane-0", self.session)),
            panes: vec![self.default_pane()],
            layouts: vec![self.default_layout()],
        })
    }

    fn pane_layout(&self, _pane_id: &str) -> Result<PaneLayout, MuxError> {
        Ok(self.default_layout())
    }

    fn agents(&self) -> Result<Vec<Agent>, MuxError> {
        Ok(Vec::new())
    }

    fn subscribe_events(&self) -> Result<async_channel::Receiver<super::MuxEvent>, MuxError> {
        Err(MuxError::Unsupported("events_push"))
    }

    fn subscribe_pane_events(
        &self,
        _pane_ids: &[String],
    ) -> Result<async_channel::Receiver<super::MuxEvent>, MuxError> {
        Err(MuxError::Unsupported("events_push"))
    }

    fn create_workspace(
        &self,
        _params: &CreateWorkspace<'_>,
    ) -> Result<WorkspaceCreatedResult, MuxError> {
        Ok(WorkspaceCreatedResult {
            workspace: self.default_workspace(),
            tab: self.default_tab(),
            root_pane: self.default_pane(),
        })
    }

    fn close_workspace(&self, workspace_id: &str) -> Result<(), MuxError> {
        kill_session(workspace_id)
    }

    fn rename_workspace(&self, workspace_id: &str, label: &str) -> Result<(), MuxError> {
        rename_session(workspace_id, label)
    }

    fn move_workspace(&self, _workspace_id: &str, _insert_index: usize) -> Result<(), MuxError> {
        Ok(())
    }

    fn move_workspace_before(
        &self,
        _workspace_id: &str,
        _before_workspace_id: &str,
    ) -> Result<(), MuxError> {
        Ok(())
    }

    fn workspace_focus(&self, _workspace_id: &str) -> Result<(), MuxError> {
        Ok(())
    }

    fn create_tab(&self, _params: &CreateTab<'_>) -> Result<TabCreatedResult, MuxError> {
        Ok(TabCreatedResult {
            tab: self.default_tab(),
            root_pane: self.default_pane(),
        })
    }

    fn close_tab(&self, _tab_id: &str) -> Result<(), MuxError> {
        Ok(())
    }

    fn rename_tab(&self, _tab_id: &str, _label: &str) -> Result<(), MuxError> {
        Ok(())
    }

    fn move_tab(&self, _tab_id: &str, _insert_index: usize) -> Result<(), MuxError> {
        Ok(())
    }

    fn tab_focus(&self, _tab_id: &str) -> Result<(), MuxError> {
        Ok(())
    }

    fn split_pane(&self, _pane_id: &str, _direction: SplitDirection) -> Result<Pane, MuxError> {
        Err(MuxError::Unsupported("split_pane"))
    }

    fn close_pane(&self, _pane_id: &str) -> Result<(), MuxError> {
        kill_session(&self.session)
    }

    fn rename_pane(&self, _pane_id: &str, _label: &str) -> Result<(), MuxError> {
        Ok(())
    }

    fn swap_pane(
        &self,
        _pane_id: &str,
        _direction: MuxDirection,
    ) -> Result<PaneLayoutActionResult, MuxError> {
        Err(MuxError::Unsupported("swap_pane"))
    }

    fn resize_pane(
        &self,
        _pane_id: &str,
        _direction: MuxDirection,
    ) -> Result<PaneLayoutActionResult, MuxError> {
        Ok(PaneLayoutActionResult {
            layout: self.default_layout(),
        })
    }

    fn toggle_pane_zoom(&self, _pane_id: &str) -> Result<PaneLayoutActionResult, MuxError> {
        Ok(PaneLayoutActionResult {
            layout: self.default_layout(),
        })
    }

    fn pane_focus(&self, _pane_id: &str) -> Result<(), MuxError> {
        Ok(())
    }

    fn move_pane_to_tab(&self, _pane_id: &str, _tab_id: &str) -> Result<PaneMoveResult, MuxError> {
        Err(MuxError::Unsupported("move_pane_to_tab"))
    }

    fn move_pane_to_new_tab(
        &self,
        _pane_id: &str,
        _workspace_id: &str,
    ) -> Result<PaneMoveResult, MuxError> {
        Err(MuxError::Unsupported("move_pane_to_new_tab"))
    }

    fn set_split_ratio(&self, _tab_id: &str, _path: &[bool], _ratio: f64) -> Result<(), MuxError> {
        Ok(())
    }

    fn send_text(&self, _pane_id: &str, text: &str) -> Result<(), MuxError> {
        let guard = self
            .stream
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(stream) = guard.as_ref() {
            stream
                .send_bytes(text.as_bytes())
                .map_err(|error| MuxError::Api(error.to_string()))
        } else {
            Err(MuxError::Api("terminal stream is not open".to_string()))
        }
    }

    fn send_keys(&self, _pane_id: &str, _keys: &[String]) -> Result<(), MuxError> {
        Err(MuxError::Unsupported("send_keys"))
    }

    fn pane_process_info(&self, pane_id: &str) -> Result<PaneProcessInfo, MuxError> {
        Ok(PaneProcessInfo {
            pane_id: pane_id.to_string(),
            shell_pid: None,
            tty: None,
            foreground_process_group_id: None,
            foreground_processes: vec![PaneProcessInfoProcess {
                pid: 0,
                name: "zsh".to_string(),
                argv: None,
                argv0: None,
                cmdline: None,
                cwd: None,
            }],
        })
    }

    fn open_shared_session(
        &self,
        key: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Result<Arc<dyn MultiplexerStream>, MuxError> {
        let session = key.unwrap_or(&self.session);
        self.open_stream(session, cols, rows)
            .map(|stream| stream as Arc<dyn MultiplexerStream>)
            .map_err(|error| MuxError::Api(error.to_string()))
    }

    fn agent_runtime(&self) -> Option<&dyn MuxAgentRuntime> {
        None
    }
}

// --- Domain 6 — the `uuyc-cli lterm attach` child stream ---

/// One `uuyc-cli lterm attach` child on a private PTY.
pub struct UuycAttachStream {
    id: String,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>,
    events: broadcast::Sender<super::StreamEvent>,
    revision: AtomicU64,
    cols: AtomicU64,
    rows: AtomicU64,
    stopped: AtomicBool,
}

impl UuycAttachStream {
    pub fn spawn(session: &str, cols: u16, rows: u16) -> Result<Arc<Self>, TuiError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TuiError::Spawn(format!("open attach pty: {error}")))?;

        let cli = uuyc_cli_path()
            .ok_or_else(|| TuiError::Spawn("uuyc-cli is not installed".to_string()))?;

        let mut builder = CommandBuilder::new(cli);
        builder.arg("lterm");
        builder.arg("attach");
        builder.arg(session);
        builder.env("TERM", "xterm-256color");

        let child = pair
            .slave
            .spawn_command(builder)
            .map_err(|error| TuiError::Spawn(format!("spawn attach child: {error}")))?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| TuiError::Spawn(format!("clone attach reader: {error}")))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| TuiError::Spawn(format!("take attach writer: {error}")))?;

        let (events, _) = broadcast::channel(OUTPUT_QUEUE_CAPACITY);
        let stream = Arc::new(Self {
            id: format!("uuyc-attach-{}", uuid::Uuid::new_v4()),
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(Some(child)),
            events,
            revision: AtomicU64::new(0),
            cols: AtomicU64::new(u64::from(cols)),
            rows: AtomicU64::new(u64::from(rows)),
            stopped: AtomicBool::new(false),
        });

        let publisher = stream.clone();
        std::thread::Builder::new()
            .name("uuyc-attach-reader".to_string())
            .spawn(move || {
                let mut chunk = [0u8; 8192];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(count) => {
                            publisher.revision.fetch_add(1, Ordering::Relaxed);
                            let _ = publisher.events.send(super::StreamEvent::Output {
                                revision: publisher.revision.load(Ordering::Relaxed),
                                bytes: chunk[..count].to_vec(),
                                published_at: Instant::now(),
                            });
                        }
                        Err(_) => break,
                    }
                }
                let _ = publisher.events.send(super::StreamEvent::Status {
                    summary: publisher.summary_locked(),
                });
            })
            .map_err(|error| TuiError::Spawn(format!("spawn attach reader: {error}")))?;

        Ok(stream)
    }

    fn summary_locked(&self) -> crate::dto::HerdrTuiSessionSummary {
        crate::dto::HerdrTuiSessionSummary {
            id: self.id.clone(),
            mode: HerdrTuiMode::Shared,
            status: if self.is_running() {
                HerdrTuiSessionStatus::Running
            } else if self.stopped.load(Ordering::Relaxed) {
                HerdrTuiSessionStatus::Stopped
            } else {
                HerdrTuiSessionStatus::Failed
            },
            cols: self.cols.load(Ordering::Relaxed) as u16,
            rows: self.rows.load(Ordering::Relaxed) as u16,
            revision: self.revision.load(Ordering::Relaxed),
        }
    }
}

impl MultiplexerStream for UuycAttachStream {
    fn id(&self) -> &str {
        &self.id
    }

    fn summary(&self) -> super::StreamSummary {
        self.summary_locked()
    }

    fn is_running(&self) -> bool {
        let mut guard = self
            .child
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match guard.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    fn viewer_count(&self) -> usize {
        self.events.receiver_count()
    }

    fn subscribe(&self) -> broadcast::Receiver<super::StreamEvent> {
        self.events.subscribe()
    }

    fn subscribe_with_startup_replay(&self) -> (broadcast::Receiver<super::StreamEvent>, Vec<u8>) {
        (self.events.subscribe(), Vec::new())
    }

    fn wake_subscribers(&self) {}

    fn send_bytes(&self, data: &[u8]) -> Result<(), super::StreamError> {
        let mut writer = self
            .writer
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        writer
            .write_all(data)
            .map_err(|error| TuiError::Io(error.to_string()))
    }

    fn send_bytes_traced(
        &self,
        data: &[u8],
        _trace_id: u64,
        _coalescible: bool,
    ) -> Result<(), super::StreamError> {
        self.send_bytes(data)
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<super::StreamSummary, super::StreamError> {
        self.master
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TuiError::Io(error.to_string()))?;
        self.cols.store(u64::from(cols), Ordering::Relaxed);
        self.rows.store(u64::from(rows), Ordering::Relaxed);
        Ok(self.summary_locked())
    }

    fn force_redraw(&self) -> Result<(), super::StreamError> {
        let cols = self.cols.load(Ordering::Relaxed) as u16;
        let rows = self.rows.load(Ordering::Relaxed) as u16;
        self.resize(cols, rows.saturating_sub(1).max(1))?;
        std::thread::sleep(Duration::from_millis(30));
        self.resize(cols, rows)?;
        Ok(())
    }

    fn stop(&self) -> bool {
        self.stopped.store(true, Ordering::Relaxed);
        // Attempt clean tmux detach via Ctrl-b d
        let _ = self.send_bytes(b"\x02d");
        std::thread::sleep(Duration::from_millis(50));
        let mut guard = self
            .child
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match guard.as_mut() {
            Some(child) => {
                if matches!(child.try_wait(), Ok(None)) {
                    child.kill().is_ok()
                } else {
                    true
                }
            }
            None => true,
        }
    }

    fn stop_with_reap(&self, reap_deadline: Option<Duration>) -> bool {
        self.stop();
        let deadline = reap_deadline.unwrap_or(Duration::from_secs(2));
        self.wait_for_exit(deadline)
    }

    fn wait_for_exit(&self, timeout: Duration) -> bool {
        let started = Instant::now();
        loop {
            if !self.is_running() {
                let mut guard = self
                    .child
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                *guard = None;
                return true;
            }
            if started.elapsed() >= timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn backend_id_and_capabilities() {
        let backend = UuycBackend;
        assert_eq!(backend.id(), "uuyc");
        let caps = backend.capabilities();
        assert!(!caps.agents);
        assert!(!caps.server_admin);
        assert!(caps.shared_tui);
        assert!(!caps.pane_history_read);
        assert!(!caps.events_push);
    }

    #[test]
    fn parse_lterm_ls_json() {
        let json_text = r#"{
            "data": {
                "sessions": [
                    {
                        "created_at_ms": 1788700905041,
                        "name": "session1",
                        "shell": "zsh",
                        "state": "detached"
                    },
                    {
                        "created_at_ms": 1788747611197,
                        "name": "开发终端",
                        "shell": "zsh",
                        "state": "running"
                    }
                ]
            },
            "success": true,
            "timestamp": "2026-09-07T02:20:12Z"
        }"#;

        let resp: LtermLsResponse = serde_json::from_str(json_text).expect("valid json");
        assert!(resp.success);
        let sessions = resp.data.expect("data exists").sessions;
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].name, "session1");
        assert_eq!(sessions[0].state.as_deref(), Some("detached"));
        assert_eq!(sessions[1].name, "开发终端");
        assert_eq!(sessions[1].state.as_deref(), Some("running"));
    }

    #[test]
    fn parse_lterm_error_json() {
        let json_text = r#"{
            "error": {
                "code": 1,
                "message": "会话不存在"
            },
            "success": false,
            "timestamp": "2026-09-07T02:19:06Z"
        }"#;

        let resp: LtermLsResponse = serde_json::from_str(json_text).expect("valid error json");
        assert!(!resp.success);
        assert_eq!(resp.error.unwrap().message.as_deref(), Some("会话不存在"));
    }

    #[test]
    fn connection_projection_shape() {
        let conn = UuycConnection::new("test_session".to_string());
        assert_eq!(conn.capabilities(), UuycBackend.capabilities());
        assert!(conn.protocol().is_none());
        assert!(!conn.server_started_with_supplied_config());

        let nav = conn.navigation_state().expect("navigation_state");
        assert_eq!(nav.focused_workspace_id.as_deref(), Some("test_session"));
        assert_eq!(nav.workspaces.len(), 1);
        assert_eq!(nav.tabs.len(), 1);

        let state = conn.visible_state().expect("visible_state");
        assert_eq!(state.workspaces.len(), 1);
        assert_eq!(state.tabs.len(), 1);
        assert_eq!(state.panes.len(), 1);
        assert_eq!(state.focused_workspace_id.as_deref(), Some("test_session"));
        assert_eq!(
            state.focused_pane_id.as_deref(),
            Some("test_session-pane-0")
        );

        let panes = conn
            .workspace_panes("test_session")
            .expect("workspace_panes");
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].pane_id, "test_session-pane-0");

        let layout = conn
            .pane_layout("test_session-pane-0")
            .expect("pane_layout");
        assert_eq!(layout.panes.len(), 1);
        assert_eq!(
            layout.focused_pane_id.as_deref(),
            Some("test_session-pane-0")
        );

        // events_push = false
        assert!(matches!(
            conn.subscribe_events(),
            Err(MuxError::Unsupported("events_push"))
        ));
    }

    #[test]
    fn uuyc_not_installed_degrades_gracefully() {
        std::env::set_var("SHARDLANE_UUYC_CLI_PATH", "/non_existent/path/to/uuyc-cli");
        assert_eq!(uuyc_cli_path(), None);
        assert_eq!(UuycBackend.list_instances(), None);
        std::env::remove_var("SHARDLANE_UUYC_CLI_PATH");
    }

    #[test]
    fn live_uuyc_session_lifecycle() {
        if uuyc_cli_path().is_none() {
            eprintln!("skipping live_uuyc_session_lifecycle: uuyc-cli not available");
            return;
        }
        let test_session = format!("uuyc_test_{}", std::process::id());
        let backend = UuycBackend;

        // 1. Open instance (creates session)
        let conn = backend
            .open_instance(&InstanceRef::named("uuyc", &test_session))
            .expect("open_instance should create session");
        assert!(has_session(&test_session));

        // 2. Ping connection
        assert!(conn.ping().is_ok());

        // 3. Rename session
        let renamed_session = format!("{}_renamed", test_session);
        assert!(conn
            .rename_workspace(&test_session, &renamed_session)
            .is_ok());
        assert!(has_session(&renamed_session));

        // 4. Delete instance (kill session)
        assert!(backend.delete_instance(&renamed_session).is_ok());
        assert!(!has_session(&renamed_session));
    }
}
