//! tmux backend adapter (docs/multiplexer-api.md Phase 5 MVP): terminal
//! multiplexing only. A tmux server socket is an Instance, a tmux session a
//! Workspace, a window a Tab, a pane a Pane. Agents and server administration
//! are absent (`MuxCapabilities` false → typed degradation); there is no push
//! event stream (`events_push = false`).
//!
//! All backend knowledge (CLI spellings, format strings, `tmux attach` child
//! spawning) lives in this file.
//!
//! Isolation contract: `SHARDLANE_TMUX_SOCKET` steers the Default bind path
//! AND `list_instances` probing, mirroring `HERDR_SOCKET_PATH`; when set, no
//! code path may touch the user's default tmux socket.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use tokio::sync::broadcast;

use super::{
    CreateTab, CreateWorkspace, InstanceListing, InstanceRef, InstanceTarget, Multiplexer,
    MultiplexerConnection, MultiplexerServerAdmin, MultiplexerStream, MuxAgentRuntime,
    MuxCapabilities, MuxDirection, MuxError, PaneHistory, SplitDirection,
};
use crate::dto::{HerdrTuiMode, HerdrTuiSessionStatus};
use crate::herdr::{
    Agent, NavigationState, Pane, PaneLayout, PaneLayoutActionResult, PaneMoveResult,
    PaneProcessInfo, PaneProcessInfoProcess, Tab, TabCreatedResult, TabSurfaceState, Workspace,
    WorkspaceCreatedResult,
};
use crate::shared_tui::TuiError;

#[cfg(test)]
use super::kit;

const TMUX_BIN: &str = "tmux";
const OUTPUT_QUEUE_CAPACITY: usize = 4096;

/// Display-name override store (Shardlane-owned, mirrors the Herdr session
/// metadata pattern): one JSON file per tmux instance.
fn instance_metadata_path(name: &str) -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config/shardlane/tmux-instances")
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

/// Isolated-testing seam (mirrors `HERDR_SOCKET_PATH`): when set, the
/// default-instance target attaches to this socket instead of the user's
/// default tmux server, so harnesses never touch live sessions.
fn default_socket_override() -> Option<PathBuf> {
    std::env::var_os("SHARDLANE_TMUX_SOCKET").map(PathBuf::from)
}

/// Resolve the tmux binary; `None` = not installed (enumeration degrades).
fn tmux_path() -> Option<PathBuf> {
    let direct = PathBuf::from(TMUX_BIN);
    if direct.is_file() {
        return Some(direct);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(TMUX_BIN))
            .find(|candidate| candidate.is_file())
    })
}

// --- Backend facade ---

#[derive(Clone, Copy, Debug, Default)]
pub struct TmuxBackend;

/// The MVP enumerates the user's default tmux server as one instance.
pub const DEFAULT_INSTANCE_NAME: &str = "default";

impl Multiplexer for TmuxBackend {
    fn id(&self) -> &'static str {
        "tmux"
    }

    fn capabilities(&self) -> MuxCapabilities {
        MuxCapabilities {
            agents: false,
            server_admin: false,
            shared_tui: true,
            pane_history_read: true,
            cross_workspace_tab_move: false,
            events_push: false,
        }
    }

    fn list_instances(&self) -> Option<Vec<InstanceListing>> {
        let tmux = tmux_path()?;
        // Probe the same server a Default bind would attach to: honor the
        // isolated-testing socket override instead of always the user's
        // default socket.
        let mut command = Command::new(&tmux);
        if let Some(socket) = default_socket_override() {
            command.arg("-S").arg(socket);
        }
        let running = command
            .args(["list-sessions"])
            .stdin(Stdio::null())
            .output()
            .ok()
            .map(|output| output.status.success())
            .unwrap_or(false);
        Some(vec![InstanceListing {
            backend: self.id().to_string(),
            name: DEFAULT_INSTANCE_NAME.to_string(),
            display_name: read_display_name(DEFAULT_INSTANCE_NAME),
            running,
            is_default: false,
        }])
    }

    fn rename_instance(&self, instance: &str, display_name: &str) -> Result<(), MuxError> {
        write_display_name(instance, display_name)
    }

    fn stop_instance(&self, _instance: &str) -> Result<(), MuxError> {
        TmuxConnection::default_server()
            .run(&["kill-server"])
            .map(|_| ())
    }

    fn delete_instance(&self, instance: &str) -> Result<(), MuxError> {
        self.stop_instance(instance)?;
        let _ = std::fs::remove_file(instance_metadata_path(instance));
        Ok(())
    }

    fn open_instance(
        &self,
        reference: &InstanceRef,
    ) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
        self.connect_instance(reference)
    }

    fn connect_instance(
        &self,
        reference: &InstanceRef,
    ) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
        let socket = match &reference.target {
            // Default resolves through the backend's environment seam
            // (SHARDLANE_TMUX_SOCKET), mirroring HERDR_SOCKET_PATH; a Named
            // session still lives on the user's own socket.
            InstanceTarget::Default => default_socket_override(),
            InstanceTarget::Named(_) => None,
            InstanceTarget::Socket(path) => Some(path.clone()),
        };
        let connection = Arc::new(TmuxConnection {
            tmux_socket: socket,
            stream: Mutex::new(None),
        });
        // Side-effect-free: verify the server answers before handing out a
        // connection (never starts a tmux server on the user's behalf).
        connection.run(&["list-sessions"])?;
        Ok(connection)
    }

    fn server_admin(&self) -> Option<&dyn MultiplexerServerAdmin> {
        None
    }
}

// --- Per-instance connection ---

pub struct TmuxConnection {
    /// Explicit server socket; `None` = the user's default tmux socket.
    tmux_socket: Option<PathBuf>,
    /// The one `tmux attach` child per connection instance (Domain 6).
    stream: Mutex<Option<Arc<TmuxAttachStream>>>,
}

impl TmuxConnection {
    fn default_server() -> Self {
        Self {
            tmux_socket: default_socket_override(),
            stream: Mutex::new(None),
        }
    }

    #[cfg(test)]
    fn with_socket(socket: PathBuf) -> Self {
        Self {
            tmux_socket: Some(socket),
            stream: Mutex::new(None),
        }
    }

    /// Run one tmux command against this connection's server.
    fn run(&self, args: &[&str]) -> Result<String, MuxError> {
        let tmux = tmux_path()
            .ok_or_else(|| MuxError::InstallFailed("tmux is not installed".to_string()))?;
        let mut command = Command::new(&tmux);
        if let Some(socket) = &self.tmux_socket {
            command.arg("-S").arg(socket);
        }
        command.args(args).stdin(Stdio::null());
        let output = command.output().map_err(MuxError::Io)?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(MuxError::Api(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ))
        }
    }

    /// One full projection snapshot: sessions → Workspaces, windows → Tabs,
    /// panes → Panes. The first session/window/pane ordering is tmux's own
    /// index order; focus is projected from window/pane active flags with the
    /// first session focused (an attached-session projection is client state
    /// we do not read).
    fn snapshot(&self) -> Result<MuxSnapshot, MuxError> {
        let sessions_text = self.run(&["list-sessions", "-F", SESSION_FORMAT])?;
        let windows_text = self.run(&["list-windows", "-a", "-F", WINDOW_FORMAT])?;
        let panes_text = self.run(&["list-panes", "-a", "-F", PANE_FORMAT])?;

        let mut workspaces = Vec::new();
        for line in sessions_text.lines().filter(|line| !line.is_empty()) {
            let fields: Vec<&str> = line.split('|').collect();
            let (Some(session_id), Some(name)) = (fields.first(), fields.get(1)) else {
                continue;
            };
            workspaces.push(Workspace {
                workspace_id: (*session_id).to_string(),
                label: Some((*name).to_string()),
                cwd: fields.get(2).map(|cwd| (*cwd).to_string()),
                agent_status: None,
                active_tab_id: None,
                // One focused Workspace keeps the Herdr-shaped projection
                // invariant ("exactly one focused") without reading tmux's
                // per-client attach state.
                focused: workspaces.is_empty(),
                tab_count: fields.get(4).and_then(|count| count.parse().ok()),
                pane_count: None,
                number: None,
            });
        }
        let session_ids: HashSet<String> = workspaces
            .iter()
            .map(|workspace| workspace.workspace_id.clone())
            .collect();

        let mut tabs = Vec::new();
        for line in windows_text.lines().filter(|line| !line.is_empty()) {
            let fields: Vec<&str> = line.split('|').collect();
            let (Some(window_id), Some(session_id), Some(name)) =
                (fields.first(), fields.get(1), fields.get(2))
            else {
                continue;
            };
            if !session_ids.contains(*session_id) {
                continue;
            }
            let active = fields.get(3).map(|flag| *flag == "1").unwrap_or(false);
            tabs.push(Tab {
                tab_id: (*window_id).to_string(),
                workspace_id: Some((*session_id).to_string()),
                label: Some((*name).to_string()),
                title: None,
                terminal_title: None,
                agent_status: None,
                pane_count: None,
                focused: active,
            });
        }
        for workspace in &mut workspaces {
            workspace.active_tab_id = tabs
                .iter()
                .find(|tab| tab.workspace_id.as_deref() == Some(&workspace.workspace_id))
                .map(|tab| tab.tab_id.clone());
        }

        let mut panes = Vec::new();
        for line in panes_text.lines().filter(|line| !line.is_empty()) {
            let fields: Vec<&str> = line.split('|').collect();
            let (Some(pane_id), Some(session_id), Some(window_id)) =
                (fields.first(), fields.get(1), fields.get(2))
            else {
                continue;
            };
            if !session_ids.contains(*session_id) {
                continue;
            }
            let pane_active = fields.get(4).map(|flag| *flag == "1").unwrap_or(false);
            let window_active = fields.get(9).map(|flag| *flag == "1").unwrap_or(false);
            let pane_cmd = fields.get(6).copied();
            let pane_title = fields.get(7).copied();
            let (detected_agent, detected_status) =
                crate::agent_hooks::sniff_agent_from_process_and_title(pane_cmd, pane_title);
            panes.push(Pane {
                pane_id: (*pane_id).to_string(),
                terminal_id: None,
                workspace_id: Some((*session_id).to_string()),
                tab_id: Some((*window_id).to_string()),
                label: None,
                title: pane_title.map(|title| (*title).to_string()),
                terminal_title: None,
                cwd: fields.get(3).map(|cwd| (*cwd).to_string()),
                agent_status: detected_status,
                agent: detected_agent,
                focused: pane_active && window_active,
                scroll: None,
            });
        }

        Ok(MuxSnapshot {
            workspaces,
            tabs,
            panes,
        })
    }

    fn state_from(snapshot: MuxSnapshot) -> crate::herdr::HerdrState {
        crate::herdr::HerdrState {
            focused_workspace_id: snapshot
                .workspaces
                .iter()
                .find(|workspace| workspace.focused)
                .map(|workspace| workspace.workspace_id.clone()),
            focused_tab_id: snapshot
                .tabs
                .iter()
                .find(|tab| tab.focused)
                .map(|tab| tab.tab_id.clone()),
            focused_pane_id: snapshot
                .panes
                .iter()
                .find(|pane| pane.focused)
                .map(|pane| pane.pane_id.clone()),
            workspaces: snapshot.workspaces,
            tabs: snapshot.tabs,
            panes: snapshot.panes.clone(),
            agents: snapshot
                .panes
                .into_iter()
                .filter_map(|pane| {
                    let agent_name = pane.agent?;
                    Some(Agent {
                        terminal_id: pane.pane_id.clone(),
                        agent: Some(agent_name),
                        workspace_id: pane.workspace_id,
                        tab_id: pane.tab_id,
                        pane_id: Some(pane.pane_id),
                        focused: pane.focused,
                        agent_status: pane.agent_status,
                        cwd: pane.cwd,
                        ..Default::default()
                    })
                })
                .collect(),
            layouts: Vec::new(),
            protocol: None,
            version: None,
        }
    }

    fn pane_layout_for(&self, pane_id: &str) -> Result<PaneLayout, MuxError> {
        let format = "#{pane_id}|#{session_id}|#{window_id}|#{pane_left}|#{pane_top}|#{pane_width}|#{pane_height}|#{pane_active}";
        let text = self.run(&["list-panes", "-a", "-F", format])?;
        for line in text.lines().filter(|line| !line.is_empty()) {
            let fields: Vec<&str> = line.split('|').collect();
            if fields.first().copied() != Some(pane_id) {
                continue;
            }
            let (Some(session_id), Some(window_id)) = (fields.get(1), fields.get(2)) else {
                continue;
            };
            let rect = crate::herdr::LayoutRect {
                x: fields
                    .get(3)
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0),
                y: fields
                    .get(4)
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0),
                width: fields
                    .get(5)
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0),
                height: fields
                    .get(6)
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0),
            };
            let focused = fields.get(7).map(|flag| *flag == "1").unwrap_or(false);
            return Ok(PaneLayout {
                tab_id: (*window_id).to_string(),
                workspace_id: Some((*session_id).to_string()),
                area: rect,
                panes: vec![crate::herdr::LayoutPane {
                    pane_id: pane_id.to_string(),
                    rect,
                    focused,
                }],
                splits: Vec::new(),
                focused_pane_id: Some(pane_id.to_string()),
                zoomed: false,
            });
        }
        Err(MuxError::NotFound(format!("pane {pane_id} not found")))
    }

    fn result_layout(&self, pane_id: &str) -> Result<PaneLayoutActionResult, MuxError> {
        Ok(PaneLayoutActionResult {
            layout: self.pane_layout_for(pane_id)?,
        })
    }

    fn pane_by_id(&self, pane_id: &str) -> Result<Pane, MuxError> {
        self.snapshot()?
            .panes
            .into_iter()
            .find(|pane| pane.pane_id == pane_id)
            .ok_or_else(|| MuxError::NotFound(format!("pane {pane_id} not found")))
    }

    fn direction_flag(direction: MuxDirection) -> &'static str {
        match direction {
            MuxDirection::Left => "-L",
            MuxDirection::Right => "-R",
            MuxDirection::Up => "-U",
            MuxDirection::Down => "-D",
        }
    }

    fn open_stream(
        &self,
        session: &str,
        cols: u16,
        rows: u16,
    ) -> Result<Arc<TmuxAttachStream>, TuiError> {
        let mut guard = self
            .stream
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(existing) = guard.as_ref() {
            if existing.is_running() {
                return Ok(existing.clone());
            }
        }
        let stream = TmuxAttachStream::spawn(self.tmux_socket.as_deref(), session, cols, rows)?;
        *guard = Some(stream.clone());
        Ok(stream)
    }
}

struct MuxSnapshot {
    workspaces: Vec<Workspace>,
    tabs: Vec<Tab>,
    panes: Vec<Pane>,
}

const SESSION_FORMAT: &str =
    "#{session_id}|#{session_name}|#{session_path}|#{session_attached}|#{session_windows}";
const WINDOW_FORMAT: &str =
    "#{window_id}|#{session_id}|#{window_name}|#{window_active}|#{window_index}";
// window_active (the session's current window) is repeated onto every pane so
// the focused-pane projection can require BOTH flags: pane_active alone is
// window-scoped and every window's active pane matches, which made the first
// window's pane win the projection and detached the layout probe from the
// window the attach client actually views and resizes.
const PANE_FORMAT: &str = "#{pane_id}|#{session_id}|#{window_id}|#{pane_current_path}|#{pane_active}|#{pane_pid}|#{pane_current_command}|#{pane_title}|#{pane_tty}|#{window_active}";

impl MultiplexerConnection for TmuxConnection {
    fn capabilities(&self) -> MuxCapabilities {
        TmuxBackend.capabilities()
    }

    fn ping(&self) -> Result<(), MuxError> {
        self.run(&["list-sessions"]).map(|_| ())
    }

    fn protocol(&self) -> Option<u32> {
        None
    }

    fn server_started_with_supplied_config(&self) -> bool {
        false
    }

    fn navigation_state(&self) -> Result<NavigationState, MuxError> {
        let snapshot = self.snapshot()?;
        Ok(NavigationState {
            focused_workspace_id: snapshot
                .workspaces
                .iter()
                .find(|workspace| workspace.focused)
                .map(|workspace| workspace.workspace_id.clone()),
            focused_tab_id: snapshot
                .tabs
                .iter()
                .find(|tab| tab.focused)
                .map(|tab| tab.tab_id.clone()),
            workspaces: snapshot.workspaces,
            tabs: snapshot.tabs,
        })
    }

    fn visible_state(&self) -> Result<crate::herdr::HerdrState, MuxError> {
        Ok(Self::state_from(self.snapshot()?))
    }

    fn host_bootstrap_state(&self) -> Result<crate::herdr::HerdrState, MuxError> {
        self.visible_state()
    }

    fn workspace_state(&self) -> Result<crate::herdr::HerdrState, MuxError> {
        self.visible_state()
    }

    fn workspace_panes(&self, workspace_id: &str) -> Result<Vec<Pane>, MuxError> {
        Ok(self
            .snapshot()?
            .panes
            .into_iter()
            .filter(|pane| pane.workspace_id.as_deref() == Some(workspace_id))
            .collect())
    }

    fn tab_surface_state(
        &self,
        workspace_id: &str,
        tab_id: &str,
    ) -> Result<TabSurfaceState, MuxError> {
        let snapshot = self.snapshot()?;
        let panes: Vec<Pane> = snapshot
            .panes
            .into_iter()
            .filter(|pane| {
                pane.workspace_id.as_deref() == Some(workspace_id)
                    && pane.tab_id.as_deref() == Some(tab_id)
            })
            .collect();
        let focused_pane_id = panes
            .iter()
            .find(|pane| pane.focused)
            .or_else(|| panes.first())
            .map(|pane| pane.pane_id.clone());
        let layout = focused_pane_id
            .as_deref()
            .map(|pane_id| self.pane_layout_for(pane_id))
            .transpose()?;
        Ok(TabSurfaceState {
            workspace_id: workspace_id.to_string(),
            tab_id: tab_id.to_string(),
            focused_pane_id,
            panes,
            layouts: layout.into_iter().collect(),
        })
    }

    fn pane_layout(&self, pane_id: &str) -> Result<PaneLayout, MuxError> {
        self.pane_layout_for(pane_id)
    }

    fn agents(&self) -> Result<Vec<Agent>, MuxError> {
        let snapshot = self.snapshot()?;
        let agents = snapshot
            .panes
            .into_iter()
            .filter_map(|pane| {
                let agent_name = pane.agent?;
                Some(Agent {
                    terminal_id: pane.pane_id.clone(),
                    agent: Some(agent_name),
                    workspace_id: pane.workspace_id,
                    tab_id: pane.tab_id,
                    pane_id: Some(pane.pane_id),
                    focused: pane.focused,
                    agent_status: pane.agent_status,
                    cwd: pane.cwd,
                    ..Default::default()
                })
            })
            .collect();
        Ok(agents)
    }

    /// `events_push = false`: typed degradation, never a fake live stream.
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
        params: &CreateWorkspace<'_>,
    ) -> Result<WorkspaceCreatedResult, MuxError> {
        let mut args = vec![
            "new-session".to_string(),
            "-d".to_string(),
            "-P".to_string(),
            "-F".to_string(),
            "#{session_id}|#{window_id}|#{pane_id}".to_string(),
        ];
        if let Some(cwd) = params.cwd {
            args.push("-c".to_string());
            args.push(cwd.to_string());
        }
        let text = self.run(&args.iter().map(String::as_str).collect::<Vec<_>>())?;
        let ids: Vec<&str> = text.trim().split('|').collect();
        let (Some(session_id), Some(window_id), Some(pane_id)) =
            (ids.first(), ids.get(1), ids.get(2))
        else {
            return Err(MuxError::Api(format!(
                "tmux new-session printed no ids: {text:?}"
            )));
        };
        Ok(WorkspaceCreatedResult {
            workspace: Workspace {
                workspace_id: (*session_id).to_string(),
                label: None,
                cwd: params.cwd.map(str::to_string),
                agent_status: None,
                active_tab_id: Some((*window_id).to_string()),
                focused: params.focus,
                tab_count: Some(1),
                pane_count: Some(1),
                number: None,
            },
            tab: Tab {
                tab_id: (*window_id).to_string(),
                workspace_id: Some((*session_id).to_string()),
                label: None,
                title: None,
                terminal_title: None,
                agent_status: None,
                pane_count: Some(1),
                focused: false,
            },
            root_pane: Pane {
                pane_id: (*pane_id).to_string(),
                terminal_id: None,
                tab_id: Some((*window_id).to_string()),
                workspace_id: Some((*session_id).to_string()),
                ..Pane::default()
            },
        })
    }

    fn close_workspace(&self, workspace_id: &str) -> Result<(), MuxError> {
        self.run(&["kill-session", "-t", workspace_id]).map(|_| ())
    }

    fn rename_workspace(&self, workspace_id: &str, label: &str) -> Result<(), MuxError> {
        self.run(&["rename-session", "-t", workspace_id, label])
            .map(|_| ())
    }

    /// tmux sessions have no order primitive; the neutral contract degrades.
    fn move_workspace(&self, _workspace_id: &str, _insert_index: usize) -> Result<(), MuxError> {
        Err(MuxError::Unsupported("workspace_reorder"))
    }

    fn move_workspace_before(
        &self,
        _workspace_id: &str,
        _before_workspace_id: &str,
    ) -> Result<(), MuxError> {
        Err(MuxError::Unsupported("workspace_reorder"))
    }

    /// Focus switches the `tmux attach` client to the session. Without an
    /// attached client the switch is a server-side no-op, which is the
    /// honest effect of focusing a session nobody is viewing.
    fn workspace_focus(&self, workspace_id: &str) -> Result<(), MuxError> {
        match self.run(&["switch-client", "-t", workspace_id]) {
            Ok(_) => Ok(()),
            Err(MuxError::Api(detail))
                if detail.contains("no current client") || detail.contains("can't establish") =>
            {
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn create_tab(&self, params: &CreateTab<'_>) -> Result<TabCreatedResult, MuxError> {
        // A missing workspace target resolves to the first session (the
        // snapshot's focused projection), mirroring "create in the bound
        // Project's instance".
        let workspace_id = match params.workspace_id {
            Some(workspace_id) => workspace_id.to_string(),
            None => self
                .snapshot()?
                .workspaces
                .first()
                .map(|workspace| workspace.workspace_id.clone())
                .ok_or_else(|| MuxError::Api("no tmux sessions exist".to_string()))?,
        };
        let mut args = vec![
            "new-window".to_string(),
            "-t".to_string(),
            workspace_id.clone(),
            "-P".to_string(),
            "-F".to_string(),
            "#{window_id}|#{pane_id}".to_string(),
        ];
        if let Some(cwd) = params.cwd {
            args.push("-c".to_string());
            args.push(cwd.to_string());
        }
        let text = self.run(&args.iter().map(String::as_str).collect::<Vec<_>>())?;
        let ids: Vec<&str> = text.trim().split('|').collect();
        let (Some(window_id), Some(pane_id)) = (ids.first(), ids.get(1)) else {
            return Err(MuxError::Api(format!(
                "tmux new-window printed no ids: {text:?}"
            )));
        };
        Ok(TabCreatedResult {
            tab: Tab {
                tab_id: (*window_id).to_string(),
                workspace_id: Some(workspace_id),
                label: None,
                title: None,
                terminal_title: None,
                agent_status: None,
                pane_count: Some(1),
                focused: false,
            },
            root_pane: Pane {
                pane_id: (*pane_id).to_string(),
                terminal_id: None,
                tab_id: Some((*window_id).to_string()),
                workspace_id: None,
                ..Pane::default()
            },
        })
    }

    fn close_tab(&self, tab_id: &str) -> Result<(), MuxError> {
        self.run(&["kill-window", "-t", tab_id]).map(|_| ())
    }

    fn rename_tab(&self, tab_id: &str, label: &str) -> Result<(), MuxError> {
        self.run(&["rename-window", "-t", tab_id, label])
            .map(|_| ())
    }

    /// Within-session reorder via target index; a occupied index is a typed
    /// error (tmux has no insert primitive).
    fn move_tab(&self, tab_id: &str, insert_index: usize) -> Result<(), MuxError> {
        let session_id = self
            .snapshot()?
            .tabs
            .into_iter()
            .find(|tab| tab.tab_id == tab_id)
            .and_then(|tab| tab.workspace_id)
            .ok_or_else(|| MuxError::NotFound(format!("tab {tab_id} not found")))?;
        self.run(&[
            "move-window",
            "-s",
            tab_id,
            "-t",
            &format!("{session_id}:{insert_index}"),
        ])
        .map(|_| ())
    }

    fn tab_focus(&self, tab_id: &str) -> Result<(), MuxError> {
        self.run(&["select-window", "-t", tab_id]).map(|_| ())
    }

    fn split_pane(&self, pane_id: &str, direction: SplitDirection) -> Result<Pane, MuxError> {
        let flag = match direction {
            SplitDirection::Right => "-h",
            SplitDirection::Down => "-v",
        };
        let text = self.run(&[
            "split-window",
            flag,
            "-t",
            pane_id,
            "-P",
            "-F",
            "#{pane_id}",
        ])?;
        let created = text.trim();
        self.pane_by_id(created)
    }

    fn close_pane(&self, pane_id: &str) -> Result<(), MuxError> {
        self.run(&["kill-pane", "-t", pane_id]).map(|_| ())
    }

    fn rename_pane(&self, pane_id: &str, label: &str) -> Result<(), MuxError> {
        self.run(&["select-pane", "-t", pane_id, "-T", label])
            .map(|_| ())
    }

    fn swap_pane(
        &self,
        pane_id: &str,
        direction: MuxDirection,
    ) -> Result<PaneLayoutActionResult, MuxError> {
        // tmux's swap primitive is vertical (-U/-D); horizontal swaps have no
        // direction form and degrade instead of guessing a target pane.
        match direction {
            MuxDirection::Up | MuxDirection::Down => {
                self.run(&["swap-pane", Self::direction_flag(direction), "-t", pane_id])?;
            }
            MuxDirection::Left | MuxDirection::Right => {
                return Err(MuxError::Unsupported("swap_pane_horizontal"));
            }
        }
        self.result_layout(pane_id)
    }

    fn resize_pane(
        &self,
        pane_id: &str,
        direction: MuxDirection,
    ) -> Result<PaneLayoutActionResult, MuxError> {
        self.run(&[
            "resize-pane",
            Self::direction_flag(direction),
            "-t",
            pane_id,
            "5",
        ])?;
        self.result_layout(pane_id)
    }

    fn toggle_pane_zoom(&self, pane_id: &str) -> Result<PaneLayoutActionResult, MuxError> {
        self.run(&["resize-pane", "-Z", "-t", pane_id])?;
        self.result_layout(pane_id)
    }

    fn pane_focus(&self, pane_id: &str) -> Result<(), MuxError> {
        self.run(&["select-pane", "-t", pane_id]).map(|_| ())
    }

    fn move_pane_to_tab(&self, pane_id: &str, tab_id: &str) -> Result<PaneMoveResult, MuxError> {
        self.run(&["join-pane", "-s", pane_id, "-t", tab_id])?;
        Ok(PaneMoveResult {
            pane: self.pane_by_id(pane_id)?,
            target_layout: self.pane_layout_for(pane_id)?,
        })
    }

    /// tmux's `break-pane` only creates a window inside the pane's own
    /// session; cross-session new-tab moves degrade in the MVP.
    fn move_pane_to_new_tab(
        &self,
        _pane_id: &str,
        _workspace_id: &str,
    ) -> Result<PaneMoveResult, MuxError> {
        Err(MuxError::Unsupported("move_pane_to_new_tab"))
    }

    fn set_split_ratio(&self, _tab_id: &str, _path: &[bool], _ratio: f64) -> Result<(), MuxError> {
        Err(MuxError::Unsupported("set_split_ratio"))
    }

    fn send_text(&self, pane_id: &str, text: &str) -> Result<(), MuxError> {
        self.run(&["send-keys", "-t", pane_id, "-l", text])
            .map(|_| ())
    }

    fn send_keys(&self, pane_id: &str, keys: &[String]) -> Result<(), MuxError> {
        if keys.is_empty() {
            return Ok(());
        }
        let mut args = vec![
            "send-keys".to_string(),
            "-t".to_string(),
            pane_id.to_string(),
        ];
        args.extend(keys.iter().cloned());
        self.run(&args.iter().map(String::as_str).collect::<Vec<_>>())
            .map(|_| ())
    }

    fn pane_process_info(&self, pane_id: &str) -> Result<PaneProcessInfo, MuxError> {
        let format =
            "#{pane_id}|#{pane_pid}|#{pane_current_command}|#{pane_current_path}|#{pane_tty}";
        let text = self.run(&["list-panes", "-a", "-F", format])?;
        for line in text.lines().filter(|line| !line.is_empty()) {
            let fields: Vec<&str> = line.split('|').collect();
            if fields.first().copied() != Some(pane_id) {
                continue;
            }
            return Ok(PaneProcessInfo {
                pane_id: pane_id.to_string(),
                shell_pid: fields.get(1).and_then(|pid| pid.parse().ok()),
                tty: fields.get(4).map(|tty| (*tty).to_string()),
                foreground_process_group_id: None,
                foreground_processes: fields
                    .get(2)
                    .map(|name| {
                        vec![PaneProcessInfoProcess {
                            pid: fields.get(1).and_then(|pid| pid.parse().ok()).unwrap_or(0),
                            name: (*name).to_string(),
                            argv: None,
                            argv0: None,
                            cmdline: None,
                            cwd: fields.get(3).map(|cwd| (*cwd).to_string()),
                        }]
                    })
                    .unwrap_or_default(),
            });
        }
        Err(MuxError::NotFound(format!("pane {pane_id} not found")))
    }

    fn read_pane_history(&self, pane_id: &str, lines: u32) -> Result<PaneHistory, MuxError> {
        // -e keeps escape sequences: the seam contract is ANSI text, matching
        // the Herdr adapter's retained-ANSI read.
        let text = self.run(&[
            "capture-pane",
            "-t",
            pane_id,
            "-p",
            "-e",
            "-S",
            &format!("-{lines}"),
        ])?;
        Ok(PaneHistory {
            pane_id: pane_id.to_string(),
            text,
        })
    }

    /// Domain 6: the per-instance `tmux attach` child stream.
    fn open_shared_session(
        &self,
        key: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Result<Arc<dyn MultiplexerStream>, MuxError> {
        let session = match key {
            Some(session) => session.to_string(),
            None => self
                .snapshot()?
                .workspaces
                .first()
                .map(|workspace| workspace.workspace_id.clone())
                .ok_or_else(|| MuxError::Api("no tmux sessions exist".to_string()))?,
        };
        self.open_stream(&session, cols, rows)
            .map(|stream| stream as Arc<dyn MultiplexerStream>)
            .map_err(|error| MuxError::Api(error.to_string()))
    }

    fn agent_runtime(&self) -> Option<&dyn MuxAgentRuntime> {
        None
    }
}

// --- Domain 6 — the `tmux attach` child stream ---

/// One `tmux attach` child on a private PTY: the same hosted-TUI-child shape
/// as the Herdr stream (byte fan-out, PTY input, grid resize), so the GUI
/// input path and the Remote TUI routes work unchanged.
pub struct TmuxAttachStream {
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

impl TmuxAttachStream {
    fn spawn(
        socket: Option<&Path>,
        session: &str,
        cols: u16,
        rows: u16,
    ) -> Result<Arc<Self>, TuiError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TuiError::Spawn(format!("open attach pty: {error}")))?;
        let tmux =
            which_tmux().ok_or_else(|| TuiError::Spawn("tmux is not installed".to_string()))?;
        let mut builder = CommandBuilder::new(tmux);
        builder.arg("-u");
        if let Some(socket) = socket {
            builder.arg("-S");
            builder.arg(socket);
        }
        builder.arg("attach");
        builder.arg("-t");
        builder.arg(session);
        builder.env("TERM", "xterm-256color");
        let lang = std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".to_string());
        let lang = if lang.to_lowercase().contains("utf") {
            lang
        } else {
            "en_US.UTF-8".to_string()
        };
        builder.env("LANG", &lang);
        builder.env("LC_ALL", &lang);
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
            id: format!("tmux-attach-{}", uuid::Uuid::new_v4()),
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
            .name("tmux-attach-reader".to_string())
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

fn which_tmux() -> Option<PathBuf> {
    tmux_path()
}

impl MultiplexerStream for TmuxAttachStream {
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

    /// The attach child repaints its full screen on start, so a late viewer
    /// needs no startup byte replay (unlike the Herdr TUI's DECSET state).
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

    /// The shared-TUI SIGWINCH nudge: a one-row shrink/restore makes tmux
    /// repaint its whole screen.
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
        let mut guard = self
            .child
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match guard.as_mut() {
            Some(child) => child.kill().is_ok(),
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

impl TmuxAttachStream {
    /// Input bytes for one pane read path used by the kit stream contract.
    #[cfg(test)]
    fn wait_for_output(&self, needle: &str, timeout: Duration) -> bool {
        let mut rx = self.events.subscribe();
        let started = Instant::now();
        while started.elapsed() < timeout {
            match rx.try_recv() {
                Ok(super::StreamEvent::Output { bytes, .. }) => {
                    if let Ok(text) = String::from_utf8(bytes) {
                        if text.contains(needle) {
                            return true;
                        }
                    }
                }
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return false,
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mux::registry::MuxRegistry;
    use std::sync::atomic::AtomicU32;

    static SCRATCH: AtomicU32 = AtomicU32::new(0);

    /// A throwaway tmux server on a dedicated socket — never the user's
    /// default server (docs/multiplexer-api.md §9.2 isolation rule).
    struct TestTmuxServer {
        socket: PathBuf,
        dir: PathBuf,
    }

    impl TestTmuxServer {
        fn start() -> Option<Self> {
            tmux_path()?;
            let dir = std::env::temp_dir().join(format!(
                "shardlane-mux-tmux-{}-{}",
                std::process::id(),
                SCRATCH.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).ok()?;
            let socket = dir.join("kit.tmux.sock");
            let output = Command::new(TMUX_BIN)
                .arg("-S")
                .arg(&socket)
                .arg("-f")
                .arg("/dev/null")
                .args([
                    "new-session",
                    "-d",
                    "-s",
                    "kit",
                    "-c",
                    &dir.display().to_string(),
                ])
                .stdin(Stdio::null())
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            Some(Self { socket, dir })
        }

        fn connection(&self) -> Arc<dyn MultiplexerConnection> {
            Arc::new(TmuxConnection::with_socket(self.socket.clone()))
        }
    }

    impl Drop for TestTmuxServer {
        fn drop(&mut self) {
            let _ = Command::new(TMUX_BIN)
                .arg("-S")
                .arg(&self.socket)
                .args(["kill-server"])
                .output();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn server() -> TestTmuxServer {
        match TestTmuxServer::start() {
            Some(server) => server,
            None => {
                eprintln!("tmux unavailable; skipping tmux adapter contract");
                std::process::exit(0);
            }
        }
    }

    #[test]
    fn tmux_backend_capabilities_degrade_typed() {
        let backend = TmuxBackend;
        assert_eq!(backend.id(), "tmux");
        let caps = backend.capabilities();
        assert!(!caps.agents && !caps.server_admin && !caps.events_push);
        assert!(!caps.cross_workspace_tab_move);
        assert!(caps.shared_tui && caps.pane_history_read);
        kit::backend_contract(&backend);
    }

    #[test]
    fn tmux_connection_satisfies_snapshot_invariants() {
        let server = server();
        let connection = server.connection();
        let state = connection
            .visible_state()
            .unwrap_or_else(|e| panic!("state: {e}"));
        assert!(!state.workspaces.is_empty());
        for tab in &state.tabs {
            assert!(
                state
                    .workspaces
                    .iter()
                    .any(|workspace| Some(&workspace.workspace_id) == tab.workspace_id.as_ref()),
                "tab {} references a missing session",
                tab.tab_id
            );
        }
        for pane in &state.panes {
            assert!(
                state
                    .tabs
                    .iter()
                    .any(|tab| Some(&tab.tab_id) == pane.tab_id.as_ref()),
                "pane {} references a missing window",
                pane.pane_id
            );
            assert!(
                pane.cwd.is_some(),
                "pane cwd projected from pane_current_path"
            );
        }
        kit::connection_contract(connection.as_ref(), TmuxBackend.capabilities());
    }

    #[test]
    fn tmux_live_structural_round_trip() {
        let server = server();
        let connection = server.connection();
        if let Err(error) = kit::live_structural_contract(connection.as_ref()) {
            panic!("live contract failed: {error}");
        }
    }

    #[test]
    fn tmux_default_target_resolves_through_socket_override() {
        // The isolated-testing seam (SHARDLANE_TMUX_SOCKET) must steer the
        // Default bind path too, not just explicit-socket binds and tests;
        // otherwise acceptance runtimes silently attach to the user's real
        // tmux server (docs/multiplexer-api.md §10 Phase 5 regression).
        let Some(server) = TestTmuxServer::start() else {
            eprintln!("tmux unavailable; skipping tmux adapter contract");
            return;
        };
        let guard = EnvGuard::set("SHARDLANE_TMUX_SOCKET", &server.socket);
        let backend = TmuxBackend;
        let connection = backend
            .connect_instance(&InstanceRef::default_instance(backend.id()))
            .unwrap_or_else(|e| panic!("default bind via override failed: {e}"));
        let state = connection
            .visible_state()
            .unwrap_or_else(|e| panic!("state: {e}"));
        assert!(
            state
                .workspaces
                .iter()
                .any(|workspace| workspace.label.as_deref() == Some("kit")),
            "Default bind must see the override server's kit session, got {:?}",
            state
                .workspaces
                .iter()
                .filter_map(|workspace| workspace.label.as_deref())
                .collect::<Vec<_>>()
        );
        drop(connection);
        guard.restore();
    }

    /// Scoped env var setter that restores the previous value on `restore`
    /// (single-threaded discipline: only this test touches this variable).
    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &Path) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }

        fn restore(self) {
            match self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn tmux_focused_pane_tracks_the_session_current_window() {
        // Regression: pane_active alone is window-scoped, so with a second
        // (current) window the FIRST window's pane won the focused projection.
        // The GUI's chrome probe then measured an un-viewed, never-resized
        // pane and its compensation loop grew the hosted grid without bound.
        let Some(server) = TestTmuxServer::start() else {
            eprintln!("tmux unavailable; skipping tmux adapter contract");
            return;
        };
        // Second window becomes the session's current window (tmux default).
        let _ = Command::new(TMUX_BIN)
            .arg("-S")
            .arg(&server.socket)
            .args(["new-window", "-t", "kit", "-n", "build"])
            .stdin(Stdio::null())
            .output();
        let connection = server.connection();
        let state = connection
            .visible_state()
            .unwrap_or_else(|e| panic!("state: {e}"));
        let focused_pane = state
            .panes
            .iter()
            .find(|pane| pane.focused)
            .unwrap_or_else(|| panic!("no focused pane"));
        let focused_tab_id = state
            .tabs
            .iter()
            .find(|tab| tab.focused)
            .map(|tab| tab.tab_id.clone())
            .unwrap_or_default();
        assert_eq!(
            focused_pane.tab_id.as_deref(),
            Some(focused_tab_id.as_str()),
            "the focused pane must live in the session's current window, got pane {:?} in tab {:?} while focused tab is {:?}",
            focused_pane.pane_id,
            focused_pane.tab_id,
            focused_tab_id
        );
    }

    #[test]
    fn tmux_pane_operations_round_trip() {
        let server = server();
        let connection = server.connection();
        let state = connection
            .visible_state()
            .unwrap_or_else(|e| panic!("state: {e}"));
        let pane_id = state.panes[0].pane_id.clone();

        let split = connection
            .split_pane(&pane_id, SplitDirection::Right)
            .unwrap_or_else(|e| panic!("split: {e}"));
        connection
            .send_text(&split.pane_id, "echo mux-kit\n")
            .unwrap_or_else(|e| panic!("send text: {e}"));
        connection
            .resize_pane(&pane_id, MuxDirection::Right)
            .unwrap_or_else(|e| panic!("resize: {e}"));
        let zoomed = connection
            .toggle_pane_zoom(&pane_id)
            .unwrap_or_else(|e| panic!("zoom: {e}"));
        let pane_tab = state
            .panes
            .iter()
            .find(|pane| pane.pane_id == pane_id)
            .and_then(|pane| pane.tab_id.clone())
            .unwrap_or_default();
        assert_eq!(zoomed.layout.tab_id, pane_tab);
        connection
            .rename_pane(&split.pane_id, "kit-pane")
            .unwrap_or_else(|e| panic!("rename pane: {e}"));
        let info = connection
            .pane_process_info(&split.pane_id)
            .unwrap_or_else(|e| panic!("pane after rename: {e}"));
        assert!(
            !info.pane_id.is_empty(),
            "renamed pane must remain addressable"
        );

        let history = connection
            .read_pane_history(&split.pane_id, 50)
            .unwrap_or_else(|e| panic!("history: {e}"));
        assert!(
            history.text.contains("mux-kit"),
            "capture-pane must echo the sent text"
        );

        connection
            .close_pane(&split.pane_id)
            .unwrap_or_else(|e| panic!("close pane: {e}"));
    }

    #[test]
    fn tmux_attach_stream_carries_input_and_resizes() {
        let server = server();
        let connection = server.connection();
        let state = connection
            .visible_state()
            .unwrap_or_else(|e| panic!("state: {e}"));
        let session = state.workspaces[0].workspace_id.clone();

        let stream = TmuxAttachStream::spawn(Some(&server.socket), &session, 80, 24)
            .unwrap_or_else(|e| panic!("spawn attach: {e}"));
        assert!(stream.is_running());
        stream
            .send_bytes(b"echo mux-kit-stream\r")
            .unwrap_or_else(|e| panic!("send bytes: {e}"));
        assert!(
            stream.wait_for_output("mux-kit-stream", Duration::from_secs(10)),
            "attach stream must carry the pane output"
        );
        stream
            .resize(100, 30)
            .unwrap_or_else(|e| panic!("resize: {e}"));
        assert_eq!(stream.summary().cols, 100);
        assert!(stream.stop_with_reap(Some(Duration::from_secs(5))));
    }

    #[test]
    fn tmux_instances_surface_in_registry_aggregation() {
        let registry = MuxRegistry::with_builtins();
        let ids: Vec<&'static str> = registry.backends().iter().map(|b| b.id()).collect();
        assert_eq!(ids, vec!["herdr", "tmux", "uuyc", "luvus"]);
    }
}
