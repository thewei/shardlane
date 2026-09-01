//! Host-owned ConversationInteraction domain model and broker.
//!
//! [INPUT]: Provider companion hooks/plugins/extensions emitting structured in-turn
//! interaction requests (questions, permissions, approvals), and user resolution actions.
//! [OUTPUT]: Provider-neutral `ConversationInteraction` domain objects, CAS-guarded
//! resolution with first-responder-wins semantics, stale revision rejection, identity-change
//! fail-closed cancellations, and dispatching to exact waiting bridge responders.
//! [POS]: S1 interaction domain / audit AF-01/AF-04/AF-15/AF-16. Never converts a question
//! into an ordinary `agent.prompt` or guessed PTY keys. Raw terminal input stays Terminal-only.

pub use crate::ids::{AgentRef, BridgeRequestId, ConversationId, InteractionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Current timestamp in milliseconds since Unix epoch.
pub fn current_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Product-level kind of an in-turn interaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationInteractionKind {
    Question,
    Permission,
    PlanApproval,
    Authentication,
    Other,
}

/// Lifecycle state of an interaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationInteractionState {
    Pending,
    Resolving,
    Resolved,
    Rejected,
    Cancelled,
    NativeFallback,
    Expired,
}

/// Semantic timeline anchor for positioning an interaction card in the Chat timeline.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ConversationInteractionAnchor {
    ToolCallId(String),
    ProviderEventId(String),
    Seq(u64),
    #[default]
    Tail,
}

/// One choice option presented to the user.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InteractionChoice {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub is_recommended: bool,
}

/// Scope of an allowed permission.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PermissionScope {
    Once,
    Session,
    Directory(String),
    Workspace,
    Custom(String),
}

/// User's response to an interaction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InteractionResponse {
    Choice {
        option_id: String,
    },
    MultiChoice {
        option_ids: Vec<String>,
    },
    Text {
        value: String,
    },
    Allow {
        scope: PermissionScope,
    },
    Deny {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Cancel,
    DelegateToTerminal,
}

/// Provider-neutral Host domain object for an in-turn interaction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ConversationInteraction {
    pub id: InteractionId,
    pub conversation_id: ConversationId,
    pub agent_ref: AgentRef,
    pub provider: String,
    pub kind: ConversationInteractionKind,
    pub state: ConversationInteractionState,
    pub prompt: String,
    #[serde(default)]
    pub choices: Vec<InteractionChoice>,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub allow_custom_text: bool,
    #[serde(default)]
    pub anchor: ConversationInteractionAnchor,
    pub revision: u64,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
}

/// Immutable record of a committed interaction resolution.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InteractionResolution {
    pub id: InteractionId,
    pub conversation_id: ConversationId,
    pub revision: u64,
    pub state: ConversationInteractionState,
    pub response: InteractionResponse,
    pub resolved_at_ms: u64,
}

/// Why an interaction was cancelled without a user resolution. Host-internal
/// only: it travels to provider-bridge responders in-process, never across
/// the Remote DTO wire.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionCancelReason {
    BridgeDisconnected,
    ProviderCancelled,
    IdentityChanged,
    /// The user actively sent `InteractionResponse::Cancel`.
    UserDismissed,
    /// The interaction passed its deadline without a resolution (C27: expiry
    /// is its own semantic, never misreported as a user dismissal).
    Expired,
}

/// Typed error returned when an interaction resolution fails.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InteractionResolveError {
    NotFound,
    StaleRevision { expected: u64, actual: u64 },
    AlreadyResolved { state: ConversationInteractionState },
    OccupantChanged { expected: String, actual: String },
    BridgeUnavailable,
    InvalidResponse(String),
}

impl fmt::Display for InteractionResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("interaction not found"),
            Self::StaleRevision { expected, actual } => write!(
                formatter,
                "stale interaction revision: expected {expected}, actual {actual}"
            ),
            Self::AlreadyResolved { state } => {
                write!(formatter, "interaction already resolved ({state:?})")
            }
            Self::OccupantChanged { expected, actual } => write!(
                formatter,
                "agent occupant changed: expected {expected}, actual {actual}"
            ),
            Self::BridgeUnavailable => formatter.write_str("provider bridge is unavailable"),
            Self::InvalidResponse(detail) => {
                write!(formatter, "invalid interaction response: {detail}")
            }
        }
    }
}

impl std::error::Error for InteractionResolveError {}

/// Disposition sent back to the waiting local bridge client.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BridgeResolutionDisposition {
    Resolved(InteractionResolution),
    NativeFallback,
    Cancelled(InteractionCancelReason),
}

pub type BridgeResponderCallback = Arc<dyn Fn(BridgeResolutionDisposition) + Send + Sync + 'static>;

/// A responder collected while the records lock was held, invoked only after
/// the lock is released (C35): a slow responder must never block
/// publish/snapshot/resolve, and a re-entrant responder (one that calls back
/// into the broker from its callback) must not deadlock on the
/// non-reentrant records mutex.
struct ResponderNotification {
    responder: BridgeResponderCallback,
    disposition: BridgeResolutionDisposition,
}

impl ResponderNotification {
    fn deliver(self) {
        (self.responder)(self.disposition);
    }
}

/// Deliver every collected notification with the records lock released.
fn deliver_notifications(notifications: Vec<ResponderNotification>) {
    for notification in notifications {
        notification.deliver();
    }
}

/// Request submitted by a provider bridge to publish a pending interaction.
pub struct ProviderInteractionRequest {
    pub conversation_id: ConversationId,
    pub agent_ref: AgentRef,
    pub provider: String,
    pub kind: ConversationInteractionKind,
    pub prompt: String,
    pub choices: Vec<InteractionChoice>,
    pub multiple: bool,
    pub allow_custom_text: bool,
    pub anchor: ConversationInteractionAnchor,
    pub occupant_fingerprint: String,
    pub bridge_request_id: BridgeRequestId,
    pub allowed_scopes: Vec<PermissionScope>,
    pub expires_at_ms: Option<u64>,
}

struct PendingInteractionRecord {
    public: ConversationInteraction,
    expected_occupant_fingerprint: String,
    bridge_request_id: BridgeRequestId,
    allowed_scopes: Vec<PermissionScope>,
    responder: Option<BridgeResponderCallback>,
    /// When the record reached a terminal state (C04): drives the short
    /// retention window after which the bounded broker drops it.
    terminal_at_ms: Option<u64>,
}

/// How long a terminal (resolved/expired/cancelled/fallback) interaction
/// record stays visible to late pollers before the bounded broker drops it.
const TERMINAL_RECORD_RETENTION_MS: u64 = 60_000;

/// Host-owned thread-safe broker for in-turn interactions.
#[derive(Default)]
pub struct ConversationInteractionBroker {
    records: Mutex<HashMap<InteractionId, PendingInteractionRecord>>,
}

impl ConversationInteractionBroker {
    pub fn new() -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
        }
    }

    /// Publish a new interaction request from a provider bridge.
    pub fn publish_request(
        &self,
        request: ProviderInteractionRequest,
        responder: Option<BridgeResponderCallback>,
    ) -> ConversationInteraction {
        let interaction_id = InteractionId::new(format!("ix_{}", uuid::Uuid::new_v4()));
        let now = current_time_ms();
        let public = ConversationInteraction {
            id: interaction_id.clone(),
            conversation_id: request.conversation_id,
            agent_ref: request.agent_ref,
            provider: request.provider,
            kind: request.kind,
            state: ConversationInteractionState::Pending,
            prompt: request.prompt,
            choices: request.choices,
            multiple: request.multiple,
            allow_custom_text: request.allow_custom_text,
            anchor: request.anchor,
            revision: 1,
            created_at_ms: now,
            expires_at_ms: request.expires_at_ms,
        };

        let record = PendingInteractionRecord {
            public: public.clone(),
            expected_occupant_fingerprint: request.occupant_fingerprint,
            bridge_request_id: request.bridge_request_id,
            allowed_scopes: request.allowed_scopes,
            responder,
            terminal_at_ms: None,
        };

        let mut lock = self.records.lock().unwrap_or_else(|p| p.into_inner());
        lock.insert(interaction_id, record);
        public
    }

    /// Resolve a pending interaction with CAS revision check and occupant validation.
    pub fn resolve(
        &self,
        conversation_id: &ConversationId,
        interaction_id: &InteractionId,
        expected_revision: u64,
        response: InteractionResponse,
        current_occupant_fingerprint: &str,
    ) -> Result<InteractionResolution, InteractionResolveError> {
        let mut lock = self.records.lock().unwrap_or_else(|p| p.into_inner());
        let record = lock
            .get_mut(interaction_id)
            .ok_or(InteractionResolveError::NotFound)?;

        if record.public.conversation_id != *conversation_id {
            return Err(InteractionResolveError::NotFound);
        }

        if record.public.state != ConversationInteractionState::Pending {
            return Err(InteractionResolveError::AlreadyResolved {
                state: record.public.state,
            });
        }

        if record.public.revision != expected_revision {
            return Err(InteractionResolveError::StaleRevision {
                expected: expected_revision,
                actual: record.public.revision,
            });
        }

        if record.expected_occupant_fingerprint != current_occupant_fingerprint {
            return Err(InteractionResolveError::OccupantChanged {
                expected: record.expected_occupant_fingerprint.clone(),
                actual: current_occupant_fingerprint.to_string(),
            });
        }

        // Validate response against allowed choices/text/scopes
        validate_interaction_response(&record.public, &record.allowed_scopes, &response)?;

        let new_state = match &response {
            InteractionResponse::Choice { .. }
            | InteractionResponse::MultiChoice { .. }
            | InteractionResponse::Text { .. }
            | InteractionResponse::Allow { .. } => ConversationInteractionState::Resolved,
            InteractionResponse::Deny { .. } => ConversationInteractionState::Rejected,
            InteractionResponse::Cancel => ConversationInteractionState::Cancelled,
            InteractionResponse::DelegateToTerminal => ConversationInteractionState::NativeFallback,
        };

        record.public.state = new_state;
        record.public.revision += 1;
        let resolved_at_ms = current_time_ms();
        record.terminal_at_ms = Some(resolved_at_ms);

        let resolution = InteractionResolution {
            id: interaction_id.clone(),
            conversation_id: conversation_id.clone(),
            revision: record.public.revision,
            state: new_state,
            response,
            resolved_at_ms,
        };

        let disposition = match new_state {
            ConversationInteractionState::NativeFallback => {
                BridgeResolutionDisposition::NativeFallback
            }
            ConversationInteractionState::Cancelled => {
                BridgeResolutionDisposition::Cancelled(InteractionCancelReason::UserDismissed)
            }
            _ => BridgeResolutionDisposition::Resolved(resolution.clone()),
        };

        let responder = record.responder.take();
        // C35: drop the records lock BEFORE invoking the responder — a
        // re-entrant responder (snapshot/resolve inside the callback) would
        // otherwise deadlock on the non-reentrant mutex, and a slow responder
        // would stall every other broker operation.
        drop(lock);
        if let Some(responder) = responder {
            responder(disposition);
        }

        Ok(resolution)
    }

    /// Shortcut for native fallback (DelegateToTerminal).
    pub fn delegate_to_terminal(
        &self,
        conversation_id: &ConversationId,
        interaction_id: &InteractionId,
        expected_revision: u64,
        current_occupant_fingerprint: &str,
    ) -> Result<InteractionResolution, InteractionResolveError> {
        self.resolve(
            conversation_id,
            interaction_id,
            expected_revision,
            InteractionResponse::DelegateToTerminal,
            current_occupant_fingerprint,
        )
    }

    /// Cancel all pending interactions associated with a bridge request id.
    pub fn cancel_by_bridge(
        &self,
        bridge_request_id: &BridgeRequestId,
        reason: InteractionCancelReason,
    ) -> Vec<InteractionId> {
        self.settle_matching(ConversationInteractionState::Cancelled, reason, |record| {
            record.bridge_request_id == *bridge_request_id
        })
    }

    /// Cancel pending interactions whose expected occupant does not match the new occupant.
    pub fn cancel_on_occupant_change(
        &self,
        conversation_id: &ConversationId,
        new_occupant_fingerprint: &str,
    ) -> Vec<InteractionId> {
        self.settle_matching(
            ConversationInteractionState::Cancelled,
            InteractionCancelReason::IdentityChanged,
            |record| {
                record.public.conversation_id == *conversation_id
                    && record.expected_occupant_fingerprint != new_occupant_fingerprint
            },
        )
    }

    /// Shared settle loop (C20): flips every pending record matching
    /// `predicate` to `state`, bumps its revision, stamps the retention
    /// clock, and returns the settled ids. Responder callbacks are collected
    /// and returned for delivery AFTER the caller releases the lock (C35).
    fn settle_matching<P>(
        &self,
        state: ConversationInteractionState,
        reason: InteractionCancelReason,
        predicate: P,
    ) -> Vec<InteractionId>
    where
        P: Fn(&PendingInteractionRecord) -> bool,
    {
        let (settled, notifications) = {
            let mut lock = self.records.lock().unwrap_or_else(|p| p.into_inner());
            Self::settle_locked(&mut lock, state, reason, predicate)
        };
        deliver_notifications(notifications);
        settled
    }

    fn settle_locked<P>(
        lock: &mut HashMap<InteractionId, PendingInteractionRecord>,
        state: ConversationInteractionState,
        reason: InteractionCancelReason,
        predicate: P,
    ) -> (Vec<InteractionId>, Vec<ResponderNotification>)
    where
        P: Fn(&PendingInteractionRecord) -> bool,
    {
        let settled_at_ms = current_time_ms();
        let mut settled = Vec::new();
        let mut notifications = Vec::new();
        for (id, record) in lock.iter_mut() {
            if record.public.state == ConversationInteractionState::Pending && predicate(record) {
                record.public.state = state;
                record.public.revision += 1;
                record.terminal_at_ms = Some(settled_at_ms);
                settled.push(id.clone());
                if let Some(responder) = record.responder.take() {
                    notifications.push(ResponderNotification {
                        responder,
                        disposition: BridgeResolutionDisposition::Cancelled(reason.clone()),
                    });
                }
            }
        }
        (settled, notifications)
    }

    /// Expire pending interactions past their deadline, then drop terminal
    /// records after the short retention window (C04: the records map used to
    /// only ever grow, so a long-lived Host leaked every settled interaction
    /// and every `snapshot()` cloned them forever). Responder callbacks are
    /// collected, never invoked under the lock (C35).
    fn expire_and_prune_locked(
        lock: &mut HashMap<InteractionId, PendingInteractionRecord>,
    ) -> Vec<ResponderNotification> {
        let (_, notifications) = Self::settle_locked(
            lock,
            ConversationInteractionState::Expired,
            InteractionCancelReason::Expired,
            |record| {
                record
                    .public
                    .expires_at_ms
                    .is_some_and(|deadline| current_time_ms() >= deadline)
            },
        );
        let now = current_time_ms();
        lock.retain(|_, record| {
            record
                .terminal_at_ms
                .is_none_or(|settled| now.saturating_sub(settled) < TERMINAL_RECORD_RETENTION_MS)
        });
        notifications
    }

    /// Get all interactions for a conversation. Expiry and bounded retention
    /// run here so every detail poll also keeps the broker bounded. Responder
    /// notifications fire only after the records lock is released (C35).
    pub fn snapshot(&self, conversation_id: &ConversationId) -> Vec<ConversationInteraction> {
        let (items, notifications) = {
            let mut lock = self.records.lock().unwrap_or_else(|p| p.into_inner());
            let notifications = Self::expire_and_prune_locked(&mut lock);
            let mut items: Vec<ConversationInteraction> = lock
                .values()
                .filter(|r| r.public.conversation_id == *conversation_id)
                .map(|r| r.public.clone())
                .collect();
            items.sort_by_key(|i| i.created_at_ms);
            (items, notifications)
        };
        deliver_notifications(notifications);
        items
    }

    /// Find an interaction by ID.
    pub fn get(&self, interaction_id: &InteractionId) -> Option<ConversationInteraction> {
        let lock = self.records.lock().unwrap_or_else(|p| p.into_inner());
        lock.get(interaction_id).map(|r| r.public.clone())
    }
}

fn validate_interaction_response(
    interaction: &ConversationInteraction,
    allowed_scopes: &[PermissionScope],
    response: &InteractionResponse,
) -> Result<(), InteractionResolveError> {
    match response {
        InteractionResponse::Choice { option_id } => {
            if interaction.choices.iter().any(|c| c.id == *option_id) {
                Ok(())
            } else {
                Err(InteractionResolveError::InvalidResponse(format!(
                    "unknown option_id {option_id:?}"
                )))
            }
        }
        InteractionResponse::MultiChoice { option_ids } => {
            if !interaction.multiple {
                return Err(InteractionResolveError::InvalidResponse(
                    "multiple choice not allowed for this interaction".to_string(),
                ));
            }
            if option_ids.is_empty() {
                return Err(InteractionResolveError::InvalidResponse(
                    "empty multi-choice response".to_string(),
                ));
            }
            for id in option_ids {
                if !interaction.choices.iter().any(|c| c.id == *id) {
                    return Err(InteractionResolveError::InvalidResponse(format!(
                        "unknown option_id {id:?}"
                    )));
                }
            }
            Ok(())
        }
        InteractionResponse::Text { value } => {
            if !interaction.allow_custom_text {
                return Err(InteractionResolveError::InvalidResponse(
                    "custom text input not allowed for this interaction".to_string(),
                ));
            }
            if value.trim().is_empty() {
                return Err(InteractionResolveError::InvalidResponse(
                    "empty custom text response".to_string(),
                ));
            }
            Ok(())
        }
        InteractionResponse::Allow { scope } => {
            if allowed_scopes.is_empty() {
                // If no specific scopes constrained, Once is standard
                Ok(())
            } else if allowed_scopes.contains(scope) {
                Ok(())
            } else {
                Err(InteractionResolveError::InvalidResponse(format!(
                    "permission scope {scope:?} not supported for this interaction"
                )))
            }
        }
        InteractionResponse::Deny { .. }
        | InteractionResponse::Cancel
        | InteractionResponse::DelegateToTerminal => Ok(()),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn sample_request(conversation_id: &ConversationId) -> ProviderInteractionRequest {
        ProviderInteractionRequest {
            conversation_id: conversation_id.clone(),
            agent_ref: AgentRef::new("agent-1"),
            provider: "claude-code".to_string(),
            kind: ConversationInteractionKind::Question,
            prompt: "Which migration strategy should I use?".to_string(),
            choices: vec![
                InteractionChoice {
                    id: "compat".to_string(),
                    label: "Keep compatibility".to_string(),
                    description: None,
                    is_recommended: true,
                },
                InteractionChoice {
                    id: "remove".to_string(),
                    label: "Remove legacy path".to_string(),
                    description: None,
                    is_recommended: false,
                },
            ],
            multiple: false,
            allow_custom_text: true,
            anchor: ConversationInteractionAnchor::Tail,
            occupant_fingerprint: "session-fp-1".to_string(),
            bridge_request_id: BridgeRequestId::new("bridge-req-1"),
            allowed_scopes: vec![PermissionScope::Once, PermissionScope::Session],
            expires_at_ms: None,
        }
    }

    #[test]
    fn publish_and_resolve_round_trip() {
        let broker = ConversationInteractionBroker::new();
        let conv_id = ConversationId::new("conv-1");
        let (tx, rx) = std::sync::mpsc::sync_channel(1);

        let interaction = broker.publish_request(
            sample_request(&conv_id),
            Some(Arc::new(move |disp| {
                tx.send(disp).unwrap();
            })),
        );

        assert_eq!(interaction.state, ConversationInteractionState::Pending);
        assert_eq!(interaction.revision, 1);

        let resolution = broker
            .resolve(
                &conv_id,
                &interaction.id,
                1,
                InteractionResponse::Choice {
                    option_id: "compat".to_string(),
                },
                "session-fp-1",
            )
            .expect("resolution should succeed");

        assert_eq!(resolution.state, ConversationInteractionState::Resolved);
        assert_eq!(resolution.revision, 2);

        let disp = rx.recv().expect("responder must receive disposition");
        match disp {
            BridgeResolutionDisposition::Resolved(res) => {
                assert_eq!(res.id, interaction.id);
                assert_eq!(
                    res.response,
                    InteractionResponse::Choice {
                        option_id: "compat".to_string()
                    }
                );
            }
            _ => panic!("unexpected disposition"),
        }
    }

    #[test]
    fn stale_revision_is_rejected() {
        let broker = ConversationInteractionBroker::new();
        let conv_id = ConversationId::new("conv-1");

        let interaction = broker.publish_request(sample_request(&conv_id), None);

        let err = broker
            .resolve(
                &conv_id,
                &interaction.id,
                99,
                InteractionResponse::Choice {
                    option_id: "compat".to_string(),
                },
                "session-fp-1",
            )
            .unwrap_err();

        assert_eq!(
            err,
            InteractionResolveError::StaleRevision {
                expected: 99,
                actual: 1
            }
        );
    }

    #[test]
    fn occupant_change_fails_closed_on_resolve() {
        let broker = ConversationInteractionBroker::new();
        let conv_id = ConversationId::new("conv-1");

        let interaction = broker.publish_request(sample_request(&conv_id), None);

        let err = broker
            .resolve(
                &conv_id,
                &interaction.id,
                1,
                InteractionResponse::Choice {
                    option_id: "compat".to_string(),
                },
                "different-session-fp",
            )
            .unwrap_err();

        assert!(matches!(
            err,
            InteractionResolveError::OccupantChanged { .. }
        ));
    }

    #[test]
    fn cancel_on_occupant_change_notifies_responder() {
        let broker = ConversationInteractionBroker::new();
        let conv_id = ConversationId::new("conv-1");
        let (tx, rx) = std::sync::mpsc::sync_channel(1);

        let interaction = broker.publish_request(
            sample_request(&conv_id),
            Some(Arc::new(move |disp| {
                tx.send(disp).unwrap();
            })),
        );

        let cancelled = broker.cancel_on_occupant_change(&conv_id, "new-occupant-fp");
        assert_eq!(cancelled, vec![interaction.id]);

        let disp = rx.recv().expect("responder must receive cancel");
        assert_eq!(
            disp,
            BridgeResolutionDisposition::Cancelled(InteractionCancelReason::IdentityChanged)
        );
    }

    #[test]
    fn race_between_two_resolvers_allows_only_one_winner() {
        let broker = Arc::new(ConversationInteractionBroker::new());
        let conv_id = ConversationId::new("conv-1");
        let (tx, rx) = std::sync::mpsc::sync_channel(1);

        let interaction = broker.publish_request(
            sample_request(&conv_id),
            Some(Arc::new(move |disp| {
                let _ = tx.send(disp);
            })),
        );

        let broker_a = Arc::clone(&broker);
        let conv_a = conv_id.clone();
        let id_a = interaction.id.clone();

        let handle_a = std::thread::spawn(move || {
            broker_a.resolve(
                &conv_a,
                &id_a,
                1,
                InteractionResponse::Choice {
                    option_id: "compat".to_string(),
                },
                "session-fp-1",
            )
        });

        let broker_b = Arc::clone(&broker);
        let conv_b = conv_id.clone();
        let id_b = interaction.id.clone();

        let handle_b = std::thread::spawn(move || {
            broker_b.resolve(
                &conv_b,
                &id_b,
                1,
                InteractionResponse::Choice {
                    option_id: "remove".to_string(),
                },
                "session-fp-1",
            )
        });

        let res_a = handle_a.join().unwrap();
        let res_b = handle_b.join().unwrap();

        let success_count = usize::from(res_a.is_ok()) + usize::from(res_b.is_ok());
        assert_eq!(success_count, 1, "exactly one resolver must win the race");

        let disp = rx.recv().expect("responder received exactly once");
        assert!(matches!(disp, BridgeResolutionDisposition::Resolved(_)));
    }

    #[test]
    fn invalid_choice_is_rejected() {
        let broker = ConversationInteractionBroker::new();
        let conv_id = ConversationId::new("conv-1");

        let interaction = broker.publish_request(sample_request(&conv_id), None);

        let err = broker
            .resolve(
                &conv_id,
                &interaction.id,
                1,
                InteractionResponse::Choice {
                    option_id: "non-existent-option".to_string(),
                },
                "session-fp-1",
            )
            .unwrap_err();

        assert!(matches!(err, InteractionResolveError::InvalidResponse(_)));
    }

    #[test]
    fn delegate_to_terminal_sets_native_fallback_state() {
        let broker = ConversationInteractionBroker::new();
        let conv_id = ConversationId::new("conv-1");
        let (tx, rx) = std::sync::mpsc::sync_channel(1);

        let interaction = broker.publish_request(
            sample_request(&conv_id),
            Some(Arc::new(move |disp| {
                tx.send(disp).unwrap();
            })),
        );

        let resolution = broker
            .delegate_to_terminal(&conv_id, &interaction.id, 1, "session-fp-1")
            .expect("fallback should succeed");

        assert_eq!(
            resolution.state,
            ConversationInteractionState::NativeFallback
        );
        assert_eq!(resolution.response, InteractionResponse::DelegateToTerminal);

        let disp = rx.recv().unwrap();
        assert_eq!(disp, BridgeResolutionDisposition::NativeFallback);
    }

    #[test]
    fn snapshot_expires_deadline_passed_interactions_with_the_expired_reason() {
        // C04/C27: expiry runs inside snapshot (clear_expired had zero
        // callers) and reports Expired, never UserDismissed.
        let broker = ConversationInteractionBroker::new();
        let conv_id = ConversationId::new("conv-1");
        let (tx, rx) = std::sync::mpsc::sync_channel(1);

        let mut request = sample_request(&conv_id);
        request.expires_at_ms = Some(current_time_ms().saturating_sub(1));
        let interaction = broker.publish_request(
            request,
            Some(Arc::new(move |disp| {
                tx.send(disp).unwrap();
            })),
        );

        let items = broker.snapshot(&conv_id);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, ConversationInteractionState::Expired);
        assert_ne!(items[0].revision, interaction.revision);
        assert_eq!(
            rx.recv().expect("responder must observe expiry"),
            BridgeResolutionDisposition::Cancelled(InteractionCancelReason::Expired)
        );
    }

    #[test]
    fn terminal_records_are_dropped_after_the_retention_window() {
        let broker = ConversationInteractionBroker::new();
        let conv_id = ConversationId::new("conv-1");

        let interaction = broker.publish_request(sample_request(&conv_id), None);
        broker
            .resolve(
                &conv_id,
                &interaction.id,
                1,
                InteractionResponse::Choice {
                    option_id: "compat".to_string(),
                },
                "session-fp-1",
            )
            .expect("resolution should succeed");
        assert_eq!(broker.snapshot(&conv_id).len(), 1, "inside retention");

        // Age the settled record past the retention window, then poll again.
        {
            let mut lock = broker.records.lock().unwrap_or_else(|p| p.into_inner());
            let record = lock.get_mut(&interaction.id).expect("record retained");
            record.terminal_at_ms =
                Some(current_time_ms().saturating_sub(TERMINAL_RECORD_RETENTION_MS + 1));
        }
        let items = broker.snapshot(&conv_id);
        assert!(items.is_empty(), "expired retention must prune the record");
        assert!(
            broker.get(&interaction.id).is_none(),
            "the bounded broker must not keep pruned records"
        );
    }

    #[test]
    fn cancel_by_bridge_settles_only_that_bridge_and_bumps_revision() {
        let broker = ConversationInteractionBroker::new();
        let conv_id = ConversationId::new("conv-1");
        let first = broker.publish_request(sample_request(&conv_id), None);

        let mut other_bridge = sample_request(&conv_id);
        other_bridge.bridge_request_id = BridgeRequestId::new("bridge-req-2");
        let second = broker.publish_request(other_bridge, None);

        let cancelled = broker.cancel_by_bridge(
            &BridgeRequestId::new("bridge-req-1"),
            InteractionCancelReason::BridgeDisconnected,
        );
        assert_eq!(cancelled, vec![first.id.clone()]);
        assert_eq!(
            broker.get(&first.id).expect("kept until retention").state,
            ConversationInteractionState::Cancelled
        );
        assert_eq!(
            broker
                .get(&second.id)
                .expect("unrelated bridge untouched")
                .state,
            ConversationInteractionState::Pending
        );
    }

    #[test]
    fn reentrant_responder_does_not_deadlock_the_resolve_path() {
        // C35: responders used to run while the records mutex was still held,
        // so a responder that re-entered the broker (snapshot() here) dead
        // locked the thread and every later broker operation.
        let broker = Arc::new(ConversationInteractionBroker::new());
        let conv_id = ConversationId::new("conv-1");
        let reentrant_broker = Arc::clone(&broker);
        let reentrant_conv = conv_id.clone();

        let interaction = broker.publish_request(
            sample_request(&conv_id),
            Some(Arc::new(move |_disposition| {
                let items = reentrant_broker.snapshot(&reentrant_conv);
                assert_eq!(items.len(), 1, "the settled record stays visible");
                assert_eq!(items[0].state, ConversationInteractionState::Resolved);
            })),
        );

        broker
            .resolve(
                &conv_id,
                &interaction.id,
                1,
                InteractionResponse::Choice {
                    option_id: "compat".to_string(),
                },
                "session-fp-1",
            )
            .expect("resolution must succeed with a re-entrant responder");
    }

    #[test]
    fn reentrant_responder_does_not_deadlock_the_settle_and_snapshot_paths() {
        // C35: the cancel/expire responders fire after the lock is released,
        // so both settle paths tolerate a responder that re-enters snapshot().
        let broker = Arc::new(ConversationInteractionBroker::new());
        let conv_id = ConversationId::new("conv-1");

        let settle_broker = Arc::clone(&broker);
        let settle_conv = conv_id.clone();
        let interaction = broker.publish_request(
            sample_request(&conv_id),
            Some(Arc::new(move |_disposition| {
                let _ = settle_broker.snapshot(&settle_conv);
            })),
        );
        let cancelled = broker.cancel_on_occupant_change(&conv_id, "new-occupant-fp");
        assert_eq!(cancelled, vec![interaction.id.clone()]);

        let expire_broker = Arc::clone(&broker);
        let expire_conv = conv_id.clone();
        let mut expiring = sample_request(&conv_id);
        expiring.bridge_request_id = BridgeRequestId::new("bridge-req-2");
        expiring.expires_at_ms = Some(current_time_ms().saturating_sub(1));
        broker.publish_request(
            expiring,
            Some(Arc::new(move |_disposition| {
                let _ = expire_broker.snapshot(&expire_conv);
            })),
        );
        let items = broker.snapshot(&conv_id);
        assert_eq!(items.len(), 2, "both settled records stay in retention");
        assert!(items
            .iter()
            .all(|item| item.state != ConversationInteractionState::Pending));
    }
}
