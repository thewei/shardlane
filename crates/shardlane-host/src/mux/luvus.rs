//! Luvus backend adapter (https://github.com/RizRiyz/luvus): implements the
//! neutral crate::mux contract over Luvus's UHP 1.0 wire protocol —
//! newline-delimited JSON-RPC on a per-session Unix socket — plus the luvus
//! TUI attach child for the shared terminal stream.
//!
//! [INPUT]: depends on serde_json (UHP RPC envelopes), UnixStream (socket
//! transport), portable-pty (attach child PTY), tokio broadcast (output
//! fan-out), crate::herdr::{resolve_user_cli, host_agent_status} (shared CLI
//! discovery and status vocabulary)
//! [OUTPUT]: exposes LuvusBackend / LuvusServerAdmin / LuvusClient
//! (MultiplexerConnection + MuxAgentRuntime) / LuvusAttachStream
//! (MultiplexerStream) and luvus_cli_path()
//! [POS]: the luvus adapter of the shardlane-host mux, peer of
//! herdr.rs/tmux.rs/uuyc.rs, assembled by registry.rs; the GUI binds named
//! sessions through the backend id "luvus"
//! [PROTOCOL]: Update this header on change, then check CLAUDE.md.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;

use super::{
    CreateTab, CreateWorkspace, InstanceListing, InstanceRef, InstanceTarget, Multiplexer,
    MultiplexerConnection, MultiplexerServerAdmin, MultiplexerStream, MuxAgentRuntime,
    MuxCapabilities, MuxDirection, MuxError, MuxEvent, MuxState, PaneHistory, SplitDirection,
};
use crate::dto::{AgentStatus, HerdrTuiMode, HerdrTuiSessionStatus};
use crate::herdr::{
    host_agent_status, resolve_user_cli, Agent, HerdrEvent, LayoutPane, LayoutRect,
    NavigationState, Pane, PaneLayout, PaneLayoutActionResult, PaneMoveResult, PaneProcessInfo,
    PaneProcessInfoProcess, Tab, TabCreatedResult, TabSurfaceState, Workspace,
    WorkspaceCreatedResult,
};
use crate::ids::AgentRef;
use crate::runtime::{
    RuntimeAgent, RuntimeAgentPromptRequest, RuntimeAgentRead, RuntimeAgentReadRequest,
    RuntimeAgentStartRequest, RuntimeAgentWait, RuntimeAgentWaitRequest,
};
use crate::shared_tui::TuiError;

const LUVUS_BIN: &str = "luvus";
const OUTPUT_QUEUE_CAPACITY: usize = 4096;

// ---------------------------------------------------------------------------
// CLI / socket discovery
// ---------------------------------------------------------------------------

/// Resolve the luvus CLI: SHARDLANE_LUVUS_CLI_PATH override, then the shared
/// user-CLI ladder (process PATH -> login-shell PATH -> user bin dirs).
pub fn luvus_cli_path() -> Option<PathBuf> {
    if let Some(custom) = std::env::var_os("SHARDLANE_LUVUS_CLI_PATH").map(PathBuf::from) {
        if !custom.as_os_str().is_empty() && custom.is_file() {
            return Some(custom);
        }
        return None;
    }
    resolve_user_cli(LUVUS_BIN)
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The default session's server socket (luvus's default = the base socket).
fn default_socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("LUVUS_SOCKET_PATH").map(PathBuf::from) {
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    home_dir().join(".luvus/luvus.sock")
}

/// Named sessions live under ~/.luvus/sessions/<name>/luvus.sock (verified
/// against luvus 0.13.4 session list); the convention is the fallback when
/// the CLI enumeration is unavailable.
fn convention_session_socket_path(session: &str) -> PathBuf {
    home_dir()
        .join(".luvus/sessions")
        .join(session)
        .join("luvus.sock")
}

// --- CLI JSON contracts (verified against luvus 0.13.4 session list --json) ---

#[derive(Debug, Deserialize)]
struct LuvusSessionList {
    #[serde(default)]
    sessions: Vec<LuvusSessionRow>,
}

#[derive(Debug, Deserialize)]
struct LuvusSessionRow {
    name: String,
    #[serde(default)]
    running: bool,
    #[serde(default, rename = "default")]
    is_default: bool,
    #[serde(default)]
    socket_path: Option<String>,
}

fn luvus_session_rows() -> Option<Vec<LuvusSessionRow>> {
    let cli = luvus_cli_path()?;
    let output = Command::new(cli)
        .args(["session", "list", "--json"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let parsed: LuvusSessionList = serde_json::from_slice(&output.stdout).ok()?;
    Some(parsed.sessions)
}

/// Display-name override store (Shardlane-owned, mirrors the Herdr/tmux/uuyc
/// pattern: luvus itself has no session rename).
fn instance_metadata_path(name: &str) -> PathBuf {
    home_dir()
        .join(".config/shardlane/luvus-instances")
        .join(format!("{name}.json"))
}

fn read_display_name(name: &str) -> Option<String> {
    let text = std::fs::read_to_string(instance_metadata_path(name)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
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
    let body = json!({ "version": 1, "display_name": trimmed });
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&body).map_err(MuxError::Json)?,
    )
    .map_err(MuxError::Io)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// UHP RPC client
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LuvusResponse {
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<LuvusApiError>,
}

#[derive(Debug, Deserialize)]
struct LuvusApiError {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// One luvus UHP client: each RPC opens a fresh socket and reads exactly one
/// response line (the same one-request-per-connection shape as HerdrClient).
#[derive(Clone)]
pub struct LuvusClient {
    socket_path: PathBuf,
    runtime_version: Option<String>,
    protocol: Option<u32>,
}

impl LuvusClient {
    /// Test-only constructor: direct socket path, no ping round trip.
    pub fn for_test_socket(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            runtime_version: None,
            protocol: Some(1),
        }
    }

    /// Verify the socket answers and record protocol/version (construction
    /// time only; the trait ping stays read-only).
    fn handshake(&mut self) -> Result<(), MuxError> {
        let result = self.call("ping", json!({}))?;
        self.protocol = result
            .get("protocol")
            .and_then(Value::as_u64)
            .map(|value| value as u32);
        self.runtime_version = result
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok(())
    }

    /// Side-effect-free connect to the default session's socket.
    pub fn connect() -> Result<Self, MuxError> {
        Self::connect_to(&default_socket_path())
    }

    pub fn connect_to(socket_path: &Path) -> Result<Self, MuxError> {
        let mut client = Self {
            socket_path: socket_path.to_path_buf(),
            runtime_version: None,
            protocol: None,
        };
        client.handshake()?;
        Ok(client)
    }

    /// Start the default session's server when needed, then connect.
    pub fn bootstrap() -> Result<Self, MuxError> {
        Self::bootstrap_session(None)
    }

    /// Start a named session's server when needed, then connect. Named
    /// sessions are the per-instance seam (luvus --session <name>).
    pub fn bootstrap_for_session(session: &str) -> Result<Self, MuxError> {
        Self::bootstrap_session(Some(session))
    }

    fn bootstrap_session(session: Option<&str>) -> Result<Self, MuxError> {
        let cli = luvus_cli_path()
            .ok_or_else(|| MuxError::InstallFailed("luvus is not installed".to_string()))?;
        let mut command = Command::new(cli);
        if let Some(session) = session {
            command.args(["--session", session]);
        }
        command
            .args(["server", "start"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let status = command.status().map_err(MuxError::Io)?;
        if !status.success() {
            return Err(MuxError::InstallFailed(format!(
                "luvus server start exited with {status}"
            )));
        }
        let socket_path = match session {
            None => default_socket_path(),
            Some(session) => Self::session_socket_path(session),
        };
        let mut client = Self {
            socket_path,
            runtime_version: None,
            protocol: None,
        };
        // The server binds its socket asynchronously after spawn.
        let started = Instant::now();
        loop {
            match client.handshake() {
                Ok(()) => return Ok(client),
                Err(error) if started.elapsed() < Duration::from_secs(5) => {
                    let _ = error;
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Resolve the socket path for a named session without starting it.
    fn session_socket_path(session: &str) -> PathBuf {
        luvus_session_rows()
            .and_then(|rows| {
                rows.into_iter()
                    .find(|row| row.name == session)
                    .and_then(|row| row.socket_path.map(PathBuf::from))
            })
            .unwrap_or_else(|| convention_session_socket_path(session))
    }

    pub fn ping(&self) -> Result<(), MuxError> {
        self.call("ping", json!({})).map(|_| ())
    }

    /// One UHP request on a fresh socket. Read timeout: ordinary calls are
    /// milliseconds on a local socket; agent.wait blocks server-side.
    fn call(&self, method: &str, params: Value) -> Result<Value, MuxError> {
        let stream = UnixStream::connect(&self.socket_path).map_err(|err| {
            MuxError::SocketUnavailable(self.socket_path.display().to_string(), err.to_string())
        })?;
        stream.set_read_timeout(Some(rpc_read_timeout(method)))?;
        let request = json!({ "id": "shardlane", "method": method, "params": params });
        let mut reader = BufReader::new(stream);
        writeln!(reader.get_mut(), "{request}")?;
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Err(MuxError::Uncertain(format!(
                "connection closed before a response was read ({method})"
            )));
        }
        let response: LuvusResponse = serde_json::from_str(line.trim())?;
        if let Some(error) = response.error {
            let message = error.message.unwrap_or_else(|| "unknown error".to_string());
            return Err(match error.code.as_deref() {
                Some("not_found") => MuxError::NotFound(message),
                Some("timeout") => MuxError::Timeout(message),
                _ => MuxError::Api(message),
            });
        }
        response
            .result
            .ok_or_else(|| MuxError::Api(format!("{method} returned no result")))
    }

    // --- Projection sources ---

    fn list_workspaces(&self) -> Result<Vec<Workspace>, MuxError> {
        let result = self.call("workspace.list", json!({}))?;
        Ok(result
            .get("workspaces")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(parse_workspace).collect())
            .unwrap_or_default())
    }

    fn list_tabs(&self, workspace_id: &str) -> Result<Vec<Tab>, MuxError> {
        let result = self.call("tab.list", json!({ "workspace_id": workspace_id }))?;
        Ok(result
            .get("tabs")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|tab| parse_tab(tab, Some(workspace_id)))
                    .collect()
            })
            .unwrap_or_default())
    }

    fn fetch_pane(&self, pane_id: &str) -> Result<Pane, MuxError> {
        let result = self.call("pane.get", json!({ "pane": pane_id }))?;
        Ok(parse_pane(&result))
    }

    fn pane_layout_of(&self, pane_id: &str) -> Result<PaneLayout, MuxError> {
        let result = self.call("pane.layout", json!({ "pane": pane_id }))?;
        Ok(parse_layout(&result))
    }

    fn list_agents(&self) -> Result<Vec<Agent>, MuxError> {
        let result = self.call("agent.list", json!({}))?;
        Ok(result
            .get("agents")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(parse_agent).collect())
            .unwrap_or_default())
    }

    fn runtime_agent_for(&self, pane_id: &str) -> Result<RuntimeAgent, MuxError> {
        let pane = self.fetch_pane(pane_id)?;
        runtime_agent_from_pane(&pane)
    }

    fn navigation_state(&self) -> Result<NavigationState, MuxError> {
        let workspaces = self.list_workspaces()?;
        let mut tabs = Vec::new();
        for workspace in &workspaces {
            tabs.extend(self.list_tabs(&workspace.workspace_id)?);
        }
        let focused_workspace_id = workspaces
            .iter()
            .find(|workspace| workspace.focused)
            .map(|workspace| workspace.workspace_id.clone())
            .or_else(|| {
                workspaces
                    .first()
                    .map(|workspace| workspace.workspace_id.clone())
            });
        let focused_tab_id = tabs
            .iter()
            .find(|tab| tab.focused)
            .map(|tab| tab.tab_id.clone());
        Ok(NavigationState {
            focused_workspace_id,
            focused_tab_id,
            workspaces,
            tabs,
        })
    }

    /// Pane pool of one workspace (pane.list) with stable tab membership
    /// resolved through the verified tab.get path (pane.list items carry no
    /// tab id, and session.snapshot carries no stable ids at all).
    fn workspace_pane_pool(&self, workspace_id: &str, tabs: &[Tab]) -> Result<Vec<Pane>, MuxError> {
        let result = self.call("pane.list", json!({ "workspace_id": workspace_id }))?;
        let mut panes: Vec<Pane> = result
            .get("panes")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(parse_pane).collect())
            .unwrap_or_default();
        let mut membership: HashMap<String, String> = HashMap::new();
        for tab in tabs {
            let row = self.call("tab.get", json!({ "tab_id": &tab.tab_id }))?;
            if let Some(ids) = row.get("panes").and_then(Value::as_array) {
                for id in ids {
                    if let Some(id) = id.as_str() {
                        membership.insert(id.to_string(), tab.tab_id.clone());
                    }
                }
            }
        }
        for pane in &mut panes {
            pane.workspace_id = Some(workspace_id.to_string());
            pane.tab_id = membership.get(&pane.pane_id).cloned();
        }
        Ok(panes)
    }

    fn subscribe_event_stream(
        &self,
        pane_scope: Option<Vec<String>>,
    ) -> Result<async_channel::Receiver<MuxEvent>, MuxError> {
        let mut stream = UnixStream::connect(&self.socket_path).map_err(|err| {
            MuxError::SocketUnavailable(self.socket_path.display().to_string(), err.to_string())
        })?;
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        writeln!(
            stream,
            "{}",
            json!({ "id": "shardlane-events", "method": "events.subscribe", "params": {} })
        )?;
        let mut reader = BufReader::new(stream);
        let mut ack = String::new();
        reader.read_line(&mut ack)?;
        let ack: LuvusResponse = serde_json::from_str(ack.trim())?;
        if let Some(error) = ack.error {
            return Err(MuxError::Api(
                error
                    .message
                    .unwrap_or_else(|| "events.subscribe failed".to_string()),
            ));
        }
        if ack
            .result
            .as_ref()
            .and_then(|result| result.get("type"))
            .and_then(Value::as_str)
            != Some("subscription_started")
        {
            return Err(MuxError::Api(format!(
                "unexpected events.subscribe response: {:?}",
                ack.result
            )));
        }
        let scope: Option<HashSet<String>> = pane_scope.map(|ids| ids.into_iter().collect());
        let (tx, rx) = async_channel::unbounded();
        std::thread::spawn(move || {
            let mut line = String::new();
            loop {
                if tx.is_closed() {
                    break;
                }
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) if line.trim().is_empty() => continue,
                    Ok(_) => {
                        let Ok(event_value) = serde_json::from_str::<Value>(line.trim()) else {
                            continue;
                        };
                        // Only {event, data} lines are events; subscription
                        // acks and errors reuse the response envelope.
                        let Some(event) = event_value.get("event").and_then(Value::as_str) else {
                            continue;
                        };
                        if let Some(scope) = &scope {
                            let pane_id = event_value.get("data").and_then(|data| {
                                data.get("pane")
                                    .or_else(|| data.get("pane_id"))
                                    .and_then(Value::as_str)
                            });
                            match pane_id {
                                Some(pane_id) if scope.contains(pane_id) => {}
                                _ => continue,
                            }
                        }
                        let event = HerdrEvent {
                            event: event.to_string(),
                            data: event_value.get("data").cloned().unwrap_or(Value::Null),
                        };
                        if tx.send_blocking(event).is_err() {
                            break;
                        }
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => break,
                }
            }
        });
        Ok(rx)
    }
}

fn rpc_read_timeout(method: &str) -> Duration {
    if method.starts_with("agent.wait") {
        Duration::from_secs(120)
    } else {
        Duration::from_secs(15)
    }
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn u32_field(value: &Value, key: &str) -> Option<u32> {
    value.get(key).and_then(Value::as_u64).map(|v| v as u32)
}

// ---------------------------------------------------------------------------
// Wire -> projection mapping (shapes verified against luvus 0.13.4)
// ---------------------------------------------------------------------------

fn parse_workspace(value: &Value) -> Workspace {
    Workspace {
        workspace_id: str_field(value, "workspace_id")
            .unwrap_or_default()
            .to_string(),
        label: str_field(value, "name").map(str::to_string),
        cwd: str_field(value, "cwd").map(str::to_string),
        agent_status: None,
        active_tab_id: str_field(value, "active_tab").map(str::to_string),
        focused: value
            .get("active")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        tab_count: u32_field(value, "tabs"),
        pane_count: None,
        // display_position is luvus's 0-based index; the projection is 1-based.
        number: str_field(value, "display_position")
            .and_then(|position| position.parse::<u32>().ok())
            .map(|position| position + 1),
    }
}

fn parse_tab(value: &Value, workspace_id: Option<&str>) -> Tab {
    Tab {
        tab_id: str_field(value, "tab_id").unwrap_or_default().to_string(),
        workspace_id: str_field(value, "workspace_id")
            .or(workspace_id)
            .map(str::to_string),
        label: str_field(value, "name").map(str::to_string),
        title: None,
        terminal_title: None,
        agent_status: None,
        pane_count: value
            .get("panes")
            .and_then(Value::as_array)
            .map(|panes| panes.len() as u32),
        focused: value
            .get("active")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

/// Snapshot panes key the id as pane_id; pane.get uses pane.
fn parse_pane(value: &Value) -> Pane {
    Pane {
        pane_id: str_field(value, "pane_id")
            .or_else(|| str_field(value, "pane"))
            .unwrap_or_default()
            .to_string(),
        terminal_id: str_field(value, "terminal_id").map(str::to_string),
        workspace_id: str_field(value, "workspace_id").map(str::to_string),
        tab_id: str_field(value, "tab_id").map(str::to_string),
        label: str_field(value, "name").map(str::to_string),
        title: None,
        terminal_title: None,
        cwd: str_field(value, "cwd").map(str::to_string),
        agent_status: str_field(value, "status")
            .or_else(|| str_field(value, "agent_status"))
            .map(str::to_string),
        agent: str_field(value, "agent").map(str::to_string),
        focused: value
            .get("focused")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        scroll: value
            .get("scroll_offset")
            .and_then(Value::as_u64)
            .map(|offset| crate::herdr::PaneScroll {
                offset_from_bottom: offset as u32,
                ..Default::default()
            }),
    }
}

fn parse_agent(value: &Value) -> Agent {
    let pane_id = str_field(value, "pane").unwrap_or_default().to_string();
    Agent {
        terminal_id: pane_id.clone(),
        name: str_field(value, "name").map(str::to_string),
        agent: str_field(value, "agent").map(str::to_string),
        agent_status: str_field(value, "status").map(str::to_string),
        workspace_id: str_field(value, "workspace").map(str::to_string),
        tab_id: str_field(value, "tab").map(str::to_string),
        pane_id: Some(pane_id),
        focused: value
            .get("focused")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        cwd: str_field(value, "cwd").map(str::to_string),
        interactive_ready: true,
        revision: value.get("revision").and_then(Value::as_u64).unwrap_or(0),
        ..Default::default()
    }
}

fn parse_rect(value: &Value) -> LayoutRect {
    LayoutRect {
        x: u32_field(value, "x").unwrap_or(0),
        y: u32_field(value, "y").unwrap_or(0),
        width: u32_field(value, "width").unwrap_or(0),
        height: u32_field(value, "height").unwrap_or(0),
    }
}

fn parse_layout(value: &Value) -> PaneLayout {
    let rect = parse_rect(value.get("rect").unwrap_or(&Value::Null));
    let focused = str_field(value, "pane").unwrap_or_default().to_string();
    PaneLayout {
        tab_id: str_field(value, "tab").unwrap_or_default().to_string(),
        workspace_id: str_field(value, "workspace").map(str::to_string),
        area: parse_rect(value.get("logical_size").unwrap_or(&Value::Null)),
        panes: vec![LayoutPane {
            pane_id: focused.clone(),
            rect,
            focused: true,
        }],
        splits: Vec::new(),
        focused_pane_id: Some(focused),
        zoomed: false,
    }
}

fn runtime_agent_from_pane(pane: &Pane) -> Result<RuntimeAgent, MuxError> {
    let workspace_id = pane
        .workspace_id
        .clone()
        .ok_or_else(|| MuxError::Api("luvus pane is missing its workspace".to_string()))?;
    let tab_id = pane
        .tab_id
        .clone()
        .ok_or_else(|| MuxError::Api("luvus pane is missing its tab".to_string()))?;
    Ok(RuntimeAgent {
        id: AgentRef::new(pane.pane_id.clone()),
        runtime_workspace_id: workspace_id,
        tab_id,
        pane_id: pane.pane_id.clone(),
        name: pane.label.clone(),
        kind: pane.agent.clone(),
        title: None,
        status: host_agent_status(pane.agent_status.as_deref()),
        interactive_ready: true,
        revision: 0,
    })
}

fn agent_status_str(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => "idle",
        AgentStatus::Working => "working",
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Unknown => "unknown",
    }
}

// ---------------------------------------------------------------------------
// Shared connection facet
// ---------------------------------------------------------------------------

impl MultiplexerConnection for LuvusClient {
    fn capabilities(&self) -> MuxCapabilities {
        LuvusBackend::default().capabilities()
    }

    fn ping(&self) -> Result<(), MuxError> {
        LuvusClient::ping(self)
    }

    fn protocol(&self) -> Option<u32> {
        self.protocol
    }

    fn server_started_with_supplied_config(&self) -> bool {
        false
    }

    fn navigation_state(&self) -> Result<NavigationState, MuxError> {
        self.navigation_state()
    }

    fn visible_state(&self) -> Result<MuxState, MuxError> {
        let navigation = self.navigation_state()?;
        let focused_workspace_id = navigation.focused_workspace_id.clone();
        // The focused tab must belong to the focused workspace: luvus keeps
        // one active tab per workspace.
        let focused_tab_id = focused_workspace_id.as_deref().and_then(|workspace_id| {
            navigation
                .tabs
                .iter()
                .filter(|tab| tab.workspace_id.as_deref() == Some(workspace_id))
                .find(|tab| tab.focused)
                .or_else(|| {
                    navigation
                        .tabs
                        .iter()
                        .find(|tab| tab.workspace_id.as_deref() == Some(workspace_id))
                })
                .map(|tab| tab.tab_id.clone())
        });
        let panes: Vec<Pane> = match focused_workspace_id.as_deref() {
            Some(workspace_id) => {
                let tabs: Vec<Tab> = navigation
                    .tabs
                    .iter()
                    .filter(|tab| tab.workspace_id.as_deref() == Some(workspace_id))
                    .cloned()
                    .collect();
                self.workspace_pane_pool(workspace_id, &tabs)?
                    .into_iter()
                    .filter(|pane| pane.tab_id.as_deref() == focused_tab_id.as_deref())
                    .collect()
            }
            None => Vec::new(),
        };
        let focused_pane_id = panes
            .iter()
            .find(|pane| pane.focused)
            .map(|pane| pane.pane_id.clone());
        let layouts: Vec<PaneLayout> = match focused_pane_id.as_deref() {
            Some(pane_id) => vec![self.pane_layout_of(pane_id)?],
            None => Vec::new(),
        };
        Ok(MuxState {
            focused_workspace_id,
            focused_tab_id,
            focused_pane_id,
            workspaces: navigation.workspaces,
            tabs: navigation.tabs,
            panes,
            agents: self.list_agents().unwrap_or_default(),
            layouts,
            protocol: self.protocol,
            version: self.runtime_version.clone(),
        })
    }

    fn host_bootstrap_state(&self) -> Result<MuxState, MuxError> {
        let navigation = self.navigation_state()?;
        let mut panes = Vec::new();
        for workspace in &navigation.workspaces {
            let tabs: Vec<Tab> = navigation
                .tabs
                .iter()
                .filter(|tab| tab.workspace_id.as_deref() == Some(workspace.workspace_id.as_str()))
                .cloned()
                .collect();
            panes.extend(self.workspace_pane_pool(&workspace.workspace_id, &tabs)?);
        }
        Ok(MuxState {
            focused_workspace_id: None,
            focused_tab_id: None,
            focused_pane_id: None,
            workspaces: navigation.workspaces,
            tabs: navigation.tabs,
            panes,
            agents: self.list_agents()?,
            layouts: Vec::new(),
            protocol: self.protocol,
            version: self.runtime_version.clone(),
        })
    }

    fn workspace_state(&self) -> Result<MuxState, MuxError> {
        // Same projection discipline as visible_state (focus-scoped snapshot).
        self.visible_state()
    }

    fn workspace_panes(&self, workspace_id: &str) -> Result<Vec<Pane>, MuxError> {
        let tabs = self.list_tabs(workspace_id)?;
        self.workspace_pane_pool(workspace_id, &tabs)
    }

    fn tab_surface_state(
        &self,
        workspace_id: &str,
        tab_id: &str,
    ) -> Result<TabSurfaceState, MuxError> {
        let tabs = self.list_tabs(workspace_id)?;
        let panes: Vec<Pane> = self
            .workspace_pane_pool(workspace_id, &tabs)?
            .into_iter()
            .filter(|pane| pane.tab_id.as_deref() == Some(tab_id))
            .collect();
        let focused_pane_id = panes
            .iter()
            .find(|pane| pane.focused)
            .map(|pane| pane.pane_id.clone())
            .or_else(|| panes.first().map(|pane| pane.pane_id.clone()));
        let layouts: Vec<PaneLayout> = match focused_pane_id.as_deref() {
            Some(pane_id) => vec![self.pane_layout_of(pane_id)?],
            None => Vec::new(),
        };
        Ok(TabSurfaceState {
            workspace_id: workspace_id.to_string(),
            tab_id: tab_id.to_string(),
            focused_pane_id,
            panes,
            layouts,
        })
    }

    fn pane_layout(&self, pane_id: &str) -> Result<PaneLayout, MuxError> {
        self.pane_layout_of(pane_id)
    }

    fn agents(&self) -> Result<Vec<Agent>, MuxError> {
        self.list_agents()
    }

    fn subscribe_events(&self) -> Result<async_channel::Receiver<MuxEvent>, MuxError> {
        self.subscribe_event_stream(None)
    }

    fn subscribe_pane_events(
        &self,
        pane_ids: &[String],
    ) -> Result<async_channel::Receiver<MuxEvent>, MuxError> {
        self.subscribe_event_stream(Some(pane_ids.to_vec()))
    }

    fn create_workspace(
        &self,
        params: &CreateWorkspace<'_>,
    ) -> Result<WorkspaceCreatedResult, MuxError> {
        let mut request = json!({ "focus": params.focus });
        if let Some(cwd) = params.cwd {
            request["cwd"] = json!(cwd);
        }
        let created = self.call("workspace.new", request)?;
        let workspace_id = str_field(&created, "workspace")
            .ok_or_else(|| MuxError::Api("workspace.new returned no id".to_string()))?
            .to_string();
        // workspace.new answers with the short 0-based index; resolve the
        // stable workspace_id (and the seeded tab + root pane) by one get.
        let row = self.call("workspace.get", json!({ "workspace": workspace_id }))?;
        let stable_id = str_field(&row, "workspace_id")
            .ok_or_else(|| MuxError::Api("workspace.get returned no id".to_string()))?
            .to_string();
        let workspace = parse_workspace(&row);
        let tabs = self.list_tabs(&stable_id)?;
        let tab = tabs
            .first()
            .ok_or_else(|| MuxError::Api("luvus created a workspace without a tab".to_string()))?;
        let tab_get = self.call("tab.get", json!({ "tab_id": tab.tab_id }))?;
        let root_pane_id = tab_get
            .get("panes")
            .and_then(Value::as_array)
            .and_then(|panes| panes.first())
            .and_then(Value::as_str)
            .ok_or_else(|| MuxError::Api("luvus tab has no root pane".to_string()))?;
        Ok(WorkspaceCreatedResult {
            workspace,
            tab: tab.clone(),
            root_pane: self.fetch_pane(root_pane_id)?,
        })
    }

    fn close_workspace(&self, workspace_id: &str) -> Result<(), MuxError> {
        self.call("workspace.close", json!({ "workspace_id": workspace_id }))?;
        Ok(())
    }

    fn rename_workspace(&self, workspace_id: &str, label: &str) -> Result<(), MuxError> {
        self.call(
            "workspace.rename",
            json!({ "workspace_id": workspace_id, "name": label }),
        )?;
        Ok(())
    }

    fn move_workspace(&self, workspace_id: &str, insert_index: usize) -> Result<(), MuxError> {
        self.call(
            "workspace.move",
            json!({ "workspace_id": workspace_id, "to": insert_index }),
        )?;
        Ok(())
    }

    fn move_workspace_before(
        &self,
        workspace_id: &str,
        before_workspace_id: &str,
    ) -> Result<(), MuxError> {
        let workspaces = self.list_workspaces()?;
        let target_index = workspaces
            .iter()
            .position(|workspace| workspace.workspace_id == before_workspace_id)
            .ok_or_else(|| {
                MuxError::NotFound(format!("workspace {before_workspace_id} not found"))
            })?;
        self.move_workspace(workspace_id, target_index)
    }

    fn workspace_focus(&self, workspace_id: &str) -> Result<(), MuxError> {
        self.call("workspace.focus", json!({ "workspace_id": workspace_id }))?;
        Ok(())
    }

    fn create_tab(&self, params: &CreateTab<'_>) -> Result<TabCreatedResult, MuxError> {
        let mut request = json!({});
        if let Some(workspace_id) = params.workspace_id {
            request["workspace_id"] = json!(workspace_id);
        }
        let created = self.call("tab.new", request)?;
        let tab_id = str_field(&created, "tab")
            .ok_or_else(|| MuxError::Api("tab.new returned no id".to_string()))?
            .to_string();
        // tab.new answers with the 1-based position; resolve the stable
        // tab_id by one numeric get before any tab_id-keyed follow-ups.
        let tab_get = self.call("tab.get", json!({ "tab": tab_id }))?;
        let tab = parse_tab(&tab_get, str_field(&tab_get, "workspace_id"));
        if params.focus {
            self.call("tab.focus", json!({ "tab_id": &tab.tab_id }))?;
        }
        let root_pane_id = tab_get
            .get("panes")
            .and_then(Value::as_array)
            .and_then(|panes| panes.first())
            .and_then(Value::as_str)
            .ok_or_else(|| MuxError::Api("luvus tab has no root pane".to_string()))?;
        Ok(TabCreatedResult {
            tab,
            root_pane: self.fetch_pane(root_pane_id)?,
        })
    }

    fn close_tab(&self, tab_id: &str) -> Result<(), MuxError> {
        self.call("tab.close", json!({ "tab_id": tab_id }))?;
        Ok(())
    }

    fn rename_tab(&self, tab_id: &str, label: &str) -> Result<(), MuxError> {
        self.call("tab.rename", json!({ "tab_id": tab_id, "name": label }))?;
        Ok(())
    }

    fn move_tab(&self, tab_id: &str, insert_index: usize) -> Result<(), MuxError> {
        // luvus tab positions are 1-based (verified tab.move {tab, to}).
        self.call(
            "tab.move",
            json!({ "tab_id": tab_id, "to": insert_index + 1 }),
        )?;
        Ok(())
    }

    fn tab_focus(&self, tab_id: &str) -> Result<(), MuxError> {
        self.call("tab.focus", json!({ "tab_id": tab_id }))?;
        Ok(())
    }

    fn split_pane(&self, pane_id: &str, direction: SplitDirection) -> Result<Pane, MuxError> {
        let direction = match direction {
            SplitDirection::Right => "right",
            SplitDirection::Down => "down",
        };
        let created = self.call(
            "pane.split",
            json!({ "pane": pane_id, "direction": direction }),
        )?;
        let new_pane_id = str_field(&created, "pane")
            .ok_or_else(|| MuxError::Api("pane.split returned no id".to_string()))?;
        self.fetch_pane(new_pane_id)
    }

    fn close_pane(&self, pane_id: &str) -> Result<(), MuxError> {
        self.call("pane.close", json!({ "pane": pane_id }))?;
        Ok(())
    }

    fn rename_pane(&self, pane_id: &str, label: &str) -> Result<(), MuxError> {
        self.call("pane.rename", json!({ "pane": pane_id, "name": label }))?;
        Ok(())
    }

    fn swap_pane(
        &self,
        pane_id: &str,
        direction: MuxDirection,
    ) -> Result<PaneLayoutActionResult, MuxError> {
        let neighbor = self.call(
            "pane.neighbor",
            json!({ "pane": pane_id, "direction": direction.as_str() }),
        )?;
        let neighbor_id = str_field(&neighbor, "neighbor")
            .ok_or_else(|| MuxError::Api("pane.neighbor returned no id".to_string()))?;
        self.call("pane.swap", json!({ "pane": pane_id, "with": neighbor_id }))?;
        let layout = self.pane_layout_of(pane_id)?;
        Ok(PaneLayoutActionResult { layout })
    }

    fn resize_pane(
        &self,
        pane_id: &str,
        direction: MuxDirection,
    ) -> Result<PaneLayoutActionResult, MuxError> {
        self.call(
            "pane.resize",
            json!({ "pane": pane_id, "direction": direction.as_str() }),
        )?;
        let layout = self.pane_layout_of(pane_id)?;
        Ok(PaneLayoutActionResult { layout })
    }

    fn toggle_pane_zoom(&self, pane_id: &str) -> Result<PaneLayoutActionResult, MuxError> {
        self.call("pane.zoom", json!({ "pane": pane_id }))?;
        let layout = self.pane_layout_of(pane_id)?;
        Ok(PaneLayoutActionResult { layout })
    }

    fn pane_focus(&self, pane_id: &str) -> Result<(), MuxError> {
        self.call("pane.focus", json!({ "pane": pane_id }))?;
        Ok(())
    }

    fn move_pane_to_tab(&self, pane_id: &str, tab_id: &str) -> Result<PaneMoveResult, MuxError> {
        let tab_get = self.call("tab.get", json!({ "tab_id": tab_id }))?;
        // luvus pane.move targets a 1-based tab position inside the pane's
        // workspace; resolve it from the tab's own index field.
        let position = str_field(&tab_get, "tab")
            .and_then(|index| index.parse::<usize>().ok())
            .ok_or_else(|| MuxError::Api("luvus tab is missing its index".to_string()))?;
        self.call("pane.move", json!({ "pane": pane_id, "tab": position }))?;
        let pane = self.fetch_pane(pane_id)?;
        let target_layout = self.pane_layout_of(pane_id)?;
        Ok(PaneMoveResult {
            pane,
            target_layout,
        })
    }

    fn move_pane_to_new_tab(
        &self,
        pane_id: &str,
        _workspace_id: &str,
    ) -> Result<PaneMoveResult, MuxError> {
        self.call("pane.move", json!({ "pane": pane_id, "new_tab": true }))?;
        let pane = self.fetch_pane(pane_id)?;
        let target_layout = self.pane_layout_of(pane_id)?;
        Ok(PaneMoveResult {
            pane,
            target_layout,
        })
    }

    fn set_split_ratio(&self, tab_id: &str, path: &[bool], ratio: f64) -> Result<(), MuxError> {
        // herdr paths are boolean steps; luvus spells them a/b. The layout
        // RPC targets the 1-based tab number, resolved from the stable id.
        let tab_get = self.call("tab.get", json!({ "tab_id": tab_id }))?;
        let position = str_field(&tab_get, "tab")
            .and_then(|index| index.parse::<usize>().ok())
            .ok_or_else(|| MuxError::Api("luvus tab is missing its index".to_string()))?;
        let steps: Vec<&str> = path
            .iter()
            .map(|step| if *step { "b" } else { "a" })
            .collect();
        self.call(
            "layout.set_split_ratio",
            json!({ "tab": position, "path": steps, "ratio": ratio }),
        )?;
        Ok(())
    }

    fn send_text(&self, pane_id: &str, text: &str) -> Result<(), MuxError> {
        self.call("pane.send_input", json!({ "pane": pane_id, "text": text }))?;
        Ok(())
    }

    fn send_keys(&self, pane_id: &str, keys: &[String]) -> Result<(), MuxError> {
        if keys.is_empty() {
            return Ok(());
        }
        // luvus has no pane-level semantic-key RPC; encode the verified key
        // vocabulary to PTY bytes client-side (same spellings Herdr accepts).
        let bytes: String = keys.iter().map(|key| encode_key(key)).collect();
        self.send_text(pane_id, &bytes)
    }

    fn pane_process_info(&self, pane_id: &str) -> Result<PaneProcessInfo, MuxError> {
        let result = self.call("pane.processes", json!({ "pane": pane_id }))?;
        let root_pid = result
            .get("root_process")
            .and_then(|root| root.get("pid"))
            .and_then(Value::as_u64);
        let executables: Vec<String> = result
            .get("executables")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        // luvus hides per-process pids/argv by design; project the root pid
        // with the observed executable names (bounded, degraded info).
        let foreground_processes = executables
            .first()
            .map(|name| {
                vec![PaneProcessInfoProcess {
                    pid: root_pid.unwrap_or(0) as u32,
                    name: name.clone(),
                    argv: None,
                    argv0: None,
                    cmdline: None,
                    cwd: None,
                }]
            })
            .unwrap_or_default();
        Ok(PaneProcessInfo {
            pane_id: pane_id.to_string(),
            shell_pid: root_pid.map(|pid| pid as u32),
            tty: None,
            foreground_process_group_id: None,
            foreground_processes,
        })
    }

    fn read_pane_history(&self, pane_id: &str, lines: u32) -> Result<PaneHistory, MuxError> {
        let result = self.call("pane.read", json!({ "pane": pane_id, "lines": lines }))?;
        Ok(PaneHistory {
            pane_id: pane_id.to_string(),
            text: str_field(&result, "text").unwrap_or_default().to_string(),
        })
    }

    fn reload_config(&self) -> Result<(), MuxError> {
        self.call("server.reload_config", json!({}))?;
        Ok(())
    }

    fn open_shared_session(
        &self,
        key: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Result<Arc<dyn MultiplexerStream>, MuxError> {
        let session = key.unwrap_or("default");
        let socket = if session == "default" {
            self.socket_path.clone()
        } else {
            Self::session_socket_path(session)
        };
        LuvusAttachStream::spawn(socket, session, cols, rows)
            .map_err(|error| MuxError::Api(error.to_string()))
            .map(|stream| stream as Arc<dyn MultiplexerStream>)
    }

    fn agent_runtime(&self) -> Option<&dyn MuxAgentRuntime> {
        Some(self)
    }
}

// ---------------------------------------------------------------------------
// Domain 7 — agent runtime facet
// ---------------------------------------------------------------------------

impl MuxAgentRuntime for LuvusClient {
    fn start_runtime_agent(
        &self,
        request: &RuntimeAgentStartRequest,
    ) -> Result<RuntimeAgent, MuxError> {
        // luvus spawns the agent beside the focused pane; name/kind are the
        // verified contract. Extra argv has no luvus RPC equivalent (degraded).
        let created = self.call(
            "agent.start",
            json!({ "name": request.name, "kind": request.kind }),
        )?;
        let pane_id = str_field(&created, "pane").unwrap_or_default().to_string();
        self.runtime_agent_for(&pane_id)
    }

    fn prompt_runtime_agent(
        &self,
        request: &RuntimeAgentPromptRequest,
    ) -> Result<RuntimeAgent, MuxError> {
        let target = request.agent_id.as_str();
        self.call(
            "agent.prompt",
            json!({ "target": target, "text": request.text }),
        )?;
        if let Some(status) = request.wait_until.first() {
            self.call(
                "agent.wait",
                json!({ "pane": target, "status": agent_status_str(*status) }),
            )?;
        }
        self.runtime_agent_for(target)
    }

    fn read_runtime_agent(
        &self,
        request: &RuntimeAgentReadRequest,
    ) -> Result<RuntimeAgentRead, MuxError> {
        let mut params = json!({ "target": request.agent_id.as_str() });
        if let Some(lines) = request.lines {
            params["lines"] = json!(lines);
        }
        let result = self.call("agent.read", params)?;
        Ok(RuntimeAgentRead {
            agent_id: request.agent_id.clone(),
            text: str_field(&result, "text").unwrap_or_default().to_string(),
            revision: result.get("revision").and_then(Value::as_u64).unwrap_or(0),
            truncated: false,
        })
    }

    fn wait_runtime_agent(
        &self,
        request: &RuntimeAgentWaitRequest,
    ) -> Result<RuntimeAgentWait, MuxError> {
        let status = request
            .until
            .first()
            .ok_or_else(|| MuxError::Api("agent wait needs a target status".to_string()))?;
        self.call(
            "agent.wait",
            json!({ "pane": request.agent_id.as_str(), "status": agent_status_str(*status) }),
        )?;
        Ok(RuntimeAgentWait {
            event: "agent.wait".to_string(),
            status: Some(*status),
        })
    }

    fn send_runtime_agent_keys(
        &self,
        agent_id: &AgentRef,
        keys: &[String],
    ) -> Result<(), MuxError> {
        if keys.is_empty() {
            return Ok(());
        }
        self.call(
            "agent.keys",
            json!({ "target": agent_id.as_str(), "keys": keys }),
        )?;
        Ok(())
    }

    fn report_pane_agent(&self, pane_id: &str, agent: &str) -> Result<(), MuxError> {
        self.call(
            "agent.report",
            json!({
                "pane": pane_id,
                "source": "shardlane",
                "agent": agent,
                "status": "working"
            }),
        )?;
        Ok(())
    }

    fn clear_pane_agent_authority(&self, pane_id: &str) -> Result<(), MuxError> {
        self.call(
            "agent.release",
            json!({ "pane": pane_id, "source": "shardlane" }),
        )?;
        Ok(())
    }

    fn agent_focus(&self, target: &str) -> Result<(), MuxError> {
        // The agent runtime facet stores pane ids as terminal ids.
        self.call("pane.focus", json!({ "pane": target }))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Backend facade
// ---------------------------------------------------------------------------

/// Encode one semantic key to the PTY bytes luvus's raw input path accepts.
fn encode_key(key: &str) -> String {
    let lower = key.to_ascii_lowercase();
    match lower.as_str() {
        "enter" | "return" => "\r".into(),
        "esc" | "escape" => "\u{1b}".into(),
        "tab" => "\t".into(),
        "backspace" => "\u{7f}".into(),
        "space" => " ".into(),
        "up" => "\u{1b}[A".into(),
        "down" => "\u{1b}[B".into(),
        "right" => "\u{1b}[C".into(),
        "left" => "\u{1b}[D".into(),
        "home" => "\u{1b}[H".into(),
        "end" => "\u{1b}[F".into(),
        "pageup" => "\u{1b}[5~".into(),
        "pagedown" => "\u{1b}[6~".into(),
        _ => {
            if let Some(letter) = lower.strip_prefix("ctrl+") {
                if letter.len() == 1 {
                    let byte = letter.as_bytes()[0];
                    if byte.is_ascii_lowercase() {
                        return ((byte - b'a' + 1) as char).to_string();
                    }
                }
            }
            key.to_string()
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LuvusBackend {
    admin: LuvusServerAdmin,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LuvusServerAdmin;

impl MultiplexerServerAdmin for LuvusServerAdmin {
    fn cli_path(&self) -> Option<PathBuf> {
        luvus_cli_path()
    }

    fn installed_cli_version(&self) -> Option<String> {
        let output = Command::new(luvus_cli_path()?)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        text.split_whitespace()
            .nth(1)
            .map(str::to_string)
            .or_else(|| text.split_whitespace().next().map(str::to_string))
    }

    fn user_config_path(&self) -> PathBuf {
        home_dir().join(".luvus/config.json")
    }
}

impl Multiplexer for LuvusBackend {
    fn id(&self) -> &'static str {
        "luvus"
    }

    fn capabilities(&self) -> MuxCapabilities {
        MuxCapabilities {
            agents: true,
            server_admin: true,
            shared_tui: true,
            pane_history_read: true,
            // tab.move reorders within its workspace only (same protocol gap
            // as Herdr).
            cross_workspace_tab_move: false,
            events_push: true,
        }
    }

    fn list_instances(&self) -> Option<Vec<InstanceListing>> {
        Some(
            luvus_session_rows()?
                .into_iter()
                .map(|row| InstanceListing {
                    backend: self.id().to_string(),
                    display_name: read_display_name(&row.name),
                    name: row.name,
                    running: row.running,
                    is_default: row.is_default,
                })
                .collect(),
        )
    }

    fn rename_instance(&self, instance: &str, display_name: &str) -> Result<(), MuxError> {
        write_display_name(instance, display_name)
    }

    fn stop_instance(&self, instance: &str) -> Result<(), MuxError> {
        let cli = luvus_cli_path()
            .ok_or_else(|| MuxError::InstallFailed("luvus is not installed".to_string()))?;
        let mut command = Command::new(cli);
        if instance == "default" {
            command.args(["server", "stop"]);
        } else {
            command.args(["session", "stop", instance]);
        }
        let status = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(MuxError::Io)?;
        if status.success() {
            Ok(())
        } else {
            Err(MuxError::Api(format!(
                "luvus session stop {instance} exited with {status}"
            )))
        }
    }

    fn delete_instance(&self, instance: &str) -> Result<(), MuxError> {
        if instance == "default" {
            return Err(MuxError::Api(
                "the default luvus session cannot be deleted".to_string(),
            ));
        }
        let cli = luvus_cli_path()
            .ok_or_else(|| MuxError::InstallFailed("luvus is not installed".to_string()))?;
        let status = Command::new(cli)
            .args(["session", "delete", instance])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(MuxError::Io)?;
        if status.success() {
            Ok(())
        } else {
            Err(MuxError::Api(format!(
                "luvus session delete {instance} exited with {status}"
            )))
        }
    }

    fn open_instance(
        &self,
        reference: &InstanceRef,
    ) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
        let client = match &reference.target {
            InstanceTarget::Default => LuvusClient::bootstrap()?,
            InstanceTarget::Named(session) => LuvusClient::bootstrap_for_session(session)?,
            InstanceTarget::Socket(path) => LuvusClient::connect_to(path)?,
        };
        Ok(Arc::new(client))
    }

    fn connect_instance(
        &self,
        reference: &InstanceRef,
    ) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
        let client = match &reference.target {
            InstanceTarget::Default => LuvusClient::connect()?,
            InstanceTarget::Named(session) => {
                LuvusClient::connect_to(&LuvusClient::session_socket_path(session))?
            }
            InstanceTarget::Socket(path) => LuvusClient::connect_to(path)?,
        };
        Ok(Arc::new(client))
    }

    fn server_admin(&self) -> Option<&dyn MultiplexerServerAdmin> {
        Some(&self.admin)
    }
}

// ---------------------------------------------------------------------------
// Domain 6 — the luvus TUI attach child stream
// ---------------------------------------------------------------------------

/// One luvus TUI child on a private PTY: the same hosted-TUI-child shape as
/// the Herdr/tmux streams (byte fan-out, PTY input, grid resize), so the GUI
/// input path and the Remote TUI routes work unchanged.
pub struct LuvusAttachStream {
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

impl LuvusAttachStream {
    fn spawn(socket: PathBuf, session: &str, cols: u16, rows: u16) -> Result<Arc<Self>, TuiError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TuiError::Spawn(format!("open attach pty: {error}")))?;
        let luvus =
            luvus_cli_path().ok_or_else(|| TuiError::Spawn("luvus is not installed".into()))?;
        let mut builder = CommandBuilder::new(luvus);
        if session != "default" {
            builder.arg("--session");
            builder.arg(session);
        }
        // The TUI resolves the server socket through LUVUS_SOCKET_PATH.
        builder.env("LUVUS_SOCKET_PATH", &socket);
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
            id: format!("luvus-attach-{}", uuid::Uuid::new_v4()),
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
            .name("luvus-attach-reader".to_string())
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

impl MultiplexerStream for LuvusAttachStream {
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
    /// needs no startup byte replay.
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

    /// The shared-TUI SIGWINCH nudge: a one-row shrink/restore makes the TUI
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::mux::kit;

    #[test]
    fn backend_id_and_capabilities() {
        let backend = LuvusBackend::default();
        assert_eq!(backend.id(), "luvus");
        let caps = backend.capabilities();
        assert!(caps.agents && caps.server_admin && caps.shared_tui);
        assert!(caps.pane_history_read && caps.events_push);
        assert!(!caps.cross_workspace_tab_move);
        kit::backend_contract(&backend);
    }

    #[test]
    fn key_encoding_maps_the_verified_vocabulary() {
        assert_eq!(encode_key("Enter"), "\r");
        assert_eq!(encode_key("esc"), "\u{1b}");
        assert_eq!(encode_key("ctrl+c"), "\u{3}");
        assert_eq!(encode_key("up"), "\u{1b}[A");
        assert_eq!(encode_key("space"), " ");
        // Unknown keys pass through verbatim (text fallback).
        assert_eq!(encode_key("x"), "x");
    }

    #[test]
    fn wire_shapes_project_to_the_neutral_model() {
        // Pinned from live luvus 0.13.4 responses.
        let workspace = parse_workspace(&json!({
            "active": true,
            "cwd": "/tmp/proj",
            "display_position": "2",
            "name": "proj",
            "tabs": 3,
            "workspace": "0",
            "workspace_id": "workspace_abc"
        }));
        assert_eq!(workspace.workspace_id, "workspace_abc");
        assert_eq!(workspace.label.as_deref(), Some("proj"));
        assert!(workspace.focused);
        assert_eq!(workspace.tab_count, Some(3));
        assert_eq!(workspace.number, Some(3));

        let tab = parse_tab(
            &json!({
                "active": false,
                "kind": "panes",
                "name": null,
                "tab": "1",
                "tab_id": "tab_abc"
            }),
            Some("workspace_abc"),
        );
        assert_eq!(tab.tab_id, "tab_abc");
        assert_eq!(tab.workspace_id.as_deref(), Some("workspace_abc"));
        assert!(!tab.focused);

        let pane = parse_pane(&json!({
            "agent": "zsh",
            "agent_status": "done",
            "cwd": "/tmp/proj",
            "focused": false,
            "pane_id": "7",
            "terminal_id": "c1de4b01"
        }));
        assert_eq!(pane.pane_id, "7");
        assert_eq!(pane.terminal_id.as_deref(), Some("c1de4b01"));
        assert_eq!(pane.agent_status.as_deref(), Some("done"));

        let layout = parse_layout(&json!({
            "logical_size": {"height": 10000, "width": 10000},
            "pane": "7",
            "rect": {"height": 10000, "width": 5000, "x": 0, "y": 0},
            "tab": "1",
            "tree": {"Leaf": 7},
            "type": "pane_layout",
            "workspace": "0"
        }));
        assert_eq!(layout.tab_id, "1");
        assert_eq!(layout.area.width, 10000);
        assert_eq!(layout.panes.len(), 1);
        assert_eq!(layout.panes[0].rect.width, 5000);
    }

    #[test]
    fn live_luvus_session_lifecycle() {
        let Some(_) = luvus_cli_path() else {
            eprintln!("skipping live_luvus_session_lifecycle: luvus CLI not installed");
            return;
        };
        let session = format!("shardlane_mux_test_{}", std::process::id());
        let backend = LuvusBackend::default();

        // 1. Open instance (starts the named session's server).
        let conn = backend
            .open_instance(&InstanceRef::named("luvus", &session))
            .expect("open_instance should start the named session");
        kit::connection_contract(conn.as_ref(), backend.capabilities());
        assert!(conn.ping().is_ok());

        // 2. Structural round trip inside the isolated named session.
        let workspace = conn
            .create_workspace(&CreateWorkspace {
                cwd: None,
                focus: false,
            })
            .expect("create workspace");
        let tab = conn
            .create_tab(&CreateTab {
                workspace_id: Some(&workspace.workspace.workspace_id),
                cwd: None,
                focus: false,
            })
            .expect("create tab");
        conn.rename_tab(&tab.tab.tab_id, "mux-kit")
            .expect("rename tab");
        let pane = conn
            .split_pane(&tab.root_pane.pane_id, SplitDirection::Right)
            .expect("split pane");
        conn.send_text(&pane.pane_id, "echo mux-kit\r")
            .expect("send text");
        std::thread::sleep(Duration::from_millis(400));
        let history = conn
            .read_pane_history(&pane.pane_id, 20)
            .expect("read history");
        assert!(history.text.len() <= 64 * 1024, "history must be bounded");
        let surface = conn
            .tab_surface_state(&workspace.workspace.workspace_id, &tab.tab.tab_id)
            .expect("tab surface");
        assert!(
            surface
                .panes
                .iter()
                .any(|item| item.pane_id == pane.pane_id),
            "split pane must appear in the tab surface"
        );
        let navigation = conn.navigation_state().expect("navigation");
        assert!(
            navigation
                .workspaces
                .iter()
                .any(|item| item.workspace_id == workspace.workspace.workspace_id),
            "created workspace must appear in navigation"
        );

        conn.close_pane(&pane.pane_id).expect("close pane");
        conn.close_tab(&tab.tab.tab_id).expect("close tab");
        conn.close_workspace(&workspace.workspace.workspace_id)
            .expect("close workspace");

        // 3. Enumeration + lifecycle of the named instance.
        let instances = backend.list_instances().expect("list instances");
        assert!(
            instances
                .iter()
                .any(|item| item.name == session && item.running),
            "running named session must surface in the enumeration"
        );
        assert!(backend.stop_instance(&session).is_ok());
        assert!(backend.delete_instance(&session).is_ok());
    }
}
