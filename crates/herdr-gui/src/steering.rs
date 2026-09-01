//! P2-1 steering composer: the "continue the conversation" input row at the bottom of a focused Agent pane.
//!
//! [INPUT]: Depends on gpui-component input's InputState/Input/InputEvent and
//! CompletionProvider mounting, new_agent::reference_provider's reference completion
//! (the Steering source only takes @ files), new_agent::reference_index's build_file_index,
//! shell_input's queue_terminal_text/queue_terminal_key (persistent controller first, RPC fallback
//! without a controller, all off the UI thread), and workspace_model's
//! sidebar_project_path_for_context (pane → project root).
//! [OUTPUT]: Exposes SteeringState (input state + file index cache + focus flags),
//! ensure_steering/schedule_steering_index_scan/steering_input_focused/
//! submit_steering/blur_steering/render_steering_composer, and
//! pane_is_agent/steering_agent_display_name; prune_steering_drafts reclaims drafts of closed
//! panes when the pane universe is authoritatively replaced.
//! [POS]: Native shell surface UI (a standalone module at the same layer as rename/status_bar); submits go through
//! the existing terminal input pipeline with zero new protocol; render placement is called by shell_pane_layout_render's
//! multi-pane leaf and single-pane branches (the single pane appends via terminal_only_view's outer v_flex, with
//! measurement shrinking with the terminal's net area); ensure_steering converges at the top of attach_focused_terminal
//! (the unified moment across all focused-terminal surface activation paths).

use super::*;
use crate::herdr::{Agent, Pane};
use std::collections::HashMap;
use std::sync::Arc;

/// Persistent state for the steering input (created when the first agent pane is focused and reused
/// thereafter; the input persists across focus switches to avoid rebuilding, and visibility is decided
/// by the render layer based on "is the focused pane an agent").
pub(crate) struct SteeringState {
    pub(crate) input: Entity<InputState>,
    /// @ file index cache (skips rebuild while the project_path attribution is fresh; P7-2 accepts the rebuild cost).
    pub(crate) file_index: Option<Arc<crate::new_agent::reference_index::ProjectFileIndex>>,
    _index_scan: Option<BackgroundJob<()>>,
    _subscriptions: Vec<Subscription>,
    /// The input holds GPUI focus (the basis for pausing the terminal's ?1004 focus reporting).
    input_focused: bool,
    /// Placeholder anchor (refreshed when the agent switches).
    agent_name: String,
    /// The focused pane id the draft/input currently belongs to (the anchor for swapping drafts on switch).
    pane_id: String,
    /// The focused tab id the draft/input currently belongs to (the anchor for draft storage by tab:
    /// the focused pane always belongs to the focused tab).
    tab_id: String,
    /// Per-pane drafts (tab_id → pane_id → unsent text): switching panes keeps drafts, submitting
    /// clears this pane's; the tab dimension lets "close whole tab" reclaim via the state.tabs alive
    /// table and "close a pane inside the focused tab" reclaim via pane replacement data (pane
    /// survival in unfocused tabs is unknowable, so they are conservatively kept); this also eradicates
    /// "A's half-typed message accidentally sent to B" (submission re-parses by the currently focused pane).
    drafts: HashMap<String, HashMap<String, String>>,
}

/// Whether the focused pane is an agent pane (Herdr projection: Pane.agent marks the hosted Agent).
pub(crate) fn pane_is_agent(pane: &Pane) -> bool {
    pane.agent.as_deref().is_some_and(|agent| !agent.is_empty())
}

/// Placeholder name: consistent with agent_notification_copy's identity chain
/// (display_agent → agent → name → "Agent").
pub(crate) fn steering_agent_display_name(agents: &[Agent], pane: &Pane) -> String {
    agents
        .iter()
        .find(|agent| agent.pane_id.as_deref() == Some(pane.pane_id.as_str()))
        .and_then(|agent| {
            agent
                .display_agent
                .clone()
                .or_else(|| agent.agent.clone())
                .or_else(|| agent.name.clone())
        })
        .or_else(|| pane.agent.clone())
        .unwrap_or_else(|| "Agent".to_string())
}

impl ShardlaneApp {
    /// Idempotent steering-state initialization (driven by focus events): build the input, attach
    /// reference completion, subscribe to Enter/focus events, refresh the placeholder, and rebuild
    /// the file index on demand.
    pub(crate) fn ensure_steering(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.focused_pane().cloned() else {
            return;
        };
        if !pane_is_agent(&pane) {
            return;
        }
        let agent_name = steering_agent_display_name(&self.state.agents, &pane);
        if self.steering.is_none() {
            let placeholder = format!("Message {agent_name}…");
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            let app_handle = cx.entity().downgrade();
            let source = crate::new_agent::reference_provider::ComposerReferenceSource::Steering;
            input.update(cx, |state, _| {
                state.lsp.completion_provider = Some(
                    crate::new_agent::reference_provider::ComposerReferenceProvider::new(
                        app_handle, source,
                    ),
                );
            });
            let subscriptions = Self::subscribe_steering_events(&input, window, cx);
            self.steering = Some(SteeringState {
                input,
                file_index: None,
                _index_scan: None,
                _subscriptions: subscriptions,
                input_focused: false,
                agent_name: agent_name.clone(),
                pane_id: pane.pane_id.clone(),
                tab_id: self.state.focused_tab_id.clone().unwrap_or_default(),
                drafts: HashMap::new(),
            });
        } else {
            // Drafts follow the pane: when the focused pane changes, swap out the old pane's draft
            // and swap in the new pane's draft (a stale agent name's placeholder refresh is judged
            // separately afterwards).
            let pane_changed = self
                .steering
                .as_ref()
                .is_some_and(|steering| steering.pane_id != pane.pane_id);
            if pane_changed {
                let input = self.steering.as_ref().map(|s| s.input.clone());
                let previous_pane_id = self
                    .steering
                    .as_ref()
                    .map(|s| s.pane_id.clone())
                    .unwrap_or_default();
                // The swapped-out draft is filed under the old pane's tab (the focused pane always
                // belongs to the focused tab, so the tab at save time is the recorded tab_id); the
                // swapped-in draft comes from the new pane's (newly focused) tab.
                let previous_tab_id = self
                    .steering
                    .as_ref()
                    .map(|s| s.tab_id.clone())
                    .unwrap_or_default();
                let current_tab_id = self.state.focused_tab_id.clone().unwrap_or_default();
                let current_text = input
                    .as_ref()
                    .map(|input| input.read(cx).value().to_string())
                    .unwrap_or_default();
                let restored = self
                    .steering
                    .as_mut()
                    .and_then(|steering| {
                        steering
                            .drafts
                            .get_mut(&current_tab_id)
                            .and_then(|tab_drafts| tab_drafts.remove(&pane.pane_id))
                    })
                    .unwrap_or_default();
                if let Some(steering) = self.steering.as_mut() {
                    if !current_text.is_empty() {
                        steering
                            .drafts
                            .entry(previous_tab_id)
                            .or_default()
                            .insert(previous_pane_id, current_text);
                    }
                    steering.pane_id = pane.pane_id.clone();
                    steering.tab_id = current_tab_id;
                }
                if let Some(input) = input {
                    input.update(cx, |state, input_cx| {
                        state.set_value(&restored, window, input_cx);
                    });
                }
            }
            let stale = self
                .steering
                .as_ref()
                .is_some_and(|steering| steering.agent_name != agent_name);
            if stale {
                if let Some(input) = self.steering.as_ref().map(|s| s.input.clone()) {
                    input.update(cx, |state, input_cx| {
                        state.set_placeholder(format!("Message {agent_name}…"), window, input_cx);
                    });
                }
                if let Some(steering) = self.steering.as_mut() {
                    steering.agent_name = agent_name;
                }
            }
        }
        self.schedule_steering_index_scan(cx);
    }

    /// Enter = submit / focus changes sync the flags (subscriptions persist with SteeringState).
    fn subscribe_steering_events(
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        let enter = cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { secondary: false } = event {
                this.submit_steering(window, cx);
            }
        });
        let handle = input.read(cx).focus_handle(cx);
        let focus = cx.on_focus(&handle, window, |this, _window, cx| {
            // Consistent with rename/dialog surfaces: when stealing focus from the terminal, clear the
            // terminal's IME composition state so a frozen preedit overlay doesn't linger on the terminal.
            this.clear_ime_state();
            if let Some(steering) = this.steering.as_mut() {
                steering.input_focused = true;
            }
            this.sync_terminal_application_focus(cx);
            cx.notify();
        });
        let blur = cx.on_blur(&handle, window, |this, _window, cx| {
            if let Some(steering) = this.steering.as_mut() {
                steering.input_focused = false;
            }
            this.sync_terminal_application_focus(cx);
            cx.notify();
        });
        vec![enter, focus, blur]
    }

    /// Whether the steering input holds focus (the gate for terminal ?1004 reporting; doesn't affect
    /// frame projection — the terminal keeps refreshing while the composer is being typed into).
    pub(crate) fn steering_input_focused(&self) -> bool {
        self.steering
            .as_ref()
            .is_some_and(|steering| steering.input_focused)
    }

    /// The focused pane's project root → rebuild the file index in the background (skip when the attribution is unchanged).
    pub(crate) fn schedule_steering_index_scan(&mut self, cx: &mut Context<Self>) {
        let Some(pane) = self.focused_pane().cloned() else {
            return;
        };
        let Some(workspace_id) = pane.workspace_id.clone() else {
            return;
        };
        let Some(project_path) = crate::workspace_model::sidebar_project_path_for_context(
            &self.visible_sidebar_projects(),
            Some(workspace_id.as_str()),
            None,
        ) else {
            return;
        };
        let fresh = self
            .steering
            .as_ref()
            .and_then(|steering| steering.file_index.as_ref())
            .is_some_and(|index| index.project_path == project_path);
        if fresh {
            return;
        }
        let scan = cx.spawn(async move |this, cx| {
            let root = project_path.clone();
            let index = cx
                .background_executor()
                .spawn(async move {
                    crate::new_agent::reference_index::build_file_index(std::path::Path::new(&root))
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if let Some(steering) = view.steering.as_mut() {
                    steering.file_index = Some(Arc::new(index));
                    cx.notify();
                }
            });
        });
        if let Some(steering) = self.steering.as_mut() {
            steering._index_scan = Some(scan);
        }
    }

    /// Submit a steering message: text goes through the persistent controller (RPC fallback when
    /// there's no controller), Enter goes through pane.send_keys named keys; clear after sending
    /// while keeping focus (for follow-up questions).
    pub(crate) fn submit_steering(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(steering) = self.steering.as_ref() else {
            return;
        };
        let message = steering.input.read(cx).value().trim().to_string();
        if message.is_empty() {
            return;
        }
        let input = steering.input.clone();
        let Some(pane) = self.focused_pane().cloned() else {
            return;
        };
        if !pane_is_agent(&pane) {
            return;
        }
        let target = pane
            .terminal_id
            .clone()
            .unwrap_or_else(|| pane.pane_id.clone());
        // Typed text and the named Enter enter the same ordered queue as two entries (same origin
        // as terminal typing: controller first, RPC fallback, failure classification, and
        // order-preserving merging are all reused).
        self.queue_terminal_text(pane.pane_id.clone(), target, message, cx);
        self.queue_terminal_key(pane.pane_id.clone(), "enter".to_string(), cx);
        // Submitting clears this pane's draft (a draft's lifetime ends at send; clearing the input happens after).
        if let Some(steering) = self.steering.as_mut() {
            if let Some(tab_drafts) = steering.drafts.get_mut(&steering.tab_id.clone()) {
                tab_drafts.remove(&pane.pane_id);
            }
        }
        input.update(cx, |state, input_cx| {
            state.set_value("", window, input_cx);
        });
    }

    /// Pane/tab closure reclaims drafts: the tab dimension uses the state.tabs alive table (closing
    /// a whole tab reclaims all of it); the pane dimension of the focused tab uses the current
    /// state.panes (pane replacement data only covers the focused tab, and pane survival in unfocused
    /// tabs is unknowable, so they are conservatively kept). Call after the pane/tab universe is
    /// authoritatively replaced (navigation reconcile / whole-domain Full state / surface application).
    pub(crate) fn prune_steering_drafts(&mut self) {
        let has_drafts = self
            .steering
            .as_ref()
            .is_some_and(|steering| !steering.drafts.is_empty());
        if !has_drafts {
            return;
        }
        let focused_tab_id = self.state.focused_tab_id.clone().unwrap_or_default();
        let live_tabs: std::collections::HashSet<&str> = self
            .state
            .tabs
            .iter()
            .map(|tab| tab.tab_id.as_str())
            .collect();
        let live_panes: std::collections::HashSet<&str> = self
            .state
            .panes
            .iter()
            .map(|pane| pane.pane_id.as_str())
            .collect();
        if let Some(steering) = self.steering.as_mut() {
            steering
                .drafts
                .retain(|tab_id, _| live_tabs.contains(tab_id.as_str()));
            if let Some(tab_drafts) = steering.drafts.get_mut(&focused_tab_id) {
                tab_drafts.retain(|pane_id, _| live_panes.contains(pane_id.as_str()));
            }
        }
    }
}
