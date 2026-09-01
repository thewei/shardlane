//! Narrow UI-independent application service contracts for Shardlane Host clients.
//!
//! Every operation takes explicit product/runtime identifiers. No service relies on
//! Herdr global focus or a desktop client's current navigation state.
//!
//! M1 convergence: the speculative Workspace/Project/Script/History/Terminal trait
//! scaffolding and the PTY-era `start_agent` request were removed. Product Agent
//! operations live on [`crate::agent_service::HostAgentService`] (over the
//! [`crate::runtime::AgentRuntime`] SPI), and product Conversation operations live
//! on [`crate::conversation_service::HostConversationService`]. Agent launch
//! becomes the Host-owned transaction introduced with the launch convergence
//! rather than a trait method here.
//!

use crate::dto::AgentStatus;
use crate::ids::AgentRef;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PromptAgentRequest {
    pub agent_id: AgentRef,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReadAgentRequest {
    pub agent_id: AgentRef,
    pub lines: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WaitAgentRequest {
    pub agent_id: AgentRef,
    pub until: Vec<AgentStatus>,
    pub timeout_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentWaitResult {
    pub agent_id: AgentRef,
    pub status: AgentStatus,
}

/// Typed disposition for a semantic Chat prompt. Clients render queue/terminal
/// guidance from this value instead of re-deriving send policy locally.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptDisposition {
    /// The Agent was sendable and the prompt was delivered exactly once.
    SentNow,
    /// The Agent is Working; one follow-up was queued for delivery after the
    /// same session settles (M4 queue semantics).
    QueuedAfterTurn,
    /// The interaction is terminal-only; the client must route the user to the
    /// explicit Terminal surface rather than sending bytes on its behalf.
    NeedsTerminal,
}

/// Internal strategy chosen by the History continuation planner (M6). Public
/// product actions stay `Continue`/`Continue with <Provider>`; the planner owns
/// this decision so Desktop and Remote cannot diverge.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationStrategy {
    /// The exact source session is already live; reuse it without creating an Agent.
    AlreadyLive,
    /// Same provider with an exact resumable native session.
    NativeResume,
    /// Different provider, or same provider without exact native resume; a new
    /// Agent receives the lossless context transfer briefing.
    ContextTransfer,
}

/// Phase at which a Conversation action failed. Sufficient for every client to
/// explain the failure and preserve the user's draft without a boolean-only
/// `accepted` contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationActionFailurePhase {
    ResolveSource,
    ResolveProject,
    PrepareTarget,
    Launch,
    Readiness,
    Snapshot,
    TransferPlan,
    Delivery,
    /// The runtime accepted the prompt but the outcome could not be verified;
    /// never automatically retried.
    DeliveryUncertain,
    Binding,
}

// C22: the `AgentService` trait was deleted (single impl, never used as a
// bound — the speculative scaffolding M1 already removed once). Its request/
// result DTOs stay: the concrete [`crate::agent_service::HostAgentService`]
// exposes the same operations as inherent methods.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::ConversationPage;

    #[test]
    fn service_requests_always_carry_explicit_targets() {
        let prompt = PromptAgentRequest {
            agent_id: AgentRef::new("agent-1"),
            text: "continue".into(),
        };
        let read = ReadAgentRequest {
            agent_id: AgentRef::new("agent-1"),
            lines: 80,
        };
        let wait = WaitAgentRequest {
            agent_id: AgentRef::new("agent-1"),
            until: vec![AgentStatus::Idle],
            timeout_ms: Some(1_000),
        };

        assert_eq!(prompt.agent_id.as_str(), "agent-1");
        assert_eq!(read.agent_id.as_str(), "agent-1");
        assert_eq!(wait.until, vec![AgentStatus::Idle]);
    }

    #[test]
    fn action_results_serialize_with_stable_snake_case_names() {
        assert_eq!(
            serde_json::to_string(&PromptDisposition::QueuedAfterTurn)
                .ok()
                .as_deref(),
            Some("\"queued_after_turn\"")
        );
        assert_eq!(
            serde_json::to_string(&ContinuationStrategy::ContextTransfer)
                .ok()
                .as_deref(),
            Some("\"context_transfer\"")
        );
        assert_eq!(
            serde_json::to_string(&ConversationActionFailurePhase::DeliveryUncertain)
                .ok()
                .as_deref(),
            Some("\"delivery_uncertain\"")
        );
        let page = ConversationPage {
            conversations: Vec::new(),
            next_cursor: None,
        };
        assert!(serde_json::to_string(&page).is_ok());
    }
}
