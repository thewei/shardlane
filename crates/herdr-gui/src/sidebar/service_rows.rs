//! [INPUT]: Constants, types, and root-level imports from the sidebar module root (`super`); full inheritance via `use super::*`.
//! [OUTPUT]: Provides the Agents section's two-line card rows: live agents (title over project · model · context-pressure meta; insight-derived meta comes only from the live subscribed session, never from transcript parsing) and history backfill rows (chronological fill up to 10 + a fixed "View more history sessions" trailing row). Services cards moved to right_panel/services_view (2026-09-03).
//! [POS]: Agent card rendering layer of `crates/herdr-gui::sidebar`; consumed by shell; mechanically split out of sidebar.rs and sharing the module-root namespace with its sibling submodules.
use super::*;
use crate::ui::menus::menu_action;

fn project_dir_name(agent: &Agent) -> Option<String> {
    agent
        .foreground_cwd
        .as_deref()
        .or(agent.cwd.as_deref())
        .and_then(|cwd| std::path::Path::new(cwd).file_name())
        .and_then(|name| name.to_str())
        .filter(|s| !s.is_empty())
        .map(String::from)
}

pub(super) fn agent_row_label(
    identity: &str,
    has_brand: bool,
    title: Option<&str>,
    project: Option<&str>,
) -> (String, Option<String>) {
    let label = if let Some(t) = title {
        crate::ui_metrics::single_line_label(t)
    } else if has_brand {
        project
            .map(String::from)
            .unwrap_or_else(|| identity.to_string())
    } else {
        match project {
            Some(p) => format!("{identity} · {p}"),
            None => identity.to_string(),
        }
    };
    let meta = if has_brand && title.is_some() {
        project.map(String::from)
    } else {
        None
    };
    (label, meta)
}

impl ShardlaneApp {
    /// History session row in the Sidebar Agents section (notate 2026-08-29):
    /// backfills history sessions chronologically when fewer than 10; clicking
    /// jumps to the matching entry in the History secondary page.
    pub(super) fn sidebar_history_session_row(
        &self,
        session: &ConversationMeta,
        dark: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let lead = agent_brand_icon(session.agent.as_str(), dark)
            .map(RowLead::Brand)
            .unwrap_or(RowLead::Icon("icons/layers.svg"));
        // Card layout (2026-09-03): title on line one; provider kind and
        // last-active time on the muted second line.
        let subtitle: Option<SharedString> = (session.updated_at > 0).then(|| {
            SharedString::from(format!(
                "{} · {}",
                session.agent.as_str(),
                crate::history::history_last_active_short(session.updated_at)
            ))
        });
        let key = session.key.clone();
        let herdr = cx.entity();
        sidebar_card_row(
            &self.sidebar_roving_agents,
            format!("shardlane-agent-history-{key}"),
            lead,
            session.title.clone(),
            subtitle,
            None,
            false,
            move |window, app| {
                herdr.update(app, |this, cx| {
                    this.open_history_session_by_key(key.clone(), window, cx);
                });
            },
            cx,
        )
        .into_any_element()
    }

    /// Trailing row of the Sidebar Agents section (notate 2026-08-29): a fixed
    /// "view more history sessions" row that opens the History secondary surface.
    pub(super) fn sidebar_history_more_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let herdr = cx.entity();
        sidebar_row(
            &self.sidebar_roving_agents,
            "shardlane-agent-history-more",
            RowLead::Icon("icons/layers.svg"),
            "View more history sessions",
            None,
            None,
            None,
            false,
            None,
            false,
            RowLevel::Primary,
            move |window, app| {
                herdr.update(app, |this, cx| {
                    this.open_history_surface_from_sidebar(cx);
                    let _ = window;
                });
            },
            cx,
        )
        .into_any_element()
    }

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
        let has_brand = agent_brand_icon(identity, dark).is_some();
        let project_name = project_dir_name(agent);
        let (label, meta) = agent_row_label(
            identity,
            has_brand,
            agent.title.as_deref(),
            project_name.as_deref(),
        );
        // Card second line: project meta plus, ONLY for the live subscribed
        // session, the insight projection's model and context pressure. Non-
        // subscribed agents never get transcript parsing just for display.
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
        let mut meta_parts: Vec<String> = Vec::new();
        if let Some(meta) = &meta {
            meta_parts.push(meta.clone());
        }
        if let Some(insight) = &live_insight {
            if let Some(model) = insight.model.as_deref() {
                meta_parts.push(model.to_string());
            }
            if let Some(pct) = insight.context_used_percent {
                meta_parts.push(format!("{pct:.0}% ctx"));
            }
        }
        let subtitle = (!meta_parts.is_empty()).then(|| SharedString::from(meta_parts.join(" · ")));
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
    use super::agent_row_label;

    #[test]
    fn brand_agent_with_title_shows_title_and_project_meta() {
        let (label, meta) =
            agent_row_label("Claude Code", true, Some("Fix login bug"), Some("my-app"));
        assert_eq!(label, "Fix login bug");
        assert_eq!(meta.as_deref(), Some("my-app"));
    }

    #[test]
    fn brand_agent_without_title_shows_project_as_label() {
        let (label, meta) = agent_row_label("Claude Code", true, None, Some("my-app"));
        assert_eq!(label, "my-app");
        assert!(meta.is_none());
    }

    #[test]
    fn brand_agent_without_title_or_project_shows_identity() {
        let (label, meta) = agent_row_label("Claude Code", true, None, None);
        assert_eq!(label, "Claude Code");
        assert!(meta.is_none());
    }

    #[test]
    fn non_brand_agent_with_project_shows_identity_dot_project() {
        let (label, meta) = agent_row_label("unknown-cli", false, None, Some("my-app"));
        assert_eq!(label, "unknown-cli · my-app");
        assert!(meta.is_none());
    }

    #[test]
    fn non_brand_agent_without_project_shows_identity() {
        let (label, meta) = agent_row_label("unknown-cli", false, None, None);
        assert_eq!(label, "unknown-cli");
        assert!(meta.is_none());
    }

    #[test]
    fn title_always_wins_regardless_of_brand() {
        let (label, _meta) = agent_row_label(
            "unknown-cli",
            false,
            Some("Implementing API"),
            Some("backend"),
        );
        assert_eq!(label, "Implementing API");
    }

    #[test]
    fn brand_agent_title_without_project_has_no_meta() {
        let (label, meta) = agent_row_label("Codex", true, Some("Review PR"), None);
        assert_eq!(label, "Review PR");
        assert!(meta.is_none());
    }
}
