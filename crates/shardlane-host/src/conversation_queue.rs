//! Host-owned safe Working queued follow-up (one per live Conversation).
//!
//! [INPUT]: the queue request (exact Conversation/Agent/provider/native
//! session identity + baseline revision + text) and a runtime adapter
//! implementing [`FollowUpRuntime`].
//! [OUTPUT]: a single-item semantic follow-up queue: enqueued while Working →
//! exact-target `agent.wait` (cancellable, no global watcher) → identity
//! re-verification → exactly one prompt. Blocked keeps the queue; identity
//! drift fails closed and returns the text; uncertain delivery is never
//! retried automatically.
//! [POS]: plan M4 / audit AF-15/AF-16. Raw Terminal Steering still belongs
//! only to Terminal; this module never sends terminal bytes.

use crate::dto::ConversationIdentity;
use crate::herdr::{
    AgentPromptParams, AgentSessionInfo, AgentWaitParams, HerdrAgentStatus, HerdrClient, HerdrError,
};
use crate::ids::{AgentRef, ConversationId};
use crate::services::PromptDisposition;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// Settled states from which a semantic prompt is sendable or explicitly
/// retained. `Blocked` is NOT sendable: the queue is retained until the user
/// unblocks through the Terminal.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WaitOutcome {
    #[default]
    Sendable,
    Blocked,
    Timeout,
}

/// Acceptance classification for the single prompt attempt.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PromptAcceptance {
    #[default]
    Accepted,
    /// Definite rejection — safe to return the text to the draft.
    Rejected(String),
    /// Outcome unknown (transport dropped mid-delivery). Never retried
    /// automatically; the user decides.
    Uncertain(String),
}

/// Runtime seam for the delivery transaction. Production implementation is
/// `HerdrClient`; tests use a counting mock.
pub trait FollowUpRuntime {
    /// Exact-target wait for a settled state. Must not be a global watcher.
    fn wait_sendable(&self, agent: &AgentRef, timeout_ms: u64) -> Result<WaitOutcome, String>;
    /// Re-resolve the exact occupant of the Agent target after settling.
    fn agent_identity(&self, agent: &AgentRef) -> Result<Option<FollowUpIdentity>, String>;
    /// Deliver exactly one semantic prompt.
    fn prompt(&self, agent: &AgentRef, text: &str) -> Result<PromptAcceptance, String>;
}

/// Typed occupant identity used for the fail-closed revalidation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FollowUpIdentity {
    pub provider: String,
    pub native_session_id: Option<String>,
    /// AC-16: opaque fingerprint over the full typed Herdr session locator.
    /// Path-backed providers expose `native_session_id = None`, so only this
    /// fingerprint discriminates a same-provider replacement occupant.
    pub session_fingerprint: String,
    pub revision: u64,
    pub interactive_ready: bool,
    pub status: crate::dto::AgentStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueueState {
    Queued,
    WaitingForTurnBoundary,
    Delivering,
    Delivered,
    FailedRecoverable(String),
    DeliveryUncertain(String),
    Cancelled,
}

/// One queued semantic follow-up. All identity material is captured at queue
/// time so delivery can fail closed if the occupant changes.
#[derive(Clone, Debug)]
pub struct QueuedFollowUp {
    pub request_id: String,
    pub conversation_id: ConversationId,
    pub agent_ref: AgentRef,
    pub provider: String,
    pub native_session_id: Option<String>,
    /// AC-16: exact-occupant fingerprint over the typed session locator.
    pub session_fingerprint: String,
    pub baseline_revision: u64,
    pub text: String,
    pub created_at: SystemTime,
    pub state: QueueState,
}

/// R3-02: outcome of the atomic delivery claim. `Claimed` carries the bound
/// snapshot + token; the worker must operate on THAT item for the whole
/// transaction.
#[derive(Clone, Debug)]
pub enum DeliveryClaim {
    /// This worker uniquely owns this exact item's delivery.
    Claimed {
        item: Box<QueuedFollowUp>,
        token: DeliveryClaimToken,
    },
    /// Another worker already holds the claim.
    AlreadyClaimed,
    /// No deliverable item exists (missing, delivered, cancelled, …).
    Gone,
}

/// Binds every post-claim operation to the exact logical item that was
/// claimed (its request_id). An A/B item replacement under the same AgentRef
/// invalidates the token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeliveryClaimToken {
    request_id: String,
}

/// Terminal finishes for [`ConversationFollowUpQueue::finish`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryFinish {
    Delivered,
    /// Cancelled / identity-changed: remove without a recoverable trail.
    Dismissed,
    /// Retain as `FailedRecoverable` — visible, recoverable, never retried.
    Failed,
    /// Retain as `DeliveryUncertain` — visible, never auto-retried.
    Uncertain,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueueError {
    /// A follow-up is already queued for this Conversation (v1: exactly one).
    Occupied,
    /// Empty text can never be a semantic prompt.
    EmptyText,
}

impl fmt::Display for QueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Occupied => formatter.write_str("a follow-up is already queued"),
            Self::EmptyText => formatter.write_str("follow-up text must not be empty"),
        }
    }
}

/// Terminal outcome after a delivery attempt; the item is removed from the
/// queue in every case except `RetainedBlocked`, `RetainedTimeout`, and
/// `CancelledBeforeDelivery`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryOutcome {
    Delivered {
        identity: ConversationIdentity,
        /// Delivered text, returned for pending-echo reconciliation.
        text: String,
    },
    /// Agent is Blocked: queue retained, nothing sent.
    RetainedBlocked,
    /// Settle wait timed out: queue retained, nothing sent.
    RetainedTimeout,
    /// The user cancelled while delivery was still waiting for the turn to
    /// settle. Nothing was sent; the text already returned to the caller via
    /// `cancel`.
    CancelledBeforeDelivery,
    /// R2-04: another worker holds the atomic delivery claim; this worker sent
    /// nothing and changed nothing.
    AlreadyClaimed,
    /// Occupant identity changed: nothing sent; text returned to the caller.
    IdentityChanged { text: String },
    /// Definite prompt rejection: nothing further sent; text recoverable.
    Failed { text: String, reason: String },
    /// Acceptance unknown: never auto-retried; text is NOT auto-restored.
    Uncertain { text: String, reason: String },
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// The one-item-per-Conversation semantic follow-up queue.
#[derive(Default)]
pub struct ConversationFollowUpQueue {
    items: Mutex<HashMap<String, QueuedFollowUp>>,
}

impl ConversationFollowUpQueue {
    pub fn new() -> Self {
        Self::default()
    }

    fn key(agent_ref: &AgentRef) -> String {
        agent_ref.as_str().to_string()
    }

    /// Enqueue one follow-up. Fails with [`QueueError::Occupied`] while an
    /// undelivered item exists for the same Conversation.
    pub fn enqueue(&self, item: QueuedFollowUp) -> Result<QueueState, QueueError> {
        if item.text.trim().is_empty() {
            return Err(QueueError::EmptyText);
        }
        let mut items = lock(&self.items);
        let key = Self::key(&item.agent_ref);
        if let Some(existing) = items.get(&key) {
            let terminal_for_old_occupant = matches!(
                existing.state,
                QueueState::FailedRecoverable(_) | QueueState::DeliveryUncertain(_)
            ) && existing.conversation_id != item.conversation_id;
            if !matches!(
                existing.state,
                QueueState::Delivered | QueueState::Cancelled
            ) && !terminal_for_old_occupant
            {
                return Err(QueueError::Occupied);
            }
        }
        let state = item.state.clone();
        items.insert(key, item);
        Ok(state)
    }

    /// Presentation snapshot of the queued item for a Conversation.
    pub fn queued(&self, agent_ref: &AgentRef) -> Option<QueuedFollowUp> {
        lock(&self.items).get(&Self::key(agent_ref)).cloned()
    }

    /// R3-02: atomically claim THE queued item for ONE delivery worker.
    /// `Queued → WaitingForTurnBoundary` is compare-and-set under the queue
    /// mutex and the returned snapshot is BOUND to the claim via its
    /// request_id (the claim token): a cancel-A-then-enqueue-B interleaving
    /// can never let a worker mutate an item it did not claim. Every later
    /// operation ([`Self::begin_delivery`], [`Self::release_claim`],
    /// [`Self::finish`]) must present the same token.
    pub fn claim_for_delivery(&self, agent_ref: &AgentRef) -> DeliveryClaim {
        let mut items = lock(&self.items);
        match items.get_mut(&Self::key(agent_ref)) {
            None => DeliveryClaim::Gone,
            Some(item) => match item.state {
                QueueState::Queued => {
                    item.state = QueueState::WaitingForTurnBoundary;
                    DeliveryClaim::Claimed {
                        item: Box::new(item.clone()),
                        token: DeliveryClaimToken {
                            request_id: item.request_id.clone(),
                        },
                    }
                }
                QueueState::WaitingForTurnBoundary => DeliveryClaim::AlreadyClaimed,
                _ => DeliveryClaim::Gone,
            },
        }
    }

    /// Compare-and-set `WaitingForTurnBoundary → Delivering` for the item
    /// named by the claim token. Returns false when the claimed item was
    /// cancelled/replaced meanwhile — the caller must not send anything.
    pub fn begin_delivery(&self, agent_ref: &AgentRef, token: &DeliveryClaimToken) -> bool {
        let mut items = lock(&self.items);
        match items.get_mut(&Self::key(agent_ref)) {
            Some(item) => {
                item.state == QueueState::WaitingForTurnBoundary
                    && item.request_id == token.request_id
                    && {
                        item.state = QueueState::Delivering;
                        true
                    }
            }
            None => false,
        }
    }

    /// Release a held claim back to `Queued` (transport error mid-transaction,
    /// retained settle result): the item stays durable and another worker may
    /// claim it later. Token-bound: a worker can never release somebody
    /// else's claim.
    pub fn release_claim(&self, agent_ref: &AgentRef, token: &DeliveryClaimToken) {
        let mut items = lock(&self.items);
        if let Some(item) = items.get_mut(&Self::key(agent_ref)) {
            if item.state == QueueState::WaitingForTurnBoundary
                && item.request_id == token.request_id
            {
                item.state = QueueState::Queued;
            }
        }
    }

    /// Terminal finish for the claimed item: DELIVERED/CANCELLED removes it;
    /// FAILED/UNCERTAIN retains it in the matching terminal state so the text
    /// stays visible and recoverable to every client (never auto-retried).
    /// Token-bound: only the worker that owns the logical item may finish it.
    pub fn finish(
        &self,
        agent_ref: &AgentRef,
        token: &DeliveryClaimToken,
        outcome: DeliveryFinish,
        reason: &str,
    ) {
        let mut items = lock(&self.items);
        let Some(item) = items.get_mut(&Self::key(agent_ref)) else {
            return;
        };
        if item.request_id != token.request_id {
            return;
        }
        match outcome {
            DeliveryFinish::Delivered | DeliveryFinish::Dismissed => {
                items.remove(&Self::key(agent_ref));
            }
            DeliveryFinish::Failed => {
                if item.state == QueueState::Delivering {
                    item.state = QueueState::FailedRecoverable(reason.to_string());
                }
            }
            DeliveryFinish::Uncertain => {
                if item.state == QueueState::Delivering {
                    item.state = QueueState::DeliveryUncertain(reason.to_string());
                }
            }
        }
    }

    /// Cancel before delivery begins, or dismiss a terminal Failed/Uncertain
    /// item. Returns the text when the item was cancellable.
    pub fn cancel(&self, agent_ref: &AgentRef) -> Option<String> {
        let mut items = lock(&self.items);
        let key = Self::key(agent_ref);
        match items.get_mut(&key) {
            Some(item)
                if matches!(
                    item.state,
                    QueueState::Queued
                        | QueueState::WaitingForTurnBoundary
                        | QueueState::FailedRecoverable(_)
                        | QueueState::DeliveryUncertain(_)
                ) =>
            {
                let text = item.text.clone();
                // C25: no observable `Cancelled` store — the item is removed
                // immediately, so the dead state write is gone.
                items.remove(&key);
                Some(text)
            }
            _ => None,
        }
    }

    /// Edit (replace) the queued text BEFORE a worker claims it. Edits are
    /// only valid in the unclaimed `Queued` state: a worker's claim snapshot
    /// is authoritative from the claim on, so allowing edits while
    /// `WaitingForTurnBoundary` would send the stale pre-edit text.
    pub fn replace_text(&self, agent_ref: &AgentRef, text: &str) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        let mut items = lock(&self.items);
        match items.get_mut(&Self::key(agent_ref)) {
            Some(item) if item.state == QueueState::Queued => {
                item.text = text.to_string();
                true
            }
            _ => false,
        }
    }

    /// The user repeated text that already exists in the transcript is never
    /// treated as acknowledgement of a new queue item (M4 test contract).
    pub fn matches_pending_text(&self, agent_ref: &AgentRef, text: &str) -> bool {
        self.queued(agent_ref).is_some_and(|item| {
            item.text.trim() == text.trim()
                && matches!(
                    item.state,
                    QueueState::Queued | QueueState::WaitingForTurnBoundary
                )
        })
    }

    /// R7-P0-01/02: check whether any unresolved older operation exists for
    /// the exact ConversationId. An unresolved item in one of these states
    /// must block a newer immediate Prompt and a Handoff snapshot from
    /// overtaking it:
    ///   - Queued / WaitingForTurnBoundary / Delivering: worker is in flight
    ///   - FailedRecoverable / DeliveryUncertain: terminal but user-visible,
    ///     must be reconciled before new operations may proceed
    ///
    /// Delivered / Cancelled items are fully terminal and do not block.
    pub fn has_unresolved_for_conversation(&self, conversation_id: &ConversationId) -> bool {
        let items = lock(&self.items);
        items.values().any(|item| {
            item.conversation_id == *conversation_id
                && matches!(
                    item.state,
                    QueueState::Queued
                        | QueueState::WaitingForTurnBoundary
                        | QueueState::Delivering
                        | QueueState::FailedRecoverable(_)
                        | QueueState::DeliveryUncertain(_)
                )
        })
    }
}

/// Execute the safe delivery transaction for one queued follow-up. Blocking;
/// callers run it on a background thread and may drop the task to stop
/// waiting (the queue item is retained unless a terminal outcome is returned).
/// The atomic delivery claim makes the transaction safe against any number of
/// concurrent workers on the process-shared queue (R2-04/CR-02).
/// Two-phase delivery transaction (P0-02). `begin` claims the item and runs
/// the (potentially long) settle wait WITHOUT holding any conversation
/// serialization; `finish` performs the revalidation + prompt under the
/// caller-provided per-Conversation lock, closing the check→prompt TOCTOU
/// between an immediate submit and a queued delivery.
pub enum DeliveryBegin {
    /// Claim acquired; run [`DeliveryTransaction::finish`] (under the
    /// conversation serialization) to deliver.
    Claimed(DeliveryTransaction),
    /// Another worker holds the claim.
    AlreadyClaimed,
    /// No deliverable item exists.
    Gone,
    /// The settle wait retained the item (blocked/timeout/transport); the
    /// claim was already released.
    Retained(DeliveryOutcome),
}

pub struct DeliveryTransaction {
    queue: Arc<ConversationFollowUpQueue>,
    agent_ref: AgentRef,
    item: QueuedFollowUp,
    token: DeliveryClaimToken,
}

impl DeliveryTransaction {
    /// Phase 1 — atomic claim + bounded settle wait. No conversation
    /// serialization is held: a long turn must not block other submits.
    pub fn begin<R: FollowUpRuntime + ?Sized>(
        queue: Arc<ConversationFollowUpQueue>,
        runtime: &R,
        agent_ref: &AgentRef,
        settle_timeout_ms: u64,
    ) -> DeliveryBegin {
        // R3-02: the claim binds the worker to the exact logical item; a
        // cancel-A/enqueue-B interleaving can never let this worker mutate an
        // item it did not claim.
        let (item, token) = match queue.claim_for_delivery(agent_ref) {
            DeliveryClaim::Claimed { item, token } => (*item, token),
            DeliveryClaim::AlreadyClaimed => return DeliveryBegin::AlreadyClaimed,
            DeliveryClaim::Gone => return DeliveryBegin::Gone,
        };
        let wait = match runtime.wait_sendable(agent_ref, settle_timeout_ms) {
            Ok(outcome) => outcome,
            Err(error) => {
                // C09: a transport failure is not the same signal as a settle
                // timeout — keep it visible so worker retries/logs can
                // distinguish them (the item is retained either way).
                crate::diagnostics::lag_log(format_args!(
                    "follow_up settle wait transport error agent={} error={error}",
                    agent_ref.as_str()
                ));
                queue.release_claim(agent_ref, &token);
                return DeliveryBegin::Retained(DeliveryOutcome::RetainedTimeout);
            }
        };
        match wait {
            WaitOutcome::Blocked => {
                queue.release_claim(agent_ref, &token);
                DeliveryBegin::Retained(DeliveryOutcome::RetainedBlocked)
            }
            WaitOutcome::Timeout => {
                queue.release_claim(agent_ref, &token);
                DeliveryBegin::Retained(DeliveryOutcome::RetainedTimeout)
            }
            WaitOutcome::Sendable => DeliveryBegin::Claimed(DeliveryTransaction {
                queue,
                agent_ref: agent_ref.clone(),
                item,
                token,
            }),
        }
    }

    /// Phase 2 — revalidation + exactly-once prompt. The caller MUST hold the
    /// per-Conversation serialization lock across this call so an immediate
    /// submit on the same conversation cannot interleave between the final
    /// sendability check and the prompt (P0-02).
    pub fn finish<R: FollowUpRuntime + ?Sized>(&self, runtime: &R) -> DeliveryOutcome {
        let agent_ref = &self.agent_ref;
        let item = &self.item;
        let token = &self.token;

        // Revalidate the exact occupant before delivery. R3-05: the occupant
        // must ALSO still be sendable right now — another client could have
        // started a new turn inside the wait→prompt window, and a queued
        // follow-up must never inject a prompt mid-turn.
        let identity = match runtime.agent_identity(agent_ref) {
            Ok(Some(identity)) => identity,
            Ok(None) => {
                self.queue
                    .finish(agent_ref, token, DeliveryFinish::Dismissed, "");
                return DeliveryOutcome::IdentityChanged {
                    text: item.text.clone(),
                };
            }
            Err(_) => {
                self.queue.release_claim(agent_ref, token);
                return DeliveryOutcome::RetainedTimeout;
            }
        };
        let identity_matches = identity.provider == item.provider
            && identity.native_session_id == item.native_session_id
            && identity.session_fingerprint == item.session_fingerprint
            && identity.interactive_ready;
        if !identity_matches {
            self.queue
                .finish(agent_ref, token, DeliveryFinish::Dismissed, "");
            return DeliveryOutcome::IdentityChanged {
                text: item.text.clone(),
            };
        }
        match identity.status {
            crate::dto::AgentStatus::Idle | crate::dto::AgentStatus::Done => {}
            _ => {
                // Mid-turn/blocked again at the final check (under the
                // conversation lock, so this view is authoritative): retain
                // and retry at the next settle instead of sending now.
                self.queue.release_claim(agent_ref, token);
                return DeliveryOutcome::RetainedTimeout;
            }
        }

        // Compare-and-set the claim into Delivering (token-bound). A cancel
        // that landed during the settle wait removed/replaced the item —
        // abort without sending.
        if !self.queue.begin_delivery(agent_ref, token) {
            return DeliveryOutcome::CancelledBeforeDelivery;
        }

        // Deliver exactly once. Past this point cancel/edit must not be
        // presented (a false cancel could tempt a duplicate resend).
        let text = item.text.clone();
        match runtime.prompt(agent_ref, &text) {
            Ok(PromptAcceptance::Accepted) => {
                self.queue
                    .finish(agent_ref, token, DeliveryFinish::Delivered, "");
                DeliveryOutcome::Delivered {
                    identity: ConversationIdentity {
                        conversation_id: item.conversation_id.clone(),
                        agent_ref: item.agent_ref.clone(),
                        provider: identity.provider,
                        native_session_id: identity.native_session_id,
                        revision: identity.revision,
                    },
                    text,
                }
            }
            Ok(PromptAcceptance::Rejected(reason)) => {
                // R3-P1: retain the item as FailedRecoverable — the text
                // stays visible/recoverable to clients instead of vanishing.
                self.queue
                    .finish(agent_ref, token, DeliveryFinish::Failed, &reason);
                DeliveryOutcome::Failed { text, reason }
            }
            Ok(PromptAcceptance::Uncertain(reason)) => {
                self.queue
                    .finish(agent_ref, token, DeliveryFinish::Uncertain, &reason);
                DeliveryOutcome::Uncertain { text, reason }
            }
            Err(error) => {
                self.queue
                    .finish(agent_ref, token, DeliveryFinish::Uncertain, &error);
                DeliveryOutcome::Uncertain {
                    text,
                    reason: error,
                }
            }
        }
    }
}

/// Single-call form of the two-phase transaction for callers that hold no
/// conversation serialization (tests, matrix runs).
pub fn run_follow_up_delivery<R: FollowUpRuntime + ?Sized>(
    queue: &Arc<ConversationFollowUpQueue>,
    runtime: &R,
    agent_ref: &AgentRef,
    settle_timeout_ms: u64,
) -> DeliveryOutcome {
    match DeliveryTransaction::begin(queue.clone(), runtime, agent_ref, settle_timeout_ms) {
        DeliveryBegin::Claimed(transaction) => transaction.finish(runtime),
        DeliveryBegin::AlreadyClaimed => DeliveryOutcome::AlreadyClaimed,
        DeliveryBegin::Gone => DeliveryOutcome::CancelledBeforeDelivery,
        DeliveryBegin::Retained(outcome) => outcome,
    }
}

/// C17: the single raw-status classifier behind EVERY status→sendability
/// projection. `agent_sendability` (the Agent-typed SSOT) and the raw-string
/// presentation projection derive from this one table so their state sets can
/// never drift again (the old handwritten projection omitted `pending` and
/// treated unknown as sendable).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RawStatusClass {
    /// working/pending/launch_pending: mid-turn.
    MidTurn,
    /// blocked/failed: the Terminal owns the interaction.
    NeedsTerminal,
    /// idle/done: settled and sendable.
    Sendable,
    /// Anything else (or an absent field): unproven.
    Unknown,
}

fn classify_raw_status(status: &str) -> RawStatusClass {
    match status.to_lowercase().as_str() {
        "working" | "pending" | "launch_pending" => RawStatusClass::MidTurn,
        "blocked" | "failed" => RawStatusClass::NeedsTerminal,
        "idle" | "done" => RawStatusClass::Sendable,
        _ => RawStatusClass::Unknown,
    }
}

/// R3-04: THE single authoritative sendability projection from a Herdr Agent.
/// Herdr may report status through `agent_status` OR `custom_status`, and
/// `launch_pending` is a typed boolean — every consumer (Host mutation,
/// History, queue final revalidation, handoff eligibility) must classify
/// through this projection instead of re-deriving from one field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentSendability {
    /// idle/done: a semantic prompt may be sent now.
    Sendable,
    /// working/launch_pending: queue after the current turn.
    MidTurn,
    /// blocked/failed: the Terminal owns the interaction.
    NeedsTerminal,
    /// No known status on any field: unproven — mutating is fail-closed.
    Unknown,
}

pub fn agent_sendability(agent: &crate::herdr::Agent) -> AgentSendability {
    if agent.launch_pending {
        return AgentSendability::MidTurn;
    }
    // Herdr can expose both fields during a transition.  They are not
    // fallback alternatives: choosing the first non-null value can turn
    // `agent_status=idle + custom_status=working` into a mid-turn injection.
    // Merge conservatively so every simultaneously-present signal participates
    // in the safety decision (blocked > mid-turn > settled > unknown).
    let statuses = [
        agent.agent_status.as_deref(),
        agent.custom_status.as_deref(),
    ];
    let mut saw_mid_turn = false;
    let mut saw_sendable = false;
    for status in statuses.into_iter().flatten() {
        match classify_raw_status(status) {
            RawStatusClass::NeedsTerminal => return AgentSendability::NeedsTerminal,
            RawStatusClass::MidTurn => saw_mid_turn = true,
            RawStatusClass::Sendable => saw_sendable = true,
            RawStatusClass::Unknown => {}
        }
    }
    if saw_mid_turn {
        AgentSendability::MidTurn
    } else if saw_sendable {
        AgentSendability::Sendable
    } else {
        AgentSendability::Unknown
    }
}

/// Authoritative prompt disposition for a mutation. `Unknown` is an explicit
/// fail-closed policy (R3-04): an agent whose state cannot be proven is never
/// prompted or queued — the client must refresh and retry.
pub fn prompt_disposition_for_agent(
    agent: &crate::herdr::Agent,
) -> Result<PromptDisposition, &'static str> {
    match agent_sendability(agent) {
        AgentSendability::Sendable => Ok(PromptDisposition::SentNow),
        AgentSendability::MidTurn => Ok(PromptDisposition::QueuedAfterTurn),
        AgentSendability::NeedsTerminal => Ok(PromptDisposition::NeedsTerminal),
        AgentSendability::Unknown => {
            Err("the conversation's current agent state is unknown; refresh and try again")
        }
    }
}

/// Presentation-only projection over a raw status string (Desktop button
/// hint). The mutation path MUST use [`prompt_disposition_for_agent`].
/// C17 (user-approved fail-closed semantics): this derives from the SAME
/// classifier as [`agent_sendability`] instead of a handwritten state set,
/// and an unknown/absent status is NOT sendable — it routes to the Terminal
/// rather than pretending `SentNow`.
pub fn herdr_prompt_disposition(status: Option<&str>) -> PromptDisposition {
    match status.map(classify_raw_status) {
        Some(RawStatusClass::Sendable) => PromptDisposition::SentNow,
        Some(RawStatusClass::MidTurn) => PromptDisposition::QueuedAfterTurn,
        Some(RawStatusClass::NeedsTerminal) | Some(RawStatusClass::Unknown) | None => {
            PromptDisposition::NeedsTerminal
        }
    }
}

// C22: `working_prompt_disposition` (dto-status twin of the above, zero
// callers outside its own test) was deleted.

/// Production adapter over the live Herdr client.
pub struct HerdrFollowUpRuntime<'a> {
    client: &'a HerdrClient,
}

impl<'a> HerdrFollowUpRuntime<'a> {
    pub fn new(client: &'a HerdrClient) -> Self {
        Self { client }
    }
}

fn herdr_agent_by_target(
    client: &HerdrClient,
    agent_ref: &str,
) -> Result<Option<crate::herdr::Agent>, String> {
    // C15: the shared pane-id/terminal-id lookup; only the error flavor
    // differs from the conversation-service caller.
    crate::conversations::find_agent_by_ref(client, agent_ref).map_err(|error| error.to_string())
}

impl FollowUpRuntime for HerdrFollowUpRuntime<'_> {
    fn wait_sendable(&self, agent: &AgentRef, timeout_ms: u64) -> Result<WaitOutcome, String> {
        let settled = self.client.wait_for_agent(&AgentWaitParams {
            target: agent.to_string(),
            until: vec![
                HerdrAgentStatus::Idle,
                HerdrAgentStatus::Done,
                HerdrAgentStatus::Blocked,
            ],
            timeout_ms: Some(timeout_ms),
        });
        let settled = match settled {
            Ok(agent) => agent,
            Err(error) => return Err(error.to_string()),
        };
        match crate::agent_sendability(&settled) {
            crate::AgentSendability::Sendable => Ok(WaitOutcome::Sendable),
            crate::AgentSendability::NeedsTerminal => Ok(WaitOutcome::Blocked),
            crate::AgentSendability::MidTurn | crate::AgentSendability::Unknown => {
                Ok(WaitOutcome::Timeout)
            }
        }
    }

    fn agent_identity(&self, agent: &AgentRef) -> Result<Option<FollowUpIdentity>, String> {
        let Some(found) = herdr_agent_by_target(self.client, agent.as_str())? else {
            return Ok(None);
        };
        let session: Option<&AgentSessionInfo> = found.agent_session.as_ref();
        // C14: the shared provider-label derivation.
        let provider = session
            .map(|session| crate::conversations::agent_provider_label(&found, session))
            .unwrap_or_else(|| found.agent.clone().unwrap_or_else(|| "unknown".into()));
        let native_session_id = session
            .and_then(|session| crate::public_native_session_id(session).ok())
            .flatten();
        let session_fingerprint = session.map(crate::session_fingerprint).unwrap_or_default();
        // P0-01: final revalidation classifies through the SAME Agent-typed
        // sendability SSOT (launch_pending + agent_status + custom_status);
        // reading only agent_status let a launch_pending agent pass as Idle
        // and re-opened the mid-turn injection window.
        let status = match crate::agent_sendability(&found) {
            crate::AgentSendability::Sendable => crate::dto::AgentStatus::Idle,
            crate::AgentSendability::MidTurn => crate::dto::AgentStatus::Working,
            crate::AgentSendability::NeedsTerminal => crate::dto::AgentStatus::Blocked,
            crate::AgentSendability::Unknown => crate::dto::AgentStatus::Unknown,
        };
        Ok(Some(FollowUpIdentity {
            provider,
            native_session_id,
            session_fingerprint,
            revision: found.revision,
            interactive_ready: found.interactive_ready,
            status,
        }))
    }

    fn prompt(&self, agent: &AgentRef, text: &str) -> Result<PromptAcceptance, String> {
        // Submit confirmation: herdr's prompt Enter is delayed; the wait keeps
        // it attached to this connection and classifies a swallowed submit as
        // a definite rejection rather than a silent no-op.
        match self.client.prompt_agent_confirmed(&AgentPromptParams {
            target: agent.to_string(),
            text: text.to_string(),
            wait: Some(crate::herdr::AgentPromptWaitOptions {
                until: vec![
                    HerdrAgentStatus::Working,
                    HerdrAgentStatus::Done,
                    HerdrAgentStatus::Blocked,
                ],
                timeout_ms: Some(15_000),
            }),
        }) {
            Ok(_) => Ok(PromptAcceptance::Accepted),
            Err(HerdrError::Api(message)) => Ok(PromptAcceptance::Rejected(message)),
            Err(HerdrError::AgentNotFound(message)) => Ok(PromptAcceptance::Rejected(message)),
            // Herdr rejects blocked agents BEFORE any input is sent — a
            // definite recoverable rejection, not "may have been delivered".
            Err(HerdrError::AgentBlocked(message)) => Ok(PromptAcceptance::Rejected(message)),
            Err(HerdrError::AgentPromptStalled(message)) => Ok(PromptAcceptance::Rejected(message)),
            Err(error) => Ok(PromptAcceptance::Uncertain(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_id_for_live_agent;
    use std::cell::RefCell;

    #[derive(Default)]
    struct MockFollowUpRuntime {
        wait_outcome: WaitOutcome,
        wait_error: Option<String>,
        identity: Option<FollowUpIdentity>,
        prompt_acceptance: PromptAcceptance,
        wait_calls: RefCell<u8>,
        prompt_calls: RefCell<u8>,
        prompt_texts: RefCell<Vec<String>>,
    }

    impl FollowUpRuntime for MockFollowUpRuntime {
        fn wait_sendable(
            &self,
            _agent: &AgentRef,
            _timeout_ms: u64,
        ) -> Result<WaitOutcome, String> {
            self.wait_calls.replace_with(|count| *count + 1);
            match self.wait_error.clone() {
                Some(error) => Err(error),
                None => Ok(self.wait_outcome),
            }
        }

        fn agent_identity(&self, _agent: &AgentRef) -> Result<Option<FollowUpIdentity>, String> {
            Ok(self.identity.clone())
        }

        fn prompt(&self, _agent: &AgentRef, text: &str) -> Result<PromptAcceptance, String> {
            self.prompt_calls.replace_with(|count| *count + 1);
            self.prompt_texts.borrow_mut().push(text.to_string());
            Ok(self.prompt_acceptance.clone())
        }
    }

    fn identity() -> FollowUpIdentity {
        FollowUpIdentity {
            provider: "claude".into(),
            native_session_id: Some("native-1".into()),
            session_fingerprint: "fingerprint-1".into(),
            revision: 9,
            interactive_ready: true,
            status: crate::dto::AgentStatus::Idle,
        }
    }

    fn item(text: &str) -> QueuedFollowUp {
        let agent_ref = AgentRef::new("pane-1");
        QueuedFollowUp {
            request_id: "req-1".into(),
            conversation_id: conversation_id_for_live_agent(&agent_ref),
            agent_ref,
            provider: "claude".into(),
            native_session_id: Some("native-1".into()),
            session_fingerprint: "fingerprint-1".into(),
            baseline_revision: 7,
            text: text.into(),
            created_at: SystemTime::now(),
            state: QueueState::Queued,
        }
    }

    fn mock() -> MockFollowUpRuntime {
        MockFollowUpRuntime {
            wait_outcome: WaitOutcome::Sendable,
            identity: Some(identity()),
            prompt_acceptance: PromptAcceptance::Accepted,
            ..Default::default()
        }
    }

    #[test]
    fn working_queue_sends_exactly_once_after_settle() {
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("next step"))
            .unwrap_or_else(|e| panic!("{e}"));
        let runtime = mock();
        let outcome = run_follow_up_delivery(&queue, &runtime, &agent, 1_000);
        match outcome {
            DeliveryOutcome::Delivered { identity, text } => {
                assert_eq!(identity.native_session_id.as_deref(), Some("native-1"));
                assert_eq!(text, "next step");
            }
            other => panic!("expected delivery, got {other:?}"),
        }
        assert_eq!(*runtime.prompt_calls.borrow(), 1);
        assert_eq!(runtime.prompt_texts.borrow()[0], "next step");
        assert!(queue.queued(&agent).is_none(), "delivered item is removed");
    }

    #[test]
    fn blocked_retains_the_queue_without_prompting() {
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("later"))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut runtime = mock();
        runtime.wait_outcome = WaitOutcome::Blocked;
        let outcome = run_follow_up_delivery(&queue, &runtime, &agent, 1_000);
        assert_eq!(outcome, DeliveryOutcome::RetainedBlocked);
        assert_eq!(*runtime.prompt_calls.borrow(), 0);
        assert!(queue.queued(&agent).is_some(), "blocked keeps the queue");
    }

    #[test]
    fn timeout_retains_the_queue_without_prompting() {
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("later"))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut runtime = mock();
        runtime.wait_outcome = WaitOutcome::Timeout;
        let outcome = run_follow_up_delivery(&queue, &runtime, &agent, 1_000);
        assert_eq!(outcome, DeliveryOutcome::RetainedTimeout);
        assert_eq!(*runtime.prompt_calls.borrow(), 0);
        assert!(queue.queued(&agent).is_some());
    }

    #[test]
    fn identity_change_fails_closed_and_returns_the_text() {
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("careful"))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut runtime = mock();
        let mut changed = identity();
        changed.native_session_id = Some("someone-else".into());
        runtime.identity = Some(changed);
        let outcome = run_follow_up_delivery(&queue, &runtime, &agent, 1_000);
        assert_eq!(
            outcome,
            DeliveryOutcome::IdentityChanged {
                text: "careful".into()
            }
        );
        assert_eq!(*runtime.prompt_calls.borrow(), 0);
        assert!(queue.queued(&agent).is_none());
    }

    #[test]
    fn path_backed_replacement_occupant_fails_closed_via_fingerprint() {
        // AC-16: for path-backed providers the public native id is None on both
        // occupants, so only the opaque session fingerprint can discriminate.
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("path-backed"))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut runtime = mock();
        let mut replacement = identity();
        replacement.native_session_id = None;
        replacement.session_fingerprint = "fingerprint-of-a-new-occupant".into();
        runtime.identity = Some(replacement);
        let outcome = run_follow_up_delivery(&queue, &runtime, &agent, 1_000);
        assert!(matches!(outcome, DeliveryOutcome::IdentityChanged { .. }));
        assert_eq!(*runtime.prompt_calls.borrow(), 0);
    }

    #[test]
    fn occupant_disappearing_fails_closed() {
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("gone"))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut runtime = mock();
        runtime.identity = None;
        let outcome = run_follow_up_delivery(&queue, &runtime, &agent, 1_000);
        assert!(matches!(outcome, DeliveryOutcome::IdentityChanged { .. }));
        assert_eq!(*runtime.prompt_calls.borrow(), 0);
    }

    #[test]
    fn non_interactive_identity_is_not_sendable() {
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("hold"))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut runtime = mock();
        let mut not_ready = identity();
        not_ready.interactive_ready = false;
        runtime.identity = Some(not_ready);
        let outcome = run_follow_up_delivery(&queue, &runtime, &agent, 1_000);
        assert!(matches!(outcome, DeliveryOutcome::IdentityChanged { .. }));
        assert_eq!(*runtime.prompt_calls.borrow(), 0);
    }

    #[test]
    fn definite_prompt_failure_is_recoverable_without_retry() {
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("retry me"))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut runtime = mock();
        runtime.prompt_acceptance = PromptAcceptance::Rejected("prompt unavailable".into());
        let outcome = run_follow_up_delivery(&queue, &runtime, &agent, 1_000);
        assert_eq!(
            outcome,
            DeliveryOutcome::Failed {
                text: "retry me".into(),
                reason: "prompt unavailable".into()
            }
        );
        assert_eq!(*runtime.prompt_calls.borrow(), 1, "one attempt, no retry");
        // R3-P1: failed deliveries retain the text as FailedRecoverable.
        let retained = queue
            .queued(&agent)
            .unwrap_or_else(|| panic!("failed item must stay visible"));
        assert!(matches!(retained.state, QueueState::FailedRecoverable(_)));
    }

    #[test]
    fn uncertain_acceptance_never_auto_retries() {
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("maybe"))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut runtime = mock();
        runtime.prompt_acceptance = PromptAcceptance::Uncertain("socket dropped".into());
        let outcome = run_follow_up_delivery(&queue, &runtime, &agent, 1_000);
        assert!(matches!(outcome, DeliveryOutcome::Uncertain { .. }));
        assert_eq!(*runtime.prompt_calls.borrow(), 1);
        // R3-P1: uncertain outcomes RETAIN the item in a terminal state —
        // visible and recoverable to clients, never auto-retried.
        let retained = queue
            .queued(&agent)
            .unwrap_or_else(|| panic!("uncertain item must stay visible"));
        assert!(matches!(retained.state, QueueState::DeliveryUncertain(_)));
        // Dismissing the terminal item is a user action.
        assert_eq!(queue.cancel(&agent), Some("maybe".into()));
        assert!(queue.queued(&agent).is_none());
    }

    #[test]
    fn only_one_follow_up_per_conversation() {
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("first"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(queue.enqueue(item("second")), Err(QueueError::Occupied));
        assert!(queue.replace_text(&agent, "edited"));
        assert_eq!(
            queue.queued(&agent).map(|item| item.text),
            Some("edited".into())
        );
    }

    #[test]
    fn cancel_before_delivery_removes_without_prompting() {
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("undo"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(queue.cancel(&agent), Some("undo".into()));
        assert!(queue.queued(&agent).is_none());
        let runtime = mock();
        let outcome = run_follow_up_delivery(&queue, &runtime, &agent, 1_000);
        assert_eq!(outcome, DeliveryOutcome::CancelledBeforeDelivery);
        assert_eq!(*runtime.prompt_calls.borrow(), 0);
    }

    #[test]
    fn cancel_during_the_settle_wait_prevents_delivery() {
        use std::sync::Arc;
        use std::sync::Condvar;
        use std::sync::Mutex;

        /// Holds the settle wait until the test releases it, simulating a
        /// Working agent whose turn has not finished.
        struct GatedRuntime {
            settled: Mutex<bool>,
            settled_cv: Condvar,
            prompt_calls: std::sync::atomic::AtomicU8,
        }

        impl FollowUpRuntime for GatedRuntime {
            fn wait_sendable(
                &self,
                _agent: &AgentRef,
                _timeout_ms: u64,
            ) -> Result<WaitOutcome, String> {
                let mut settled = self.settled.lock().unwrap_or_else(|p| p.into_inner());
                while !*settled {
                    settled = self
                        .settled_cv
                        .wait(settled)
                        .unwrap_or_else(|p| p.into_inner());
                }
                Ok(WaitOutcome::Sendable)
            }

            fn agent_identity(
                &self,
                _agent: &AgentRef,
            ) -> Result<Option<FollowUpIdentity>, String> {
                Ok(Some(identity()))
            }

            fn prompt(&self, _agent: &AgentRef, _text: &str) -> Result<PromptAcceptance, String> {
                self.prompt_calls
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(PromptAcceptance::Accepted)
            }
        }

        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("racing cancel"))
            .unwrap_or_else(|e| panic!("{e}"));
        let runtime = Arc::new(GatedRuntime {
            settled: Mutex::new(false),
            settled_cv: Condvar::new(),
            prompt_calls: std::sync::atomic::AtomicU8::new(0),
        });
        let delivery_runtime = runtime.clone();
        let delivery_queue = queue.clone();
        let delivery_agent = agent.clone();
        let handle = std::thread::spawn(move || {
            run_follow_up_delivery(&delivery_queue, &*delivery_runtime, &delivery_agent, 1_000)
        });

        // Cancel while the delivery is parked in the settle wait.
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(queue.cancel(&agent), Some("racing cancel".into()));

        // Let the agent settle; the delivery must abort instead of prompting.
        {
            let mut settled = runtime.settled.lock().unwrap_or_else(|p| p.into_inner());
            *settled = true;
        }
        runtime.settled_cv.notify_all();
        let outcome = handle.join().unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(outcome, DeliveryOutcome::CancelledBeforeDelivery);
        assert_eq!(
            runtime
                .prompt_calls
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    }

    #[test]
    fn eight_concurrent_workers_produce_exactly_one_prompt() {
        // R2-04/CR-02: the process-shared queue plus an atomic claim must
        // collapse any number of racing delivery workers into one prompt.
        use std::sync::atomic::{AtomicU8, Ordering};
        use std::sync::Arc;

        struct RacingRuntime {
            prompt_calls: AtomicU8,
        }

        impl FollowUpRuntime for RacingRuntime {
            fn wait_sendable(
                &self,
                _agent: &AgentRef,
                _timeout_ms: u64,
            ) -> Result<WaitOutcome, String> {
                // Every worker parks briefly so all eight race the claim.
                std::thread::sleep(std::time::Duration::from_millis(30));
                Ok(WaitOutcome::Sendable)
            }

            fn agent_identity(
                &self,
                _agent: &AgentRef,
            ) -> Result<Option<FollowUpIdentity>, String> {
                Ok(Some(identity()))
            }

            fn prompt(&self, _agent: &AgentRef, _text: &str) -> Result<PromptAcceptance, String> {
                self.prompt_calls.fetch_add(1, Ordering::SeqCst);
                Ok(PromptAcceptance::Accepted)
            }
        }

        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("race me"))
            .unwrap_or_else(|e| panic!("{e}"));
        let runtime = Arc::new(RacingRuntime {
            prompt_calls: AtomicU8::new(0),
        });
        let mut handles = Vec::new();
        for _ in 0..8 {
            let queue = queue.clone();
            let runtime = runtime.clone();
            let agent = agent.clone();
            handles.push(std::thread::spawn(move || {
                run_follow_up_delivery(&queue, &*runtime, &agent, 1_000)
            }));
        }
        let mut delivered = 0;
        let mut already_claimed = 0;
        for handle in handles {
            match handle.join().unwrap_or_else(|error| panic!("{error:?}")) {
                DeliveryOutcome::Delivered { .. } => delivered += 1,
                DeliveryOutcome::AlreadyClaimed => already_claimed += 1,
                other => panic!("unexpected racing outcome: {other:?}"),
            }
        }
        assert_eq!(delivered, 1, "exactly one worker delivers");
        assert_eq!(already_claimed, 7, "every other worker observes the claim");
        assert_eq!(runtime.prompt_calls.load(Ordering::SeqCst), 1);
        assert!(queue.queued(&agent).is_none());
    }

    #[test]
    fn cancel_then_requeue_race_never_touches_the_replacement_item() {
        // R3-02: a worker that read item A must never operate on a
        // replacement item B enqueued under the same AgentRef. The claim
        // token binds every operation to the exact logical item.
        use std::sync::atomic::{AtomicU8, Ordering};
        use std::sync::{Arc, Condvar, Mutex};

        struct GatedCancelRaceRuntime {
            /// Blocks the settle wait until the test has swapped A for B.
            release: Mutex<bool>,
            cv: Condvar,
            prompt_calls: AtomicU8,
        }

        impl FollowUpRuntime for GatedCancelRaceRuntime {
            fn wait_sendable(
                &self,
                _agent: &AgentRef,
                _timeout_ms: u64,
            ) -> Result<WaitOutcome, String> {
                let mut release = self.release.lock().unwrap_or_else(|p| p.into_inner());
                while !*release {
                    release = self.cv.wait(release).unwrap_or_else(|p| p.into_inner());
                }
                Ok(WaitOutcome::Sendable)
            }

            fn agent_identity(
                &self,
                _agent: &AgentRef,
            ) -> Result<Option<FollowUpIdentity>, String> {
                Ok(Some(identity()))
            }

            fn prompt(&self, _agent: &AgentRef, _text: &str) -> Result<PromptAcceptance, String> {
                self.prompt_calls.fetch_add(1, Ordering::SeqCst);
                Ok(PromptAcceptance::Accepted)
            }
        }

        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("item A"))
            .unwrap_or_else(|e| panic!("{e}"));
        let runtime = Arc::new(GatedCancelRaceRuntime {
            release: Mutex::new(false),
            cv: Condvar::new(),
            prompt_calls: AtomicU8::new(0),
        });
        let worker_queue = Arc::clone(&queue);
        let worker_runtime = Arc::clone(&runtime);
        let worker_agent = agent.clone();
        let worker = std::thread::spawn(move || {
            run_follow_up_delivery(&worker_queue, &*worker_runtime, &worker_agent, 5_000)
        });
        // Worker is parked in the settle wait holding item A's claim. Swap
        // the item under it: cancel A, enqueue B.
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(queue.cancel(&agent), Some("item A".into()));
        queue
            .enqueue(item("item B"))
            .unwrap_or_else(|e| panic!("{e}"));
        {
            let mut release = runtime.release.lock().unwrap_or_else(|p| p.into_inner());
            *release = true;
        }
        runtime.cv.notify_all();
        let outcome = worker.join().unwrap_or_else(|error| panic!("{error:?}"));
        // The worker's token pointed at A; B replaced it — no prompt, no
        // state damage, and B stays pristine-Queued for its own owner.
        assert!(
            matches!(outcome, DeliveryOutcome::CancelledBeforeDelivery),
            "worker must abort on its own item's disappearance: {outcome:?}"
        );
        assert_eq!(runtime.prompt_calls.load(Ordering::SeqCst), 0);
        let retained = queue.queued(&agent).unwrap_or_else(|| panic!("B survives"));
        assert_eq!(retained.text, "item B");
        assert_eq!(retained.state, QueueState::Queued);
    }

    #[test]
    fn repeated_transcript_text_is_not_queue_acknowledgement() {
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(item("same words"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(queue.matches_pending_text(&agent, "same words"));
        assert!(!queue.matches_pending_text(&agent, "different"));
        // Delivered/cancelled items never acknowledge anything.
        queue.cancel(&agent);
        assert!(!queue.matches_pending_text(&agent, "same words"));
    }

    #[test]
    fn empty_text_is_rejected_at_enqueue() {
        let queue = ConversationFollowUpQueue::new();
        assert_eq!(queue.enqueue(item("   ")), Err(QueueError::EmptyText));
    }

    #[test]
    fn agent_sendability_covers_every_status_field() {
        use crate::herdr::Agent;
        // launch_pending is a typed boolean and wins over any string status.
        let mut pending = Agent {
            ..Default::default()
        };
        pending.launch_pending = true;
        pending.agent_status = Some("idle".into());
        assert_eq!(agent_sendability(&pending), AgentSendability::MidTurn);

        // A working custom status remains authoritative when it is the only
        // signal present.
        let mut custom_working = Agent {
            ..Default::default()
        };
        custom_working.custom_status = Some("working".into());
        assert_eq!(
            agent_sendability(&custom_working),
            AgentSendability::MidTurn
        );

        // Both fields can coexist during a Herdr transition.  Never let the
        // idle/done field mask a working or blocked signal.
        let mut conflicting_working = Agent {
            agent_status: Some("idle".into()),
            custom_status: Some("working".into()),
            ..Default::default()
        };
        assert_eq!(
            agent_sendability(&conflicting_working),
            AgentSendability::MidTurn
        );
        conflicting_working.agent_status = Some("done".into());
        assert_eq!(
            agent_sendability(&conflicting_working),
            AgentSendability::MidTurn
        );

        let conflicting_blocked = Agent {
            agent_status: Some("idle".into()),
            custom_status: Some("blocked".into()),
            ..Default::default()
        };
        assert_eq!(
            agent_sendability(&conflicting_blocked),
            AgentSendability::NeedsTerminal
        );

        let conflicting_idle = Agent {
            agent_status: Some("working".into()),
            custom_status: Some("idle".into()),
            ..Default::default()
        };
        assert_eq!(
            agent_sendability(&conflicting_idle),
            AgentSendability::MidTurn
        );

        let mut blocked = Agent {
            ..Default::default()
        };
        blocked.agent_status = Some("blocked".into());
        assert_eq!(agent_sendability(&blocked), AgentSendability::NeedsTerminal);

        let mut idle = Agent {
            ..Default::default()
        };
        idle.agent_status = Some("done".into());
        assert_eq!(agent_sendability(&idle), AgentSendability::Sendable);

        // Unknown is explicit: never guessed into a mutation path.
        let unknown = Agent {
            ..Default::default()
        };
        assert_eq!(agent_sendability(&unknown), AgentSendability::Unknown);
        assert!(prompt_disposition_for_agent(&unknown).is_err());
        assert_eq!(
            prompt_disposition_for_agent(&idle),
            Ok(PromptDisposition::SentNow)
        );
    }

    #[test]
    fn raw_status_disposition_matches_the_desktop_capability_seam() {
        use super::herdr_prompt_disposition;
        // C17: the projection shares the SSOT classifier — mid-turn states
        // queue (including `pending`, previously omitted), blocked/failed
        // need the Terminal, idle/done send now, and anything UNKNOWN is
        // fail-closed NeedsTerminal (user-approved: Chat must not allow
        // sending when the agent state is unknown).
        assert_eq!(
            herdr_prompt_disposition(Some("working")),
            PromptDisposition::QueuedAfterTurn
        );
        assert_eq!(
            herdr_prompt_disposition(Some("pending")),
            PromptDisposition::QueuedAfterTurn
        );
        assert_eq!(
            herdr_prompt_disposition(Some("launch_pending")),
            PromptDisposition::QueuedAfterTurn
        );
        assert_eq!(
            herdr_prompt_disposition(Some("blocked")),
            PromptDisposition::NeedsTerminal
        );
        assert_eq!(
            herdr_prompt_disposition(Some("failed")),
            PromptDisposition::NeedsTerminal
        );
        assert_eq!(
            herdr_prompt_disposition(Some("idle")),
            PromptDisposition::SentNow
        );
        assert_eq!(
            herdr_prompt_disposition(Some("done")),
            PromptDisposition::SentNow
        );
        // Fail-closed: absent and unrecognizable statuses never send now.
        assert_eq!(
            herdr_prompt_disposition(None),
            PromptDisposition::NeedsTerminal
        );
        assert_eq!(
            herdr_prompt_disposition(Some("mystery-state")),
            PromptDisposition::NeedsTerminal
        );
        // Case-insensitive like the SSOT merge.
        assert_eq!(
            herdr_prompt_disposition(Some("WORKING")),
            PromptDisposition::QueuedAfterTurn
        );
    }

    #[test]
    fn raw_string_projection_agrees_with_the_agent_typed_ssot() {
        // C17: for every raw status the presentation projection must agree
        // with the Agent-typed fail-closed SSOT — including the unknown
        // states, which route to the Terminal on both paths.
        use super::herdr_prompt_disposition;
        for status in [
            "working",
            "pending",
            "launch_pending",
            "blocked",
            "failed",
            "idle",
            "done",
            "mystery-state",
        ] {
            let agent = crate::herdr::Agent {
                agent_status: Some(status.into()),
                ..Default::default()
            };
            let typed = prompt_disposition_for_agent(&agent);
            let raw = herdr_prompt_disposition(Some(status));
            match typed {
                Ok(disposition) => assert_eq!(raw, disposition, "status {status:?}"),
                Err(_) => assert_eq!(
                    raw,
                    PromptDisposition::NeedsTerminal,
                    "unknown status {status:?} must be fail-closed"
                ),
            }
        }
    }

    // R7-P0-01: verify has_unresolved_for_conversation blocks ordering gaps.
    #[test]
    fn has_unresolved_blocks_while_queued() {
        let queue = ConversationFollowUpQueue::default();
        let agent_ref = AgentRef::new("pane-a");
        let conv_id = conversation_id_for_live_agent(&agent_ref);
        let other_ref = AgentRef::new("pane-b");
        let other_conv_id = conversation_id_for_live_agent(&other_ref);

        // Nothing queued: no unresolved.
        assert!(!queue.has_unresolved_for_conversation(&conv_id));

        // Queue an item for conversation A.
        let mut follow_up = item("text-a");
        follow_up.agent_ref = agent_ref.clone();
        follow_up.conversation_id = conv_id.clone();
        queue
            .enqueue(follow_up)
            .unwrap_or_else(|error| panic!("enqueue must succeed on empty slot: {error:?}"));
        assert!(
            queue.has_unresolved_for_conversation(&conv_id),
            "Queued state must be treated as unresolved"
        );

        // A different conversation must not be affected.
        assert!(
            !queue.has_unresolved_for_conversation(&other_conv_id),
            "Unrelated conversations must remain independent"
        );
    }

    #[test]
    fn has_unresolved_clears_after_cancel() {
        let queue = ConversationFollowUpQueue::default();
        let agent_ref = AgentRef::new("pane-c");
        let conv_id = conversation_id_for_live_agent(&agent_ref);
        let mut follow_up = item("uncertain-text");
        follow_up.agent_ref = agent_ref.clone();
        follow_up.conversation_id = conv_id.clone();
        queue
            .enqueue(follow_up)
            .unwrap_or_else(|error| panic!("enqueue must succeed on empty slot: {error:?}"));
        assert!(queue.has_unresolved_for_conversation(&conv_id));

        // Cancel clears the item.
        queue.cancel(&agent_ref);
        assert!(
            !queue.has_unresolved_for_conversation(&conv_id),
            "Cancelled state must not block subsequent operations"
        );
    }

    #[test]
    fn has_unresolved_independent_conversations_do_not_interfere() {
        let queue = ConversationFollowUpQueue::default();
        let ref_a = AgentRef::new("pane-x");
        let conv_a = conversation_id_for_live_agent(&ref_a);
        let ref_b = AgentRef::new("pane-y");
        let conv_b = conversation_id_for_live_agent(&ref_b);

        let mut item_a = item("a");
        item_a.agent_ref = ref_a.clone();
        item_a.conversation_id = conv_a.clone();
        item_a.request_id = "req-a".into();
        queue
            .enqueue(item_a)
            .unwrap_or_else(|error| panic!("enqueue a: {error:?}"));

        // Conv B is still clear.
        assert!(!queue.has_unresolved_for_conversation(&conv_b));

        queue.cancel(&ref_a);
        assert!(!queue.has_unresolved_for_conversation(&conv_a));

        // Conv B remains clear throughout.
        assert!(!queue.has_unresolved_for_conversation(&conv_b));
    }
}
