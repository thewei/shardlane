//! [INPUT]: Existing imports and types from the crate root (via the history module root glob: `use super::*` chain).
//! [OUTPUT]: For the crate::history family: the transcript card-stream rendering (render_transcript) — full-width message cards / role badges / timestamps / Markdown copy / tool-call cards / edge lazy-load cursor.
//! [POS]: Transcript-view responsibility slice of the herdr-gui History surface; mechanically split out of history.rs.
use super::*;

/// Three-tier type scale for the conversation body (same as the earlier
/// conversation area; coexists with theme tokens: the message area is a reading
/// surface, so half a step larger than the UI tokens is intentional).
pub(super) const HISTORY_MSG_USER: gpui::Pixels = px(13.5);

/// Shared Markdown cache budget for expanded bodies (source bytes; same 2MiB
/// order of magnitude as Chat).
pub(super) const HISTORY_MD_BUDGET_BYTES: usize = 2 << 20;

pub(super) struct TranscriptRenderState<'a> {
    pub(super) theme: &'a ContentSurfaceTheme,
    /// Shared ConversationSurface viewport (R1).
    pub(super) viewport: &'a mut crate::agent_ui::conversation_surface::ConversationViewportState,
    pub(super) expanded_content: &'a HashSet<HistoryExpandedContent>,
    pub(super) composer: Option<AnyElement>,
    /// In-conversation find bar (shown when ⌘F is open, notate 08-29 round five).
    pub(super) find_bar: Option<AnyElement>,
    pub(super) herdr: Entity<ShardlaneApp>,
}

pub(super) fn render_transcript(
    transcript: &CachedTranscriptWindow,
    project_path_display: String,
    state: TranscriptRenderState<'_>,
) -> AnyElement {
    let render_started = Instant::now();
    let TranscriptRenderState {
        theme,
        viewport,
        expanded_content,
        composer,
        find_bar,
        herdr,
    } = state;
    let meta = &transcript.meta;
    let dark = theme.is_dark;

    let mut times: Vec<String> = Vec::new();
    if meta.created_at > 0 {
        times.push(format!("Created {}", history_abs_date(meta.created_at)));
    }
    if meta.updated_at > 0 {
        times.push(format!("Updated {}", history_abs_date(meta.updated_at)));
    }
    let project_badge = std::path::Path::new(&project_path_display)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| meta.project_name.clone());

    let header = v_flex()
        .flex_shrink_0()
        .px(px(24.0))
        .pt(px(16.0))
        .pb(px(14.0))
        .gap(px(10.0))
        .border_b_1()
        .border_color(theme.border.opacity(0.4))
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .justify_between()
                .items_center()
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_2()
                        .items_center()
                        .child(
                            agent_brand_icon(meta.agent.as_str(), dark)
                                .map(|path| {
                                    img(path).size(px(18.0)).flex_shrink_0().into_any_element()
                                })
                                .unwrap_or_else(|| {
                                    Icon::new(ComponentIconName::Bot)
                                        .with_size(px(16.0))
                                        .into_any_element()
                                }),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .font_weight(FontWeight::BOLD)
                                .text_size(theme::FONT_META)
                                .text_color(theme.foreground)
                                .child(meta.agent.display_name()),
                        )
                        .child(history_badge(
                            project_badge,
                            theme.hover,
                            theme.foreground.opacity(0.85),
                        ))
                        .when_some(meta.git_branch.clone(), |row, branch| {
                            row.child(history_badge(
                                format!("⑂ {branch}"),
                                theme.hover.opacity(0.6),
                                theme.muted,
                            ))
                        })
                        .when_some(meta.model.clone(), |row, model| {
                            row.child(history_outline_badge(
                                model,
                                gpui::rgb(HISTORY_MODEL_BADGE).into(),
                            ))
                        })
                        .when_some(
                            meta.source.clone().filter(|source| !source.is_empty()),
                            |row, source| row.child(history_outline_badge(source, theme.success)),
                        ),
                )
                .child({
                    let export_herdr = herdr.clone();
                    let export_meta = meta.clone();
                    let delete_herdr = herdr.clone();
                    let delete_meta = meta.clone();
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(
                            Button::new("history-detail-export")
                                .xsmall()
                                .ghost()
                                .label("Export")
                                .tooltip("Export as Markdown")
                                .on_click(move |_, window, app| {
                                    export_herdr.update(app, |view, cx| {
                                        view.export_history_markdown(
                                            export_meta.clone(),
                                            window,
                                            cx,
                                        );
                                    });
                                }),
                        )
                        .child(
                            Button::new("history-detail-delete")
                                .xsmall()
                                .ghost()
                                .icon(Icon::empty().path("icons/trash.svg").with_size(px(12.0)))
                                .label("Delete")
                                .tooltip("Permanently delete conversation")
                                .on_click(move |_, window, app| {
                                    delete_herdr.update(app, |view, cx| {
                                        view.confirm_delete_history_session(
                                            delete_meta.clone(),
                                            window,
                                            cx,
                                        );
                                    });
                                }),
                        )
                }),
        )
        .child(
            // H4 (notate 2026-08-29): single-line title with overflow ellipsis. A
            // text div hung directly on a flex_col gets measured at a tiny width
            // ("V2 Confirmation"→"V2", "Goal…"→"Goa", recorded in notate 08-29
            // round three); the badges/meta row in the same header (h_flex with
            // min_w_0 children) measures full width in practice — mirror that
            // structure to host the title.
            h_flex().w_full().min_w_0().child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_size(theme::FONT_APP_TITLE)
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.foreground)
                    .line_clamp(1)
                    .truncate()
                    .child(
                        if meta.title.trim().is_empty() || meta.title == "Untitled" {
                            if let Some(first_user) = transcript.messages.iter().find(|m| {
                                m.role == shardlane_history::Role::User && !m.text.trim().is_empty()
                            }) {
                                history_one_line(&first_user.text, 120)
                            } else {
                                meta.title.clone()
                            }
                        } else {
                            meta.title.clone()
                        },
                    ),
            ),
        )
        .child(
            h_flex()
                .w_full()
                .min_w_0()
                .justify_between()
                .items_center()
                .text_size(theme::FONT_META)
                .text_color(theme.muted)
                .child(
                    h_flex()
                        .min_w_0()
                        .gap_1()
                        .items_center()
                        .child(Icon::new(ComponentIconName::Folder).with_size(px(12.0)))
                        .child(
                            div()
                                .min_w_0()
                                .line_clamp(1)
                                .truncate()
                                .child(project_path_display),
                        ),
                )
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_2()
                        .items_center()
                        .child(div().child(format!("{} messages", transcript.total_messages)))
                        .when_some(meta.tokens_used, |row, tokens| {
                            row.child(div().child("·")).child(
                                div().child(format!("{} tokens", history_fmt_tokens(tokens))),
                            )
                        })
                        .when(!times.is_empty(), |row| {
                            row.child(div().child("·"))
                                .child(div().child(times.join("  ·  ")))
                        }),
                ),
        );

    // ── Shared Conversation projection (agent_ui::conversation) ──
    // Turn boundaries and the Worked fold come from the shared layer; History
    // only supplies expansion state and rendering.
    // The fold/expand key is the seq of the turn's first message — stable
    // across pages.
    let turns = crate::agent_ui::conversation::derive_turns(&transcript.messages);
    let expanded_turns: HashSet<usize> = expanded_content
        .iter()
        .filter_map(|content| match content {
            HistoryExpandedContent::Turn(seq) => turns.iter().position(|turn| {
                transcript
                    .messages
                    .get(turn.start)
                    .is_some_and(|message| message.seq == *seq)
            }),
            _ => None,
        })
        .collect();
    let rows = crate::agent_ui::conversation::folded_conversation_rows(
        &transcript.messages,
        &turns,
        &HashSet::new(),
        &expanded_turns,
        false,
    );

    // Row signatures → minimal splice accounting in the shared viewport (same
    // implementation as Live).
    let signatures: Vec<u64> = rows
        .iter()
        .map(|row| {
            crate::agent_ui::conversation_surface::conversation_row_signature(
                &transcript.messages,
                row,
            )
        })
        .collect();
    viewport.sync_rows(signatures);

    let elapsed = render_started.elapsed();
    if elapsed >= Duration::from_millis(8) {
        lag_log(format_args!(
            "history.transcript.render messages={} total_messages={} {:.1}ms",
            transcript.messages.len(),
            transcript.total_messages,
            elapsed.as_secs_f64() * 1_000.0
        ));
    }

    let weak = herdr.downgrade();
    let rows_for_closure = std::rc::Rc::new(rows);
    let row_count = rows_for_closure.len();
    let row_theme = *theme;
    let render_row = Box::new(move |ix: usize, window: &mut Window, app: &mut App| {
        let Some(view) = weak.upgrade() else {
            return div().into_any_element();
        };
        let rows = rows_for_closure.clone();
        view.update(app, |view, cx| {
            view.render_history_transcript_row(ix, &rows, &row_theme, window, cx)
        })
    });

    // R1: the full shared ConversationSurface (header keeps History's richer
    // two-line block; layout/viewport/scrollbar/paging come from the same source
    // as Live).
    crate::agent_ui::conversation_surface::conversation_surface(
        crate::agent_ui::conversation_surface::ConversationSurfaceProps {
            viewport,
            theme,
            header: header.into_any_element(),
            row_count,
            render_row,
            overlay: None,
            find_bar,
            composer,
            pager: None,
        },
    )
}

impl ShardlaneApp {
    /// Render one History transcript row (the shared surface's list callback;
    /// only visible rows are processed).
    /// Row structure/copy menu/target highlight keep History semantics; geometry
    /// is unified by the shared surface.
    pub(super) fn render_history_transcript_row(
        &mut self,
        ix: usize,
        rows: &std::rc::Rc<Vec<crate::agent_ui::conversation::ConversationRow>>,
        theme: &ContentSurfaceTheme,
        _window: &mut Window,
        cx: &mut Context<ShardlaneApp>,
    ) -> AnyElement {
        let Some(transcript) = self.history.transcript.as_ref() else {
            return div().into_any_element();
        };
        let Some(row) = rows.get(ix).copied() else {
            return div().into_any_element();
        };
        let turns = crate::agent_ui::conversation::derive_turns(&transcript.messages);
        let expanded_content = &self.history.expanded_content;
        let palette = crate::agent_ui::markdown::render::Palette::from_source(
            crate::agent_ui::markdown::palette_source_from_active(cx),
        );
        let md = &mut self.history.viewport.markdown;
        let selection = self.history.viewport.selection.clone();
        // ⌘F hit-set snapshot (closures cannot borrow cx; FindHit is cloneable).
        let find_lookup: Option<(std::rc::Rc<Vec<crate::agent_ui::find::FindHit>>, usize)> = self
            .history
            .find
            .as_ref()
            .map(|find| (std::rc::Rc::new(find.hits.clone()), find.active));

        // notate 08-29 round three (view unification): answer bodies always go
        // through the same Markdown engine as Chat; the "long answer plain-text
        // preview + Show more fold" branch was removed — it was the source of
        // view divergence (## /**/ - rendered literally). Volume is bounded by
        // the page window (transcript window + virtualization rendering only
        // visible rows) and the Markdown cache budget (HISTORY_MD_BUDGET_BYTES),
        // the same cost model as Live.
        let mut render_answer_body = move |message: &shardlane_history::TranscriptMessage,
                                           _theme: &ContentSurfaceTheme|
              -> AnyElement {
            let row_key: std::rc::Rc<str> = std::rc::Rc::from(
                format!("history-{}-{}", transcript.meta.key, message.seq).as_str(),
            );
            // ⌘F highlight (notate 08-29 round five).
            let search = find_lookup.as_ref().and_then(|(hits, active)| {
                crate::agent_ui::find::highlights_for(hits, message.seq, *active)
            });
            let rendered = md.render_answer(
                message.seq,
                &message.text,
                false,
                HISTORY_MD_BUDGET_BYTES,
                None,
                row_key,
                &palette,
                crate::agent_ui::markdown::render::Metrics::BODY,
                &selection.clone(),
                search,
            );
            div()
                .w_full()
                .min_w_0()
                .child(rendered.unwrap_or_else(|| div().into_any_element()))
                .into_any_element()
        };
        // User prompts use the same plain-text card as Chat (user_prompt_card), no clipping.
        let render_user_body = |message: &shardlane_history::TranscriptMessage,
                                theme: &ContentSurfaceTheme|
         -> AnyElement {
            div()
                .w_full()
                .min_w_0()
                .whitespace_normal()
                .text_size(HISTORY_MSG_USER)
                .text_color(theme.foreground)
                .child(message.text.clone())
                .into_any_element()
        };

        enum RenderedRow {
            Message { seq: i64, copy_text: String },
            Static,
        }
        let (rendered, element): (RenderedRow, AnyElement) = match row {
            crate::agent_ui::conversation::ConversationRow::UserPrompt(message_index) => {
                let message = &transcript.messages[message_index];
                let body = render_user_body(message, theme);
                let time_label = message.timestamp.map(history_fmt_msg_time);
                // CHAT-A05: the User card shares the same visual primitive as Chat.
                let card = crate::agent_ui::conversation_view::user_prompt_card(
                    time_label,
                    false,
                    body.into_any_element(),
                    theme,
                );
                (
                    RenderedRow::Message {
                        seq: message.seq,
                        copy_text: message.text.clone(),
                    },
                    card,
                )
            }
            crate::agent_ui::conversation::ConversationRow::Reasoning(message_index) => {
                let message = &transcript.messages[message_index];
                if message
                    .thinking
                    .as_ref()
                    .is_none_or(|t| t.trim().is_empty())
                {
                    return div().into_any_element();
                }
                let content = HistoryExpandedContent::Thinking(message.seq);
                let expanded = expanded_content.contains(&content);
                let toggle_herdr = cx.entity();
                let reasoning = crate::agent_ui::conversation_view::reasoning_row(
                    message,
                    crate::agent_ui::conversation_view::ReasoningPresentation::Expandable {
                        expanded,
                        on_toggle: std::rc::Rc::new(move |_window, app| {
                            toggle_herdr
                                .update(app, |view, cx| view.toggle_history_content(content, cx));
                        }),
                    },
                    theme,
                );
                (
                    RenderedRow::Message {
                        seq: message.seq,
                        copy_text: String::new(),
                    },
                    reasoning,
                )
            }
            crate::agent_ui::conversation::ConversationRow::ToolActivity {
                message: message_index,
                tool: tool_index,
            } => {
                let message = &transcript.messages[message_index];
                let Some(tool_call) = message.tool_calls.get(tool_index) else {
                    return div().into_any_element();
                };
                let content = HistoryExpandedContent::Tools(message.seq);
                let expanded = expanded_content.contains(&content);
                let activity_herdr = cx.entity();
                let tool_for_row = tool_call.clone();
                let mut cluster = v_flex().w_full().min_w_0().gap(px(4.0));
                cluster = cluster.child(crate::agent_ui::activity::render_activity_row(
                    SharedString::from(format!("history-activity-{}-{tool_index}", message.seq)),
                    &tool_for_row,
                    expanded,
                    move |_, app| {
                        activity_herdr
                            .update(app, |view, cx| view.toggle_history_content(content, cx));
                    },
                    theme,
                ));
                if expanded {
                    cluster = cluster.child(div().w_full().min_w_0().pl(px(16.0)).child(
                        crate::agent_ui::activity::render_tool_detail(tool_call, theme),
                    ));
                }
                (
                    RenderedRow::Message {
                        seq: message.seq,
                        copy_text: String::new(),
                    },
                    cluster.into_any_element(),
                )
            }
            crate::agent_ui::conversation::ConversationRow::Answer(message_index) => {
                let message = &transcript.messages[message_index];
                let body = render_answer_body(message, theme);
                (
                    RenderedRow::Message {
                        seq: message.seq,
                        copy_text: message.text.clone(),
                    },
                    body,
                )
            }
            crate::agent_ui::conversation::ConversationRow::ContextBoundary(message_index) => {
                let message = &transcript.messages[message_index];
                (
                    RenderedRow::Message {
                        seq: message.seq,
                        copy_text: message.text.clone(),
                    },
                    crate::agent_ui::conversation_view::context_boundary_row(message, theme),
                )
            }
            crate::agent_ui::conversation::ConversationRow::TurnFold(turn_index) => {
                let Some(turn) = turns.get(turn_index) else {
                    return div().into_any_element();
                };
                let Some(fold_seq) = transcript.messages.get(turn.start).map(|m| m.seq) else {
                    return div().into_any_element();
                };
                let step_count = crate::agent_ui::conversation::turn_work_step_count(
                    &transcript.messages,
                    &turns,
                    turn_index,
                );
                let duration = crate::agent_ui::activity::turn_work_duration(
                    &transcript.messages,
                    turn.range.clone(),
                );
                let content = HistoryExpandedContent::Turn(fold_seq);
                let expanded = self.history.expanded_content.contains(&content);
                let toggle_herdr = cx.entity();
                let label = crate::agent_ui::activity::worked_summary_label(step_count, duration);
                (
                    RenderedRow::Static,
                    crate::agent_ui::activity::render_worked_fold_row(
                        SharedString::from(format!("history-turn-fold-{fold_seq}")),
                        label,
                        expanded,
                        move |_, app| {
                            toggle_herdr
                                .update(app, |view, cx| view.toggle_history_content(content, cx));
                        },
                        theme,
                    )
                    .into_any_element(),
                )
            }
            crate::agent_ui::conversation::ConversationRow::ResponseFooter(turn_index) => {
                let Some(turn) = turns.get(turn_index) else {
                    return div().into_any_element();
                };
                let copy_text = crate::agent_ui::activity::turn_answer_text(
                    &transcript.messages,
                    turn.range.clone(),
                );
                if copy_text.is_empty() {
                    return div().into_any_element();
                }
                let time_label = transcript.messages[turn.range.end.saturating_sub(1)]
                    .timestamp
                    .map(history_fmt_msg_time);
                (
                    RenderedRow::Static,
                    crate::agent_ui::activity::render_turn_footer(
                        SharedString::from(format!("history-turn-footer-{turn_index}")),
                        time_label,
                        copy_text,
                        theme,
                    ),
                )
            }
            // The static projection never produces Working rows.
            crate::agent_ui::conversation::ConversationRow::WorkingIndicator => {
                return div().into_any_element();
            }
        };

        let is_target = match &rendered {
            RenderedRow::Message { seq, .. } => self.history.target_seq == Some(*seq),
            RenderedRow::Static => false,
        };
        // notate 08-29 round five: the row container carries the msg-row group —
        // hover reveals the copy button in the bottom right.
        let base_wrapper = div()
            .w_full()
            .min_w_0()
            .group("shardlane-msg-row")
            .relative()
            .px(px(16.0))
            .py(px(6.0))
            .when(ix == 0, |wrapper| wrapper.pt(px(14.0)))
            .when(ix + 1 == rows.len(), |wrapper| wrapper.pb(px(14.0)))
            .when(is_target, |wrapper| {
                wrapper
                    .rounded(px(8.0))
                    .bg(theme.primary.opacity(0.10))
                    .border_1()
                    .border_color(theme.primary.opacity(0.45))
            })
            .child(element);
        let row_element: AnyElement = match &rendered {
            RenderedRow::Message { seq, copy_text } if !copy_text.is_empty() => {
                let menu_text = copy_text.clone();
                base_wrapper
                    .child(
                        crate::agent_ui::conversation_view::hover_copy_message_button(
                            ("history-msg-copy", *seq as u64),
                            copy_text.clone(),
                            theme,
                        ),
                    )
                    .context_menu(move |menu, _, _| {
                        let menu_text = menu_text.clone();
                        menu.item(
                            PopupMenuItem::new("Copy message").on_click(move |_, _, app| {
                                app.write_to_clipboard(
                                    crepuscularity_gpui::ClipboardItem::new_string(
                                        menu_text.clone(),
                                    ),
                                );
                            }),
                        )
                    })
                    .into_any_element()
            }
            _ => base_wrapper.into_any_element(),
        };
        row_element
    }
}
