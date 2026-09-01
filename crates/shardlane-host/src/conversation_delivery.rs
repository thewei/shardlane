//! Host-owned queued follow-up delivery coordinator (R2-03/CR-01, R3-03/06).
//!
//! [INPUT]: the process-shared [`ConversationFollowUpQueue`] and a Herdr
//! socket resolution policy (default discovery or an explicit override).
//! [OUTPUT]: `ConversationDeliveryCoordinator` — on enqueue the Host process
//! arranges exactly one delivery worker (token claim → settle wait →
//! identity/sendable re-verification → exactly one prompt; Blocked/Timeout
//! keep the retry); and provides the Host-level `MutationLedger` (Prompt
//! claim/replay) and `LaunchOperationLedger` (Agent launch claim/replay).
//! No client (Desktop Chat, History Continue, Remote/Mobile prompt) needs to
//! or can drive delivery itself.
//! [POS]: R3-03: the worker's exit decision (no successor item + marker
//! removal) completes atomically inside the same gate lock, and schedule
//! takes that same lock after enqueue — a successor item cannot be
//! abandoned. R3-06: the default retry budget is unbounded — while an item
//! exists the Host always has one owner (the worker parks on Blocked; once
//! the user clears it via Terminal, delivery resumes without any client
//! involvement).

use crate::conversation_queue::FollowUpRuntime;
use crate::conversation_queue::{
    ConversationFollowUpQueue, DeliveryBegin, DeliveryOutcome, DeliveryTransaction,
    HerdrFollowUpRuntime,
};
use crate::diagnostics::lag_log;
use crate::dto::ConversationIdentity;
use crate::herdr::HerdrClient;
use crate::ids::AgentRef;
use crate::services::PromptDisposition;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::time::Instant;

const SETTLE_TIMEOUT_MS: u64 = 45_000;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(10);

/// Process-level delivery owner for queued semantic follow-ups. Construct one
/// per Host process (GUI main), inject it everywhere the queue used to be
/// injected, and never run `run_follow_up_delivery` from a client surface.
pub struct ConversationDeliveryCoordinator {
    queue: Arc<ConversationFollowUpQueue>,
    socket_override: Option<PathBuf>,
    /// R3-03: active-worker gate. The exit decision ("no successor item" +
    /// "remove marker") is ATOMIC under this lock, and `schedule` checks the
    /// gate under the same lock AFTER the item is enqueued — a successor item
    /// can therefore never slip between a dying worker and a no-op schedule.
    gate: Mutex<HashMap<String, ()>>,
    /// P0-02: per-Conversation mutation serialization. The authoritative
    /// submit decision AND the queued delivery's final recheck→prompt both run
    /// under this lock, closing the check→prompt TOCTOU between an immediate
    /// submit and a queued delivery on the same conversation.
    serializers: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// P0-03: Host-level semantic mutation ledger (process-wide, shared by
    /// Desktop/Remote/History). A stable logical request id replays the first
    /// outcome — including a delivery-uncertain tombstone — instead of
    /// re-executing the prompt.
    ledger: MutationLedger,
    /// Host launch-operation ledger shared by Desktop/Remote/History launch
    /// paths; committed target structures are replayable by operation id.
    launch_ledger: LaunchOperationLedger,
    retry_delay: Duration,
    /// R3-06: default is UNBOUNDED — while a deliverable item exists the Host
    /// keeps exactly one worker owning its lifecycle. Tests may bound it.
    max_attempts: u32,
    #[cfg(test)]
    test_runtime: Option<Arc<dyn FollowUpRuntime + Send + Sync>>,
}

fn lock_workers(
    workers: &Mutex<HashMap<String, ()>>,
) -> std::sync::MutexGuard<'_, HashMap<String, ()>> {
    workers.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Whether the queue still holds a deliverable (non-terminal, undelivered)
/// item for the conversation — the exit gate's successor check.
fn deliverable_item_exists(queue: &ConversationFollowUpQueue, agent_ref: &AgentRef) -> bool {
    queue.queued(agent_ref).is_some_and(|item| {
        matches!(
            item.state,
            crate::conversation_queue::QueueState::Queued
                | crate::conversation_queue::QueueState::WaitingForTurnBoundary
        )
    })
}

impl ConversationDeliveryCoordinator {
    pub fn new(
        queue: Arc<ConversationFollowUpQueue>,
        socket_override: Option<PathBuf>,
    ) -> Arc<Self> {
        Arc::new(Self {
            queue,
            socket_override,
            gate: Mutex::new(HashMap::new()),
            serializers: Mutex::new(HashMap::new()),
            ledger: MutationLedger::new(),
            launch_ledger: LaunchOperationLedger::new(),
            retry_delay: DEFAULT_RETRY_DELAY,
            max_attempts: u32::MAX,
            #[cfg(test)]
            test_runtime: None,
        })
    }

    /// The process-shared queue this coordinator owns delivery for.
    pub fn queue(&self) -> &Arc<ConversationFollowUpQueue> {
        &self.queue
    }

    /// P0-02: the per-Conversation mutation serialization lock.
    pub fn conversation_lock(&self, agent_ref: &AgentRef) -> Arc<Mutex<()>> {
        let key = agent_ref.as_str().to_string();
        let mut serializers = self
            .serializers
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        // A pane can host an unbounded sequence of Agents over the lifetime of
        // a Host.  Drop map-only serializer slots once no caller and no queued
        // item still references them; active operation Arcs naturally keep a
        // slot alive across this pruning pass.
        serializers.retain(|key, slot| {
            Arc::strong_count(slot) > 1 || self.queue.queued(&AgentRef::new(key.clone())).is_some()
        });
        serializers
            .entry(key)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// P0-03: the Host-level semantic mutation ledger.
    pub fn ledger(&self) -> &MutationLedger {
        &self.ledger
    }

    /// Host-level Agent launch operation ledger.
    pub fn launch_ledger(&self) -> &LaunchOperationLedger {
        &self.launch_ledger
    }

    /// Number of conversations with an active Host delivery worker
    /// (diagnostics/tests).
    pub fn active_workers(&self) -> usize {
        lock_workers(&self.gate).len()
    }

    /// Test seam: retry policy + optional injected runtime (no socket).
    #[cfg(test)]
    fn with_retry_policy(
        queue: Arc<ConversationFollowUpQueue>,
        retry_delay: Duration,
        max_attempts: u32,
        test_runtime: Option<Arc<dyn FollowUpRuntime + Send + Sync>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            queue,
            socket_override: None,
            gate: Mutex::new(HashMap::new()),
            serializers: Mutex::new(HashMap::new()),
            ledger: MutationLedger::new(),
            launch_ledger: LaunchOperationLedger::new(),
            retry_delay,
            max_attempts,
            test_runtime,
        })
    }

    fn connect(&self) -> Result<HerdrClient, String> {
        match &self.socket_override {
            Some(path) => HerdrClient::connect_to(path),
            None => HerdrClient::connect(),
        }
        .map_err(|error| error.to_string())
    }

    /// Arm exactly one Host delivery worker for the conversation's queued
    /// item. Callers MUST have enqueued (or verified) the item BEFORE calling
    /// — the worker's exit gate treats "queue empty" as exit, and the gate
    /// lock makes every enqueue→schedule→worker-exit interleaving safe.
    pub fn schedule(self: &Arc<Self>, agent_ref: &AgentRef) {
        let key = agent_ref.as_str().to_string();
        {
            let mut gate = lock_workers(&self.gate);
            if gate.contains_key(&key) {
                return;
            }
            gate.insert(key.clone(), ());
        }
        let coordinator = Arc::clone(self);
        let agent = agent_ref.clone();
        // R4-P1: spawn failures (thread exhaustion) are retried with backoff;
        // only after bounded retries do we release the gate and log. The item
        // stays queued either way — a later schedule() re-arms it.
        let mut spawned = Err(std::io::Error::other("not attempted"));
        for attempt in 0..3_u32 {
            spawned = std::thread::Builder::new()
                .name("shardlane-followup-delivery".to_string())
                .spawn({
                    let coordinator = Arc::clone(&coordinator);
                    let agent = agent.clone();
                    move || coordinator.run_worker(&agent)
                });
            if spawned.is_ok() {
                break;
            }
            lag_log(format_args!(
                "follow-up delivery worker spawn retry {}/3 agent={}",
                attempt + 1,
                agent_ref.as_str()
            ));
            std::thread::sleep(Duration::from_millis(50));
        }
        if spawned.is_err() {
            lag_log(format_args!(
                "follow-up delivery worker spawn failed after retries; item stays queued"
            ));
            lock_workers(&self.gate).remove(&key);
        }
    }

    fn run_worker(&self, agent_ref: &AgentRef) {
        let key = agent_ref.as_str().to_string();
        'worker: loop {
            for attempt in 0..self.max_attempts {
                let outcome = self.attempt_delivery(agent_ref, attempt);
                match outcome {
                    Some(DeliveryOutcome::Delivered { .. }) => {
                        lag_log(format_args!(
                            "follow-up delivered agent={}",
                            agent_ref.as_str()
                        ));
                        break;
                    }
                    Some(DeliveryOutcome::RetainedBlocked)
                    | Some(DeliveryOutcome::RetainedTimeout) => {
                        lag_log(format_args!(
                            "follow-up retained agent={} attempt={}",
                            agent_ref.as_str(),
                            attempt + 1
                        ));
                        if attempt + 1 >= self.max_attempts {
                            break;
                        }
                        std::thread::sleep(self.retry_delay);
                    }
                    Some(terminal) => {
                        // Failed/Uncertain/IdentityChanged/Cancelled/AlreadyClaimed
                        // are terminal: never auto-retry a semantic prompt.
                        lag_log(format_args!(
                            "follow-up terminal outcome ({terminal:?}) agent={}",
                            agent_ref.as_str()
                        ));
                        break;
                    }
                    None => {
                        if attempt + 1 >= self.max_attempts {
                            break;
                        }
                        std::thread::sleep(self.retry_delay);
                    }
                }
            }
            // R3-03 atomic exit gate: successor check + marker removal under
            // ONE lock. schedule() runs AFTER enqueue and takes this lock, so
            // either it observes our marker and relies on this gate, or we
            // observe its item and keep the worker alive — never a lost wake.
            let mut gate = lock_workers(&self.gate);
            if deliverable_item_exists(&self.queue, agent_ref) {
                drop(gate);
                continue 'worker;
            }
            gate.remove(&key);
        }
    }

    fn attempt_delivery(&self, agent_ref: &AgentRef, attempt: u32) -> Option<DeliveryOutcome> {
        #[cfg(test)]
        if let Some(runtime) = &self.test_runtime {
            return Some(self.run_serialized_delivery(runtime.as_ref(), agent_ref));
        }
        let client = match self.connect() {
            Ok(client) => client,
            Err(error) => {
                lag_log(format_args!(
                    "follow-up delivery connect failed agent={} attempt={} error={error}",
                    agent_ref.as_str(),
                    attempt + 1
                ));
                return None;
            }
        };
        let runtime = HerdrFollowUpRuntime::new(&client);
        Some(self.run_serialized_delivery(&runtime, agent_ref))
    }

    /// P0-02: phase 1 (claim + settle wait) runs unserialized; phase 2
    /// (revalidation + prompt) runs under the conversation lock so it cannot
    /// interleave with an immediate submit on the same conversation.
    fn run_serialized_delivery<R: FollowUpRuntime + ?Sized>(
        &self,
        runtime: &R,
        agent_ref: &AgentRef,
    ) -> DeliveryOutcome {
        match DeliveryTransaction::begin(
            Arc::clone(&self.queue),
            runtime,
            agent_ref,
            SETTLE_TIMEOUT_MS,
        ) {
            DeliveryBegin::Claimed(transaction) => {
                let lock = self.conversation_lock(agent_ref);
                let _guard = lock.lock().unwrap_or_else(|poison| poison.into_inner());
                transaction.finish(runtime)
            }
            DeliveryBegin::AlreadyClaimed => DeliveryOutcome::AlreadyClaimed,
            DeliveryBegin::Gone => DeliveryOutcome::CancelledBeforeDelivery,
            DeliveryBegin::Retained(outcome) => outcome,
        }
    }
}

/// P0-03: Host-level semantic mutation ledger. Bounded (TTL + capacity),
/// process-wide. Exact replay of a logical request id returns the recorded
/// outcome — success or the delivery-uncertain tombstone — without
/// re-executing the prompt. A reused id with a different fingerprint is an
/// explicit conflict.
pub struct MutationLedger {
    entries: Mutex<HashMap<String, (Instant, String, LedgerOutcome)>>,
    order: Mutex<VecDeque<String>>,
    /// Admission lock for the claim→execute→complete transaction.  The
    /// Conversation lock protects same-target ordering; this small process
    /// gate closes the remaining race when a logical id is concurrently used
    /// against two targets before either one records its outcome.
    transaction: Mutex<()>,
}

#[derive(Clone)]
enum LedgerOutcome {
    /// (disposition, exact Conversation identity) of the accepted submission.
    Accepted(PromptDisposition, ConversationIdentity),
    /// Definite non-acceptance (needs terminal).
    NeedsTerminal,
    /// Fail-closed status projection. This is replayed as the same rejection,
    /// not as a successful NeedsTerminal disposition.
    RejectedUnknown(String),
    /// Tombstone: the mutation may already have been delivered. The lookup
    /// surfaces this as an Err carrying `delivery_uncertain` so callers map
    /// it to the same class as a live uncertain outcome.
    DeliveryUncertain(String),
}

const LEDGER_TTL: Duration = Duration::from_secs(10 * 60);
const LEDGER_CAPACITY: usize = 512;

impl MutationLedger {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            order: Mutex::new(VecDeque::new()),
            transaction: Mutex::new(()),
        }
    }

    pub fn transaction_lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.transaction
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Replay lookup for (key, fingerprint):
    /// - `Ok(Some((disposition, identity)))` — replay the recorded outcome;
    /// - `Ok(None)` — no record, execute the mutation;
    /// - `Err(reason)` starting with `delivery_uncertain:` — tombstone replay;
    /// - any other `Err` — fingerprint conflict (reused id, different body).
    pub fn lookup(
        &self,
        key: &str,
        fingerprint: &str,
    ) -> Result<Option<(PromptDisposition, Option<ConversationIdentity>)>, String> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match entries.get(key) {
            Some((at, stored, outcome)) if at.elapsed() < LEDGER_TTL => {
                if stored != fingerprint {
                    return Err(
                        "this request_id was already used for a different mutation".to_string()
                    );
                }
                Ok(Some(match outcome {
                    LedgerOutcome::Accepted(disposition, identity) => {
                        (*disposition, Some(identity.clone()))
                    }
                    LedgerOutcome::NeedsTerminal => (PromptDisposition::NeedsTerminal, None),
                    LedgerOutcome::RejectedUnknown(reason) => {
                        return Err(format!("rejected_unknown: {reason}"))
                    }
                    LedgerOutcome::DeliveryUncertain(reason) => {
                        return Err(format!("delivery_uncertain: {reason}"))
                    }
                }))
            }
            _ => Ok(None),
        }
    }

    pub fn record_accepted(
        &self,
        key: String,
        fingerprint: String,
        disposition: PromptDisposition,
        identity: ConversationIdentity,
    ) {
        self.store(
            key,
            fingerprint,
            LedgerOutcome::Accepted(disposition, identity),
        );
    }

    pub fn record_needs_terminal(&self, key: String, fingerprint: String) {
        self.store(key, fingerprint, LedgerOutcome::NeedsTerminal);
    }

    pub fn record_rejected_unknown(&self, key: String, fingerprint: String, reason: &str) {
        self.store(
            key,
            fingerprint,
            LedgerOutcome::RejectedUnknown(reason.to_string()),
        );
    }

    pub fn record_uncertain(&self, key: String, fingerprint: String, reason: &str) {
        self.store(
            key,
            fingerprint,
            LedgerOutcome::DeliveryUncertain(reason.to_string()),
        );
    }

    fn store(&self, key: String, fingerprint: String, outcome: LedgerOutcome) {
        let mut order = self
            .order
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some((at, _, _)) = entries.get(&key) {
            if at.elapsed() < LEDGER_TTL {
                return;
            }
            // `lookup` treats an expired record as a miss. Remove its stale
            // order entry as well, otherwise the new operation would be
            // silently discarded by the old key and could never be replayed.
            entries.remove(&key);
            order.retain(|entry| entry != &key);
        }
        entries.insert(key.clone(), (Instant::now(), fingerprint, outcome));
        order.push_back(key);
        while order.len() > LEDGER_CAPACITY {
            if let Some(oldest) = order.pop_front() {
                entries.remove(&oldest);
            }
        }
        while let Some(front) = order.front() {
            match entries.get(front) {
                Some((at, _, _)) if at.elapsed() >= LEDGER_TTL => {
                    let front = front.clone();
                    order.pop_front();
                    entries.remove(&front);
                }
                _ => break,
            }
        }
    }
}

impl Default for MutationLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// Host-owned idempotency for Agent launch operations. Unlike the prompt
/// ledger, a launch result contains runtime structure, so it is kept as a
/// typed in-memory outcome and replayed to every caller that presents the
/// same operation id/fingerprint. A committed failure (`AgentCreated` or an
/// uncertain start) is retained just like a successful launch; pre-commit
/// failures are released so the caller may deliberately retry preparation.
pub struct LaunchOperationLedger {
    inner: Mutex<LaunchLedgerInner>,
}

struct LaunchLedgerInner {
    entries: HashMap<String, Arc<LaunchOperationEntry>>,
    order: VecDeque<String>,
}

struct LaunchOperationEntry {
    fingerprint: String,
    created_at: Instant,
    state: Mutex<LaunchLedgerState>,
    changed: std::sync::Condvar,
}

enum LaunchLedgerState {
    Running,
    Finished(
        Box<
            Result<
                crate::agent_launch::AgentLaunchOutcome,
                crate::agent_launch::AgentLaunchFailure,
            >,
        >,
    ),
}

const LAUNCH_LEDGER_TTL: Duration = Duration::from_secs(10 * 60);
const LAUNCH_LEDGER_CAPACITY: usize = 256;

impl LaunchOperationLedger {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(LaunchLedgerInner {
                entries: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    /// Execute or replay one launch. Concurrent duplicates wait on the same
    /// entry; a different fingerprint is rejected before any runtime call.
    pub fn execute<F>(
        &self,
        operation_id: &str,
        fingerprint: String,
        run: F,
    ) -> Result<crate::agent_launch::AgentLaunchOutcome, crate::agent_launch::AgentLaunchFailure>
    where
        F: FnOnce() -> Result<
            crate::agent_launch::AgentLaunchOutcome,
            crate::agent_launch::AgentLaunchFailure,
        >,
    {
        if operation_id.trim().is_empty() {
            return Err(crate::agent_launch::AgentLaunchFailure::OperationConflict(
                "launch operation_id must not be empty".to_string(),
            ));
        }
        let (entry, owner) = {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            self.prune_locked(&mut inner);
            if let Some(existing) = inner.entries.get(operation_id).cloned() {
                if existing.fingerprint != fingerprint {
                    return Err(crate::agent_launch::AgentLaunchFailure::OperationConflict(
                        "this launch operation_id was already used for a different request"
                            .to_string(),
                    ));
                }
                (existing, false)
            } else {
                let entry = Arc::new(LaunchOperationEntry {
                    fingerprint,
                    created_at: Instant::now(),
                    state: Mutex::new(LaunchLedgerState::Running),
                    changed: std::sync::Condvar::new(),
                });
                inner
                    .entries
                    .insert(operation_id.to_string(), Arc::clone(&entry));
                inner.order.push_back(operation_id.to_string());
                (entry, true)
            }
        };

        if !owner {
            let mut state = entry
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            while matches!(*state, LaunchLedgerState::Running) {
                state = entry
                    .changed
                    .wait(state)
                    .unwrap_or_else(|poison| poison.into_inner());
            }
            return match &*state {
                LaunchLedgerState::Finished(result) => result.as_ref().clone(),
                LaunchLedgerState::Running => unreachable!("launch waiter must be completed"),
            };
        }

        let result = run();
        let retain = match &result {
            Ok(_)
            | Err(crate::agent_launch::AgentLaunchFailure::AgentCreated { .. })
            | Err(crate::agent_launch::AgentLaunchFailure::AgentStartUncertain { .. }) => true,
            Err(_) => false,
        };
        {
            let mut state = entry
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            *state = LaunchLedgerState::Finished(Box::new(result.clone()));
            entry.changed.notify_all();
        }
        if !retain {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if inner
                .entries
                .get(operation_id)
                .is_some_and(|current| Arc::ptr_eq(current, &entry))
            {
                inner.entries.remove(operation_id);
                inner.order.retain(|id| id != operation_id);
            }
        }
        result
    }

    fn prune_locked(&self, inner: &mut LaunchLedgerInner) {
        let mut retained = VecDeque::with_capacity(inner.order.len());
        while let Some(id) = inner.order.pop_front() {
            let Some(entry) = inner.entries.get(&id).cloned() else {
                continue;
            };
            let running = entry
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let expired = !matches!(*running, LaunchLedgerState::Running)
                && entry.created_at.elapsed() >= LAUNCH_LEDGER_TTL;
            drop(running);
            if expired {
                inner.entries.remove(&id);
            } else {
                retained.push_back(id);
            }
        }
        inner.order = retained;
        while inner.order.len() > LAUNCH_LEDGER_CAPACITY {
            let Some(id) = inner.order.pop_front() else {
                break;
            };
            let Some(entry) = inner.entries.get(&id).cloned() else {
                continue;
            };
            let running = entry
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if matches!(*running, LaunchLedgerState::Running) {
                inner.order.push_back(id);
                break;
            }
            inner.entries.remove(&id);
        }
    }
}

impl Default for LaunchOperationLedger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_queue::{PromptAcceptance, QueueState, QueuedFollowUp, WaitOutcome};
    use crate::conversations::session_fingerprint;
    use crate::herdr::AgentSessionInfo;

    fn claude_session(value: &str) -> AgentSessionInfo {
        AgentSessionInfo {
            agent: "claude".into(),
            kind: "id".into(),
            source: "herdr:claude".into(),
            value: value.into(),
        }
    }

    fn queue_item(queue: &Arc<ConversationFollowUpQueue>, request_id: &str, text: &str) {
        let agent = AgentRef::new("pane-1");
        queue
            .enqueue(QueuedFollowUp {
                request_id: request_id.into(),
                conversation_id: crate::ConversationId::new("conv_2_test"),
                agent_ref: agent,
                provider: "claude".into(),
                native_session_id: Some("native-1".into()),
                session_fingerprint: session_fingerprint(&claude_session("native-1")),
                baseline_revision: 9,
                text: text.into(),
                created_at: std::time::SystemTime::now(),
                state: QueueState::Queued,
            })
            .unwrap_or_else(|error| panic!("{error}"));
    }

    fn wait_until(deadline_ms: u64, predicate: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_millis(deadline_ms);
        while !predicate() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Scripted runtime: settle results and prompt acceptance in sequence.
    struct ScriptedRuntime {
        settle_results: Mutex<Vec<WaitOutcome>>,
        prompt_results: Mutex<Vec<PromptAcceptance>>,
        prompt_calls: std::sync::atomic::AtomicU8,
        prompt_texts: Mutex<Vec<String>>,
        /// Runs at the moment the Nth prompt is about to be accepted — used
        /// to interleave client actions mid-delivery (race tests).
        on_prompt: Option<Box<dyn Fn() + Send + Sync>>,
    }

    impl ScriptedRuntime {
        fn delivering() -> Self {
            Self {
                settle_results: Mutex::new(vec![WaitOutcome::Sendable]),
                prompt_results: Mutex::new(vec![PromptAcceptance::Accepted]),
                prompt_calls: std::sync::atomic::AtomicU8::new(0),
                prompt_texts: Mutex::new(Vec::new()),
                on_prompt: None,
            }
        }
    }

    impl FollowUpRuntime for ScriptedRuntime {
        fn wait_sendable(
            &self,
            _agent: &AgentRef,
            _timeout_ms: u64,
        ) -> Result<WaitOutcome, String> {
            let mut results = self
                .settle_results
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if results.is_empty() {
                // After the script: settled (successor deliveries proceed).
                return Ok(WaitOutcome::Sendable);
            }
            Ok(results.remove(0))
        }

        fn agent_identity(
            &self,
            _agent: &AgentRef,
        ) -> Result<Option<crate::conversation_queue::FollowUpIdentity>, String> {
            Ok(Some(crate::conversation_queue::FollowUpIdentity {
                provider: "claude".into(),
                native_session_id: Some("native-1".into()),
                session_fingerprint: session_fingerprint(&claude_session("native-1")),
                revision: 9,
                interactive_ready: true,
                status: crate::dto::AgentStatus::Idle,
            }))
        }

        fn prompt(&self, _agent: &AgentRef, text: &str) -> Result<PromptAcceptance, String> {
            self.prompt_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.prompt_texts
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(text.to_string());
            if let Some(hook) = &self.on_prompt {
                hook();
            }
            let mut results = self
                .prompt_results
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if results.is_empty() {
                return Ok(PromptAcceptance::Accepted);
            }
            Ok(results.remove(0))
        }
    }

    #[test]
    fn remote_only_working_prompt_is_delivered_with_no_client_surface() {
        // R2-03/CR-01 acceptance (injected-runtime form): enqueue + schedule is
        // the entire client involvement.
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let coordinator = ConversationDeliveryCoordinator::with_retry_policy(
            queue.clone(),
            Duration::from_millis(20),
            5,
            Some(Arc::new(ScriptedRuntime::delivering())),
        );
        let agent = AgentRef::new("pane-1");
        queue_item(&queue, "req-1", "next step");
        coordinator.schedule(&agent);
        wait_until(2_000, || queue.queued(&agent).is_none());
        assert!(queue.queued(&agent).is_none(), "Host worker delivers alone");
        wait_until(2_000, || coordinator.active_workers() == 0);
    }

    #[test]
    fn delivered_then_immediate_requeue_is_not_lost_before_marker_removal() {
        // R3-03 lost-wakeup: item B is enqueued and schedule(B) runs WHILE the
        // worker for A is still between "delivered A" and "marker removal".
        // The atomic exit gate must adopt B instead of orphaning it.
        let queue = Arc::new(ConversationFollowUpQueue::new());
        // The successor race window is between finish(A) removing A and the
        // worker's exit-gate check. A spinner thread enqueues B + schedule(B)
        // the instant A disappears — whether the gate ADOPTS B or schedule
        // spawns a fresh worker, B must never be orphaned.
        // Shared slot: the hook must schedule the REAL coordinator (the one
        // carrying the runtime), which only exists after construction.
        let coordinator_slot: Arc<Mutex<Option<Arc<ConversationDeliveryCoordinator>>>> =
            Arc::new(Mutex::new(None));
        let hook_slot = Arc::clone(&coordinator_slot);
        let hook_queue = Arc::clone(&queue);
        let runtime = ScriptedRuntime {
            on_prompt: Some(Box::new(move || {
                let slot = Arc::clone(&hook_slot);
                let queue = Arc::clone(&hook_queue);
                std::thread::spawn(move || {
                    let agent = AgentRef::new("pane-1");
                    let deadline = std::time::Instant::now() + Duration::from_secs(2);
                    while std::time::Instant::now() < deadline {
                        if queue.queued(&agent).is_none() {
                            queue_item(&queue, "req-B", "successor item");
                            if let Some(coordinator) = slot
                                .lock()
                                .unwrap_or_else(|poison| poison.into_inner())
                                .clone()
                            {
                                coordinator.schedule(&agent);
                            }
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                });
            })),
            ..ScriptedRuntime::delivering()
        };
        let coordinator = ConversationDeliveryCoordinator::with_retry_policy(
            queue.clone(),
            Duration::from_millis(20),
            5,
            Some(Arc::new(runtime)),
        );
        *coordinator_slot
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(Arc::clone(&coordinator));
        let agent = AgentRef::new("pane-1");
        queue_item(&queue, "req-A", "first item");
        coordinator.schedule(&agent);
        wait_until(3_000, || queue.queued(&agent).is_none());
        assert!(
            queue.queued(&agent).is_none(),
            "the exit gate must deliver the successor item B"
        );
        wait_until(3_000, || coordinator.active_workers() == 0);
    }

    #[test]
    fn long_blocked_agent_keeps_an_owner_and_recovers_without_any_client() {
        // R3-06: unbounded budget — the worker stays through Blocked retries;
        // when the agent finally settles, delivery resumes with no client.
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let runtime = ScriptedRuntime {
            settle_results: Mutex::new(vec![
                WaitOutcome::Blocked,
                WaitOutcome::Blocked,
                WaitOutcome::Blocked,
                WaitOutcome::Sendable,
            ]),
            ..ScriptedRuntime::delivering()
        };
        let coordinator = ConversationDeliveryCoordinator::with_retry_policy(
            queue.clone(),
            Duration::from_millis(20),
            u32::MAX,
            Some(Arc::new(runtime)),
        );
        let agent = AgentRef::new("pane-1");
        queue_item(&queue, "req-blocked", "held while blocked");
        coordinator.schedule(&agent);
        wait_until(4_000, || queue.queued(&agent).is_none());
        assert!(
            queue.queued(&agent).is_none(),
            "Host keeps ownership through Blocked and delivers after settle"
        );
        wait_until(2_000, || coordinator.active_workers() == 0);
    }

    /// One-shot fake Herdr socket over a tempdir; each accepted connection
    /// consumes the next scripted response line (real connect-path test).
    fn scripted_herdr(responses: Vec<String>) -> std::path::PathBuf {
        let socket_path = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("{error}"))
            .keep()
            .join("herdr-delivery.sock");
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
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !socket_path.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        socket_path
    }

    #[test]
    fn real_connect_path_delivers_through_the_herdr_adapter() {
        // End-to-end over the scripted Herdr socket: connect ping → settle
        // wait → occupant list → prompt accepted.
        let socket = scripted_herdr(vec![
            r#"{"id":"","result":{"type":"pong","version":"0.8.2","protocol":20,"capabilities":{}}}"#.into(),
            r#"{"id":"","result":{"type":"agent","agent":{"terminal_id":"term-1","pane_id":"pane-1","agent_status":"idle","agent_session":{"agent":"claude","kind":"id","source":"herdr:claude","value":"native-1"}}}}"#.into(),
            r#"{"id":"","result":{"type":"agent_list","agents":[{"terminal_id":"term-1","pane_id":"pane-1","agent_status":"idle","interactive_ready":true,"revision":9,"agent_session":{"agent":"claude","kind":"id","source":"herdr:claude","value":"native-1"}}]}}"#.into(),
            r#"{"id":"","result":{"agent":{"terminal_id":"term-1","pane_id":"pane-1","agent_status":"idle"}}}"#.into(),
        ]);
        let queue = Arc::new(ConversationFollowUpQueue::new());
        let coordinator = ConversationDeliveryCoordinator::new(queue.clone(), Some(socket));
        let agent = AgentRef::new("pane-1");
        queue_item(&queue, "req-socket", "via real adapter");
        coordinator.schedule(&agent);
        wait_until(3_000, || queue.queued(&agent).is_none());
        assert!(queue.queued(&agent).is_none());
        wait_until(2_000, || coordinator.active_workers() == 0);
    }

    #[test]
    fn ledger_replays_the_exact_identity_for_an_accepted_operation() {
        let ledger = MutationLedger::new();
        let identity = ConversationIdentity {
            conversation_id: crate::ConversationId::new("conv_live"),
            agent_ref: AgentRef::new("pane-1"),
            provider: "claude".into(),
            native_session_id: Some("native-1".into()),
            revision: 7,
        };
        ledger.record_accepted(
            "prompt:req-identity".into(),
            "conv_live|text=hello".into(),
            PromptDisposition::SentNow,
            identity.clone(),
        );
        assert_eq!(
            ledger.lookup("prompt:req-identity", "conv_live|text=hello"),
            Ok(Some((PromptDisposition::SentNow, Some(identity))))
        );
    }

    #[test]
    fn ledger_replays_unknown_rejection_as_the_same_error() {
        let ledger = MutationLedger::new();
        ledger.record_rejected_unknown(
            "prompt:req-unknown".into(),
            "conv_live|text=hello".into(),
            "state is unknown",
        );
        assert_eq!(
            ledger.lookup("prompt:req-unknown", "conv_live|text=hello"),
            Err("rejected_unknown: state is unknown".into())
        );
    }

    fn dummy_launch_outcome() -> crate::agent_launch::AgentLaunchOutcome {
        crate::agent_launch::AgentLaunchOutcome {
            agent_ref: AgentRef::new("pane-launch"),
            pane_id: "pane-launch".into(),
            tab_id: "tab-launch".into(),
            workspace_id: "workspace-launch".into(),
            identity: None,
            task_title: None,
            created: crate::herdr::TabCreatedResult {
                tab: crate::herdr::Tab {
                    tab_id: "tab-launch".into(),
                    workspace_id: Some("workspace-launch".into()),
                    label: None,
                    title: None,
                    terminal_title: None,
                    agent_status: None,
                    pane_count: None,
                    focused: false,
                },
                root_pane: crate::herdr::Pane::default(),
            },
            layout: crate::herdr::PaneLayout::default(),
        }
    }

    #[test]
    fn launch_ledger_coalesces_concurrent_retries_and_replays_the_target() {
        let ledger = Arc::new(LaunchOperationLedger::new());
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let calls = Arc::new(std::sync::atomic::AtomicU8::new(0));
        let handles = (0..8)
            .map(|_| {
                let ledger = Arc::clone(&ledger);
                let barrier = Arc::clone(&barrier);
                let calls = Arc::clone(&calls);
                std::thread::spawn(move || {
                    barrier.wait();
                    ledger.execute("launch-op", "same-shape".into(), || {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(10));
                        Ok(dummy_launch_outcome())
                    })
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            let outcome = handle
                .join()
                .unwrap_or_else(|_| panic!("launch retry thread panicked"))
                .unwrap_or_else(|error| panic!("launch should succeed: {error}"));
            assert_eq!(outcome.pane_id, "pane-launch");
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn launch_ledger_rejects_a_reused_id_with_a_different_shape() {
        let ledger = LaunchOperationLedger::new();
        let _ = ledger
            .execute("launch-op", "shape-a".into(), || Ok(dummy_launch_outcome()))
            .unwrap_or_else(|error| panic!("first launch should succeed: {error}"));
        let error =
            match ledger.execute("launch-op", "shape-b".into(), || Ok(dummy_launch_outcome())) {
                Ok(_) => panic!("different shape must conflict before a second launch"),
                Err(error) => error,
            };
        assert!(matches!(
            error,
            crate::agent_launch::AgentLaunchFailure::OperationConflict(_)
        ));
    }
}
