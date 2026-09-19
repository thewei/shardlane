//! [INPUT]: Depends on the main-crate namespace forwarded by the new_agent module root (super), ShardlaneApp's history-family catalog query recent_history_sessions_for_project, the crate-visible navigation entries open_history_session_from_search/open_project_history, the i18n seam, theme tokens, and agent brand icons.
//! [OUTPUT]: Provides ShardlaneApp::new_agent_recent_history_block — the recent-session list under the New Agent headline (latest RECENT_HISTORY_LIMIT by last update, exact project-path match) plus the trailing "More history records" jump row.
//! [POS]: new_agent 的最近会话展示切片，由 page.rs 的 new_agent_page 装配；点击复用 History surface 自己的入口（open_history_session_from_search / open_project_history），会话选择与跳转保持单一行为路径。
use super::*;

/// Number of sessions previewed under the headline.
pub(super) const RECENT_HISTORY_LIMIT: usize = 5;

impl ShardlaneApp {
    /// The headline's recent-session block: up to five sessions of the
    /// selected Project (newest update first) plus the "More history records"
    /// row that opens the History surface filtered to the same exact project
    /// path. `None` while no Project path is resolved (picker still on
    /// "Select Project").
    pub(super) fn new_agent_recent_history_block(
        &self,
        project_path: Option<String>,
        dark: bool,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let project_path = project_path.filter(|path| !path.is_empty())?;
        let theme = cx.theme();
        let foreground = theme.foreground;
        let muted = theme.muted_foreground;
        let hover_bg = theme.foreground.opacity(0.045);
        let app = cx.entity();

        let sessions =
            self.recent_history_sessions_for_project(&project_path, RECENT_HISTORY_LIMIT);
        let rows = sessions
            .into_iter()
            .map(|session| {
                let row_herdr = app.clone();
                let click_session = session.clone();
                let brand = crate::assets::agent_brand_icon(session.agent.as_str(), dark)
                    .map(str::to_string);
                div()
                    .id(SharedString::from(format!(
                        "new-agent-recent-history-{}",
                        session.key
                    )))
                    .w_full()
                    .h(px(34.0))
                    .px(px(10.0))
                    .rounded(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .cursor_pointer()
                    .hover(|style| style.bg(hover_bg))
                    .child(
                        brand
                            .as_deref()
                            .map(|path| img(path).size(px(14.0)).into_any_element())
                            .unwrap_or_else(|| {
                                Icon::new(ComponentIconName::Bot)
                                    .with_size(px(14.0))
                                    .text_color(muted)
                                    .into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(theme::FONT_BODY)
                            .text_color(foreground)
                            .child(session.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(theme::FONT_META)
                            .text_color(muted)
                            .child(session.agent.display_name()),
                    )
                    .on_click(move |_, _, app_cx| {
                        row_herdr.update(app_cx, |this, cx| {
                            this.open_history_session_from_search(click_session.clone(), None, cx)
                        });
                    })
            })
            .collect::<Vec<_>>();

        // Trailing jump: the History surface, entered with the same exact
        // project-path filter the preview above applies.
        let more_herdr = app;
        let more_label = i18n::t("new_agent.recent_history_more");
        let more_row = div()
            .id("new-agent-recent-history-more")
            .w_full()
            .h(px(30.0))
            .px(px(10.0))
            .rounded(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .cursor_pointer()
            .hover(|style| style.bg(hover_bg))
            .child(
                Icon::new(ComponentIconName::ArrowRight)
                    .with_size(px(13.0))
                    .text_color(muted)
                    .into_any_element(),
            )
            .child(
                div()
                    .text_size(theme::FONT_META)
                    .text_color(muted)
                    .child(more_label),
            )
            .on_click(move |_, _, app_cx| {
                let project = project_path.clone();
                more_herdr.update(app_cx, |this, cx| this.open_project_history(project, cx));
            });

        Some(
            v_flex()
                .w_full()
                .max_w(px(720.0))
                .mt(px(24.0))
                .gap(px(2.0))
                .children(rows)
                .child(more_row)
                .into_any_element(),
        )
    }
}
