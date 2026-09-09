//! Chat work surface: binding resolution, live sync, rendering, and Herdr prompt submission.
//!
//! [INPUT]: Depends on chat::model's pure model, agent_ui's conversation/activity/
//! markdown primitives and AgentComposer, herdr::HerdrClient (agent.prompt / agent
//! projection), and shardlane-history's HistoryCatalog/LiveSession/LiveSync.
//! [OUTPUT]: Provides ChatUi (GUI-domain state) and ShardlaneApp's chat method group
//! (toggle_chat_surface / chat_surface_view / submit_chat_prompt, etc.) to the crate.
//! toggle_chat_surface writes the Chat/Terminal choice through to the per-instance
//! persisted workspace state (config.json workspace_state).
//! [POS]: The surface domain of herdr-gui `chat`. The TUI always stays alive; Chat is
//! only the semantic sidecar presentation of the same Herdr Agent: it never launches
//! provider processes, never parses TUI/ANSI, and ordinary prompts go only through
//! the Herdr agent.prompt and only in safe states, with blocked always falling back
//! explicitly to Terminal. LiveSession is owned exclusively by a background worker
//! (file I/O/decoding never touches the UI thread); the UI consumes only LiveSync
//! incremental upserts; the transcript is virtualized with a GPUI list (Bottom
//! anchoring = geometric tail-following; when content is shorter than one screen the
//! viewport flips to Top-aligned, with append tail-following compensated via
//! following_tail), so render work grows only with visible rows. The agent
//! identity+status title has moved up into the centered window Header
//! (notate 2026-08-29); the content area has no separate status bar.

use std::time::Duration;

use gpui::AppContext as _;
use gpui::{
    div, px, AnyElement, App, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Styled as _, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{InputEvent, InputState},
    v_flex, Icon, IconName as ComponentIconName, Sizable as _, WindowExt as _,
};
use shardlane_history::{
    resolve_session_source_locator, AgentId, ConversationRef, HistoryCatalog, LiveChange, LiveSync,
    SessionSourceLocator,
};

use super::model::{
    chat_send_capability, normalize_provider, ChatBinding, ChatModel, ChatSendCapability,
    PendingSubmission, QueuedFollowUpPhase, QueuedFollowUpView, WorkSurfaceMode,
};
use crate::agent_ui::activity;
use crate::agent_ui::composer::{AgentComposer, ComposerSendState};
use crate::agent_ui::conversation::ConversationRow;
use crate::agent_ui::conversation_surface::{
    conversation_row_signature, conversation_surface, ConversationSurfaceProps,
    ConversationViewportState, PENDING_ROW_SIGNATURE,
};
use crate::agent_ui::conversation_view;
use crate::agent_ui::markdown::palette_source_from_active;
use crate::agent_ui::markdown::render::{Metrics, Palette};
use crate::ui_metrics::SPACE_ICON;
use crate::{ContentSurfaceTheme, ShardlaneApp};
use gpui::prelude::FluentBuilder as _;

/// Slow-paced retry while unbound (source not ready yet).
const CHAT_UNBOUND_RETRY: Duration = Duration::from_secs(2);
/// Fallback sync cadence when the FS wake is missing/failing (never waited on
/// the normal path when the watcher is established).
/// audit CHAT-A04: events are the primary path; the timer is only a safety net
/// for an unavailable watcher or lost events.
const CHAT_SYNC_FALLBACK: Duration = Duration::from_secs(1);
/// Minimum interval for the status fallback RPC (time-driven; decoupled from
/// the event sync cadence).
const CHAT_STATUS_POLL_INTERVAL: Duration = Duration::from_secs(1);
/// Upper bound on the settle wait for M4 follow-up delivery; on timeout the
/// queue is kept and re-driven by status polling.
/// Raw source-byte budget for the Markdown view cache (the parsed structure is
/// roughly 17x the source; the budget counts source bytes).
const CHAT_MD_BUDGET_BYTES: usize = 2 << 20;
/// Live session lease held by the worker (M3: LiveSession state is owned by the
/// Host ConversationSessionManager; the worker only drives incremental sync and
/// holds the FS wake).
struct WorkerLive {
    source: ConversationRef,
    lease: shardlane_host::LiveLease,
    _wake: Option<shardlane_history::live::wake::FileWake>,
    wake_rx: Option<async_channel::Receiver<()>>,
}

impl WorkerLive {
    fn new(
        manager: &std::sync::Arc<shardlane_host::ConversationSessionManager>,
        source: ConversationRef,
    ) -> anyhow::Result<Self> {
        let file_ref = shardlane_history::models::SessionFileRef {
            agent: source.agent,
            native_id: source.native_id.clone(),
            file_path: source.file_path.clone(),
            mtime_ms: source.mtime_ms,
            size: source.size,
        };
        let lease = manager.subscribe(file_ref)?;
        let wake = shardlane_history::live::wake::FileWake::start(&source.file_path);
        let wake_rx = wake.as_ref().map(|wake| wake.receiver());
        Ok(Self {
            source,
            lease,
            _wake: wake,
            wake_rx,
        })
    }
}

/// Chat's GUI-domain state (disposable; no runtime ownership; LiveSession lives
/// in the worker).
#[derive(Default)]
pub(crate) struct ChatUi {
    pub(crate) model: ChatModel,
    /// R4-P0-03: logical prompt request id (bound to the text; reused on
    /// ambiguous retries, released on a definitive outcome) — paired with the
    /// Host ledger.
    pub(crate) pending_prompt_request: Option<PendingPromptOperation>,
    /// The handoff's logical operation id is bound to the exact source/target;
    /// when the target exists but a later phase is uncertain, clicking again
    /// must reuse the same Host launch-ledger entry.
    pub(crate) pending_handoff_operation: Option<PendingHandoffOperation>,
    /// Chat's own prompt input (independent from New Agent; same shared
    /// Composer shell).
    pub(crate) prompt: Option<Entity<InputState>>,
    pub(crate) prompt_subscription: Option<gpui::Subscription>,
    /// Both the focus and blur subscriptions must be held (dropping either one
    /// leaves the focus state stale).
    pub(crate) prompt_focus_subscriptions: Vec<gpui::Subscription>,
    pub(crate) prompt_focused: bool,
    /// Real handle of the live sync worker: dropping it cancels (released on
    /// hide/re-key).
    pub(crate) sync_job: Option<crate::BackgroundJob<()>>,
    pub(crate) sync_generation: u64,
    /// M4 follow-up delivery transaction handle + generation (a re-key
    /// invalidates in-flight applications).
    /// Window handle captured at enqueue time (the resume path needs a Window
    /// to backfill the draft).
    /// M7: Live Handoff transaction in-flight flag (progress and entry
    /// re-entrancy protection).
    pub(crate) handoff_in_flight: bool,
    /// M8: session insight HUD expanded state.
    pub(crate) hud_open: bool,
    /// Catalog exact-lookup in-flight flag (prevents duplicate spawns during a
    /// slow scan; cleared on re-key).
    pub(crate) lookup_in_flight: bool,
    /// Shared ConversationSurface viewport (virtual list + row signatures +
    /// Markdown cache + selection + tool expansion + overlay scrollbar + scroll
    /// intents).
    pub(crate) viewport: ConversationViewportState,
    /// Find within conversation (⌘F, notate 08-29 round five).
    pub(crate) find: Option<crate::agent_ui::find::ConversationFind>,
    pub(crate) find_focused: bool,
    /// Active pending/historical interactions (from the Host
    /// ConversationInteractionBroker).
    pub(crate) active_interactions:
        Vec<shardlane_host::conversation_interactions::ConversationInteraction>,
    /// The user's selections on interaction cards (a set of choice ids).
    pub(crate) selected_interaction_choices: std::collections::HashSet<String>,
    /// The user's custom input text on interaction cards.
    pub(crate) custom_interaction_input: Option<String>,
    /// Interaction response submission in-flight flag (prevents double submits).
    pub(crate) interaction_in_flight: bool,
}

/// Logical Prompt identity is bound to the exact Conversation as well as its
/// text.  Reusing a request id after switching panes would otherwise produce a
/// misleading ledger fingerprint conflict in the new Conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingPromptOperation {
    pub(crate) conversation_id: String,
    pub(crate) request_id: String,
    pub(crate) text: String,
}

/// Logical Live Handoff identity.  The source Conversation is part of the
/// fingerprint so a pane switch can never reuse an operation id for another
/// source; the target/project pair keeps an explicit retry on the same intent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingHandoffOperation {
    pub(crate) fingerprint: String,
    pub(crate) operation_id: String,
}

/// Chat entry capability check (PEX-1): the capability registry is the single
/// authority.
/// Chat technical capability (live decoding + transport).
pub(crate) fn chat_supported(agent: AgentId) -> bool {
    shardlane_history::provider_capabilities(agent)
        .is_some_and(|caps| caps.live == shardlane_history::LiveCapability::AppendLog)
}

/// Chat product-visibility gate (R4): technical capability + exposure not
/// Hidden + user enabled.
pub(crate) fn chat_entry_allowed(
    agent: AgentId,
    config: &crate::settings::ApplicationConfig,
) -> bool {
    chat_supported(agent)
        && shardlane_history::provider_exposed(agent)
        && config.providers.is_enabled(agent)
}

/// Display-name list of supported providers (registry-derived; only
/// product-visible entries).
pub(crate) fn chat_supported_labels() -> String {
    shardlane_history::live_capable_agents()
        .iter()
        .filter(|agent| shardlane_history::provider_exposed(**agent))
        .map(|agent| agent.display_name())
        .collect::<Vec<_>>()
        .join(" / ")
}

impl ShardlaneApp {
    /// Whether the Chat composer holds keyboard focus (used as the input-surface
    /// guard by handle_keystroke).
    /// Applies only in Chat presentation mode: when switching back to Terminal,
    /// if the blur never fired because the view was detached (acceptance
    /// regression: a stale true routed every keystroke into a nonexistent input
    /// box, silencing the host TUI's keyboard entirely), mode gating guarantees
    /// the Terminal view is never affected by stale state.
    pub(crate) fn chat_prompt_focused(&self) -> bool {
        self.chat.model.mode == WorkSurfaceMode::Chat && self.chat.prompt_focused
    }

    /// The supported agent matching the focused pane (Chat entry condition).
    pub(crate) fn focused_chat_agent(&self) -> Option<(&crate::herdr::Agent, AgentId)> {
        let focused_pane = self.state.focused_pane_id.clone();
        let agents = &self.state.agents;
        let candidate = agents
            .iter()
            .find(|agent| {
                agent
                    .pane_id
                    .as_deref()
                    .is_some_and(|pane_id| Some(pane_id) == focused_pane.as_deref())
            })
            .or_else(|| agents.iter().find(|agent| agent.focused))?;
        let session = candidate.agent_session.as_ref()?;
        let provider = normalize_provider(&session.agent, candidate.agent.as_deref())?;
        // R4: Chat entry = technical capability + exposure + user enabled
        // (disabled does not kill the runtime, it only stops entering the Chat
        // semantic surface).
        chat_entry_allowed(provider, &self.config).then_some((candidate, provider))
    }

    /// Terminal ⇄ Chat presentation switch. The TUI stays alive throughout; the
    /// switch never touches Herdr focus.
    pub(crate) fn toggle_chat_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = match self.chat.model.mode {
            WorkSurfaceMode::Terminal => {
                if self.focused_chat_agent().is_none() {
                    window.push_notification(
                        format!("Chat needs a supported {} agent", chat_supported_labels()),
                        cx,
                    );
                    return;
                }
                WorkSurfaceMode::Chat
            }
            WorkSurfaceMode::Chat => WorkSurfaceMode::Terminal,
        };
        self.chat.model.mode = next;
        self.clear_ime_state();
        self.sync_terminal_application_focus(cx);
        if next == WorkSurfaceMode::Chat {
            self.ensure_chat_source(cx);
            self.start_chat_sync_worker(cx);
            if let Some(prompt) = self.chat.prompt.as_ref() {
                prompt.update(cx, |state, input_cx| state.focus(window, input_cx));
            }
        } else {
            // Hide means stop: dropping the worker handle cancels it; focus flags
            // are cleared as well (view detachment may not fire blur, and a stale
            // flag would disable Terminal's keyboard).
            self.chat.sync_generation = self.chat.sync_generation.wrapping_add(1);
            self.chat.sync_job = None;
            self.chat.prompt_focused = false;
        }
        self.persist_current_workspace_state(cx);
        cx.notify();
    }

    /// Binding resolution (full focus-identity validation, safe to call every
    /// frame):
    /// - supported agent + exact session (typed locator: NativeId/FilePath) →
    ///   Bind / Rebind;
    /// - supported agent but session projection not yet reported → keep the
    ///   same-pane binding, otherwise clear the old binding and enter
    ///   connecting;
    /// - session identity present but the locator is MetadataOnly (unknown kind
    ///   / no semantic source) → explicit failure, never bind, never guess;
    /// - non-agent / unsupported focus → Park (clear binding and presentation,
    ///   never show an old conversation).
    ///
    /// While the source is not ready, show the connecting state; never degrade
    /// into cwd/mtime guessing.
    pub(crate) fn ensure_chat_source(&mut self, cx: &mut Context<Self>) {
        let focused = {
            let found = self.focused_chat_agent();
            found.map(|(agent, provider)| {
                (
                    provider,
                    agent
                        .pane_id
                        .clone()
                        .unwrap_or_else(|| agent.terminal_id.clone()),
                    agent.agent_session.as_ref().map(|session| {
                        (
                            session.kind.clone(),
                            session.source.clone(),
                            session.value.clone(),
                        )
                    }),
                )
            })
        };
        let Some((provider, pane_key, session_info)) = focused else {
            self.park_chat_unavailable();
            return;
        };
        // Supported agent but session identity not yet reported: a projection
        // gap. Keep an existing same-pane binding, otherwise clear the old
        // pane's binding (never show an old conversation across panes) and
        // enter connecting.
        let Some((kind, source, value)) = session_info else {
            if self
                .chat
                .model
                .binding
                .as_ref()
                .is_some_and(|binding| binding.pane_key == pane_key)
            {
                return;
            }
            self.reset_chat_presentation(None);
            self.chat.model.binding = None;
            self.chat.model.unavailable = None;
            self.chat.model.live_error = Some("Connecting conversation…".to_string());
            return;
        };
        // CHAT-A10: typed resolution of the Herdr kind/source/value contract
        // (pure function, fully synchronous); the GUI no longer guesses paths
        // from the value's shape (contains `/`, whether it is a file).
        let locator = resolve_session_source_locator(provider, &kind, &source, &value);
        if !matches!(
            locator,
            SessionSourceLocator::NativeId { .. } | SessionSourceLocator::FilePath { .. }
        ) {
            // Session identity present but Herdr reported a semantic source kind
            // we cannot consume: explicit failure, never bind, never retry by
            // guessing.
            self.reset_chat_presentation(None);
            self.chat.model.binding = None;
            self.chat.model.unavailable = None;
            self.chat.model.live_error =
                Some("This session has no readable semantic source for Chat".to_string());
            return;
        }
        let native_id = locator.native_identity().to_string();
        self.chat.model.unavailable = None;
        // M4: refresh the queued projection from the Host queue every frame
        // (queue items of different panes do not affect each other).
        {
            let agent_ref = shardlane_host::AgentRef::new(pane_key.clone());
            let queued_view =
                self.follow_up_queue
                    .queued(&agent_ref)
                    .map(|item| QueuedFollowUpView {
                        text: item.text.clone(),
                        phase: match item.state {
                            shardlane_host::QueueState::Queued => QueuedFollowUpPhase::Queued,
                            shardlane_host::QueueState::WaitingForTurnBoundary => {
                                QueuedFollowUpPhase::WaitingForTurn
                            }
                            shardlane_host::QueueState::Delivering => {
                                QueuedFollowUpPhase::Delivering
                            }
                            _ => QueuedFollowUpPhase::Queued,
                        },
                    });
            if self.chat.model.set_queued_follow_up(queued_view) {
                cx.notify();
            }
        }
        let matches_current = self.chat.model.binding.as_ref().is_some_and(|binding| {
            binding.pane_key == pane_key
                && binding.native_session_id == native_id
                && binding.agent == provider
        });
        if !matches_current {
            // (Re)bind: rebuild the presentation with a new generation; in-flight
            // lookups are invalidated. Same-pane local pending is kept (History
            // Composer continuation → Live takeover, R3).
            self.chat.lookup_in_flight = false;
            let keep_pane = pane_key.clone();
            self.reset_chat_presentation(Some(&keep_pane));
            // AC-03/AC-16: bind the session-exact v2 ConversationId and the typed
            // session fingerprint; a new occupant means a new binding identity,
            // and queueing/delivery fail closed on that basis.
            let typed_session = shardlane_host::herdr::AgentSessionInfo {
                agent: shardlane_host::herdr_agent_kind(provider).to_string(),
                kind,
                source,
                value,
            };
            let session_fingerprint = shardlane_host::session_fingerprint(&typed_session);
            self.chat.model.binding = Some(ChatBinding {
                agent: provider,
                native_session_id: native_id.clone(),
                source: ConversationRef {
                    agent: provider,
                    native_id: native_id.clone(),
                    file_path: String::new(),
                    mtime_ms: 0,
                    size: 0,
                },
                conversation_id: shardlane_host::conversation_id_for_live_session(
                    &shardlane_host::AgentRef::new(pane_key.clone()),
                    &typed_session,
                )
                .as_str()
                .to_string(),
                pane_key,
                session_fingerprint,
            });
            self.chat.model.load_generation += 1;
        }
        if self.chat.lookup_in_flight {
            return;
        }
        if self
            .chat
            .model
            .binding
            .as_ref()
            .is_some_and(|binding| !binding.source.file_path.is_empty())
        {
            return;
        }
        // Exact lookup (background): includes the rescan when the source is not
        // ready yet (the fix for Retry previously having no effect on existing
        // bindings).
        self.chat.lookup_in_flight = true;
        let generation = self.chat.model.load_generation;
        let roster = self.history.roster.clone();
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    match locator {
                        // kind=path: the value is the session file path, used
                        // directly as the exact source.
                        SessionSourceLocator::FilePath { agent, path } => {
                            if !roster.owns_active_path(agent, &path) {
                                return anyhow::Ok(None);
                            }
                            let native_id = std::path::Path::new(&path)
                                .file_stem()
                                .and_then(|stem| stem.to_str())
                                .unwrap_or(path.as_str())
                                .to_string();
                            let (mtime_ms, size) = std::fs::metadata(&path)
                                .map(|meta| {
                                    use std::os::unix::fs::MetadataExt as _;
                                    (meta.mtime() * 1000, meta.len() as i64)
                                })
                                .unwrap_or((0, 0));
                            anyhow::Ok(Some(ConversationRef {
                                agent,
                                native_id,
                                file_path: path,
                                mtime_ms,
                                size,
                            }))
                        }
                        // kind=id: exact lookup through the catalog's (agent, native_id).
                        SessionSourceLocator::NativeId { agent, native_id } => {
                            let db_path = crate::history::transcript_source::history_db_path();
                            let catalog = HistoryCatalog::open(&db_path)
                                .or_else(|_| HistoryCatalog::open_initialized(&db_path))?;
                            let source = catalog.session_source_by_native(agent, &native_id)?;
                            anyhow::Ok(source.filter(|source| {
                                roster.owns_active_path(source.agent, &source.file_path)
                            }))
                        }
                        // Already intercepted above; defensively unreachable.
                        SessionSourceLocator::MetadataOnly { .. } => anyhow::Ok(None),
                    }
                })
                .await;
            this.update(cx, |view, cx| {
                if view.chat.model.load_generation != generation {
                    return;
                }
                view.chat.lookup_in_flight = false;
                match loaded {
                    Ok(Some(source)) => {
                        if let Some(binding) = view.chat.model.binding.as_mut() {
                            binding.source = source;
                        }
                        view.chat.model.live_error = None;
                        cx.notify();
                    }
                    Ok(None) => {
                        // The session file is not in the catalog yet (the window
                        // right after a new agent starts): connecting state.
                        view.chat.model.live_error = Some("Connecting conversation…".to_string());
                        cx.notify();
                    }
                    Err(error) => {
                        view.chat.model.live_error = Some(error.to_string());
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    /// Focus is not on a supported agent: clear binding and presentation
    /// (Park). Idempotent; the worker stays in the slow-retry state and
    /// automatically rebinds once focus returns to a supported agent.
    fn park_chat_unavailable(&mut self) {
        let message = format!(
            "Chat needs a supported {} agent in the focused pane",
            chat_supported_labels()
        );
        if self.chat.model.binding.is_none()
            && self.chat.model.unavailable.as_deref() == Some(message.as_str())
        {
            return;
        }
        self.reset_chat_presentation(None);
        self.chat.model.binding = None;
        self.chat.model.unavailable = Some(message);
        self.chat.model.live_error = None;
        self.chat.model.load_generation += 1;
        self.chat.lookup_in_flight = false;
    }

    /// Rebuild the presentation state wholesale (shared by rebind/park); list
    /// state is re-keyed as well (Bottom anchoring = pinned to the bottom on
    /// the first frame). When `keep_pending_for` is given, the same pane's
    /// local pending is not dropped (the History Composer continuation → Live
    /// takeover case, R3).
    fn reset_chat_presentation(&mut self, keep_pending_for: Option<&str>) {
        let keep_pending = keep_pending_for.is_some_and(|pane_key| {
            self.chat
                .model
                .pending
                .as_ref()
                .is_some_and(|pending| pending.pane_key == pane_key)
        });
        self.chat.model.snapshot = None;
        if !keep_pending {
            self.chat.model.pending = None;
        }
        self.chat.model.submitting = false;
        self.chat.find = None;
        self.chat.viewport.reset();
    }

    /// Live sync worker: sole LiveSession ownership; alive only in Chat
    /// presentation mode.
    /// File metadata/read/decode all run in the background; the UI consumes
    /// only LiveSync upserts.
    /// The wait side is driven primarily by the FS signal (FileWake) with a
    /// bounded timer as fallback; signal storms are collapsed via bounded(1),
    /// and bursts catch up immediately by draining the channel. When the
    /// binding identity (agent + file_path) changes the worker re-keys the
    /// session itself, and UI state is protected by rebind generation bumping.
    pub(crate) fn start_chat_sync_worker(&mut self, cx: &mut Context<Self>) {
        if self.chat.sync_job.is_some() {
            return;
        }
        self.chat.sync_generation = self.chat.sync_generation.wrapping_add(1);
        let generation = self.chat.sync_generation;
        let manager = self.conversation_sessions.clone();
        self.chat.sync_job = Some(cx.spawn(async move |this, cx| {
            let mut active: Option<WorkerLive> = None;
            let mut resync_immediately = false;
            let mut next_status_poll = std::time::Instant::now();
            loop {
                // Lifecycle gate: alive only in Chat presentation mode (handle
                // drop is the double-safeguard cancellation).
                let alive = this
                    .update(cx, |view, _| {
                        view.chat.model.mode == WorkSurfaceMode::Chat
                            && view.chat.sync_generation == generation
                    })
                    .unwrap_or(false);
                if !alive {
                    return;
                }
                // Expected identity = the current binding source. Missing
                // binding / source not ready → slow-paced bind retry.
                let desired = this
                    .update(cx, |view, _| {
                        view.chat
                            .model
                            .binding
                            .as_ref()
                            .filter(|binding| !binding.source.file_path.is_empty())
                            .map(|binding| binding.source.clone())
                    })
                    .unwrap_or(None);
                let source = match desired {
                    Some(source) => source,
                    None => {
                        active = None;
                        cx.background_executor().timer(CHAT_UNBOUND_RETRY).await;
                        this.update(cx, |view, cx| view.ensure_chat_source(cx)).ok();
                        continue;
                    }
                };
                let session_matches = active.as_ref().is_some_and(|live| {
                    live.source.agent == source.agent && live.source.file_path == source.file_path
                });
                if !session_matches {
                    // (Re)open: subscribe to the shared LiveSession via the Host
                    // session manager, hydrate existing content in the
                    // background + establish the file wake + first delivery.
                    active = None;
                    let open_manager = manager.clone();
                    let opened = cx
                        .background_executor()
                        .spawn(async move {
                            let live = WorkerLive::new(&open_manager, source)?;
                            // open pre-advance accounting: the initial content must
                            // be delivered explicitly, otherwise the first sync is
                            // Unchanged and the UI stays on Connecting forever.
                            let initial = open_manager.initial_delivery(&live.lease)?;
                            anyhow::Ok((live, initial))
                        })
                        .await;
                    match opened {
                        Ok((live, initial)) => {
                            let applied = this.update(cx, |view, cx| {
                                if view.chat.model.mode != WorkSurfaceMode::Chat
                                    || view.chat.sync_generation != generation
                                {
                                    return false;
                                }
                                view.apply_chat_sync_result(Some(Ok(initial)), cx);
                                true
                            });
                            if !applied.unwrap_or(false) {
                                return;
                            }
                            active = Some(live);
                        }
                        Err(error) => {
                            this.update(cx, |view, cx| {
                                view.chat.model.live_error = Some(error.to_string());
                                cx.notify();
                            })
                            .ok();
                            cx.background_executor().timer(CHAT_UNBOUND_RETRY).await;
                            continue;
                        }
                    }
                    // Sync once immediately in the first round (captures appends
                    // right after open).
                    resync_immediately = true;
                }
                // Wait side: FS signal primary, bounded timer fallback; skips the
                // wait when catching up on a burst.
                if !resync_immediately {
                    resync_immediately = false;
                    let wake_rx = active.as_ref().and_then(|live| live.wake_rx.clone());
                    let fallback = cx.background_executor().timer(CHAT_SYNC_FALLBACK);
                    match wake_rx {
                        Some(rx) => {
                            let wake = rx.recv();
                            futures::pin_mut!(wake);
                            futures::pin_mut!(fallback);
                            // Either wake source arriving first is equivalent:
                            // bounded(1) already collapsed the storm into one
                            // commit; a closed channel is treated as the
                            // fallback path.
                            let _ = futures::future::select(wake, fallback).await;
                        }
                        None => {
                            futures::pin_mut!(fallback);
                            let _ = fallback.await;
                        }
                    }
                }
                // Background incremental sync: the shared session syncs
                // thread-safely through the Host manager, so the UI thread does
                // zero I/O.
                // The lease clone participates briefly (reference counting keeps
                // the session alive) and is released afterwards.
                let sync_manager = manager.clone();
                let sync_lease = active.as_ref().map(|live| live.lease.clone());
                let outcome: Option<anyhow::Result<LiveSync>> = if let Some(lease) = sync_lease {
                    Some(
                            cx.background_executor()
                                .spawn(async move {
                                    sync_manager.sync(&lease).map_err(anyhow::Error::from)
                                })
                                .await,
                        )
                } else {
                    None
                };
                let applied = this.update(cx, |view, cx| {
                    if view.chat.model.mode != WorkSurfaceMode::Chat
                        || view.chat.sync_generation != generation
                    {
                        return false;
                    }
                    view.apply_chat_sync_result(outcome, cx);
                    true
                });
                if !applied.unwrap_or(false) {
                    return;
                }
                // Burst catch-up: more signals arrived while waiting → sync
                // another round immediately (skipping the wait).
                if active
                    .as_ref()
                    .and_then(|live| live.wake_rx.as_ref())
                    .is_some_and(|rx| rx.try_recv().is_ok())
                {
                    resync_immediately = true;
                }
                // Authoritative status fallback (time-driven, low frequency):
                // the event projection may lag (observed: the TUI had finished
                // but agent_status was still working). Matches by pane/terminal
                // dual identity and notifies only when the status actually
                // changes.
                let now = std::time::Instant::now();
                if now >= next_status_poll {
                    next_status_poll = now + CHAT_STATUS_POLL_INTERVAL;
                    let fetch = this
                        .update(cx, |view, _| {
                            let pane_key = view
                                .chat
                                .model
                                .binding
                                .as_ref()
                                .map(|binding| binding.pane_key.clone());
                            match pane_key {
                                Some(pane_key) => Some((pane_key, view.client.clone())),
                                None => None,
                            }
                        })
                        .ok()
                        .flatten();
                    if let Some((pane_key, Some(client))) = fetch {
                        let status = cx
                            .background_executor()
                            .spawn(async move {
                                client.agents().ok().and_then(|agents| {
                                    agents
                                        .into_iter()
                                        .find(|agent| {
                                            agent.pane_id.as_deref() == Some(pane_key.as_str())
                                                || (agent.pane_id.is_none()
                                                    && agent.terminal_id == pane_key)
                                        })
                                        .and_then(|agent| {
                                            agent.agent_status.or(agent.custom_status)
                                        })
                                })
                            })
                            .await;
                        this.update(cx, |view, cx| {
                            if view.chat.model.set_herdr_status(status) {
                                if view
                                    .chat
                                    .model
                                    .herdr_status
                                    .as_deref()
                                    .is_some_and(crate::chat::model::is_working_family)
                                {
                                    view.schedule_working_indicator_repaint(cx);
                                }
                                cx.notify();
                            }
                            // M4: re-drive queued follow-ups after settle/unblock.
                            view.maybe_resume_follow_up_delivery(cx);
                        })
                        .ok();
                    }
                }
            }
        }));
    }

    /// One-shot delayed repaint for the anti-flicker Working indicator: the
    /// projection only reveals the Working row after the delay, and a quiet
    /// transcript produces no other wake — without this repaint the row would
    /// stay invisible until the next file activity.
    fn schedule_working_indicator_repaint(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(crate::chat::model::WORKING_INDICATOR_DELAY)
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    /// Apply one sync result from the worker. Unchanged means zero action and
    /// zero repaint (A04: a resolved no-op must not trigger a repaint);
    /// changes/resets are handed to the pure model's incremental apply.
    fn apply_chat_sync_result(
        &mut self,
        result: Option<anyhow::Result<LiveSync>>,
        cx: &mut Context<Self>,
    ) {
        match result {
            None => {}
            Some(Err(error)) => {
                self.chat.model.live_error = Some(error.to_string());
                cx.notify();
            }
            Some(Ok(sync)) => {
                if sync.is_unchanged() {
                    return;
                }
                if sync.change == LiveChange::Reset {
                    // Re-key rebuild: the Markdown view cache is invalidated
                    // along with it, avoiding cross-generation remounting.
                    self.chat.viewport.markdown.clear();
                }
                self.chat.model.apply_sync(&sync);
                cx.notify();
            }
        }
    }

    /// Chat prompt submission (C7): one Send is exactly one agent.prompt;
    /// blocked/working are rejected; on failure the draft is kept for retry.
    /// Asynchronous completion must verify the binding identity and generation
    /// captured at submission (A09: a completion after switching agents must
    /// not pollute the current presentation).
    /// CR-05/R2-02: Chat sending goes only through the authoritative Host
    /// transaction (submit_conversation_prompt): fresh-occupant validation
    /// (conv_2 fingerprint) + fresh status + Host disposition; queueing is
    /// delivered by the Host coordinator. `chat_send_capability` is only the
    /// button presentation hint and no longer chooses the mutation path.
    pub(crate) fn submit_chat_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.chat.prompt.as_ref() else {
            return;
        };
        let text = prompt.read(cx).value().trim().to_string();
        if text.is_empty() || self.chat.model.submitting {
            return;
        }
        // R4-P1: no presentation-side early return. The stale UI status must
        // never BLOCK the authoritative Host transaction — send always goes
        // through submit_conversation_prompt, and the Host decides.
        let Some(binding) = self.chat.model.binding.clone() else {
            return;
        };
        let Some(client) = self.client.clone() else {
            self.chat.model.last_error = Some("Herdr runtime is unavailable".to_string());
            cx.notify();
            return;
        };
        let target_conversation =
            shardlane_host::ConversationId::new(binding.conversation_id.clone());
        let baseline_len = self
            .chat
            .model
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.messages.len())
            .unwrap_or(0);
        let prompt_entity = prompt.clone();
        // Completion guard identity: pane + native session + generation (A09).
        let guard_pane = binding.pane_key.clone();
        let guard_native = binding.native_session_id.clone();
        let guard_generation = self.chat.model.load_generation;
        // R4-P0-03 (Desktop half): a logical mutation id lives as long as the
        // ambiguous retry — the Host ledger (P0-03) makes the reuse safe and
        // a fresh id after a response loss would bypass it. A different text
        // is a new logical mutation and gets a new id.
        let request_id = match self.chat.pending_prompt_request.clone() {
            Some(pending)
                if pending.conversation_id == binding.conversation_id && pending.text == text =>
            {
                pending.request_id
            }
            _ => format!(
                "chat-{}-{}",
                binding.pane_key,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|since| since.as_millis())
                    .unwrap_or(0)
            ),
        };
        self.chat.pending_prompt_request = Some(PendingPromptOperation {
            conversation_id: binding.conversation_id.clone(),
            request_id: request_id.clone(),
            text: text.clone(),
        });
        let delivery = self.delivery.clone();
        self.chat.model.submitting = true;
        self.chat.model.last_error = None;
        let window_handle = window.window_handle();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let send_text = text.clone();
            let submit_text = send_text.clone();
            let submission = cx
                .background_executor()
                .spawn(async move {
                    let client = match client.as_herdr() {
                        Some(herdr) => herdr.clone(),
                        None => return Err("this instance does not support agents".to_string()),
                    };
                    let service = shardlane_host::HostConversationService::new(
                        &client,
                        std::path::PathBuf::new(),
                    );
                    service
                        .submit_conversation_prompt(
                            &target_conversation,
                            &delivery,
                            &request_id,
                            &submit_text,
                        )
                        .map_err(|error| error.to_string())
                })
                .await;
            let mut clear_prompt = false;
            let mut notify_terminal = false;
            this.update(cx, |view, cx| {
                view.chat.model.submitting = false;
                let guards_match = view.chat.model.binding.as_ref().is_some_and(|binding| {
                    binding.pane_key == guard_pane && binding.native_session_id == guard_native
                }) && view.chat.model.load_generation == guard_generation;
                let mut release_prompt_id = false;
                let mut prompt_uncertain = false;
                match submission {
                    Ok(result) => {
                        release_prompt_id = true;
                        match result.disposition {
                            shardlane_host::PromptDisposition::SentNow => {
                                if guards_match {
                                    view.chat.model.pending = Some(PendingSubmission {
                                        text: send_text.clone(),
                                        baseline_len,
                                        pane_key: guard_pane.clone(),
                                    });
                                }
                                clear_prompt = true;
                            }
                            shardlane_host::PromptDisposition::QueuedAfterTurn => {
                                // The Host coordinator owns delivery; locally we
                                // only refresh the queue projection.
                                view.sync_queued_follow_up_view(
                                    &shardlane_host::AgentRef::new(guard_pane.clone()),
                                    cx,
                                );
                                clear_prompt = true;
                            }
                            shardlane_host::PromptDisposition::NeedsTerminal => {
                                notify_terminal = true;
                            }
                        }
                    }
                    Err(error) => {
                        let uncertain = error.contains("uncertain") || error.contains("not read");
                        if uncertain {
                            prompt_uncertain = true;
                        } else {
                            release_prompt_id = true;
                        }
                        view.chat.model.last_error = Some(error);
                    }
                }
                if release_prompt_id {
                    view.chat.pending_prompt_request = None;
                }
                let _ = prompt_uncertain; // id intentionally retained for the retry
                cx.notify();
            })
            .ok();
            if notify_terminal {
                let _ = cx.update_window(window_handle, |_, window, cx| {
                    window.push_notification("Switch to Terminal for this interaction", cx);
                });
            }
            if clear_prompt {
                let _ = cx.update_window(window_handle, |_, window, cx| {
                    prompt_entity.update(cx, |state, input_cx| {
                        state.set_value("", window, input_cx);
                    });
                });
            }
        })
        .detach();
    }

    /// Refresh the presentation projection from the Host queue; repaint only on
    /// change.
    fn sync_queued_follow_up_view(
        &mut self,
        agent_ref: &shardlane_host::AgentRef,
        cx: &mut Context<Self>,
    ) {
        let view = self
            .follow_up_queue
            .queued(agent_ref)
            .map(|item| QueuedFollowUpView {
                text: item.text.clone(),
                phase: match item.state {
                    shardlane_host::QueueState::Queued => QueuedFollowUpPhase::Queued,
                    shardlane_host::QueueState::WaitingForTurnBoundary => {
                        QueuedFollowUpPhase::WaitingForTurn
                    }
                    shardlane_host::QueueState::Delivering => QueuedFollowUpPhase::Delivering,
                    shardlane_host::QueueState::Delivered
                    | shardlane_host::QueueState::Cancelled => QueuedFollowUpPhase::Queued,
                    shardlane_host::QueueState::FailedRecoverable(_)
                    | shardlane_host::QueueState::DeliveryUncertain(_) => {
                        QueuedFollowUpPhase::Queued
                    }
                },
            });
        if self.chat.model.set_queued_follow_up(view) {
            cx.notify();
        }
    }

    fn current_chat_agent_ref(&self) -> shardlane_host::AgentRef {
        shardlane_host::AgentRef::new(
            self.chat
                .model
                .binding
                .as_ref()
                .map(|binding| binding.pane_key.clone())
                .unwrap_or_default(),
        )
    }

    /// M7: perform a Live Handoff (Working source = `Handoff after current turn`).
    /// The source agent is never stopped; on success FocusIntent navigates to
    /// the target Chat.
    pub(crate) fn start_live_handoff(
        &mut self,
        target: shardlane_history::AgentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.chat.handoff_in_flight {
            return;
        }
        let Some(binding) = self.chat.model.binding.clone() else {
            return;
        };
        if binding.source.file_path.is_empty() {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(project_path) = self
            .chat
            .model
            .binding
            .as_ref()
            .and_then(|_| self.focused_chat_project_path())
            .filter(|path| !path.trim().is_empty())
            .or_else(|| {
                // Degraded: use the focused runtime workspace's Project path.
                self.focused_chat_project_path()
            })
        else {
            window.push_notification("Handoff needs a project path", cx);
            return;
        };
        let working = self
            .chat
            .model
            .herdr_status
            .as_deref()
            .map(|status| matches!(status, "working" | "launch_pending"))
            .unwrap_or(false);
        let source = shardlane_history::models::SessionFileRef {
            agent: binding.agent,
            native_id: binding.native_session_id.clone(),
            file_path: binding.source.file_path.clone(),
            mtime_ms: binding.source.mtime_ms,
            size: binding.source.size,
        };
        let instruction: Option<String> = None;
        let handoff_fingerprint = format!(
            "source-conversation={}|source-agent={}|source-native={}|source-path={}|target={}|project={}|instruction={}",
            binding.conversation_id,
            binding.pane_key,
            source.native_id,
            source.file_path,
            target.as_str(),
            project_path,
            instruction.as_deref().unwrap_or_default(),
        );
        let operation_id = match self.chat.pending_handoff_operation.as_ref() {
            Some(pending) if pending.fingerprint == handoff_fingerprint => {
                pending.operation_id.clone()
            }
            _ => format!(
                "handoff-{}-{}",
                binding.conversation_id,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default()
            ),
        };
        self.chat.pending_handoff_operation = Some(PendingHandoffOperation {
            fingerprint: handoff_fingerprint,
            operation_id: operation_id.clone(),
        });
        let request = shardlane_host::LiveHandoffRequest {
            source,
            source_agent_ref: shardlane_host::AgentRef::new(binding.pane_key.clone()),
            target_provider: target,
            launch: shardlane_host::AgentLaunchIntent {
                operation_id,
                workspace_id: None,
                project_path,
                branch: String::new(),
                mode: shardlane_host::AgentLaunchMode::Build,
                permission: shardlane_host::AgentPermission::AskApproval,
                agent: target,
                prompt: String::new(),
                attachments: Vec::new(),
                extra_args: Vec::new(),
                skip_initial_prompt: false,
            },
            instruction,
        };
        self.chat.handoff_in_flight = true;
        cx.notify();
        let window_handle = window.window_handle();
        let state_client = client.clone();
        let delivery = std::sync::Arc::clone(&self.delivery);
        let source_agent_ref = request.source_agent_ref.clone();
        cx.spawn(async move |this, cx| {
            let working_source = working;
            let result = cx
                .background_executor()
                .spawn(async move {
                    let app_data = crate::settings::app_data_dir();
                    let preparation =
                        shardlane_host::GitWorktreePreparation::new(app_data.join("worktrees"));
                    let store = shardlane_history::TransferArtifactStore::new(
                        app_data.join("transfer-artifacts"),
                    );
                    let limits = shardlane_history::TransferLimits::default();
                    // AC-08: the Host reads the authoritative status itself; this
                    // side no longer passes the UI-captured working flag.
                    let _ = working_source;
                    let client = match client.as_herdr() {
                        Some(herdr) => herdr.clone(),
                        None => {
                            return Err(shardlane_host::LiveHandoffFailure::WaitFailed(
                                "this instance does not support agents".to_string(),
                            ))
                        }
                    };
                    shardlane_host::run_live_handoff_with_delivery(
                        &client,
                        &preparation,
                        &store,
                        &limits,
                        &request,
                        45_000,
                        &delivery,
                    )
                })
                .await;
            let target_ids = match &result {
                Ok(outcome) => Some((
                    Some(outcome.launch.workspace_id.clone()),
                    outcome.launch.tab_id.clone(),
                    outcome.launch.pane_id.clone(),
                )),
                Err(shardlane_host::LiveHandoffFailure::CreatedNeedsAttention {
                    tab_id,
                    pane_id,
                    ..
                }) => Some((None, tab_id.clone(), pane_id.clone())),
                Err(_) => None,
            };
            let succeeded = this
                .update(cx, |view, cx| {
                    view.chat.handoff_in_flight = false;
                    match result {
                        Ok(outcome) => {
                            view.chat.pending_handoff_operation = None;
                            view.status = crate::ConnectionStatus::Connected;
                            if let Ok(state) = state_client.visible_state() {
                                view.state = state;
                            }
                            // Target structure applied + semantic binding → Chat;
                            // the source agent stays untouched.
                            view.apply_created_tab(
                                outcome.launch.created,
                                outcome.launch.layout,
                                Some(outcome.launch.workspace_id.clone()),
                                true,
                            );
                            if outcome.launch.identity.is_some() {
                                view.chat.model.mode = WorkSurfaceMode::Chat;
                                view.chat.model.load_generation += 1;
                            }
                            view.notify_sidebar(cx);
                            cx.notify();
                            true
                        }
                        Err(shardlane_host::LiveHandoffFailure::CreatedNeedsAttention {
                            agent_ref,
                            tab_id,
                            pane_id,
                            detail,
                            ..
                        }) => {
                            // Target creation committed. Preserve the target
                            // focus and expose the setup issue; the handoff
                            // action remains closed until the user reconciles
                            // this exact pane rather than launching another.
                            view.status = crate::ConnectionStatus::Connected;
                            if let Ok(state) = state_client.visible_state() {
                                view.state = state;
                            }
                            view.chat.model.last_error = Some(format!(
                                "Handoff target {} needs attention: {}",
                                agent_ref.as_str(),
                                detail
                            ));
                            view.chat.model.mode = WorkSurfaceMode::Terminal;
                            // Focus is applied after the spawned update via
                            // the existing window-handle seam below.
                            let _ = (tab_id, pane_id);
                            cx.notify();
                            true
                        }
                        Err(error) => {
                            // No created target was returned, so this
                            // operation can be deliberately retried with a
                            // fresh id after the source-side problem is fixed.
                            view.chat.pending_handoff_operation = None;
                            view.chat.model.last_error = Some(error.to_string());
                            cx.notify();
                            false
                        }
                    }
                })
                .unwrap_or(false);
            let _ = source_agent_ref;
            if let (true, Some((workspace_id, tab_id, pane_id))) = (succeeded, target_ids) {
                let _ = cx.update_window(window_handle, |_, window, cx| {
                    let _ = this.update(cx, |view, cx| {
                        view.complete_navigation_after_authoritative_selection(
                            workspace_id,
                            Some(tab_id),
                            Some(pane_id),
                            window,
                            cx,
                        );
                    });
                });
            }
        })
        .detach();
    }

    fn focused_chat_project_path(&self) -> Option<String> {
        self.state
            .focused_workspace_id
            .as_deref()
            .and_then(|workspace_id| {
                self.state
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == workspace_id)
            })
            .and_then(|workspace| workspace.cwd.clone())
    }

    /// Status polling hook: re-arm with the Host coordinator while queue items
    /// remain (idempotent; no-op if the worker is already running). The GUI no
    /// longer owns the delivery worker (R2-03/CR-05).
    fn maybe_resume_follow_up_delivery(&mut self, _cx: &mut Context<Self>) {
        let Some(binding) = self.chat.model.binding.clone() else {
            return;
        };
        let agent_ref = shardlane_host::AgentRef::new(binding.pane_key.clone());
        let Some(item) = self.follow_up_queue.queued(&agent_ref) else {
            return;
        };
        if matches!(
            item.state,
            shardlane_host::QueueState::Queued | shardlane_host::QueueState::WaitingForTurnBoundary
        ) {
            self.delivery.schedule(&agent_ref);
        }
    }

    /// M8: session insight HUD panel (only known fields; never guesses unknowns).
    fn chat_insight_panel(&mut self, theme: &ContentSurfaceTheme) -> AnyElement {
        let insight = self.chat.model.insight.clone().unwrap_or_default();
        let mut rows: Vec<(&'static str, String)> = Vec::new();
        if let Some(model) = insight.model.as_deref() {
            rows.push(("Model", model.to_string()));
        }
        if let Some(ctx_pct) = insight.context_used_percent {
            rows.push(("Context", format!("{ctx_pct:.1}% used")));
        } else if let Some(tokens) = insight.tokens_used {
            rows.push(("Context", format!("{tokens} tokens (provider-reported)")));
        }
        if let Some(cache_pct) = insight.cache_hit_percent {
            let ttl_str = insight
                .cache_ttl_secs
                .map(|s| format!(" · ttl≈{}m", s / 60))
                .unwrap_or_default();
            rows.push(("Cache", format!("{cache_pct:.1}% hit{ttl_str}")));
        }
        if let Some(allowance) = insight.allowance.as_ref() {
            let window = allowance
                .window
                .as_deref()
                .map(|window| format!(" ({window})"))
                .unwrap_or_default();
            rows.push((
                "Allowance",
                format!("{} / {}{window}", allowance.used, allowance.limit),
            ));
        }
        if let Some(duration) = insight.session_duration_ms {
            rows.push((
                "Session",
                format!("{}m {}s", duration / 60_000, (duration % 60_000) / 1_000),
            ));
        }
        rows.push(("Turns", insight.turn_count.to_string()));
        rows.push(("Tools", insight.tool_call_count.to_string()));
        rows.push(("Messages", insight.message_count.to_string()));
        if insight.compacted {
            rows.push((
                "Freshness",
                "compacted (context was summarized)".to_string(),
            ));
        } else if insight.stale {
            rows.push(("Freshness", "stale — no recent activity".to_string()));
        }
        let pressure = insight.pressure();
        let mut panel = v_flex()
            .w_full()
            .max_w(px(560.0))
            .mx_auto()
            .my_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .gap_1();
        for (label, value) in rows {
            panel = panel.child(
                h_flex()
                    .w_full()
                    .gap(px(12.0))
                    .text_size(crate::theme::FONT_META)
                    .child(
                        div()
                            .w(px(84.0))
                            .flex_shrink_0()
                            .text_color(theme.muted)
                            .child(label),
                    )
                    .child(div().min_w_0().truncate().child(value)),
            );
        }
        match pressure {
            shardlane_host::InsightPressure::HighContext => {
                panel = panel.child(
                    div()
                        .text_size(crate::theme::FONT_META)
                        .text_color(theme.danger)
                        .child("High context window usage (≥85%). Compaction recommended."),
                );
            }
            shardlane_host::InsightPressure::CriticalAllowance => {
                panel = panel.child(
                    div()
                        .text_size(crate::theme::FONT_META)
                        .text_color(theme.danger)
                        .child("Allowance nearly exhausted."),
                );
            }
            shardlane_host::InsightPressure::Stale => {
                panel = panel.child(
                    div()
                        .text_size(crate::theme::FONT_META)
                        .text_color(theme.muted)
                        .child("This session has been idle for a while."),
                );
            }
            _ => {}
        }
        panel.into_any_element()
    }

    /// M4 queue action row: Edit (cancel and backfill the draft) / Cancel
    /// (cancel only).
    /// Once Delivering/uncertain is reached, stop showing a fake cancel that
    /// could cause a duplicate send.
    fn queued_follow_up_actions(
        &mut self,
        theme: &ContentSurfaceTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cancellable = self
            .chat
            .model
            .queued_follow_up
            .as_ref()
            .is_some_and(|queued| {
                matches!(
                    queued.phase,
                    QueuedFollowUpPhase::Queued | QueuedFollowUpPhase::WaitingForTurn
                )
            });
        let edit_entity = cx.entity();
        let cancel_entity = cx.entity();
        let mut row = h_flex()
            .w_full()
            .min_w_0()
            .gap(px(8.0))
            .items_center()
            .text_size(crate::theme::FONT_META)
            .text_color(theme.muted);
        if cancellable {
            row = row
                .child(
                    Button::new("chat-follow-up-edit")
                        .ghost()
                        .xsmall()
                        .label("Edit")
                        .on_click(move |_, window, app| {
                            edit_entity.update(app, |this, cx| {
                                this.edit_queued_follow_up(window, cx);
                            });
                            let _ = window;
                        }),
                )
                .child(
                    Button::new("chat-follow-up-cancel")
                        .ghost()
                        .xsmall()
                        .label("Cancel")
                        .on_click(move |_, window, app| {
                            cancel_entity.update(app, |this, cx| {
                                this.cancel_queued_follow_up(cx);
                            });
                            let _ = window;
                        }),
                );
        } else {
            row = row.child(div().child("delivering…"));
        }
        row.into_any_element()
    }

    /// Edit: cancel the queued item and backfill its text into the draft
    /// (only before delivery starts).
    fn edit_queued_follow_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let agent_ref = self.current_chat_agent_ref();
        if let Some(text) = self.follow_up_queue.cancel(&agent_ref) {
            if let Some(prompt) = self.chat.prompt.clone() {
                prompt.update(cx, |state, input_cx| {
                    state.set_value(&text, window, input_cx);
                });
            }
        }
        self.sync_queued_follow_up_view(&agent_ref, cx);
    }

    /// Cancel: cancel the queued item only (only before delivery starts).
    fn cancel_queued_follow_up(&mut self, cx: &mut Context<Self>) {
        let agent_ref = self.current_chat_agent_ref();
        self.follow_up_queue.cancel(&agent_ref);
        self.sync_queued_follow_up_view(&agent_ref, cx);
    }

    /// Current selection text from drag-selection in the Chat body Markdown
    /// (the ⌘C copy exit, notate 08-29 round four).
    pub(crate) fn chat_transcript_selection_text(&self) -> Option<String> {
        self.chat
            .viewport
            .selection
            .selection
            .borrow()
            .selected_text()
    }

    // ── Find within conversation (⌘F, notate 08-29 round five) ──

    pub(crate) fn toggle_chat_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.chat.find.is_some() {
            self.conversation_find_close(window, cx);
            return;
        }
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("Find in conversation", window, cx);
            state
        });
        let herdr = cx.entity();
        let subscriptions = crate::agent_ui::find::subscribe_find_input(
            &input,
            &herdr,
            window,
            cx,
            |this, window, cx| this.chat_find_rerun(window, cx),
        );
        input.update(cx, |state, cx| state.focus(window, cx));
        self.chat.find = Some(crate::agent_ui::find::ConversationFind {
            input,
            subscriptions,
            hits: Vec::new(),
            active: 0,
        });
        cx.notify();
    }

    fn chat_find_rerun(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let query = self
            .chat
            .find
            .as_ref()
            .map(|find| find.input.read(cx))
            .map(|state| state.value().to_string())
            .unwrap_or_default();
        let messages: Vec<(i64, &str)> = self
            .chat
            .model
            .snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .messages
                    .iter()
                    .map(|message| (message.seq, message.text.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        let hits = crate::agent_ui::find::collect_find_hits(&messages, &query);
        if let Some(find) = self.chat.find.as_mut() {
            find.hits = hits;
            find.active = 0;
            let first = find.hits.first().map(|hit| hit.seq);
            if let Some(seq) = first {
                self.chat_scroll_to_seq(seq);
            }
        }
        cx.notify();
    }

    pub(crate) fn chat_find_step(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        step: isize,
    ) {
        let Some(find) = self.chat.find.as_mut() else {
            return;
        };
        if find.hits.is_empty() {
            return;
        }
        let len = find.hits.len() as isize;
        find.active = ((find.active as isize + step).rem_euclid(len)) as usize;
        let seq = find.hits[find.active].seq;
        self.chat_scroll_to_seq(seq);
        cx.notify();
        let _ = window;
    }

    pub(crate) fn chat_find_close(&mut self, cx: &mut Context<Self>) {
        if self.chat.find.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn chat_find_highlights_for(
        &self,
        seq: i64,
    ) -> Option<crate::agent_ui::markdown::render::SearchHighlights> {
        let find = self.chat.find.as_ref()?;
        crate::agent_ui::find::highlights_for(&find.hits, seq, find.active)
    }

    fn chat_scroll_to_seq(&mut self, seq: i64) {
        let messages = self.chat.model.snapshot.as_ref();
        let rows = self.chat.model.rows();
        let ix = rows.iter().position(|row| match row {
            crate::agent_ui::conversation::ConversationRow::UserPrompt(index)
            | crate::agent_ui::conversation::ConversationRow::ContextBoundary(index)
            | crate::agent_ui::conversation::ConversationRow::Answer(index) => messages
                .and_then(|snapshot| snapshot.messages.get(*index))
                .is_some_and(|message| message.seq == seq),
            crate::agent_ui::conversation::ConversationRow::ToolActivity { message, .. } => {
                messages
                    .and_then(|snapshot| snapshot.messages.get(*message))
                    .is_some_and(|message| message.seq == seq)
            }
            // A find hit inside a compacted run still scrolls to the group.
            crate::agent_ui::conversation::ConversationRow::ToolGroup { message, .. } => messages
                .and_then(|snapshot| snapshot.messages.get(*message))
                .is_some_and(|message| message.seq == seq),
            _ => false,
        });
        if let Some(ix) = ix {
            self.chat.viewport.list.scroll_to_reveal_item(ix);
        }
    }

    /// Render the Chat work surface (TUI liveness is guaranteed at the dispatch
    /// layer).
    /// The transcript is virtualized with a GPUI list: elements are built only
    /// for visible ± overdraw rows.
    pub(crate) fn chat_surface_view(
        &mut self,
        theme: &ContentSurfaceTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.ensure_chat_prompt(window, cx);
        // Worker idempotent self-healing (normally started by the toggle; rebuilt
        // as a fallback when the handle is missing).
        self.start_chat_sync_worker(cx);
        // The Herdr status is the authoritative live status: sync it with the
        // focused agent projection before rendering.
        if let Some((agent, _provider)) = self.focused_chat_agent() {
            let status = agent
                .agent_status
                .clone()
                .or_else(|| agent.custom_status.clone());
            if self.chat.model.set_herdr_status(status)
                && self
                    .chat
                    .model
                    .herdr_status
                    .as_deref()
                    .is_some_and(crate::chat::model::is_working_family)
            {
                self.schedule_working_indicator_repaint(cx);
            }
        }
        let palette = Palette::from_source(palette_source_from_active(cx));
        let connecting = self.chat.model.snapshot.is_none();

        // The agent identity+status bar (40px header) has moved up into the
        // centered window Header as the title; the content area no longer
        // renders a separate status bar (notate 2026-08-29 A2/A4). M8: when the
        // HUD is expanded, render the bounded insight panel here (data comes
        // from the Host insight projection, zero I/O).
        let header = if self.chat.hud_open {
            self.chat_insight_panel(theme).into_any_element()
        } else {
            div().into_any_element()
        };

        // Frame-path row signature diff: structural changes converge to a minimal
        // splice/reset (virtualization accounting).
        // When content is shorter than one screen the surface flips to Top
        // alignment (pinned to the top); for appends in the Top state,
        // tail-following is compensated by "pin to tail before sync → reveal the
        // last row after sync" (unneeded with Bottom anchoring).
        let was_following_tail = self.chat.viewport.following_tail();
        self.sync_chat_list_rows();
        if was_following_tail && self.chat.viewport.is_top_aligned() {
            let last_row =
                self.chat.model.rows().len() + usize::from(self.chat.model.pending.is_some());
            if last_row > 0 {
                self.chat.viewport.list.scroll_to_reveal_item(last_row - 1);
            }
        }
        let connecting_view = connecting.then(|| chat_connecting_view(self, cx.entity(), *theme));
        let weak = cx.entity().downgrade();
        let row_theme = *theme;
        let row_palette = palette;
        let composer = self.chat_composer(theme, window, cx);
        let pending_interaction = self
            .chat
            .active_interactions
            .iter()
            .find(|i| {
                i.state
                    == shardlane_host::conversation_interactions::ConversationInteractionState::Pending
            })
            .cloned();
        let composer_with_interaction = match pending_interaction {
            Some(interaction) => {
                let card = self.render_active_pending_interaction(&interaction, theme, cx);
                v_flex()
                    .w_full()
                    .gap(px(10.0))
                    .child(card)
                    .child(composer)
                    .into_any_element()
            }
            None => composer,
        };
        let render_row = Box::new(move |ix: usize, window: &mut Window, app: &mut App| {
            let Some(view) = weak.upgrade() else {
                return div().into_any_element();
            };
            view.update(app, |view, cx| {
                view.render_chat_row(ix, &row_theme, &row_palette, window, cx)
            })
        });
        let find_bar = self
            .chat
            .find
            .as_ref()
            .map(|find| crate::agent_ui::find::conversation_find_bar(find, &cx.entity(), cx));
        conversation_surface(ConversationSurfaceProps {
            viewport: &mut self.chat.viewport,
            theme,
            header,
            row_count: self.chat.model.rows().len()
                + usize::from(self.chat.model.pending.is_some()),
            render_row,
            overlay: connecting_view,
            find_bar,
            composer: Some(composer_with_interaction),
            pager: None,
        })
    }

    /// Row signature diff → minimal splice/reset. A signature = the row
    /// discriminant + a digest of the message content length;
    /// pure appends (the common case: new messages / streaming growth at the
    /// tail) cause zero resets and only register additions.
    fn sync_chat_list_rows(&mut self) {
        let messages = self
            .chat
            .model
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.messages.as_slice())
            .unwrap_or(&[]);
        let mut signatures: Vec<u64> = self
            .chat
            .model
            .rows()
            .iter()
            .map(|row| conversation_row_signature(messages, self.chat.model.turns(), row))
            .collect();
        if self.chat.model.pending.is_some() {
            signatures.push(PENDING_ROW_SIGNATURE);
        }
        self.chat.viewport.sync_rows(signatures);
    }

    /// Render one transcript row (the list callback; only visible rows are
    /// processed). A trailing local pending submission row occupies one
    /// virtual row slot.
    fn render_chat_row(
        &mut self,
        ix: usize,
        theme: &ContentSurfaceTheme,
        palette: &Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows_len = self.chat.model.rows().len();
        let metrics = Metrics::BODY;
        let row = self.chat.model.rows().get(ix).copied();
        // notate 08-29 round five: copyable rows (user/answer/pending) collect
        // their raw text for the hover copy button.
        let mut copy_text: Option<String> = None;
        let element = match row {
            Some(ConversationRow::UserPrompt(message_index)) => {
                let message = self
                    .chat
                    .model
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.messages.get(message_index))
                    .cloned();
                match message {
                    Some(message) => {
                        copy_text = Some(message.text.clone());
                        chat_user_prompt_row(&message, theme)
                    }
                    None => div().into_any_element(),
                }
            }
            Some(ConversationRow::Answer(message_index)) => {
                let message = self
                    .chat
                    .model
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.messages.get(message_index))
                    .cloned();
                match message {
                    Some(message) => {
                        copy_text = Some(message.text.clone());
                        self.chat_answer_row(&message, palette, metrics, theme, window, cx)
                    }
                    None => div().into_any_element(),
                }
            }
            Some(ConversationRow::ContextBoundary(message_index)) => {
                let Some(message) = self
                    .chat
                    .model
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.messages.get(message_index))
                    .cloned()
                else {
                    return div().into_any_element();
                };
                let seq = message.seq;
                let expanded = self.chat.model.context_boundary_expanded(seq);
                let toggle_herdr = cx.entity();
                div()
                    .w_full()
                    .min_w_0()
                    .child(conversation_view::context_boundary_row(
                        &message,
                        expanded,
                        Some(std::rc::Rc::new(move |_, app| {
                            toggle_herdr.update(app, |this, cx| {
                                this.chat.model.toggle_context_boundary(seq);
                                cx.notify();
                            });
                        })),
                        theme,
                    ))
                    .into_any_element()
            }
            Some(ConversationRow::TurnThinking(turn_index)) => {
                let Some(turn) = self.chat.model.turns().get(turn_index).cloned() else {
                    return div().into_any_element();
                };
                let Some(snapshot) = self.chat.model.snapshot.as_ref() else {
                    return div().into_any_element();
                };
                let thinking =
                    crate::agent_ui::conversation::turn_thinking_text(&snapshot.messages, &turn);
                if thinking.trim().is_empty() {
                    return div().into_any_element();
                }
                let live = self
                    .chat
                    .model
                    .herdr_status
                    .as_deref()
                    .is_some_and(|status| matches!(status, "working" | "launch_pending"));
                let is_last_turn = self.chat.model.turns().len().saturating_sub(1) == turn_index;
                // ChatGPT contract: the block streams its tail while the turn
                // is still thinking (busy, newest turn, no text yet); it
                // collapses with a measured duration once the turn talks. The
                // user's pin overrides everything.
                let streaming_thinking = live
                    && is_last_turn
                    && !crate::agent_ui::conversation::turn_has_visible_text(
                        &snapshot.messages,
                        &turn,
                    );
                let duration = if streaming_thinking {
                    None
                } else {
                    crate::agent_ui::conversation::turn_thinking_duration(&snapshot.messages, &turn)
                };
                let turn_seq = snapshot
                    .messages
                    .get(turn.start)
                    .map(|message| message.seq)
                    .unwrap_or(0);
                let expanded = self
                    .chat
                    .model
                    .reasoning_expanded(turn_seq, streaming_thinking);
                let toggle_herdr = cx.entity();
                let streaming_for_toggle = streaming_thinking;
                let pill = crate::agent_ui::conversation_view::reasoning_row(
                    &thinking,
                    crate::agent_ui::conversation_view::ReasoningPresentation::Live {
                        streaming_thinking,
                        duration,
                        expanded,
                        on_toggle: std::rc::Rc::new(move |_, app| {
                            toggle_herdr.update(app, |this, cx| {
                                this.chat
                                    .model
                                    .toggle_reasoning(turn_seq, streaming_for_toggle);
                                cx.notify();
                            });
                        }),
                    },
                    theme,
                );
                div()
                    .w_full()
                    .min_w_0()
                    .px(px(16.0))
                    .py(px(4.0))
                    .child(pill)
                    .into_any_element()
            }
            Some(ConversationRow::ToolActivity { message, tool }) => {
                // Rows re-emitted from an expanded group indent under the
                // group header.
                let in_group = ix > 0
                    && matches!(
                        self.chat.model.rows().get(ix - 1),
                        Some(ConversationRow::ToolGroup { .. })
                    );
                let tool_call = self
                    .chat
                    .model
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.messages.get(message))
                    .and_then(|message| message.tool_calls.get(tool))
                    .cloned();
                match tool_call {
                    Some(tool_call) => {
                        self.chat_tool_row(message, tool, &tool_call, in_group, theme, cx)
                    }
                    None => div().into_any_element(),
                }
            }
            Some(ConversationRow::ToolGroup {
                message,
                tool,
                count,
            }) => {
                let Some(seq) = self
                    .chat
                    .model
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.messages.get(message))
                    .map(|message| message.seq)
                else {
                    return div().into_any_element();
                };
                let expanded = self.chat.model.tool_group_expanded(seq, tool);
                let toggle_herdr = cx.entity();
                crate::agent_ui::activity::render_tool_group_row(
                    SharedString::from(format!("chat-tool-group-{seq}-{tool}")),
                    count,
                    expanded,
                    move |_, app| {
                        toggle_herdr.update(app, |this, cx| {
                            this.chat.model.toggle_tool_group(seq, tool);
                            cx.notify();
                        });
                    },
                    theme,
                )
            }
            Some(ConversationRow::ResponseFooter(turn_index)) => {
                self.chat_turn_footer_row(turn_index, theme)
            }
            Some(ConversationRow::WorkingIndicator) => conversation_view::working_row(theme),
            Some(ConversationRow::TurnStopped(_)) => conversation_view::stopped_row(theme),
            None => {
                // Virtual pending row slot (only when pending exists and ix is
                // exactly the last row).
                if ix == rows_len {
                    let pending = self.chat.model.pending.clone();
                    match pending {
                        Some(pending) => chat_pending_prompt_row(&pending.text, theme),
                        None => div().into_any_element(),
                    }
                } else {
                    div().into_any_element()
                }
            }
        };
        // Every row is wrapped in the same 720px reading column (same axis as
        // the Composer); copyable rows carry the msg-row group — hover reveals
        // the copy button in the bottom right (notate 08-29 round five).
        let row = div()
            .w_full()
            .min_w_0()
            .flex()
            .justify_center()
            .child(element);
        match copy_text {
            Some(text) if !text.trim().is_empty() => row
                .group("shardlane-msg-row")
                .relative()
                .child(conversation_view::hover_copy_message_button(
                    ("chat-msg-copy", ix as u64),
                    text,
                    theme,
                ))
                .into_any_element(),
            _ => row.into_any_element(),
        }
    }
    pub(crate) fn render_active_pending_interaction(
        &self,
        interaction: &shardlane_host::conversation_interactions::ConversationInteraction,
        theme: &ContentSurfaceTheme,
        cx: &Context<Self>,
    ) -> AnyElement {
        let entity = cx.entity();
        let selected = self.chat.selected_interaction_choices.clone();
        let custom_input = self.chat.custom_interaction_input.clone();
        let in_flight = self.chat.interaction_in_flight;

        let on_select = {
            let entity = entity.clone();
            std::rc::Rc::new(move |choice: &str, _window: &mut Window, app: &mut App| {
                let choice = choice.to_string();
                entity.update(app, |view, cx| {
                    view.chat.selected_interaction_choices.clear();
                    view.chat.selected_interaction_choices.insert(choice);
                    cx.notify();
                });
            })
        };

        let on_toggle = {
            let entity = entity.clone();
            std::rc::Rc::new(move |choice: &str, _window: &mut Window, app: &mut App| {
                let choice = choice.to_string();
                entity.update(app, |view, cx| {
                    if view.chat.selected_interaction_choices.contains(&choice) {
                        view.chat.selected_interaction_choices.remove(&choice);
                    } else {
                        view.chat.selected_interaction_choices.insert(choice);
                    }
                    cx.notify();
                });
            })
        };

        let on_custom = {
            let entity = entity.clone();
            std::rc::Rc::new(move |input: String, _window: &mut Window, app: &mut App| {
                entity.update(app, |view, cx| {
                    view.chat.custom_interaction_input = Some(input);
                    cx.notify();
                });
            })
        };

        let on_submit = {
            let entity = entity.clone();
            std::rc::Rc::new(
                move |resp: shardlane_host::conversation_interactions::InteractionResponse,
                      window: &mut Window,
                      app: &mut App| {
                    entity.update(app, |view, cx| {
                        view.submit_interaction_response(resp, window, cx);
                    });
                },
            )
        };

        let on_delegate = {
            let entity = entity.clone();
            std::rc::Rc::new(move |window: &mut Window, app: &mut App| {
                entity.update(app, |view, cx| {
                    view.delegate_interaction_to_terminal(window, cx);
                });
            })
        };

        let presentation =
            crate::agent_ui::interaction_view::InteractionPresentation::Interactive {
                selected_choices: selected,
                custom_input,
                in_flight,
                on_select_choice: on_select,
                on_toggle_choice: on_toggle,
                on_custom_input_change: on_custom,
                on_submit,
                on_delegate_terminal: on_delegate,
            };

        crate::agent_ui::interaction_view::interaction_card(interaction, presentation, theme)
    }

    pub(crate) fn submit_interaction_response(
        &mut self,
        response: shardlane_host::conversation_interactions::InteractionResponse,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(interaction) = self.chat.active_interactions.first().cloned() else {
            return;
        };
        let Some(binding) = self.chat.model.binding.as_ref() else {
            return;
        };
        let Some(client) = self.client.clone() else {
            return;
        };

        let conversation_id = shardlane_host::ConversationId::new(binding.conversation_id.clone());
        let target_interaction_id = interaction.id.clone();
        let lookup_id = interaction.id.clone();
        let revision = interaction.revision;
        let window_handle = window.window_handle();

        self.chat.interaction_in_flight = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let client = match client.as_herdr() {
                        Some(herdr) => herdr.clone(),
                        None => {
                            return Err(shardlane_host::ConversationServiceError::Herdr(
                                shardlane_host::mux::MuxError::Unsupported("agents").into(),
                            ))
                        }
                    };
                    let service = shardlane_host::HostConversationService::new(
                        &client,
                        std::path::PathBuf::new(),
                    );
                    service.resolve_conversation_interaction(
                        &conversation_id,
                        &target_interaction_id,
                        revision,
                        response,
                    )
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                view.chat.interaction_in_flight = false;
                match outcome {
                    Ok(resolution) => {
                        view.chat.selected_interaction_choices.clear();
                        view.chat.custom_interaction_input = None;
                        if let Some(pos) = view
                            .chat
                            .active_interactions
                            .iter()
                            .position(|i| i.id == lookup_id)
                        {
                            view.chat.active_interactions[pos].state = resolution.state;
                        }
                    }
                    Err(err) => {
                        let _ = window_handle.update(cx, |_, window, cx| {
                            window
                                .push_notification(format!("Failed to submit response: {err}"), cx);
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn delegate_interaction_to_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(interaction) = self.chat.active_interactions.first().cloned() else {
            self.chat.model.mode = WorkSurfaceMode::Terminal;
            self.clear_ime_state();
            self.sync_terminal_application_focus(cx);
            cx.notify();
            return;
        };
        let Some(binding) = self.chat.model.binding.as_ref() else {
            self.chat.model.mode = WorkSurfaceMode::Terminal;
            self.clear_ime_state();
            self.sync_terminal_application_focus(cx);
            cx.notify();
            return;
        };
        let Some(client) = self.client.clone() else {
            self.chat.model.mode = WorkSurfaceMode::Terminal;
            self.clear_ime_state();
            self.sync_terminal_application_focus(cx);
            cx.notify();
            return;
        };

        let conversation_id = shardlane_host::ConversationId::new(binding.conversation_id.clone());
        let interaction_id = interaction.id;
        let revision = interaction.revision;
        let window_handle = window.window_handle();

        cx.spawn(async move |this, cx| {
            let _ = cx
                .background_executor()
                .spawn(async move {
                    let client = match client.as_herdr() {
                        Some(herdr) => herdr.clone(),
                        None => {
                            return Err(shardlane_host::ConversationServiceError::Herdr(
                                shardlane_host::mux::MuxError::Unsupported("agents").into(),
                            ))
                        }
                    };
                    let service = shardlane_host::HostConversationService::new(
                        &client,
                        std::path::PathBuf::new(),
                    );
                    service.delegate_interaction_to_terminal(
                        &conversation_id,
                        &interaction_id,
                        revision,
                    )
                })
                .await;

            let _ = this.update(cx, |view, cx| {
                view.chat.model.mode = WorkSurfaceMode::Terminal;
                view.clear_ime_state();
                view.sync_terminal_application_focus(cx);
                let _ = window_handle.update(cx, |_, window, cx| {
                    window.push_notification("Switched to Terminal to continue interaction", cx);
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn ensure_chat_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.chat.prompt.is_some() {
            return;
        }
        // M4: in the Working state the composer semantics are the queued
        // `Send after turn`.
        let working = self
            .chat
            .model
            .herdr_status
            .as_deref()
            .map(|status| matches!(status, "working" | "launch_pending"))
            .unwrap_or(false);
        let placeholder = if working {
            "Add a follow-up or correction…"
        } else {
            "Reply to the agent…"
        };
        let prompt = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder(SharedString::from(placeholder), window, cx);
            state
        });
        let subscription = cx.subscribe_in(
            &prompt,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { secondary: false } = event {
                    let has_text = this
                        .chat
                        .prompt
                        .as_ref()
                        .is_some_and(|prompt| !prompt.read(cx).value().trim().is_empty());
                    if has_text && !this.chat.model.submitting {
                        this.submit_chat_prompt(window, cx);
                    }
                }
            },
        );
        // Focus tracking: while the chat composer holds focus, keystrokes belong
        // to the input box — same guard semantics as steering input (otherwise
        // observe_keystrokes would encode characters into the hidden PTY).
        // Both the focus and blur subscriptions must be held (A06: the previous
        // get_or_insert dropped the blur subscription, so prompt_focused could
        // go stale).
        let focus_handle = prompt.read(cx).focus_handle(cx);
        let focus_sub = cx.on_focus(&focus_handle, window, |this, _window, cx| {
            this.chat.prompt_focused = true;
            this.sync_terminal_application_focus(cx);
            cx.notify();
        });
        let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            this.chat.prompt_focused = false;
            this.sync_terminal_application_focus(cx);
            cx.notify();
        });
        // Focus on creation: the Input is only built on the Chat's first frame
        // render; the focus at toggle time would be a null reference.
        prompt.update(cx, |state, input_cx| state.focus(window, input_cx));
        self.chat.prompt = Some(prompt);
        self.chat.prompt_subscription = Some(subscription);
        self.chat.prompt_focus_subscriptions = vec![focus_sub, blur_sub];
    }

    /// Chat configuration of the shared AgentComposer (same component shell;
    /// Chat owns the prompt and submission semantics).
    fn chat_composer(
        &mut self,
        theme: &ContentSurfaceTheme,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(prompt) = self.chat.prompt.clone() else {
            return div().into_any_element();
        };
        let has_text = !prompt.read(cx).value().trim().is_empty();
        let status = self.chat.model.herdr_status.clone();
        let capability = chat_send_capability(status.as_deref(), has_text);
        let queued = self.chat.model.queued_follow_up.is_some();
        let send_state = match capability {
            _ if self.chat.model.submitting => ComposerSendState::Busy,
            // M4: Working with text = `Send after turn` (enqueue, no instant
            // injection).
            ChatSendCapability::Ready | ChatSendCapability::Working if !queued => {
                ComposerSendState::Ready
            }
            _ => ComposerSendState::Disabled,
        };
        let submit_herdr = cx.entity();
        let mut composer = AgentComposer::new("chat-composer", prompt)
            .send_state(send_state)
            .on_send(move |window, app| {
                submit_herdr.update(app, |this, cx| {
                    this.submit_chat_prompt(window, cx);
                });
            });
        if let Some(error) = self.chat.model.last_error.clone() {
            composer = composer.footer(vec![h_flex()
                .w_full()
                .min_w_0()
                .gap(SPACE_ICON)
                .text_size(crate::theme::FONT_META)
                .text_color(theme.danger)
                .child(Icon::new(ComponentIconName::TriangleAlert).with_size(px(11.0)))
                .child(div().min_w_0().truncate().child(error))
                .into_any_element()]);
        } else {
            match capability {
                ChatSendCapability::Blocked => {
                    let switch_herdr = cx.entity();
                    composer = composer.footer(vec![h_flex()
                        .w_full()
                        .min_w_0()
                        .gap(px(8.0))
                        .items_center()
                        .text_size(crate::theme::FONT_META)
                        .text_color(theme.muted)
                        .child(div().child("Agent needs input in the terminal."))
                        .child(
                            Button::new("chat-open-terminal")
                                .ghost()
                                .xsmall()
                                .icon(ComponentIconName::SquareTerminal)
                                .label("Open Terminal")
                                .on_click(move |_, window, app| {
                                    switch_herdr.update(app, |this, cx| {
                                        this.chat.model.mode = WorkSurfaceMode::Terminal;
                                        view_sync_terminal_after_switch(this, cx);
                                        cx.notify();
                                    });
                                    let _ = window;
                                }),
                        )
                        .into_any_element()]);
                }
                ChatSendCapability::Working => {
                    let mut footer = vec![h_flex()
                        .w_full()
                        .min_w_0()
                        .gap(SPACE_ICON)
                        .text_size(crate::theme::FONT_META)
                        .text_color(theme.muted)
                        .child(
                            div().child("Agent is working — your follow-up sends after this turn."),
                        )
                        .into_any_element()];
                    if let Some(queued) = self.chat.model.queued_follow_up.clone() {
                        footer.push(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap(SPACE_ICON)
                                .items_center()
                                .text_size(crate::theme::FONT_META)
                                .text_color(theme.muted)
                                .child(
                                    div().child(format!("1 follow-up {} ·", queued.phase_label())),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .truncate()
                                        .child(queued.text.clone()),
                                )
                                .into_any_element(),
                        );
                        footer.push(self.queued_follow_up_actions(theme, cx));
                    }
                    composer = composer.footer(footer);
                }
                _ => {}
            }
        }
        composer.into_any_element()
    }

    /// Assistant answer row: the incremental Markdown engine (shares the cache
    /// and render entry with History). `mend` is enabled only for the tail
    /// answer currently streaming (A11: settled content parses with
    /// final-draft semantics).
    fn chat_answer_row(
        &mut self,
        message: &shardlane_history::TranscriptMessage,
        palette: &Palette,
        metrics: Metrics,
        _theme: &ContentSurfaceTheme,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let seq = message.seq;
        let streaming = self
            .chat
            .model
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.messages.last())
            .is_some_and(|last| last.seq == seq)
            && matches!(
                self.chat.model.herdr_status.as_deref(),
                Some("working") | Some("launch_pending")
            );
        let row_key: std::rc::Rc<str> = std::rc::Rc::from(format!("chat-{seq}").as_str());
        // ⌘F highlight (notate 08-29 round five): find hits are highlighted
        // within that message's rendering.
        let search = self.chat_find_highlights_for(seq);
        let body = self
            .chat
            .viewport
            .markdown
            .render_answer(
                seq,
                &message.text,
                streaming,
                CHAT_MD_BUDGET_BYTES,
                streaming.then_some(seq),
                row_key,
                palette,
                metrics,
                &self.chat.viewport.selection,
                search,
            )
            .unwrap_or_else(|| div().into_any_element());
        div()
            .w_full()
            .min_w_0()
            .px(px(16.0))
            .py(px(4.0))
            .child(body)
            .into_any_element()
    }

    fn chat_tool_row(
        &mut self,
        message_index: usize,
        tool_index: usize,
        tool_call: &shardlane_history::ToolCall,
        in_group: bool,
        theme: &ContentSurfaceTheme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let seq = self
            .chat
            .model
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.messages.get(message_index))
            .map(|message| message.seq)
            .unwrap_or(0);
        let tool_key = format!("{seq}-{tool_index}");
        let expanded = self.chat.viewport.expanded_tools.contains(&tool_key);
        let tool_for_row = tool_call.clone();
        let detail = expanded.then(|| activity::render_tool_detail(tool_call, theme));
        let toggle_herdr = cx.entity();
        let key_for_toggle = tool_key.clone();
        let mut cluster = v_flex()
            .w_full()
            .min_w_0()
            .when(in_group, |cluster| cluster.pl(px(14.0)))
            .gap(px(4.0));
        cluster = cluster.child(activity::render_activity_row(
            SharedString::from(format!("chat-activity-{tool_key}")),
            &tool_for_row,
            expanded,
            move |_, app_cx| {
                toggle_herdr.update(app_cx, |this, cx| {
                    if !this.chat.viewport.expanded_tools.remove(&key_for_toggle) {
                        this.chat
                            .viewport
                            .expanded_tools
                            .insert(key_for_toggle.clone());
                    }
                    cx.notify();
                });
            },
            theme,
        ));
        if let Some(detail) = detail {
            cluster = cluster.child(div().w_full().min_w_0().pl(px(16.0)).child(detail));
        }
        div()
            .w_full()
            .min_w_0()
            .px(px(16.0))
            .py(px(1.0))
            .child(cluster)
            .into_any_element()
    }

    fn chat_turn_footer_row(&self, turn_index: usize, theme: &ContentSurfaceTheme) -> AnyElement {
        let Some(turn) = self.chat.model.turns().get(turn_index) else {
            return div().into_any_element();
        };
        let Some(snapshot) = self.chat.model.snapshot.as_ref() else {
            return div().into_any_element();
        };
        let copy_text = activity::turn_answer_text(&snapshot.messages, turn.range.clone());
        if copy_text.is_empty() {
            return div().into_any_element();
        }
        let time_label = snapshot.messages[turn.range.end.saturating_sub(1)]
            .timestamp
            .map(history_msg_time_label);
        div()
            .w_full()
            .min_w_0()
            .px(px(16.0))
            .py(px(2.0))
            .child(activity::render_turn_footer(
                SharedString::from(format!("chat-turn-footer-{turn_index}")),
                time_label,
                copy_text,
                theme,
            ))
            .into_any_element()
    }
}

fn view_sync_terminal_after_switch(this: &mut ShardlaneApp, cx: &mut Context<ShardlaneApp>) {
    this.clear_ime_state();
    this.sync_terminal_application_focus(cx);
}

/// Connecting view (rendered when a binding exists but the source is not ready
/// yet): distinguishes "exact catalog lookup in progress" from a real failure
/// (retryable).
fn chat_connecting_view(
    app: &ShardlaneApp,
    retry_entity: Entity<ShardlaneApp>,
    theme: ContentSurfaceTheme,
) -> AnyElement {
    let error = app.chat.model.live_error.clone();
    let retriable = error
        .as_deref()
        .is_some_and(|text| !text.contains("Connecting conversation"));
    let mut column = div()
        .w_full()
        .pt(px(48.0))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(10.0))
        .child(
            div()
                .text_size(crate::theme::FONT_BODY)
                .text_color(if retriable { theme.danger } else { theme.muted })
                .child(error.unwrap_or_else(|| "Connecting conversation…".to_string())),
        );
    if retriable {
        column = column.child(
            Button::new("chat-retry-binding")
                .ghost()
                .xsmall()
                .label("Retry")
                .on_click(move |_, _, app| {
                    retry_entity.update(app, |this, cx| {
                        this.chat.model.load_generation += 1;
                        this.chat.model.live_error = None;
                        this.chat.lookup_in_flight = false;
                        this.ensure_chat_source(cx);
                        cx.notify();
                    });
                }),
        );
    }
    column.into_any_element()
}

/// Chat user prompt row: shared user_prompt_card + plain-text body (outer
/// padding owned by Chat).
fn chat_user_prompt_row(
    message: &shardlane_history::TranscriptMessage,
    theme: &ContentSurfaceTheme,
) -> AnyElement {
    let time_label = message.timestamp.map(history_msg_time_label);
    let body = div()
        .w_full()
        .min_w_0()
        .whitespace_normal()
        .text_size(crate::theme::FONT_SECTION_TITLE)
        .text_color(theme.foreground)
        .child(message.text.clone())
        .into_any_element();
    div()
        .w_full()
        .min_w_0()
        .px(px(16.0))
        .pt(px(18.0))
        .pb(px(6.0))
        .child(conversation_view::user_prompt_card(
            time_label, false, body, theme,
        ))
        .into_any_element()
}

/// Local pending submission row (A15): a low-contrast pending variant of the
/// shared user_prompt_card; removed by reconciliation as soon as the echo
/// arrives.
fn chat_pending_prompt_row(text: &str, theme: &ContentSurfaceTheme) -> AnyElement {
    let body = div()
        .w_full()
        .min_w_0()
        .whitespace_normal()
        .text_size(crate::theme::FONT_SECTION_TITLE)
        .text_color(theme.foreground.opacity(0.75))
        .child(text.to_string())
        .into_any_element();
    div()
        .w_full()
        .min_w_0()
        .px(px(16.0))
        .pt(px(18.0))
        .pb(px(6.0))
        .child(conversation_view::user_prompt_card(None, true, body, theme))
        .into_any_element()
}

fn history_msg_time_label(timestamp: i64) -> String {
    let secs = if timestamp > 1_000_000_000_000 {
        timestamp / 1000
    } else {
        timestamp
    };
    let secs_in_day = secs.rem_euclid(86_400);
    format!("{:02}:{:02}", secs_in_day / 3600, (secs_in_day % 3600) / 60)
}
