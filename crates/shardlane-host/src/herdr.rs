//! Typed Herdr runtime integration layer: Unix socket RPC, event subscription,
//! and the AgentRuntime SPI implementation.
//!
//! [INPUT]: depends on serde/serde_json (RPC serialization), thiserror (error
//! types), async-channel (event channel), crate::diagnostics (lag_log), and
//! crate::{ids,dto,runtime} (Host contracts)
//! [OUTPUT]: exposes HerdrClient/HerdrError/HerdrEvent and all typed protocol
//! wrappers (workspace/tab/pane/agent/terminal methods, AgentRuntime for
//! HerdrClient), plus herdr/wax CLI discovery (resolve_user_cli/herdr_cli_path:
//! a login-shell PATH fallback for the packaged .app so Finder launches don't
//! hit io NotFound)
//! [POS]: shardlane-host's runtime adapter implementing the SPI defined in
//! runtime.rs; consumed by both herdr-gui (desktop) and shardlane-remote
//! (remote API)

use crate::diagnostics::lag_log;
use crate::{
    AgentRef as HostAgentRef, AgentRuntime, AgentStatus as HostAgentStatus, RuntimeAgent,
    RuntimeAgentPromptRequest, RuntimeAgentRead, RuntimeAgentReadFormat, RuntimeAgentReadRequest,
    RuntimeAgentStartRequest, RuntimeAgentWait, RuntimeAgentWaitRequest,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env,
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HerdrError {
    #[error("herdr is not installed and `wax install herdr` failed: {0}")]
    InstallFailed(String),
    #[error("herdr socket unavailable at {0}: {1}")]
    SocketUnavailable(String, String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("herdr api: {0}")]
    Api(String),
    /// Herdr structured error code=agent_not_found: the basis for mapping 404
    /// in the remote API.
    #[error("herdr api: {0}")]
    AgentNotFound(String),
    /// Herdr structured error code=agent_blocked: the prompt was rejected
    /// before any input was sent (AC-02: never raw-Enter recover for this).
    #[error("herdr api: {0}")]
    AgentBlocked(String),
    /// Herdr structured error code=agent_prompt_stalled: the prompt text was
    /// accepted but no state change was observed (text parked in the
    /// composer; the one proven scenario where AC-02 allows one extra Enter).
    #[error("herdr api: {0}")]
    AgentPromptStalled(String),
    /// Herdr structured error code=timeout: the server-side wait timed out.
    /// The request may have taken effect; the outcome is uncertain
    /// (AC-02: no automatic resend/key supplementation).
    #[error("herdr api timeout: {0}")]
    ApiTimeout(String),
    /// The request was written but no response was read (e.g. a local socket
    /// read timeout). The outcome is uncertain: automatic retries or key
    /// supplementation are forbidden (AC-01/AC-02).
    #[error("delivery uncertain: {0}")]
    DeliveryUncertain(String),
    #[error("incompatible protocol: requires {min}+; got {actual}")]
    IncompatibleProtocol { min: u32, actual: u32 },
}

const MIN_SUPPORTED_PROTOCOL: u32 = 19;
/// Source identifier for Agent correction reports: marks client-manually-
/// reported attribution under herdr's agent_session.source semantics,
/// distinct from herdr's own detection (`herdr:*`).
const CLIENT_AGENT_REPORT_SOURCE: &str = "shardlane";

fn supports_protocol(protocol: u32) -> bool {
    protocol >= MIN_SUPPORTED_PROTOCOL
}

/// Display identity for the Herdr runtime endpoint currently connected by the GUI.
#[derive(Clone, Debug)]
pub struct DeviceEndpoint {
    pub label: String,
}

impl Default for DeviceEndpoint {
    fn default() -> Self {
        Self {
            label: "localhost".to_string(),
        }
    }
}

// --- Snapshot data model ---

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct HerdrState {
    #[serde(default)]
    pub focused_workspace_id: Option<String>,
    #[serde(default)]
    pub focused_tab_id: Option<String>,
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    pub workspaces: Vec<Workspace>,
    pub tabs: Vec<Tab>,
    pub panes: Vec<Pane>,
    pub agents: Vec<Agent>,
    pub layouts: Vec<PaneLayout>,
    #[serde(default)]
    pub protocol: Option<u32>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NavigationState {
    pub focused_workspace_id: Option<String>,
    pub focused_tab_id: Option<String>,
    pub workspaces: Vec<Workspace>,
    pub tabs: Vec<Tab>,
}

#[derive(Clone, Debug, Default)]
pub struct TabSurfaceState {
    pub workspace_id: String,
    pub tab_id: String,
    pub focused_pane_id: Option<String>,
    pub panes: Vec<Pane>,
    pub layouts: Vec<PaneLayout>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PaneMoveResult {
    pub pane: Pane,
    pub target_layout: PaneLayout,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PaneLayoutActionResult {
    pub layout: PaneLayout,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PaneProcessInfoProcess {
    pub pid: u32,
    pub name: String,
    #[serde(default)]
    pub argv: Option<Vec<String>>,
    #[serde(default)]
    pub argv0: Option<String>,
    #[serde(default)]
    pub cmdline: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PaneProcessInfo {
    pub pane_id: String,
    #[serde(default)]
    pub shell_pid: Option<u32>,
    #[serde(default)]
    pub tty: Option<String>,
    #[serde(default)]
    pub foreground_process_group_id: Option<u32>,
    #[serde(default)]
    pub foreground_processes: Vec<PaneProcessInfoProcess>,
}

#[derive(Debug, Deserialize)]
struct PaneMoveResponse {
    move_result: PaneMoveResult,
}

#[derive(Debug, Deserialize)]
struct PaneResizeResponse {
    resize: PaneLayoutActionResult,
}

#[derive(Debug, Deserialize)]
struct PaneSwapResponse {
    swap: PaneLayoutActionResult,
}

#[derive(Debug, Deserialize)]
struct PaneProcessInfoResponse {
    process_info: PaneProcessInfo,
}

#[derive(Debug, Deserialize)]
struct PaneCreatedResponse {
    pane: Pane,
}

#[derive(Debug, Deserialize)]
struct PaneZoomResponse {
    zoom: PaneLayoutActionResult,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TabCreatedResult {
    pub tab: Tab,
    pub root_pane: Pane,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WorkspaceCreatedResult {
    pub workspace: Workspace,
    pub tab: Tab,
    pub root_pane: Pane,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Workspace {
    pub workspace_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub active_tab_id: Option<String>,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub tab_count: Option<u32>,
    #[serde(default)]
    pub pane_count: Option<u32>,
    #[serde(default)]
    pub number: Option<u32>,
}

impl Workspace {
    #[cfg(test)]
    pub fn stub(id: &str, cwd: Option<&str>) -> Self {
        Self {
            workspace_id: id.into(),
            label: None,
            cwd: cwd.map(str::to_string),
            agent_status: None,
            active_tab_id: None,
            focused: false,
            tab_count: None,
            pane_count: None,
            number: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Tab {
    pub tab_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub terminal_title: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub pane_count: Option<u32>,
    #[serde(default)]
    pub focused: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Pane {
    pub pane_id: String,
    #[serde(default)]
    pub terminal_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub terminal_title: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub scroll: Option<PaneScroll>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct PaneScroll {
    #[serde(default)]
    pub offset_from_bottom: u32,
    #[serde(default)]
    pub max_offset_from_bottom: u32,
    #[serde(default)]
    pub viewport_rows: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct AgentSessionInfo {
    pub agent: String,
    pub kind: String,
    pub source: String,
    pub value: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Agent {
    pub terminal_id: String,
    #[serde(default)]
    pub agent_session: Option<AgentSessionInfo>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub display_agent: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub custom_status: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub pane_id: Option<String>,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub foreground_cwd: Option<String>,
    #[serde(default)]
    pub terminal_title: Option<String>,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
    #[serde(default)]
    pub interactive_ready: bool,
    #[serde(default)]
    pub launch_pending: bool,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub state_change_seq: u64,
    #[serde(default)]
    pub screen_detection_skipped: bool,
    #[serde(default)]
    pub state_labels: HashMap<String, String>,
    #[serde(default)]
    pub tokens: HashMap<String, String>,
}

// --- Typed Herdr Agent operations ---

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrAgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrReadSource {
    Visible,
    Recent,
    RecentUnwrapped,
    // Kept despite zero local constructors: `detection` is a real Herdr read
    // source on the wire and this enum must keep decoding every server reply.
    Detection,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrReadFormat {
    Text,
    Ansi,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AgentStartParams {
    pub name: String,
    pub kind: String,
    pub pane_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct AgentPromptWaitOptions {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub until: Vec<HerdrAgentStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AgentPromptParams {
    pub target: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<AgentPromptWaitOptions>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AgentReadParams {
    pub target: String,
    pub source: HerdrReadSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<u32>,
    pub format: HerdrReadFormat,
    pub strip_ansi: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AgentWaitParams {
    pub target: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub until: Vec<HerdrAgentStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AgentSendKeysParams {
    pub target: String,
    pub keys: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AgentStartedResult {
    pub agent: Agent,
    // Kept (unused field, serde-renamed `argv`): the Herdr 0.8.2 wire shape
    // always carries it and this struct must keep decoding every reply.
    #[serde(rename = "argv")]
    pub _argv: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AgentPromptedResult {
    pub agent: Agent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct PaneReadResult {
    pub pane_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub source: HerdrReadSource,
    pub format: HerdrReadFormat,
    pub text: String,
    pub revision: u64,
    pub truncated: bool,
}

// --- Layout snapshot ---

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PaneLayout {
    pub tab_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    pub area: LayoutRect,
    pub panes: Vec<LayoutPane>,
    #[serde(default)]
    pub splits: Vec<LayoutSplit>,
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    #[serde(default)]
    pub zoomed: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct LayoutRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LayoutPane {
    pub pane_id: String,
    pub rect: LayoutRect,
    #[serde(default)]
    pub focused: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LayoutSplit {
    #[serde(default)]
    pub id: Option<String>,
    pub direction: String,
    pub ratio: f64,
    pub rect: LayoutRect,
}

// --- Herdr events ---

#[derive(Clone, Debug, Deserialize)]
pub struct HerdrEvent {
    pub event: String,
    pub data: Value,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentStatusPatch {
    pub pane_id: String,
    pub workspace_id: String,
    pub agent_status: Option<Option<String>>,
    pub agent_session: Option<Option<AgentSessionInfo>>,
    pub name: Option<Option<String>>,
    pub agent: Option<Option<String>>,
    pub display_agent: Option<Option<String>>,
    pub title: Option<Option<String>>,
    pub custom_status: Option<Option<String>>,
    pub tab_id: Option<Option<String>>,
    pub cwd: Option<Option<String>>,
    pub foreground_cwd: Option<Option<String>>,
    pub focused: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneScrollPatch {
    pub pane_id: String,
    pub workspace_id: String,
    pub scroll: PaneScroll,
}

impl HerdrEvent {
    pub fn refreshes_navigation_focus_projection(&self) -> bool {
        matches!(
            self.event.as_str(),
            "workspace_focused"
                | "tab_focused"
                | "pane_focused"
                | "workspace.focused"
                | "tab.focused"
                | "pane.focused"
        )
    }

    pub fn refreshes_navigation_projection(&self) -> bool {
        matches!(
            self.event.as_str(),
            "workspace_created"
                | "workspace_updated"
                | "workspace_metadata_updated"
                | "workspace_closed"
                | "workspace_renamed"
                | "workspace_moved"
                | "workspace_reordered"
                | "tab_created"
                | "tab_closed"
                | "tab_renamed"
                | "tab_moved"
                | "pane_moved"
                // Keep dotted spellings for compatibility with older runtimes.
                | "workspace.created"
                | "workspace.updated"
                | "workspace.metadata_updated"
                | "workspace.closed"
                | "workspace.renamed"
                | "workspace.moved"
                | "workspace.reordered"
                | "tab.created"
                | "tab.closed"
                | "tab.renamed"
                | "tab.moved"
                | "pane.moved"
        )
    }

    pub fn refreshes_tab_surface_projection(&self) -> bool {
        matches!(
            self.event.as_str(),
            "pane_created"
                | "pane_closed"
                | "pane_exited"
                | "pane_moved"
                | "pane.created"
                | "pane.closed"
                | "pane.exited"
                | "pane.moved"
        )
    }

    pub fn affected_workspace_id(&self) -> Option<String> {
        self.data
            .get("workspace_id")
            .and_then(Value::as_str)
            .or_else(|| {
                self.data
                    .get("workspace")
                    .and_then(|workspace| workspace.get("workspace_id"))
                    .and_then(Value::as_str)
            })
            .or_else(|| {
                self.data
                    .get("tab")
                    .and_then(|tab| tab.get("workspace_id"))
                    .and_then(Value::as_str)
            })
            .or_else(|| {
                self.data
                    .get("pane")
                    .and_then(|pane| pane.get("workspace_id"))
                    .and_then(Value::as_str)
            })
            .or_else(|| {
                self.data
                    .get("layout")
                    .and_then(|layout| layout.get("workspace_id"))
                    .and_then(Value::as_str)
            })
            .map(str::to_string)
    }

    pub fn updated_layout(&self) -> Option<PaneLayout> {
        if !matches!(self.event.as_str(), "layout_updated" | "layout.updated") {
            return None;
        }
        serde_json::from_value(self.data.get("layout")?.clone()).ok()
    }

    pub fn pane_scroll_patch(&self) -> Option<PaneScrollPatch> {
        let data = match self.event.as_str() {
            "pane_scroll_changed" | "pane.scroll_changed" => &self.data,
            "pane_updated" | "pane.updated" => self.data.get("pane")?,
            _ => return None,
        };
        Some(PaneScrollPatch {
            pane_id: data.get("pane_id")?.as_str()?.to_string(),
            workspace_id: data.get("workspace_id")?.as_str()?.to_string(),
            scroll: serde_json::from_value(data.get("scroll")?.clone()).ok()?,
        })
    }

    pub fn agent_status_patch(&self) -> Option<AgentStatusPatch> {
        let data = match self.event.as_str() {
            "pane_agent_status_changed" | "pane.agent_status_changed" => &self.data,
            "pane_updated" | "pane.updated" => self.data.get("pane")?,
            _ => return None,
        };
        let nullable_string = |key: &str| match data.get(key) {
            None => None,
            Some(Value::Null) => Some(None),
            Some(Value::String(value)) => Some(Some(value.clone())),
            Some(_) => None,
        };
        let nullable_agent_session = match data.get("agent_session") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(value) => serde_json::from_value(value.clone()).ok().map(Some),
        };
        Some(AgentStatusPatch {
            pane_id: data.get("pane_id")?.as_str()?.to_string(),
            workspace_id: data.get("workspace_id")?.as_str()?.to_string(),
            agent_status: nullable_string("agent_status"),
            agent_session: nullable_agent_session,
            name: nullable_string("name"),
            agent: nullable_string("agent"),
            display_agent: nullable_string("display_agent"),
            title: nullable_string("title"),
            custom_status: nullable_string("custom_status"),
            tab_id: nullable_string("tab_id"),
            cwd: nullable_string("cwd"),
            foreground_cwd: nullable_string("foreground_cwd"),
            focused: data.get("focused").and_then(Value::as_bool),
        })
    }

    pub fn refreshes_agents(&self) -> bool {
        matches!(
            self.event.as_str(),
            "pane_updated"
                | "pane_closed"
                | "pane_exited"
                | "pane_moved"
                | "pane_agent_detected"
                | "pane_agent_status_changed"
                | "pane.updated"
                | "pane.closed"
                | "pane.exited"
                | "pane.moved"
                | "pane.agent_detected"
                | "pane.agent_status_changed"
        )
    }
}

// --- API list responses ---

#[derive(Clone, Debug, Deserialize)]
struct PingResponse {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    protocol: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceList {
    workspaces: Vec<Workspace>,
}

#[derive(Debug, Deserialize)]
struct TabList {
    tabs: Vec<Tab>,
}

#[derive(Debug, Deserialize)]
struct PaneList {
    panes: Vec<Pane>,
}

#[derive(Debug, Deserialize)]
struct AgentList {
    agents: Vec<Agent>,
}

#[derive(Debug, Deserialize)]
struct PaneReadResponse {
    read: PaneReadResult,
}

/// `agent.wait` reply (Herdr 0.8.2 ABI): a typed `agent_info` object carrying
/// the Agent projection at the moment the wait satisfied — not an event
/// envelope.
#[derive(Debug, Deserialize)]
struct AgentWaitResponse {
    // Kept (unused field): the vendored Herdr `agent.wait` reply is a typed
    // `agent_info` envelope; pinning `type` documents and validates that ABI.
    #[serde(rename = "type")]
    _kind: String,
    #[serde(default)]
    agent: Option<Agent>,
}

#[derive(Debug, Deserialize)]
struct PaneLayoutResponse {
    layout: PaneLayout,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    result: Option<T>,
    error: Option<ApiError>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    code: Option<String>,
    message: Option<String>,
}

#[derive(Clone)]
pub struct HerdrClient {
    socket_path: PathBuf,
    runtime_version: Option<String>,
    protocol: Option<u32>,
    /// True only when this client bootstrap actually started the Herdr server with an explicit
    /// caller-supplied config path. It is never inferred for a pre-existing user server.
    server_started_with_supplied_config: bool,
}

impl HerdrClient {
    /// Test-only constructor: direct socket path, bypasses bootstrap (for mock socket tests).
    pub fn for_test_socket(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            runtime_version: None,
            protocol: Some(20),
            server_started_with_supplied_config: false,
        }
    }

    pub fn bootstrap() -> Result<Self, HerdrError> {
        Self::bootstrap_with_server_config_path(None)
    }

    /// Bootstrap a client for one named Herdr session (one session = one
    /// workspace under the multi-instance model; herdr's own `default`
    /// session is an ordinary member). The session's server is started when
    /// its socket is not alive; an already-running server is never restarted
    /// or mutated.
    pub fn bootstrap_for_session(session: &str) -> Result<Self, HerdrError> {
        Self::bootstrap_for_session_with_config_path(Some(session), None)
    }

    pub fn bootstrap_for_session_with_config_path(
        session: Option<&str>,
        server_config_path: Option<&Path>,
    ) -> Result<Self, HerdrError> {
        ensure_herdr_installed()?;
        let socket_path = Self::session_socket_path(session.unwrap_or("default"));
        let needs_server_start = !socket_path.exists()
            || (Self {
                socket_path: socket_path.clone(),
                runtime_version: None,
                protocol: None,
                server_started_with_supplied_config: false,
            })
            .ping()
            .is_err();
        let server_started_with_supplied_config = if needs_server_start {
            start_server_for_session(session, server_config_path)?;
            wait_for_socket(&socket_path)?;
            server_config_path.is_some()
        } else {
            false
        };
        let mut client = Self {
            socket_path,
            runtime_version: None,
            protocol: None,
            server_started_with_supplied_config,
        };
        let ping = client.ping_info()?;
        if let Some(protocol) = ping.protocol {
            if !supports_protocol(protocol) {
                return Err(HerdrError::IncompatibleProtocol {
                    min: MIN_SUPPORTED_PROTOCOL,
                    actual: protocol,
                });
            }
        }
        client.runtime_version = ping.version;
        client.protocol = ping.protocol.or(Some(MIN_SUPPORTED_PROTOCOL));
        Ok(client)
    }

    /// Socket path for a session name (herdr's `default` = the base socket).
    pub fn session_socket_path(session: &str) -> PathBuf {
        session_socket_path_for(session)
    }

    /// Bootstrap the shared Herdr runtime, applying `server_config_path` only when this call
    /// actually has to start a new server. An already-running user server is never reloaded or
    /// mutated; callers can therefore give a Shardlane-derived config without changing an
    /// existing system Herdr session.
    pub fn bootstrap_with_server_config_path(
        server_config_path: Option<&Path>,
    ) -> Result<Self, HerdrError> {
        Self::bootstrap_for_session_with_config_path(None, server_config_path)
    }

    /// Side-effect-free connect: returns an error when the socket is missing
    /// or ping fails; never starts a server. The remote API
    /// (shardlane-remote) uses this entry — when Herdr is down the outward
    /// semantics are host_unavailable, not starting a runtime on the user's
    /// behalf.
    pub fn connect() -> Result<Self, HerdrError> {
        let socket_path = socket_path();
        Self::connect_to(&socket_path)
    }

    /// Side-effect-free connect with an explicit socket path (for isolated
    /// test injection).
    pub fn connect_to(socket_path: &Path) -> Result<Self, HerdrError> {
        if !socket_path.exists() {
            return Err(HerdrError::SocketUnavailable(
                socket_path.display().to_string(),
                "socket not found; connect() never starts a server".to_string(),
            ));
        }
        let mut client = Self {
            socket_path: socket_path.to_path_buf(),
            runtime_version: None,
            protocol: None,
            server_started_with_supplied_config: false,
        };
        let ping = client.ping_info()?;
        if let Some(protocol) = ping.protocol {
            if !supports_protocol(protocol) {
                return Err(HerdrError::IncompatibleProtocol {
                    min: MIN_SUPPORTED_PROTOCOL,
                    actual: protocol,
                });
            }
        }
        client.runtime_version = ping.version;
        client.protocol = ping.protocol.or(Some(MIN_SUPPORTED_PROTOCOL));
        Ok(client)
    }

    fn ping_info(&self) -> Result<PingResponse, HerdrError> {
        self.call("ping", json!({}))
    }

    pub fn ping(&self) -> Result<(), HerdrError> {
        let _ = self.ping_info()?;
        Ok(())
    }

    /// Whether this client instance is attached to a Herdr server that this bootstrap call
    /// started with the supplied config path. False for every pre-existing/user-owned server.
    pub fn server_started_with_supplied_config(&self) -> bool {
        self.server_started_with_supplied_config
    }

    /// Cached protocol version negotiated at bootstrap (no socket RPC).
    pub fn protocol(&self) -> Option<u32> {
        self.protocol
    }

    /// Refresh only global Workspace/Tab navigation metadata.
    ///
    /// Runtime event reconciliation uses this lighter projection so background pane churn in
    /// unrelated Workspaces never forces the visible terminal surface through a full snapshot.
    pub fn navigation_state(&self) -> Result<NavigationState, HerdrError> {
        let workspaces: WorkspaceList = self.call("workspace.list", json!({}))?;
        let focused_workspace_id = workspaces
            .workspaces
            .iter()
            .find(|workspace| workspace.focused)
            .map(|workspace| workspace.workspace_id.clone())
            .or_else(|| {
                workspaces
                    .workspaces
                    .first()
                    .map(|workspace| workspace.workspace_id.clone())
            });
        let tabs: TabList = self.call("tab.list", json!({}))?;
        let focused_tab_id = tabs
            .tabs
            .iter()
            .find(|tab| tab.focused)
            .map(|tab| tab.tab_id.clone())
            .or_else(|| {
                focused_workspace_id.as_deref().and_then(|workspace_id| {
                    workspaces
                        .workspaces
                        .iter()
                        .find(|workspace| workspace.workspace_id == workspace_id)
                        .and_then(|workspace| workspace.active_tab_id.clone())
                })
            })
            .or_else(|| {
                focused_workspace_id.as_deref().and_then(|workspace_id| {
                    tabs.tabs
                        .iter()
                        .find(|tab| tab.workspace_id.as_deref() == Some(workspace_id))
                        .map(|tab| tab.tab_id.clone())
                })
            });
        Ok(NavigationState {
            focused_workspace_id,
            focused_tab_id,
            workspaces: workspaces.workspaces,
            tabs: tabs.tabs,
        })
    }

    /// Fetch only the state needed to render the client shell and currently visible terminal.
    ///
    /// Workspace, tab metadata, and Agent indexes remain global because Sidebar/search can show
    /// them without focusing their Workspace. Panes/layout stay scoped to the focused terminal
    /// surface so hidden terminal state never becomes an expensive global projection.
    pub fn visible_state(&self) -> Result<HerdrState, HerdrError> {
        let navigation = self.navigation_state()?;
        let focused_workspace_id = navigation.focused_workspace_id.clone();
        let focused_tab_id = navigation.focused_tab_id.clone();
        let workspace_panes: PaneList = if let Some(workspace_id) = focused_workspace_id.as_deref()
        {
            self.call("pane.list", json!({ "workspace_id": workspace_id }))?
        } else {
            PaneList { panes: Vec::new() }
        };
        let panes = focused_tab_id
            .as_deref()
            .map(|tab_id| {
                workspace_panes
                    .panes
                    .into_iter()
                    .filter(|pane| pane.tab_id.as_deref() == Some(tab_id))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let focused_pane_id = panes
            .iter()
            .find(|pane| pane.focused)
            .map(|pane| pane.pane_id.clone())
            .or_else(|| panes.first().map(|pane| pane.pane_id.clone()));

        let agents: AgentList = self
            .call("agent.list", json!({}))
            .unwrap_or(AgentList { agents: Vec::new() });
        let layouts = if let Some(pane_id) = focused_pane_id.as_deref() {
            self.call::<PaneLayoutResponse>("pane.layout", json!({ "pane_id": pane_id }))
                .map(|response| vec![response.layout])
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(HerdrState {
            focused_workspace_id,
            focused_tab_id,
            focused_pane_id,
            workspaces: navigation.workspaces,
            tabs: navigation.tabs,
            panes,
            agents: agents.agents,
            layouts,
            // The constructor baked the MIN_SUPPORTED_PROTOCOL fallback in
            // (C30): no re-encoding at the state build site.
            protocol: self.protocol,
            version: self.runtime_version.clone(),
        })
    }

    /// Host-level full projection state: workspace/tab/agent plus the panes
    /// of every workspace. Unlike `visible_state` (focus-scoped, visibility-
    /// disciplined), this is the minimal sufficient snapshot for remote
    /// bootstrap / cross-client projection; it excludes pane geometry
    /// layouts.
    pub fn host_bootstrap_state(&self) -> Result<HerdrState, HerdrError> {
        let navigation = self.navigation_state()?;
        let mut panes = Vec::new();
        for workspace in &navigation.workspaces {
            panes.extend(self.workspace_panes(&workspace.workspace_id)?);
        }
        let agents = self.agents()?;
        Ok(HerdrState {
            focused_workspace_id: None,
            focused_tab_id: None,
            focused_pane_id: None,
            workspaces: navigation.workspaces,
            tabs: navigation.tabs,
            panes,
            agents,
            layouts: Vec::new(),
            // The constructor baked the MIN_SUPPORTED_PROTOCOL fallback in
            // (C30): no re-encoding at the state build sites.
            protocol: self.protocol,
            version: self.runtime_version.clone(),
        })
    }

    /// Workspace/agent projection without the per-workspace `pane.list`
    /// fan-out (C32). Herdr's `pane.list` accepts a null `workspace_id`
    /// (protocol 20 `PaneListParams`), which is the all-workspaces form, so
    /// callers that only correlate workspaces/tabs/agents — project
    /// resolution, path→workspace matching — pay 4 RPCs
    /// (workspace.list + tab.list + pane.list + agent.list) instead of
    /// [`Self::host_bootstrap_state`]'s 3 + N (one per workspace).
    pub fn workspace_state(&self) -> Result<HerdrState, HerdrError> {
        let navigation = self.navigation_state()?;
        let panes: PaneList = self.call("pane.list", json!({}))?;
        let agents = self.agents()?;
        Ok(HerdrState {
            focused_workspace_id: None,
            focused_tab_id: None,
            focused_pane_id: None,
            workspaces: navigation.workspaces,
            tabs: navigation.tabs,
            panes: panes.panes,
            agents,
            layouts: Vec::new(),
            // The constructor baked the MIN_SUPPORTED_PROTOCOL fallback in
            // (C30): no re-encoding at the state build sites.
            protocol: self.protocol,
            version: self.runtime_version.clone(),
        })
    }

    /// List the current panes of one workspace (authoritative runtime projection).
    /// A controller EOF cannot distinguish "shell exited" from "taken over by
    /// another client"; use this to verify pane survival before any
    /// destructive decision (e.g. closing a workspace).
    pub fn workspace_panes(&self, workspace_id: &str) -> Result<Vec<Pane>, HerdrError> {
        let panes: PaneList = self.call("pane.list", json!({ "workspace_id": workspace_id }))?;
        Ok(panes.panes)
    }

    /// Refresh only the terminal surface owned by one tab.
    ///
    /// Tab navigation already knows the global Workspace/Tab metadata, so paying for
    /// workspace.list + tab.list + agent.list again on every focus would duplicate the
    /// event-driven shell reconciliation and visibly stall navigation.
    pub fn tab_surface_state(
        &self,
        workspace_id: &str,
        tab_id: &str,
    ) -> Result<TabSurfaceState, HerdrError> {
        let workspace_panes: PaneList =
            self.call("pane.list", json!({ "workspace_id": workspace_id }))?;
        let panes = workspace_panes
            .panes
            .into_iter()
            .filter(|pane| pane.tab_id.as_deref() == Some(tab_id))
            .collect::<Vec<_>>();
        let focused_pane_id = panes
            .iter()
            .find(|pane| pane.focused)
            .map(|pane| pane.pane_id.clone())
            .or_else(|| panes.first().map(|pane| pane.pane_id.clone()));
        let layouts = if let Some(pane_id) = focused_pane_id.as_deref() {
            self.call::<PaneLayoutResponse>("pane.layout", json!({ "pane_id": pane_id }))
                .map(|response| vec![response.layout])
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(TabSurfaceState {
            workspace_id: workspace_id.to_string(),
            tab_id: tab_id.to_string(),
            focused_pane_id,
            panes,
            layouts,
        })
    }

    pub fn agents(&self) -> Result<Vec<Agent>, HerdrError> {
        let agents: AgentList = self.call("agent.list", json!({}))?;
        Ok(agents.agents)
    }

    /// Manual claim: declares that a pane is running the given agent (herdr
    /// short id, e.g. "claude"/"pi"). state is "unknown" — the client does
    /// not guess status; herdr events correct it later.
    pub fn report_pane_agent(&self, pane_id: &str, agent: &str) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "pane.report_agent",
            json!({
                "pane_id": pane_id,
                "source": CLIENT_AGENT_REPORT_SOURCE,
                "agent": agent,
                "state": "unknown",
            }),
        )?;
        Ok(())
    }

    /// Manual denial: clears every agent determination/declaration on a pane
    /// (including herdr's own detection); the main correction path for
    /// false detections.
    pub fn clear_pane_agent_authority(&self, pane_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("pane.clear_agent_authority", json!({ "pane_id": pane_id }))?;
        Ok(())
    }

    pub fn start_agent(&self, params: &AgentStartParams) -> Result<AgentStartedResult, HerdrError> {
        self.call("agent.start", serde_json::to_value(params)?)
    }

    pub fn prompt_agent(
        &self,
        params: &AgentPromptParams,
    ) -> Result<AgentPromptedResult, HerdrError> {
        self.call("agent.prompt", serde_json::to_value(params)?)
    }

    pub fn read_agent(&self, params: &AgentReadParams) -> Result<PaneReadResult, HerdrError> {
        self.call::<PaneReadResponse>("agent.read", serde_json::to_value(params)?)
            .map(|response| response.read)
    }

    pub fn wait_for_agent(&self, params: &AgentWaitParams) -> Result<Agent, HerdrError> {
        let response =
            self.call::<AgentWaitResponse>("agent.wait", serde_json::to_value(params)?)?;
        response
            .agent
            .ok_or_else(|| HerdrError::Api("agent.wait returned no agent projection".to_string()))
    }

    /// Submit-confirmed semantic prompt.
    ///
    /// Error-policy (AC-02): the raw-Enter recovery applies to exactly ONE
    /// proven condition — Herdr's `agent_prompt_stalled`, meaning the text was
    /// accepted and parked in the composer but no state change was observed.
    /// Every other error returns without keys:
    /// - `agent_blocked`: Herdr rejected before any input was sent;
    /// - `agent_not_found` / validation errors: no prompt exists to recover;
    /// - `timeout` (server wait) and local socket read timeouts: the prompt
    ///   may already have been submitted — delivery is uncertain, so neither
    ///   a retry nor an Enter may run automatically.
    pub fn prompt_agent_confirmed(
        &self,
        params: &AgentPromptParams,
    ) -> Result<AgentPromptedResult, HerdrError> {
        match self.prompt_agent(params) {
            Ok(prompted) => Ok(prompted),
            Err(HerdrError::AgentPromptStalled(detail)) => {
                // The one proven recovery: the parked line is submitted with
                // ONE explicit Enter — the text is never retyped — against the
                // same target, and delivery is confirmed by observing a
                // lifecycle transition (Working/Done/Blocked). Once Enter is
                // accepted by the runtime, submission is no longer provably
                // absent; a lost confirmation must therefore be surfaced as
                // DeliveryUncertain, never as a retryable stalled rejection.
                if let Err(error) = self.send_agent_keys(&AgentSendKeysParams {
                    target: params.target.clone(),
                    keys: vec!["enter".to_string()],
                }) {
                    return Err(match error {
                        HerdrError::DeliveryUncertain(detail) => HerdrError::DeliveryUncertain(
                            format!("parked prompt Enter delivery uncertain: {detail}"),
                        ),
                        other => other,
                    });
                }
                match self.wait_for_agent(&AgentWaitParams {
                    target: params.target.clone(),
                    until: vec![
                        HerdrAgentStatus::Working,
                        HerdrAgentStatus::Done,
                        HerdrAgentStatus::Blocked,
                    ],
                    timeout_ms: Some(10_000),
                }) {
                    Ok(agent) => Ok(AgentPromptedResult { agent }),
                    Err(confirmation) => {
                        lag_log(format_args!(
                            "prompt parked-submit Enter confirmation failed: {confirmation}"
                        ));
                        Err(HerdrError::DeliveryUncertain(format!(
                            "parked prompt Enter was sent but submission confirmation was lost: {confirmation}; original stalled state: {detail}"
                        )))
                    }
                }
            }
            // Server-side wait timeout: the submission may already be running.
            Err(HerdrError::ApiTimeout(detail)) => Err(HerdrError::DeliveryUncertain(format!(
                "agent.prompt wait timed out; the prompt may have been accepted: {detail}"
            ))),
            // Local socket read timeout: the request was written; the server
            // may have typed and submitted the text. Never auto-Enter/retry.
            Err(HerdrError::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Err(HerdrError::DeliveryUncertain(format!(
                    "agent.prompt response not read (socket timeout); delivery uncertain: {error}"
                )))
            }
            Err(other) => Err(other),
        }
    }

    pub fn send_agent_keys(&self, params: &AgentSendKeysParams) -> Result<(), HerdrError> {
        if params.keys.is_empty() {
            return Ok(());
        }
        let _: Value = self.call("agent.send_keys", serde_json::to_value(params)?)?;
        Ok(())
    }

    /// Subscribe to global runtime events on a persistent socket.
    /// Pane-scoped events are subscribed separately once the current Pane set is known.
    pub fn subscribe_events(&self) -> Result<async_channel::Receiver<HerdrEvent>, HerdrError> {
        self.subscribe_event_specs(
            "shardlane-events",
            json!([
                {"type": "workspace.created"},
                {"type": "workspace.updated"},
                {"type": "workspace.metadata_updated"},
                {"type": "workspace.closed"},
                {"type": "workspace.renamed"},
                {"type": "workspace.moved"},
                {"type": "workspace.reordered"},
                {"type": "workspace.focused"},
                {"type": "tab.created"},
                {"type": "tab.closed"},
                {"type": "tab.renamed"},
                {"type": "tab.moved"},
                {"type": "tab.focused"},
                {"type": "pane.created"},
                {"type": "pane.closed"},
                {"type": "pane.updated"},
                {"type": "pane.focused"},
                {"type": "pane.exited"},
                {"type": "pane.moved"},
                {"type": "pane.agent_detected"},
                {"type": "layout.updated"},
            ]),
        )
    }

    /// Subscribe to Pane-scoped runtime events for the exact currently materialized Pane set.
    /// Protocols 19-20 require `pane_id` for both of these event kinds.
    pub fn subscribe_pane_events(
        &self,
        pane_ids: &[String],
    ) -> Result<async_channel::Receiver<HerdrEvent>, HerdrError> {
        let mut subscriptions = Vec::with_capacity(pane_ids.len().saturating_mul(2));
        for pane_id in pane_ids {
            subscriptions.push(json!({
                "type": "pane.scroll_changed",
                "pane_id": pane_id,
            }));
            subscriptions.push(json!({
                "type": "pane.agent_status_changed",
                "pane_id": pane_id,
            }));
        }
        self.subscribe_event_specs("shardlane-pane-events", Value::Array(subscriptions))
    }

    fn subscribe_event_specs(
        &self,
        id: &str,
        subscriptions: Value,
    ) -> Result<async_channel::Receiver<HerdrEvent>, HerdrError> {
        let mut stream = UnixStream::connect(&self.socket_path).map_err(|err| {
            HerdrError::SocketUnavailable(self.socket_path.display().to_string(), err.to_string())
        })?;
        // A bounded timeout lets a canceled GPUI subscription drop its receiver and lets the
        // socket reader notice that closure even when Herdr is otherwise idle.
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        let request = json!({
            "id": id,
            "method": "events.subscribe",
            "params": { "subscriptions": subscriptions }
        });
        writeln!(stream, "{request}")?;

        // events.subscribe is long-lived, but the first line is still a normal API response.
        // Validate it synchronously so an invalid subscription never masquerades as a live
        // event stream and leave startup waiting on a receiver that has already disconnected.
        let mut reader = BufReader::new(stream);
        let mut ack = String::new();
        reader.read_line(&mut ack)?;
        parse_subscription_ack(&ack)?;

        let (tx, rx) = async_channel::unbounded();
        thread::spawn(move || {
            let mut line = String::new();
            let mut dropped: u64 = 0;
            loop {
                if tx.is_closed() {
                    break;
                }
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) if line.trim().is_empty() => continue,
                    Ok(_) => match serde_json::from_str::<HerdrEvent>(&line) {
                        Ok(event) => {
                            dropped = 0;
                            if tx.send_blocking(event).is_err() {
                                break;
                            }
                        }
                        Err(_) => {
                            dropped += 1;
                            // Logarithmic throttling: no log flood during an
                            // envelope-drift storm, but kept continuously
                            // visible to avoid the blind spot of "the event
                            // stream silently dropping to zero while status
                            // still reads connected".
                            if dropped.is_power_of_two() {
                                lag_log(format_args!(
                                    "herdr.events dropped_unparseable total={dropped}"
                                ));
                            }
                        }
                    },
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

    pub fn split_right(&self, pane_id: &str) -> Result<Pane, HerdrError> {
        self.split_pane(pane_id, "right")
    }

    pub fn split_down(&self, pane_id: &str) -> Result<Pane, HerdrError> {
        self.split_pane(pane_id, "down")
    }

    /// Shared split wrapper (C12): `split_right`/`split_down` differ only in
    /// Herdr's direction literal.
    fn split_pane(&self, pane_id: &str, direction: &str) -> Result<Pane, HerdrError> {
        let response: PaneCreatedResponse =
            self.call("pane.split", pane_split_params(pane_id, direction))?;
        Ok(response.pane)
    }

    pub fn close_pane(&self, pane_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("pane.close", json!({ "pane_id": pane_id }))?;
        Ok(())
    }

    pub fn toggle_pane_zoom(&self, pane_id: &str) -> Result<PaneLayoutActionResult, HerdrError> {
        let response: PaneZoomResponse =
            self.call("pane.zoom", json!({ "pane_id": pane_id, "mode": "toggle" }))?;
        Ok(response.zoom)
    }

    /// Focus a pane directly. Protocol: pane.focus + PaneTarget {pane_id}.
    /// Protocol 20 exposes direct focus; the historical zoom-toggle focus
    /// workaround must not be restored (2026-08-26 audit TUI-05).
    pub fn pane_focus(&self, pane_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("pane.focus", json!({ "pane_id": pane_id }))?;
        Ok(())
    }

    /// Focus a workspace (TUI mode navigation chain step 1).
    /// Protocol: workspace.focus + WorkspaceTarget.
    pub fn workspace_focus(&self, workspace_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("workspace.focus", json!({ "workspace_id": workspace_id }))?;
        Ok(())
    }

    /// Focus a tab. Protocol: tab.focus + TabTarget.
    pub fn tab_focus(&self, tab_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("tab.focus", json!({ "tab_id": tab_id }))?;
        Ok(())
    }

    /// Focus an agent by terminal_id (AgentTarget; caller falls back to workspace/tab/pane chain on failure).
    pub fn agent_focus(&self, target: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("agent.focus", json!({ "target": target }))?;
        Ok(())
    }

    pub fn read_pane_recent_ansi(&self, pane_id: &str, lines: u32) -> Result<String, HerdrError> {
        let response: PaneReadResponse =
            self.call("pane.read", pane_read_recent_params(pane_id, lines))?;
        Ok(response.read.text)
    }

    pub fn set_split_ratio(
        &self,
        tab_id: &str,
        path: &[bool],
        ratio: f64,
    ) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "layout.set_split_ratio",
            layout_set_split_ratio_params(tab_id, path, ratio),
        )?;
        Ok(())
    }

    pub fn create_tab(&self, workspace_id: Option<&str>) -> Result<TabCreatedResult, HerdrError> {
        self.create_tab_at(workspace_id, None)
    }

    pub fn create_tab_at(
        &self,
        workspace_id: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<TabCreatedResult, HerdrError> {
        self.create_tab_at_with_focus(workspace_id, cwd, true)
    }

    /// Create a tab without changing Herdr's global focus. Remote/mobile mutations use
    /// this explicit form so a client-local navigation cannot move the Mac surface.
    pub fn create_tab_at_with_focus(
        &self,
        workspace_id: Option<&str>,
        cwd: Option<&str>,
        focus: bool,
    ) -> Result<TabCreatedResult, HerdrError> {
        self.call(
            "tab.create",
            tab_create_params_with_focus(workspace_id, cwd, focus),
        )
    }

    pub fn move_tab(&self, tab_id: &str, insert_index: usize) -> Result<(), HerdrError> {
        let result: Value = self.call(
            "tab.move",
            json!({ "tab_id": tab_id, "insert_index": insert_index }),
        )?;
        // Protocol 20 schema contract: the tab.move success result is a bare
        // ok discriminator (no authoritative payload); authoritative
        // correction arrives via the tab.moved event (carrying the whole
        // workspace TabInfo list).
        if !tab_move_result_is_ok(&result) {
            return Err(HerdrError::Api(format!(
                "tab.move returned unexpected result shape: {result}"
            )));
        }
        Ok(())
    }

    pub fn close_tab(&self, tab_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("tab.close", json!({ "tab_id": tab_id }))?;
        Ok(())
    }

    pub fn move_workspace(
        &self,
        workspace_id: &str,
        insert_index: usize,
    ) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "workspace.move",
            json!({ "workspace_id": workspace_id, "insert_index": insert_index }),
        )?;
        Ok(())
    }

    pub fn move_workspace_before(
        &self,
        workspace_id: &str,
        before_workspace_id: &str,
    ) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "workspace.move_block",
            json!({
                "workspace_ids": [workspace_id],
                "before_workspace_id": before_workspace_id,
            }),
        )?;
        Ok(())
    }

    pub fn create_workspace_at(
        &self,
        cwd: Option<&str>,
    ) -> Result<WorkspaceCreatedResult, HerdrError> {
        self.create_workspace_at_with_focus(cwd, true)
    }

    pub fn create_workspace_at_with_focus(
        &self,
        cwd: Option<&str>,
        focus: bool,
    ) -> Result<WorkspaceCreatedResult, HerdrError> {
        self.call("workspace.create", json!({ "cwd": cwd, "focus": focus }))
    }

    pub fn rename_workspace(&self, workspace_id: &str, label: &str) -> Result<(), HerdrError> {
        let _: Value = self.call(
            "workspace.rename",
            json!({ "workspace_id": workspace_id, "label": label }),
        )?;
        Ok(())
    }

    pub fn rename_tab(&self, tab_id: &str, label: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("tab.rename", json!({ "tab_id": tab_id, "label": label }))?;
        Ok(())
    }

    pub fn rename_pane(&self, pane_id: &str, label: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("pane.rename", json!({ "pane_id": pane_id, "label": label }))?;
        Ok(())
    }

    pub fn move_pane_to_new_tab(
        &self,
        pane_id: &str,
        workspace_id: &str,
    ) -> Result<PaneMoveResult, HerdrError> {
        self.call::<PaneMoveResponse>("pane.move", pane_move_new_tab_params(pane_id, workspace_id))
            .map(|response| response.move_result)
    }

    pub fn move_pane_to_tab(
        &self,
        pane_id: &str,
        tab_id: &str,
    ) -> Result<PaneMoveResult, HerdrError> {
        self.call::<PaneMoveResponse>("pane.move", pane_move_tab_params(pane_id, tab_id))
            .map(|response| response.move_result)
    }

    pub fn close_workspace(&self, workspace_id: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("workspace.close", json!({ "workspace_id": workspace_id }))?;
        Ok(())
    }

    pub fn pane_layout(&self, pane_id: &str) -> Result<PaneLayout, HerdrError> {
        self.call::<PaneLayoutResponse>("pane.layout", json!({ "pane_id": pane_id }))
            .map(|response| response.layout)
    }

    pub fn send_keys(&self, pane_id: &str, keys: &[String]) -> Result<(), HerdrError> {
        if keys.is_empty() {
            return Ok(());
        }
        let _: Value = self.call("pane.send_keys", pane_send_keys_params(pane_id, keys))?;
        Ok(())
    }

    pub fn send_text(&self, pane_id: &str, text: &str) -> Result<(), HerdrError> {
        let _: Value = self.call("pane.send_text", pane_send_text_params(pane_id, text))?;
        Ok(())
    }

    pub fn resize_pane(
        &self,
        pane_id: &str,
        direction: &str,
    ) -> Result<PaneLayoutActionResult, HerdrError> {
        self.call::<PaneResizeResponse>(
            "pane.resize",
            json!({ "pane_id": pane_id, "direction": direction, "amount": 0.05 }),
        )
        .map(|response| response.resize)
    }

    pub fn swap_pane(
        &self,
        pane_id: &str,
        direction: &str,
    ) -> Result<PaneLayoutActionResult, HerdrError> {
        self.call::<PaneSwapResponse>(
            "pane.swap",
            json!({ "source_pane_id": pane_id, "direction": direction }),
        )
        .map(|response| response.swap)
    }

    pub fn pane_process_info(&self, pane_id: &str) -> Result<PaneProcessInfo, HerdrError> {
        self.call::<PaneProcessInfoResponse>("pane.process_info", json!({ "pane_id": pane_id }))
            .map(|response| response.process_info)
    }

    pub fn reload_config(&self) -> Result<(), HerdrError> {
        let _: Value = self.call("server.reload_config", json!({}))?;
        Ok(())
    }

    fn call<T: DeserializeOwned>(&self, method: &str, params: Value) -> Result<T, HerdrError> {
        let started = Instant::now();
        let stream = UnixStream::connect(&self.socket_path).map_err(|err| {
            HerdrError::SocketUnavailable(self.socket_path.display().to_string(), err.to_string())
        })?;
        // Same read timeout as the subscription path (see rpc_read_timeout):
        // a hung RPC must be able to fail, otherwise the navigation overlay
        // and a manual Refresh's ping would freeze forever.
        stream.set_read_timeout(Some(rpc_read_timeout(method, &params)))?;
        // C02 privacy: capture the optional params trace BEFORE the request is
        // built. The default log line never includes params — agent.prompt
        // carries the user's full prompt text, pane.send_text terminal text,
        // agent.start argv — and the lag log is world-readable /tmp.
        let params_trace = crate::diagnostics::rpc_params_trace_enabled()
            .then(|| truncate_params_for_trace(&params));
        let request = json!({ "id": "shardlane", "method": method, "params": params });
        let mut reader = BufReader::new(stream);
        // R2-01/CR-04: make the request phase explicit. A failure before the
        // request was written = definitely not accepted; any lost response
        // after the request was written successfully (EOF/connection
        // reset/truncation/timeout) on a mutating RPC means "the server may
        // have accepted it" → DeliveryUncertain; the caller must never retry
        // or apply destructive compensation.
        let mutating = is_mutating_rpc(method);
        if let Err(error) = writeln!(reader.get_mut(), "{request}") {
            return Err(HerdrError::Io(error));
        }
        let mut line = String::new();
        let read = reader.read_line(&mut line);
        let response = match read {
            Ok(0) => {
                return Err(post_write_loss(
                    mutating,
                    "connection closed before a response was read",
                ))
            }
            Ok(_) => match serde_json::from_str::<ApiResponse<T>>(&line) {
                Ok(response) => response,
                Err(error) => {
                    return Err(if mutating {
                        HerdrError::DeliveryUncertain(format!(
                            "malformed/truncated response after the mutation was written: {error}"
                        ))
                    } else {
                        HerdrError::Json(error)
                    })
                }
            },
            Err(error) => {
                return Err(if mutating {
                    HerdrError::DeliveryUncertain(format!(
                        "response not read after the mutation was written: {error}"
                    ))
                } else {
                    HerdrError::Io(error)
                })
            }
        };
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let bytes = line.len();
        match &params_trace {
            Some(params) => lag_log(format_args!(
                "herdr.call {method} {ms:.2}ms resp_bytes={bytes} params={params}"
            )),
            None => lag_log(format_args!(
                "herdr.call {method} {ms:.2}ms resp_bytes={bytes}"
            )),
        }
        if let Some(error) = response.error {
            let message = error.message.unwrap_or_else(|| "unknown error".to_string());
            Err(match error.code.as_deref() {
                Some("agent_not_found") => HerdrError::AgentNotFound(message),
                Some("agent_blocked") => HerdrError::AgentBlocked(message),
                Some("agent_prompt_stalled") => HerdrError::AgentPromptStalled(message),
                Some("timeout") => HerdrError::ApiTimeout(message),
                _ => HerdrError::Api(message),
            })
        } else if let Some(result) = response.result {
            Ok(result)
        } else if mutating {
            // R3: a legal JSON envelope with neither result nor error after
            // a mutation was written is still an ambiguous outcome — the
            // server may have acted. Never classify it as a definitive
            // failure (which could tear down a valid Agent's tab).
            Err(HerdrError::DeliveryUncertain(
                "response carried neither result nor error after the mutation was written"
                    .to_string(),
            ))
        } else {
            Err(HerdrError::Api("missing result".to_string()))
        }
    }
}

/// C02: trace-only upper bound for logged RPC params, so even opt-in
/// diagnostics stay bounded and never dump full transcripts.
const TRACE_PARAMS_MAX_CHARS: usize = 200;

fn truncate_params_for_trace(params: &Value) -> String {
    let rendered = params.to_string();
    if rendered.chars().count() <= TRACE_PARAMS_MAX_CHARS {
        return rendered;
    }
    rendered
        .chars()
        .take(TRACE_PARAMS_MAX_CHARS)
        .collect::<String>()
        + "…"
}

/// Server-side state-mutating RPCs. For these, a lost response after the
/// request bytes were written is delivery-uncertain, never a definitive
/// failure (R2-01/CR-04).
fn is_mutating_rpc(method: &str) -> bool {
    // Complete set of mutating RPCs observed on the Herdr client surface
    // (every call site in this file, audited R3; C03 added the layout/pane
    // geometry mutations whose lost responses must stay delivery-uncertain).
    // Reads (agent.list, pane.list/read/layout/process_info, tab.list,
    // workspace.list, ping) are excluded: a lost read response is a plain
    // failure, never delivery uncertainty. The contract test
    // `every_rpc_method_used_in_this_file_is_classified_exactly_once` proves
    // every method string used by this file's wrappers falls into exactly one
    // of the mutating/read classes.
    matches!(
        method,
        "agent.start"
            | "agent.prompt"
            | "agent.send_keys"
            | "agent.stop"
            | "agent.focus"
            | "pane.send_keys"
            | "pane.send_text"
            | "pane.write"
            | "pane.report_agent"
            | "pane.clear_agent_authority"
            | "pane.close"
            | "pane.focus"
            | "pane.move"
            | "pane.rename"
            | "pane.resize"
            | "pane.split"
            | "pane.swap"
            | "pane.zoom"
            | "layout.set_split_ratio"
            | "tab.create"
            | "tab.close"
            | "tab.focus"
            | "tab.rename"
            | "tab.move"
            | "tab.split"
            | "workspace.create"
            | "workspace.close"
            | "workspace.focus"
            | "workspace.move"
            | "workspace.move_block"
            | "workspace.rename"
            | "workspace.reorder"
            | "server.reload_config"
    )
}

/// Read-only RPCs observed on the Herdr client surface. Kept beside
/// `is_mutating_rpc` so the classification contract test can prove every
/// method used in this file is in exactly one class.
#[cfg(test)]
const READ_METHODS: &[&str] = &[
    "ping",
    "workspace.list",
    "tab.list",
    "pane.list",
    "pane.layout",
    "pane.read",
    "pane.process_info",
    "agent.list",
    "agent.read",
    "agent.wait",
];

fn post_write_loss(mutating: bool, detail: &str) -> HerdrError {
    if mutating {
        HerdrError::DeliveryUncertain(format!("the mutation may already be accepted ({detail})"))
    } else {
        HerdrError::Api(detail.to_string())
    }
}

impl AgentRuntime for HerdrClient {
    type Error = HerdrError;

    fn start_runtime_agent(
        &self,
        request: &RuntimeAgentStartRequest,
    ) -> Result<RuntimeAgent, Self::Error> {
        let AgentStartedResult { agent, _argv: _ } = self.start_agent(&AgentStartParams {
            name: request.name.clone(),
            kind: request.kind.clone(),
            pane_id: request.pane_id.as_str().to_string(),
            args: request.args.clone(),
            timeout_ms: request.timeout_ms,
        })?;
        runtime_agent_from_herdr(agent)
    }

    fn prompt_runtime_agent(
        &self,
        request: &RuntimeAgentPromptRequest,
    ) -> Result<RuntimeAgent, Self::Error> {
        let wait = (!request.wait_until.is_empty() || request.timeout_ms.is_some()).then(|| {
            AgentPromptWaitOptions {
                until: request
                    .wait_until
                    .iter()
                    .copied()
                    .map(herdr_agent_status)
                    .collect(),
                timeout_ms: request.timeout_ms,
            }
        });
        let prompted = self.prompt_agent(&AgentPromptParams {
            target: request.agent_id.as_str().to_string(),
            text: request.text.clone(),
            wait,
        })?;
        runtime_agent_from_herdr(prompted.agent)
    }

    fn read_runtime_agent(
        &self,
        request: &RuntimeAgentReadRequest,
    ) -> Result<RuntimeAgentRead, Self::Error> {
        let (herdr_format, strip) = match request.format {
            RuntimeAgentReadFormat::Text => (HerdrReadFormat::Text, true),
            RuntimeAgentReadFormat::Ansi => (HerdrReadFormat::Ansi, false),
        };
        let read = self.read_agent(&AgentReadParams {
            target: request.agent_id.as_str().to_string(),
            source: HerdrReadSource::RecentUnwrapped,
            lines: request.lines,
            format: herdr_format,
            strip_ansi: strip,
        })?;
        Ok(RuntimeAgentRead {
            agent_id: request.agent_id.clone(),
            text: read.text,
            revision: read.revision,
            truncated: read.truncated,
        })
    }

    fn wait_runtime_agent(
        &self,
        request: &RuntimeAgentWaitRequest,
    ) -> Result<RuntimeAgentWait, Self::Error> {
        let agent = self.wait_for_agent(&AgentWaitParams {
            target: request.agent_id.as_str().to_string(),
            until: request
                .until
                .iter()
                .copied()
                .map(herdr_agent_status)
                .collect(),
            timeout_ms: request.timeout_ms,
        })?;
        // The vendored Herdr 0.8.2 `agent.wait` reply carries the settled
        // Agent projection (`type: "agent_info"`), not an event envelope.
        Ok(RuntimeAgentWait {
            status: agent
                .agent_status
                .as_deref()
                .map(|status| host_agent_status(Some(status))),
            event: "agent_info".to_string(),
        })
    }

    fn send_runtime_agent_keys(
        &self,
        agent_id: &HostAgentRef,
        keys: &[String],
    ) -> Result<(), Self::Error> {
        self.send_agent_keys(&AgentSendKeysParams {
            target: agent_id.as_str().to_string(),
            keys: keys.to_vec(),
        })
    }
}

fn runtime_agent_from_herdr(agent: Agent) -> Result<RuntimeAgent, HerdrError> {
    let runtime_workspace_id = agent
        .workspace_id
        .clone()
        .ok_or_else(|| HerdrError::Api("agent response missing workspace_id".to_string()))?;
    let tab_id = agent
        .tab_id
        .clone()
        .ok_or_else(|| HerdrError::Api("agent response missing tab_id".to_string()))?;
    let pane_id = agent
        .pane_id
        .clone()
        .ok_or_else(|| HerdrError::Api("agent response missing pane_id".to_string()))?;
    Ok(RuntimeAgent {
        id: HostAgentRef::new(pane_id.clone()),
        runtime_workspace_id,
        tab_id,
        pane_id,
        name: agent.name,
        kind: agent.agent,
        title: agent.title,
        status: host_agent_status(agent.agent_status.as_deref()),
        interactive_ready: agent.interactive_ready,
        revision: agent.revision,
    })
}

fn herdr_agent_status(status: HostAgentStatus) -> HerdrAgentStatus {
    match status {
        HostAgentStatus::Idle => HerdrAgentStatus::Idle,
        HostAgentStatus::Working => HerdrAgentStatus::Working,
        HostAgentStatus::Blocked => HerdrAgentStatus::Blocked,
        HostAgentStatus::Done => HerdrAgentStatus::Done,
        HostAgentStatus::Unknown => HerdrAgentStatus::Unknown,
    }
}

pub fn host_agent_status(status: Option<&str>) -> HostAgentStatus {
    match status {
        Some("idle") => HostAgentStatus::Idle,
        Some("working") => HostAgentStatus::Working,
        Some("blocked") => HostAgentStatus::Blocked,
        Some("done") => HostAgentStatus::Done,
        _ => HostAgentStatus::Unknown,
    }
}

// --- Event projection ---

fn tab_move_result_is_ok(result: &Value) -> bool {
    result.get("type").and_then(Value::as_str) == Some("ok")
}

fn pane_split_params(pane_id: &str, direction: &str) -> Value {
    json!({
        "target_pane_id": pane_id,
        "direction": direction,
        "focus": true,
    })
}

// ----------------------------------------------------------------------------
// RPC parameter constructors (pure functions) and read-timeout policy
//
// The request shapes are the verified protocol 19-20 contract: inline json!
// literals cannot be pinned by tests, so they are extracted here and guarded
// byte-for-byte by the request_param_shapes_match_verified_protocol_facades
// contract test. Compare against the live schema before changing any shape.
// ----------------------------------------------------------------------------

/// Read-timeout ceiling for ordinary RPCs. Herdr is a local unix socket;
/// normal calls return in milliseconds. Exceeding this ceiling is treated as
/// the server being unreachable, letting callers take the existing
/// error/recovery paths instead of hanging forever.
const RPC_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Fallback for `agent.wait` without an explicit timeout: covers Herdr's
/// default wait plus margin.
const AGENT_WAIT_DEFAULT_TIMEOUT_MS: u64 = 60_000;
const RPC_TIMEOUT_MARGIN_MS: u64 = 5_000;

fn rpc_read_timeout(method: &str, params: &Value) -> Duration {
    if method == "agent.wait" {
        let wait_ms = params
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(AGENT_WAIT_DEFAULT_TIMEOUT_MS);
        Duration::from_millis(wait_ms + RPC_TIMEOUT_MARGIN_MS)
    } else if method == "agent.start" {
        // AC-01: `agent.start` waits server-side for interactive readiness
        // (up to timeout_ms). The local read timeout must cover it, or one
        // successful-but-slow start would be misread as a client failure,
        // triggering the wrong retry/reap path.
        params
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .map(|start_ms| Duration::from_millis(start_ms + RPC_TIMEOUT_MARGIN_MS))
            .unwrap_or(RPC_READ_TIMEOUT)
    } else if method == "agent.prompt" {
        // A prompt with a submit-confirmation wait blocks on herdr's side for
        // up to wait.timeout_ms; the socket read must outlive it.
        params
            .get("wait")
            .and_then(|wait| wait.get("timeout_ms"))
            .and_then(Value::as_u64)
            .map(|wait_ms| Duration::from_millis(wait_ms + RPC_TIMEOUT_MARGIN_MS))
            .unwrap_or(RPC_READ_TIMEOUT)
    } else {
        RPC_READ_TIMEOUT
    }
}

fn pane_read_recent_params(pane_id: &str, lines: u32) -> Value {
    json!({
        "pane_id": pane_id,
        "source": "recent",
        "lines": lines,
        "format": "ansi",
        "strip_ansi": false
    })
}

fn layout_set_split_ratio_params(tab_id: &str, path: &[bool], ratio: f64) -> Value {
    json!({
        "tab_id": tab_id,
        "path": path,
        "ratio": ratio.clamp(0.05, 0.95)
    })
}

fn tab_create_params_with_focus(
    workspace_id: Option<&str>,
    cwd: Option<&str>,
    focus: bool,
) -> Value {
    json!({ "workspace_id": workspace_id, "cwd": cwd, "focus": focus })
}

#[cfg(test)]
fn tab_create_params(workspace_id: Option<&str>, cwd: Option<&str>) -> Value {
    tab_create_params_with_focus(workspace_id, cwd, true)
}

fn pane_move_new_tab_params(pane_id: &str, workspace_id: &str) -> Value {
    json!({
        "pane_id": pane_id,
        "destination": {
            "type": "new_tab",
            "workspace_id": workspace_id
        },
        "focus": true
    })
}

fn pane_move_tab_params(pane_id: &str, tab_id: &str) -> Value {
    json!({
        "pane_id": pane_id,
        "destination": {
            "type": "tab",
            "tab_id": tab_id,
            "split": "right",
            "target_pane_id": null
        },
        "focus": true
    })
}

fn pane_send_keys_params(pane_id: &str, keys: &[String]) -> Value {
    json!({ "pane_id": pane_id, "keys": keys })
}

fn pane_send_text_params(pane_id: &str, text: &str) -> Value {
    json!({ "pane_id": pane_id, "text": text })
}

fn parse_subscription_ack(line: &str) -> Result<(), HerdrError> {
    let response: ApiResponse<Value> = serde_json::from_str(line)?;
    if let Some(error) = response.error {
        return Err(HerdrError::Api(
            error.message.unwrap_or_else(|| "unknown error".to_string()),
        ));
    }
    let Some(result) = response.result else {
        return Err(HerdrError::Api(
            "events.subscribe returned no result".to_string(),
        ));
    };
    if result.get("type").and_then(Value::as_str) != Some("subscription_started") {
        return Err(HerdrError::Api(format!(
            "unexpected events.subscribe response: {result}"
        )));
    }
    Ok(())
}

impl HerdrState {
    /// Get the layout for a given tab.
    pub fn layout_for_tab(&self, tab_id: &str) -> Option<&PaneLayout> {
        self.layouts.iter().find(|l| l.tab_id == tab_id)
    }
}

// ---- CLI discovery ----
// A packaged .app launched from Finder/Dock only sees launchd's system PATH
// (/usr/bin:/bin:/usr/sbin:/sbin); herdr (installed by wax into ~/.local/bin
// and similar) and wax (~/.cargo/bin) are not on it, so a bare `Command::new`
// gets io NotFound (the packaged app's "io: No such file or directory
// (os error 2)").
// Resolution order: process PATH (zero overhead for terminal/dev launches) →
// login shell PATH (zsh -lic, cached once per process, the same proven
// pattern as agent_cli::resolve_binary) → conventional user-level bin
// directories as a fallback.

/// Cache for the login shell PATH query (`zsh -lic` costs ~100ms; run once).
static LOGIN_SHELL_PATH: OnceLock<Option<String>> = OnceLock::new();

/// Finds an executable in a colon-separated PATH value (pure, testable).
fn find_in_path_value(path_value: &str, name: &str) -> Option<PathBuf> {
    path_value
        .split(':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(dir).join(name))
        .find(|candidate| candidate.is_file())
}

/// User-level CLI install directories (wax/cargo/homebrew conventions): the
/// fallback when the login shell is unavailable.
fn fallback_cli_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".wax/bin"));
        dirs.push(home.join(".cargo/bin"));
    }
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs
}

/// The login shell's full PATH. When the packaged .app's process PATH lacks
/// user-level directories, `zsh -lic` sources ~/.zprofile and friends to get
/// the user's real PATH; stdout noise from .zshrc only pollutes the first
/// segment and does not affect hits on later real directories.
fn login_shell_path() -> Option<String> {
    LOGIN_SHELL_PATH
        .get_or_init(|| {
            let output = Command::new("/bin/zsh")
                .args(["-lic", "printf %s \"$PATH\""])
                .stdin(Stdio::null())
                .output()
                .ok()?;
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            (!path.is_empty()).then_some(path)
        })
        .clone()
}

/// Resolves the absolute path of a user-level installed CLI (shared by
/// herdr/wax).
pub fn resolve_user_cli(name: &str) -> Option<PathBuf> {
    if let Some(path) = env::var_os("PATH") {
        if let Some(path) = path.to_str() {
            if let Some(found) = find_in_path_value(path, name) {
                return Some(found);
            }
        }
    }
    if let Some(login_path) = login_shell_path() {
        if let Some(found) = find_in_path_value(&login_path, name) {
            return Some(found);
        }
    }
    fallback_cli_directories()
        .into_iter()
        .find(|dir| dir.join(name).is_file())
        .map(|dir| dir.join(name))
}

/// Absolute path of the herdr CLI. Terminal launches hit the process PATH
/// directly with zero extra overhead; GUI terminal session control
/// (terminal_stream.rs) and bootstrap share this entry point.
pub fn herdr_cli_path() -> Option<PathBuf> {
    resolve_user_cli("herdr")
}

/// Environment keys that would make a child believe it is running inside an
/// existing Herdr pane. A Host-owned TUI child must be a normal Herdr client,
/// while retaining the user's config and socket discovery.
pub const HERDR_TUI_STRIP_ENV_KEYS: &[&str] = &[
    "HERDR_ENV",
    "HERDR_BIN_PATH",
    "HERDR_WORKSPACE_ID",
    "HERDR_TAB_ID",
    "HERDR_PANE_ID",
    "HERDR_STARTUP_CWD",
    "HERDR_CLIENT_SOCKET_PATH",
    "HERDR_SESSION",
    "TERM",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERMINFO",
    "COLORTERM",
];

/// Build a spawn environment for a Host-owned Herdr TUI child. This pure seam
/// is shared by the native and Remote surfaces so neither client invents a
/// second runtime or leaks a pane identity into the child.
pub fn sanitized_tui_env(
    source: &std::collections::HashMap<String, String>,
) -> Vec<(String, String)> {
    source
        .iter()
        .filter(|(key, _)| !HERDR_TUI_STRIP_ENV_KEYS.contains(&key.as_str()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Resolve the user's canonical Herdr config path, honoring an explicit
/// `HERDR_CONFIG_PATH` and otherwise using the standard per-user location.
pub fn herdr_user_config_path() -> PathBuf {
    if let Some(path) = env::var_os("HERDR_CONFIG_PATH") {
        return PathBuf::from(path);
    }
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/herdr/config.toml")
}

/// Build the normal Herdr TUI command for a Host-owned shared session. The
/// child uses the same user config/socket discovery as the desktop TUI; only
/// inherited pane/nesting and terminal override variables are removed.
pub fn herdr_tui_command() -> Result<Command, String> {
    let herdr_cli = herdr_cli_path()
        .ok_or_else(|| "herdr CLI not found; install Herdr with `wax install herdr`".to_string())?;
    let environment: std::collections::HashMap<String, String> = env::vars().collect();
    let mut command = Command::new(herdr_cli);
    command.env_clear();
    for (key, value) in sanitized_tui_env(&environment) {
        command.env(key, value);
    }
    command.env(
        "HERDR_CONFIG_PATH",
        herdr_user_config_path().to_string_lossy().to_string(),
    );
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    Ok(command)
}

pub fn installed_cli_version() -> Option<String> {
    let output = Command::new(herdr_cli_path()?)
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!version.is_empty()).then_some(version)
}

fn ensure_herdr_installed() -> Result<(), HerdrError> {
    if herdr_cli_path().is_some() {
        return Ok(());
    }
    let Some(wax) = resolve_user_cli("wax") else {
        return Err(HerdrError::InstallFailed(
            "herdr CLI not found and wax unavailable; install Herdr with `wax install herdr`"
                .to_string(),
        ));
    };
    let output = Command::new(wax).args(["install", "herdr"]).output()?;
    if output.status.success() && herdr_cli_path().is_some() {
        Ok(())
    } else {
        Err(HerdrError::InstallFailed(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}

fn server_command(herdr: &Path, config_path: Option<&Path>) -> Command {
    let mut command = Command::new(herdr);
    command
        .arg("server")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(config_path) = config_path {
        command.env("HERDR_CONFIG_PATH", config_path);
    }
    command
}

/// Start (or no-op when already running) the server for one named Herdr
/// session. `None` targets the user's default instance. The session name is
/// injected into the child's environment only; the caller's environment is
/// never mutated.
fn start_server_for_session(
    session: Option<&str>,
    config_path: Option<&Path>,
) -> Result<(), HerdrError> {
    let herdr = herdr_cli_path().ok_or_else(|| {
        HerdrError::InstallFailed(
            "herdr CLI not found; install Herdr with `wax install herdr`".to_string(),
        )
    })?;
    let mut command = server_command(&herdr, config_path);
    if let Some(session) = session {
        command.env("HERDR_SESSION", session);
    }
    command.spawn()?;
    Ok(())
}

fn wait_for_socket(path: &Path) -> Result<(), HerdrError> {
    for _ in 0..30 {
        if path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(HerdrError::SocketUnavailable(
        path.display().to_string(),
        "timed out waiting for herdr server".to_string(),
    ))
}

fn socket_path_for_session_name(session: &str) -> PathBuf {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config/herdr/sessions")
        .join(session)
        .join("herdr.sock")
}

/// One row of `herdr session list --json`: a Herdr instance.
#[derive(Clone, Debug)]
pub struct HerdrSessionListing {
    pub name: String,
    pub running: bool,
    /// `true` for the user's default instance (`~/.config/herdr`).
    pub is_default: bool,
}

/// Enumerates every Herdr instance the CLI can see (default + named sessions,
/// running or stopped). `None` on any failure — callers degrade gracefully.
/// Workspaces ARE these instances (multi-instance model); there is no
/// Shardlane-side registry.
pub fn list_sessions() -> Option<Vec<HerdrSessionListing>> {
    let herdr = herdr_cli_path()?;
    let output = Command::new(herdr)
        .args(["session", "list", "--json"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let sessions = parsed.get("sessions")?.as_array()?;
    Some(
        sessions
            .iter()
            .filter_map(|session| {
                let name = session.get("name")?.as_str()?.to_string();
                let running = session
                    .get("running")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let is_default = session
                    .get("default")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                Some(HerdrSessionListing {
                    name,
                    running,
                    is_default,
                })
            })
            .collect(),
    )
}

/// Socket path for a session name. Herdr's own `default` session lives at the
/// base socket (`~/.config/herdr/herdr.sock`); every other session lives under
/// `~/.config/herdr/sessions/<name>/`. Free-function form for callers that
/// only need the location.
pub fn session_socket_path_for(session: &str) -> PathBuf {
    if session == "default" {
        socket_path()
    } else {
        socket_path_for_session_name(session)
    }
}

/// The directory herdr persists a session in (herdr's own `default` session
/// uses the base config dir, other sessions `sessions/<name>`).
pub fn session_dir_for(session: &str) -> PathBuf {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if session == "default" {
        home.join(".config/herdr")
    } else {
        home.join(".config/herdr/sessions").join(session)
    }
}

/// Per-session metadata file (`workspace.json` inside the session dir):
/// Shardlane-side extras — display name — stored on herdr's own session disk
/// layout, so the name travels with the session.
pub fn session_metadata_path_for(session: &str) -> PathBuf {
    session_dir_for(session).join("workspace.json")
}

/// The session's display name from its metadata file. `None` = no metadata
/// (callers fall back to the raw session name).
pub fn read_session_display_name(session: &str) -> Option<String> {
    let text = std::fs::read_to_string(session_metadata_path_for(session)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("display_name")?
        .as_str()
        .map(str::to_string)
        .filter(|name| !name.trim().is_empty())
}

/// Writes (or replaces) the session's display-name metadata file.
pub fn write_session_display_name(session: &str, display_name: &str) -> Result<(), String> {
    let name = display_name.trim();
    if name.is_empty() {
        return Err("display name is empty".to_string());
    }
    let path = session_metadata_path_for(session);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let body = serde_json::json!({ "version": 1, "display_name": name });
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn socket_path() -> PathBuf {
    if let Some(path) = env::var_os("HERDR_SOCKET_PATH") {
        return PathBuf::from(path);
    }
    if let Some(session) = env::var_os("HERDR_SESSION") {
        return socket_path_for_session_name(&session.to_string_lossy());
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config/herdr/herdr.sock")
}

/// Stops a named session's server (`herdr session stop <name>`). Idempotent
/// from the caller's perspective: stopping an already-stopped session is fine.
pub fn stop_session(session: &str) -> Result<(), String> {
    let herdr = herdr_cli_path()
        .ok_or_else(|| "herdr CLI not found; install Herdr with `wax install herdr`".to_string())?;
    let status = std::process::Command::new(&herdr)
        .args(["session", "stop", session])
        .output()
        .map_err(|error| error.to_string())?;
    if status.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&status.stderr).trim().to_string())
    }
}

/// Deletes a STOPPED session (`herdr session delete <name>`); herdr rejects
/// deleting a running one — call [`stop_session`] first.
pub fn delete_session(session: &str) -> Result<(), String> {
    let herdr = herdr_cli_path()
        .ok_or_else(|| "herdr CLI not found; install Herdr with `wax install herdr`".to_string())?;
    let status = std::process::Command::new(&herdr)
        .args(["session", "delete", session])
        .output()
        .map_err(|error| error.to_string())?;
    if status.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&status.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_should_default_to_config_dir() {
        let path = socket_path();
        assert!(path.ends_with(".config/herdr/herdr.sock") || path.ends_with("herdr.sock"));
    }

    #[test]
    fn server_command_applies_config_override_only_when_explicitly_requested() {
        let herdr = Path::new("/tmp/fake-herdr");
        let config = Path::new("/tmp/shardlane-herdr.toml");
        let with_override = server_command(herdr, Some(config));
        let explicit = with_override
            .get_envs()
            .find(|(key, _)| *key == "HERDR_CONFIG_PATH")
            .and_then(|(_, value)| value)
            .map(PathBuf::from);
        assert_eq!(explicit.as_deref(), Some(config));

        let inherited = server_command(herdr, None);
        assert!(inherited
            .get_envs()
            .all(|(key, _)| key != "HERDR_CONFIG_PATH"));
    }

    #[test]
    fn direct_or_test_connections_never_claim_server_config_ownership() {
        let client = HerdrClient::for_test_socket(PathBuf::from("/tmp/herdr-test.sock"));
        assert!(!client.server_started_with_supplied_config());
    }

    #[test]
    fn command_exists_should_find_shell() {
        assert!(resolve_user_cli("sh")
            .is_some_and(|path| { path.is_absolute() && path.ends_with("sh") }));
    }

    #[test]
    fn find_in_path_value_locates_file_and_skips_empty_segments() {
        let dir = std::env::temp_dir().join("shardlane-path-scan-test");
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create test dir: {e}"));
        let bin = dir.join("herdr");
        std::fs::write(&bin, b"#!/bin/sh\n").unwrap_or_else(|e| panic!("write fake herdr: {e}"));
        let path_value = format!(":/definitely/missing::{}:", dir.display());
        let found = find_in_path_value(&path_value, "herdr")
            .unwrap_or_else(|| panic!("herdr must be found"));
        assert_eq!(found, bin);
        assert!(find_in_path_value(&path_value, "wax").is_none());
    }

    #[test]
    fn fallback_cli_directories_cover_user_bins_and_homebrew() {
        let dirs = fallback_cli_directories();
        if let Some(home) = env::var_os("HOME") {
            let home = PathBuf::from(home);
            assert!(dirs.contains(&home.join(".local/bin")));
            assert!(dirs.contains(&home.join(".cargo/bin")));
        }
        assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")));
    }

    #[test]
    fn tui_spawn_env_strips_nested_pane_identity_but_keeps_socket_and_config() {
        let mut source = std::collections::HashMap::new();
        for key in HERDR_TUI_STRIP_ENV_KEYS {
            source.insert((*key).to_string(), "leak".to_string());
        }
        source.insert("HOME".into(), "/tmp/home".into());
        source.insert("HERDR_SOCKET_PATH".into(), "/tmp/herdr.sock".into());
        source.insert("HERDR_CONFIG_PATH".into(), "/tmp/herdr.toml".into());
        let env = sanitized_tui_env(&source);
        let keys: Vec<&str> = env.iter().map(|(key, _)| key.as_str()).collect();
        for stripped in HERDR_TUI_STRIP_ENV_KEYS {
            assert!(!keys.contains(stripped), "{stripped} must be stripped");
        }
        assert!(keys.contains(&"HOME"));
        assert!(keys.contains(&"HERDR_SOCKET_PATH"));
        assert!(keys.contains(&"HERDR_CONFIG_PATH"));
    }

    #[test]
    fn login_shell_path_is_nonempty_when_shell_available() {
        if let Some(path) = login_shell_path() {
            assert!(path.contains("/bin"));
        }
    }

    #[test]
    fn every_rpc_method_used_in_this_file_is_classified_exactly_once() {
        // C03 contract: scan this source for every `self.call` site (with
        // optional turbofish and a literal method name) and prove each method
        // falls into exactly one of {mutating, read}. A wrapper added without
        // updating the classification fails here instead of silently degrading
        // the R2-01/CR-04 lost-response semantics. Comment lines are skipped
        // so documenting an example call cannot fake a call site.
        let source = include_str!("herdr.rs");
        let mut methods: Vec<&str> = Vec::new();
        let mut search_from = 0;
        while let Some(offset) = source[search_from..].find(".call") {
            let absolute = search_from + offset;
            let after = absolute + ".call".len();
            search_from = after;
            let line_start = source[..absolute].rfind('\n').map(|i| i + 1).unwrap_or(0);
            if source[line_start..absolute].trim_start().starts_with("//") {
                continue;
            }
            let mut cursor = &source[after..];
            if let Some(generic) = cursor.strip_prefix("::<") {
                let Some(end) = generic.find('>') else {
                    continue;
                };
                cursor = &generic[end + 1..];
            }
            let Some(after_paren) = cursor.trim_start().strip_prefix('(') else {
                continue;
            };
            let Some(after_quote) = after_paren.trim_start().strip_prefix('"') else {
                continue;
            };
            let Some(end) = after_quote.find('"') else {
                continue;
            };
            methods.push(&after_quote[..end]);
        }
        assert!(
            methods.contains(&"ping"),
            "scanner must find the literal-method call sites"
        );
        assert!(
            methods.contains(&"layout.set_split_ratio")
                && methods.contains(&"workspace.move_block")
                && methods.contains(&"pane.resize")
                && methods.contains(&"pane.swap"),
            "the four C03 mutation wrappers must be scanned"
        );
        for method in methods {
            let mutating = is_mutating_rpc(method);
            let read = READ_METHODS.contains(&method);
            assert!(
                mutating ^ read,
                "rpc method {method:?} must be classified as exactly one of \
                 mutating/read (mutating={mutating}, read={read})"
            );
        }
    }

    #[test]
    fn trace_params_are_bounded_and_char_boundary_safe() {
        let small = json!({ "pane_id": "w1:p1" });
        assert_eq!(truncate_params_for_trace(&small), r#"{"pane_id":"w1:p1"}"#);
        let wide = json!({ "text": "🦀".repeat(TRACE_PARAMS_MAX_CHARS) });
        let truncated = truncate_params_for_trace(&wide);
        assert!(truncated.chars().count() <= TRACE_PARAMS_MAX_CHARS + 1);
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn ping_response_carries_runtime_version_and_protocol() {
        let ping: PingResponse = parse_json(
            r#"{"type":"pong","version":"0.8.0","protocol":19,"capabilities":{"live_handoff":true}}"#,
        );
        assert_eq!(ping.version.as_deref(), Some("0.8.0"));
        assert_eq!(ping.protocol, Some(19));
    }

    #[test]
    fn protocol_20_ping_is_parsed_and_supported() {
        let ping: PingResponse = parse_json(
            r#"{"type":"pong","version":"0.8.2","protocol":20,"capabilities":{"live_handoff":true}}"#,
        );
        assert_eq!(ping.version.as_deref(), Some("0.8.2"));
        assert_eq!(ping.protocol, Some(20));
        assert!(supports_protocol(20));
    }

    #[test]
    fn protocol_compatibility_accepts_forward_compatible_protocols() {
        assert!(!supports_protocol(18));
        assert!(supports_protocol(19));
        assert!(supports_protocol(20));
        assert!(supports_protocol(21));
        assert!(supports_protocol(99));
    }

    #[test]
    fn event_refresh_policy_stays_inside_herdr_projection_boundary() {
        let pane_update = HerdrEvent {
            event: "pane_updated".to_string(),
            data: Value::Null,
        };
        assert!(!pane_update.refreshes_navigation_projection());
        assert!(pane_update.refreshes_agents());

        for event in ["pane_focused", "layout_updated", "tab_focused"] {
            let event = HerdrEvent {
                event: event.to_string(),
                data: Value::Null,
            };
            assert!(!event.refreshes_navigation_projection());
            assert!(!event.refreshes_agents());
        }

        let workspace_focus = HerdrEvent {
            event: "workspace_focused".to_string(),
            data: json!({"type":"workspace_focused","workspace_id":"w2"}),
        };
        assert!(!workspace_focus.refreshes_navigation_projection());
        assert!(workspace_focus.refreshes_navigation_focus_projection());
        assert!(!workspace_focus.refreshes_agents());

        for event in ["tab_focused", "pane_focused", "tab.focused", "pane.focused"] {
            let focus = HerdrEvent {
                event: event.to_string(),
                data: json!({ "type": event }),
            };
            assert!(
                focus.refreshes_navigation_focus_projection(),
                "{event} must refresh runtime focus projection"
            );
        }

        let background_pane = HerdrEvent {
            event: "pane_created".to_string(),
            data: json!({
                "type":"pane_created",
                "pane":{"workspace_id":"w9","tab_id":"w9:t1","pane_id":"w9:p1"}
            }),
        };
        assert!(!background_pane.refreshes_navigation_projection());
        assert!(background_pane.refreshes_tab_surface_projection());
        assert_eq!(
            background_pane.affected_workspace_id().as_deref(),
            Some("w9")
        );

        for event in ["pane_closed", "pane_exited", "pane_moved"] {
            let event = HerdrEvent {
                event: event.to_string(),
                data: json!({"workspace_id":"w9","pane_id":"w9:p1"}),
            };
            assert!(
                event.refreshes_agents(),
                "{event:?} must reconcile Agent projection"
            );
        }

        let tab_created = HerdrEvent {
            event: "tab_created".to_string(),
            data: json!({"type":"tab_created","tab":{"workspace_id":"w9","tab_id":"w9:t2"}}),
        };
        assert!(tab_created.refreshes_navigation_projection());
        assert!(!tab_created.refreshes_tab_surface_projection());

        let pane_moved = HerdrEvent {
            event: "pane_moved".to_string(),
            data: json!({
                "type":"pane_moved",
                "pane":{"workspace_id":"w9","tab_id":"w9:t2","pane_id":"w9:p1"}
            }),
        };
        assert!(pane_moved.refreshes_navigation_projection());
        assert!(pane_moved.refreshes_tab_surface_projection());
        assert_eq!(pane_moved.affected_workspace_id().as_deref(), Some("w9"));
    }

    #[test]
    fn pane_scroll_events_preserve_herdr_authoritative_viewport_metrics() {
        let changed = HerdrEvent {
            event: "pane.scroll_changed".to_string(),
            data: json!({
                "pane_id": "w3Z:p9",
                "workspace_id": "w3Z",
                "scroll": {
                    "offset_from_bottom": 44,
                    "max_offset_from_bottom": 3614,
                    "viewport_rows": 57
                }
            }),
        };
        assert_eq!(
            changed.pane_scroll_patch(),
            Some(PaneScrollPatch {
                pane_id: "w3Z:p9".to_string(),
                workspace_id: "w3Z".to_string(),
                scroll: PaneScroll {
                    offset_from_bottom: 44,
                    max_offset_from_bottom: 3614,
                    viewport_rows: 57,
                },
            })
        );

        let updated = HerdrEvent {
            event: "pane_updated".to_string(),
            data: json!({
                "pane": {
                    "pane_id": "w3Z:p9",
                    "workspace_id": "w3Z",
                    "scroll": {
                        "offset_from_bottom": 0,
                        "max_offset_from_bottom": 3700,
                        "viewport_rows": 57
                    }
                }
            }),
        };
        assert_eq!(
            updated
                .pane_scroll_patch()
                .map(|patch| patch.scroll.max_offset_from_bottom),
            Some(3700)
        );
    }

    #[test]
    fn pane_command_results_match_protocol_19_20_wrapper_shapes() -> Result<(), String> {
        let move_response: ApiResponse<PaneMoveResponse> = parse_json(
            r#"{"result":{"type":"pane_move","move_result":{"changed":true,"previous_pane_id":"w1:p1","previous_workspace_id":"w1","previous_tab_id":"w1:t1","pane":{"pane_id":"w1:p1","workspace_id":"w1","tab_id":"w1:t2"},"target_layout":{"tab_id":"w1:t2","workspace_id":"w1","area":{"x":0,"y":0,"width":80,"height":24},"panes":[{"pane_id":"w1:p1","rect":{"x":0,"y":0,"width":80,"height":24},"focused":true}],"splits":[],"focused_pane_id":"w1:p1","zoomed":false},"focused_pane_id":"w1:p1"}},"error":null}"#,
        );
        let moved = move_response
            .result
            .ok_or_else(|| "missing pane move response".to_string())?
            .move_result;
        assert_eq!(moved.pane.pane_id, "w1:p1");
        assert_eq!(moved.target_layout.tab_id, "w1:t2");

        let resize_response: ApiResponse<PaneResizeResponse> = parse_json(
            r#"{"result":{"type":"pane_resize","resize":{"changed":true,"pane_id":"w1:p2","focused_pane_id":"w1:p2","layout":{"tab_id":"w1:t1","workspace_id":"w1","area":{"x":0,"y":0,"width":80,"height":24},"panes":[{"pane_id":"w1:p1","rect":{"x":0,"y":0,"width":35,"height":24},"focused":false},{"pane_id":"w1:p2","rect":{"x":35,"y":0,"width":45,"height":24},"focused":true}],"splits":[],"focused_pane_id":"w1:p2","zoomed":false}}},"error":null}"#,
        );
        let resize = resize_response
            .result
            .ok_or_else(|| "missing pane resize response".to_string())?
            .resize;
        assert_eq!(resize.layout.focused_pane_id.as_deref(), Some("w1:p2"));

        let swap_response: ApiResponse<PaneSwapResponse> = parse_json(
            r#"{"result":{"type":"pane_swap","swap":{"changed":true,"source_pane_id":"w1:p1","target_pane_id":"w1:p2","focused_pane_id":"w1:p1","layout":{"tab_id":"w1:t1","workspace_id":"w1","area":{"x":0,"y":0,"width":80,"height":24},"panes":[{"pane_id":"w1:p2","rect":{"x":0,"y":0,"width":40,"height":24},"focused":false},{"pane_id":"w1:p1","rect":{"x":40,"y":0,"width":40,"height":24},"focused":true}],"splits":[],"focused_pane_id":"w1:p1","zoomed":false}}},"error":null}"#,
        );
        let swap = swap_response
            .result
            .ok_or_else(|| "missing pane swap response".to_string())?
            .swap;
        assert_eq!(swap.layout.focused_pane_id.as_deref(), Some("w1:p1"));

        let split_response: ApiResponse<PaneCreatedResponse> = parse_json(
            r#"{"result":{"type":"pane_created","pane":{"pane_id":"w1:p3","terminal_id":"term_3","workspace_id":"w1","tab_id":"w1:t1","focused":true,"agent_status":"unknown","revision":1}},"error":null}"#,
        );
        let split = split_response
            .result
            .ok_or_else(|| "missing pane split response".to_string())?
            .pane;
        assert_eq!(split.pane_id, "w1:p3");
        assert_eq!(split.workspace_id.as_deref(), Some("w1"));
        assert_eq!(split.tab_id.as_deref(), Some("w1:t1"));

        let zoom_response: ApiResponse<PaneZoomResponse> = parse_json(
            r#"{"result":{"type":"pane_zoom","zoom":{"changed":true,"zoom_changed":true,"focus_changed":false,"pane_id":"w1:p2","focused_pane_id":"w1:p2","zoomed":true,"layout":{"tab_id":"w1:t1","workspace_id":"w1","area":{"x":0,"y":0,"width":80,"height":24},"panes":[{"pane_id":"w1:p1","rect":{"x":0,"y":0,"width":40,"height":24},"focused":false},{"pane_id":"w1:p2","rect":{"x":40,"y":0,"width":40,"height":24},"focused":true}],"splits":[],"focused_pane_id":"w1:p2","zoomed":true}}},"error":null}"#,
        );
        let zoom = zoom_response
            .result
            .ok_or_else(|| "missing pane zoom response".to_string())?
            .zoom;
        assert!(zoom.layout.zoomed);
        assert_eq!(zoom.layout.focused_pane_id.as_deref(), Some("w1:p2"));

        let process_response: ApiResponse<PaneProcessInfoResponse> = parse_json(
            r#"{"result":{"type":"pane_process_info","process_info":{"pane_id":"w1:p1","shell_pid":42,"tty":"/dev/ttys001","foreground_process_group_id":43,"foreground_processes":[{"pid":43,"name":"cargo","argv":["cargo","test"],"cwd":"/tmp/repo"}]}},"error":null}"#,
        );
        let process_info = process_response
            .result
            .ok_or_else(|| "missing pane process info response".to_string())?
            .process_info;
        assert_eq!(process_info.pane_id, "w1:p1");
        assert_eq!(process_info.shell_pid, Some(42));
        assert_eq!(process_info.foreground_processes.len(), 1);
        Ok(())
    }

    #[test]
    fn agent_status_event_exposes_authoritative_incremental_patch() {
        let event = HerdrEvent {
            event: "pane_agent_status_changed".to_string(),
            data: json!({
                "type":"pane_agent_status_changed",
                "pane_id":"w1:p1",
                "workspace_id":"w1",
                "agent_status":"working",
                "agent":"codex",
                "display_agent":"Codex",
                "title":null,
                "state_labels":{}
            }),
        };
        assert_eq!(
            event.agent_status_patch(),
            Some(AgentStatusPatch {
                pane_id: "w1:p1".to_string(),
                workspace_id: "w1".to_string(),
                agent_status: Some(Some("working".to_string())),
                agent: Some(Some("codex".to_string())),
                display_agent: Some(Some("Codex".to_string())),
                title: Some(None),
                ..AgentStatusPatch::default()
            })
        );

        let pane_updated = HerdrEvent {
            event: "pane_updated".to_string(),
            data: json!({
                "type":"pane_updated",
                "pane": {
                    "pane_id":"w1:p1",
                    "terminal_id":"term_1",
                    "workspace_id":"w1",
                    "tab_id":"w1:t1",
                    "focused":true,
                    "agent_status":"blocked",
                    "agent":"codex",
                    "display_agent":"Codex",
                    "title":"Needs input",
                    "revision":7
                }
            }),
        };
        assert_eq!(
            pane_updated.agent_status_patch(),
            Some(AgentStatusPatch {
                pane_id: "w1:p1".to_string(),
                workspace_id: "w1".to_string(),
                agent_status: Some(Some("blocked".to_string())),
                agent: Some(Some("codex".to_string())),
                display_agent: Some(Some("Codex".to_string())),
                title: Some(Some("Needs input".to_string())),
                tab_id: Some(Some("w1:t1".to_string())),
                focused: Some(true),
                ..AgentStatusPatch::default()
            })
        );

        let metadata_only_update = HerdrEvent {
            event: "pane_updated".to_string(),
            data: json!({
                "type":"pane_updated",
                "pane": {
                    "pane_id":"w1:p1",
                    "workspace_id":"w1",
                    "tab_id":"w1:t1",
                    "focused":false,
                    "cwd":"/work/project",
                    "foreground_cwd":"/work/project/packages/api",
                    "title":"API server"
                }
            }),
        };
        assert_eq!(
            metadata_only_update.agent_status_patch(),
            Some(AgentStatusPatch {
                pane_id: "w1:p1".to_string(),
                workspace_id: "w1".to_string(),
                title: Some(Some("API server".to_string())),
                tab_id: Some(Some("w1:t1".to_string())),
                cwd: Some(Some("/work/project".to_string())),
                foreground_cwd: Some(Some("/work/project/packages/api".to_string())),
                focused: Some(false),
                ..AgentStatusPatch::default()
            })
        );
    }

    #[test]
    fn layout_events_expose_incremental_surface_payloads() {
        let layout = HerdrEvent {
            event: "layout_updated".to_string(),
            data: json!({
                "type":"layout_updated",
                "layout": {
                    "workspace_id":"w1",
                    "tab_id":"w1:t2",
                    "zoomed":false,
                    "area":{"x":0,"y":0,"width":80,"height":24},
                    "focused_pane_id":"w1:p2",
                    "panes":[{"pane_id":"w1:p2","rect":{"x":0,"y":0,"width":80,"height":24},"focused":true}],
                    "splits":[]
                }
            }),
        };
        assert_eq!(
            layout
                .updated_layout()
                .and_then(|layout| layout.focused_pane_id),
            Some("w1:p2".to_string())
        );
    }

    #[test]
    fn workspace_order_events_refresh_navigation_projection() {
        for event in [
            "workspace_moved",
            "workspace_reordered",
            "workspace.moved",
            "workspace.reordered",
        ] {
            let event = HerdrEvent {
                event: event.to_string(),
                data: json!({}),
            };
            assert!(event.refreshes_navigation_projection(), "{event:?}");
        }
    }

    #[test]
    fn tab_move_result_contract_expects_bare_ok_discriminator() {
        // Protocol 20 schema: the tab.move success result = {"type":"ok"} (no
        // authoritative payload, unlike pane.move → move_result); the
        // authoritative correction is the tab.moved event carrying the whole
        // workspace TabInfo list.
        let ok: Value =
            serde_json::from_str(r#"{"type":"ok"}"#).unwrap_or_else(|error| panic!("{error}"));
        assert!(tab_move_result_is_ok(&ok));
        let moved_payload: Value = serde_json::from_str(r#"{"type":"tab_moved","tabs":[]}"#)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(!tab_move_result_is_ok(&moved_payload));
        let bare: Value = serde_json::from_str("{}").unwrap_or_else(|error| panic!("{error}"));
        assert!(!tab_move_result_is_ok(&bare));
    }

    #[test]
    fn pane_split_uses_protocol_19_target_pane_field() {
        let params = pane_split_params("w1:p2", "right");
        assert_eq!(
            params.get("target_pane_id").and_then(Value::as_str),
            Some("w1:p2")
        );
        assert_eq!(
            params.get("direction").and_then(Value::as_str),
            Some("right")
        );
        assert_eq!(params.get("focus").and_then(Value::as_bool), Some(true));
        assert!(params.get("pane_id").is_none());
    }

    #[test]
    fn subscription_ack_rejects_protocol_errors_before_startup_waits_on_receiver() {
        let error = match parse_subscription_ack(
            r#"{"id":"","error":{"code":"invalid_request","message":"invalid request: missing field `pane_id`"}}"#,
        ) {
            Ok(()) => panic!("invalid subscription must not become a live receiver"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("missing field `pane_id`"));

        if let Err(error) = parse_subscription_ack(
            r#"{"id":"shardlane-events","result":{"type":"subscription_started"}}"#,
        ) {
            panic!("valid subscription acknowledgement should pass: {error}");
        }
    }

    #[test]
    fn list_responses_should_parse_herdr_socket_shape() {
        let workspaces: WorkspaceList = parse_json(
            r#"{"type":"workspace_list","workspaces":[{"workspace_id":"w1","label":"repo"}]}"#,
        );
        let tabs: TabList =
            parse_json(r#"{"type":"tab_list","tabs":[{"tab_id":"w1:t1","pane_count":3}]}"#);
        let panes: PaneList = parse_json(
            r#"{"type":"pane_list","panes":[{"pane_id":"w1:p1","terminal_id":"term_1","agent_status":"working"}]}"#,
        );

        assert_eq!(workspaces.workspaces[0].workspace_id, "w1");
        assert_eq!(tabs.tabs[0].tab_id, "w1:t1");
        assert_eq!(tabs.tabs[0].pane_count, Some(3));
        assert_eq!(panes.panes[0].agent_status.as_deref(), Some("working"));
        assert_eq!(panes.panes[0].terminal_id.as_deref(), Some("term_1"));
    }

    #[test]
    fn workspace_state_issues_fewer_rpcs_than_host_bootstrap_state() {
        // C32: callers that only correlate workspaces/tabs/agents must not
        // pay the per-workspace pane.list fan-out. The all-workspaces
        // pane.list form (no workspace_id in the params) returns the same
        // projection in 4 RPCs instead of 3 + N.
        let workspace_list = r#"{"id":"","result":{"type":"workspace_list","workspaces":[{"workspace_id":"w1"},{"workspace_id":"w2"}]}}"#;
        let tab_list = r#"{"id":"","result":{"type":"tab_list","tabs":[]}}"#;
        let pane_list = r#"{"id":"","result":{"type":"pane_list","panes":[]}}"#;
        let agent_list = r#"{"id":"","result":{"type":"agent_list","agents":[]}}"#;

        let (bootstrap_socket, bootstrap_requests) = scripted_request_recorder(vec![
            workspace_list.into(),
            tab_list.into(),
            pane_list.into(),
            pane_list.into(),
            agent_list.into(),
        ]);
        let bootstrap_client = HerdrClient::for_test_socket(bootstrap_socket);
        bootstrap_client
            .host_bootstrap_state()
            .unwrap_or_else(|error| panic!("bootstrap state: {error}"));
        let bootstrap_rpcs = bootstrap_requests
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .len();

        let (state_socket, state_requests) = scripted_request_recorder(vec![
            workspace_list.into(),
            tab_list.into(),
            pane_list.into(),
            agent_list.into(),
        ]);
        let state_client = HerdrClient::for_test_socket(state_socket);
        let state = state_client
            .workspace_state()
            .unwrap_or_else(|error| panic!("workspace state: {error}"));
        let state_rpcs = state_requests
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .len();

        assert_eq!(state.workspaces.len(), 2, "same workspace projection");
        assert!(
            state_rpcs < bootstrap_rpcs,
            "narrow path must be strictly cheaper: {state_rpcs} vs {bootstrap_rpcs}"
        );
        assert_eq!(bootstrap_rpcs, 5, "3 + one pane.list per workspace");
        assert_eq!(
            state_rpcs, 4,
            "one all-workspaces pane.list replaces the fan-out"
        );
        let pane_list_request = state_requests
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .iter()
            .find(|(method, _)| method == "pane.list")
            .unwrap_or_else(|| panic!("pane.list recorded"))
            .clone();
        assert!(
            pane_list_request.1.get("workspace_id").is_none(),
            "the narrowed accessor must use the all-workspaces pane.list form: {:?}",
            pane_list_request.1
        );
    }

    #[test]
    fn agent_report_source_is_client_scoped_and_state_unknown() {
        // Contract: the manual claim uses shardlane as source and
        // state=unknown, never impersonating herdr's own detection.
        let params = json!({
            "pane_id": "w1:p1",
            "source": CLIENT_AGENT_REPORT_SOURCE,
            "agent": "claude",
            "state": "unknown",
        });
        assert_eq!(params["source"], "shardlane");
        assert_eq!(params["state"], "unknown");
        assert_eq!(params["agent"], "claude");
    }

    #[test]
    fn agent_list_should_parse_cross_workspace_agents() {
        let agents: AgentList = parse_json(
            r#"{"type":"agent_list","agents":[{"terminal_id":"term_1","agent":"pi","agent_status":"idle","workspace_id":"w1","tab_id":"w1:t1","pane_id":"w1:p1"},{"terminal_id":"term_2","agent":"devin","agent_status":"working","workspace_id":"w2","tab_id":"w2:t1","pane_id":"w2:p1"}]}"#,
        );

        assert_eq!(agents.agents.len(), 2);
        assert_eq!(agents.agents[0].workspace_id.as_deref(), Some("w1"));
        assert_eq!(agents.agents[1].agent.as_deref(), Some("devin"));
    }

    #[test]
    fn rpc_read_timeout_defaults_for_ordinary_calls() {
        assert_eq!(
            rpc_read_timeout("workspace.list", &json!({})),
            Duration::from_secs(10)
        );
        assert_eq!(
            rpc_read_timeout("pane.read", &json!({ "lines": 256 })),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn agent_prompt_with_wait_extends_the_read_timeout() {
        assert_eq!(
            rpc_read_timeout(
                "agent.prompt",
                &json!({ "target": "p", "text": "t", "wait": { "timeout_ms": 15_000 } })
            ),
            Duration::from_millis(20_000)
        );
        assert_eq!(
            rpc_read_timeout("agent.prompt", &json!({ "target": "p", "text": "t" })),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn agent_start_read_timeout_covers_the_server_startup_wait() {
        // AC-01: a start that waits up to 45s server-side must not die at the
        // ordinary 10s socket read boundary.
        assert_eq!(
            rpc_read_timeout("agent.start", &json!({ "timeout_ms": 45_000 })),
            Duration::from_millis(50_000)
        );
        assert_eq!(
            rpc_read_timeout("agent.start", &json!({ "timeout_ms": 300_000 })),
            Duration::from_millis(305_000)
        );
        // Without an explicit timeout the ordinary bound still applies.
        assert_eq!(
            rpc_read_timeout("agent.start", &json!({})),
            Duration::from_secs(10)
        );
    }

    /// What the fake server does after reading one request.
    enum ServerAction {
        /// Send this raw line as the response.
        Line(String),
        /// Close the connection without responding.
        Drop,
    }

    /// Scripted one-shot fake Herdr socket: answers each accepted connection
    /// with the next scripted action and records the requested methods.
    /// `keep`ing the tempdir means cleanup is the OS's business.
    fn scripted_herdr_server_actions(
        actions: Vec<ServerAction>,
    ) -> (
        std::path::PathBuf,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        let socket_path = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("tempdir: {error}"))
            .keep()
            .join("herdr-test.sock");
        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = received.clone();
        let bind_path = socket_path.clone();
        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            let listener = match std::os::unix::net::UnixListener::bind(&bind_path) {
                Ok(listener) => listener,
                Err(_) => return,
            };
            for action in actions {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut line = String::new();
                let mut reader = BufReader::new(&mut stream);
                if reader.read_line(&mut line).is_err() {
                    break;
                }
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if let Some(method) = value.get("method").and_then(Value::as_str) {
                        recorded
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner())
                            .push(method.to_string());
                    }
                }
                match action {
                    ServerAction::Line(response) => {
                        let _ = writeln!(stream, "{response}");
                        let _ = stream.flush();
                    }
                    ServerAction::Drop => {
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                    }
                }
            }
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !socket_path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        (socket_path, received)
    }

    fn scripted_herdr_server(
        responses: Vec<String>,
    ) -> (
        std::path::PathBuf,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    ) {
        scripted_herdr_server_actions(responses.into_iter().map(ServerAction::Line).collect())
    }

    /// Recorded (method, params) pairs captured by [`scripted_request_recorder`].
    type RecordedRequests = std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>;

    /// Scripted one-shot fake Herdr socket that records (method, params) per
    /// request — the C32 RPC-count assertions need the request params, not
    /// just the method names.
    fn scripted_request_recorder(responses: Vec<String>) -> (std::path::PathBuf, RecordedRequests) {
        let socket_path = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("tempdir: {error}"))
            .keep()
            .join("herdr-test.sock");
        let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = received.clone();
        let bind_path = socket_path.clone();
        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            let listener = match std::os::unix::net::UnixListener::bind(&bind_path) {
                Ok(listener) => listener,
                Err(_) => return,
            };
            for response in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut line = String::new();
                let mut reader = BufReader::new(&mut stream);
                if reader.read_line(&mut line).is_err() {
                    break;
                }
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    let method = value
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    recorded
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner())
                        .push((method, value.get("params").cloned().unwrap_or(Value::Null)));
                }
                let _ = writeln!(stream, "{response}");
                let _ = stream.flush();
            }
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !socket_path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        (socket_path, received)
    }

    fn prompt_params() -> AgentPromptParams {
        AgentPromptParams {
            target: "pane-1".to_string(),
            text: "do the thing".to_string(),
            wait: None,
        }
    }

    #[test]
    fn prompt_agent_blocked_never_sends_raw_keys() {
        let (socket, received) = scripted_herdr_server(vec![
            r#"{"id":"","error":{"code":"agent_blocked","message":"agent is blocked"}}"#.into(),
        ]);
        let client = HerdrClient::for_test_socket(socket);
        let error = match client.prompt_agent_confirmed(&prompt_params()) {
            Err(error) => error,
            Ok(_) => panic!("blocked prompt must error"),
        };
        assert!(matches!(error, HerdrError::AgentBlocked(_)), "{error:?}");
        let requests = received.lock().unwrap_or_else(|p| p.into_inner()).clone();
        assert_eq!(
            requests,
            vec!["agent.prompt".to_string()],
            "no raw Enter/keys follow a semantic rejection"
        );
    }

    #[test]
    fn prompt_agent_not_found_returns_typed_error_without_keys() {
        let (socket, received) = scripted_herdr_server(vec![
            r#"{"id":"","error":{"code":"agent_not_found","message":"no agent"}}"#.into(),
        ]);
        let client = HerdrClient::for_test_socket(socket);
        let error = match client.prompt_agent_confirmed(&prompt_params()) {
            Err(error) => error,
            Ok(_) => panic!("missing agent must error"),
        };
        assert!(matches!(error, HerdrError::AgentNotFound(_)), "{error:?}");
        assert_eq!(
            received.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            vec!["agent.prompt".to_string()]
        );
    }

    #[test]
    fn prompt_server_timeout_is_delivery_uncertain_and_never_retried() {
        let (socket, received) = scripted_herdr_server(vec![
            r#"{"id":"","error":{"code":"timeout","message":"wait timed out"}}"#.into(),
        ]);
        let client = HerdrClient::for_test_socket(socket);
        let error = match client.prompt_agent_confirmed(&prompt_params()) {
            Err(error) => error,
            Ok(_) => panic!("timeout must error"),
        };
        assert!(
            matches!(error, HerdrError::DeliveryUncertain(_)),
            "{error:?}"
        );
        assert_eq!(
            received.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            vec!["agent.prompt".to_string()],
            "uncertain delivery must not auto-retry or send Enter"
        );
    }

    #[test]
    fn prompt_parked_stalled_recovers_with_exactly_one_enter_and_confirms() {
        // The one sanctioned recovery: agent_prompt_stalled → one Enter on the
        // same target → lifecycle confirmation through agent.wait.
        let (socket, received) = scripted_herdr_server(vec![
            r#"{"id":"","error":{"code":"agent_prompt_stalled","message":"no state change"}}"#
                .into(),
            r#"{"id":"","result":{}}"#.into(),
            r#"{"id":"","result":{"type":"agent","agent":{"terminal_id":"term-1"}}}"#.into(),
        ]);
        let client = HerdrClient::for_test_socket(socket);
        client
            .prompt_agent_confirmed(&prompt_params())
            .unwrap_or_else(|error| panic!("parked recovery must succeed: {error}"));
        assert_eq!(
            received.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            vec![
                "agent.prompt".to_string(),
                "agent.send_keys".to_string(),
                "agent.wait".to_string(),
            ]
        );
    }

    #[test]
    fn mutating_rpc_eof_after_write_is_delivery_uncertain() {
        // R2-01/CR-04: the request was written; the server closing before a
        // response means the mutation may be accepted — never definitive.
        let (socket, received) = scripted_herdr_server_actions(vec![ServerAction::Drop]);
        let client = HerdrClient::for_test_socket(socket);
        let error = match client.prompt_agent(&prompt_params()) {
            Err(error) => error,
            Ok(_) => panic!("EOF must error"),
        };
        assert!(
            matches!(error, HerdrError::DeliveryUncertain(_)),
            "{error:?}"
        );
        assert_eq!(
            received.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            vec!["agent.prompt".to_string()]
        );
    }

    #[test]
    fn mutating_rpc_malformed_response_after_write_is_delivery_uncertain() {
        let (socket, _received) =
            scripted_herdr_server_actions(vec![ServerAction::Line("this-is-not-json\n".into())]);
        let client = HerdrClient::for_test_socket(socket);
        let error = match client.start_agent(&crate::herdr::AgentStartParams {
            name: "a".into(),
            kind: "claude".into(),
            pane_id: "p1".into(),
            args: Vec::new(),
            timeout_ms: Some(1_000),
        }) {
            Err(error) => error,
            Ok(_) => panic!("garbage response must error"),
        };
        assert!(
            matches!(error, HerdrError::DeliveryUncertain(_)),
            "{error:?}"
        );
    }

    #[test]
    fn non_mutating_rpc_malformed_response_stays_a_plain_parse_error() {
        let (socket, _received) =
            scripted_herdr_server_actions(vec![ServerAction::Line("not-json\n".into())]);
        let client = HerdrClient::for_test_socket(socket);
        let error = match client.agents() {
            Err(error) => error,
            Ok(_) => panic!("garbage response must error"),
        };
        assert!(
            matches!(error, HerdrError::Json(_)),
            "read RPCs must not claim delivery uncertainty: {error:?}"
        );
    }

    #[test]
    fn prompt_parked_stalled_recovery_failure_is_delivery_uncertain_after_enter() {
        let (socket, received) = scripted_herdr_server(vec![
            r#"{"id":"","error":{"code":"agent_prompt_stalled","message":"no state change"}}"#
                .into(),
            r#"{"id":"","result":{}}"#.into(),
            r#"{"id":"","error":{"code":"timeout","message":"confirm timed out"}}"#.into(),
        ]);
        let client = HerdrClient::for_test_socket(socket);
        let error = match client.prompt_agent_confirmed(&prompt_params()) {
            Err(error) => error,
            Ok(_) => panic!("failed recovery must error"),
        };
        assert!(
            matches!(error, HerdrError::DeliveryUncertain(_)),
            "{error:?}"
        );
        assert_eq!(
            received.lock().unwrap_or_else(|p| p.into_inner()).clone(),
            vec![
                "agent.prompt".to_string(),
                "agent.send_keys".to_string(),
                "agent.wait".to_string(),
            ]
        );
    }

    #[test]
    fn agent_wait_timeout_derives_from_params_margin() {
        assert_eq!(
            rpc_read_timeout("agent.wait", &json!({ "timeout_ms": 30_000 })),
            Duration::from_millis(35_000)
        );
    }

    #[test]
    fn agent_wait_without_timeout_uses_default_margin() {
        assert_eq!(
            rpc_read_timeout("agent.wait", &json!({})),
            Duration::from_millis(65_000)
        );
    }

    #[test]
    fn request_param_shapes_match_verified_protocol_facades() {
        // The seven verified protocol 19-20 interaction request shapes. Any
        // field addition/removal/rename/default change must first be checked
        // against the live Herdr schema, then mirrored into the expectations
        // here.
        assert_eq!(
            pane_read_recent_params("w1:p1", 256),
            json!({
                "pane_id": "w1:p1",
                "source": "recent",
                "lines": 256,
                "format": "ansi",
                "strip_ansi": false
            })
        );
        assert_eq!(
            layout_set_split_ratio_params("w1:t1", &[true, false], 0.5),
            json!({ "tab_id": "w1:t1", "path": [true, false], "ratio": 0.5 })
        );
        // clamp boundaries: ratio always lands in [0.05, 0.95].
        assert_eq!(
            layout_set_split_ratio_params("w1:t1", &[], 1.2),
            json!({ "tab_id": "w1:t1", "path": [], "ratio": 0.95 })
        );
        assert_eq!(
            layout_set_split_ratio_params("w1:t1", &[], 0.0),
            json!({ "tab_id": "w1:t1", "path": [], "ratio": 0.05 })
        );
        // None → null serialization is the key pin for tab.create.
        assert_eq!(
            tab_create_params(None, None),
            json!({ "workspace_id": null, "cwd": null, "focus": true })
        );
        assert_eq!(
            tab_create_params(Some("w1"), Some("/work/demo")),
            json!({ "workspace_id": "w1", "cwd": "/work/demo", "focus": true })
        );
        assert_eq!(
            pane_move_new_tab_params("w1:p1", "w1"),
            json!({
                "pane_id": "w1:p1",
                "destination": { "type": "new_tab", "workspace_id": "w1" },
                "focus": true
            })
        );
        assert_eq!(
            pane_move_tab_params("w1:p1", "w1:t2"),
            json!({
                "pane_id": "w1:p1",
                "destination": {
                    "type": "tab",
                    "tab_id": "w1:t2",
                    "split": "right",
                    "target_pane_id": null
                },
                "focus": true
            })
        );
        let keys = vec!["CTRL_C".to_string(), "ENTER".to_string()];
        assert_eq!(
            pane_send_keys_params("w1:p1", &keys),
            json!({ "pane_id": "w1:p1", "keys": ["CTRL_C", "ENTER"] })
        );
        assert_eq!(
            pane_send_text_params("w1:p1", "cargo test\n"),
            json!({ "pane_id": "w1:p1", "text": "cargo test\n" })
        );
    }

    #[test]
    fn agent_operation_params_match_protocol_20_schema_shapes() {
        let start = serde_json::to_value(AgentStartParams {
            name: "mobile-agent".to_string(),
            kind: "pi".to_string(),
            pane_id: "w1:p1".to_string(),
            args: vec!["--model".to_string(), "test".to_string()],
            timeout_ms: Some(30_000),
        })
        .unwrap_or_else(|error| panic!("failed to serialize agent.start params: {error}"));
        assert_eq!(start["name"], "mobile-agent");
        assert_eq!(start["kind"], "pi");
        assert_eq!(start["pane_id"], "w1:p1");
        assert_eq!(start["args"], json!(["--model", "test"]));
        assert_eq!(start["timeout_ms"], 30_000);

        let prompt = serde_json::to_value(AgentPromptParams {
            target: "term_1".to_string(),
            text: "continue".to_string(),
            wait: Some(AgentPromptWaitOptions {
                until: vec![HerdrAgentStatus::Idle, HerdrAgentStatus::Done],
                timeout_ms: Some(5_000),
            }),
        })
        .unwrap_or_else(|error| panic!("failed to serialize agent.prompt params: {error}"));
        assert_eq!(prompt["target"], "term_1");
        assert_eq!(prompt["text"], "continue");
        assert_eq!(prompt["wait"]["until"], json!(["idle", "done"]));
        assert_eq!(prompt["wait"]["timeout_ms"], 5_000);

        let read = serde_json::to_value(AgentReadParams {
            target: "term_1".to_string(),
            source: HerdrReadSource::RecentUnwrapped,
            lines: Some(80),
            format: HerdrReadFormat::Text,
            strip_ansi: true,
        })
        .unwrap_or_else(|error| panic!("failed to serialize agent.read params: {error}"));
        assert_eq!(read["source"], "recent_unwrapped");
        assert_eq!(read["format"], "text");
        assert_eq!(read["lines"], 80);
        assert_eq!(read["strip_ansi"], true);

        let wait = serde_json::to_value(AgentWaitParams {
            target: "term_1".to_string(),
            until: vec![HerdrAgentStatus::Blocked],
            timeout_ms: None,
        })
        .unwrap_or_else(|error| panic!("failed to serialize agent.wait params: {error}"));
        assert_eq!(wait["until"], json!(["blocked"]));
        assert!(wait.get("timeout_ms").is_none());

        let send_keys = serde_json::to_value(AgentSendKeysParams {
            target: "term_1".to_string(),
            keys: vec!["CTRL_C".to_string()],
        })
        .unwrap_or_else(|error| panic!("failed to serialize agent.send_keys params: {error}"));
        assert_eq!(send_keys["target"], "term_1");
        assert_eq!(send_keys["keys"], json!(["CTRL_C"]));
    }

    #[test]
    fn agent_operation_responses_decode_current_herdr_shapes() {
        let started: AgentStartedResult = parse_json(
            r#"{"type":"agent_started","agent":{"terminal_id":"term_1","agent":"pi","agent_status":"idle","workspace_id":"w1","tab_id":"w1:t1","pane_id":"w1:p1","focused":false,"revision":1,"interactive_ready":true,"launch_pending":false,"state_change_seq":3,"screen_detection_skipped":false,"state_labels":{"phase":"idle"},"tokens":{}},"argv":["pi"]}"#,
        );
        assert_eq!(started.agent.terminal_id, "term_1");
        assert_eq!(started.agent.agent.as_deref(), Some("pi"));
        assert!(started.agent.interactive_ready);
        assert_eq!(started.agent.revision, 1);
        assert_eq!(started.agent.state_change_seq, 3);
        assert_eq!(
            started.agent.state_labels.get("phase").map(String::as_str),
            Some("idle")
        );
        assert_eq!(started._argv, vec!["pi"]);

        let prompted: AgentPromptedResult = parse_json(
            r#"{"type":"agent_prompted","agent":{"terminal_id":"term_1","agent_status":"working","workspace_id":"w1","tab_id":"w1:t1","pane_id":"w1:p1","focused":false,"revision":2}}"#,
        );
        assert_eq!(prompted.agent.agent_status.as_deref(), Some("working"));

        let read: PaneReadResponse = parse_json(
            r#"{"type":"pane_read","read":{"pane_id":"w1:p1","workspace_id":"w1","tab_id":"w1:t1","source":"recent","format":"text","text":"ready\n","revision":7,"truncated":false}}"#,
        );
        assert_eq!(read.read.source, HerdrReadSource::Recent);
        assert_eq!(read.read.format, HerdrReadFormat::Text);
        assert_eq!(read.read.text, "ready\n");
        assert!(!read.read.truncated);

        let waited: AgentWaitResponse = parse_json(
            r#"{"type":"agent_info","agent":{"terminal_id":"term_1","agent_status":"idle","workspace_id":"w1","tab_id":"w1:t1","pane_id":"w1:p1","focused":false,"revision":8}}"#,
        );
        assert_eq!(waited._kind, "agent_info");
        assert_eq!(
            waited.agent.and_then(|agent| agent.agent_status),
            Some("idle".into())
        );
    }

    #[test]
    #[ignore = "real isolated Herdr runtime smoke; uses only a local fake pi executable"]
    fn isolated_agent_runtime_contract_smoke_uses_fake_local_agent() {
        use std::os::unix::fs::PermissionsExt as _;

        struct ServerGuard(std::process::Child);
        impl Drop for ServerGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir failed: {error}"));
        let home = temp.path().join("home");
        let bin = temp.path().join("bin");
        let project = temp.path().join("project");
        for dir in [&home, &bin, &project] {
            if let Err(error) = std::fs::create_dir_all(dir) {
                panic!("failed to create {}: {error}", dir.display());
            }
        }

        let fake_pi = bin.join("pi");
        if let Err(error) = std::fs::copy("/bin/cat", &fake_pi) {
            panic!("failed to create fake pi executable: {error}");
        }
        let mut permissions = match std::fs::metadata(&fake_pi) {
            Ok(metadata) => metadata.permissions(),
            Err(error) => panic!("failed to stat fake pi: {error}"),
        };
        permissions.set_mode(0o755);
        if let Err(error) = std::fs::set_permissions(&fake_pi, permissions) {
            panic!("failed to chmod fake pi: {error}");
        }

        let socket_path = temp.path().join("herdr-agent-smoke.sock");
        let inherited_path = env::var("PATH").unwrap_or_default();
        let smoke_path = format!("{}:{inherited_path}", bin.display());
        let server = match Command::new("herdr")
            .arg("server")
            .env("HOME", &home)
            .env("HERDR_SOCKET_PATH", &socket_path)
            .env("PATH", smoke_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(server) => server,
            Err(error) => panic!("failed to start isolated Herdr server: {error}"),
        };
        let _server = ServerGuard(server);
        if let Err(error) = wait_for_socket(&socket_path) {
            panic!("isolated Herdr socket did not become ready: {error}");
        }

        let client = HerdrClient {
            socket_path,
            runtime_version: None,
            protocol: Some(20),
            server_started_with_supplied_config: false,
        };
        if let Err(error) = client.ping() {
            panic!("isolated Herdr ping failed: {error}");
        }
        let project_path = project.to_string_lossy().into_owned();
        let created = match client.create_workspace_at(Some(&project_path)) {
            Ok(created) => created,
            Err(error) => panic!("failed to create isolated workspace: {error}"),
        };
        let pane_id = created.root_pane.pane_id.clone();

        let started = match client.start_agent(&AgentStartParams {
            name: "fake-mobile-agent".to_string(),
            kind: "pi".to_string(),
            pane_id,
            args: Vec::new(),
            timeout_ms: Some(10_000),
        }) {
            Ok(started) => started,
            Err(error) => panic!("agent.start failed against fake local agent: {error}"),
        };
        assert!(!started.agent.terminal_id.is_empty());
        assert_eq!(started.agent.name.as_deref(), Some("fake-mobile-agent"));
        assert_eq!(started.agent.agent.as_deref(), Some("pi"));
        assert!(!started.agent.launch_pending);
        let target = created.root_pane.pane_id.clone();

        let wait_client = client.clone();
        let wait_target = target.clone();
        let wait_thread = thread::spawn(move || {
            wait_client.wait_for_agent(&AgentWaitParams {
                target: wait_target,
                until: vec![
                    HerdrAgentStatus::Idle,
                    HerdrAgentStatus::Working,
                    HerdrAgentStatus::Blocked,
                    HerdrAgentStatus::Done,
                    HerdrAgentStatus::Unknown,
                ],
                timeout_ms: Some(5_000),
            })
        });
        thread::sleep(Duration::from_millis(100));

        if let Err(error) = client.prompt_agent(&AgentPromptParams {
            target: target.clone(),
            text: "hello-from-shardlane".to_string(),
            wait: None,
        }) {
            panic!("agent.prompt failed against fake local agent: {error}");
        }

        match wait_thread.join() {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => panic!("agent.wait failed against fake local agent: {error}"),
            Err(_) => panic!("agent.wait smoke thread panicked"),
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let read = match client.read_agent(&AgentReadParams {
                target: target.clone(),
                source: HerdrReadSource::RecentUnwrapped,
                lines: Some(80),
                format: HerdrReadFormat::Text,
                strip_ansi: true,
            }) {
                Ok(read) => read,
                Err(error) => panic!("agent.read failed against fake local agent: {error}"),
            };
            if read.text.contains("hello-from-shardlane") {
                break;
            }
            if Instant::now() >= deadline {
                panic!(
                    "fake Agent prompt did not appear in agent.read output: {}",
                    read.text
                );
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn parse_json<T: serde::de::DeserializeOwned>(json: &str) -> T {
        match serde_json::from_str(json) {
            Ok(value) => value,
            Err(err) => panic!("{err}"),
        }
    }
}
