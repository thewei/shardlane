//! Header Agent overview panel: one process-wide directory of live Agents,
//! aggregated across every bound window/instance, surfaced as a Popover around
//! the Header's agent summary chip.
//!
//! The directory is a presentation projection only: every window pushes its own
//! authoritative Herdr agent state into the shared map (plus the client-owned
//! unread/review markers), and nothing here infers runtime lifecycle. Jump
//! routing reuses the window registry (the same owner lookup as notification
//! clicks), so a row for an Agent bound to another window activates there.
//!
//! [INPUT]: ShardlaneApp state (per-window agents + unread/review flags), the
//! shared agent_directory map, gpui-component Popover, sidebar brand icons.
//! [OUTPUT]: AgentDirectoryEntry / AgentDirectoryRow, AgentPanelFilter,
//! sync_agent_directory_into, build_agent_overview_rows, agent_overview_panel
//! (the Popover), and the AgentPanelFilterHolder keyed-state entity.
//! [POS]: A sibling of switcher_panel.rs — same Popover/trigger composition,
//! different data source (process-wide directory instead of the bound
//! instance's projection).
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::*;
use crate::status::AttentionLevel;
use crepuscularity_gpui::{img, Corner, ElementId, Styled};
use gpui_component::popover::Popover;
use gpui_component::Selectable;

/// One process-wide Agent entry. Written by each window's projection sync
/// (notify_status_bar choke point); pane_id is the map key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentDirectoryEntry {
    pub(crate) pane_id: String,
    /// Stable primary identity (Herdr Agent model); survives pane_id churn.
    pub(crate) terminal_id: String,
    pub(crate) identity: String,
    pub(crate) project_name: String,
    /// The binding key of the window that pushed this entry
    /// (ProjectBinding.project_id, e.g. a Herdr session name).
    pub(crate) instance_key: String,
    /// Raw Herdr status ("working"/"idle"/"done"/"blocked"/...). The single
    /// authority for the displayed attention level.
    pub(crate) agent_status: Option<String>,
    /// Client-owned: an attention-worthy transition happened while nobody
    /// was looking at this Agent.
    pub(crate) unread: bool,
    /// Client-owned: the Agent finished and the user has not reviewed yet.
    pub(crate) review_pending: bool,
}

impl AgentDirectoryEntry {
    pub(crate) fn attention(&self) -> AttentionLevel {
        self.agent_status
            .as_deref()
            .map(crate::status::attention_for_raw_status)
            .unwrap_or(AttentionLevel::Idle)
    }

    /// Hook-level "the Agent is waiting for the user" signal
    /// (PermissionRequest / AskUserQuestion arrive as `blocked`).
    pub(crate) fn needs_input(&self) -> bool {
        self.agent_status.as_deref() == Some("blocked")
    }
}

/// One sortable/filterable row snapshot for the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentDirectoryRow {
    pub(crate) entry: AgentDirectoryEntry,
    pub(crate) attention: AttentionLevel,
    pub(crate) brand_icon: Option<&'static str>,
    pub(crate) owns_current_window: bool,
}

/// Panel filter tabs. `Working` is the default so the panel opens on the
/// Agents that are actually doing something right now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AgentPanelFilter {
    #[default]
    Working,
    Attention,
    Review,
    Idle,
    All,
}

impl AgentPanelFilter {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::Attention => "Needs attention",
            Self::Review => "Review",
            Self::Idle => "Idle",
            Self::All => "All",
        }
    }

    pub(crate) fn matches(self, attention: AttentionLevel) -> bool {
        match self {
            Self::Working => attention == AttentionLevel::Working,
            Self::Attention => attention == AttentionLevel::NeedsAttention,
            Self::Review => attention == AttentionLevel::ReadyForReview,
            Self::Idle => attention == AttentionLevel::Idle,
            Self::All => true,
        }
    }

    const TABS: [Self; 5] = [
        Self::Working,
        Self::Attention,
        Self::Review,
        Self::Idle,
        Self::All,
    ];
}

/// Mutable holder for the popover-local filter selection (lives in the
/// popover's keyed state — entities must not be created during
/// ShardlaneApp's own render pass).
pub(crate) struct AgentPanelFilterHolder(pub(crate) AgentPanelFilter);

/// Merge one window's live Agents into the process-wide directory under its
/// instance key. Entries whose pane vanished from this window's projection are
/// removed; other windows' entries are untouched. Pure and testable.
pub(crate) fn sync_agent_directory_into(
    directory: &mut HashMap<String, AgentDirectoryEntry>,
    instance_key: &str,
    project_name: &str,
    agents: &[crate::herdr::Agent],
    unread: &HashSet<String>,
    review_pending: &HashSet<String>,
) {
    let stale: Vec<String> = directory
        .values()
        .filter(|entry| {
            entry.instance_key == instance_key
                && !agents
                    .iter()
                    .any(|agent| agent.pane_id.as_deref() == Some(entry.pane_id.as_str()))
        })
        .map(|entry| entry.pane_id.clone())
        .collect();
    for pane_id in stale {
        directory.remove(&pane_id);
    }
    for agent in agents {
        let Some(pane_id) = agent.pane_id.as_deref() else {
            continue;
        };
        let identity = crate::sidebar::agent_identity(agent).unwrap_or("Agent");
        directory.insert(
            pane_id.to_string(),
            AgentDirectoryEntry {
                pane_id: pane_id.to_string(),
                terminal_id: agent.terminal_id.clone(),
                identity: identity.to_string(),
                project_name: project_name.to_string(),
                instance_key: instance_key.to_string(),
                agent_status: crate::status::agent_effective_status(agent).map(str::to_string),
                unread: unread.contains(pane_id),
                review_pending: review_pending.contains(&agent.terminal_id),
            },
        );
    }
}

/// Sort: actionable first (NeedsAttention, ReadyForReview, Working, Idle),
/// then unread, then identity. Deterministic for tests.
pub(crate) fn row_sort_key(row: &AgentDirectoryRow) -> (u8, bool, String) {
    let attention_rank = match row.attention {
        AttentionLevel::NeedsAttention => 0,
        AttentionLevel::ReadyForReview => 1,
        AttentionLevel::Working => 2,
        AttentionLevel::Idle => 3,
    };
    (
        attention_rank,
        !row.entry.unread,
        row.entry.identity.to_lowercase(),
    )
}

/// Snapshot the process-wide directory into sorted, current-window-aware rows.
pub(crate) fn build_agent_overview_rows(app: &ShardlaneApp, dark: bool) -> Vec<AgentDirectoryRow> {
    let own_instance = app
        .bound_project()
        .map(|binding| binding.project_id.clone());
    let directory = app
        .shared
        .agent_directory
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut rows: Vec<AgentDirectoryRow> = directory
        .values()
        .map(|entry| AgentDirectoryRow {
            attention: entry.attention(),
            brand_icon: crate::assets::agent_brand_icon(&entry.identity, dark),
            owns_current_window: own_instance.as_deref() == Some(entry.instance_key.as_str()),
            entry: entry.clone(),
        })
        .collect();
    rows.sort_by_key(row_sort_key);
    rows
}

/// The Header Agent overview Popover: filter tabs over the directory rows
/// (default Working), each row jumping to its Agent (cross-window rows route
/// through the owning window) with an inline "Mark reviewed" action for
/// review-pending Agents.
pub(crate) fn agent_overview_panel(
    herdr: Entity<ShardlaneApp>,
    rows: Vec<AgentDirectoryRow>,
    anchor: Corner,
    popover_id: &'static str,
    filter_key: &'static str,
    mut trigger: impl Selectable + Styled + IntoElement + 'static,
) -> Popover {
    let trigger_style = trigger.style().clone();
    Popover::new(SharedString::from(popover_id))
        .anchor(anchor)
        .trigger(trigger)
        .trigger_style(trigger_style)
        .content(move |_, window, cx| {
            let popover = cx.entity();
            let filter_holder =
                window.use_keyed_state(SharedString::from(filter_key), cx, |_window, cx| {
                    cx.new(|_| AgentPanelFilterHolder(AgentPanelFilter::Working))
                });
            // use_keyed_state's S is itself an Entity (the same shape the
            // switcher panels use for Entity<InputState>), so unwrap both.
            let filter = filter_holder.read(cx).read(cx).0;
            let component_theme = cx.theme().clone();

            let mut tabs = h_flex()
                .w_full()
                .px(px(8.0))
                .pt(px(6.0))
                .pb(px(4.0))
                .gap(px(4.0));
            for tab in AgentPanelFilter::TABS {
                let holder = filter_holder.clone();
                let selected = tab == filter;
                tabs = tabs.child(
                    div()
                        .id(ElementId::Name(SharedString::from(format!(
                            "agent-filter-{tab:?}"
                        ))))
                        .px(px(7.0))
                        .py(px(2.0))
                        .rounded(px(5.0))
                        .text_size(px(11.0))
                        .cursor_pointer()
                        .when(selected, |s| {
                            s.bg(component_theme.primary.opacity(0.18))
                                .text_color(component_theme.primary)
                        })
                        .when(!selected, |s| {
                            s.text_color(component_theme.muted_foreground).hover(|s| {
                                s.bg(component_theme.foreground.opacity(crate::theme::WASH_HOVER))
                            })
                        })
                        .on_click(move |_, _, app| {
                            holder.update(app, |holder, cx| {
                                holder.update(cx, |holder, cx| {
                                    holder.0 = tab;
                                    cx.notify();
                                });
                            });
                        })
                        .child(tab.label()),
                );
            }

            let matched: Vec<AgentDirectoryRow> = rows
                .iter()
                .filter(|row| filter.matches(row.attention))
                .cloned()
                .collect();
            let mut list = v_flex().w_full().gap(px(1.0));
            if matched.is_empty() {
                list = list.child(
                    div()
                        .w_full()
                        .px(px(10.0))
                        .py(px(14.0))
                        .text_size(px(12.0))
                        .text_color(component_theme.muted_foreground)
                        .child("No agents in this state"),
                );
            }
            for row in matched {
                let entry = row.entry.clone();
                let row_herdr = herdr.clone();
                let row_popover = popover.clone();
                let entry_click = entry.clone();
                let mark_herdr = herdr.clone();
                let mark_popover = popover.clone();
                let mark_key = entry.terminal_id.clone();
                let status_color = row.attention.color(cx);
                let status_label: SharedString = if entry.needs_input() {
                    "Needs input".into()
                } else {
                    row.attention.label().into()
                };
                let row_element = h_flex()
                    .id(ElementId::Name(SharedString::from(format!(
                        "agent-row-{}",
                        entry.pane_id
                    ))))
                    .group("agent-panel-row")
                    .w_full()
                    .h(px(32.0))
                    .px(px(10.0))
                    .rounded(px(6.0))
                    .gap(px(8.0))
                    .items_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(component_theme.foreground.opacity(crate::theme::WASH_HOVER)))
                    .on_click(move |_, window, app| {
                        row_popover.update(app, |state, cx| state.dismiss(window, cx));
                        let entry = entry_click.clone();
                        row_herdr.update(app, |this, cx| {
                            this.jump_to_directory_agent(&entry, window, cx);
                        });
                    })
                    .child(match row.brand_icon {
                        Some(path) => img(path).size(px(14.0)).flex_shrink_0().into_any_element(),
                        None => Icon::empty()
                            .path("icons/square-terminal.svg")
                            .with_size(px(14.0))
                            .text_color(component_theme.muted_foreground)
                            .flex_shrink_0()
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex_shrink()
                            .text_size(px(12.5))
                            .text_color(component_theme.sidebar_foreground)
                            .truncate()
                            .child(entry.identity.clone()),
                    )
                    .when(entry.unread, |r| {
                        r.child(
                            div()
                                .size(px(6.0))
                                .rounded_full()
                                .flex_shrink_0()
                                .bg(component_theme.primary),
                        )
                    })
                    .when(row.entry.review_pending, |r| {
                        r.child(
                            div()
                                .flex_shrink_0()
                                .px(px(4.0))
                                .rounded(px(4.0))
                                .bg(component_theme.success.opacity(0.16))
                                .text_size(px(10.0))
                                .text_color(component_theme.success)
                                .child("Review"),
                        )
                    })
                    .child(div().flex_1().min_w_0())
                    .child(
                        div()
                            .flex_shrink_0()
                            .max_w(px(120.0))
                            .text_size(px(10.5))
                            .text_color(component_theme.muted_foreground)
                            .truncate()
                            .child(row.entry.project_name.clone()),
                    )
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .gap(px(4.0))
                            .items_center()
                            .child(crate::status::status_glyph(
                                ElementId::Name(SharedString::from(format!(
                                    "agent-panel-status-{}",
                                    row.entry.pane_id
                                ))),
                                row.attention,
                                cx,
                            ))
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(status_color)
                                    .child(status_label),
                            ),
                    )
                    .when(row.entry.review_pending, |r| {
                        r.child(
                            div()
                                .id(ElementId::Name(SharedString::from(format!(
                                    "agent-review-{}",
                                    row.entry.pane_id
                                ))))
                                .size(px(18.0))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.0))
                                .opacity(0.0)
                                .group_hover("agent-panel-row", |s| s.opacity(1.0))
                                .hover(|s| {
                                    s.bg(component_theme
                                        .foreground
                                        .opacity(crate::theme::WASH_HOVER))
                                })
                                .on_click(move |_, window, app| {
                                    app.stop_propagation();
                                    mark_popover.update(app, |state, cx| state.dismiss(window, cx));
                                    mark_herdr.update(app, |this, cx| {
                                        this.mark_agent_reviewed(&mark_key, cx);
                                    });
                                })
                                .tooltip(crate::ui::tooltip::tooltip_fn("Mark reviewed"))
                                .child(
                                    Icon::new(ComponentIconName::CircleCheck)
                                        .xsmall()
                                        .text_color(component_theme.success),
                                ),
                        )
                    })
                    .into_any_element();
                list = list.child(row_element);
            }

            v_flex().w(px(340.0)).py(px(4.0)).child(tabs).child(
                div()
                    .id(SharedString::from(format!("{popover_id}-scroll")))
                    .w_full()
                    .max_h(px(380.0))
                    .overflow_y_scroll()
                    .child(list),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(status: Option<&str>, unread: bool, review: bool) -> AgentDirectoryEntry {
        AgentDirectoryEntry {
            pane_id: "p1".into(),
            terminal_id: "t1".into(),
            identity: "claude".into(),
            project_name: "proj".into(),
            instance_key: "session".into(),
            agent_status: status.map(str::to_string),
            unread,
            review_pending: review,
        }
    }

    fn row(status: Option<&str>, unread: bool) -> AgentDirectoryRow {
        let entry = entry(status, unread, false);
        let attention = entry.attention();
        AgentDirectoryRow {
            entry,
            attention,
            brand_icon: None,
            owns_current_window: true,
        }
    }

    #[test]
    fn filter_matches_attention_levels() {
        assert!(AgentPanelFilter::Working.matches(AttentionLevel::Working));
        assert!(!AgentPanelFilter::Working.matches(AttentionLevel::Idle));
        assert!(AgentPanelFilter::Review.matches(AttentionLevel::ReadyForReview));
        assert!(AgentPanelFilter::Attention.matches(AttentionLevel::NeedsAttention));
        assert!(AgentPanelFilter::All.matches(AttentionLevel::Idle));
    }

    #[test]
    fn needs_input_is_the_blocked_raw_status() {
        assert!(entry(Some("blocked"), false, false).needs_input());
        assert!(!entry(Some("working"), false, false).needs_input());
    }

    #[test]
    fn rows_sort_actionable_first_then_unread_then_identity() {
        let mut rows = [
            row(Some("idle"), false),
            row(Some("done"), false),
            row(Some("working"), true),
        ];
        rows.sort_by_key(row_sort_key);
        assert_eq!(rows[0].attention, AttentionLevel::ReadyForReview);
        assert_eq!(rows[1].attention, AttentionLevel::Working);
        assert_eq!(rows[2].attention, AttentionLevel::Idle);
    }

    #[test]
    fn sync_replaces_window_entries_and_prunes_stale_panes() {
        let mut directory: HashMap<String, AgentDirectoryEntry> = HashMap::new();
        let mut agent = crate::herdr::Agent {
            terminal_id: "t1".into(),
            pane_id: Some("p1".into()),
            agent_status: Some("working".into()),
            ..Default::default()
        };
        let unread: HashSet<String> = HashSet::new();
        let review: HashSet<String> = HashSet::new();
        sync_agent_directory_into(
            &mut directory,
            "s1",
            "proj",
            &[agent.clone()],
            &unread,
            &review,
        );
        assert_eq!(directory.len(), 1);
        assert_eq!(directory["p1"].identity, "Agent");

        // The pane vanished: the instance's entry is pruned.
        sync_agent_directory_into(&mut directory, "s1", "proj", &[], &unread, &review);
        assert_eq!(directory.len(), 0);

        // Another instance's entries are never pruned by this window's sync.
        let foreign = entry(Some("idle"), false, false);
        directory.insert("foreign".into(), foreign);
        agent.pane_id = Some("p2".into());
        sync_agent_directory_into(&mut directory, "s1", "proj", &[agent], &unread, &review);
        assert!(directory.contains_key("foreign"));
        assert!(directory.contains_key("p2"));
    }
}
