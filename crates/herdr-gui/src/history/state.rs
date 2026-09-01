//! [INPUT]: Existing imports and types from the crate root (via the history module root glob: `use super::*` chain).
//! [OUTPUT]: For the crate::history family: History-surface UI state and data projections — HistoryUiState / expanded content / sort-key types + ShardlaneApp state methods for watcher, bootstrap, filtering, sorting, summaries merging, etc.
//! [POS]: State responsibility slice of the herdr-gui History surface; mechanically split out of history.rs.
use super::*;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum HistoryExpandedContent {
    Thinking(i64),
    Tools(i64),
    /// Expanded state of the Worked fold: value is the seq of the turn's first
    /// message (a stable key across pages).
    Turn(i64),
}

/// Conversation list sort keys (same three keys + direction as before).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum HistorySortKey {
    #[default]
    Updated,
    Created,
    Messages,
}

/// Time filters (notate 08-29 round five: sidebar filter menu): rolling-window semantics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum HistoryTimeFilter {
    #[default]
    Any,
    Last24Hours,
    Last7Days,
    Last30Days,
}

impl HistoryTimeFilter {
    /// Cutoff (epoch seconds); Any → None. meta.updated_at mixes milliseconds
    /// and seconds, so compare after normalizing with history_epoch_secs.
    pub(super) fn cutoff_secs(self) -> Option<i64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let days = match self {
            Self::Any => return None,
            Self::Last24Hours => 1,
            Self::Last7Days => 7,
            Self::Last30Days => 30,
        };
        Some(now.saturating_sub(days * 86_400))
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Any => "Any time",
            Self::Last24Hours => "Last 24 hours",
            Self::Last7Days => "Last 7 days",
            Self::Last30Days => "Last 30 days",
        }
    }

    pub(super) const TIME_FILTERS: [Self; 4] = [
        Self::Any,
        Self::Last24Hours,
        Self::Last7Days,
        Self::Last30Days,
    ];
}

#[derive(Default)]
pub(crate) struct HistoryUiState {
    pub(crate) open: bool,
    pub(crate) detail_only: bool,
    pub(crate) loading: bool,
    pub(crate) sessions: Vec<ConversationMeta>,
    /// List-row Description previews (key → one-line truncation of the first
    /// user message), refreshed with the list.
    pub(crate) descriptions: HashMap<String, String>,
    pub(crate) total_sessions: Option<usize>,
    pub(crate) selected_key: Option<String>,
    pub(crate) transcript: Option<CachedTranscriptWindow>,
    pub(crate) error: Option<String>,
    pub(crate) generation: u64,
    pub(crate) transcript_generation: u64,
    pub(super) transcript_loading_key: Option<String>,
    pub(super) transcript_job: Option<BackgroundJob<()>>,
    pub(super) expanded_content: HashSet<HistoryExpandedContent>,
    pub(crate) continue_agent: Option<AgentId>,
    pub(crate) search_open: bool,
    pub(crate) resume_in_flight: bool,
    /// Stable logical Continue operation kept with the History composer.  A
    /// retry after an ambiguous response reuses this id only for the same
    /// Conversation/instruction fingerprint; switching sessions starts fresh.
    pub(crate) pending_continue_operation_id: Option<String>,
    pub(crate) pending_continue_fingerprint: Option<String>,
    /// History Composer input (same AgentComposer shell as Live Chat; R3).
    pub(crate) prompt: Option<gpui::Entity<InputState>>,
    pub(crate) prompt_focused: bool,
    pub(crate) prompt_subscription: Option<gpui::Subscription>,
    pub(crate) prompt_focus_subscriptions: Vec<gpui::Subscription>,
    /// Continue+Prompt transaction in flight (the draft stays in InputState and
    /// is only cleared on success).
    /// Draft recovered when the transaction fails (restored when reopening
    /// History or retrying).
    pub(crate) composer_draft_saved: Option<String>,
    /// M6: set after the planner returns NeedsProjectSelection; once the user
    /// resolves it via Project selection, `pending_project` is backfilled and the
    /// flow retried (no runtime changes happen before that).
    pub(crate) pending_project_selection: bool,
    pub(crate) pending_project: Option<String>,
    /// Adapter roster matching the configuration snapshot; scanner/watcher/detail/
    /// export all consume only this generation, preventing the same provider's
    /// default roots and custom roots from diverging.
    pub(crate) roster: Arc<shardlane_history::HistoryAdapterRoster>,
    pub(super) filter_agent: Option<AgentId>,
    pub(super) filter_project: Option<String>,
    pub(super) filter_time: HistoryTimeFilter,
    pub(super) sort_key: HistorySortKey,
    pub(super) sort_ascending: bool,
    pub(super) target_seq: Option<i64>,
    /// Shared ConversationSurface viewport (virtual list + row signatures +
    /// Markdown cache + selection + overlay scrollbar + scroll intents).
    pub(super) viewport: crate::agent_ui::conversation_surface::ConversationViewportState,
    /// Find within conversation (⌘F, notate 08-29 round five).
    pub(super) find: Option<crate::agent_ui::find::ConversationFind>,
    pub(super) find_focused: bool,
    pub(super) watcher: Option<HistoryWatcher>,
    pub(super) watch_loop_started: bool,
    /// Insights tab active flag.
    pub(super) insights_tab_open: bool,
    /// Last computed InsightsSnapshot (loaded in background).
    pub(crate) insights: Option<shardlane_history::InsightsSnapshot>,
    pub(super) insights_loading: bool,
}

pub(super) fn summary_metas(
    summaries: &[shardlane_history::SessionSummary],
) -> Vec<ConversationMeta> {
    summaries
        .iter()
        .map(|summary| summary.meta.clone())
        .collect()
}

fn toggled_history_surface_state(open: bool, detail_only: bool) -> (bool, bool) {
    if open && detail_only {
        (true, false)
    } else {
        (!open, false)
    }
}

impl ShardlaneApp {
    pub(super) fn resolved_history_session_project_path(
        &self,
        session: &ConversationMeta,
    ) -> String {
        history_session_project_path_display(
            &session.project_path,
            &session.project_name,
            &self.state,
            &self.scripts,
        )
    }

    /// History-session backfill for the Sidebar Agents section (notate
    /// 2026-08-29): when live agents are fewer than 10, backfill history
    /// sessions by most recently updated up to 10 (row 11 is a fixed "view more").
    /// Deduplication uses only the stable (provider, native session id) identity —
    /// history sessions already present as live agents are hidden from this
    /// projection; never guessed by title/path.
    pub(crate) fn sidebar_agent_history_sessions(
        &self,
        live_count: usize,
    ) -> Vec<ConversationMeta> {
        const AGENT_SECTION_ROW_CAPACITY: usize = 10;
        let fill = AGENT_SECTION_ROW_CAPACITY.saturating_sub(live_count);
        if fill == 0 {
            return Vec::new();
        }
        let live_identities: HashSet<(&str, &str)> = self
            .state
            .agents
            .iter()
            .filter_map(|agent| {
                let session = agent.agent_session.as_ref()?;
                // agent_session.agent is already the Herdr protocol short id ("claude" etc).
                Some((session.agent.as_str(), session.value.as_str()))
            })
            .collect();
        self.history
            .sessions
            .iter()
            .filter(|session| !session.archived)
            .filter(|session| {
                !live_identities.contains(&(
                    crate::agent_cli::herdr_agent_id(session.agent),
                    session.id.as_str(),
                ))
            })
            .take(fill)
            .cloned()
            .collect()
    }

    /// Sidebar "view more history sessions" entry: ensure the History secondary
    /// surface is open.
    pub(crate) fn open_history_surface_from_sidebar(&mut self, cx: &mut Context<Self>) {
        if self.history.open {
            return;
        }
        self.history.open = true;
        self.history.detail_only = false;
        self.history.search_open = false;
        self.new_agent_open = false;
        self.show_settings = false;
        self.show_help = false;
        self.clear_ime_state();
        self.ensure_history_watcher(cx);
        if !self.history.sessions.is_empty() {
            self.reconcile_history_selection(cx);
        }
        self.notify_sidebar(cx);
        self.sync_terminal_application_focus(cx);
        cx.notify();
    }

    /// Current selection text from drag-selection in the History body Markdown
    /// (the ⌘C copy exit, notate 08-29 round four).
    pub(crate) fn history_transcript_selection_text(&self) -> Option<String> {
        self.history
            .viewport
            .selection
            .selection
            .borrow()
            .selected_text()
    }

    pub(crate) fn bootstrap_sidebar_history(&mut self, cx: &mut Context<Self>) {
        self.ensure_history_watcher(cx);
        if self.history.sessions.is_empty() && !self.history.loading {
            self.refresh_history(false, cx);
        }
    }

    /// Replace the source snapshot after ApplicationConfig reload. The watcher
    /// is tied to the old roots, so drop it before starting the new generation.
    pub(crate) fn rebuild_history_roster(
        &mut self,
        policy: &shardlane_history::HistorySourcePolicy,
        cx: &mut Context<Self>,
    ) {
        self.history.roster = Arc::new(shardlane_history::HistoryAdapterRoster::new(policy));
        // Invalidate every background consumer that captured the previous
        // source snapshot before replacing the watcher and scheduling a scan.
        self.history.generation = self.history.generation.wrapping_add(1);
        self.history.transcript_generation = self.history.transcript_generation.wrapping_add(1);
        self.history.transcript_job.take();
        self.history.transcript_loading_key = None;
        self.history_source_probe = None;
        self.history_source_probe_requested.set(false);
        self.history.watcher.take();
        self.history.watch_loop_started = false;
        self.ensure_history_watcher(cx);
    }

    pub(super) fn ensure_history_watcher(&mut self, cx: &mut Context<Self>) {
        if self.history.watcher.is_none() {
            self.history.watcher = HistoryWatcher::start(&self.history.roster.active);
        }
        if self.history.watch_loop_started {
            return;
        }
        let Some(watcher) = self.history.watcher.as_ref() else {
            return;
        };
        let dirty = watcher.dirty_receiver();
        self.history.watch_loop_started = true;
        cx.spawn(async move |this, cx| {
            // PERF-01 remainder (audit 2026-08-27): while busy, we must not consume
            // the dirty signal, sleep, and return to recv() — that just means
            // "discard this change, wait for the next file event". Instead use
            // pending_dirty: keep the pending refresh intent while loading; after
            // backing off, run one more refresh; the first backoff cycle after
            // loading finishes is guaranteed to converge.
            let mut pending_dirty = false;
            loop {
                if !pending_dirty && dirty.recv().await.is_err() {
                    break;
                }
                let refreshed = this
                    .update(cx, |view, cx| {
                        if view.history.loading {
                            return false;
                        }
                        view.refresh_history(false, cx);
                        true
                    })
                    .unwrap_or(true);
                if refreshed {
                    pending_dirty = false;
                } else {
                    pending_dirty = true;
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                }
            }
        })
        .detach();
    }

    pub(crate) fn toggle_history(
        &mut self,
        _: &OpenHistory,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (open, detail_only) =
            toggled_history_surface_state(self.history.open, self.history.detail_only);
        self.history.open = open;
        self.history.detail_only = detail_only;
        self.history.search_open = false;
        self.new_agent_open = false;
        self.show_settings = false;
        self.show_help = false;
        self.clear_ime_state();
        self.sync_terminal_application_focus(cx);
        self.notify_sidebar(cx);
        // SBX-02: secondary-surface mutual exclusion — while History is open,
        // Settings steps aside.
        if self.history.open {
            self.show_settings = false;
        }
        cx.notify();
        if self.history.open {
            self.ensure_history_watcher(cx);
            if !self.history.sessions.is_empty() {
                self.reconcile_history_selection(cx);
            }
        } else {
            self.cancel_history_transcript_load();
        }
        if self.history.open && self.history.sessions.is_empty() && !self.history.loading {
            self.refresh_history(false, cx);
        }
    }

    pub(crate) fn leave_history_surface(&mut self, cx: &mut Context<Self>) {
        if !self.history.open && !self.history.search_open {
            return;
        }
        self.history.open = false;
        self.history.search_open = false;
        self.cancel_history_transcript_load();
        self.sync_terminal_application_focus(cx);
        self.notify_sidebar(cx);
    }

    pub(super) fn cancel_history_transcript_load(&mut self) {
        let was_loading = self.history.transcript_loading_key.take().is_some();
        self.history.transcript_job.take();
        if was_loading {
            self.history.transcript_generation = self.history.transcript_generation.wrapping_add(1);
        }
    }

    pub(super) fn history_session_matches_filters(&self, session: &ConversationMeta) -> bool {
        history_session_matches_filters(
            session,
            self.history.filter_agent,
            self.history.filter_project.as_deref(),
            self.history.filter_time,
        )
    }

    /// Find-input focus flag (consumed by the shell_input guard).
    pub(crate) fn history_find_focused(&self) -> bool {
        self.history.find_focused
    }

    /// Find-input focus write (shell_input on_focus/on_blur callback).
    pub(crate) fn history_set_find_focused(&mut self, focused: bool) {
        self.history.find_focused = focused;
    }

    /// Time entry of the sidebar filter menu (notate 08-29 round five).
    pub(super) fn set_history_time_filter(
        &mut self,
        filter: HistoryTimeFilter,
        cx: &mut Context<Self>,
    ) {
        if self.history.filter_time != filter {
            self.history.filter_time = filter;
            self.reconcile_history_selection(cx);
        }
    }

    /// Whether any filter is active (drives the menu's "Clear filters" visibility).
    pub(super) fn history_filters_active(&self) -> bool {
        self.history.filter_agent.is_some()
            || self.history.filter_project.is_some()
            || self.history.filter_time != HistoryTimeFilter::Any
    }

    pub(super) fn first_filtered_history_session(&self) -> Option<ConversationMeta> {
        self.history
            .sessions
            .iter()
            .find(|session| self.history_session_matches_filters(session))
            .cloned()
    }

    pub(super) fn reconcile_history_selection(&mut self, cx: &mut Context<Self>) {
        let selected = self
            .history
            .selected_key
            .as_deref()
            .and_then(|key| {
                self.history.sessions.iter().find(|session| {
                    session.key == key && self.history_session_matches_filters(session)
                })
            })
            .cloned()
            .or_else(|| self.first_filtered_history_session());
        if let Some(session) = selected {
            self.select_history_session(session, cx);
        } else {
            self.history.selected_key = None;
            self.history.transcript = None;
            self.cancel_history_transcript_load();
            cx.notify();
        }
    }

    /// Re-sort the current session list in place (called when the sort key or
    /// direction changes, or when a new list arrives).
    pub(super) fn sort_history_sessions_in_place(&mut self) {
        let key = self.history.sort_key;
        let ascending = self.history.sort_ascending;
        self.history.sessions.sort_by(|a, b| {
            let order = match key {
                HistorySortKey::Updated => b.updated_at.cmp(&a.updated_at),
                HistorySortKey::Created => b.created_at.cmp(&a.created_at),
                HistorySortKey::Messages => b.message_count.cmp(&a.message_count),
            };
            if ascending { order.reverse() } else { order }.then_with(|| a.key.cmp(&b.key))
        });
    }

    /// A new list arrived: replace sessions + descriptions wholesale and re-sort.
    pub(super) fn apply_history_summaries(
        &mut self,
        summaries: Vec<shardlane_history::SessionSummary>,
    ) {
        self.history.descriptions = summaries
            .iter()
            .map(|summary| (summary.meta.key.clone(), summary.description.clone()))
            .collect();
        self.history.sessions = summary_metas(&summaries);
        self.sort_history_sessions_in_place();
    }

    /// When the row set is unchanged, merge only Descriptions (rows from the old
    /// catalog get filled in gradually by backfill).
    pub(super) fn merge_history_descriptions(
        &mut self,
        summaries: Vec<shardlane_history::SessionSummary>,
    ) {
        for summary in summaries {
            if !summary.description.is_empty() {
                self.history
                    .descriptions
                    .insert(summary.meta.key, summary.description);
            }
        }
    }

    pub(super) fn set_history_agent_filter(
        &mut self,
        agent: Option<AgentId>,
        cx: &mut Context<Self>,
    ) {
        self.history.filter_agent = agent;
        self.reconcile_history_selection(cx);
    }

    pub(super) fn set_history_project_filter(
        &mut self,
        project: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.history.filter_project = project;
        self.reconcile_history_selection(cx);
    }

    pub(super) fn clear_history_filters(&mut self, cx: &mut Context<Self>) {
        self.history.filter_agent = None;
        self.history.filter_project = None;
        self.history.filter_time = HistoryTimeFilter::Any;
        self.reconcile_history_selection(cx);
    }

    /// Trigger a background computation of InsightsSnapshot.
    /// No-op if already loading.
    pub(crate) fn load_history_insights(&mut self, cx: &mut Context<Self>) {
        if self.history.insights_loading {
            return;
        }
        self.history.insights_loading = true;
        cx.notify();
        let db_path = history_db_path();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { shardlane_history::compute_insights(&db_path) })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.history.insights_loading = false;
                match result {
                    Ok(snap) => {
                        view.history.insights = Some(snap);
                        cx.notify();
                    }
                    Err(e) => {
                        view.history.insights = None;
                        eprintln!("insights compute failed: {e}");
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::toggled_history_surface_state;

    #[test]
    fn toggle_from_detail_only_keeps_history_open_and_exits_detail_mode() {
        assert_eq!(toggled_history_surface_state(true, true), (true, false));
    }

    #[test]
    fn toggle_from_closed_opens_history_and_toggle_from_list_closes_it() {
        assert_eq!(toggled_history_surface_state(false, false), (true, false));
        assert_eq!(toggled_history_surface_state(true, false), (false, false));
    }
}
