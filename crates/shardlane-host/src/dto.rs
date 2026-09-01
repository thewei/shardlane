//! Serializable Shardlane Host projections shared by non-GPUI clients.
//!
//! These DTOs are product-level contracts. They intentionally contain no GPUI/AppKit
//! objects, Herdr socket paths, generic JSON payloads, or full History transcripts.
//!

pub use crate::conversation_interactions::{
    ConversationInteraction, ConversationInteractionAnchor, ConversationInteractionKind,
    ConversationInteractionState, InteractionChoice, InteractionId, InteractionResolution,
    InteractionResponse, PermissionScope,
};
use crate::ids::{AgentRef, ConversationId, PaneId, ProjectId, TabId, WorkspaceId};
use serde::{Deserialize, Serialize};

pub const HOST_API_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostInfo {
    pub host_id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostCapabilities {
    pub agent_control: bool,
    pub scripts: bool,
    pub history: bool,
    pub terminal_text: bool,
    pub terminal_stream: bool,
    /// Semantic Conversation projection (Live + History) is available.
    #[serde(default)]
    pub conversation_view: bool,
    /// A Live Conversation can be hydrated and incrementally refreshed.
    #[serde(default)]
    pub conversation_live: bool,
    /// History Continue owns the Continue/Fork → readiness → prompt transaction.
    #[serde(default)]
    pub history_continue: bool,
    /// Host-owned shared/global Herdr TUI lifecycle is available. Focus, Tab
    /// selection, input, and resize are intentionally shared with other clients.
    #[serde(default)]
    pub herdr_tui: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceSummary {
    pub id: WorkspaceId,
    pub name: String,
    pub color: String,
    pub active: bool,
    pub project_ids: Vec<ProjectId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectSummary {
    pub id: ProjectId,
    pub workspace_id: WorkspaceId,
    pub label: String,
    /// Display/search metadata only. Clients must never use this path as Project identity.
    pub project_path: Option<String>,
    pub runtime_available: bool,
    pub tab_ids: Vec<TabId>,
    /// C23 scope decision: kept — the mobile Zod bootstrap contract
    /// (herdr-mobile `projectSummarySchema`) still consumes it.
    pub agent_refs: Vec<AgentRef>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TabSummary {
    pub id: TabId,
    pub project_id: ProjectId,
    pub label: Option<String>,
    pub title: Option<String>,
    pub pane_ids: Vec<PaneId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PaneSummary {
    pub id: PaneId,
    pub tab_id: TabId,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub agent_ref: Option<AgentRef>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentSummary {
    pub id: AgentRef,
    pub project_id: ProjectId,
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub title: Option<String>,
    pub status: AgentStatus,
    /// Present only when Herdr reported a typed session identity; clients can use this
    /// opaque id to open the semantic Conversation surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<ConversationId>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub revision: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

// C23: the stale Script DTO subtree (`ScriptKind`/`ScriptStatus`/
// `ScriptSummary`, `ProjectSummary.script_ids`, `HostBootstrap.scripts`) was
// deleted — the remote always emitted `scripts: false` + empty arrays and the
// GUI owns its own script types. Wire-shape change: golden fixtures were
// regenerated; the mobile Zod mirror must drop `script_ids`/`scripts` in the
// same window.

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConversationSummary {
    pub id: ConversationId,
    pub project_id: ProjectId,
    pub source: ConversationSource,
    pub agent_kind: String,
    pub title: String,
    #[serde(default)]
    pub status: Option<AgentStatus>,
    #[serde(default)]
    pub sendable: bool,
    #[serde(default)]
    pub live_agent_ref: Option<AgentRef>,
    pub updated_at_ms: Option<u64>,
    #[serde(default)]
    pub revision: u64,
}

/// Product-level Conversation origin. Provider history files and Herdr live
/// projections share the same semantic surface but keep different controllers.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationSource {
    Live,
    History,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationItemKind {
    User,
    Assistant,
    Reasoning,
    Tool,
    Activity,
    Meta,
    CompactSummary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConversationToolCall {
    pub id: String,
    pub name: String,
    pub input_preview: String,
    #[serde(default)]
    pub input: Option<String>,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub is_error: bool,
}

/// Bounded, provider-neutral semantic item used by both Live Chat and History.
/// `seq` is the source ordering key; `id` remains stable across window refreshes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConversationItem {
    pub id: String,
    pub seq: u64,
    pub kind: ConversationItemKind,
    #[serde(default)]
    pub role: Option<String>,
    pub text: String,
    #[serde(default)]
    pub thinking: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ConversationToolCall>,
    #[serde(default)]
    pub timestamp_ms: Option<u64>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostBootstrap {
    pub host: HostInfo,
    pub capabilities: HostCapabilities,
    pub workspaces: Vec<WorkspaceSummary>,
    pub projects: Vec<ProjectSummary>,
    pub tabs: Vec<TabSummary>,
    pub panes: Vec<PaneSummary>,
    pub agents: Vec<AgentSummary>,
    /// Bounded metadata only; transcript messages are fetched through HistoryService.
    pub conversations: Vec<ConversationSummary>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentOutput {
    pub agent_id: AgentRef,
    pub text: String,
    pub revision: u64,
    pub truncated: bool,
}

// C22: the dead `ConversationMessage {seq, role, text}` DTO was deleted —
// zero constructors/readers; `ConversationItem` is the semantic wire shape.

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConversationWindow {
    pub conversation_id: ConversationId,
    #[serde(default)]
    pub revision: u64,
    pub items: Vec<ConversationItem>,
    #[serde(default)]
    pub first_seq: Option<u64>,
    #[serde(default)]
    pub last_seq: Option<u64>,
    pub has_older: bool,
    pub has_newer: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConversationPage {
    pub conversations: Vec<ConversationSummary>,
    pub next_cursor: Option<String>,
}

/// Canonical Conversation identity returned by Host Conversation mutations.
/// Provider-native ids are opaque provider identities; path-backed provider
/// sessions serialize `None` so provider file paths never cross the DTO seam.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConversationIdentity {
    pub conversation_id: ConversationId,
    pub agent_ref: AgentRef,
    pub provider: String,
    pub native_session_id: Option<String>,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConversationMutation {
    pub identity: ConversationIdentity,
    pub accepted: bool,
}

/// Summary plus bounded window: the read projection returned by Conversation
/// detail queries for both Live and History sources.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConversationDetail {
    pub conversation: ConversationSummary,
    pub window: ConversationWindow,
    #[serde(default)]
    pub interactions: Vec<ConversationInteraction>,
}

/// The normal Terminal surface is a single Host-owned Herdr TUI session. This
/// mode is deliberately not parameterized by Project/Tab/Pane/client identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrTuiMode {
    Shared,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrTuiSessionStatus {
    Starting,
    Running,
    Stopped,
    Failed,
}

/// Opaque shared TUI session identity and current viewport projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HerdrTuiSessionSummary {
    pub id: String,
    pub mode: HerdrTuiMode,
    pub status: HerdrTuiSessionStatus,
    pub cols: u16,
    pub rows: u16,
    pub revision: u64,
}

/// Semantic input accepted by the Host-owned TUI session. Text and paste are
/// committed UTF-8; named keys are encoded by the Host adapter so clients do
/// not handcraft terminal escape sequences.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HerdrTuiInput {
    Text {
        text: String,
    },
    Paste {
        text: String,
    },
    Key {
        code: HerdrTuiKeyCode,
        #[serde(default)]
        modifiers: HerdrTuiModifiers,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HerdrTuiKeyCode {
    Enter,
    Escape,
    Tab,
    Backspace,
    Space,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    /// Letter keys so remote/native clients can express terminal control
    /// shortcuts (Ctrl+C interrupt, Ctrl+D EOF, Ctrl+L clear, Ctrl+R search).
    /// Encoded Host-side: Ctrl+letter → 0x01..=0x1A, Shift → uppercase,
    /// Alt/Meta → ESC prefix.
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct HerdrTuiModifiers {
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub meta: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_is_serializable_without_runtime_transport_details() {
        let bootstrap = HostBootstrap {
            host: HostInfo {
                host_id: "host-1".into(),
                name: "Mac".into(),
                version: "0.1.11".into(),
                api_version: HOST_API_VERSION,
            },
            capabilities: HostCapabilities {
                agent_control: true,
                scripts: true,
                history: true,
                terminal_text: true,
                terminal_stream: false,
                conversation_view: true,
                conversation_live: true,
                history_continue: true,
                herdr_tui: true,
            },
            workspaces: vec![],
            projects: vec![],
            tabs: vec![],
            panes: vec![],
            agents: vec![],
            conversations: vec![],
        };

        let json = serde_json::to_string(&bootstrap)
            .unwrap_or_else(|error| panic!("bootstrap serialization failed: {error}"));
        assert!(json.contains("\"api_version\":1"));
        assert!(!json.contains("socket_path"));
        assert!(!json.contains("transcript"));
    }

    #[test]
    fn tui_input_uses_semantic_key_wire_shape() {
        let input = HerdrTuiInput::Key {
            code: HerdrTuiKeyCode::Tab,
            modifiers: HerdrTuiModifiers {
                shift: true,
                ..HerdrTuiModifiers::default()
            },
        };
        let value = serde_json::to_value(input)
            .unwrap_or_else(|error| panic!("TUI input must serialize: {error}"));
        assert_eq!(value["kind"], "key");
        assert_eq!(value["code"], "tab");
        assert_eq!(value["modifiers"]["shift"], true);
    }
}
