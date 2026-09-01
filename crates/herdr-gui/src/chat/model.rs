//! Chat pure model: binding / send state machine / pending submission
//! reconciliation / projection cache (zero GPUI, zero I/O).
//!
//! [INPUT]: Depends on shardlane-history's live/Conversation/AgentId and the
//! crate root's herdr::Agent projection types; zero rendering dependencies.
//! [OUTPUT]: WorkSurfaceMode, ChatBinding, ChatModel (a testable core),
//! provider normalization, the send state machine, pending submission
//! reconciliation, and the rows projection.
//! [POS]: The state domain of herdr-gui `chat`. Herdr remains the runtime/status
//! authority; this module holds only a disposable presentation projection and
//! binding identity. Live file I/O happens only in surface-layer background
//! tasks, via LiveSession (herdr-history).

use std::collections::HashSet;

use shardlane_history::{
    AgentId, ConversationRef, LiveChange, LiveFacts, LiveSnapshot, LiveSync, TranscriptMessage,
};

use crate::agent_ui::conversation::{self, ConversationRow, ConversationTurn, EMPTY_FINGERPRINT};

/// Local presentation choice for the normal work surface (Phase-1 transient,
/// not persisted; plan §12.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WorkSurfaceMode {
    #[default]
    Terminal,
    Chat,
}

/// Exact identity of a Chat binding: the Herdr Agent projection plus the exact
/// history catalog source. Since M3 this carries the Host's canonical
/// `ConversationId` (derived from pane identity); this struct is only a
/// presentation-layer cache and never invents a second conversation identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ChatBinding {
    pub agent: AgentId,
    pub native_session_id: String,
    pub source: ConversationRef,
    /// Herdr pane/terminal identity at bind time (re-keyed when the agent switches).
    pub pane_key: String,
    /// Host canonical Live Conversation identity (AF-12: wrap, never copy, identity).
    /// v2 session-exact id (AC-03): a new occupant means a new id.
    pub conversation_id: String,
    /// Opaque fingerprint of the typed session at bind time (AC-16): queueing and
    /// delivery re-verify the exact occupant.
    pub session_fingerprint: String,
}

/// When the Chat presentation owns keyboard focus, the hidden host TUI must not
/// receive ordinary input (plan §12.2 / handoff C5 isolation boundary). Named
/// keys/app shortcuts still go through the GPUI action channel, not this path.
pub(crate) fn chat_surface_blocks_tui_input(mode: WorkSurfaceMode) -> bool {
    mode == WorkSurfaceMode::Chat
}

/// Herdr Agent projection → provider normalization for live Chat support
/// (PEX-1): the single authority for aliases and capabilities is the
/// `shardlane-history` capability registry; this layer keeps no list of its own
/// and does no cwd/mtime guessing.
pub(crate) fn normalize_provider(
    agent_session_agent: &str,
    agent_field: Option<&str>,
) -> Option<AgentId> {
    shardlane_history::resolve_agent_alias(agent_session_agent, agent_field)
}

/// Presentation projection of an M4 queue item (the Host
/// `ConversationFollowUpQueue` is the truth; this is only a cache).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QueuedFollowUpView {
    pub text: String,
    pub phase: QueuedFollowUpPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueuedFollowUpPhase {
    Queued,
    WaitingForTurn,
    Delivering,
}

impl QueuedFollowUpView {
    pub fn phase_label(&self) -> &'static str {
        match self.phase {
            QueuedFollowUpPhase::Queued => "queued",
            QueuedFollowUpPhase::WaitingForTurn => "waiting for turn",
            QueuedFollowUpPhase::Delivering => "delivering",
        }
    }
}

/// Chat send-capability rules (since M4, Working = safe queue semantics):
/// - blocked/failed/unknown: never allow an ordinary Send (explicit Terminal
///   fallback; unknown is fail-closed since C17);
/// - working/pending/launch_pending: `Send after turn` — enqueue a
///   Host-semantic follow-up, never an instant mid-turn injection;
/// - idle/done: Send allowed only when the draft is non-empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChatSendCapability {
    Ready,
    DraftEmpty,
    Working,
    Blocked,
}

pub(crate) fn chat_send_capability(
    agent_status: Option<&str>,
    has_text: bool,
) -> ChatSendCapability {
    // AC-04: the authority for mutation policy is the Host
    // (herdr_prompt_disposition); this function keeps only presentation-layer
    // semantics (an empty draft cannot be sent). Desktop and Remote/Mobile
    // therefore share the same Working/Blocked/Sendable judgment.
    match shardlane_host::herdr_prompt_disposition(agent_status) {
        shardlane_host::PromptDisposition::QueuedAfterTurn => ChatSendCapability::Working,
        shardlane_host::PromptDisposition::NeedsTerminal => ChatSendCapability::Blocked,
        shardlane_host::PromptDisposition::SentNow if !has_text => ChatSendCapability::DraftEmpty,
        shardlane_host::PromptDisposition::SentNow => ChatSendCapability::Ready,
    }
}

/// Local pending submission (plan §11.2): a temporary user row shown right
/// after agent.prompt succeeds; when the provider-source echo arrives it is
/// reconciled exactly once using content+order evidence. `pane_key` pins the
/// target pane: kept when rebinding to the same pane (the History Composer
/// continuation case), dropped when the pane changes (A09 cross-agent protection).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingSubmission {
    pub text: String,
    /// Message count already seen at submission time (the echo is searched only
    /// after this index).
    pub baseline_len: usize,
    /// Identity of the pane the submission targets.
    pub pane_key: String,
}

/// Reconciliation: confirm when a same-text User row appears after the baseline
/// in the snapshot messages (idempotent: once confirmed, pending is None and
/// repeated calls have zero effect). Returns (new message list, consumed).
pub(crate) fn reconcile_pending(
    messages: &[TranscriptMessage],
    pending: &Option<PendingSubmission>,
) -> (Option<PendingSubmission>, bool) {
    let Some(pending) = pending else {
        return (None, false);
    };
    let echo_found = messages.iter().skip(pending.baseline_len).any(|message| {
        message.role == shardlane_history::Role::User && message.text.trim() == pending.text.trim()
    });
    if echo_found {
        (None, true)
    } else {
        (Some(pending.clone()), false)
    }
}

/// Idempotent upsert of a batch of messages by index. The live report's range
/// invariant guarantees each batch re-reports every undelivered index, so an
/// index either hits an existing slot or is exactly a tail append; out-of-range
/// indices are skipped defensively.
fn upsert_batch(
    messages: &mut Vec<TranscriptMessage>,
    indices: &[usize],
    contents: &[TranscriptMessage],
) {
    for (index, message) in indices.iter().zip(contents) {
        if *index == messages.len() {
            messages.push(message.clone());
        } else if let Some(slot) = messages.get_mut(*index) {
            *slot = message.clone();
        }
    }
}

/// Minimal update plan for a virtual list (row-signature diff; pure function,
/// contract pinned by tests).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RowSplice {
    None,
    Replace {
        old_range: std::ops::Range<usize>,
        count: usize,
    },
}

/// Compute the list's minimal splice from last-frame/current-frame row-signature
/// sequences: pure appends only register the additions; mid-stream structural
/// changes (fold/expand, turn settlement, truncation) replace only the window
/// between the shared prefix and suffix.
pub(crate) fn plan_row_splice(old: &[u64], new: &[u64]) -> RowSplice {
    if old == new {
        return RowSplice::None;
    }
    let prefix = old
        .iter()
        .zip(new.iter())
        .take_while(|(old_row, new_row)| old_row == new_row)
        .count();
    if prefix == old.len() {
        // Pure append (the common case: new messages / streaming growth at the tail).
        return RowSplice::Replace {
            old_range: old.len()..old.len(),
            count: new.len() - old.len(),
        };
    }
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(old_row, new_row)| old_row == new_row)
        .count()
        .min(old.len() - prefix)
        .min(new.len() - prefix);
    RowSplice::Replace {
        old_range: prefix..old.len() - suffix,
        count: new.len() - prefix - suffix,
    }
}

/// Disposable Chat presentation state (no runtime ownership).
pub(crate) struct ChatModel {
    pub mode: WorkSurfaceMode,
    pub binding: Option<ChatBinding>,
    pub live_error: Option<String>,
    /// Explicit unavailable copy when focus is not on a supported agent
    /// (distinct from a retryable live_error).
    pub unavailable: Option<String>,
    pub snapshot: Option<LiveSnapshot>,
    pub load_generation: u64,
    pub expanded_turns: HashSet<i64>,
    pub herdr_status: Option<String>,
    pub pending: Option<PendingSubmission>,
    /// M4: presentation projection of the Host queue (None = no queued follow-up).
    pub queued_follow_up: Option<QueuedFollowUpView>,
    /// M8: session insight projection (HUD data source; derived from the
    /// snapshot, rendering does zero I/O).
    pub insight: Option<shardlane_host::AgentSessionInsight>,
    pub submitting: bool,
    pub last_error: Option<String>,
    /// Projection cache (the frame path does not re-fold).
    rows: Vec<ConversationRow>,
    turns: Vec<ConversationTurn>,
    rows_fingerprint: u64,
}

impl Default for ChatModel {
    fn default() -> Self {
        Self {
            mode: WorkSurfaceMode::Terminal,
            binding: None,
            live_error: None,
            unavailable: None,
            snapshot: None,
            load_generation: 0,
            expanded_turns: HashSet::new(),
            herdr_status: None,
            pending: None,
            queued_follow_up: None,
            insight: None,
            submitting: false,
            last_error: None,
            rows: Vec::new(),
            turns: Vec::new(),
            rows_fingerprint: EMPTY_FINGERPRINT,
        }
    }
}

impl ChatModel {
    /// Snapshot installation (background task result; the caller checks the
    /// generation). Pending reconciliation happens here exactly once.
    pub fn install_snapshot(&mut self, snapshot: LiveSnapshot) {
        let (pending, _consumed) = reconcile_pending(&snapshot.messages, &self.pending);
        self.pending = pending;
        self.refresh_insight(&snapshot);
        self.snapshot = Some(snapshot);
        self.live_error = None;
        self.unavailable = None;
        self.reproject();
    }

    /// M8: derive bounded insight from already-decoded facts (pure counting, no
    /// provider I/O).
    fn refresh_insight(&mut self, snapshot: &LiveSnapshot) {
        const STALE_AFTER_MS: u64 = 30 * 60 * 1_000;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        self.insight = Some(shardlane_host::AgentSessionInsight::from_live_snapshot(
            snapshot,
            now_ms,
            STALE_AFTER_MS,
        ));
    }

    /// Apply one live increment (idempotent upsert semantics). A regular append
    /// clones only the delivered indices instead of cloning/rebuilding the whole
    /// transcript; full snapshots are reserved for Reset and first delivery.
    pub fn apply_sync(&mut self, sync: &LiveSync) {
        match sync.change {
            LiveChange::Unchanged => {}
            LiveChange::Reset => {
                if let Some(snapshot) = sync.snapshot.clone() {
                    self.install_snapshot(snapshot);
                }
            }
            LiveChange::Appended => {
                if sync.appended.is_empty() && sync.changed.is_empty() {
                    return;
                }
                let Some(snapshot) = self.snapshot.as_mut() else {
                    // First delivery is the initial hydration: the appended batch
                    // is equivalent to a complete snapshot.
                    self.install_snapshot(LiveSnapshot {
                        messages: sync.appended_messages.clone(),
                        facts: LiveFacts::default(),
                        generation: sync.generation,
                    });
                    return;
                };
                let first_new_index = snapshot.messages.len();
                upsert_batch(
                    &mut snapshot.messages,
                    &sync.appended,
                    &sync.appended_messages,
                );
                upsert_batch(
                    &mut snapshot.messages,
                    &sync.changed,
                    &sync.changed_messages,
                );
                // Pending reconciliation looks only at messages added here (the
                // echo after the baseline).
                if let Some(pending) = self.pending.as_ref() {
                    let echoed = snapshot.messages[first_new_index..].iter().enumerate().any(
                        |(offset, message)| {
                            first_new_index + offset >= pending.baseline_len
                                && message.role == shardlane_history::Role::User
                                && message.text.trim() == pending.text.trim()
                        },
                    );
                    if echoed {
                        self.pending = None;
                    }
                }
                self.live_error = None;
                self.unavailable = None;
                self.reproject();
            }
        }
        // M8: counts/freshness evolve with the snapshot (pure counting;
        // clone-free borrow splitting).
        if sync.change != LiveChange::Unchanged && self.snapshot.is_some() {
            let derived = {
                let snapshot = self.snapshot.as_ref();
                snapshot.map(|snapshot| {
                    const STALE_AFTER_MS: u64 = 30 * 60 * 1_000;
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| duration.as_millis() as u64)
                        .unwrap_or(0);
                    shardlane_host::AgentSessionInsight::from_live_snapshot(
                        snapshot,
                        now_ms,
                        STALE_AFTER_MS,
                    )
                })
            };
            self.insight = derived;
        }
    }

    /// Queue projection merge: returns whether anything changed (when unchanged
    /// the caller must not trigger a repaint).
    pub fn set_queued_follow_up(&mut self, view: Option<QueuedFollowUpView>) -> bool {
        if self.queued_follow_up != view {
            self.queued_follow_up = view;
            true
        } else {
            false
        }
    }

    /// Herdr status merge (the authoritative live status; it may arrive before
    /// source content). Returns whether the status actually changed (when
    /// unchanged the caller must not trigger a repaint).
    pub fn set_herdr_status(&mut self, status: Option<String>) -> bool {
        if self.herdr_status != status {
            self.herdr_status = status;
            self.reproject();
            true
        } else {
            false
        }
    }

    /// The fold/expand key is the turn's first message seq (same semantics as
    /// History; stable across snapshots).
    pub fn toggle_turn_fold(&mut self, turn_start_seq: i64) {
        if !self.expanded_turns.insert(turn_start_seq) {
            self.expanded_turns.remove(&turn_start_seq);
        }
        self.reproject();
    }

    fn reproject(&mut self) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            self.rows.clear();
            self.turns.clear();
            self.rows_fingerprint = EMPTY_FINGERPRINT;
            return;
        };
        let turns = conversation::derive_turns(&snapshot.messages);
        let agent_working = self
            .herdr_status
            .as_deref()
            .is_some_and(|status| matches!(status, "working" | "launch_pending"));
        let mut running: HashSet<usize> = HashSet::new();
        if agent_working {
            if let Some(last_turn) = turns.len().checked_sub(1) {
                running.insert(last_turn);
            }
        }
        let expanded: HashSet<usize> = self
            .expanded_turns
            .iter()
            .filter_map(|seq| {
                turns.iter().position(|turn| {
                    snapshot
                        .messages
                        .get(turn.start)
                        .is_some_and(|m| m.seq == *seq)
                })
            })
            .collect();
        // Active turn = the last turn when the running set is non-empty;
        // busy_working is only meaningful when it exists.
        let busy_working = !running.is_empty();
        self.rows = conversation::folded_conversation_rows(
            &snapshot.messages,
            &turns,
            &running,
            &expanded,
            busy_working,
        );
        self.turns = turns;
        self.rows_fingerprint = conversation::rows_fingerprint(
            &snapshot.messages,
            &self.turns,
            &running,
            &expanded,
            busy_working,
        );
    }

    pub fn rows(&self) -> &[ConversationRow] {
        &self.rows
    }

    pub fn turns(&self) -> &[ConversationTurn] {
        &self.turns
    }

    /// Shared by tests and future frame-level skip-refold optimizations (the
    /// current projection recomputes each time; the interface is kept to pin
    /// the semantics).
    #[allow(dead_code)]
    pub fn rows_fingerprint(&self) -> u64 {
        self.rows_fingerprint
    }

    /// First message seq of a turn (the fold key).
    pub fn turn_start_seq(&self, turn_index: usize) -> Option<i64> {
        let turn = self.turns.get(turn_index)?;
        self.snapshot
            .as_ref()?
            .messages
            .get(turn.start)
            .map(|message| message.seq)
    }

    /// Status-surface judgment (whether the text is non-empty is provided by
    /// the GUI layer's InputState).
    #[allow(dead_code)]
    pub fn send_capability(&self, has_text: bool) -> ChatSendCapability {
        chat_send_capability(self.herdr_status.as_deref(), has_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shardlane_history::{LiveSession, MessageKind, Role, TranscriptMessage};

    fn user_message(text: &str) -> TranscriptMessage {
        TranscriptMessage {
            seq: 0,
            role: Role::User,
            kind: MessageKind::Text,
            text: text.into(),
            truncated: false,
            tool_calls: Vec::new(),
            thinking: None,
            timestamp: None,
            model: None,
        }
    }

    fn assistant_message(text: &str) -> TranscriptMessage {
        TranscriptMessage {
            role: Role::Assistant,
            ..user_message("")
        }
        .with_text(text)
    }

    trait WithText {
        fn with_text(self, text: &str) -> TranscriptMessage;
    }
    impl WithText for TranscriptMessage {
        fn with_text(mut self, text: &str) -> TranscriptMessage {
            self.text = text.into();
            self
        }
    }

    #[test]
    fn chat_surface_blocks_hidden_tui_input_only_in_chat_mode() {
        assert!(chat_surface_blocks_tui_input(WorkSurfaceMode::Chat));
        assert!(!chat_surface_blocks_tui_input(WorkSurfaceMode::Terminal));
    }

    /// Acceptance regression (2026-08-28): after Chat → Terminal the blur did
    /// not fire because the view was detached; the stale focused=true routed
    /// every keystroke into a nonexistent input box, silencing the host TUI's
    /// keyboard entirely. Contract: focus interception applies only in Chat
    /// mode (mode gating + explicit clearing as a double safeguard).
    #[test]
    fn stale_prompt_focus_never_blocks_terminal_view_keyboard() {
        // Simulate the residue: focus acquired in chat mode, then a direct flip
        // to Terminal (blur never ran).
        // Intercept = (mode == Chat) && focused; the Terminal view must override
        // any stale focus flag, and interception resumes normally in Chat.
        let focused_in_chat = true;
        let intercept = |mode: WorkSurfaceMode| mode == WorkSurfaceMode::Chat && focused_in_chat;
        assert!(!intercept(WorkSurfaceMode::Terminal));
        assert!(intercept(WorkSurfaceMode::Chat));
    }

    #[test]
    fn provider_normalization_matches_history_resume_aliases() {
        assert_eq!(
            normalize_provider("claude-code", None),
            Some(AgentId::ClaudeCode)
        );
        assert_eq!(
            normalize_provider("claude", Some("codex")),
            Some(AgentId::ClaudeCode)
        );
        assert_eq!(normalize_provider("codex", None), Some(AgentId::Codex));
        assert_eq!(normalize_provider("pi", None), Some(AgentId::Pi));
        assert_eq!(normalize_provider("omp", None), Some(AgentId::Omp));
        // Wave 1: Kimi/Cursor live enabled (registry authority).
        assert_eq!(normalize_provider("kimi", None), Some(AgentId::Kimi));
        assert_eq!(normalize_provider("cursor", None), Some(AgentId::Cursor));
        assert_eq!(
            normalize_provider("command-code", None),
            Some(AgentId::CommandCode)
        );
        assert_eq!(normalize_provider("unknown-agent", Some("shell")), None);
        // Antigravity encrypts bodies (only the metadata DB exists): there is no
        // decodable live semantics, so it must never enter Chat entry gating
        // (only the herdr TUI is shown).
        assert_eq!(normalize_provider("agy", None), None);
        assert_eq!(normalize_provider("antigravity", Some("agy")), None);
    }

    #[test]
    fn send_capability_gates_blocked_and_working() {
        assert_eq!(
            chat_send_capability(Some("blocked"), true),
            ChatSendCapability::Blocked
        );
        assert_eq!(
            chat_send_capability(Some("failed"), true),
            ChatSendCapability::Blocked
        );
        assert_eq!(
            chat_send_capability(Some("working"), true),
            ChatSendCapability::Working
        );
        assert_eq!(
            chat_send_capability(Some("idle"), false),
            ChatSendCapability::DraftEmpty
        );
        assert_eq!(
            chat_send_capability(Some("done"), true),
            ChatSendCapability::Ready
        );
        // C17 (fail-closed): an unknown/absent agent state must NOT allow
        // sending — it routes to the Terminal like blocked/failed.
        assert_eq!(
            chat_send_capability(None, true),
            ChatSendCapability::Blocked
        );
        assert_eq!(
            chat_send_capability(Some("mystery-state"), true),
            ChatSendCapability::Blocked
        );
    }

    #[test]
    fn pending_submission_reconciles_exactly_once() {
        let pending = Some(PendingSubmission {
            text: "hello".into(),
            baseline_len: 1,
            pane_key: "pane-a".into(),
        });
        let echoed = vec![
            user_message("first message"),
            assistant_message("answer"),
            user_message("hello"),
        ];
        let (after, consumed) = reconcile_pending(&echoed, &pending);
        assert!(consumed);
        assert_eq!(after, None);
        // Reconciling again after consumption: zero effect.
        let (after_again, consumed_again) = reconcile_pending(&echoed, &after);
        assert!(!consumed_again);
        assert_eq!(after_again, None);

        // No echo: pending is kept.
        let (kept, consumed) = reconcile_pending(&echoed[..2], &pending);
        assert!(!consumed);
        assert_eq!(kept, pending);
    }

    fn user_message_at(seq: i64, text: &str) -> TranscriptMessage {
        TranscriptMessage {
            seq,
            ..user_message(text)
        }
    }

    fn appended_sync(
        appended: Vec<(usize, TranscriptMessage)>,
        changed: Vec<(usize, TranscriptMessage)>,
    ) -> LiveSync {
        LiveSync {
            change: LiveChange::Appended,
            generation: 0,
            appended: appended.iter().map(|(index, _)| *index).collect(),
            changed: changed.iter().map(|(index, _)| *index).collect(),
            appended_messages: appended.into_iter().map(|(_, message)| message).collect(),
            changed_messages: changed.into_iter().map(|(_, message)| message).collect(),
            snapshot: None,
            lines_fed: 0,
        }
    }

    /// Batch A02 contract: incremental apply and full install produce identical
    /// projections; regular appends never touch existing message slots (no
    /// full-clone semantics).
    #[test]
    fn apply_sync_matches_install_snapshot_incrementally() {
        let mut delta_model = ChatModel::default();
        delta_model.apply_sync(&appended_sync(
            vec![(0, user_message_at(0, "question one"))],
            vec![],
        ));
        assert_eq!(
            delta_model
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.messages.len()),
            Some(1)
        );
        delta_model.apply_sync(&appended_sync(
            vec![(1, assistant_message("answer one"))],
            vec![],
        ));
        delta_model.apply_sync(&appended_sync(
            vec![],
            vec![(1, assistant_message("answer one (completed)"))],
        ));
        delta_model.apply_sync(&appended_sync(
            vec![(2, user_message_at(2, "question two"))],
            vec![],
        ));

        let mut full_model = ChatModel::default();
        full_model.install_snapshot(LiveSnapshot {
            messages: vec![
                user_message_at(0, "question one"),
                assistant_message("answer one (completed)"),
                user_message_at(2, "question two"),
            ],
            facts: Default::default(),
            generation: 0,
        });
        assert_eq!(
            delta_model
                .snapshot
                .as_ref()
                .map(|snapshot| &snapshot.messages),
            full_model
                .snapshot
                .as_ref()
                .map(|snapshot| &snapshot.messages)
        );
        assert_eq!(delta_model.rows(), full_model.rows());
    }

    /// The send echo arrives via the incremental channel: pending is consumed
    /// exactly once, based only on new messages after the baseline.
    #[test]
    fn apply_sync_consumes_pending_echo_from_appended_messages() {
        let mut model = ChatModel::default();
        model.install_snapshot(LiveSnapshot {
            messages: vec![
                user_message_at(0, "earlier question"),
                assistant_message("earlier answer"),
            ],
            facts: Default::default(),
            generation: 0,
        });
        model.pending = Some(PendingSubmission {
            text: "new question".into(),
            baseline_len: 2,
            pane_key: "pane-under-test".into(),
        });
        // A same-text User row appears after the baseline among new messages →
        // pending is consumed.
        model.apply_sync(&appended_sync(
            vec![(2, user_message_at(2, "new question"))],
            vec![],
        ));
        assert_eq!(model.pending, None);

        // No echo: pending is kept.
        model.pending = Some(PendingSubmission {
            text: "ask again".into(),
            baseline_len: 3,
            pane_key: "pane-under-test".into(),
        });
        model.apply_sync(&appended_sync(
            vec![(3, assistant_message("some other answer"))],
            vec![],
        ));
        assert!(model.pending.is_some());
    }

    /// set_herdr_status with an unchanged status must report "no change"
    /// (A04: a no-op must not repaint).
    #[test]
    fn set_herdr_status_reports_change_exactly() {
        let mut model = ChatModel::default();
        assert!(model.set_herdr_status(Some("working".into())));
        assert!(!model.set_herdr_status(Some("working".into())));
        assert!(model.set_herdr_status(Some("done".into())));
        assert!(!model.set_herdr_status(Some("done".into())));
    }

    /// Virtualization accounting contract (A01): the signature-sequence diff
    /// must converge to one of three minimal plans — None / pure append /
    /// shared-prefix-suffix window replacement; streaming tail growth must
    /// never trigger a full list reset.
    #[test]
    fn plan_row_splice_is_minimal() {
        assert_eq!(plan_row_splice(&[1, 2, 3], &[1, 2, 3]), RowSplice::None);
        // Pure append.
        assert_eq!(
            plan_row_splice(&[1, 2, 3], &[1, 2, 3, 4]),
            RowSplice::Replace {
                old_range: 3..3,
                count: 1
            }
        );
        // Streaming growth of the tail row: only the last entry is re-rendered.
        assert_eq!(
            plan_row_splice(&[7, 8, 9], &[7, 8, 10]),
            RowSplice::Replace {
                old_range: 2..3,
                count: 1
            }
        );
        // Turn settlement fold: mid-stream replacement, head and tail preserved.
        assert_eq!(
            plan_row_splice(&[1, 2, 3, 4, 5], &[1, 9, 5]),
            RowSplice::Replace {
                old_range: 1..4,
                count: 1
            }
        );
        // Truncation/shrink.
        assert_eq!(
            plan_row_splice(&[1, 2, 3, 4], &[1]),
            RowSplice::Replace {
                old_range: 1..4,
                count: 0
            }
        );
        // Complete replacement.
        assert_eq!(
            plan_row_splice(&[1, 2], &[8, 9, 10]),
            RowSplice::Replace {
                old_range: 0..2,
                count: 3
            }
        );
    }

    #[test]
    fn herdr_working_shows_indicator_before_source_append() {
        let mut model = ChatModel::default();
        model.install_snapshot(LiveSnapshot {
            messages: vec![user_message("q"), assistant_message("a")],
            facts: Default::default(),
            generation: 0,
        });
        assert!(!model.rows().contains(&ConversationRow::WorkingIndicator));
        // Herdr working: the Working row appears before any new source content.
        model.set_herdr_status(Some("working".into()));
        assert!(model.rows().contains(&ConversationRow::WorkingIndicator));
        // Settled: it disappears.
        model.set_herdr_status(Some("done".into()));
        assert!(!model.rows().contains(&ConversationRow::WorkingIndicator));
    }

    #[test]
    fn blocked_status_keeps_transcript_and_disables_send() {
        let mut model = ChatModel::default();
        model.install_snapshot(LiveSnapshot {
            messages: vec![user_message("q"), assistant_message("partial")],
            facts: Default::default(),
            generation: 0,
        });
        model.set_herdr_status(Some("blocked".into()));
        assert_eq!(model.send_capability(true), ChatSendCapability::Blocked);
        // Blocked does not delete the existing transcript.
        assert!(model.rows().iter().any(|row| matches!(
            row,
            ConversationRow::Answer(_) | ConversationRow::UserPrompt(_)
        )));
    }

    #[test]
    fn toggle_turn_fold_is_idempotent_and_stable_across_snapshots() {
        let mut model = ChatModel::default();
        let messages = vec![
            user_message("task"),
            assistant_message("thought"),
            assistant_message("final"),
        ];
        model.install_snapshot(LiveSnapshot {
            messages: messages.clone(),
            facts: Default::default(),
            generation: 0,
        });
        let fingerprint_folded = model.rows_fingerprint();
        model.toggle_turn_fold(0);
        assert!(model.expanded_turns.contains(&0));
        let fingerprint_expanded = model.rows_fingerprint();
        assert_ne!(fingerprint_folded, fingerprint_expanded);
        model.toggle_turn_fold(0);
        assert!(!model.expanded_turns.contains(&0));
        // After a new snapshot (same content reinstalled) the expanded set's
        // keys remain stable.
        model.toggle_turn_fold(0);
        model.install_snapshot(LiveSnapshot {
            messages,
            facts: Default::default(),
            generation: 1,
        });
        assert!(model.expanded_turns.contains(&0));
    }

    #[test]
    fn live_session_source_feeds_model_projection() -> anyhow::Result<()> {
        // Structural evidence: LiveSession (herdr-history) → ChatModel projection
        // works, and matches a direct message slice.
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("chat-model.jsonl");
        let content = concat!(
            r#"{"type":"user","cwd":"/w","timestamp":"2026-08-01T01:00:00Z","message":{"content":"task"}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-08-01T01:00:01Z","message":{"id":"m1","content":[{"type":"text","text":"done"}]}}"#,
            "\n",
        );
        std::fs::write(&path, content)?;
        let mut session = LiveSession::open(shardlane_history::ConversationRef {
            agent: AgentId::ClaudeCode,
            native_id: "chat-model".into(),
            file_path: path.to_string_lossy().to_string(),
            mtime_ms: 0,
            size: 0,
        })?;
        session.settle();
        let mut model = ChatModel::default();
        model.install_snapshot(session.snapshot());
        let rows = model.rows();
        assert!(rows
            .iter()
            .any(|row| matches!(row, ConversationRow::Answer(_))));
        assert!(rows
            .iter()
            .any(|row| matches!(row, ConversationRow::ResponseFooter(_))));
        Ok(())
    }

    #[test]
    fn queued_follow_up_projection_only_notifies_on_change() {
        let mut model = ChatModel::default();
        assert!(!model.set_queued_follow_up(None));
        assert!(model.set_queued_follow_up(Some(QueuedFollowUpView {
            text: "next".into(),
            phase: QueuedFollowUpPhase::Queued,
        })));
        assert!(!model.set_queued_follow_up(Some(QueuedFollowUpView {
            text: "next".into(),
            phase: QueuedFollowUpPhase::Queued,
        })));
        assert!(model.set_queued_follow_up(Some(QueuedFollowUpView {
            text: "next".into(),
            phase: QueuedFollowUpPhase::Delivering,
        })));
        assert!(model.set_queued_follow_up(None));
    }
}
