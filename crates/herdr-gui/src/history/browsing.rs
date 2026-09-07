//! [INPUT]: Existing imports and types from the crate root (via the history module root glob: `use super::*` chain).
//! [OUTPUT]: For the crate::history family: the conversation browsing flow — refresh / selection / search-hit navigation / scroll targeting / lazy transcript paging / infinite scroll / content expand-collapse.
//! [POS]: Browsing responsibility slice of the herdr-gui History surface; mechanically split out of history.rs.
use super::*;

pub(super) const HISTORY_PAGE_LIMIT: usize = 500;

impl ShardlaneApp {
    pub(crate) fn refresh_history(&mut self, full: bool, cx: &mut Context<Self>) {
        self.history.generation = self.history.generation.wrapping_add(1);
        let generation = self.history.generation;
        let load_cached_first = self.history.sessions.is_empty();
        self.history.loading = true;
        if load_cached_first {
            self.history.total_sessions = None;
        }
        self.history.error = None;
        cx.notify();
        let db_path = history_db_path();
        let roster = self.history.roster.clone();

        cx.spawn(async move |this, cx| {
            if load_cached_first {
                let cached_db_path = db_path.clone();
                let cached = cx
                    .background_executor()
                    .spawn(async move {
                        let catalog = HistoryCatalog::open(&cached_db_path)
                            .map_err(|error| error.to_string())?;
                        catalog
                            .list_session_summaries(HISTORY_PAGE_LIMIT)
                            .map_err(|error| error.to_string())
                    })
                    .await;
                let still_current = this
                    .update(cx, |view, cx| {
                        if view.history.generation != generation {
                            return false;
                        }
                        match cached {
                            Ok(summaries) if !summaries.is_empty() => {
                                view.apply_history_summaries(summaries);
                                view.notify_sidebar(cx);
                                if view.history.open {
                                    view.reconcile_history_selection(cx);
                                }
                                cx.notify();
                            }
                            Ok(_) => {}
                            Err(error) => view.history.error = Some(error),
                        }
                        true
                    })
                    .unwrap_or(false);
                if !still_current {
                    return;
                }
            }

            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut catalog =
                        HistoryCatalog::open(&db_path).map_err(|error| error.to_string())?;
                    let scan_started = Instant::now();
                    let report =
                        scan(&roster.active, &mut catalog, full)
                            .map_err(|error| error.to_string())?;
                    let scan_elapsed = scan_started.elapsed();
                    if report.parsed > 0 || scan_elapsed >= Duration::from_millis(100) {
                        lag_log(format_args!(
                            "history.scan full={full} discovered={} parsed={} prewarmed={} cache_evicted={} unchanged={} removed={} errors={} {:.1}ms",
                            report.discovered,
                            report.parsed,
                            report.prewarmed,
                            report.cache_evicted,
                            report.unchanged,
                            report.removed,
                            report.errors.len(),
                            scan_elapsed.as_secs_f64() * 1_000.0
                        ));
                    }
                    let summaries = catalog
                        .list_session_summaries(HISTORY_PAGE_LIMIT)
                        .map_err(|error| error.to_string())?;
                    let total = catalog
                        .count_sessions()
                        .map_err(|error| error.to_string())?;
                    Ok::<_, String>((report, summaries, total))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.history.generation != generation {
                    return;
                }
                view.history.loading = false;
                match result {
                    Ok((report, summaries, total)) => {
                        let sessions_changed =
                            view.history.sessions != summary_metas(&summaries);
                        let total_changed = view.history.total_sessions != Some(total);
                        view.history.error =
                            (!report.errors.is_empty()).then(|| report.errors.join("\n"));
                        if sessions_changed {
                            view.apply_history_summaries(summaries);
                        } else {
                            // Even with unchanged rows, absorb freshly backfilled
                            // descriptions (incremental backfill).
                            view.merge_history_descriptions(summaries);
                        }
                        view.history.total_sessions = Some(total);
                        if sessions_changed || total_changed {
                            view.notify_sidebar(cx);
                        }
                        if view.history.open && sessions_changed {
                            view.reconcile_history_selection(cx);
                        }
                    }
                    Err(error) => view.history.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn open_history_session_from_search(
        &mut self,
        session: ConversationMeta,
        target_seq: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        self.history.open = true;
        self.history.detail_only = true;
        self.history.search_open = false;
        self.new_agent_open = false;
        self.show_settings = false;
        self.show_help = false;
        if !self
            .history
            .sessions
            .iter()
            .any(|candidate| candidate.key == session.key)
        {
            self.history.sessions.push(session.clone());
        }
        self.ensure_history_watcher(cx);
        if let Some(seq) = target_seq {
            self.select_history_search_hit(session, seq, cx);
        } else {
            self.select_history_session(session, cx);
        }
        self.notify_sidebar(cx);
        self.sync_terminal_application_focus(cx);
    }

    pub(crate) fn open_project_history(&mut self, project_path: String, cx: &mut Context<Self>) {
        self.history.open = true;
        self.history.detail_only = false;
        self.history.search_open = false;
        self.new_agent_open = false;
        self.show_settings = false;
        self.show_help = false;
        self.history.filter_project = Some(project_path);
        self.reconcile_history_selection(cx);
        cx.notify();
    }

    pub(super) fn select_history_session(
        &mut self,
        session: ConversationMeta,
        cx: &mut Context<Self>,
    ) {
        self.select_history_session_at(session, None, cx);
    }

    pub(super) fn select_history_search_hit(
        &mut self,
        session: ConversationMeta,
        seq: i64,
        cx: &mut Context<Self>,
    ) {
        self.select_history_session_at(session, Some(seq), cx);
    }

    pub(super) fn select_history_session_at(
        &mut self,
        session: ConversationMeta,
        target_seq: Option<i64>,
        cx: &mut Context<Self>,
    ) {
        self.history.target_seq = target_seq;
        if self.history.selected_key.as_deref() == Some(session.key.as_str()) {
            if self.history.transcript.as_ref().is_some_and(|transcript| {
                target_seq.is_none()
                    || target_seq.is_some_and(|seq| {
                        transcript.messages.iter().any(|message| message.seq == seq)
                    })
            }) {
                self.scroll_history_to_target();
                cx.notify();
                return;
            }
            if self.history.transcript_loading_key.as_deref() == Some(session.key.as_str()) {
                return;
            }
        }
        self.cancel_history_transcript_load();
        self.history.find = None;
        self.history.selected_key = Some(session.key.clone());
        self.history.continue_agent = Some(session.agent);
        self.history.transcript = None;
        self.history.transcript_loading_key = Some(session.key.clone());
        self.history.expanded_content.clear();
        self.history.viewport.markdown.clear();
        self.history.transcript_generation = self.history.transcript_generation.wrapping_add(1);
        let generation = self.history.transcript_generation;
        let session_key = session.key.clone();
        let session_size = session.size_bytes;
        let db_path = history_db_path();
        let roster = self.history.roster.clone();
        cx.notify();

        let script = cx.spawn(async move |this, cx| {
            let started = Instant::now();
            let result = cx
                .background_executor()
                .spawn(async move {
                    load_full_transcript(&db_path, &session, roster.as_ref()).map(|parsed| {
                        let total = parsed.mainline.len();
                        CachedTranscriptWindow {
                            meta: parsed.meta,
                            total_messages: total,
                            start: 0,
                            messages: parsed.mainline,
                            sidechains: parsed.sidechains,
                            unknown_line_count: parsed.unknown_line_count,
                        }
                    })
                })
                .await;
            let elapsed = started.elapsed();
            if elapsed >= Duration::from_millis(30) {
                lag_log(format_args!(
                    "history.transcript.load key={session_key} source_bytes={session_size} {:.1}ms",
                    elapsed.as_secs_f64() * 1000.0
                ));
            }
            let _ = this.update(cx, |view, cx| {
                if view.history.transcript_generation != generation {
                    return;
                }
                view.history.transcript_loading_key = None;
                match result {
                    Ok(transcript) => {
                        view.history.error = None;
                        view.history.transcript = Some(transcript);
                        if target_seq.is_none() {
                            // No jump target = open the newest page (Bottom anchoring =
                            // pinned to the bottom on the first frame).
                            view.history.viewport.reset();
                        }
                        view.scroll_history_to_target();
                    }
                    Err(error) => view.history.error = Some(error.to_string()),
                }
                cx.notify();
            });
        });
        self.history.transcript_job = Some(script);
    }

    pub(super) fn scroll_history_to_target(&mut self) {
        let Some(seq) = self.history.target_seq else {
            return;
        };
        let Some(transcript) = self.history.transcript.as_ref() else {
            return;
        };
        if let Some(index) = transcript
            .messages
            .iter()
            .position(|message| message.seq == seq)
        {
            // +1: row 0 of the shared surface shares the same counting basis with
            // the list header before frame_reset (no extra rows), so indices align
            // directly with message indexes.
            self.history.viewport.list.scroll_to_reveal_item(index);
        }
    }

    pub(super) fn toggle_history_content(
        &mut self,
        content: HistoryExpandedContent,
        cx: &mut Context<Self>,
    ) {
        if !self.history.expanded_content.insert(content) {
            self.history.expanded_content.remove(&content);
        }
        cx.notify();
    }

    // ── Find within conversation (⌘F, notate 08-29 round five) ──

    pub(crate) fn toggle_history_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.find.is_some() {
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
            |this, _window, cx| this.history_find_rerun(cx),
        );
        input.update(cx, |state, cx| state.focus(window, cx));
        self.history.find = Some(crate::agent_ui::find::ConversationFind {
            input,
            subscriptions,
            hits: Vec::new(),
            active: 0,
        });
        cx.notify();
    }

    fn history_find_rerun(&mut self, cx: &mut Context<Self>) {
        let query = self
            .history
            .find
            .as_ref()
            .map(|find| find.input.read(cx))
            .map(|state| state.value().to_string())
            .unwrap_or_default();
        let messages: Vec<(i64, &str)> = self
            .history
            .transcript
            .as_ref()
            .map(|transcript| {
                transcript
                    .messages
                    .iter()
                    .map(|message| (message.seq, message.text.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        let hits = crate::agent_ui::find::collect_find_hits(&messages, &query);
        if let Some(find) = self.history.find.as_mut() {
            find.hits = hits;
            find.active = 0;
            let first = find.hits.first().map(|hit| hit.seq);
            if let Some(seq) = first {
                self.history_find_scroll_to_seq(seq);
            }
        }
        cx.notify();
    }

    pub(crate) fn history_find_step(&mut self, cx: &mut Context<Self>, step: isize) {
        let Some(find) = self.history.find.as_mut() else {
            return;
        };
        if find.hits.is_empty() {
            return;
        }
        let len = find.hits.len() as isize;
        find.active = ((find.active as isize + step).rem_euclid(len)) as usize;
        let seq = find.hits[find.active].seq;
        self.history_find_scroll_to_seq(seq);
        cx.notify();
    }

    pub(crate) fn history_find_close(&mut self, cx: &mut Context<Self>) {
        if self.history.find.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn confirm_delete_history_session(
        &mut self,
        session: ConversationMeta,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let app = cx.entity();
        let title = if session.title.trim().is_empty() {
            "Untitled Conversation".to_string()
        } else {
            session.title.clone()
        };
        let file_path = session.file_path.clone();
        let key = session.key.clone();
        let dialog_width =
            responsive_dialog_width(window.bounds().size.width.to_f64(), 0.84, 320.0, 440.0);
        window.open_dialog(cx, move |dialog, _window, cx| {
            let confirm_app = app.clone();
            let confirm_key = key.clone();
            let confirm_file_path = file_path.clone();
            dialog
                .title("Delete Conversation")
                .w(px(dialog_width))
                .button_props(
                    gpui_component::dialog::DialogButtonProps::default()
                        .ok_text("Delete")
                        .cancel_text("Cancel"),
                )
                .footer(|ok, cancel, window, cx| vec![cancel(window, cx), ok(window, cx)])
                .child(
                    v_flex()
                        .gap(crate::ui_metrics::DIALOG_CONTENT_GAP)
                        .child(div().text_size(crate::theme::FONT_BODY).child(format!(
                            "Are you sure you want to permanently delete \"{title}\"?"
                        )))
                        .child(
                            div()
                                .text_size(crate::theme::FONT_META)
                                .text_color(cx.theme().danger)
                                .child("This will physically remove the source file from disk."),
                        )
                        .when(!file_path.is_empty(), |el| {
                            el.child(
                                div()
                                    .text_size(crate::theme::FONT_META)
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(file_path.clone()),
                            )
                        }),
                )
                .on_ok(move |_, window, app| {
                    confirm_app.update(app, |view, cx| {
                        view.delete_history_session_confirmed(
                            confirm_key.clone(),
                            confirm_file_path.clone(),
                            window,
                            cx,
                        );
                    });
                    true
                })
        });
    }

    pub(crate) fn delete_history_session_confirmed(
        &mut self,
        key: String,
        file_path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 1. Physically delete source file from disk
        let raw_path = if let Some((db, _)) = file_path.split_once('#') {
            db
        } else {
            &file_path
        };
        let p = std::path::Path::new(raw_path);
        if p.exists() && p.is_file() {
            let _ = std::fs::remove_file(p);
        }

        // 2. Remove from catalog database
        let db_path = history_db_path();
        let key_clone = key.clone();
        cx.background_executor()
            .spawn(async move {
                if let Ok(mut catalog) = HistoryCatalog::open(&db_path) {
                    let _ = catalog.delete_session(&key_clone);
                }
            })
            .detach();

        // 3. Update in-memory state
        self.history.sessions.retain(|s| s.key != key);
        if self.history.selected_key.as_deref() == Some(&key) {
            self.history.selected_key = None;
            self.history.transcript = None;
        }
        window.push_notification("Conversation permanently deleted", cx);
        self.notify_sidebar(cx);
        cx.notify();
    }

    fn history_find_scroll_to_seq(&mut self, seq: i64) {
        use crate::agent_ui::conversation::ConversationRow;
        let Some(transcript) = self.history.transcript.as_ref() else {
            return;
        };
        let turns = crate::agent_ui::conversation::derive_turns(&transcript.messages);
        let rows = crate::agent_ui::conversation::folded_conversation_rows(
            &transcript.messages,
            &turns,
            false,
            false,
            false,
            &expanded_tool_groups_of(&self.history.expanded_content),
        );
        let ix = rows.iter().position(|row| match row {
            ConversationRow::UserPrompt(index) | ConversationRow::Answer(index) => transcript
                .messages
                .get(*index)
                .is_some_and(|message| message.seq == seq),
            ConversationRow::ToolActivity { message, .. } => transcript
                .messages
                .get(*message)
                .is_some_and(|message| message.seq == seq),
            _ => false,
        });
        if let Some(ix) = ix {
            self.history.viewport.list.scroll_to_reveal_item(ix);
        }
    }

    pub(crate) fn open_history_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // notate 2026-08-29: entering search from History's top right presets the
        // scope to `#history` (the search box remains the single global entry; the
        // scope chip's × clears back to global).
        let preset = |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            let Some(picker) = view.client_picker.as_ref() else {
                return;
            };
            let input = picker.input.clone();
            let list = picker.list.clone();
            let query = input.read(cx).value().to_string();
            if query.contains("#history") {
                return;
            }
            let scoped = format!("#history {query}");
            input.update(cx, |state, cx| state.set_value(&scoped, window, cx));
            let job = list.update(cx, |state, lcx| {
                state.delegate_mut().perform_search(&scoped, window, lcx)
            });
            job.detach();
        };
        if self.search_open {
            preset(self, window, cx);
            return;
        }
        self.open_search(&OpenSearch, window, cx);
        preset(self, window, cx);
    }
}
