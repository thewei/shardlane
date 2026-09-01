//! [INPUT]: Existing imports and types from the crate root (via the history module root glob: `use super::*` chain).
//! [OUTPUT]: For the crate::history family: the History sidebar list rendering (history_sidebar).
//! [POS]: Sidebar-view responsibility slice of the herdr-gui History surface; mechanically split out of history.rs.
use super::*;

impl ShardlaneApp {
    /// Sidebar filter button (notate 08-29 round five): Agent/Project/Time
    /// submenus + clear; highlights while any filter is active. Shares state
    /// with the page filters (HistoryUiState.filter_*).
    pub(super) fn history_sidebar_filter_button(
        &self,
        filters_active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use gpui_component::button::ButtonVariants as _;
        use gpui_component::menu::DropdownMenu as _;
        use gpui_component::Sizable as _;
        let herdr = cx.entity();
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
        project_options
            .sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
        project_options.dedup_by(|left, right| left.0 == right.0);
        let filter_agent = self.history.filter_agent;
        let filter_project = self.history.filter_project.clone();
        let filter_time = self.history.filter_time;

        gpui_component::button::Button::new("history-sidebar-filter")
            .ghost()
            .xsmall()
            .icon(
                gpui_component::Icon::empty()
                    .path("icons/list-filter.svg")
                    .with_size(px(13.0)),
            )
            .tooltip("Filter conversations")
            .selected(filters_active)
            .dropdown_menu_with_anchor(gpui::Corner::BottomRight, move |mut menu, window, cx| {
                // Agent (AI Provider) submenu.
                let agent_herdr = herdr.clone();
                let agent_options = agent_options.clone();
                menu = menu.submenu("Agent", window, cx, move |submenu, _, _| {
                    let all_herdr = agent_herdr.clone();
                    let mut submenu = submenu.item(
                        PopupMenuItem::new("All Agents")
                            .checked(filter_agent.is_none())
                            .on_click(move |_, _, app| {
                                all_herdr.update(app, |view, cx| {
                                    view.set_history_agent_filter(None, cx)
                                });
                            }),
                    );
                    for agent in &agent_options {
                        let agent = *agent;
                        let item_herdr = agent_herdr.clone();
                        submenu = submenu.item(
                            PopupMenuItem::new(agent.display_name())
                                .checked(filter_agent == Some(agent))
                                .on_click(move |_, _, app| {
                                    item_herdr.update(app, |view, cx| {
                                        view.set_history_agent_filter(Some(agent), cx)
                                    });
                                }),
                        );
                    }
                    submenu
                });
                // Project submenu.
                let project_herdr = herdr.clone();
                let project_options = project_options.clone();
                let filter_project = filter_project.clone();
                menu = menu.submenu("Project", window, cx, move |submenu, _, _| {
                    let all_herdr = project_herdr.clone();
                    let mut submenu = submenu.item(
                        PopupMenuItem::new("All Projects")
                            .checked(filter_project.is_none())
                            .on_click(move |_, _, app| {
                                all_herdr.update(app, |view, cx| {
                                    view.set_history_project_filter(None, cx)
                                });
                            }),
                    );
                    for (path, name) in &project_options {
                        let path = path.clone();
                        let item_herdr = project_herdr.clone();
                        submenu = submenu.item(
                            PopupMenuItem::new(name.clone())
                                .checked(filter_project.as_deref() == Some(path.as_str()))
                                .on_click(move |_, _, app| {
                                    item_herdr.update(app, |view, cx| {
                                        view.set_history_project_filter(Some(path.clone()), cx)
                                    });
                                }),
                        );
                    }
                    submenu
                });
                // Time submenu (rolling windows).
                let time_herdr = herdr.clone();
                menu = menu.submenu("Time", window, cx, move |submenu, _, _| {
                    HistoryTimeFilter::TIME_FILTERS.iter().copied().fold(
                        submenu,
                        |submenu, filter| {
                            let item_herdr = time_herdr.clone();
                            submenu.item(
                                PopupMenuItem::new(filter.label())
                                    .checked(filter_time == filter)
                                    .on_click(move |_, _, app| {
                                        item_herdr.update(app, |view, cx| {
                                            view.set_history_time_filter(filter, cx)
                                        });
                                    }),
                            )
                        },
                    )
                });
                if filters_active {
                    let clear_herdr = herdr.clone();
                    menu = menu.item(PopupMenuItem::separator()).item(
                        PopupMenuItem::new("Clear Filters").on_click(move |_, _, app| {
                            clear_herdr.update(app, |view, cx| view.clear_history_filters(cx));
                        }),
                    );
                }
                menu
            })
            .into_any_element()
    }

    pub(crate) fn history_sidebar(&self, theme: UiTheme, cx: &mut Context<Self>) -> AnyElement {
        let dark = theme.bg <= 0x808080;
        let back_herdr = cx.entity();
        let insights_herdr_clone = back_herdr.clone();
        let list_herdr = cx.entity();
        let selected_key = self.history.selected_key.clone();
        // notate 08-29 round five: the sidebar list uses the same filter
        // predicate as the page list.
        let sessions = self
            .history
            .sessions
            .iter()
            .filter(|session| self.history_session_matches_filters(session))
            .cloned()
            .collect::<Vec<_>>();
        let descriptions = self.history.descriptions.clone();
        let sidebar_theme = cx.theme().clone();
        let list_theme = sidebar_theme.clone();
        let session_project_paths: HashMap<String, String> = sessions
            .iter()
            .map(|session| {
                (
                    session.key.clone(),
                    self.resolved_history_session_project_path(session),
                )
            })
            .collect();

        let session_list = if sessions.is_empty() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .px_4()
                .text_center()
                .text_size(theme::FONT_META)
                .text_color(sidebar_theme.muted)
                .child(if self.history.loading {
                    h_flex()
                        .gap_2()
                        .child(Spinner::new().small())
                        .child("Loading history…")
                        .into_any_element()
                } else {
                    v_flex()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(ComponentIconName::BookOpen).small())
                        .child("No history yet")
                        .into_any_element()
                })
                .into_any_element()
        } else {
            uniform_list(
                "history-sidebar-sessions",
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
                            let title_for_tooltip = session.title.clone();
                            // Card language (notate 2026-08-29 H1-H3): the card chrome
                            // appears only on selected/hover; unselected is a quiet row;
                            // roomier line spacing (outer pb).
                            // Non-title text uses sidebar_foreground for contrast instead
                            // of sitting on the background.
                            div()
                                .id(gpui::ElementId::Name(
                                    format!("history-sidebar-session-{}", session.key).into(),
                                ))
                                .w_full()
                                .min_w_0()
                                .h(px(76.0))
                                .pb(px(10.0))
                                .cursor_pointer()
                                .active(|style| style.bg(list_theme.sidebar_accent))
                                .child(
                                    v_flex()
                                        .w_full()
                                        .h_full()
                                        .min_w_0()
                                        .px(px(10.0))
                                        .py(px(6.0))
                                        .justify_center()
                                        .gap(SPACE_ICON)
                                        .rounded(list_theme.radius)
                                        .overflow_hidden()
                                        .text_color(list_theme.sidebar_foreground)
                                        .when(selected, |row| {
                                            row.bg(list_theme.sidebar_foreground.opacity(0.13))
                                                .border_1()
                                                .border_color(
                                                    list_theme.sidebar_foreground.opacity(0.12),
                                                )
                                        })
                                        .when(!selected, |row| {
                                            row.hover(|style| {
                                                style
                                                    .bg(list_theme.sidebar_foreground.opacity(0.06))
                                            })
                                        })
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .min_w_0()
                                                .items_center()
                                                .gap_2()
                                                .child(lead)
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
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
                                                .text_color(if selected {
                                                    list_theme.sidebar_foreground.opacity(0.78)
                                                } else {
                                                    list_theme.sidebar_foreground.opacity(0.62)
                                                })
                                                .child(description),
                                        )
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .min_w_0()
                                                .justify_between()
                                                .items_center()
                                                .gap_2()
                                                .text_size(theme::FONT_META)
                                                .text_color(if selected {
                                                    list_theme.sidebar_foreground.opacity(0.78)
                                                } else {
                                                    list_theme.sidebar_foreground.opacity(0.62)
                                                })
                                                .child(div().min_w_0().truncate().child(format!(
                                                    "{} · {}",
                                                    session.agent.display_name(),
                                                    project_path_label
                                                )))
                                                .when(session.message_count > 0, |meta| {
                                                    meta.child(
                                                        div()
                                                            .flex_shrink_0()
                                                            .text_size(theme::FONT_META)
                                                            .child(format!(
                                                                "{} msgs",
                                                                session.message_count
                                                            )),
                                                    )
                                                }),
                                        ),
                                )
                                .tooltip(move |_, cx| {
                                    cx.new(|_| Tooltip::new(title_for_tooltip.clone())).into()
                                })
                                .shardlane_interactive(
                                    list_theme.primary.opacity(INTERACTIVE_FOCUS_OPACITY),
                                    move |window, app| {
                                        let compact_history = history_uses_compact_layout(
                                            window.bounds().size.width.to_f64(),
                                        );
                                        click_herdr.update(app, |view, cx| {
                                            if compact_history {
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

        v_flex()
            .size_full()
            .bg(sidebar_theme.sidebar)
            .child(
                v_flex()
                    .w_full()
                    .flex_shrink_0()
                    .px(SIDEBAR_EDGE_INSET)
                    .pt(px(8.0))
                    .pb(px(8.0))
                    .gap_2()
                    .child(crate::sidebar::secondary_sidebar_back_row(
                        "history-back-to-app",
                        move |_, app| {
                            back_herdr.update(app, |view, cx| view.return_to_app_surface(cx));
                        },
                        cx,
                    ))
                    .child({
                        // Search/Refresh moved up into the content-area header (right of
                        // the titlebar); the sidebar row keeps title + total count +
                        // filter menu + Insights button (notate 08-29 round five).
                        let total = self.history.total_sessions.map(|total| total.to_string());
                        let filters_active = self.history_filters_active();
                        let insights_open = self.history.insights_tab_open;
                        let insights_herdr = insights_herdr_clone;
                        h_flex()
                            .w_full()
                            .px(px(8.0))
                            .justify_between()
                            .items_center()
                            .child(
                                h_flex()
                                    .gap(px(6.0))
                                    .items_center()
                                    .child(
                                        div()
                                            .text_size(theme::FONT_SECTION_TITLE)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(sidebar_theme.sidebar_foreground)
                                            .child("History"),
                                    )
                                    .when_some(total, |row, total| {
                                        row.child(
                                            div()
                                                .px(px(7.0))
                                                .py(px(1.0))
                                                .rounded(px(9.0))
                                                .bg(sidebar_theme.sidebar_foreground.opacity(0.10))
                                                .text_size(theme::FONT_META)
                                                .text_color(
                                                    sidebar_theme.sidebar_foreground.opacity(0.75),
                                                )
                                                .child(total),
                                        )
                                    }),
                            )
                            .child(
                                h_flex()
                                    .gap(px(2.0))
                                    .child(
                                        gpui_component::button::Button::new(
                                            "history-insights-toggle",
                                        )
                                        .ghost()
                                        .xsmall()
                                        .icon(
                                            gpui_component::Icon::empty()
                                                .path("icons/chart-column.svg")
                                                .with_size(px(13.0)),
                                        )
                                        .tooltip(if insights_open {
                                            "Back to conversations"
                                        } else {
                                            "Show statistics"
                                        })
                                        .selected(insights_open)
                                        .on_click(move |_, _, app| {
                                            insights_herdr.update(app, |view, cx| {
                                                view.history.insights_tab_open =
                                                    !view.history.insights_tab_open;
                                                if view.history.insights_tab_open
                                                    && view.history.insights.is_none()
                                                    && !view.history.insights_loading
                                                {
                                                    view.load_history_insights(cx);
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .into_any_element(),
                                    )
                                    .child(self.history_sidebar_filter_button(filters_active, cx)),
                            )
                    }),
            )
            .child(session_list)
            .into_any_element()
    }
}
