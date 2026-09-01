//! [INPUT]: Existing imports and types from the crate root (via the history module root glob: `use super::*` chain).
//! [OUTPUT]: For the crate::history family: the History main page rendering (history_page) — master-detail layout, compact breakpoint policy, conversation cards, right-click context menu.
//! [POS]: Page-view responsibility slice of the herdr-gui History surface; mechanically split out of history.rs.
use super::*;

pub(super) const HISTORY_LIST_MAX_WIDTH: f32 = 336.0;

pub(super) const HISTORY_LIST_MIN_WIDTH: f32 = 180.0;

pub(super) fn history_conversation_context_menu(
    menu: PopupMenu,
    session: ConversationMeta,
    herdr: Entity<ShardlaneApp>,
) -> PopupMenu {
    let open_session = session.clone();
    let open_herdr = herdr.clone();
    let continue_session = session.clone();
    let continue_herdr = herdr.clone();
    let title = session.title.clone();
    let project_path = session.project_path.clone();

    let mut menu = menu.item(PopupMenuItem::new("Open Conversation").on_click(
        move |_, window, app| {
            let compact_history = history_uses_compact_layout(window.bounds().size.width.to_f64());
            open_herdr.update(app, |view, cx| {
                if compact_history {
                    view.history.detail_only = true;
                }
                view.select_history_session(open_session.clone(), cx);
            });
        },
    ));
    // M6: Continue gating = a continuation plan is available (AlreadyLive /
    // NativeResume / ContextTransfer — any strategy reachable), no longer the
    // static resume_supported.
    if history_continuation_available(&session) {
        menu = menu.item(PopupMenuItem::new("Continue Conversation").on_click(
            move |_, window, app| {
                continue_herdr.update(app, |view, cx| {
                    view.continue_history_session(continue_session.clone(), window, cx)
                });
            },
        ));
    }

    let title_for_copy = title.clone();
    menu = menu
        .item(PopupMenuItem::separator())
        .item(PopupMenuItem::new("Copy Title").on_click(move |_, _, app| {
            app.write_to_clipboard(crepuscularity_gpui::ClipboardItem::new_string(
                title_for_copy.clone(),
            ));
        }));
    if !project_path.trim().is_empty() {
        menu = menu.item(
            PopupMenuItem::new("Copy Project Path").on_click(move |_, _, app| {
                app.write_to_clipboard(crepuscularity_gpui::ClipboardItem::new_string(
                    project_path.clone(),
                ));
            }),
        );
    }
    let id_for_copy = session.id.clone();
    menu = menu.item(
        PopupMenuItem::new("Copy Session ID").on_click(move |_, _, app| {
            app.write_to_clipboard(crepuscularity_gpui::ClipboardItem::new_string(
                id_for_copy.clone(),
            ));
        }),
    );
    if !session.file_path.trim().is_empty() {
        let file_path = session.file_path.clone();
        menu = menu.item(
            PopupMenuItem::new("Reveal Source File").on_click(move |_, _, _| {
                let _ = std::process::Command::new("open")
                    .arg("-R")
                    .arg(&file_path)
                    .spawn();
            }),
        );
    }
    let export_session = session;
    let export_herdr = herdr.clone();
    menu = menu.item(
        PopupMenuItem::new("Export as Markdown…").on_click(move |_, window, app| {
            export_herdr.update(app, |view, cx| {
                view.export_history_markdown(export_session.clone(), window, cx)
            });
        }),
    );
    menu
}

pub(super) fn history_uses_compact_layout(window_width: f64) -> bool {
    window_width < SIDEBAR_AUTO_COLLAPSE_WINDOW_WIDTH
}

pub(super) fn history_should_render_detail(compact_history: bool, detail_only: bool) -> bool {
    !compact_history || detail_only
}

impl ShardlaneApp {
    pub(crate) fn history_page(
        &mut self,
        theme: UiTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let component_theme = cx.theme().clone();
        let content_theme = self.content_surface_theme(window);
        let content_button = content_theme.button_variant(cx);
        let dark = theme.bg <= 0x808080;
        let herdr = cx.entity();
        // Sort menu (same as the earlier conversation list: three keys + direction,
        // checked marks the current state).
        let sort_key = self.history.sort_key;
        let sort_ascending = self.history.sort_ascending;
        let sort_label = match sort_key {
            HistorySortKey::Updated => "Date updated",
            HistorySortKey::Created => "Date created",
            HistorySortKey::Messages => "Message count",
        };
        let sort_menu_herdr = herdr.clone();
        let sort_menu = DropdownButton::new("history-sort")
            .xsmall()
            .button(
                Button::new("history-sort-button")
                    .custom(content_button)
                    .xsmall()
                    .label(sort_label)
                    .tooltip("Sort conversations"),
            )
            .dropdown_menu(move |mut menu, _, _| {
                let key_item = |menu: gpui_component::menu::PopupMenu,
                                label: &'static str,
                                key: HistorySortKey| {
                    let h = sort_menu_herdr.clone();
                    menu.item(PopupMenuItem::new(label).checked(sort_key == key).on_click(
                        move |_, _, app| {
                            h.update(app, |view, cx| {
                                if view.history.sort_key != key {
                                    view.history.sort_key = key;
                                    view.sort_history_sessions_in_place();
                                    cx.notify();
                                }
                            });
                        },
                    ))
                };
                menu = key_item(menu, "Date updated", HistorySortKey::Updated);
                menu = key_item(menu, "Date created", HistorySortKey::Created);
                menu = key_item(menu, "Message count", HistorySortKey::Messages);
                let dir_h = sort_menu_herdr.clone();
                menu = menu.separator().item(
                    PopupMenuItem::new("Descending")
                        .checked(!sort_ascending)
                        .on_click(move |_, _, app| {
                            dir_h.update(app, |view, cx| {
                                if view.history.sort_ascending {
                                    view.history.sort_ascending = false;
                                    view.sort_history_sessions_in_place();
                                    cx.notify();
                                }
                            });
                        }),
                );
                let dir_h = sort_menu_herdr.clone();
                menu.item(
                    PopupMenuItem::new("Ascending")
                        .checked(sort_ascending)
                        .on_click(move |_, _, app| {
                            dir_h.update(app, |view, cx| {
                                if !view.history.sort_ascending {
                                    view.history.sort_ascending = true;
                                    view.sort_history_sessions_in_place();
                                    cx.notify();
                                }
                            });
                        }),
                )
            });
        let back_herdr = herdr.clone();
        let clear_filters_herdr = herdr.clone();
        let compact_history = history_uses_compact_layout(window.bounds().size.width.to_f64());
        let filtered_sessions = self
            .history
            .sessions
            .iter()
            .filter(|session| self.history_session_matches_filters(session))
            .cloned()
            .collect::<Vec<_>>();
        let mut agent_options = self
            .history
            .sessions
            .iter()
            .map(|session| session.agent)
            .collect::<Vec<_>>();
        agent_options.sort_by_key(|agent| agent.as_str());
        agent_options.dedup();
        let mut project_options = self
            .history
            .sessions
            .iter()
            .filter(|session| !session.project_path.is_empty())
            .map(|session| (session.project_path.clone(), session.project_name.clone()))
            .collect::<Vec<_>>();
        project_options.sort_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)));
        project_options.dedup_by(|left, right| left.0 == right.0);

        let agent_filter_label = self
            .history
            .filter_agent
            .map(AgentId::display_name)
            .unwrap_or("All Agents");
        let project_filter_label = compact_history_label(
            self.history
                .filter_project
                .as_deref()
                .and_then(|path| {
                    self.history
                        .sessions
                        .iter()
                        .find(|session| session.project_path == path)
                        .map(|session| session.project_name.as_str())
                })
                .unwrap_or("All Projects"),
            24,
        );

        let agent_menu_herdr = herdr.clone();
        let agent_filter = DropdownButton::new("history-agent-filter")
            .xsmall()
            .button(
                Button::new("history-agent-filter-button")
                    .custom(content_button)
                    .xsmall()
                    .selected(self.history.filter_agent.is_some())
                    .label(agent_filter_label),
            )
            .dropdown_menu(move |mut menu, _, _| {
                let all_herdr = agent_menu_herdr.clone();
                menu = menu.item(PopupMenuItem::new("All Agents").on_click(
                    move |_, _window, app| {
                        all_herdr.update(app, |view, cx| view.set_history_agent_filter(None, cx));
                    },
                ));
                for agent in &agent_options {
                    let agent = *agent;
                    let filter_herdr = agent_menu_herdr.clone();
                    menu = menu.item(PopupMenuItem::new(agent.display_name()).on_click(
                        move |_, _window, app| {
                            filter_herdr.update(app, |view, cx| {
                                view.set_history_agent_filter(Some(agent), cx)
                            });
                        },
                    ));
                }
                menu
            });

        let project_menu_herdr = herdr.clone();
        let project_filter = DropdownButton::new("history-project-filter")
            .xsmall()
            .button(
                Button::new("history-project-filter-button")
                    .custom(content_button)
                    .xsmall()
                    .selected(self.history.filter_project.is_some())
                    .label(project_filter_label),
            )
            .dropdown_menu(move |mut menu, _, _| {
                let all_herdr = project_menu_herdr.clone();
                menu = menu.item(PopupMenuItem::new("All Projects").on_click(
                    move |_, _window, app| {
                        all_herdr.update(app, |view, cx| view.set_history_project_filter(None, cx));
                    },
                ));
                for (path, name) in &project_options {
                    let path = path.clone();
                    let filter_herdr = project_menu_herdr.clone();
                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                        move |_, _window, app| {
                            filter_herdr.update(app, |view, cx| {
                                view.set_history_project_filter(Some(path.clone()), cx)
                            });
                        },
                    ));
                }
                menu
            });

        let session_list = if filtered_sessions.is_empty() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .px_4()
                .text_center()
                .text_color(component_theme.muted)
                .child(
                    if self.history.loading && self.history.sessions.is_empty() {
                        h_flex()
                            .gap_2()
                            .child(Spinner::new().small())
                            .child("Loading history…")
                            .into_any_element()
                    } else if self.history.sessions.is_empty() {
                        v_flex()
                            .items_center()
                            .gap_2()
                            .child(Icon::new(ComponentIconName::BookOpen).small())
                            .child(
                                div()
                                    .text_color(component_theme.foreground)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("No history yet"),
                            )
                            .child(div().text_size(theme::FONT_META).child(
                                "Agent conversations will appear here when history is indexed.",
                            ))
                            .into_any_element()
                    } else {
                        v_flex()
                            .items_center()
                            .gap_2()
                            .child(Icon::new(ComponentIconName::Search).small())
                            .child(
                                div()
                                    .text_color(component_theme.foreground)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("No matching conversations"),
                            )
                            .child(
                                Button::new("history-clear-filters")
                                    .custom(content_button)
                                    .xsmall()
                                    .label("Clear filters")
                                    .on_click(move |_, _, app| {
                                        clear_filters_herdr
                                            .update(app, |view, cx| view.clear_history_filters(cx));
                                    }),
                            )
                            .into_any_element()
                    },
                )
                .into_any_element()
        } else {
            let sessions = filtered_sessions;
            let selected_key = self.history.selected_key.clone();
            let descriptions = self.history.descriptions.clone();
            let list_theme = content_theme;
            let list_herdr = herdr.clone();
            let open_detail_only = compact_history;
            let session_project_paths: HashMap<String, String> = sessions
                .iter()
                .map(|session| {
                    (
                        session.key.clone(),
                        self.resolved_history_session_project_path(session),
                    )
                })
                .collect();
            // Fixed row height: uniform_list virtualizes with a single item height;
            // variable-height rows would misalign. Title 1 line + Description 2
            // lines + meta 1 line fully fit within this height.
            const SESSION_ROW_HEIGHT: f32 = 100.0;
            uniform_list(
                "history-session-scroll",
                sessions.len(),
                move |range, _window, _app| {
                    range
                        .filter_map(|index| sessions.get(index).cloned())
                        .map(|session| {
                            let selected = selected_key.as_deref() == Some(session.key.as_str());
                            let session_for_click = session.clone();
                            let click_herdr = list_herdr.clone();
                            let menu_session = session.clone();
                            let menu_herdr = list_herdr.clone();
                            let project_path_label = session_project_paths
                                .get(&session.key)
                                .cloned()
                                .unwrap_or_else(|| session.project_name.clone());
                            let description =
                                history_session_list_description(&session, &descriptions);
                            let lead = agent_brand_icon(session.agent.as_str(), dark)
                                .map(|path| img(path).size(px(15.0)).into_any_element())
                                .unwrap_or_else(|| {
                                    Icon::new(ComponentIconName::BookOpen)
                                        .small()
                                        .into_any_element()
                                });
                            div()
                                .id(gpui::ElementId::Name(
                                    format!("history-session-{}", session.key).into(),
                                ))
                                .w_full()
                                .mb(px(6.0))
                                .h(px(SESSION_ROW_HEIGHT))
                                .px_3()
                                .py_2()
                                .rounded(px(6.0))
                                .cursor_pointer()
                                .overflow_hidden()
                                .text_color(list_theme.foreground)
                                .when(selected, |row| row.bg(list_theme.active))
                                .when(!selected, |row| {
                                    row.hover(|style| style.bg(list_theme.hover))
                                })
                                .active(|style| style.bg(list_theme.active))
                                .child(
                                    v_flex()
                                        .w_full()
                                        .h_full()
                                        .justify_center()
                                        .gap(SPACE_ICON)
                                        .child(
                                            h_flex().gap(px(8.0)).min_w_0().child(lead).child(
                                                div()
                                                    .min_w_0()
                                                    .flex_1()
                                                    .truncate()
                                                    .text_size(px(13.5))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(session.title.clone()),
                                            ),
                                        )
                                        .child(
                                            div()
                                                .min_w_0()
                                                .line_clamp(2)
                                                .text_ellipsis()
                                                .whitespace_normal()
                                                .text_size(theme::FONT_DESCRIPTION)
                                                .text_color(list_theme.foreground.opacity(0.62))
                                                .child(description),
                                        )
                                        .child(
                                            div()
                                                .min_w_0()
                                                .truncate()
                                                .text_size(theme::FONT_META)
                                                .text_color(list_theme.foreground.opacity(0.62))
                                                .child(format!(
                                                    "{} · {}",
                                                    session.agent.display_name(),
                                                    project_path_label
                                                )),
                                        ),
                                )
                                .shardlane_interactive(
                                    list_theme.primary.opacity(INTERACTIVE_FOCUS_OPACITY),
                                    move |_, app| {
                                        click_herdr.update(app, |view, cx| {
                                            if open_detail_only {
                                                view.history.detail_only = true;
                                            }
                                            view.select_history_session(
                                                session_for_click.clone(),
                                                cx,
                                            )
                                        });
                                    },
                                )
                                .context_menu(move |menu, _, _| {
                                    history_conversation_context_menu(
                                        menu,
                                        menu_session.clone(),
                                        menu_herdr.clone(),
                                    )
                                })
                        })
                        .collect::<Vec<_>>()
                },
            )
            .flex_1()
            .min_h_0()
            .px_2()
            .pb_2()
            .into_any_element()
        };

        let insights_tab_open = self.history.insights_tab_open;

        // Trigger insights load lazily when the insights view is active.
        if insights_tab_open && self.history.insights.is_none() && !self.history.insights_loading {
            self.load_history_insights(cx);
        }

        let insights_snap = self.history.insights.clone();
        let component_theme_for_insights = component_theme.clone();

        let list_panel = v_flex()
            .h_full()
            .when(compact_history, |panel| panel.flex_1().w_full())
            .when(!compact_history, |panel| {
                panel
                    .w(relative(0.28))
                    .min_w(px(HISTORY_LIST_MIN_WIDTH))
                    .max_w(px(HISTORY_LIST_MAX_WIDTH))
                    .flex_shrink_0()
                    .border_r_1()
                    .border_color(component_theme.sidebar_border)
            })
            .bg(content_theme.background)
            .text_color(content_theme.foreground)
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .flex_wrap()
                    .px_2()
                    .py_2()
                    .gap_1()
                    .bg(content_theme.background)
                    .child(agent_filter)
                    .child(project_filter)
                    .child(sort_menu),
            )
            .child(session_list)
            .into_any_element();

        // R3: the History Detail composer (same AgentComposer shell as Live Chat).
        self.ensure_history_composer(window, cx);
        let history_composer = self.history_composer_element(&content_theme, cx);
        // In-conversation find bar (shown when ⌘F is open, notate 08-29 round five).
        let history_find_bar = self
            .history
            .find
            .as_ref()
            .map(|find| crate::agent_ui::find::conversation_find_bar(find, &herdr, cx));

        let show_detail = history_should_render_detail(compact_history, self.history.detail_only);
        let detail = if insights_tab_open && show_detail {
            // Insights panel replaces the transcript when active.
            insights_panel(
                insights_snap.as_ref(),
                &content_theme,
                &component_theme_for_insights,
            )
        } else if show_detail {
            match &self.history.transcript {
                Some(transcript) => render_transcript(
                    transcript,
                    self.resolved_history_session_project_path(&transcript.meta),
                    TranscriptRenderState {
                        theme: &content_theme,
                        viewport: &mut self.history.viewport,
                        expanded_content: &self.history.expanded_content,
                        composer: history_composer,
                        find_bar: history_find_bar,
                        herdr: herdr.clone(),
                    },
                ),
                None if self.history.selected_key.is_some() => v_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .child(Spinner::new().small())
                    .into_any_element(),
                None => v_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .text_center()
                    .text_color(content_theme.muted)
                    .child(Icon::new(ComponentIconName::BookOpen).small())
                    .child(
                        div()
                            .text_color(content_theme.foreground)
                            .font_weight(FontWeight::MEDIUM)
                            .child("Select a conversation"),
                    )
                    .child(
                        // UX (2026-08-27 walkthrough): with an empty library this line
                        // is the wrong guidance — there is nothing to select.
                        div()
                            .text_size(theme::FONT_META)
                            .child("No conversations yet. Run an agent in a terminal and it will appear here."),
                    )
                    .into_any_element(),
            }
        } else {
            div().into_any_element()
        };

        // H7 (notate 2026-08-29): the desktop 42px breadcrumb bar was removed
        // entirely — the title lives in the titlebar, and Search/Refresh moved to
        // the titlebar's right. The narrow-window detail_only back button remains
        // the single exception (needed for navigation).
        let compact_back_row = (compact_history && self.history.detail_only).then(|| {
            h_flex()
                .h(px(42.0))
                .bg(content_theme.background)
                .flex_shrink_0()
                .px(CONTENT_INSET)
                .child(
                    Button::new("history-back")
                        .custom(content_button)
                        .xsmall()
                        .icon(ComponentIconName::ArrowLeft)
                        .label("Back to conversations")
                        .on_click(move |_, _, app| {
                            back_herdr.update(app, |view, cx| {
                                view.history.detail_only = false;
                                cx.notify();
                            });
                        }),
                )
                .into_any_element()
        });

        v_flex()
            .size_full()
            .bg(content_theme.background)
            .text_color(content_theme.foreground)
            .children(compact_back_row)
            .when_some(self.history.error.as_ref(), |page, error| {
                page.child(
                    div()
                        .flex_shrink_0()
                        .px(CONTENT_INSET)
                        .py_2()
                        .bg(content_theme.danger.opacity(0.08))
                        .text_size(theme::FONT_DESCRIPTION)
                        .text_color(content_theme.danger)
                        .child(error.clone()),
                )
            })
            .child(if compact_history {
                h_flex()
                    .w_full()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .when(!self.history.detail_only, |row| row.child(list_panel))
                    .when(self.history.detail_only, |row| {
                        row.child(
                            div()
                                .w_full()
                                .h_full()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .overflow_hidden()
                                .child(detail),
                        )
                    })
                    .into_any_element()
            } else {
                // Wide window: only show the detail panel.
                // The session list is in the Sidebar on the left.
                h_flex()
                    .w_full()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(
                        div()
                            .w_full()
                            .h_full()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .overflow_hidden()
                            .child(detail),
                    )
                    .into_any_element()
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_master_detail_collapses_below_compact_width() {
        assert!(history_uses_compact_layout(360.0));
        assert!(history_uses_compact_layout(480.0));
        assert!(history_uses_compact_layout(720.0));
        assert!(history_uses_compact_layout(979.0));
        assert!(!history_uses_compact_layout(980.0));
        assert!(!history_uses_compact_layout(1280.0));
        assert!(!history_should_render_detail(true, false));
        assert!(history_should_render_detail(true, true));
        assert!(history_should_render_detail(false, false));
    }
}
