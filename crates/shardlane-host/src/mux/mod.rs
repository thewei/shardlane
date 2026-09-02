//! Backend-neutral Multiplexer API — the Host runtime seam (see
//! `docs/multiplexer-api.md`).
//!
//! The macOS shell and the Remote API consume runtime state only through these
//! traits; concrete backends live in adapter files (`herdr.rs`, future
//! `tmux.rs`) and are assembled by [`registry::MuxRegistry`]. Backend-specific
//! knowledge (socket shapes, CLI invocations, protocol gates, child spawn
//! commands, wire formats) stays inside adapter files; code above this module
//! never branches on backend identity — capability differences flow through
//! [`MuxCapabilities`] and the `Option` accessors.
//!
//! The projection types re-exported here (`Workspace`, `Tab`, `Pane`, …) are
//! the product-domain model; their shape is shared with the Herdr adapter
//! implementation so the migration stays behavior-identical.

pub mod herdr;
pub mod kit;
pub mod registry;
pub mod tmux;

pub use registry::MuxRegistry;

use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;

use crate::herdr::{
    Agent, HerdrError, HerdrEvent, HerdrState, NavigationState, Pane, PaneLayout,
    PaneLayoutActionResult, PaneMoveResult, PaneProcessInfo, TabCreatedResult, TabSurfaceState,
    WorkspaceCreatedResult,
};

// --- Neutral projection types (shared shape with the Herdr adapter) ---

pub use crate::herdr::{AgentSessionInfo, PaneScroll, PaneScrollPatch, Workspace};
pub use crate::ids::{AgentRef, PaneId};
/// Full projection snapshot returned by the state queries.
pub type MuxState = HerdrState;

/// Neutral projection event. The event-name vocabulary matched by the
/// classification helpers (`refreshes_*`, `affected_workspace_id`, …) is the
/// canonical mux vocabulary: adapters emit these names, whatever their native
/// wire spelling is.
pub type MuxEvent = HerdrEvent;

/// Neutral agent runtime operation types (shared with `crate::runtime`).
pub use crate::runtime::{
    RuntimeAgent, RuntimeAgentRead, RuntimeAgentReadFormat, RuntimeAgentStartRequest,
    RuntimeAgentWait, RuntimeAgentWaitRequest,
};

// --- Errors ---

#[derive(Debug, Error)]
pub enum MuxError {
    #[error("backend is not installed and automatic installation failed: {0}")]
    InstallFailed(String),
    #[error("backend socket unavailable at {0}: {1}")]
    SocketUnavailable(String, String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("backend api: {0}")]
    Api(String),
    /// Structured not-found (e.g. agent_not_found): the basis for mapping 404
    /// in the remote API.
    #[error("backend api: {0}")]
    NotFound(String),
    /// Structured blocked rejection that happened before any input was sent.
    #[error("backend api: {0}")]
    Blocked(String),
    /// The text was accepted but no state change was observed.
    #[error("backend api: {0}")]
    PromptStalled(String),
    /// Server-side wait timed out; the request may have taken effect.
    #[error("backend api timeout: {0}")]
    Timeout(String),
    /// The request was written but no response was read: the outcome is
    /// uncertain and callers must never retry automatically.
    #[error("delivery uncertain: {0}")]
    Uncertain(String),
    #[error("incompatible protocol: requires {min}+; got {actual}")]
    IncompatibleProtocol { min: u32, actual: u32 },
    /// The connected backend does not provide this capability. Produced by the
    /// trait's default gated methods; adapters that override them must keep
    /// this coherence (kit contract).
    #[error("capability not supported by this backend: {0}")]
    Unsupported(&'static str),
}

impl From<HerdrError> for MuxError {
    fn from(error: HerdrError) -> Self {
        match error {
            HerdrError::InstallFailed(detail) => Self::InstallFailed(detail),
            HerdrError::SocketUnavailable(socket, detail) => {
                Self::SocketUnavailable(socket, detail)
            }
            HerdrError::Io(inner) => Self::Io(inner),
            HerdrError::Json(inner) => Self::Json(inner),
            HerdrError::Api(detail) => Self::Api(detail),
            HerdrError::AgentNotFound(detail) => Self::NotFound(detail),
            HerdrError::AgentBlocked(detail) => Self::Blocked(detail),
            HerdrError::AgentPromptStalled(detail) => Self::PromptStalled(detail),
            HerdrError::ApiTimeout(detail) => Self::Timeout(detail),
            HerdrError::DeliveryUncertain(detail) => Self::Uncertain(detail),
            HerdrError::IncompatibleProtocol { min, actual } => {
                Self::IncompatibleProtocol { min, actual }
            }
        }
    }
}

/// Reverse mapping for the Herdr-concrete product surfaces (Domain 7/9
/// services, protocol gate) that still speak `HerdrError` at their boundary.
/// `Unsupported` has no Herdr counterpart and degrades to a plain API error.
impl From<MuxError> for HerdrError {
    fn from(error: MuxError) -> Self {
        match error {
            MuxError::InstallFailed(detail) => Self::InstallFailed(detail),
            MuxError::SocketUnavailable(socket, detail) => Self::SocketUnavailable(socket, detail),
            MuxError::Io(inner) => Self::Io(inner),
            MuxError::Json(inner) => Self::Json(inner),
            MuxError::Api(detail) => Self::Api(detail),
            MuxError::NotFound(detail) => Self::AgentNotFound(detail),
            MuxError::Blocked(detail) => Self::AgentBlocked(detail),
            MuxError::PromptStalled(detail) => Self::AgentPromptStalled(detail),
            MuxError::Timeout(detail) => Self::ApiTimeout(detail),
            MuxError::Uncertain(detail) => Self::DeliveryUncertain(detail),
            MuxError::IncompatibleProtocol { min, actual } => {
                Self::IncompatibleProtocol { min, actual }
            }
            MuxError::Unsupported(capability) => Self::Api(format!(
                "capability not supported by this backend: {capability}"
            )),
        }
    }
}

// --- Instance addressing ---

/// Which instance of a backend to open. `Default` keeps the backend's own
/// environment-based routing (Herdr resolves `HERDR_SOCKET_PATH` /
/// `HERDR_SESSION`); `Named` addresses one session; `Socket` attaches to an
/// explicit socket path (SSH bridges, isolated tests).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstanceTarget {
    Default,
    Named(String),
    Socket(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceRef {
    /// Backend id as registered in [`registry::MuxRegistry`] (e.g. `"herdr"`).
    pub backend: String,
    pub target: InstanceTarget,
}

impl InstanceRef {
    pub fn default_instance(backend: &str) -> Self {
        Self {
            backend: backend.to_string(),
            target: InstanceTarget::Default,
        }
    }

    pub fn named(backend: &str, session: &str) -> Self {
        Self {
            backend: backend.to_string(),
            target: InstanceTarget::Named(session.to_string()),
        }
    }

    pub fn socket(backend: &str, path: PathBuf) -> Self {
        Self {
            backend: backend.to_string(),
            target: InstanceTarget::Socket(path),
        }
    }
}

/// One row of the backend's instance enumeration. `display_name` is the
/// Shardlane-owned override; `None` means callers fall back to `name`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstanceListing {
    pub backend: String,
    pub name: String,
    pub display_name: Option<String>,
    pub running: bool,
    pub is_default: bool,
}

// --- Capabilities ---

/// Backend capability declaration. Every field must have a named degradation
/// consumer in the UI/Remote surface (docs/multiplexer-api.md §7); a field
/// without a consumer must not exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MuxCapabilities {
    pub agents: bool,
    pub server_admin: bool,
    pub shared_tui: bool,
    pub pane_history_read: bool,
    pub cross_workspace_tab_move: bool,
    pub events_push: bool,
}

// --- Backend facade ---

pub trait Multiplexer: Send + Sync {
    /// Backend id used in [`InstanceRef::backend`] and registry routing.
    fn id(&self) -> &'static str;

    fn capabilities(&self) -> MuxCapabilities;

    /// Enumerate instances. `None` = enumeration is unavailable (e.g. CLI
    /// missing); callers degrade gracefully — this mirrors the Herdr adapter's
    /// existing contract.
    fn list_instances(&self) -> Option<Vec<InstanceListing>>;

    /// Write the Shardlane-owned display-name override for one instance.
    fn rename_instance(&self, instance: &str, display_name: &str) -> Result<(), MuxError>;

    fn stop_instance(&self, instance: &str) -> Result<(), MuxError>;

    fn delete_instance(&self, instance: &str) -> Result<(), MuxError>;

    /// Open a connection, starting the instance when it is not running
    /// (Herdr bootstrap semantics).
    fn open_instance(
        &self,
        reference: &InstanceRef,
    ) -> Result<std::sync::Arc<dyn MultiplexerConnection>, MuxError>;

    /// Side-effect-free connect: never starts an instance; fails when it is
    /// unreachable.
    fn connect_instance(
        &self,
        reference: &InstanceRef,
    ) -> Result<std::sync::Arc<dyn MultiplexerConnection>, MuxError>;

    /// Server-administration facet; `None` when `capabilities().server_admin`
    /// is false.
    fn server_admin(&self) -> Option<&dyn MultiplexerServerAdmin>;
}

pub trait MultiplexerServerAdmin: Send + Sync {
    fn cli_path(&self) -> Option<PathBuf>;
    fn installed_cli_version(&self) -> Option<String>;
    fn user_config_path(&self) -> PathBuf;
}

// --- Structural operation parameters ---

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    Right,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MuxDirection {
    Left,
    Right,
    Up,
    Down,
}

impl MuxDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CreateTab<'a> {
    pub workspace_id: Option<&'a str>,
    pub cwd: Option<&'a str>,
    pub focus: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CreateWorkspace<'a> {
    pub cwd: Option<&'a str>,
    pub focus: bool,
}

/// Bounded pane history read (attach bootstrap / remote pane output).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneHistory {
    pub pane_id: String,
    /// ANSI-encoded retained output, bounded by the requested line budget.
    pub text: String,
}

// --- Per-instance connection ---

pub trait MultiplexerConnection: Send + Sync {
    // Lifecycle / info
    fn ping(&self) -> Result<(), MuxError>;
    /// This connection's backend capabilities (same declaration as the
    /// backend's [`Multiplexer::capabilities`]); consumers degrade per
    /// catalog domain instead of string-matching backend ids.
    fn capabilities(&self) -> MuxCapabilities;
    /// Cached protocol version negotiated at open (no round trip); `None`
    /// when the backend has no protocol notion.
    fn protocol(&self) -> Option<u32>;
    fn server_started_with_supplied_config(&self) -> bool;

    // Domain 2 — projection snapshots
    fn navigation_state(&self) -> Result<NavigationState, MuxError>;
    fn visible_state(&self) -> Result<MuxState, MuxError>;
    fn host_bootstrap_state(&self) -> Result<MuxState, MuxError>;
    fn workspace_state(&self) -> Result<MuxState, MuxError>;
    fn workspace_panes(&self, workspace_id: &str) -> Result<Vec<Pane>, MuxError>;
    fn tab_surface_state(
        &self,
        workspace_id: &str,
        tab_id: &str,
    ) -> Result<TabSurfaceState, MuxError>;
    fn pane_layout(&self, pane_id: &str) -> Result<PaneLayout, MuxError>;
    fn agents(&self) -> Result<Vec<Agent>, MuxError>;

    // Domain 3 — events
    fn subscribe_events(&self) -> Result<async_channel::Receiver<MuxEvent>, MuxError>;
    fn subscribe_pane_events(
        &self,
        pane_ids: &[String],
    ) -> Result<async_channel::Receiver<MuxEvent>, MuxError>;

    // Workspaces
    fn create_workspace(
        &self,
        params: &CreateWorkspace<'_>,
    ) -> Result<WorkspaceCreatedResult, MuxError>;
    fn close_workspace(&self, workspace_id: &str) -> Result<(), MuxError>;
    fn rename_workspace(&self, workspace_id: &str, label: &str) -> Result<(), MuxError>;
    fn move_workspace(&self, workspace_id: &str, insert_index: usize) -> Result<(), MuxError>;
    fn move_workspace_before(
        &self,
        workspace_id: &str,
        before_workspace_id: &str,
    ) -> Result<(), MuxError>;
    fn workspace_focus(&self, workspace_id: &str) -> Result<(), MuxError>;

    // Domain 4 — tabs
    fn create_tab(&self, params: &CreateTab<'_>) -> Result<TabCreatedResult, MuxError>;
    fn close_tab(&self, tab_id: &str) -> Result<(), MuxError>;
    fn rename_tab(&self, tab_id: &str, label: &str) -> Result<(), MuxError>;
    fn move_tab(&self, tab_id: &str, insert_index: usize) -> Result<(), MuxError>;
    fn tab_focus(&self, tab_id: &str) -> Result<(), MuxError>;

    // Domain 5 — panes
    fn split_pane(&self, pane_id: &str, direction: SplitDirection) -> Result<Pane, MuxError>;
    fn close_pane(&self, pane_id: &str) -> Result<(), MuxError>;
    fn rename_pane(&self, pane_id: &str, label: &str) -> Result<(), MuxError>;
    fn swap_pane(
        &self,
        pane_id: &str,
        direction: MuxDirection,
    ) -> Result<PaneLayoutActionResult, MuxError>;
    fn resize_pane(
        &self,
        pane_id: &str,
        direction: MuxDirection,
    ) -> Result<PaneLayoutActionResult, MuxError>;
    fn toggle_pane_zoom(&self, pane_id: &str) -> Result<PaneLayoutActionResult, MuxError>;
    fn pane_focus(&self, pane_id: &str) -> Result<(), MuxError>;
    fn move_pane_to_tab(&self, pane_id: &str, tab_id: &str) -> Result<PaneMoveResult, MuxError>;
    fn move_pane_to_new_tab(
        &self,
        pane_id: &str,
        workspace_id: &str,
    ) -> Result<PaneMoveResult, MuxError>;
    fn set_split_ratio(&self, tab_id: &str, path: &[bool], ratio: f64) -> Result<(), MuxError>;
    fn send_text(&self, pane_id: &str, text: &str) -> Result<(), MuxError>;
    fn send_keys(&self, pane_id: &str, keys: &[String]) -> Result<(), MuxError>;
    fn pane_process_info(&self, pane_id: &str) -> Result<PaneProcessInfo, MuxError>;

    /// Bounded retained-history read. Default: capability
    /// `pane_history_read` unsupported.
    fn read_pane_history(&self, _pane_id: &str, _lines: u32) -> Result<PaneHistory, MuxError> {
        Err(MuxError::Unsupported("pane_history_read"))
    }

    /// Apply the backend server's reloaded configuration. Default: capability
    /// `server_admin` unsupported.
    fn reload_config(&self) -> Result<(), MuxError> {
        Err(MuxError::Unsupported("server_admin"))
    }

    // Domain 6 — shared terminal stream. Default: capability
    // `shared_tui` unsupported. (The Herdr adapter's stream registry is
    // owned by the host application process and is consumed directly by
    // the GUI/Remote; this seam is the tmux attach-child path.)
    fn open_shared_session(
        &self,
        _key: Option<&str>,
        _cols: u16,
        _rows: u16,
    ) -> Result<Arc<dyn MultiplexerStream>, MuxError> {
        Err(MuxError::Unsupported("shared_tui"))
    }

    // Domain 7 — agents (capability `agents`)
    fn agent_runtime(&self) -> Option<&dyn MuxAgentRuntime> {
        None
    }

    // Adapter escape hatch: Herdr-protocol-self concerns only (protocol
    // version gate, CLI version display, TUI protocol gate). Whitelist-
    // reviewed per docs/multiplexer-api.md §5.
    fn as_herdr(&self) -> Option<&crate::herdr::HerdrClient> {
        None
    }
}

// --- Domain 7 — agent runtime facet ---

/// Neutral agent mutation facet. `agent_runtime()` returns `None` unless
/// `capabilities().agents` is true; callers degrade per catalog domain 7.
pub trait MuxAgentRuntime: Send + Sync {
    fn start_runtime_agent(
        &self,
        request: &RuntimeAgentStartRequest,
    ) -> Result<RuntimeAgent, MuxError>;

    fn prompt_runtime_agent(
        &self,
        request: &RuntimeAgentPromptRequest,
    ) -> Result<RuntimeAgent, MuxError>;

    fn read_runtime_agent(
        &self,
        request: &RuntimeAgentReadRequest,
    ) -> Result<RuntimeAgentRead, MuxError>;

    fn wait_runtime_agent(
        &self,
        request: &RuntimeAgentWaitRequest,
    ) -> Result<RuntimeAgentWait, MuxError>;

    fn send_runtime_agent_keys(&self, agent_id: &AgentRef, keys: &[String])
        -> Result<(), MuxError>;

    /// Manual claim: declares that a pane is running the given agent; the
    /// backend corrects status later through its own events.
    fn report_pane_agent(&self, pane_id: &str, agent: &str) -> Result<(), MuxError>;

    /// Manual denial: clears every agent determination on a pane (the main
    /// correction path for false detections).
    fn clear_pane_agent_authority(&self, pane_id: &str) -> Result<(), MuxError>;

    /// Focus an agent by its opaque terminal id (callers fall back to the
    /// workspace/tab/pane focus chain on failure).
    fn agent_focus(&self, target: &str) -> Result<(), MuxError>;
}

pub use crate::runtime::{RuntimeAgentPromptRequest, RuntimeAgentReadRequest};

impl std::str::FromStr for MuxDirection {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            other => Err(format!(
                "direction must be \"left\", \"right\", \"up\" or \"down\", got \"{other}\""
            )),
        }
    }
}

// --- Domain 6 — terminal byte stream ---

pub use crate::dto::HerdrTuiSessionSummary as StreamSummary;
pub use crate::shared_tui::{TuiError as StreamError, TuiEvent as StreamEvent};

/// Terminal byte-stream facet over one shared session (docs §6, domain 6).
/// The Herdr implementation wraps the hosted TUI child; a `tmux attach` child
/// fills the same shape.
pub trait MultiplexerStream: Send + Sync {
    fn id(&self) -> &str;
    fn summary(&self) -> StreamSummary;
    fn is_running(&self) -> bool;
    fn viewer_count(&self) -> usize;
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<StreamEvent>;
    /// Subscribe with the startup byte replay so late viewers can restore
    /// DECSET state; returns the replayed bytes alongside the receiver.
    fn subscribe_with_startup_replay(
        &self,
    ) -> (tokio::sync::broadcast::Receiver<StreamEvent>, Vec<u8>);
    fn wake_subscribers(&self);
    fn send_bytes(&self, data: &[u8]) -> Result<(), StreamError>;
    /// Traced input variant (latency diagnostics carry a caller-supplied
    /// trace id and the packet's coalescibility class).
    fn send_bytes_traced(
        &self,
        data: &[u8],
        trace_id: u64,
        coalescible: bool,
    ) -> Result<(), StreamError>;
    fn resize(&self, cols: u16, rows: u16) -> Result<StreamSummary, StreamError>;
    fn force_redraw(&self) -> Result<(), StreamError>;
    fn stop(&self) -> bool;
    fn stop_with_reap(&self, reap_deadline: Option<std::time::Duration>) -> bool;
    fn wait_for_exit(&self, timeout: std::time::Duration) -> bool;
}
