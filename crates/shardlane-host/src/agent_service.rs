//! Host-owned product Agent application service over the runtime SPI.
//!
//! [INPUT]: a connected `HerdrClient` (which is also the production
//! implementation of [`crate::runtime::AgentRuntime`]) and the Host
//! `ProjectIndex` projection.
//! [OUTPUT]: `HostAgentService` — the single high-level entry point for
//! product-level Agent query / semantic prompt/read/wait/keys; plus the
//! agent→summary projection shared with Remote bootstrap
//! (`agent_summaries_from_state`), eliminating a second Agent summary
//! mapping.
//! [POS]: audit AF-14. GPUI/HTTP semantic code must not bypass this service
//! to reach `AgentRuntime` directly. The Agent launch transaction lands in
//! M2 as a separate module of this crate, not as launch policy duplicated on
//! the SPI.

use crate::dto::{AgentOutput, AgentStatus, AgentSummary};
use crate::herdr::{host_agent_status, HerdrClient, HerdrError, HerdrState};
use crate::ids::{AgentRef, PaneId, ProjectId, TabId};
use crate::project_index::ProjectIndex;
use crate::services::{AgentWaitResult, PromptAgentRequest, ReadAgentRequest, WaitAgentRequest};

/// Single agent-summary projection shared by the Remote bootstrap and the
/// Host Agent service. Agents without a runtime workspace projection or a
/// resolvable tab are skipped exactly as the bootstrap does today.
pub fn agent_summaries_from_state(state: &HerdrState) -> Vec<AgentSummary> {
    let index = ProjectIndex::build_from_state(state);
    let panes_by_id: std::collections::HashMap<&str, &crate::herdr::Pane> = state
        .panes
        .iter()
        .map(|pane| (pane.pane_id.as_str(), pane))
        .collect();
    state
        .agents
        .iter()
        .filter_map(|agent| {
            let pane_id = agent.pane_id.as_deref().filter(|id| !id.is_empty())?;
            let project = agent
                .workspace_id
                .as_deref()
                .and_then(|workspace_id| index.for_runtime_id(workspace_id))?;
            let tab_id = agent
                .tab_id
                .clone()
                .or_else(|| {
                    panes_by_id
                        .get(pane_id)
                        .and_then(|pane| pane.tab_id.clone())
                })
                .or_else(|| project.tab_ids.first().cloned())?;
            let project_id = match &project.runtime_workspace_id {
                Some(runtime_id) => crate::project_id_for_runtime_workspace(runtime_id),
                // C10: one shared "unresolved project" identity, never a
                // fabricated per-site sentinel.
                None => crate::unresolved_project_id(),
            };
            Some(AgentSummary {
                id: AgentRef::new(pane_id.to_string()),
                project_id,
                tab_id: TabId::new(tab_id),
                pane_id: PaneId::new(pane_id.to_string()),
                name: agent.name.clone(),
                kind: agent.agent.clone().or_else(|| agent.display_agent.clone()),
                title: agent.title.clone(),
                status: host_agent_status(agent.agent_status.as_deref()),
                conversation_id: agent.agent_session.as_ref().and_then(|session| {
                    semantic_session_identity(session).then(|| {
                        crate::conversation_id_for_live_session(
                            &AgentRef::new(pane_id.to_string()),
                            session,
                        )
                    })
                }),
                revision: agent.revision,
            })
        })
        .collect()
}

/// Same semantic-identity gate the bootstrap applies: a Conversation id is only
/// exposed when the Herdr session locator is typed and resolvable.
fn semantic_session_identity(session: &crate::herdr::AgentSessionInfo) -> bool {
    if session.value.trim().is_empty() {
        return false;
    }
    matches!(
        crate::resolve_agent_session_source(session),
        Ok(shardlane_history::SessionSourceLocator::NativeId { .. })
            | Ok(shardlane_history::SessionSourceLocator::FilePath { .. })
    )
}

/// Concrete high-level Agent service. Owns no launch strategy (M2 transaction)
/// and never mutates client focus.
pub struct HostAgentService<'a> {
    client: &'a HerdrClient,
}

impl<'a> HostAgentService<'a> {
    pub fn new(client: &'a HerdrClient) -> Self {
        Self { client }
    }

    fn bootstrap_summaries(&self) -> Result<Vec<AgentSummary>, HerdrError> {
        Ok(agent_summaries_from_state(
            &self.client.host_bootstrap_state()?,
        ))
    }

    // C22: the single-impl `AgentService` trait was speculative scaffolding
    // (the exact pattern M1 removed once already); these are the same
    // operations as inherent methods on the concrete service.
    pub fn list_agents(&self, project_id: &ProjectId) -> Result<Vec<AgentSummary>, HerdrError> {
        Ok(self
            .bootstrap_summaries()?
            .into_iter()
            .filter(|summary| &summary.project_id == project_id)
            .collect())
    }

    pub fn prompt_agent(&self, request: &PromptAgentRequest) -> Result<AgentSummary, HerdrError> {
        // AC-17: the v1 Agent service is an adapter over the canonical
        // semantic prompt executor — never a second semantic implementation.
        let mutation = crate::conversation_service::prompt_live_agent(
            self.client,
            &request.agent_id,
            &request.text,
        )
        .map_err(|error| HerdrError::Api(error.to_string()))?;
        // `agent.prompt` has already committed at this point.  Bootstrap is a
        // post-commit enrichment read and may legitimately fail or lag behind
        // the mutation; do not turn that into a retryable prompt failure.
        if let Ok(summaries) = self.bootstrap_summaries() {
            if let Some(summary) = summaries
                .into_iter()
                .find(|summary| summary.id == request.agent_id)
            {
                return Ok(summary);
            }
        }
        Ok(AgentSummary {
            id: mutation.identity.agent_ref.clone(),
            // C10: one shared "unresolved project" identity — the old
            // `project_id_for_runtime_workspace("")` sentinel was not even
            // decodable and differed from the other orphan spelling.
            project_id: crate::unresolved_project_id(),
            tab_id: TabId::new(String::new()),
            pane_id: PaneId::new(mutation.identity.agent_ref.as_str().to_string()),
            name: None,
            kind: Some(mutation.identity.provider),
            title: None,
            status: AgentStatus::Unknown,
            conversation_id: Some(mutation.identity.conversation_id),
            revision: mutation.identity.revision,
        })
    }

    pub fn read_agent(&self, request: &ReadAgentRequest) -> Result<AgentOutput, HerdrError> {
        use crate::runtime::{AgentRuntime, RuntimeAgentReadFormat, RuntimeAgentReadRequest};
        let read = self.client.read_runtime_agent(&RuntimeAgentReadRequest {
            agent_id: request.agent_id.clone(),
            lines: Some(request.lines),
            format: RuntimeAgentReadFormat::Text,
        })?;
        Ok(AgentOutput {
            agent_id: read.agent_id,
            text: read.text,
            revision: read.revision,
            truncated: read.truncated,
        })
    }

    pub fn wait_agent(&self, request: &WaitAgentRequest) -> Result<AgentWaitResult, HerdrError> {
        use crate::runtime::{AgentRuntime, RuntimeAgentWaitRequest};
        let wait = self.client.wait_runtime_agent(&RuntimeAgentWaitRequest {
            agent_id: request.agent_id.clone(),
            until: request.until.clone(),
            timeout_ms: request.timeout_ms,
        })?;
        Ok(AgentWaitResult {
            agent_id: request.agent_id.clone(),
            status: wait.status.unwrap_or(AgentStatus::Unknown),
        })
    }

    pub fn send_agent_keys(&self, agent_id: &AgentRef, keys: &[String]) -> Result<(), HerdrError> {
        use crate::runtime::AgentRuntime;
        self.client.send_runtime_agent_keys(agent_id, keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_summaries_skip_agents_without_runtime_projection() {
        let state = HerdrState::default();
        assert!(agent_summaries_from_state(&state).is_empty());
    }
}
