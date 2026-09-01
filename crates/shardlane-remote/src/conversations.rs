//! Remote API v2 semantic Conversation surface.
//!
//! [INPUT]: Authenticated v2 HTTP requests carrying explicit opaque
//! Project/Agent/Conversation IDs.
//! [OUTPUT]: Bounded Live/History Conversation summaries/windows plus Host-owned
//! prompt and Continue mutations, encoded over the wire.
//! [POS]: audit AF-01/AF-04/AF-06 — this module is only an adapter layer of
//! decode → Host service call → encode. Provider resume/workspace/start/
//! readiness policy, live semantic source resolution, and the exactly-once
//! prompt transaction all live in `shardlane-host::HostConversationService`;
//! nothing of them may be re-owned here.

use crate::bootstrap::map_bootstrap_error;
use crate::error::ApiError;
use crate::server::map_herdr_error;
use crate::state::RemoteState;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use shardlane_host::herdr::HerdrClient;
use shardlane_host::{
    AgentRef, ConversationDetail, ConversationLocator, ConversationPage, ConversationServiceError,
    ConversationWindow, ConversationWindowBounds, HistoryConversationService,
    HostConversationService, ProjectId,
};
use std::collections::HashSet;
use std::sync::Arc;

const MAX_HISTORY_LIST: usize = 200;

#[derive(Clone, Debug, Deserialize)]
pub struct ConversationListQuery {
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConversationSearchQuery {
    pub q: String,
    pub project_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConversationWindowQuery {
    pub anchor_seq: Option<u64>,
    pub before: Option<u32>,
    pub after: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConversationPromptBody {
    pub text: String,
    pub request_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ConversationContinueBody {
    /// Optional continuation instruction. Empty is a valid Continue.
    #[serde(default)]
    pub text: Option<String>,
    pub request_id: Option<String>,
    pub provider: Option<String>,
    /// Project path override once the client resolved a picker choice.
    #[serde(default)]
    pub project_path: Option<String>,
}

/// M6 continuation result: strategy + exact identities for client navigation.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ConversationContinueResponse {
    pub accepted: bool,
    pub strategy: shardlane_host::ContinuationStrategy,
    /// A successful Continue always returns the exact Live Conversation identity.
    pub identity: shardlane_host::ConversationIdentity,
    pub instruction: Option<shardlane_host::PromptDisposition>,
    /// Runtime structure created for NativeResume/ContextTransfer targets.
    pub launched: Option<LaunchedTarget>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct LaunchedTarget {
    pub workspace_id: String,
    pub tab_id: String,
    pub pane_id: String,
}

fn envelope_id(state: &RemoteState, client_id: Option<&str>) -> String {
    client_id
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| state.next_request_id())
}

fn clamp_limit(value: Option<usize>, default: usize, max: usize) -> usize {
    value.unwrap_or(default).clamp(1, max)
}

impl From<&ConversationWindowQuery> for ConversationWindowBounds {
    fn from(query: &ConversationWindowQuery) -> Self {
        Self {
            anchor_seq: query.anchor_seq,
            before: query.before,
            after: query.after,
        }
    }
}

/// Host Conversation errors → the wire envelopes. The adapter owns only the
/// transport mapping; message content comes from the Host service.
///
/// D05: "not found" is classified only through the typed
/// [`ConversationServiceError::NotFound`] variant. The former
/// `Runtime(message) if message.contains("not found")` substring arm was
/// removed: no Host production path produces a Runtime error carrying that
/// substring (missing history conversations come back as the typed
/// NotFound variant), so the arm could only misclassify an unrelated
/// internal failure as 404 when its text happened to contain the words.
pub(crate) fn map_conversation_error(
    error: ConversationServiceError,
    request_id: String,
) -> ApiError {
    match error {
        ConversationServiceError::Invalid(message) => {
            ApiError::invalid_request(message, request_id)
        }
        ConversationServiceError::NotFound(message) => ApiError::not_found(message, request_id),
        ConversationServiceError::Herdr(error) => map_herdr_error(error, request_id),
        ConversationServiceError::Runtime(message) => {
            ApiError::runtime_unavailable(message, request_id)
        }
        ConversationServiceError::NeedsProject(message) => {
            ApiError::needs_project_selection(message, request_id)
        }
        ConversationServiceError::AgentCreated {
            agent_ref,
            tab_id,
            pane_id,
            phase,
            detail,
        } => {
            let target = crate::error::AgentCreatedTarget {
                agent_ref: agent_ref.as_str().to_string(),
                tab_id: tab_id.clone(),
                pane_id: pane_id.clone(),
                phase: format!("{phase:?}"),
            };
            ApiError::agent_created_with_target(
                format!(
                    "agent created: agent_ref={}, tab_id={tab_id}, pane_id={pane_id}, phase={phase:?}; {detail}",
                    agent_ref.as_str()
                ),
                request_id,
                target,
            )
        }
    }
}

fn conversation_client(
    state: &RemoteState,
    instance: Option<&str>,
) -> Result<HerdrClient, ConversationServiceError> {
    crate::bootstrap::connect_herdr_for(state, instance).map_err(ConversationServiceError::Herdr)
}

/// Build the Host Conversation service, sharing the process live session
/// manager when the GUI injected one (AF-05: one semantic session owner).
fn host_conversation_service<'a>(
    state: &'a RemoteState,
    client: &'a HerdrClient,
) -> HostConversationService<'a> {
    let service = HostConversationService::new(client, state.history_db_path());
    match state.conversation_sessions.clone() {
        Some(manager) => service.with_shared_sessions(manager),
        None => service,
    }
}

pub async fn project_conversations(
    State(state): State<Arc<RemoteState>>,
    Path(project_id): Path<String>,
    Query(query): Query<ConversationListQuery>,
) -> Response {
    let request_id = state.next_request_id();
    let project_id = ProjectId::new(project_id);
    if shardlane_host::resolve_project_id(&project_id).is_err() {
        return ApiError::invalid_request("invalid project_id", request_id).into_response();
    }
    let limit = clamp_limit(query.limit, 50, MAX_HISTORY_LIST);
    let work_state = state.clone();
    let error_request_id = request_id.clone();
    let instance = crate::bootstrap::current_instance_scope();
    let result = tokio::task::spawn_blocking(move || -> Result<ConversationPage, ApiError> {
        let bootstrap = crate::bootstrap::build_bootstrap_for(&work_state, instance.as_deref())
            .map_err(|error| map_bootstrap_error(error, error_request_id.clone()))?;
        let project = bootstrap
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .ok_or_else(|| ApiError::not_found("unknown project", error_request_id.clone()))?;
        let mut conversations = bootstrap
            .conversations
            .iter()
            .filter(|conversation| conversation.project_id == project.id)
            .cloned()
            .collect::<Vec<_>>();
        if let Some(path) = project.project_path.as_deref() {
            // History is an additive projection. A read-only/uninitialized Host
            // may have no catalog yet; live Herdr Conversations remain usable
            // and the list endpoint still returns a valid bounded page.
            // D14: the catalog handle is shared per-process (opened lazily).
            if let Ok(catalog) = work_state.history_catalog() {
                if let Some(catalog) = catalog.as_ref() {
                    let history = HistoryConversationService::new(catalog);
                    conversations.extend(
                        history
                            .list_for_project(path, project.id.clone(), limit)
                            .map_err(|error| ApiError::internal(error, error_request_id.clone()))?,
                    );
                }
            }
        }
        let mut seen = HashSet::new();
        conversations.retain(|conversation| seen.insert(conversation.id.clone()));
        conversations.truncate(limit);
        Ok(ConversationPage {
            conversations,
            next_cursor: None,
        })
    })
    .await;
    match result {
        Ok(Ok(page)) => Json(page).into_response(),
        Ok(Err(error)) => error.into_response(),
        Err(error) => ApiError::internal(format!("conversation list failed: {error}"), request_id)
            .into_response(),
    }
}

pub async fn search_conversations(
    State(state): State<Arc<RemoteState>>,
    Query(query): Query<ConversationSearchQuery>,
) -> Response {
    let request_id = state.next_request_id();
    if query.q.trim().is_empty() {
        return Json(ConversationPage {
            conversations: Vec::new(),
            next_cursor: None,
        })
        .into_response();
    }
    let limit = clamp_limit(query.limit, 30, MAX_HISTORY_LIST);
    let work_state = state.clone();
    let error_request_id = request_id.clone();
    let instance = crate::bootstrap::current_instance_scope();
    let result = tokio::task::spawn_blocking(move || -> Result<ConversationPage, ApiError> {
        // D04: the bootstrap projection (full runtime pull + config read +
        // ProjectIndex rebuild) is required only to resolve an explicit
        // project_id. A global search — the common case — must not build it
        // just to drop the result (and must not fail when only the Herdr
        // runtime is unreachable but the local history catalog is usable).
        let (project_paths, project_id) = if let Some(project_id) = query.project_id.as_deref() {
            let bootstrap = crate::bootstrap::build_bootstrap_for(&work_state, instance.as_deref())
                .map_err(|error| map_bootstrap_error(error, error_request_id.clone()))?;
            let project_id = ProjectId::new(project_id);
            let project = bootstrap
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .ok_or_else(|| ApiError::not_found("unknown project", error_request_id.clone()))?;
            (
                project.project_path.clone().into_iter().collect::<Vec<_>>(),
                Some(project.id.clone()),
            )
        } else {
            (Vec::new(), None)
        };
        // D14: the shared per-process catalog handle (open failure → internal;
        // failures are not cached, the next request retries).
        let catalog_guard = work_state
            .history_catalog()
            .map_err(|error| ApiError::internal(error, error_request_id.clone()))?;
        let history = match catalog_guard.as_ref() {
            Some(catalog) => HistoryConversationService::new(catalog),
            None => {
                return Err(ApiError::internal(
                    "shared history catalog unavailable".to_string(),
                    error_request_id.clone(),
                ))
            }
        };
        Ok(ConversationPage {
            conversations: history
                .search(&query.q, &project_paths, project_id, limit)
                .map_err(|error| ApiError::internal(error, error_request_id.clone()))?,
            next_cursor: None,
        })
    })
    .await;
    match result {
        Ok(Ok(page)) => Json(page).into_response(),
        Ok(Err(error)) => error.into_response(),
        Err(error) => {
            ApiError::internal(format!("conversation search failed: {error}"), request_id)
                .into_response()
        }
    }
}

pub async fn agent_conversation(
    State(state): State<Arc<RemoteState>>,
    Path(agent_ref): Path<String>,
    Query(query): Query<ConversationWindowQuery>,
) -> Response {
    let request_id = state.next_request_id();
    if agent_ref.trim().is_empty() {
        return ApiError::invalid_request("agent_ref must not be empty", request_id)
            .into_response();
    }
    let work_state = state.clone();
    let instance = crate::bootstrap::current_instance_scope();
    let result = tokio::task::spawn_blocking(
        move || -> Result<ConversationDetail, ConversationServiceError> {
            let client = conversation_client(&work_state, instance.as_deref())?;
            let service = host_conversation_service(&work_state, &client);
            service.live_detail(
                &AgentRef::new(agent_ref),
                ConversationWindowBounds::from(&query),
            )
        },
    )
    .await;
    match result {
        Ok(Ok(detail)) => Json(detail).into_response(),
        Ok(Err(error)) => map_conversation_error(error, request_id).into_response(),
        Err(error) => ApiError::internal(format!("live conversation failed: {error}"), request_id)
            .into_response(),
    }
}

pub async fn get_conversation(
    State(state): State<Arc<RemoteState>>,
    Path(conversation_id): Path<String>,
) -> Response {
    let request_id = state.next_request_id();
    let id = shardlane_host::ConversationId::new(conversation_id);
    let work_state = state.clone();
    let instance = crate::bootstrap::current_instance_scope();
    let result = tokio::task::spawn_blocking(
        move || -> Result<shardlane_host::ConversationSummary, ConversationServiceError> {
            let client = conversation_client(&work_state, instance.as_deref())?;
            let service = host_conversation_service(&work_state, &client);
            service.conversation_summary(&id)
        },
    )
    .await;
    match result {
        Ok(Ok(summary)) => Json(summary).into_response(),
        Ok(Err(error)) => map_conversation_error(error, request_id).into_response(),
        Err(error) => {
            ApiError::internal(format!("conversation lookup failed: {error}"), request_id)
                .into_response()
        }
    }
}

pub async fn conversation_window(
    State(state): State<Arc<RemoteState>>,
    Path(conversation_id): Path<String>,
    Query(query): Query<ConversationWindowQuery>,
) -> Response {
    let request_id = state.next_request_id();
    let id = shardlane_host::ConversationId::new(conversation_id);
    let work_state = state.clone();
    let instance = crate::bootstrap::current_instance_scope();
    let result = tokio::task::spawn_blocking(
        move || -> Result<ConversationWindow, ConversationServiceError> {
            let client = conversation_client(&work_state, instance.as_deref())?;
            let service = host_conversation_service(&work_state, &client);
            service.conversation_window(&id, ConversationWindowBounds::from(&query))
        },
    )
    .await;
    match result {
        Ok(Ok(window)) => Json(window).into_response(),
        Ok(Err(error)) => map_conversation_error(error, request_id).into_response(),
        Err(error) => {
            ApiError::internal(format!("conversation window failed: {error}"), request_id)
                .into_response()
        }
    }
}

pub async fn prompt_conversation(
    State(state): State<Arc<RemoteState>>,
    Path(conversation_id): Path<String>,
    body: Result<Json<ConversationPromptBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    // D01: shared body decode → unified ApiError envelope.
    let body = match crate::server::decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    // D11: one id throughout. `envelope_id` already yields the client's
    // logical id when supplied (else a fresh transport envelope id), so the
    // former id/logical_request_id/host_operation_id triple was the same
    // string computed three times.
    let id = envelope_id(&state, body.request_id.as_deref());
    let conversation_id = shardlane_host::ConversationId::new(conversation_id);
    if let Err(error) = shardlane_host::resolve_conversation_id(&conversation_id) {
        return ApiError::invalid_request(error.to_string(), id.clone()).into_response();
    }
    let work_state = state.clone();
    let text = body.text;
    // A03: the client-supplied request_id (+ operation) is the idempotency key.
    // The bearer token is the authenticated client-identity boundary; exact
    // replays return the first response, concurrent duplicates coalesce, and
    // the underlying Host mutation never re-executes inside the TTL window.
    let idempotency_key = format!("prompt:{id}");
    // R2-10: the idempotency entry binds the exact mutation shape (target +
    // canonical body, excluding request_id). A reused id with a different
    // body conflicts instead of replaying the old result.
    let idempotency_fingerprint = format!("prompt|{conversation_id}|text={}", text.trim());
    let error_id = id.clone();
    let host_operation_id_for_host = id.clone();
    let outcome = state
        .mutations
        .execute(idempotency_key, idempotency_fingerprint, || {
            let work_state = work_state.clone();
            let conversation_id = conversation_id.clone();
            let text = text.clone();
            let error_id = error_id.clone();
            async move {
                let instance = crate::bootstrap::current_instance_scope();
                let result = tokio::task::spawn_blocking(
                    move || -> Result<ConversationPromptResponse, ConversationServiceError> {
                        let client = conversation_client(&work_state, instance.as_deref())?;
                        let service = host_conversation_service(&work_state, &client);
                        let submission = service.submit_conversation_prompt(
                            &conversation_id,
                            &work_state.delivery,
                            &host_operation_id_for_host,
                            &text,
                        )?;
                        // NeedsTerminal is a truthful non-acceptance: the text was
                        // NOT sent or queued. Surface it as a typed terminal error
                        // so clients keep the draft and route the user to the
                        // explicit Terminal surface.
                        if submission.disposition
                            == shardlane_host::PromptDisposition::NeedsTerminal
                        {
                            return Err(ConversationServiceError::Invalid(
                                "the agent is blocked; resolve it in the Terminal first"
                                    .to_string(),
                            ));
                        }
                        Ok(ConversationPromptResponse::from(submission))
                    },
                )
                .await
                .map_err(|error| ConversationServiceError::Runtime(format!("join: {error}")));
                match result {
                    Ok(Ok(response)) => serde_json::to_value(response).map_err(|error| {
                        ApiError::internal(format!("serialize: {error}"), error_id)
                    }),
                    Ok(Err(error)) => Err(map_conversation_error(error, error_id)),
                    Err(join_error) => Err(map_conversation_error(join_error, error_id)),
                }
            }
        })
        .await;
    let mut response = match outcome {
        Ok(value) => Json(value).into_response(),
        Err(error) => error.into_response(),
    };
    if let Ok(value) = axum::http::HeaderValue::from_str(&id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

/// AC-04 wire shape: disposition is authoritative Host output. `identity` is
/// present for SentNow and QueuedAfterTurn; NeedsTerminal is surfaced as a
/// terminal error envelope instead.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ConversationPromptResponse {
    pub identity: Option<shardlane_host::ConversationIdentity>,
    pub accepted: bool,
    pub disposition: shardlane_host::PromptDisposition,
}

impl From<shardlane_host::PromptSubmission> for ConversationPromptResponse {
    fn from(submission: shardlane_host::PromptSubmission) -> Self {
        match submission.disposition {
            shardlane_host::PromptDisposition::SentNow => Self {
                identity: submission.mutation.map(|mutation| mutation.identity),
                accepted: true,
                disposition: shardlane_host::PromptDisposition::SentNow,
            },
            shardlane_host::PromptDisposition::QueuedAfterTurn => Self {
                identity: submission
                    .queued
                    .map(|item| shardlane_host::ConversationIdentity {
                        conversation_id: item.conversation_id,
                        agent_ref: item.agent_ref,
                        provider: item.provider,
                        native_session_id: item.native_session_id,
                        revision: item.baseline_revision,
                    })
                    .or_else(|| submission.mutation.map(|mutation| mutation.identity)),
                accepted: true,
                disposition: shardlane_host::PromptDisposition::QueuedAfterTurn,
            },
            shardlane_host::PromptDisposition::NeedsTerminal => Self {
                identity: None,
                accepted: false,
                disposition: shardlane_host::PromptDisposition::NeedsTerminal,
            },
        }
    }
}

pub async fn continue_conversation(
    State(state): State<Arc<RemoteState>>,
    Path(conversation_id): Path<String>,
    body: Result<Json<ConversationContinueBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    // D01: shared body decode → unified ApiError envelope.
    let body = match crate::server::decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    // D11: one id throughout (see prompt_conversation).
    let id = envelope_id(&state, body.request_id.as_deref());
    let conversation_id = shardlane_host::ConversationId::new(conversation_id);
    let Ok(ConversationLocator::History(_key)) =
        shardlane_host::resolve_conversation_id(&conversation_id)
    else {
        return ApiError::invalid_request("continue requires a history conversation", id)
            .into_response();
    };
    let work_state = state.clone();
    let provider = body.provider;
    let instruction = body.text;
    let project_override = body.project_path;
    // A03: Continue is the highest-risk duplicate (runtime/worktree/Agent
    // creation); the same request_id must never launch twice.
    let idempotency_key = format!("continue:{id}");
    let idempotency_fingerprint = format!(
        "continue|{conversation_id}|provider={}|project={}|text={}",
        provider.as_deref().unwrap_or_default(),
        project_override.as_deref().unwrap_or_default(),
        instruction.as_deref().map(str::trim).unwrap_or_default(),
    );
    let error_id = id.clone();
    let logical_request_id = id.clone();
    let result = state
        .mutations
        .execute(idempotency_key, idempotency_fingerprint, || {
            let work_state = work_state.clone();
            let conversation_id = conversation_id.clone();
            let provider = provider.clone();
            let instruction = instruction.clone();
            let project_override = project_override.clone();
            let error_id = error_id.clone();
            async move {
                let instance = crate::bootstrap::current_instance_scope();
                let executed = tokio::task::spawn_blocking(
                    move || -> Result<ConversationContinueResponse, ConversationServiceError> {
                        let client = conversation_client(&work_state, instance.as_deref())?;
                        let service = host_conversation_service(&work_state, &client);
                        let app_data = work_state
                            .settings_path
                            .parent()
                            .map(|parent| parent.to_path_buf())
                            .unwrap_or_else(|| std::path::PathBuf::from("."));
                        let preparation =
                            shardlane_host::GitWorktreePreparation::new(app_data.join("worktrees"));
                        let artifact_store = shardlane_history::TransferArtifactStore::new(
                            app_data.join("transfer-artifacts"),
                        );
                        let transfer_limits = shardlane_history::TransferLimits::default();
                        let request = shardlane_host::ContinuationRequest {
                            operation_id: logical_request_id.clone(),
                            conversation_id,
                            target_provider: provider
                                .as_deref()
                                .and_then(shardlane_history::AgentId::from_slug),
                            instruction,
                            project_override,
                        };
                        // AC-06: the Host transaction now owns queued delivery
                        // internally, so no client needs to drive the queue
                        // after ReusedLive.
                        match service.continue_conversation(
                            &preparation,
                            &artifact_store,
                            &transfer_limits,
                            &work_state.delivery,
                            &request,
                        )? {
                            shardlane_host::ContinuationResult::NeedsProjectSelection => {
                                Err(ConversationServiceError::NeedsProject(
                                    "select a project to continue this conversation".to_string(),
                                ))
                            }
                            shardlane_host::ContinuationResult::ReusedLive {
                                identity,
                                instruction_disposition,
                            } => {
                                // CR-03: accepted tells the client whether the
                                // instruction was actually taken. NeedsTerminal
                                // is an explicit non-acceptance so clients keep
                                // the draft instead of treating it as sent.
                                let accepted = instruction_disposition
                                    .as_ref()
                                    .is_some_and(|disposition| {
                                        *disposition
                                            != shardlane_host::PromptDisposition::NeedsTerminal
                                    })
                                    || instruction_disposition.is_none();
                                Ok(ConversationContinueResponse {
                                    accepted,
                                    strategy: shardlane_host::ContinuationStrategy::AlreadyLive,
                                    identity,
                                    instruction: instruction_disposition,
                                    launched: None,
                                })
                            }
                            shardlane_host::ContinuationResult::Launched {
                                strategy,
                                outcome,
                                briefing_sha256,
                            } => {
                                let _ = briefing_sha256;
                                let identity = outcome.identity.clone().ok_or_else(|| {
                                    ConversationServiceError::Runtime(
                                        "History Continue launched an Agent without a Live Conversation identity"
                                            .to_string(),
                                    )
                                })?;
                                Ok(ConversationContinueResponse {
                                    accepted: true,
                                    strategy,
                                    identity,
                                    instruction: None,
                                    launched: Some(LaunchedTarget {
                                        workspace_id: outcome.workspace_id.clone(),
                                        tab_id: outcome.tab_id.clone(),
                                        pane_id: outcome.pane_id.clone(),
                                    }),
                                })
                            }
                        }
                    },
                )
                .await
                .map_err(|error| {
                    ConversationServiceError::Runtime(format!("join: {error}"))
                });
                match executed {
                    Ok(Ok(response)) => serde_json::to_value(response).map_err(|error| {
                        ApiError::internal(format!("serialize: {error}"), error_id)
                    }),
                    Ok(Err(error)) | Err(error) => {
                        Err(map_conversation_error(error, error_id))
                    }
                }
            }
        })
        .await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => error.into_response(),
    }
}

/// R4-P1: queue-state projection for one live Conversation. Remote/Mobile
/// clients poll this to observe async delivery outcomes (queued / waiting /
/// failed-recoverable / delivery-uncertain / delivered-gone) and the retained
/// text of a terminal failure — "accepted = queued" is no longer a blind spot.
#[derive(serde::Serialize)]
pub struct ConversationQueueStateResponse {
    pub agent_ref: String,
    pub state: &'static str,
    pub text: Option<String>,
    pub reason: Option<String>,
    pub request_id: Option<String>,
}

pub async fn conversation_queue_state(
    axum::extract::State(state): axum::extract::State<Arc<RemoteState>>,
    axum::extract::Path(conversation_id): axum::extract::Path<String>,
) -> Response {
    let request_id = state.next_request_id();
    let conversation_id = shardlane_host::ConversationId::new(conversation_id);
    let Ok(locator) = shardlane_host::resolve_conversation_id(&conversation_id) else {
        return ApiError::invalid_request("invalid conversation id", request_id).into_response();
    };
    let Some(agent_ref) = shardlane_host::live_agent_ref(&locator) else {
        return ApiError::invalid_request("queue state requires a live conversation", request_id)
            .into_response();
    };
    let work_state = state.clone();
    let requested = conversation_id.clone();
    let agent_ref = agent_ref.clone();
    let error_request_id = request_id.clone();
    let instance = crate::bootstrap::current_instance_scope();
    let result = tokio::task::spawn_blocking(
        move || -> Result<ConversationQueueStateResponse, ApiError> {
            // AgentRef/pane is only an index.  Prove the current typed occupant
            // still maps to the requested opaque Conversation before exposing a
            // retained queue receipt; otherwise a stale history id could read the
            // next Agent's text from the same pane.
            let client = conversation_client(&work_state, instance.as_deref())
                .map_err(|error| map_conversation_error(error, error_request_id.clone()))?;
            let current = client
                .agents()
                .map_err(|error| map_herdr_error(error, error_request_id.clone()))?
                .into_iter()
                .find(|agent| agent.pane_id.as_deref() == Some(agent_ref.as_str()));
            let current_conversation = current
                .as_ref()
                .and_then(|agent| agent.agent_session.as_ref())
                .map(|session| {
                    shardlane_host::conversation_id_for_live_session(&agent_ref, session)
                });
            if current_conversation.as_ref() != Some(&requested) {
                return Err(ApiError::not_found(
                    "the live Conversation occupant has changed; reopen the Conversation",
                    error_request_id.clone(),
                ));
            }

            let item = work_state.delivery.queue().queued(&agent_ref);
            let response = match item {
                None => ConversationQueueStateResponse {
                    agent_ref: agent_ref.as_str().to_string(),
                    state: "idle",
                    text: None,
                    reason: None,
                    request_id: None,
                },
                Some(item) if item.conversation_id == requested => {
                    let (state_name, reason) = match &item.state {
                        shardlane_host::QueueState::Queued => ("queued", None),
                        shardlane_host::QueueState::WaitingForTurnBoundary => {
                            ("waiting_for_turn", None)
                        }
                        shardlane_host::QueueState::Delivering => ("delivering", None),
                        shardlane_host::QueueState::Delivered => ("delivered", None),
                        shardlane_host::QueueState::FailedRecoverable(reason) => {
                            ("failed_recoverable", Some(reason.clone()))
                        }
                        shardlane_host::QueueState::DeliveryUncertain(reason) => {
                            ("delivery_uncertain", Some(reason.clone()))
                        }
                        shardlane_host::QueueState::Cancelled => ("cancelled", None),
                    };
                    let terminal = matches!(
                        item.state,
                        shardlane_host::QueueState::FailedRecoverable(_)
                            | shardlane_host::QueueState::DeliveryUncertain(_)
                    );
                    ConversationQueueStateResponse {
                        agent_ref: item.agent_ref.as_str().to_string(),
                        state: state_name,
                        text: terminal.then(|| item.text.clone()),
                        reason,
                        request_id: terminal.then(|| item.request_id.clone()),
                    }
                }
                Some(_) => {
                    return Err(ApiError::not_found(
                        "the queued operation belongs to a different Conversation",
                        error_request_id.clone(),
                    ));
                }
            };
            Ok(response)
        },
    )
    .await;
    match result {
        Ok(Ok(response)) => Json(response).into_response(),
        Ok(Err(error)) => error.into_response(),
        Err(error) => ApiError::internal(
            format!("conversation queue lookup failed: {error}"),
            request_id,
        )
        .into_response(),
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConversationResolveInteractionBody {
    pub interaction_id: String,
    pub revision: u64,
    pub response: shardlane_host::conversation_interactions::InteractionResponse,
}

pub async fn resolve_conversation_interaction(
    State(state): State<Arc<RemoteState>>,
    axum::extract::Path(conversation_id): axum::extract::Path<String>,
    body: Result<Json<ConversationResolveInteractionBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    // D01 + D13: decode through the shared helper, so a malformed body gets
    // the unified ApiError envelope (400 invalid_request) instead of axum's
    // bare text 422 rejection.
    let body = match crate::server::decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    let conversation_id = shardlane_host::ConversationId::new(conversation_id);
    let interaction_id = shardlane_host::InteractionId::new(body.interaction_id);
    let revision = body.revision;
    let response = body.response;
    let work_state = state.clone();
    let error_request_id = request_id.clone();

    let instance = crate::bootstrap::current_instance_scope();
    let result = tokio::task::spawn_blocking(move || {
        let client = conversation_client(&work_state, instance.as_deref())
            .map_err(|error| map_conversation_error(error, error_request_id.clone()))?;
        // D13: route construction through the shared helper so the service
        // carries the real history db path and the injected live-session
        // manager (a bare HostConversationService::new with an empty path
        // bypassed both).
        let service = host_conversation_service(&work_state, &client);
        service
            .resolve_conversation_interaction(&conversation_id, &interaction_id, revision, response)
            .map_err(|error| map_conversation_error(error, error_request_id))
    })
    .await;

    match result {
        Ok(Ok(resolution)) => Json(resolution).into_response(),
        Ok(Err(error)) => error.into_response(),
        Err(error) => {
            ApiError::internal(format!("resolve interaction failed: {error}"), request_id)
                .into_response()
        }
    }
}

pub async fn delegate_conversation_interaction(
    State(state): State<Arc<RemoteState>>,
    axum::extract::Path((conversation_id, interaction_id)): axum::extract::Path<(String, String)>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let request_id = state.next_request_id();
    let conversation_id = shardlane_host::ConversationId::new(conversation_id);
    let interaction_id = shardlane_host::InteractionId::new(interaction_id);
    let revision = query
        .get("revision")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1);
    let work_state = state.clone();
    let error_request_id = request_id.clone();

    let instance = crate::bootstrap::current_instance_scope();
    let result = tokio::task::spawn_blocking(move || {
        let client = conversation_client(&work_state, instance.as_deref())
            .map_err(|error| map_conversation_error(error, error_request_id.clone()))?;
        // D13: same shared-helper construction as resolve (real history db
        // path + injected live-session manager).
        let service = host_conversation_service(&work_state, &client);
        service
            .delegate_interaction_to_terminal(&conversation_id, &interaction_id, revision)
            .map_err(|error| map_conversation_error(error, error_request_id))
    })
    .await;

    match result {
        Ok(Ok(_resolution)) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => error.into_response(),
        Err(error) => {
            ApiError::internal(format!("delegate interaction failed: {error}"), request_id)
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_query_maps_to_host_bounds() {
        let query = ConversationWindowQuery {
            anchor_seq: Some(7),
            before: Some(0),
            after: Some(999),
        };
        let bounds = ConversationWindowBounds::from(&query);
        assert_eq!(bounds.anchor_seq, Some(7));
        assert_eq!(bounds.before, Some(0));
        assert_eq!(bounds.after, Some(999));
        assert_eq!(shardlane_host::clamp_conversation_window(0), 1);
        assert_eq!(
            shardlane_host::clamp_conversation_window(999),
            shardlane_host::MAX_CONVERSATION_WINDOW
        );
        assert_eq!(clamp_limit(Some(999), 1, 20), 20);
    }

    #[test]
    fn history_and_live_ids_use_the_same_opaque_prefix() {
        let history = shardlane_host::conversation_id_for_history_key("claude-code:s1");
        let live = shardlane_host::conversation_id_for_live_agent(&AgentRef::new("pane-1"));
        assert!(history.as_str().starts_with("conv_1_"));
        assert!(live.as_str().starts_with("conv_1_"));
        assert_ne!(history, live);
    }

    #[test]
    fn host_errors_map_to_distinct_wire_envelopes() {
        let invalid = map_conversation_error(
            ConversationServiceError::Invalid("text must not be empty".into()),
            "req-1".into(),
        );
        assert_eq!(invalid.status, axum::http::StatusCode::BAD_REQUEST);
        let not_found = map_conversation_error(
            ConversationServiceError::NotFound("conversation not found".into()),
            "req-2".into(),
        );
        assert_eq!(not_found.status, axum::http::StatusCode::NOT_FOUND);
        // D05: 404 comes only from the typed NotFound variant — a Runtime
        // error whose text happens to contain "not found" must stay an
        // internal/unavailable envelope, never a substring-derived 404.
        let runtime_not_found = map_conversation_error(
            ConversationServiceError::Runtime("history source not found".into()),
            "req-3".into(),
        );
        assert_eq!(
            runtime_not_found.status,
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(runtime_not_found.code, "runtime_unavailable");
        let runtime = map_conversation_error(
            ConversationServiceError::Runtime("readiness failed".into()),
            "req-4".into(),
        );
        assert_eq!(runtime.status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }
}
