//! Plan 060 Phase 1 — Ctrl-Tab Agent Switcher overlay.
//!
//! The switch targets are Herdr's authoritative live Agents (with `terminal_id` as the
//! stable identity). The MRU list holds at most 10 entries, is client-only, and is
//! ordered by access. Hold Ctrl-Tab to cycle; release Ctrl to commit; Esc restores
//! the original selection.
//!
//! [INPUT]: `ShardlaneApp.state.agents` (Herdr live projection)
//! [OUTPUT]: `render_agent_switcher` returns `Option<AnyElement>` (a centered overlay)

use super::*;

const MAX_MRU: usize = 10;

pub(super) struct AgentSwitcherState {
    pub open: bool,
    /// Ordered terminal_id list arranged in the current overlay (snapshotted at open time).
    pub ordered: Vec<String>,
    /// Index of the currently highlighted row in the overlay.
    pub highlighted: Option<usize>,
    /// The terminal_id selected before opening (restored on Esc).
    original: Option<String>,
    /// MRU access sequence (persistent; unaffected by overlay open/close).
    recent: Vec<String>,
    /// The overlay's focus handle (used for keyboard capture).
    pub focus: FocusHandle,
    /// The focus from before the overlay opened (restored on close).
    previous_focus: Option<FocusHandle>,
}

impl AgentSwitcherState {
    pub(super) fn new(focus: FocusHandle) -> Self {
        Self {
            open: false,
            ordered: Vec::new(),
            highlighted: None,
            original: None,
            recent: Vec::new(),
            focus,
            previous_focus: None,
        }
    }

    /// Called each time the user accesses an Agent (records MRU).
    pub(super) fn record_access(&mut self, terminal_id: &str) {
        self.recent.retain(|r| r != terminal_id);
        self.recent.insert(0, terminal_id.to_string());
        self.recent.truncate(MAX_MRU * 2);
    }

    /// Close the overlay and reset transient state, returning the previous focus handle.
    fn dismiss(&mut self) -> Option<FocusHandle> {
        self.open = false;
        self.ordered.clear();
        self.highlighted = None;
        self.original = None;
        self.previous_focus.take()
    }
}

/// Build the overlay ordering from the current live Agent list and MRU records.
fn ordered_agents(
    current: Option<&str>,
    recent: &[String],
    live_terminal_ids: &[String],
) -> Vec<String> {
    let live: std::collections::HashSet<&str> =
        live_terminal_ids.iter().map(|s| s.as_str()).collect();
    let mut seen = std::collections::HashSet::with_capacity(live_terminal_ids.len());
    let mut ordered = Vec::with_capacity(live_terminal_ids.len().min(MAX_MRU));
    let mut push = |id: &str| {
        if ordered.len() < MAX_MRU && live.contains(id) && seen.insert(id.to_string()) {
            ordered.push(id.to_string());
        }
    };
    if let Some(cur) = current {
        push(cur);
    }
    for r in recent {
        push(r.as_str());
    }
    // Append agents that are live but never accessed
    for id in live_terminal_ids {
        push(id.as_str());
    }
    ordered
}

fn initial_highlight(ordered: &[String], current: Option<&str>, reverse: bool) -> Option<usize> {
    if ordered.is_empty() {
        return None;
    }
    if ordered.first().map(|s| s.as_str()) == current {
        if ordered.len() == 1 {
            return Some(0);
        }
        return Some(if reverse { ordered.len() - 1 } else { 1 });
    }
    Some(if reverse { ordered.len() - 1 } else { 0 })
}

impl ShardlaneApp {
    pub(super) fn switch_agent_next(
        &mut self,
        _: &SwitchAgentNext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle_agent_switcher(false, window, cx);
    }

    pub(super) fn switch_agent_prev(
        &mut self,
        _: &SwitchAgentPrev,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle_agent_switcher(true, window, cx);
    }

    pub(super) fn confirm_agent_switch(
        &mut self,
        _: &ConfirmAgentSwitch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.finish_agent_switcher(window, cx);
    }

    pub(super) fn cancel_agent_switch(
        &mut self,
        _: &CancelAgentSwitch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_agent_switcher(window, cx);
    }

    /// Auto-commit when Ctrl is released.
    pub(super) fn agent_switcher_modifiers_changed(
        &mut self,
        event: &gpui::ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.agent_switcher.open && !event.modifiers.control {
            self.finish_agent_switcher(window, cx);
        }
    }

    fn cycle_agent_switcher(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.agent_switcher.open {
            self.open_agent_switcher(reverse, window, cx);
            return;
        }
        let Some(current_index) = self.agent_switcher.highlighted else {
            self.cancel_agent_switcher(window, cx);
            return;
        };
        let len = self.agent_switcher.ordered.len();
        if len == 0 {
            self.cancel_agent_switcher(window, cx);
            return;
        }
        let next = if reverse {
            (current_index + len - 1) % len
        } else {
            (current_index + 1) % len
        };
        self.agent_switcher.highlighted = Some(next);
        cx.notify();
    }

    fn open_agent_switcher(&mut self, reverse: bool, window: &mut Window, cx: &mut Context<Self>) {
        let live_ids: Vec<String> = self
            .state
            .agents
            .iter()
            .map(|a| a.terminal_id.clone())
            .collect();
        if live_ids.is_empty() {
            return;
        }
        let current = self
            .state
            .agents
            .iter()
            .find(|a| a.focused)
            .map(|a| a.terminal_id.clone());
        let ordered = ordered_agents(
            current.as_deref(),
            &self.agent_switcher.recent.clone(),
            &live_ids,
        );
        let Some(highlighted_index) = initial_highlight(&ordered, current.as_deref(), reverse)
        else {
            return;
        };
        self.agent_switcher.open = true;
        self.agent_switcher.original = current;
        self.agent_switcher.ordered = ordered;
        self.agent_switcher.highlighted = Some(highlighted_index);
        self.agent_switcher.previous_focus = window.focused(cx);
        let focus = self.agent_switcher.focus.clone();
        // Focus two frames later so the overlay has joined the dispatch tree
        window.on_next_frame(move |window, _cx| {
            window.on_next_frame(move |window, _cx| {
                window.focus(&focus);
            });
        });
        cx.notify();
    }

    fn finish_agent_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.agent_switcher.open {
            return;
        }
        let highlighted_index = self.agent_switcher.highlighted;
        let selected = highlighted_index.and_then(|i| self.agent_switcher.ordered.get(i).cloned());
        let original = self.agent_switcher.original.clone();
        let previous_focus = self.agent_switcher.dismiss();
        if let Some(ref target) = selected {
            if Some(target) != original.as_ref() {
                let agent = self.state.agents.iter().find(|a| &a.terminal_id == target);
                if let Some(agent) = agent {
                    let intent = FocusIntent::agent(
                        agent.terminal_id.clone(),
                        agent.workspace_id.clone(),
                        agent.tab_id.clone(),
                        agent.pane_id.clone(),
                    );
                    self.apply_focus_intent(intent, window, cx);
                }
            }
        }
        if let Some(pf) = previous_focus {
            window.focus(&pf);
        }
        cx.notify();
    }

    pub(super) fn cancel_agent_switcher(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.agent_switcher.open {
            return;
        }
        if let Some(pf) = self.agent_switcher.dismiss() {
            window.focus(&pf);
        }
        cx.notify();
    }

    /// Called from the main render path: returns the overlay element (None means don't render).
    pub(super) fn render_agent_switcher(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.agent_switcher.open {
            return None;
        }

        let theme = cx.theme().clone();
        let ui_theme = self.theme(window);
        let is_dark = ui_theme.bg <= 0x808080;

        let focus = self.agent_switcher.focus.clone();
        let highlighted_index = self.agent_switcher.highlighted;
        let ordered = self.agent_switcher.ordered.clone();

        let rows: Vec<AnyElement> = ordered
            .iter()
            .enumerate()
            .filter_map(|(i, terminal_id)| {
                let agent = self
                    .state
                    .agents
                    .iter()
                    .find(|a| &a.terminal_id == terminal_id)?;
                let highlighted = highlighted_index == Some(i);
                let title = agent
                    .title
                    .clone()
                    .or_else(|| agent.name.clone())
                    .unwrap_or_else(|| "Agent".to_string());
                let title = crate::ui_metrics::single_line_label(&title);
                let provider_str = agent
                    .agent_session
                    .as_ref()
                    .map(|s| s.agent.clone())
                    .unwrap_or_default();
                let status_text = agent.agent_status.clone().unwrap_or_default();

                let brand_icon = crate::assets::agent_brand_icon(&provider_str, is_dark);
                let icon_path = brand_icon.unwrap_or("icons/terminal.svg");

                let row_bg = if highlighted {
                    theme.list_hover
                } else {
                    gpui::Hsla::transparent_black()
                };

                let idx = i;
                let row_el = div()
                    .id(SharedString::from(format!("agent-sw-row-{i}")))
                    .h(px(44.0))
                    .w_full()
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .rounded(px(8.0))
                    .bg(row_bg)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.list_hover))
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.agent_switcher.highlighted != Some(idx) {
                            this.agent_switcher.highlighted = Some(idx);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.agent_switcher.highlighted = Some(idx);
                        this.finish_agent_switcher(window, cx);
                        cx.stop_propagation();
                    }))
                    .child(
                        Icon::empty()
                            .path(icon_path)
                            .with_size(px(16.0))
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.0))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(if highlighted {
                                        theme.foreground
                                    } else {
                                        theme.secondary_foreground
                                    })
                                    .child(title),
                            )
                            .when(!status_text.is_empty(), |el| {
                                el.child(
                                    div()
                                        .truncate()
                                        .text_size(px(11.0))
                                        .text_color(theme.muted_foreground)
                                        .child(status_text),
                                )
                            }),
                    )
                    .into_any_element();
                Some(row_el)
            })
            .collect();

        if rows.is_empty() {
            return None;
        }

        let overlay_width = px(320.0);
        let card = div()
            .id("agent-switcher-overlay")
            .key_context("AgentSwitcher")
            .track_focus(&focus)
            .w(overlay_width)
            .max_h(px(440.0))
            .p(px(8.0))
            .rounded(px(14.0))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_xl()
            .overflow_y_scroll()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_action(cx.listener(
                |this: &mut ShardlaneApp, _: &ConfirmAgentSwitch, window, cx| {
                    this.finish_agent_switcher(window, cx);
                },
            ))
            .on_action(cx.listener(
                |this: &mut ShardlaneApp, _: &CancelAgentSwitch, window, cx| {
                    this.cancel_agent_switcher(window, cx);
                },
            ))
            .on_action(
                cx.listener(|this: &mut ShardlaneApp, _: &SwitchAgentNext, window, cx| {
                    this.cycle_agent_switcher(false, window, cx);
                }),
            )
            .on_action(
                cx.listener(|this: &mut ShardlaneApp, _: &SwitchAgentPrev, window, cx| {
                    this.cycle_agent_switcher(true, window, cx);
                }),
            )
            .children(rows);

        let layer = div()
            .id("agent-switcher-layer")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.cancel_agent_switcher(window, cx)),
            )
            .child(card);

        Some(gpui::deferred(layer).with_priority(6).into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_agents_respects_mru_and_caps_at_max() {
        let live: Vec<String> = (0..12).map(|i| format!("t{i}")).collect();
        let current = Some("t0");
        let recent: Vec<String> = (1..=8).map(|i| format!("t{i}")).collect();
        let result = ordered_agents(current, &recent, &live);
        assert!(result.len() <= MAX_MRU);
        assert_eq!(result[0], "t0");
        assert_eq!(result[1], "t1");
    }

    #[test]
    fn initial_highlight_skips_current_on_forward() {
        let ordered: Vec<String> = vec!["t0".into(), "t1".into(), "t2".into()];
        assert_eq!(initial_highlight(&ordered, Some("t0"), false), Some(1));
    }

    #[test]
    fn initial_highlight_wraps_backward() {
        let ordered: Vec<String> = vec!["t0".into(), "t1".into(), "t2".into()];
        assert_eq!(initial_highlight(&ordered, Some("t0"), true), Some(2));
    }
}
