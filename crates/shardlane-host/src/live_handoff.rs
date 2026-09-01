//! Host Live Handoff — the same lossless transfer engine, Agent-to-Agent.
//!
//! [INPUT]: a resolved live exact source (SessionFileRef + AgentRef +
//! state), the target Provider, the reused M2 launch intent, an optional
//! instruction.
//! [OUTPUT]: `run_live_handoff` / `run_live_handoff_with_delivery` — a
//! Working source first does `Handoff after current turn` (exact-target
//! settle wait → identity re-verification → snapshot); the production entry
//! also holds the source Conversation reservation, then delivers the full
//! context + optional instruction as the target Agent's sole initial briefing
//! through the M5 engine, keeping the created target as
//! `CreatedNeedsAttention`. The source Agent is never stopped/closed/
//! mutated. No Host FocusTarget; each client navigates itself on success.
//! [POS]: plan M7 / audit AF-17/AF-18. No unverified in-flight snapshot
//! (v1 has no `Handoff now`); an uncertain briefing is never re-sent
//! automatically.

use crate::agent_launch::{
    AgentLaunchFailure, AgentLaunchIntent, AgentLaunchOutcome, AgentLaunchRuntime,
    ProjectPreparation,
};
use crate::context_transfer::{
    run_context_transfer, run_context_transfer_with_ledger, ContextTransferFailure,
    ContextTransferRequest, ContextTransferSource,
};
use crate::ids::AgentRef;
use shardlane_history::models::SessionFileRef;
use shardlane_history::{AgentId, TransferArtifactStore, TransferLimits};
use std::fmt;

#[derive(Clone, Debug)]
pub struct LiveHandoffRequest {
    /// Exact live source: the resolved provider-native session plus the live
    /// Agent target that owns it right now.
    pub source: SessionFileRef,
    pub source_agent_ref: AgentRef,
    pub target_provider: AgentId,
    /// Launch inputs for the target (project/branch/mode/permission). The
    /// engine overwrites `agent` and `prompt`.
    pub launch: AgentLaunchIntent,
    /// Optional continuation instruction handed to the target.
    pub instruction: Option<String>,
}

#[derive(Debug)]
pub enum LiveHandoffFailure {
    /// The exact source could not be resolved.
    SourceUnresolved(String),
    /// The settle wait failed or timed out; the source is untouched.
    WaitFailed(String),
    /// The live occupant changed before the snapshot; fail closed.
    SourceIdentityChanged,
    /// The source kept starting new turns inside the bounded window; retry
    /// later instead of snapshotting an in-flight turn (AC-08).
    SourceBusy,
    /// The source Agent is Blocked/Failed: it is waiting for terminal input,
    /// not a completed turn — full-context handoff would lie (R2-08).
    SourceBlocked {
        provider: &'static str,
    },
    /// The provider source did not flush the completed turn inside the
    /// bounded freshness window; transferring a hash-valid but stale file
    /// would lie about "full context" (AC-07).
    WaitForSourceFlush {
        provider: &'static str,
    },
    Snapshot(String),
    /// The transfer engine could not deliver (artifact unavailable, launch
    /// phase failure, …); the source is still untouched.
    Transfer(String),
    /// The target launch committed a runtime Agent, but a later setup phase
    /// failed.  Preserve the exact target so clients can focus/reconcile it
    /// instead of offering a blind second handoff.
    CreatedNeedsAttention {
        agent_ref: AgentRef,
        tab_id: String,
        pane_id: String,
        phase: crate::agent_launch::CreatedAgentPhase,
        detail: String,
    },
    /// R7-P0-02: an older accepted operation (queued Prompt, uncertain/failed
    /// delivery) still exists for the exact source Conversation.  Snapshotting
    /// now would omit that user intent from the transferred context.
    SourceHasPendingOperation,
}

impl fmt::Display for LiveHandoffFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceUnresolved(message) => {
                write!(formatter, "handoff source unresolved: {message}")
            }
            Self::WaitFailed(message) => {
                write!(formatter, "handoff wait failed: {message}")
            }
            Self::SourceIdentityChanged => {
                formatter.write_str("the source conversation changed before the handoff snapshot")
            }
            Self::SourceBusy => formatter.write_str(
                "the source agent kept starting turns; retry the handoff when it settles",
            ),
            Self::SourceBlocked { provider } => write!(
                formatter,
                // C26: keep the message on one line — the collapsed newline
                // used to render as a run of spaces.
                "the {provider} source agent is blocked and needs the Terminal; \
                 resolve it before handing off full context"
            ),
            Self::WaitForSourceFlush { provider } => write!(
                formatter,
                "the {provider} source did not flush the completed turn yet; retry the handoff shortly"
            ),
            Self::Snapshot(message) => write!(formatter, "handoff snapshot failed: {message}"),
            Self::Transfer(message) => write!(formatter, "handoff transfer failed: {message}"),
            Self::CreatedNeedsAttention {
                agent_ref,
                tab_id,
                pane_id: _,
                phase: _,
                detail,
            } => write!(
                formatter,
                "handoff target already created (pane={}, tab={}) but needs attention: {detail}",
                agent_ref.as_str(),
                tab_id
            ),
            Self::SourceHasPendingOperation => formatter.write_str(
                "the source conversation has a pending operation; \
                 deliver or cancel it before handing off",
            ),
        }
    }
}

impl std::error::Error for LiveHandoffFailure {}

/// Bounded settle cycles: a source that keeps starting new turns must surface
/// `SourceBusy`, never an in-flight snapshot (AC-08).
const HANDOFF_MAX_SETTLE_CYCLES: usize = 3;
/// Bounded wait for the provider source to flush the settled turn (AC-07).
const HANDOFF_SOURCE_FLUSH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const HANDOFF_SOURCE_FLUSH_POLL: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Clone, Debug)]
pub struct LiveHandoffOutcome {
    pub launch: AgentLaunchOutcome,
    pub briefing_sha256: String,
    /// P0-08: what the fence actually PROVED about the source snapshot.
    /// `VerifiedFlush` observed a post-settle advance (the flushed output of
    /// the just-completed turn); `StableStat` only proved the file quiescent
    /// — the provider-native completed-turn watermark is still required
    /// before "lossless/full context" may be claimed for live sources.
    pub source_fidelity: SourceFidelity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceFidelity {
    VerifiedFlush,
    StableStat,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct SourceStat {
    mtime_ms: i64,
    size: i64,
}

fn source_stat(source: &SessionFileRef) -> Option<SourceStat> {
    let metadata = std::fs::metadata(&source.file_path).ok()?;
    let mtime_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0);
    Some(SourceStat {
        mtime_ms,
        size: i64::try_from(metadata.len()).unwrap_or(i64::MAX),
    })
}

/// R2-08: one Host state projection for handoff eligibility. Herdr may report
/// status through `agent_status` OR `custom_status`, and `launch_pending` is a
/// typed boolean — handoff must not duplicate a narrower interpretation.
enum SourceHandoffState {
    MidTurn,
    Blocked,
    Settled,
    Unknown,
}

fn handoff_state(agent: &crate::herdr::Agent) -> SourceHandoffState {
    // R3-04/R2-08: handoff classifies through the SAME Agent-typed
    // sendability SSOT as the prompt transaction — no parallel status
    // interpretation.
    match crate::conversation_queue::agent_sendability(agent) {
        crate::conversation_queue::AgentSendability::MidTurn => SourceHandoffState::MidTurn,
        crate::conversation_queue::AgentSendability::NeedsTerminal => SourceHandoffState::Blocked,
        crate::conversation_queue::AgentSendability::Sendable => SourceHandoffState::Settled,
        crate::conversation_queue::AgentSendability::Unknown => SourceHandoffState::Unknown,
    }
}

/// Execute one live handoff. Blocking; callers run it on a background thread.
///
/// AC-08: the Host derives the source status itself — the caller no longer
/// supplies a presentation-captured `working_source` flag. AC-07: after the
/// settle wait, a source-freshness barrier requires the provider file to
/// advance past the pre-wait stat before the snapshot, so "full context"
/// cannot silently mean "hash-valid but stale".
pub fn run_live_handoff<R: AgentLaunchRuntime, P: ProjectPreparation>(
    runtime: &R,
    preparation: &P,
    store: &TransferArtifactStore,
    limits: &TransferLimits,
    request: &LiveHandoffRequest,
    settle_timeout_ms: u64,
) -> Result<LiveHandoffOutcome, LiveHandoffFailure> {
    run_live_handoff_inner(
        runtime,
        preparation,
        store,
        limits,
        request,
        settle_timeout_ms,
        None,
    )
}

/// Production entry point for a handoff from a live Conversation.  The
/// reservation is held from the initial source read through the settle/fidelity
/// fence and source snapshot, using the same per-Conversation coordinator lock
/// as semantic Prompt.  A Prompt submitted concurrently therefore waits until
/// the handoff has captured its exact boundary instead of starting a new source
/// turn inside the snapshot transaction.
pub fn run_live_handoff_with_delivery<R: AgentLaunchRuntime, P: ProjectPreparation>(
    runtime: &R,
    preparation: &P,
    store: &TransferArtifactStore,
    limits: &TransferLimits,
    request: &LiveHandoffRequest,
    settle_timeout_ms: u64,
    delivery: &std::sync::Arc<crate::conversation_delivery::ConversationDeliveryCoordinator>,
) -> Result<LiveHandoffOutcome, LiveHandoffFailure> {
    run_live_handoff_inner(
        runtime,
        preparation,
        store,
        limits,
        request,
        settle_timeout_ms,
        Some(delivery),
    )
}

fn run_live_handoff_inner<R: AgentLaunchRuntime, P: ProjectPreparation>(
    runtime: &R,
    preparation: &P,
    store: &TransferArtifactStore,
    limits: &TransferLimits,
    request: &LiveHandoffRequest,
    settle_timeout_ms: u64,
    delivery: Option<
        &std::sync::Arc<crate::conversation_delivery::ConversationDeliveryCoordinator>,
    >,
) -> Result<LiveHandoffOutcome, LiveHandoffFailure> {
    // The source reservation is deliberately acquired before the first
    // occupant/status read.  Holding the guard until immediately before the
    // target launch ensures the snapshot boundary cannot be pierced by a new
    // Prompt from another Host client. The unreserved wrapper above remains a
    // deterministic test seam for callers that do not own a process Host.
    let reservation_lock = delivery.map(|owner| owner.conversation_lock(&request.source_agent_ref));
    let _reservation = reservation_lock
        .as_ref()
        .map(|lock| lock.lock().unwrap_or_else(|poison| poison.into_inner()));
    // 1. Authoritative occupant read + exact identity verification.
    let read_occupant = || {
        runtime
            .agent_by_pane(request.source_agent_ref.as_str())
            .map_err(|error| LiveHandoffFailure::SourceUnresolved(error.to_string()))?
            .ok_or_else(|| {
                LiveHandoffFailure::SourceUnresolved(
                    "the source agent is no longer present in the runtime projection".to_string(),
                )
            })
    };
    let verify_identity = |current: &crate::herdr::Agent| {
        // C16: the exact session correlation is the shared history-continuation
        // helper — declared locator kind decides id vs path, never duplicated.
        let session_matches = current.agent_session.as_ref().is_some_and(|session| {
            crate::history_continuation::history_session_matches_herdr_identity(
                request.source.agent,
                &request.source.native_id,
                &request.source.file_path,
                session,
            )
        });
        if session_matches {
            Ok(())
        } else {
            Err(LiveHandoffFailure::SourceIdentityChanged)
        }
    };

    let mut current = read_occupant()?;
    verify_identity(&current)?;

    // R7-P0-02: after acquiring the source reservation and verifying exact
    // occupant identity, check whether an older accepted operation already
    // exists for this source Conversation.  Snapshotting now would omit that
    // user intent from the transferred context.
    if let Some(delivery_owner) = delivery {
        // Derive the Conversation id from the source session for queue lookup.
        let source_conversation_id = current
            .agent_session
            .as_ref()
            .map(|session| {
                crate::conversations::conversation_id_for_live_session(
                    &request.source_agent_ref,
                    session,
                )
            })
            .unwrap_or_else(|| crate::conversation_id_for_live_agent(&request.source_agent_ref));
        if delivery_owner
            .queue()
            .has_unresolved_for_conversation(&source_conversation_id)
        {
            return Err(LiveHandoffFailure::SourceHasPendingOperation);
        }
    }

    let mut waited = false;
    // The freshness baseline is sampled at entry; see the fence below for why
    // the proof must observe an advance AFTER the settle, not from here.
    let baseline_stat = source_stat(&request.source);

    // 2. `Handoff after current turn` on the Host's own reading of the status
    //    (R2-08: custom_status + launch_pending included; Blocked is a
    //    terminal-needs state, never a snapshot candidate). Bounded cycles: a
    //    source that keeps starting turns is busy, never an in-flight snapshot.
    for _ in 0..HANDOFF_MAX_SETTLE_CYCLES {
        match handoff_state(&current) {
            SourceHandoffState::MidTurn => {
                waited = true;
                let settled = runtime
                    .wait_agent_settled(request.source_agent_ref.as_str(), settle_timeout_ms)
                    .map_err(|error| LiveHandoffFailure::WaitFailed(error.to_string()))?;
                if settled == crate::agent_launch::SettledStatus::Blocked {
                    return Err(LiveHandoffFailure::SourceBlocked {
                        provider: request.source.agent.display_name(),
                    });
                }
                current = read_occupant()?;
                verify_identity(&current)?;
            }
            SourceHandoffState::Blocked => {
                return Err(LiveHandoffFailure::SourceBlocked {
                    provider: request.source.agent.display_name(),
                });
            }
            SourceHandoffState::Settled => break,
            SourceHandoffState::Unknown => {
                return Err(LiveHandoffFailure::SourceUnresolved(
                    "the source agent status is unknown; refusing an unproven snapshot".to_string(),
                ));
            }
        }
    }
    match handoff_state(&current) {
        SourceHandoffState::MidTurn => return Err(LiveHandoffFailure::SourceBusy),
        SourceHandoffState::Blocked => {
            return Err(LiveHandoffFailure::SourceBlocked {
                provider: request.source.agent.display_name(),
            });
        }
        SourceHandoffState::Settled => {}
        SourceHandoffState::Unknown => {
            return Err(LiveHandoffFailure::SourceUnresolved(
                "the source agent status is unknown; refusing an unproven snapshot".to_string(),
            ));
        }
    }

    // 3. AC-07/R2-06 source-freshness fence. A stat advance observed at any
    //    time BEFORE the settle proves nothing (the provider may append
    //    intermediate events mid-turn). The proof must be:
    //    - waited (a turn just completed): the source file advances past a
    //      stat sampled AFTER the settle wait returned — that flush can only
    //      be post-turn output — bounded by the flush window;
    //    - already settled at entry: the file is QUIESCENT (two consecutive
    //      identical stats), proving we are not racing an in-flight flush.
    //    A provider that cannot satisfy its fence fails closed
    //    (`WaitForSourceFlush`) instead of transferring a hash-valid but
    //    stale "full context". A provider-native completed-turn watermark
    //    remains the strictly stronger long-term fence.
    // P0-08: the fence grade is part of the outcome so callers stop claiming
    // "lossless/full context" for a snapshot the fence cannot prove complete.
    let fidelity;
    if waited {
        let Some(post_settle) = source_stat(&request.source) else {
            return Err(LiveHandoffFailure::Snapshot(
                "the source file cannot be stat-probed for a freshness fence".to_string(),
            ));
        };
        let deadline = std::time::Instant::now() + HANDOFF_SOURCE_FLUSH_TIMEOUT;
        loop {
            let advanced = source_stat(&request.source).is_some_and(|stat| {
                stat.mtime_ms > post_settle.mtime_ms || stat.size > post_settle.size
            });
            if advanced {
                fidelity = SourceFidelity::VerifiedFlush;
                break;
            }
            if std::time::Instant::now() >= deadline {
                return Err(LiveHandoffFailure::WaitForSourceFlush {
                    provider: request.source.agent.display_name(),
                });
            }
            std::thread::sleep(HANDOFF_SOURCE_FLUSH_POLL);
        }
    } else {
        let Some(first) = source_stat(&request.source) else {
            return Err(LiveHandoffFailure::Snapshot(
                "the source file cannot be stat-probed for a freshness fence".to_string(),
            ));
        };
        std::thread::sleep(HANDOFF_SOURCE_FLUSH_POLL);
        let second = source_stat(&request.source);
        match (baseline_stat, second) {
            (Some(base), Some(now)) if base == now && now == first => {
                // Quiescent, but quiescence ≠ completeness: the provider
                // watermark remains the only proof that the LAST completed
                // turn is present.
                fidelity = SourceFidelity::StableStat;
            }
            _ => {
                return Err(LiveHandoffFailure::WaitForSourceFlush {
                    provider: request.source.agent.display_name(),
                });
            }
        }
    }

    // 4. Snapshot + briefing + target launch through the shared engine. The
    //    source Agent is never stopped, closed, or mutated.
    let transfer_request = ContextTransferRequest {
        source: ContextTransferSource::Live(request.source.clone()),
        target_provider: request.target_provider,
        launch: request.launch.clone(),
        instruction: request.instruction.clone(),
    };
    // Keep the reservation through the shared transfer call.  That call owns
    // the immutable source snapshot internally; retaining the guard until the
    // target launch/briefing result is classified is conservative and avoids
    // exposing a half-reconciled handoff to a competing Prompt.  The guard is
    // released on every return path below.
    let transfer = match delivery {
        Some(owner) => run_context_transfer_with_ledger(
            runtime,
            preparation,
            store,
            limits,
            &transfer_request,
            owner.launch_ledger(),
        ),
        None => run_context_transfer(runtime, preparation, store, limits, &transfer_request),
    };
    match transfer {
        Ok(outcome) => Ok(LiveHandoffOutcome {
            launch: outcome.launch,
            briefing_sha256: outcome.briefing_sha256,
            source_fidelity: fidelity,
        }),
        Err(ContextTransferFailure::Snapshot(error)) => {
            Err(LiveHandoffFailure::Snapshot(error.to_string()))
        }
        Err(ContextTransferFailure::ArtifactInaccessible) => Err(LiveHandoffFailure::Transfer(
            "the full-context artifact is not accessible to the target agent".to_string(),
        )),
        Err(ContextTransferFailure::Launch(AgentLaunchFailure::InitialPromptFailed(error))) => {
            Err(LiveHandoffFailure::Transfer(format!(
                "the target agent started but the briefing was rejected: {error}"
            )))
        }
        Err(ContextTransferFailure::Launch(AgentLaunchFailure::AgentCreated {
            agent_ref,
            tab_id,
            pane_id,
            phase,
            detail,
        })) => Err(LiveHandoffFailure::CreatedNeedsAttention {
            agent_ref,
            tab_id,
            pane_id,
            phase,
            detail,
        }),
        Err(ContextTransferFailure::Launch(AgentLaunchFailure::AgentStartUncertain {
            agent_ref,
            tab_id,
            pane_id,
            detail,
        })) => Err(LiveHandoffFailure::CreatedNeedsAttention {
            agent_ref,
            tab_id,
            pane_id,
            phase: crate::agent_launch::CreatedAgentPhase::StartUncertain,
            detail,
        }),
        Err(ContextTransferFailure::Launch(error)) => {
            Err(LiveHandoffFailure::Transfer(error.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_launch::{AgentLaunchMode, AgentPermission};
    use crate::herdr::{
        AgentSessionInfo, AgentStartedResult, TabCreatedResult, WorkspaceCreatedResult,
    };
    use std::cell::RefCell;
    use std::path::PathBuf;

    #[derive(Default)]
    struct MockHandoffRuntime {
        wait_error: Option<&'static str>,
        /// Current source-occupant projection; flips to `settled_agent` when
        /// the settle wait completes (a working agent stays working until the
        /// turn actually settles).
        identity_agent: RefCell<Option<crate::herdr::Agent>>,
        settled_agent: Option<crate::herdr::Agent>,
        wait_calls: RefCell<u8>,
        start_calls: RefCell<u8>,
        /// Simulates the provider flushing the settled turn to the source file
        /// shortly AFTER the settle wait returns (post-settle flush — the only
        /// advance the R2-06 fence accepts).
        settle_flush_path: Option<PathBuf>,
    }

    impl MockHandoffRuntime {
        fn with_identity(agent: Option<crate::herdr::Agent>) -> Self {
            Self {
                identity_agent: RefCell::new(agent.clone()),
                settled_agent: agent,
                ..Default::default()
            }
        }
    }

    struct MockError(&'static str);
    impl fmt::Display for MockError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.0)
        }
    }

    fn source(dir: &std::path::Path) -> SessionFileRef {
        let path = dir.join("live-session.jsonl");
        std::fs::write(&path, "LIVE-CONTEXT\n").unwrap_or_else(|error| panic!("{error}"));
        SessionFileRef {
            agent: AgentId::ClaudeCode,
            native_id: "live-1".into(),
            file_path: path.to_string_lossy().into_owned(),
            mtime_ms: 0,
            size: 0,
        }
    }

    fn live_agent(session: Option<AgentSessionInfo>) -> crate::herdr::Agent {
        agent_with_status(session, "idle")
    }

    fn agent_with_status(session: Option<AgentSessionInfo>, status: &str) -> crate::herdr::Agent {
        crate::herdr::Agent {
            pane_id: Some("pane-7".into()),
            interactive_ready: true,
            agent_status: Some(status.to_string()),
            agent_session: session,
            ..Default::default()
        }
    }

    fn matching_session() -> AgentSessionInfo {
        AgentSessionInfo {
            agent: "claude".into(),
            kind: "id".into(),
            source: "herdr:claude".into(),
            value: "live-1".into(),
        }
    }

    fn ready_target() -> crate::herdr::Agent {
        crate::herdr::Agent {
            pane_id: Some("pane-9".into()),
            interactive_ready: true,
            agent_session: Some(AgentSessionInfo {
                agent: "codex".into(),
                kind: "id".into(),
                source: "herdr:codex".into(),
                value: "native-9".into(),
            }),
            ..Default::default()
        }
    }

    impl AgentLaunchRuntime for MockHandoffRuntime {
        type Error = MockError;
        fn create_tab_without_focus(
            &self,
            _workspace_id: Option<&str>,
            _cwd: &str,
        ) -> Result<TabCreatedResult, Self::Error> {
            Ok(TabCreatedResult {
                tab: crate::herdr::Tab {
                    tab_id: "tab-9".into(),
                    workspace_id: Some("ws-1".into()),
                    label: None,
                    title: None,
                    terminal_title: None,
                    agent_status: None,
                    pane_count: None,
                    focused: false,
                },
                root_pane: crate::herdr::Pane {
                    pane_id: "pane-9".into(),
                    ..Default::default()
                },
            })
        }
        fn create_workspace_without_focus(
            &self,
            _cwd: &str,
        ) -> Result<WorkspaceCreatedResult, Self::Error> {
            Err(MockError("unexpected workspace creation"))
        }
        fn workspace_id_for_path(&self, _path: &str) -> Result<Option<String>, Self::Error> {
            Ok(Some("ws-1".into()))
        }
        fn start_agent(
            &self,
            _params: &crate::herdr::AgentStartParams,
        ) -> Result<AgentStartedResult, Self::Error> {
            self.start_calls.replace_with(|count| *count + 1);
            Ok(AgentStartedResult {
                agent: ready_target(),
                _argv: Vec::new(),
            })
        }
        fn wait_agent_idle(&self, _target: &str, _timeout_ms: u64) -> Result<(), Self::Error> {
            Ok(())
        }
        fn wait_shell_ready(&self, _pane_id: &str, _timeout_ms: u64) -> Result<(), Self::Error> {
            Ok(())
        }
        fn wait_agent_settled(
            &self,
            _target: &str,
            _timeout_ms: u64,
        ) -> Result<crate::agent_launch::SettledStatus, Self::Error> {
            self.wait_calls.replace_with(|count| *count + 1);
            if let Some(path) = &self.settle_flush_path {
                let path = path.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    use std::io::Write;
                    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&path) {
                        let _ = file.write_all(b"SETTLED-FLUSH\n");
                    }
                });
            }
            if let Some(settled) = &self.settled_agent {
                self.identity_agent.replace(Some(settled.clone()));
            }
            match self.wait_error {
                Some(error) => Err(MockError(error)),
                None => Ok(crate::agent_launch::SettledStatus::Idle),
            }
        }
        fn agent_by_pane(&self, pane_id: &str) -> Result<Option<crate::herdr::Agent>, Self::Error> {
            if pane_id == "pane-7" {
                Ok(self.identity_agent.borrow().clone())
            } else {
                Ok(Some(ready_target()))
            }
        }
        fn prompt_agent_once(&self, _target: &str, _text: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn send_agent_keys(&self, _target: &str, _keys: &[String]) -> Result<(), Self::Error> {
            Ok(())
        }
        fn rename_tab(&self, _tab_id: &str, _label: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn rename_pane(&self, _pane_id: &str, _label: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn close_tab(&self, _tab_id: &str) -> Result<(), Self::Error> {
            Ok(())
        }
        fn pane_layout(&self, _pane_id: &str) -> Result<crate::herdr::PaneLayout, Self::Error> {
            Ok(crate::herdr::PaneLayout::default())
        }
    }

    struct FixedPreparation(PathBuf);
    impl ProjectPreparation for FixedPreparation {
        fn prepare(
            &self,
            _path: &str,
            _branch: &str,
        ) -> Result<crate::agent_launch::PreparedProject, String> {
            Ok(crate::agent_launch::PreparedProject {
                cwd: self.0.clone(),
                worktree_created: false,
            })
        }
    }

    fn request(_dir: &std::path::Path, source: SessionFileRef) -> LiveHandoffRequest {
        LiveHandoffRequest {
            source,
            source_agent_ref: AgentRef::new("pane-7"),
            target_provider: AgentId::Codex,
            launch: AgentLaunchIntent {
                operation_id: "test-handoff".into(),
                workspace_id: None,
                project_path: "/work/demo".into(),
                branch: String::new(),
                mode: AgentLaunchMode::Build,
                permission: AgentPermission::AskApproval,
                agent: AgentId::Codex,
                prompt: String::new(),
                attachments: Vec::new(),
                skip_initial_prompt: false,
                extra_args: Vec::new(),
            },
            instruction: Some("Take over from here".into()),
        }
    }

    #[test]
    fn idle_source_hands_off_without_waiting() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let runtime = MockHandoffRuntime::with_identity(Some(live_agent(Some(matching_session()))));
        let outcome = run_live_handoff(
            &runtime,
            &FixedPreparation(PathBuf::from("/work/demo")),
            &store,
            &TransferLimits::default(),
            &request(dir.path(), source(dir.path())),
            1_000,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            *runtime.wait_calls.borrow(),
            0,
            "idle source settles immediately"
        );
        assert_eq!(*runtime.start_calls.borrow(), 1);
        assert_eq!(outcome.launch.pane_id, "pane-9");
    }

    #[test]
    fn working_source_waits_for_the_current_turn_and_flushes() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        // AC-08: the Host reads the status itself; the caller passes no flag.
        let working = agent_with_status(Some(matching_session()), "working");
        let idle = live_agent(Some(matching_session()));
        let runtime = MockHandoffRuntime {
            identity_agent: RefCell::new(Some(working)),
            settled_agent: Some(idle),
            settle_flush_path: Some(dir.path().join("live-session.jsonl")),
            ..Default::default()
        };
        let outcome = run_live_handoff(
            &runtime,
            &FixedPreparation(PathBuf::from("/work/demo")),
            &store,
            &TransferLimits::default(),
            &request(dir.path(), source(dir.path())),
            1_000,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            *runtime.wait_calls.borrow(),
            1,
            "working source waits exactly once"
        );
        assert_eq!(*runtime.start_calls.borrow(), 1);
        assert!(outcome.briefing_sha256.len() == 64);
    }

    #[test]
    fn blocked_source_needs_the_terminal_and_is_never_snapshotted() {
        // R2-08: Blocked is waiting-for-terminal-input, not a completed turn;
        // handoff must fail closed with zero target launches.
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let runtime = MockHandoffRuntime::with_identity(Some(agent_with_status(
            Some(matching_session()),
            "blocked",
        )));
        let result = run_live_handoff(
            &runtime,
            &FixedPreparation(PathBuf::from("/work/demo")),
            &store,
            &TransferLimits::default(),
            &request(dir.path(), source(dir.path())),
            1_000,
        );
        assert!(matches!(
            result,
            Err(LiveHandoffFailure::SourceBlocked { .. })
        ));
        assert_eq!(*runtime.start_calls.borrow(), 0);
    }

    #[test]
    fn settled_source_without_a_file_flush_fails_closed_instead_of_going_stale() {
        // AC-07: after the settle wait the provider file must advance; without
        // proof of the flush the handoff is WaitForSourceFlush, never a
        // hash-valid but stale "full context" transfer.
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let working = agent_with_status(Some(matching_session()), "working");
        let idle = live_agent(Some(matching_session()));
        let runtime = MockHandoffRuntime {
            identity_agent: RefCell::new(Some(working)),
            settled_agent: Some(idle),
            ..Default::default()
        };
        let result = run_live_handoff(
            &runtime,
            &FixedPreparation(PathBuf::from("/work/demo")),
            &store,
            &TransferLimits::default(),
            &request(dir.path(), source(dir.path())),
            1_000,
        );
        assert!(matches!(
            result,
            Err(LiveHandoffFailure::WaitForSourceFlush { .. })
        ));
        assert_eq!(*runtime.start_calls.borrow(), 0);
    }

    #[test]
    fn wait_failure_never_touches_the_target() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let working = agent_with_status(Some(matching_session()), "working");
        let runtime = MockHandoffRuntime {
            wait_error: Some("timeout"),
            identity_agent: RefCell::new(Some(working)),
            ..Default::default()
        };
        let result = run_live_handoff(
            &runtime,
            &FixedPreparation(PathBuf::from("/work/demo")),
            &store,
            &TransferLimits::default(),
            &request(dir.path(), source(dir.path())),
            1_000,
        );
        assert!(matches!(result, Err(LiveHandoffFailure::WaitFailed(_))));
        assert_eq!(*runtime.start_calls.borrow(), 0);
    }

    #[test]
    fn source_identity_change_fails_closed() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let mut changed = matching_session();
        changed.value = "someone-else".into();
        let runtime = MockHandoffRuntime::with_identity(Some(live_agent(Some(changed))));
        let result = run_live_handoff(
            &runtime,
            &FixedPreparation(PathBuf::from("/work/demo")),
            &store,
            &TransferLimits::default(),
            &request(dir.path(), source(dir.path())),
            1_000,
        );
        assert!(matches!(
            result,
            Err(LiveHandoffFailure::SourceIdentityChanged)
        ));
        assert_eq!(*runtime.start_calls.borrow(), 0);
    }

    #[test]
    fn missing_source_agent_fails_closed() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let runtime = MockHandoffRuntime::default();
        let result = run_live_handoff(
            &runtime,
            &FixedPreparation(PathBuf::from("/work/demo")),
            &store,
            &TransferLimits::default(),
            &request(dir.path(), source(dir.path())),
            1_000,
        );
        assert!(matches!(
            result,
            Err(LiveHandoffFailure::SourceUnresolved(_))
        ));
        assert_eq!(*runtime.start_calls.borrow(), 0);
    }
}
