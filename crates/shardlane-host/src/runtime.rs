//! Runtime-facing service-provider interfaces used by concrete Host services.
//!
//! This layer is still UI-independent. It may describe runtime correlation needed by the
//! Host, but it does not expose Herdr transport/socket details to remote-facing DTOs.
//!

use crate::dto::AgentStatus;
use crate::ids::{AgentRef, PaneId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeAgent {
    /// Opaque Host Agent target. The Herdr adapter currently backs this with the
    /// hosting Pane ID because Herdr Agent commands accept Pane IDs or unique names,
    /// but not terminal IDs. Clients must never depend on that encoding.
    pub id: AgentRef,
    pub runtime_workspace_id: String,
    pub tab_id: String,
    pub pane_id: String,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub title: Option<String>,
    pub status: AgentStatus,
    pub interactive_ready: bool,
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeAgentStartRequest {
    pub name: String,
    pub kind: String,
    pub pane_id: PaneId,
    pub args: Vec<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeAgentPromptRequest {
    pub agent_id: AgentRef,
    pub text: String,
    pub wait_until: Vec<AgentStatus>,
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RuntimeAgentReadFormat {
    #[default]
    Text,
    Ansi,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeAgentReadRequest {
    pub agent_id: AgentRef,
    pub lines: Option<u32>,
    pub format: RuntimeAgentReadFormat,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeAgentRead {
    pub agent_id: AgentRef,
    pub text: String,
    pub revision: u64,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeAgentWaitRequest {
    pub agent_id: AgentRef,
    pub until: Vec<AgentStatus>,
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeAgentWait {
    pub event: String,
    pub status: Option<AgentStatus>,
}

pub trait AgentRuntime {
    type Error;

    fn start_runtime_agent(
        &self,
        request: &RuntimeAgentStartRequest,
    ) -> Result<RuntimeAgent, Self::Error>;

    fn prompt_runtime_agent(
        &self,
        request: &RuntimeAgentPromptRequest,
    ) -> Result<RuntimeAgent, Self::Error>;

    fn read_runtime_agent(
        &self,
        request: &RuntimeAgentReadRequest,
    ) -> Result<RuntimeAgentRead, Self::Error>;

    fn wait_runtime_agent(
        &self,
        request: &RuntimeAgentWaitRequest,
    ) -> Result<RuntimeAgentWait, Self::Error>;

    fn send_runtime_agent_keys(
        &self,
        agent_id: &AgentRef,
        keys: &[String],
    ) -> Result<(), Self::Error>;
}
