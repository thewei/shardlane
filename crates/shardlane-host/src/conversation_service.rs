//! Host-owned semantic Conversation application service.
//!
//! [INPUT]: a connected `HerdrClient`, the Shardlane history catalog path,
//! and the Conversation/Agent targets resolved by the caller.
//! [OUTPUT]: the detail/window read projections for Live/History
//! Conversations, semantic prompt with a commit point and the Host
//! MutationLedger, and the Host-exclusive History Continue (Fork → readiness
//! → identity verify → exactly-once prompt; fail-closed when the Live
//! identity is missing) production policy.
//! [POS]: the landing spot of audits AF-01/AF-04/AF-05/AF-06. Remote and the
//! native GUI only decode/encode; they must not own provider resume/
//! workspace/start/readiness policy outside this module. This service does
//! no client focus navigation.

use crate::conversation_delivery::ConversationDeliveryCoordinator;
use crate::conversation_interactions::{
    ConversationInteractionBroker, InteractionResolution, InteractionResolveError,
    InteractionResponse,
};
use crate::conversation_queue::{QueueState, QueuedFollowUp};
use crate::conversations::{
    agent_provider_kind, agent_provider_label, continued_identity,
    conversation_id_for_live_session, find_agent_by_ref, history_summary, live_agent_ref,
    live_session_source, live_summary, normalize_live_snapshot, public_native_session_id,
    session_fingerprint, HistoryConversationService,
};
use crate::dto::{
    ConversationDetail, ConversationIdentity, ConversationMutation, ConversationSummary,
    ConversationWindow,
};
use crate::herdr::{
    host_agent_status, AgentPromptParams, AgentSessionInfo, HerdrClient, HerdrError,
};
use crate::ids::{AgentRef, ConversationId, InteractionId, ProjectId};
use crate::project_index::ProjectIndex;
use crate::services::PromptDisposition;
use shardlane_history::models::{SessionFileRef, SessionMeta};
use shardlane_history::{HistoryCatalog, LiveSession};
use std::fmt;
use std::path::PathBuf;

pub const DEFAULT_CONVERSATION_WINDOW: u32 = 80;
pub const MAX_CONVERSATION_WINDOW: u32 = 200;

#[derive(Debug)]
pub enum ConversationServiceError {
    /// The request itself is invalid (empty text, wrong Conversation source).
    Invalid(String),
    /// The requested Conversation/Agent does not exist.
    NotFound(String),
    /// A Herdr runtime call failed.
    Herdr(HerdrError),
    /// A semantic operation failed without a transport-level cause.
    Runtime(String),
    /// Continuation requires a Project before any runtime mutation (M6).
    NeedsProject(String),
    /// The Agent launch committed a runtime target, but a later setup phase
    /// failed. Callers must reconcile/focus this target and must not retry the
    /// whole launch blindly.
    AgentCreated {
        agent_ref: AgentRef,
        tab_id: String,
        pane_id: String,
        phase: crate::agent_launch::CreatedAgentPhase,
        detail: String,
    },
}

impl fmt::Display for ConversationServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => formatter.write_str(message),
            Self::NotFound(message) => formatter.write_str(message),
            Self::Herdr(error) => write!(formatter, "{error}"),
            Self::Runtime(message) => formatter.write_str(message),
            Self::NeedsProject(message) => formatter.write_str(message),
            Self::AgentCreated {
                agent_ref,
                tab_id,
                pane_id: _,
                phase: _,
                detail,
            } => write!(
                formatter,
                "agent created (pane={}, tab={}) but setup requires attention: {detail}",
                agent_ref.as_str(),
                tab_id
            ),
        }
    }
}

impl std::error::Error for ConversationServiceError {}

/// Bounded window request shared by Live and History detail queries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConversationWindowBounds {
    pub anchor_seq: Option<u64>,
    pub before: Option<u32>,
    pub after: Option<u32>,
}

/// AC-04 result of the authoritative prompt submission: what the Host decided
/// happened to the text. `SentNow` carries the mutation identity;
/// `QueuedAfterTurn` carries the single queued item; `NeedsTerminal` carries
/// nothing because no semantic mutation was attempted.
#[derive(Clone, Debug)]
pub struct PromptSubmission {
    pub disposition: PromptDisposition,
    pub mutation: Option<ConversationMutation>,
    pub queued: Option<QueuedFollowUp>,
}

pub fn clamp_conversation_window(value: u32) -> u32 {
    value.clamp(1, MAX_CONVERSATION_WINDOW)
}

fn window_half(bounds: ConversationWindowBounds) -> (usize, usize) {
    // Values are clamped to [1, 200], so the widening cast is lossless
    // (C38: the old `unwrap_or(80)` fallback was unreachable).
    let before =
        clamp_conversation_window(bounds.before.unwrap_or(DEFAULT_CONVERSATION_WINDOW)) as usize;
    let after =
        clamp_conversation_window(bounds.after.unwrap_or(DEFAULT_CONVERSATION_WINDOW)) as usize;
    (before, after)
}

fn find_herdr_agent(
    client: &HerdrClient,
    agent_ref: &str,
) -> Result<crate::herdr::Agent, ConversationServiceError> {
    find_agent_by_ref(client, agent_ref)
        .map_err(ConversationServiceError::Herdr)?
        .ok_or_else(|| ConversationServiceError::NotFound("agent not found".to_string()))
}

/// Project correlation for a live agent, using the same ProjectIndex mapping the
/// Host bootstrap projection applies (runtime workspace id first, discovered
/// project path second). Never infers from cwd/mtime.
///
/// C32/C33: the agent already carries its runtime workspace id, so the
/// Project identity derives from it directly (zero extra RPCs); the narrowed
/// `workspace_state` projection + ProjectIndex path lookup is only the
/// fallback when no workspace id exists.
fn live_agent_project_id(
    client: &HerdrClient,
    agent: &crate::herdr::Agent,
) -> Result<ProjectId, ConversationServiceError> {
    if let Some(workspace_id) = agent.workspace_id.as_deref() {
        // Runtime-workspace-first: identical to the ProjectIndex projection's
        // `runtime_workspace_id → project_id_for_runtime_workspace` mapping,
        // without the bootstrap RPC fan-out or the index rebuild.
        return Ok(crate::project_id_for_runtime_workspace(workspace_id));
    }
    // Miss fallback: the discovered-path phase of the same ProjectIndex
    // mapping (no per-workspace pane.list fan-out — C32).
    let Some(discovered_path) = crate::project_index::non_empty(agent.foreground_cwd.as_deref())
        .or_else(|| crate::project_index::non_empty(agent.cwd.as_deref()))
    else {
        return Err(ConversationServiceError::Runtime(
            "agent project could not be resolved".to_string(),
        ));
    };
    let state = client
        .workspace_state()
        .map_err(ConversationServiceError::Herdr)?;
    let index = ProjectIndex::build_from_state(&state);
    Ok(match index.for_project_path(discovered_path) {
        // C10: one shared "unresolved project" identity, never a fabricated
        // per-site sentinel.
        Some(projection) => match &projection.runtime_workspace_id {
            Some(runtime_id) => crate::project_id_for_runtime_workspace(runtime_id),
            None => crate::unresolved_project_id(),
        },
        None => crate::project_id_for_path(discovered_path),
    })
}

fn live_summary_for_agent(
    client: &HerdrClient,
    agent: &crate::herdr::Agent,
    agent_ref: &AgentRef,
    snapshot_revision: u64,
) -> Result<ConversationSummary, ConversationServiceError> {
    let session = agent.agent_session.as_ref().ok_or_else(|| {
        ConversationServiceError::Runtime("agent has no typed conversation identity".to_string())
    })?;
    let project_id = live_agent_project_id(client, agent)?;
    let provider = agent_provider_label(agent, session);
    Ok(live_summary(
        agent_ref.clone(),
        Some(session),
        project_id,
        provider,
        agent
            .title
            .clone()
            .or_else(|| agent.name.clone())
            .unwrap_or_else(|| "Live conversation".to_string()),
        host_agent_status(agent.agent_status.as_deref()),
        agent.revision.max(snapshot_revision),
    ))
}

/// The concrete Host Conversation service. Constructed per request batch by
/// adapters (Remote handler, native background task); it holds no UI state and
/// never mutates client focus. When a shared [`ConversationSessionManager`] is
/// attached, live reads are served from the subscribed session instead of
/// opening a parallel parser for the same Conversation (audit AF-05).
pub struct HostConversationService<'a> {
    client: &'a HerdrClient,
    history_db_path: PathBuf,
    shared_sessions:
        Option<std::sync::Arc<crate::conversation_sessions::ConversationSessionManager>>,
    interaction_broker: Option<std::sync::Arc<ConversationInteractionBroker>>,
}

impl<'a> HostConversationService<'a> {
    pub fn new(client: &'a HerdrClient, history_db_path: impl Into<PathBuf>) -> Self {
        Self {
            client,
            history_db_path: history_db_path.into(),
            shared_sessions: None,
            interaction_broker: None,
        }
    }

    /// Serve live reads through the process-shared subscribed session manager.
    pub fn with_shared_sessions(
        mut self,
        manager: std::sync::Arc<crate::conversation_sessions::ConversationSessionManager>,
    ) -> Self {
        self.shared_sessions = Some(manager);
        self
    }

    /// Attach the Host interaction broker for in-turn interactions.
    pub fn with_interaction_broker(
        mut self,
        broker: std::sync::Arc<ConversationInteractionBroker>,
    ) -> Self {
        self.interaction_broker = Some(broker);
        self
    }

    fn catalog(&self) -> Result<HistoryCatalog, ConversationServiceError> {
        HistoryCatalog::open(&self.history_db_path)
            .map_err(|error| ConversationServiceError::Runtime(error.to_string()))
    }

    pub fn history_session(
        &self,
        key: &str,
    ) -> Result<Option<SessionMeta>, ConversationServiceError> {
        let catalog = self.catalog()?;
        HistoryConversationService::new(&catalog)
            .session(key)
            .map_err(ConversationServiceError::Runtime)
    }

    /// History detail: bounded window plus summary, exactly the projection the
    /// Host history seam defines (no provider knowledge in this method).
    pub fn history_detail(
        &self,
        key: &str,
        bounds: ConversationWindowBounds,
    ) -> Result<ConversationDetail, ConversationServiceError> {
        let catalog = self.catalog()?;
        let service = HistoryConversationService::new(&catalog);
        let meta = service
            .session(key)
            .map_err(ConversationServiceError::Runtime)?
            .ok_or_else(|| {
                ConversationServiceError::NotFound("conversation not found".to_string())
            })?;
        let (before, after) = window_half(bounds);
        let (conversation, window) = service
            .window(
                key,
                crate::project_id_for_path(&meta.project_path),
                bounds.anchor_seq,
                before as u32,
                after as u32,
                MAX_CONVERSATION_WINDOW,
            )
            .map_err(ConversationServiceError::Runtime)?;
        Ok(ConversationDetail {
            conversation,
            window,
            interactions: vec![],
        })
    }

    /// Live detail: exact typed agent session → provider source → bounded
    /// semantic window. Provider parsing stays behind the history/live adapters;
    /// this method owns only correlation, bounds, and the summary projection.
    pub fn live_detail(
        &self,
        agent_ref: &AgentRef,
        bounds: ConversationWindowBounds,
    ) -> Result<ConversationDetail, ConversationServiceError> {
        self.live_detail_with_fingerprint(agent_ref, bounds, None)
    }

    /// R2-07/CR-12: read paths that resolved a v2 session-exact locator pass
    /// its fingerprint here; a stale ConversationId must fail closed instead
    /// of silently serving the pane's replacement occupant.
    fn live_detail_with_fingerprint(
        &self,
        agent_ref: &AgentRef,
        bounds: ConversationWindowBounds,
        expected_fingerprint: Option<&str>,
    ) -> Result<ConversationDetail, ConversationServiceError> {
        let client = self.client;
        let agent = find_herdr_agent(client, agent_ref.as_str())?;
        let session = agent.agent_session.as_ref().ok_or_else(|| {
            ConversationServiceError::Runtime(
                "agent has no typed conversation identity".to_string(),
            )
        })?;
        if let Some(expected) = expected_fingerprint {
            if crate::session_fingerprint(session) != expected {
                return Err(ConversationServiceError::Invalid(
                    // C26: keep the message on one line — the collapsed
                    // newline used to render as a run of spaces.
                    "conversation identity is stale; the pane now hosts a different session; \
                     reopen the conversation"
                        .to_string(),
                ));
            }
        }
        let (all_items, snapshot_generation) = match self.live_source(session)? {
            Some(source) => {
                let snapshot = match &self.shared_sessions {
                    Some(manager) => manager.shared_snapshot(&source)?,
                    None => LiveSession::open(source.clone())
                        .map_err(|error| ConversationServiceError::Runtime(error.to_string()))?
                        .snapshot(),
                };
                (normalize_live_snapshot(&snapshot), snapshot.generation)
            }
            // Brand-new live session: no transcript indexed yet. Serve an empty
            // window so the client can render the conversation and send the
            // first prompt; the transcript appears once the agent writes it.
            None => (Vec::new(), 0),
        };
        let (before, after) = window_half(bounds);
        let limit = before.saturating_add(after).max(1);
        let anchor_index = bounds.anchor_seq.map(|seq| {
            // Resolve an exact source sequence first. If a live snapshot has
            // compacted that sequence, use the first later sequence as the stable
            // forward anchor; never treat `seq` as a zero-based array index.
            all_items
                .iter()
                .position(|item| item.seq == seq)
                .or_else(|| all_items.iter().position(|item| item.seq > seq))
                .unwrap_or(all_items.len())
        });
        let start = anchor_index
            .map(|index| index.saturating_sub(before))
            .unwrap_or_else(|| all_items.len().saturating_sub(limit));
        let end = start.saturating_add(limit).min(all_items.len());
        let items = all_items[start.min(all_items.len())..end].to_vec();
        let revision = agent.revision.max(snapshot_generation);
        let conversation = live_summary_for_agent(client, &agent, agent_ref, snapshot_generation)?;
        let conversation_id = conversation_id_for_live_session(agent_ref, session);
        let interactions = self
            .interaction_broker
            .as_ref()
            .map(|broker| broker.snapshot(&conversation_id))
            .unwrap_or_default();
        let window = ConversationWindow {
            conversation_id,
            revision,
            first_seq: items.first().map(|item| item.seq),
            last_seq: items.last().map(|item| item.seq),
            has_older: start > 0,
            has_newer: end < all_items.len(),
            items,
        };
        Ok(ConversationDetail {
            conversation,
            window,
            interactions,
        })
    }

    /// Resolve an in-turn interaction through the Host interaction broker with
    /// exact live occupant validation.
    pub fn resolve_conversation_interaction(
        &self,
        conversation_id: &ConversationId,
        interaction_id: &InteractionId,
        expected_revision: u64,
        response: InteractionResponse,
    ) -> Result<InteractionResolution, ConversationServiceError> {
        let broker = self.interaction_broker.as_ref().ok_or_else(|| {
            ConversationServiceError::Runtime("interaction broker is not configured".to_string())
        })?;

        let locator = crate::resolve_conversation_id(conversation_id)
            .map_err(|error| ConversationServiceError::Invalid(error.to_string()))?;

        let current_fingerprint = match &locator {
            crate::ConversationLocator::Live(agent_ref)
            | crate::ConversationLocator::LiveSession { agent_ref, .. } => {
                let agent = find_herdr_agent(self.client, agent_ref.as_str())?;
                let session = agent.agent_session.as_ref().ok_or_else(|| {
                    ConversationServiceError::Runtime(
                        "agent has no typed conversation identity".to_string(),
                    )
                })?;
                crate::session_fingerprint(session)
            }
            crate::ConversationLocator::History(_) => {
                return Err(ConversationServiceError::Invalid(
                    "cannot resolve interactions on read-only history conversations".to_string(),
                ));
            }
        };

        broker
            .resolve(
                conversation_id,
                interaction_id,
                expected_revision,
                response,
                &current_fingerprint,
            )
            .map_err(|error| match error {
                InteractionResolveError::NotFound => {
                    ConversationServiceError::NotFound("interaction not found".to_string())
                }
                InteractionResolveError::StaleRevision { .. }
                | InteractionResolveError::AlreadyResolved { .. }
                | InteractionResolveError::OccupantChanged { .. }
                | InteractionResolveError::InvalidResponse(_) => {
                    ConversationServiceError::Invalid(error.to_string())
                }
                InteractionResolveError::BridgeUnavailable => {
                    ConversationServiceError::Runtime(error.to_string())
                }
            })
    }

    /// Delegate a pending interaction back to the native Terminal View.
    pub fn delegate_interaction_to_terminal(
        &self,
        conversation_id: &ConversationId,
        interaction_id: &InteractionId,
        expected_revision: u64,
    ) -> Result<InteractionResolution, ConversationServiceError> {
        self.resolve_conversation_interaction(
            conversation_id,
            interaction_id,
            expected_revision,
            InteractionResponse::DelegateToTerminal,
        )
    }

    fn live_source(
        &self,
        session: &AgentSessionInfo,
    ) -> Result<Option<SessionFileRef>, ConversationServiceError> {
        live_session_source(&self.catalog()?, session).map_err(ConversationServiceError::Runtime)
    }

    /// Dispatch a detail query by canonical Conversation id.
    pub fn conversation_detail(
        &self,
        conversation_id: &ConversationId,
        bounds: ConversationWindowBounds,
    ) -> Result<ConversationDetail, ConversationServiceError> {
        let locator = crate::resolve_conversation_id(conversation_id)
            .map_err(|error| ConversationServiceError::Invalid(error.to_string()))?;
        match &locator {
            crate::ConversationLocator::History(key) => self.history_detail(key, bounds),
            // v1 Live ids are an explicitly documented legacy pane alias for
            // READS only; v2 session-exact ids validate the current occupant
            // (R2-07/CR-12) so a stale identity never serves a replacement.
            crate::ConversationLocator::Live(agent_ref) => {
                self.live_detail_with_fingerprint(agent_ref, bounds, None)
            }
            crate::ConversationLocator::LiveSession {
                agent_ref,
                fingerprint,
            } => self.live_detail_with_fingerprint(agent_ref, bounds, Some(fingerprint)),
        }
    }

    /// C34: summary-only live read. The summary needs the agent projection
    /// and the snapshot generation — never the full transcript, so the whole
    /// snapshot normalization the detail path performs (only to slice it
    /// away again) is skipped here.
    fn live_summary_with_fingerprint(
        &self,
        agent_ref: &AgentRef,
        expected_fingerprint: Option<&str>,
    ) -> Result<ConversationSummary, ConversationServiceError> {
        let client = self.client;
        let agent = find_herdr_agent(client, agent_ref.as_str())?;
        let session = agent.agent_session.as_ref().ok_or_else(|| {
            ConversationServiceError::Runtime(
                "agent has no typed conversation identity".to_string(),
            )
        })?;
        if let Some(expected) = expected_fingerprint {
            if crate::session_fingerprint(session) != expected {
                return Err(ConversationServiceError::Invalid(
                    // C26: keep the message on one line — the collapsed
                    // newline used to render as a run of spaces.
                    "conversation identity is stale; the pane now hosts a different session; \
                     reopen the conversation"
                        .to_string(),
                ));
            }
        }
        let snapshot_generation = match self.live_source(session)? {
            Some(source) => match &self.shared_sessions {
                Some(manager) => manager.shared_snapshot(&source)?.generation,
                None => {
                    LiveSession::open(source.clone())
                        .map_err(|error| ConversationServiceError::Runtime(error.to_string()))?
                        .snapshot()
                        .generation
                }
            },
            // Brand-new live session: no transcript indexed yet (generation 0,
            // matching the detail path).
            None => 0,
        };
        live_summary_for_agent(client, &agent, agent_ref, snapshot_generation)
    }

    pub fn conversation_summary(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationSummary, ConversationServiceError> {
        let locator = crate::resolve_conversation_id(conversation_id)
            .map_err(|error| ConversationServiceError::Invalid(error.to_string()))?;
        match &locator {
            crate::ConversationLocator::History(key) => {
                let meta = self.history_session(key)?.ok_or_else(|| {
                    ConversationServiceError::NotFound("conversation not found".to_string())
                })?;
                Ok(history_summary(
                    &meta,
                    crate::project_id_for_path(&meta.project_path),
                ))
            }
            crate::ConversationLocator::Live(agent_ref) => {
                self.live_summary_with_fingerprint(agent_ref, None)
            }
            crate::ConversationLocator::LiveSession {
                agent_ref,
                fingerprint,
            } => self.live_summary_with_fingerprint(agent_ref, Some(fingerprint)),
        }
    }

    pub fn conversation_window(
        &self,
        conversation_id: &ConversationId,
        bounds: ConversationWindowBounds,
    ) -> Result<ConversationWindow, ConversationServiceError> {
        Ok(self.conversation_detail(conversation_id, bounds)?.window)
    }

    /// Semantic prompt against an exact live Agent target. Returns the typed
    /// mutation identity; callers own presentation only. This is the only
    /// production semantic prompt path — clients never call `agent.prompt`
    /// directly.
    pub fn prompt_live_conversation(
        &self,
        agent_ref: &AgentRef,
        text: &str,
    ) -> Result<ConversationMutation, ConversationServiceError> {
        if text.trim().is_empty() {
            return Err(ConversationServiceError::Invalid(
                "text must not be empty".to_string(),
            ));
        }
        let client = self.client;
        // Resolve the exact identity BEFORE the mutation. This gives the
        // commit path a durable fallback identity when the post-prompt
        // projection read is unavailable; a successful `agent.prompt` must
        // never be reported as failed merely because enrichment lost the
        // socket afterwards.
        let before = find_herdr_agent(client, agent_ref.as_str())?;
        let before_session = before.agent_session.as_ref().ok_or_else(|| {
            ConversationServiceError::Runtime(
                "Herdr prompt target has no typed session identity".to_string(),
            )
        })?;
        let provider = agent_provider_kind(before_session).ok_or_else(|| {
            ConversationServiceError::Runtime(format!(
                "unknown Herdr agent provider: {}",
                before_session.agent
            ))
        })?;
        // C13: the committed fallback identity shares the one
        // `continued_identity` constructor instead of hand-assembling the
        // DTO (it also propagates the MetadataOnly failure like every
        // sibling site).
        let committed_identity =
            continued_identity(agent_ref.clone(), provider, before_session, before.revision)
                .map_err(|error| ConversationServiceError::Runtime(error.to_string()))?;
        // Submit confirmation (see agent_launch): the wait keeps herdr's
        // delayed Enter attached to this connection and surfaces a swallowed
        // submit as a typed failure instead of a parked composer.
        client
            .prompt_agent_confirmed(&AgentPromptParams {
                target: agent_ref.to_string(),
                text: text.to_string(),
                wait: Some(crate::herdr::AgentPromptWaitOptions {
                    until: vec![
                        crate::herdr::HerdrAgentStatus::Working,
                        crate::herdr::HerdrAgentStatus::Done,
                        crate::herdr::HerdrAgentStatus::Blocked,
                    ],
                    timeout_ms: Some(15_000),
                }),
            })
            .map_err(ConversationServiceError::Herdr)?;
        // `agent.prompt` is the semantic commit point. Refreshing the
        // projection is useful for a newer revision, but it is post-commit
        // enrichment and therefore best-effort.
        match self.identity_after_prompt(agent_ref) {
            Ok(mutation) => Ok(mutation),
            Err(error) => {
                crate::diagnostics::lag_log(format_args!(
                    "prompt accepted but identity refresh degraded target={} error={error}",
                    agent_ref.as_str()
                ));
                Ok(ConversationMutation {
                    identity: committed_identity,
                    accepted: true,
                })
            }
        }
    }

    /// AC-04: the one authoritative semantic prompt entry for EVERY client
    /// (Remote/Mobile, Desktop-shared callers, History AlreadyLive). It first
    /// proves the exact session occupant behind the ConversationId (AC-03),
    /// then applies the Host disposition policy: idle/done → send now,
    /// working/launch_pending → queue after the turn, blocked/failed → the
    /// Terminal owns the interaction. Presentation layers may hint, but only
    /// this operation decides.
    pub fn submit_conversation_prompt(
        &self,
        conversation_id: &ConversationId,
        delivery: &std::sync::Arc<ConversationDeliveryCoordinator>,
        request_id: &str,
        text: &str,
    ) -> Result<PromptSubmission, ConversationServiceError> {
        if text.trim().is_empty() {
            return Err(ConversationServiceError::Invalid(
                "text must not be empty".to_string(),
            ));
        }
        if request_id.trim().is_empty() {
            return Err(ConversationServiceError::Invalid(
                "request_id must not be empty".to_string(),
            ));
        }
        let normalized_text = text.trim().to_string();
        let locator = crate::resolve_conversation_id(conversation_id)
            .map_err(|error| ConversationServiceError::Invalid(error.to_string()))?;
        let agent_ref = live_agent_ref(&locator).cloned().ok_or_else(|| {
            ConversationServiceError::Invalid("prompt requires a live conversation".to_string())
        })?;

        // P0-03: the Host-level ledger makes the logical request id the
        // idempotency key for EVERY client (Desktop included), not just the
        // Remote HTTP layer. A replay returns the recorded outcome — or the
        // uncertainty tombstone — without re-executing the prompt. The lookup
        // deliberately happens after the Conversation reservation below so
        // lookup+execute+record is one serialized transaction for an exact
        // live target; two same-id callers cannot both observe a miss.
        let ledger_key = format!("prompt:{request_id}");
        let ledger_fingerprint = format!("{conversation_id}|text={normalized_text}");
        // P0-02: the ENTIRE decide→mutate transaction runs under the
        // conversation serialization, and the queued delivery's final
        // recheck→prompt takes the same lock — an immediate submit can never
        // interleave between a queued delivery's sendability check and its
        // prompt (and vice versa).
        let conversation_lock = delivery.conversation_lock(&agent_ref);
        let _serialization = conversation_lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        // The per-Conversation lock above orders source turns.  The ledger
        // admission gate additionally makes lookup→mutation→record one
        // atomic claim for a logical id even if a buggy/legacy caller points
        // that id at another AgentRef concurrently.
        let _ledger_transaction = delivery.ledger().transaction_lock();

        match delivery.ledger().lookup(&ledger_key, &ledger_fingerprint) {
            Err(reason) if reason.starts_with("delivery_uncertain:") => {
                return Err(ConversationServiceError::Herdr(
                    crate::herdr::HerdrError::DeliveryUncertain(
                        reason.trim_start_matches("delivery_uncertain:").to_string(),
                    ),
                ));
            }
            Err(reason) if reason.starts_with("rejected_unknown:") => {
                return Err(ConversationServiceError::Invalid(
                    reason
                        .trim_start_matches("rejected_unknown:")
                        .trim()
                        .to_string(),
                ));
            }
            Err(reason) => {
                return Err(ConversationServiceError::Invalid(reason));
            }
            Ok(Some((disposition, identity))) => {
                return Ok(PromptSubmission {
                    disposition,
                    mutation: identity.map(|identity| ConversationMutation {
                        identity,
                        accepted: true,
                    }),
                    queued: None,
                });
            }
            Ok(None) => {}
        }

        let agent = find_herdr_agent(self.client, agent_ref.as_str())?;
        let session = agent.agent_session.as_ref().ok_or_else(|| {
            ConversationServiceError::Runtime(
                "agent has no typed conversation identity".to_string(),
            )
        })?;
        crate::validate_live_occupant(&locator, session).map_err(|error| {
            ConversationServiceError::Invalid(format!(
                "conversation identity is stale ({error}); reopen the conversation"
            ))
        })?;
        // R3-04: authoritative sendability comes from the Agent-typed SSOT;
        // Unknown is an explicit fail-closed refusal, never a guessed SentNow.
        let disposition = match crate::conversation_queue::prompt_disposition_for_agent(&agent) {
            Ok(disposition) => disposition,
            Err(reason) => {
                delivery.ledger().record_rejected_unknown(
                    ledger_key.clone(),
                    ledger_fingerprint.clone(),
                    reason,
                );
                return Err(ConversationServiceError::Invalid(reason.to_string()));
            }
        };
        match disposition {
            PromptDisposition::NeedsTerminal => {
                delivery
                    .ledger()
                    .record_needs_terminal(ledger_key, ledger_fingerprint);
                Ok(PromptSubmission {
                    disposition: PromptDisposition::NeedsTerminal,
                    mutation: None,
                    queued: None,
                })
            }
            PromptDisposition::QueuedAfterTurn => {
                let provider = agent_provider_label(&agent, session);
                // C08: fail closed exactly like the sibling sites — a
                // metadata-only session must never enter the queue only to
                // be rejected by the delivery-time identity revalidation.
                let native_session_id = public_native_session_id(session)
                    .map_err(|error| ConversationServiceError::Runtime(error.to_string()))?;
                let item = QueuedFollowUp {
                    request_id: request_id.to_string(),
                    conversation_id: conversation_id.clone(),
                    agent_ref: agent_ref.clone(),
                    provider,
                    native_session_id,
                    session_fingerprint: session_fingerprint(session),
                    baseline_revision: agent.revision,
                    text: normalized_text.clone(),
                    created_at: std::time::SystemTime::now(),
                    state: QueueState::Queued,
                };
                let mut snapshot = item.clone();
                snapshot.state = delivery
                    .queue()
                    .enqueue(item)
                    .map_err(|error| ConversationServiceError::Invalid(error.to_string()))?;
                delivery.schedule(&agent_ref);
                delivery.ledger().record_accepted(
                    ledger_key,
                    ledger_fingerprint,
                    PromptDisposition::QueuedAfterTurn,
                    ConversationIdentity {
                        conversation_id: conversation_id.clone(),
                        agent_ref: agent_ref.clone(),
                        provider: snapshot.provider.clone(),
                        native_session_id: snapshot.native_session_id.clone(),
                        revision: snapshot.baseline_revision,
                    },
                );
                Ok(PromptSubmission {
                    disposition: PromptDisposition::QueuedAfterTurn,
                    mutation: None,
                    queued: Some(snapshot),
                })
            }
            PromptDisposition::SentNow => {
                // R7-P0-01: before allowing an immediate send, check whether
                // an older accepted operation already exists for this exact
                // Conversation.  A mutex is not a FIFO sequencer: winning the
                // Conversation lock does NOT mean no earlier operation was
                // already accepted.  If an unresolved queue item exists, the
                // newer immediate Prompt must NOT overtake it.
                if delivery
                    .queue()
                    .has_unresolved_for_conversation(conversation_id)
                {
                    return Err(ConversationServiceError::Invalid(
                        "a follow-up is already queued for this conversation; \
                         deliver or cancel it before sending a new message"
                            .to_string(),
                    ));
                }
                match self.prompt_live_conversation(&agent_ref, &normalized_text) {
                    Ok(mutation) => {
                        delivery.ledger().record_accepted(
                            ledger_key,
                            ledger_fingerprint,
                            PromptDisposition::SentNow,
                            mutation.identity.clone(),
                        );
                        Ok(PromptSubmission {
                            disposition: PromptDisposition::SentNow,
                            mutation: Some(mutation),
                            queued: None,
                        })
                    }
                    Err(ConversationServiceError::Herdr(
                        crate::herdr::HerdrError::DeliveryUncertain(reason),
                    )) => {
                        // P0-03: tombstone the uncertainty at the HOST layer —
                        // the same logical id can never re-execute the prompt.
                        delivery
                            .ledger()
                            .record_uncertain(ledger_key, ledger_fingerprint, &reason);
                        Err(ConversationServiceError::Herdr(
                            crate::herdr::HerdrError::DeliveryUncertain(reason),
                        ))
                    }
                    Err(error) => Err(error),
                }
            }
        }
    }

    fn identity_after_prompt(
        &self,
        agent_ref: &AgentRef,
    ) -> Result<ConversationMutation, ConversationServiceError> {
        let agent = find_herdr_agent(self.client, agent_ref.as_str())?;
        let session = agent.agent_session.as_ref().ok_or_else(|| {
            ConversationServiceError::Runtime(
                "Herdr prompt returned no typed session identity".to_string(),
            )
        })?;
        let provider = agent_provider_kind(session).ok_or_else(|| {
            ConversationServiceError::Runtime(format!(
                "unknown Herdr agent provider: {}",
                session.agent
            ))
        })?;
        // C13: the same shared identity constructor as the committed
        // fallback; the v2 conversation id is derived from the post-prompt
        // session, which is the occupant this identity actually names.
        let identity = continued_identity(agent_ref.clone(), provider, session, agent.revision)
            .map_err(|error| ConversationServiceError::Runtime(error.to_string()))?;
        Ok(ConversationMutation {
            identity,
            accepted: true,
        })
    }

    /// Host-owned History Continue: canonical planner (AlreadyLive →
    /// NativeResume → ContextTransfer → NeedsProjectSelection → Unsupported)
    /// executed through the M2 launch transaction and the M5 transfer engine.
    pub fn continue_conversation<P: crate::agent_launch::ProjectPreparation>(
        &self,
        preparation: &P,
        artifact_store: &shardlane_history::TransferArtifactStore,
        transfer_limits: &shardlane_history::TransferLimits,
        delivery: &std::sync::Arc<ConversationDeliveryCoordinator>,
        request: &crate::history_continuation::ContinuationRequest,
    ) -> Result<crate::history_continuation::ContinuationResult, ConversationServiceError> {
        let catalog = self.catalog()?;
        let deps = crate::history_continuation::ContinuationDeps {
            runtime: self.client,
            preparation,
            artifact_store,
            transfer_limits,
            delivery,
        };
        let result = crate::history_continuation::continue_history_conversation(
            self.client,
            &catalog,
            &deps,
            request,
        )?;
        // New Agent launches may intentionally degrade to a Terminal when no
        // semantic session binding exists. History Continue has a stronger
        // contract: a successful mutation must return the Live Conversation
        // identity so every client can converge on ConversationSurface.
        if let crate::history_continuation::ContinuationResult::Launched { outcome, .. } = &result {
            require_live_identity(outcome.identity.as_ref())?;
        }
        Ok(result)
    }
}

/// AC-04/AC-17: the single semantic prompt executor. Every surface — the
/// authoritative `submit_conversation_prompt`, the legacy v1 Agent endpoint,
/// and `HostAgentService::prompt_agent` — routes through this function so no
/// second semantic-prompt implementation can drift from the AC-02 error
/// policy or the exactly-once submit confirmation. The prompt path never
/// opens the history catalog, so the service is constructed with an empty db
/// path.
pub fn prompt_live_agent(
    client: &HerdrClient,
    agent_ref: &AgentRef,
    text: &str,
) -> Result<ConversationMutation, ConversationServiceError> {
    HostConversationService::new(client, std::path::PathBuf::new())
        .prompt_live_conversation(agent_ref, text)
}

fn require_live_identity(
    identity: Option<&ConversationIdentity>,
) -> Result<(), ConversationServiceError> {
    identity.map(|_| ()).ok_or_else(|| {
        ConversationServiceError::Runtime(
            "History Continue launched an Agent without a Live Conversation identity".to_string(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_bounds_are_small_and_deterministic() {
        assert_eq!(clamp_conversation_window(0), 1);
        assert_eq!(clamp_conversation_window(999), MAX_CONVERSATION_WINDOW);
        assert_eq!(
            clamp_conversation_window(DEFAULT_CONVERSATION_WINDOW),
            DEFAULT_CONVERSATION_WINDOW
        );
        let (before, after) = window_half(ConversationWindowBounds::default());
        assert_eq!((before, after), (80, 80));
        let (before, after) = window_half(ConversationWindowBounds {
            anchor_seq: None,
            before: Some(0),
            after: Some(999),
        });
        assert_eq!((before, after), (1, 200));
    }

    #[test]
    fn empty_prompt_text_is_rejected_as_invalid_not_runtime() {
        let error = ConversationServiceError::Invalid("text must not be empty".into());
        assert!(matches!(error, ConversationServiceError::Invalid(_)));
        assert_eq!(error.to_string(), "text must not be empty");
    }

    #[test]
    fn history_continue_requires_a_live_identity_after_launch() {
        let error = match require_live_identity(None) {
            Ok(()) => panic!("missing identity must fail closed"),
            Err(error) => error,
        };
        assert_eq!(
            error.to_string(),
            "History Continue launched an Agent without a Live Conversation identity"
        );
        let identity = ConversationIdentity {
            conversation_id: ConversationId::new("conv_live"),
            agent_ref: AgentRef::new("pane-1"),
            provider: "claude".into(),
            native_session_id: None,
            revision: 1,
        };
        assert!(require_live_identity(Some(&identity)).is_ok());
    }

    // --- C32/C33/C34: live summary/detail projections over a scripted Herdr
    // socket and a real (tiny) Claude transcript file. ---

    /// One-shot scripted Herdr socket: each accepted connection consumes the
    /// next response line.
    fn scripted_socket(responses: Vec<String>) -> std::path::PathBuf {
        let socket_path = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("tempdir: {error}"))
            .keep()
            .join("conv-service.sock");
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
                let _ = writeln!(stream, "{response}");
                let _ = stream.flush();
            }
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !socket_path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        socket_path
    }

    fn agent_list_response(agent_json: &str) -> String {
        format!(r#"{{"id":"","result":{{"type":"agent_list","agents":[{agent_json}]}}}}"#)
    }

    fn path_backed_claude_agent(
        transcript_path: &std::path::Path,
        workspace_id: Option<&str>,
        cwd: Option<&std::path::Path>,
    ) -> String {
        let transcript = transcript_path.to_string_lossy();
        let workspace = workspace_id
            .map(|id| format!(r#""workspace_id":"{id}","#))
            .unwrap_or_default();
        let cwd = cwd
            .map(|path| {
                format!(
                    r#""cwd":"{}","#,
                    path.to_string_lossy().replace('\\', "\\\\")
                )
            })
            .unwrap_or_default();
        format!(
            r#"{{"terminal_id":"term-1","pane_id":"pane-1",{workspace}{cwd}"agent_status":"idle","interactive_ready":true,"revision":7,"agent_session":{{"agent":"claude","kind":"path","source":"herdr:claude","value":"{transcript}"}}}}"#
        )
    }

    fn write_claude_transcript(path: &std::path::Path) {
        std::fs::write(
            path,
            concat!(
                r#"{"type":"user","timestamp":"2026-08-01T01:00:00Z","message":{"content":"hello"}}"#,
                "\n",
                r#"{"type":"assistant","timestamp":"2026-08-01T01:00:01Z","message":{"id":"m1","content":[{"type":"text","text":"hi"}]}}"#,
                "\n",
            ),
        )
        .unwrap_or_else(|error| panic!("write transcript: {error}"));
    }

    #[test]
    fn live_summary_matches_the_detail_projection_and_skips_normalization() {
        // C34: the summary-only path must return byte-identical summary
        // fields without normalizing the whole transcript (the detail path
        // keeps that). C33: the ProjectId derives from the agent's own
        // workspace id with zero extra RPCs.
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let transcript = temp.path().join("claude.jsonl");
        write_claude_transcript(&transcript);
        let agent = path_backed_claude_agent(&transcript, Some("w-live"), None);
        // One agent.list for the summary read, one for the detail read.
        let socket = scripted_socket(vec![
            agent_list_response(&agent),
            agent_list_response(&agent),
        ]);
        let client = crate::herdr::HerdrClient::for_test_socket(socket);
        let service = HostConversationService::new(&client, temp.path().join("history.db"));

        let agent_ref = AgentRef::new("pane-1");
        let v1_id = crate::conversations::conversation_id_for_live_agent(&agent_ref);
        let summary = service
            .conversation_summary(&v1_id)
            .unwrap_or_else(|error| panic!("summary: {error}"));
        let detail = service
            .live_detail(&agent_ref, ConversationWindowBounds::default())
            .unwrap_or_else(|error| panic!("detail: {error}"));

        assert_eq!(summary, detail.conversation, "C34: identical summaries");
        assert_eq!(
            summary.project_id,
            crate::project_id_for_runtime_workspace("w-live"),
            "C33: project derives from the agent's own workspace id"
        );
        assert_eq!(summary.source, crate::dto::ConversationSource::Live);
        assert_eq!(detail.window.items.len(), 2, "detail keeps its window");
    }

    #[test]
    fn live_summary_falls_back_to_the_project_path_only_without_workspace_id() {
        // C33 miss path: no workspace id → the discovered-path phase of the
        // same ProjectIndex mapping, over the narrowed workspace_state
        // projection (4 RPCs: workspace.list/tab.list/pane.list/agent.list).
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let transcript = temp.path().join("claude.jsonl");
        write_claude_transcript(&transcript);
        let agent = path_backed_claude_agent(&transcript, None, Some(temp.path()));
        let workspace_list = format!(
            r#"{{"id":"","result":{{"type":"workspace_list","workspaces":[{{"workspace_id":"w1","cwd":"{}"}}]}}}}"#,
            temp.path().to_string_lossy()
        );
        let socket = scripted_socket(vec![
            agent_list_response(&agent),
            workspace_list,
            r#"{"id":"","result":{"type":"tab_list","tabs":[]}}"#.into(),
            r#"{"id":"","result":{"type":"pane_list","panes":[]}}"#.into(),
            agent_list_response(&agent),
        ]);
        let client = crate::herdr::HerdrClient::for_test_socket(socket);
        let service = HostConversationService::new(&client, temp.path().join("history.db"));

        let agent_ref = AgentRef::new("pane-1");
        let v1_id = crate::conversations::conversation_id_for_live_agent(&agent_ref);
        let summary = service
            .conversation_summary(&v1_id)
            .unwrap_or_else(|error| panic!("summary: {error}"));
        assert_eq!(
            summary.project_id,
            crate::project_id_for_runtime_workspace("w1"),
            "the discovered path attaches the live agent to the matching runtime project"
        );
    }

    #[test]
    fn live_summary_without_workspace_id_or_path_fails_closed() {
        // C33: an agent with no workspace id and no discoverable path keeps
        // the previous fail-closed behavior instead of a guessed project.
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let transcript = temp.path().join("claude.jsonl");
        write_claude_transcript(&transcript);
        let agent = path_backed_claude_agent(&transcript, None, None);
        let socket = scripted_socket(vec![
            agent_list_response(&agent),
            // workspace_state is consulted before the missing-path refusal.
            r#"{"id":"","result":{"type":"workspace_list","workspaces":[]}}"#.into(),
            r#"{"id":"","result":{"type":"tab_list","tabs":[]}}"#.into(),
            r#"{"id":"","result":{"type":"pane_list","panes":[]}}"#.into(),
            agent_list_response(&agent),
        ]);
        let client = crate::herdr::HerdrClient::for_test_socket(socket);
        let service = HostConversationService::new(&client, temp.path().join("history.db"));

        let agent_ref = AgentRef::new("pane-1");
        let v1_id = crate::conversations::conversation_id_for_live_agent(&agent_ref);
        let error = match service.conversation_summary(&v1_id) {
            Ok(_) => panic!("unresolvable project must fail closed"),
            Err(error) => error,
        };
        assert!(
            matches!(error, ConversationServiceError::Runtime(ref message) if message.contains("could not be resolved")),
            "{error:?}"
        );
    }

    #[test]
    fn live_summary_rejects_a_stale_v2_identity_like_the_detail_path() {
        // C34/R2-07: the summary-only path keeps the exact-occupant
        // validation — a v2 id pointing at a replaced session fails closed
        // before any transcript read.
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let stale_transcript = temp.path().join("claude.jsonl");
        let live_transcript = temp.path().join("replacement.jsonl");
        write_claude_transcript(&stale_transcript);
        write_claude_transcript(&live_transcript);
        let path_session = |path: &std::path::Path| crate::herdr::AgentSessionInfo {
            agent: "claude".into(),
            kind: "path".into(),
            source: "herdr:claude".into(),
            value: path.to_string_lossy().to_string(),
        };
        let live_agent = path_backed_claude_agent(&live_transcript, Some("w-live"), None);
        let socket = scripted_socket(vec![agent_list_response(&live_agent)]);
        let client = crate::herdr::HerdrClient::for_test_socket(socket);
        let service = HostConversationService::new(&client, temp.path().join("history.db"));

        let agent_ref = AgentRef::new("pane-1");
        let stale_id = crate::conversations::conversation_id_for_live_session(
            &agent_ref,
            &path_session(&stale_transcript),
        );
        assert_ne!(
            crate::session_fingerprint(&path_session(&stale_transcript)),
            crate::session_fingerprint(&path_session(&live_transcript)),
            "the two occupants must be distinguishable"
        );
        let error = match service.conversation_summary(&stale_id) {
            Ok(_) => panic!("stale v2 id must fail closed"),
            Err(error) => error,
        };
        assert!(
            matches!(error, ConversationServiceError::Invalid(ref message) if message.contains("stale")),
            "{error:?}"
        );
    }
}
