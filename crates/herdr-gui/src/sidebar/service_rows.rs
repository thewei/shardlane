//! [INPUT]: Constants, types, and root-level imports from the sidebar module root (`super`); full inheritance via `use super::*`.
//! [OUTPUT]: Provides the Agents section's two-line card rows for LIVE agents only (title over project · model · context-pressure meta; insight-derived meta comes only from the live subscribed session, never from transcript parsing); history sessions live in the History surface, not as sidebar backfill (2026-09-03). Services cards moved to right_panel/services_view (2026-09-03).
//! [POS]: Agent card rendering layer of `crates/herdr-gui::sidebar`; consumed by shell; mechanically split out of sidebar.rs and sharing the module-root namespace with its sibling submodules.
use super::*;
use crate::ui::menus::menu_action;

fn agent_project_info(agent: &Agent, workspaces: &[Workspace]) -> (Option<String>, Option<String>) {
    let ws = agent
        .workspace_id
        .as_deref()
        .and_then(|id| workspaces.iter().find(|w| w.workspace_id == id));
    let cwd = agent
        .foreground_cwd
        .clone()
        .or_else(|| agent.cwd.clone())
        .or_else(|| ws.and_then(|w| w.cwd.clone()));
    let name = ws
        .and_then(|w| w.label.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            cwd.as_deref().and_then(|c| {
                std::path::Path::new(c)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .filter(|s| !s.is_empty())
                    .map(String::from)
            })
        });
    (name, cwd)
}

pub(super) fn agent_row_label(identity: &str, title: Option<&str>) -> String {
    if let Some(t) = title {
        crate::ui_metrics::single_line_label(t)
    } else {
        identity.to_string()
    }
}

fn agent_card_meta_element(
    project_name: Option<&str>,
    git_snapshot: Option<&git_status::GitStatusSnapshot>,
    live_insight: Option<&shardlane_host::AgentSessionInsight>,
    theme: &gpui_component::Theme,
) -> Option<AnyElement> {
    let has_project = project_name.is_some();
    let has_git = git_snapshot.is_some();
    let has_insight = live_insight
        .as_ref()
        .is_some_and(|i| i.model.is_some() || i.context_used_percent.is_some());

    if !has_project && !has_git && !has_insight {
        return None;
    }

    let mut row = h_flex().min_w_0().items_center().gap(px(3.0));

    // 1. Project name
    if let Some(project) = project_name {
        row = row.child(
            div()
                .flex_shrink_0()
                .max_w(px(85.0))
                .truncate()
                .text_size(crate::theme::FONT_META)
                .text_color(theme.muted_foreground)
                .child(project.to_string()),
        );
    }

    // 2. Branch name & Git change status
    if let Some(snapshot) = git_snapshot {
        if has_project {
            row = row.child(
                div()
                    .flex_shrink_0()
                    .text_size(crate::theme::FONT_META)
                    .text_color(theme.muted_foreground.opacity(0.5))
                    .child("·"),
            );
        }
        row = row
            .child(
                icon("icons/git-branch.svg")
                    .with_size(px(10.5))
                    .text_color(theme.muted_foreground.opacity(0.7)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .max_w(px(70.0))
                    .text_size(crate::theme::FONT_META)
                    .text_color(theme.muted_foreground.opacity(0.85))
                    .truncate()
                    .child(snapshot.branch.clone()),
            );

        if snapshot.additions > 0 || snapshot.deletions > 0 {
            row = row.child(
                h_flex()
                    .gap(px(2.0))
                    .items_center()
                    .flex_shrink_0()
                    .when(snapshot.additions > 0, |r| {
                        r.child(
                            div()
                                .text_size(crate::theme::FONT_META)
                                .text_color(theme.success)
                                .child(format!("+{}", snapshot.additions)),
                        )
                    })
                    .when(snapshot.deletions > 0, |r| {
                        r.child(
                            div()
                                .text_size(crate::theme::FONT_META)
                                .text_color(theme.danger)
                                .child(format!("-{}", snapshot.deletions)),
                        )
                    }),
            );
        }
    }

    // 3. Live insight (Model & Context Pressure)
    if let Some(insight) = live_insight {
        if let Some(model) = insight.model.as_deref() {
            if has_project || has_git {
                row = row.child(
                    div()
                        .flex_shrink_0()
                        .text_size(crate::theme::FONT_META)
                        .text_color(theme.muted_foreground.opacity(0.5))
                        .child("·"),
                );
            }
            row = row.child(
                div()
                    .flex_shrink_0()
                    .max_w(px(70.0))
                    .truncate()
                    .text_size(crate::theme::FONT_META)
                    .text_color(theme.muted_foreground.opacity(0.7))
                    .child(model.to_string()),
            );
        }
        if let Some(pct) = insight.context_used_percent {
            row = row.child(
                div()
                    .flex_shrink_0()
                    .text_size(crate::theme::FONT_META)
                    .text_color(theme.muted_foreground.opacity(0.7))
                    .child(format!("{pct:.0}% ctx")),
            );
        }
    }

    Some(row.into_any_element())
}

impl ShardlaneApp {
    pub(super) fn sidebar_agent_row(
        &self,
        agent: &Agent,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let workspace_id = agent.workspace_id.clone();
        let tab_id = agent.tab_id.clone();
        let pane_id = agent.pane_id.clone();
        let identity = agent_identity(agent).unwrap_or("Agent");
        let (project_name, project_cwd) = agent_project_info(agent, &self.state.workspaces);
        let label = agent_row_label(identity, agent.title.as_deref());
        let git_snapshot = project_cwd
            .as_deref()
            .and_then(|cwd| self.find_sidebar_git_status(cwd));

        // Card second line: project name, git branch + changes, and ONLY for the
        // live subscribed session, the insight projection's model and context pressure.
        let live_insight = self
            .chat
            .model
            .binding
            .as_ref()
            .and_then(|binding| {
                let session = agent.agent_session.as_ref()?;
                (binding.native_session_id == session.value)
                    .then(|| self.chat.model.insight.clone())
            })
            .flatten();
        let theme = cx.theme();
        let subtitle = agent_card_meta_element(
            project_name.as_deref(),
            git_snapshot,
            live_insight.as_ref(),
            theme,
        );
        let lead = agent_brand_icon(identity, dark)
            .map(RowLead::Brand)
            .unwrap_or(RowLead::Icon("icons/square-terminal.svg"));
        let id = agent
            .pane_id
            .as_deref()
            .unwrap_or(agent.terminal_id.as_str())
            .to_string();
        let terminal_id_for_focus = agent.terminal_id.clone();
        let herdr = cx.entity();
        let deny_herdr = herdr.clone();
        let deny_pane = agent.pane_id.clone();

        let status = agent
            .agent_status
            .as_deref()
            .or(agent.custom_status.as_deref())
            .map(crate::status::attention_for_raw_status);
        // Agent row highlight follows the client-local selection (focused_pane_id);
        // it does not switch on Herdr runtime focus events.
        let active = agent
            .pane_id
            .as_deref()
            .is_some_and(|pane_id| self.state.focused_pane_id.as_deref() == Some(pane_id));

        sidebar_card_row(
            &self.sidebar_roving_agents,
            format!("shardlane-agent-{id}"),
            lead,
            label,
            subtitle,
            status,
            active,
            move |window, app| {
                herdr.update(app, |this, cx| {
                    // FocusIntent::Agent (audit P2-6): terminal_id is the stable primary
                    // identity; a missing pane_id no longer degrades into a lost focus;
                    // when `agent.focus` fails the focus chain automatically falls back
                    // to workspace→tab→pane.
                    let intent = FocusIntent::agent(
                        terminal_id_for_focus.clone(),
                        workspace_id.clone(),
                        tab_id.clone(),
                        pane_id.clone(),
                    );
                    this.apply_focus_intent(intent, window, cx);
                });
            },
            cx,
        )
        .context_menu(move |menu, _, _| {
            let Some(deny_pane) = deny_pane.clone() else {
                return menu;
            };
            menu.item(menu_action(
                "Not an Agent",
                &deny_herdr,
                move |this, window, cx| {
                    this.deny_pane_agent_by_id(deny_pane.clone(), window, cx);
                },
            ))
        })
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{agent_project_info, agent_row_label};
    use shardlane_host::herdr::{Agent, Workspace};

    #[test]
    fn agent_with_title_shows_title() {
        let label = agent_row_label("Claude Code", Some("Fix login bug"));
        assert_eq!(label, "Fix login bug");
    }

    #[test]
    fn agent_without_title_shows_identity() {
        let label = agent_row_label("Claude Code", None);
        assert_eq!(label, "Claude Code");
    }

    #[test]
    fn non_brand_agent_without_title_shows_identity() {
        let label = agent_row_label("unknown-cli", None);
        assert_eq!(label, "unknown-cli");
    }

    #[test]
    fn title_always_wins_over_identity() {
        let label = agent_row_label("unknown-cli", Some("Implementing API"));
        assert_eq!(label, "Implementing API");
    }

    #[test]
    fn project_info_prefers_workspace_label() {
        let agent = Agent {
            terminal_id: "term-1".into(),
            workspace_id: Some("ws-1".into()),
            cwd: Some("/path/to/my-repo".into()),
            ..Default::default()
        };
        let workspaces = vec![Workspace {
            workspace_id: "ws-1".into(),
            label: Some("CustomProject".into()),
            cwd: Some("/path/to/my-repo".into()),
            agent_status: None,
            active_tab_id: None,
            focused: false,
            tab_count: None,
            pane_count: None,
            number: None,
        }];
        let (name, cwd) = agent_project_info(&agent, &workspaces);
        assert_eq!(name.as_deref(), Some("CustomProject"));
        assert_eq!(cwd.as_deref(), Some("/path/to/my-repo"));
    }

    #[test]
    fn project_info_falls_back_to_dir_name() {
        let agent = Agent {
            terminal_id: "term-1".into(),
            workspace_id: None,
            foreground_cwd: Some("/path/to/repo-name".into()),
            ..Default::default()
        };
        let (name, cwd) = agent_project_info(&agent, &[]);
        assert_eq!(name.as_deref(), Some("repo-name"));
        assert_eq!(cwd.as_deref(), Some("/path/to/repo-name"));
    }
}
