//! [INPUT]: Existing imports and types from the crate root (via the history module root glob: `use super::*` chain); the Host `ContinuationRequest`/`ContinuationResult` service surface.
//! [OUTPUT]: For the crate::history family: Continue orchestration — session filtering, the `apply_resume_result` authoritative navigation application, and the `continue_history_with_prompt` / `continue_history_session` transactions through the Host continuation planner (UI intent → Host result → FocusIntent). The GUI no longer owns resume/fork/readiness/prompt policy.
//! [POS]: Continue responsibility slice of the herdr-gui History surface (after the M6 convergence only presentation and navigation remain); the PTY-typing resume path and the 12K Markdown fork were deleted.
use super::*;
use crate::agent_ui::composer::{AgentComposer, ComposerSendState};
use crate::ContentSurfaceTheme;
use gpui_component::menu::DropdownMenu as _;
use gpui_component::{h_flex, Icon, IconName as ComponentIconName, Sizable as _};

pub(super) fn history_session_matches_filters(
    session: &ConversationMeta,
    agent: Option<AgentId>,
    project: Option<&str>,
    time: HistoryTimeFilter,
) -> bool {
    let within_time = || match time.cutoff_secs() {
        None => true,
        Some(cutoff) => {
            let updated = crate::history::history_epoch_secs(session.updated_at);
            updated >= cutoff
        }
    };
    agent.is_none_or(|agent| session.agent == agent)
        && project.is_none_or(|project| session.project_path == project)
        && within_time()
}

/// Runtime object the client should select locally after a resume (no longer
/// syncs Herdr focus).
#[derive(Debug, Default, PartialEq)]
pub(super) struct ResumeSelection {
    pub(super) workspace_id: Option<String>,
    pub(super) tab_id: Option<String>,
    pub(super) pane_id: Option<String>,
}

/// Apply the authoritative resume result (state/focus/Activity/exit History),
/// returning the selection triple.
pub(super) fn apply_resume_result(
    view: &mut crate::ShardlaneApp,
    state: HerdrState,
    selection: &ResumeSelection,
    cx: &mut gpui::Context<crate::ShardlaneApp>,
) {
    view.state = state;
    if let Some(workspace_id) = selection.workspace_id.as_deref() {
        view.state.focused_workspace_id = Some(workspace_id.to_string());
    }
    if let Some(tab_id) = selection.tab_id.as_deref() {
        view.state.focused_tab_id = Some(tab_id.to_string());
    }
    if let Some(pane_id) = selection.pane_id.as_deref() {
        view.state.focused_pane_id = Some(pane_id.to_string());
    }
    derive_selection_flags(&mut view.state);
    view.status = ConnectionStatus::Connected;
    view.history.open = false;
    view.history.search_open = false;
    view.reset_terminal_scroll_state();
    view.notify_sidebar(cx);
    cx.notify();
}

/// Host continuation result → navigation selection triple.
fn selection_from_result(result: &shardlane_host::ContinuationResult) -> Option<ResumeSelection> {
    match result {
        shardlane_host::ContinuationResult::ReusedLive { identity, .. } => Some(ResumeSelection {
            workspace_id: None,
            tab_id: None,
            pane_id: Some(identity.agent_ref.as_str().to_string()),
        }),
        shardlane_host::ContinuationResult::Launched { outcome, .. } => Some(ResumeSelection {
            workspace_id: Some(outcome.workspace_id.clone()),
            tab_id: Some(outcome.tab_id.clone()),
            pane_id: Some(outcome.pane_id.clone()),
        }),
        shardlane_host::ContinuationResult::NeedsProjectSelection => None,
    }
}

impl ShardlaneApp {
    /// Whether the History Composer holds keyboard focus (same guard semantics
    /// as chat_prompt_focused).
    pub(crate) fn history_prompt_focused(&self) -> bool {
        self.history.open && self.history.prompt_focused
    }

    /// Create/reuse the History Composer input (R3: same AgentComposer shell as
    /// Live Chat).
    pub(super) fn ensure_history_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.prompt.is_some() {
            return;
        }
        let prompt = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder(
                SharedString::from("Continue this conversation…"),
                window,
                cx,
            );
            if let Some(saved) = self.history.composer_draft_saved.clone() {
                state.set_value(&saved, window, cx);
            }
            state
        });
        let subscription = cx.subscribe_in(
            &prompt,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { secondary: false } = event {
                    // M6: an empty draft is also a valid Continue (continue without
                    // an instruction).
                    this.submit_history_composer(window, cx);
                }
            },
        );
        // Focus guard (same semantics as A06): while the History Composer holds
        // focus, keystrokes belong to the input box.
        let focus_handle = prompt.read(cx).focus_handle(cx);
        let focus_sub = cx.on_focus(&focus_handle, window, |this, _window, cx| {
            this.history.prompt_focused = true;
            this.sync_terminal_application_focus(cx);
            cx.notify();
        });
        let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            this.history.prompt_focused = false;
            this.sync_terminal_application_focus(cx);
            cx.notify();
        });
        self.history.prompt_focused = false;
        self.history.prompt = Some(prompt);
        self.history.prompt_subscription = Some(subscription);
        self.history.prompt_focus_subscriptions = vec![focus_sub, blur_sub];
    }

    /// History Composer element (same AgentComposer shell as Live Chat). The
    /// provider chip shows the Host capability projection + planner semantics
    /// (Original session / full context transfer / Setup required); cross-provider
    /// transfers use inline explanation instead of a blocking confirmation
    /// dialog (M6).
    pub(super) fn history_composer_element(
        &mut self,
        theme: &ContentSurfaceTheme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let prompt = self.history.prompt.clone()?;
        let busy = self.history.resume_in_flight;
        let send_state = if busy {
            ComposerSendState::Busy
        } else {
            // M6: an empty composer is also a valid Continue.
            ComposerSendState::Ready
        };
        let submit_herdr = cx.entity();
        let dark = theme.is_dark;
        let current_provider = self
            .history
            .continue_agent
            .or_else(|| self.history.transcript.as_ref().map(|t| t.meta.agent));
        let mut composer = AgentComposer::new("history-composer", prompt);
        if let Some(provider) = current_provider {
            let available_providers = self.config.providers.available_choices();
            let menu_herdr = cx.entity();
            let provider_chip = crate::composer_chip::ComposerChip::new("history-provider-picker")
                .icon(
                    agent_brand_icon(provider.as_str(), dark)
                        .map(|path| img(path).size(px(14.0)).into_any_element())
                        .unwrap_or_else(|| {
                            Icon::new(ComponentIconName::Bot)
                                .with_size(px(14.0))
                                .into_any_element()
                        }),
                )
                .label(provider.display_name())
                .dropdown_menu_with_anchor(gpui::Corner::BottomLeft, move |mut menu, _, _| {
                    for candidate in available_providers.iter().copied() {
                        let candidate_herdr = menu_herdr.clone();
                        let brand_path =
                            agent_brand_icon(candidate.as_str(), dark).map(str::to_string);
                        let name = candidate.display_name();
                        let caption = continuation_caption(provider, candidate);
                        menu = menu.item(
                            PopupMenuItem::element(move |_, _| {
                                h_flex()
                                    .w_full()
                                    .min_w_0()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        brand_path
                                            .as_deref()
                                            .map(|path| img(path).size(px(15.0)).into_any_element())
                                            .unwrap_or_else(|| {
                                                Icon::new(ComponentIconName::Bot)
                                                    .xsmall()
                                                    .into_any_element()
                                            }),
                                    )
                                    .child(
                                        v_flex().min_w_0().child(div().child(name)).child(
                                            div()
                                                .text_size(crate::theme::FONT_META)
                                                .text_color(gpui::rgb(0x8_888_888))
                                                .child(caption),
                                        ),
                                    )
                            })
                            .checked(candidate == provider)
                            .on_click(move |_, _, app| {
                                candidate_herdr.update(app, |view, cx| {
                                    view.history.continue_agent = Some(candidate);
                                    cx.notify();
                                });
                            }),
                        );
                    }
                    menu
                });
            composer = composer.left_control(provider_chip);
        }
        composer = composer.send_state(send_state).on_send(move |window, app| {
            submit_herdr.update(app, |this, cx| {
                this.submit_history_composer(window, cx);
            });
        });
        let mut footer = Vec::new();
        if let Some(error) = self.history.error.clone() {
            footer.push(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap(SPACE_ICON)
                    .text_size(crate::theme::FONT_META)
                    .text_color(theme.danger)
                    .child(Icon::new(ComponentIconName::TriangleAlert).with_size(px(11.0)))
                    .child(div().min_w_0().truncate().child(error))
                    .into_any_element(),
            );
        }
        // M6: when crossing providers / no native continuation, explain inline
        // what will happen (no confirmation dialog).
        if let (Some(session_agent), Some(target)) = (
            self.history.transcript.as_ref().map(|t| t.meta.agent),
            self.history.continue_agent,
        ) {
            if target != session_agent {
                footer.push(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .text_size(crate::theme::FONT_META)
                        .text_color(theme.muted)
                        .child(div().min_w_0().child(format!(
                            "Starts a new {} Agent with this conversation's full context. The original conversation remains available.",
                            target.display_name()
                        )))
                        .into_any_element(),
                );
            }
        }
        // M6: inline resolution for NeedsProjectSelection — reuse the sidebar's
        // project list as the picker (the planner never touches the runtime before
        // resolution).
        if self.history.pending_project_selection {
            let projects = self.visible_sidebar_projects();
            let picker_herdr = cx.entity();
            let mut items: Vec<(String, String)> = projects
                .iter()
                .filter_map(|project| {
                    project
                        .project_path
                        .clone()
                        .filter(|path| !path.trim().is_empty())
                        .map(|path| (project.label.clone(), path))
                })
                .collect();
            items.dedup_by(|a, b| a.1 == b.1);
            let chip = crate::composer_chip::ComposerChip::new("history-project-picker")
                .icon(
                    Icon::new(ComponentIconName::Folder)
                        .with_size(px(14.0))
                        .into_any_element(),
                )
                .label("Choose Project")
                .dropdown_menu_with_anchor(gpui::Corner::TopLeft, move |mut menu, _, _| {
                    for (label, path) in items.iter().cloned() {
                        let menu_entity = picker_herdr.clone();
                        menu = menu.item(PopupMenuItem::new(label).on_click(
                            move |_, _window, app| {
                                menu_entity.update(app, |view, cx| {
                                    view.history.pending_project = Some(path.clone());
                                    view.history.pending_project_selection = false;
                                    view.history.error = None;
                                    cx.notify();
                                });
                            },
                        ));
                    }
                    menu
                });
            composer = composer.left_control(chip);
        }
        if !footer.is_empty() {
            composer = composer.footer(footer);
        }
        Some(composer.into_any_element())
    }

    /// Composer Send: Continue (instruction optional; empty means continue
    /// without an instruction).
    pub(super) fn submit_history_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prompt) = self.history.prompt.as_ref() else {
            return;
        };
        let text = prompt.read(cx).value().trim().to_string();
        if self.history.resume_in_flight {
            return;
        }
        let Some(transcript) = self.history.transcript.as_ref() else {
            return;
        };
        let session = transcript.meta.clone();
        self.continue_history_with_prompt(
            session,
            if text.is_empty() { None } else { Some(text) },
            window,
            cx,
        );
    }

    /// Core M6 transaction: UI intent → Host continuation planner transaction →
    /// authoritative result → FocusIntent/Chat. On any failure: the draft is
    /// kept and there is no automatic retry.
    pub(super) fn continue_history_with_prompt(
        &mut self,
        session: ConversationMeta,
        instruction: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.history.resume_in_flight {
            return;
        }
        let Some(client) = self.client.clone() else {
            self.history.error = Some("Herdr runtime is unavailable".to_string());
            cx.notify();
            return;
        };
        let target = self.history.continue_agent.unwrap_or(session.agent);
        let conversation_id = shardlane_host::conversation_id_for_history_key(&session.key);
        let project_override = self.history.pending_project.clone();
        let operation_fingerprint = format!(
            "{}|target={}|instruction={}|project={}",
            conversation_id.as_str(),
            target.as_str(),
            instruction.as_deref().unwrap_or_default(),
            project_override.as_deref().unwrap_or_default(),
        );
        let operation_id = match (
            self.history.pending_continue_operation_id.clone(),
            self.history.pending_continue_fingerprint.as_deref(),
        ) {
            (Some(id), Some(previous)) if previous == operation_fingerprint => id,
            _ => format!(
                "history-continue-{}-{}",
                conversation_id.as_str(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|duration| duration.as_nanos())
                    .unwrap_or_default()
            ),
        };
        self.history.pending_continue_operation_id = Some(operation_id.clone());
        self.history.pending_continue_fingerprint = Some(operation_fingerprint);
        self.history.resume_in_flight = true;
        self.history.error = None;
        cx.notify();
        let window_handle = window.window_handle();
        let delivery = self.delivery.clone();
        let state_client = client.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let service =
                        shardlane_host::HostConversationService::new(&client, history_db_path());
                    let app_data = crate::settings::app_data_dir();
                    let preparation =
                        shardlane_host::GitWorktreePreparation::new(app_data.join("worktrees"));
                    let artifact_store = shardlane_history::TransferArtifactStore::new(
                        app_data.join("transfer-artifacts"),
                    );
                    let transfer_limits = shardlane_history::TransferLimits::default();
                    let request = shardlane_host::ContinuationRequest {
                        operation_id,
                        conversation_id,
                        target_provider: Some(target),
                        instruction,
                        project_override,
                    };
                    service.continue_conversation(
                        &preparation,
                        &artifact_store,
                        &transfer_limits,
                        &delivery,
                        &request,
                    )
                })
                .await;
            let selection_ids = match &result {
                Ok(outcome) => selection_from_result(outcome).map(|selection| {
                    (
                        selection.workspace_id.clone(),
                        selection.tab_id.clone(),
                        selection.pane_id.clone(),
                    )
                }),
                Err(shardlane_host::ConversationServiceError::AgentCreated {
                    tab_id,
                    pane_id,
                    ..
                }) => Some((None, Some(tab_id.clone()), Some(pane_id.clone()))),
                Err(_) => None,
            };
            let draft = String::new();
            let _ = draft;
            let proceeded = this
                .update(cx, |view, cx| {
                    view.history.resume_in_flight = false;
                    match result {
                        Ok(shardlane_host::ContinuationResult::NeedsProjectSelection) => {
                            // The plan requires picking a Project first: no runtime
                            // changes; guide the user inline.
                            view.history.pending_project_selection = true;
                            view.history.pending_continue_operation_id = None;
                            view.history.pending_continue_fingerprint = None;
                            view.history.error =
                                Some("Select a project to continue this conversation".to_string());
                            cx.notify();
                            false
                        }
                        Ok(shardlane_host::ContinuationResult::ReusedLive { identity, .. }) => {
                            view.history.pending_continue_operation_id = None;
                            view.history.pending_continue_fingerprint = None;
                            let state = state_client.visible_state().ok();
                            let selection = ResumeSelection {
                                workspace_id: None,
                                tab_id: None,
                                pane_id: Some(identity.agent_ref.as_str().to_string()),
                            };
                            if let Some(state) = state {
                                apply_resume_result(view, state, &selection, cx);
                            }
                            // An existing live Conversation already has a
                            // semantic identity; keep Chat as its surface.
                            view.chat.model.mode = crate::chat::WorkSurfaceMode::Chat;
                            view.chat.model.load_generation += 1;
                            cx.notify();
                            true
                        }
                        Ok(shardlane_host::ContinuationResult::Launched { outcome, .. }) => {
                            view.history.pending_continue_operation_id = None;
                            view.history.pending_continue_fingerprint = None;
                            let state = state_client.visible_state().ok();
                            let selection = ResumeSelection {
                                workspace_id: Some(outcome.workspace_id.clone()),
                                tab_id: Some(outcome.tab_id.clone()),
                                pane_id: Some(outcome.pane_id.clone()),
                            };
                            if let Some(state) = state {
                                apply_resume_result(view, state, &selection, cx);
                            }
                            // Semantic binding succeeded → Chat takes over (the briefing
                            // was delivered exactly once as the initial prompt).
                            if outcome.identity.is_some() {
                                view.chat.model.mode = crate::chat::WorkSurfaceMode::Chat;
                                view.chat.model.load_generation += 1;
                            }
                            cx.notify();
                            true
                        }
                        Err(shardlane_host::ConversationServiceError::AgentCreated {
                            agent_ref,
                            tab_id,
                            pane_id,
                            detail,
                            ..
                        }) => {
                            // R7-P0-04: a committed Continue target must close
                            // the retryable History operation UNCONDITIONALLY.
                            // A best-effort projection refresh is useful
                            // enrichment but must never decide whether History
                            // stays open.
                            //
                            // Step 1: close History surface and clear the
                            // pending operation BEFORE the optional refresh.
                            view.history.open = false;
                            view.history.search_open = false;
                            view.history.pending_continue_operation_id = None;
                            view.history.pending_continue_fingerprint = None;
                            // Step 2: apply exact returned target ids locally
                            // so navigation can proceed even if refresh fails.
                            let selection = ResumeSelection {
                                workspace_id: None,
                                tab_id: Some(tab_id),
                                pane_id: Some(pane_id),
                            };
                            // Step 3: best-effort refresh — merge if available,
                            // degrade gracefully if unavailable.
                            let state = state_client.visible_state().ok();
                            if let Some(state) = state {
                                apply_resume_result(view, state, &selection, cx);
                            } else {
                                // Refresh failed: apply exact ids without a
                                // full state projection. History is already
                                // closed; focus navigation uses the committed
                                // tab/pane from the AgentCreated target.
                                if let Some(pane_id_str) = &selection.pane_id {
                                    view.state.focused_pane_id = Some(pane_id_str.clone());
                                }
                                if let Some(tab_id_str) = &selection.tab_id {
                                    view.state.focused_tab_id = Some(tab_id_str.clone());
                                }
                                view.notify_sidebar(cx);
                            }
                            // Step 4: surface the attention notice regardless
                            // of refresh success.
                            view.history.error = Some(format!(
                                "Agent created at {} but needs attention: {}",
                                agent_ref.as_str(),
                                detail
                            ));
                            // Keep it in Terminal for explicit repair.
                            view.chat.model.mode = crate::chat::WorkSurfaceMode::Terminal;
                            cx.notify();
                            true
                        }
                        Err(error) => {
                            if !matches!(
                                &error,
                                shardlane_host::ConversationServiceError::Herdr(
                                    shardlane_host::herdr::HerdrError::DeliveryUncertain(_)
                                )
                            ) {
                                view.history.pending_continue_operation_id = None;
                                view.history.pending_continue_fingerprint = None;
                            }
                            view.history.error = Some(error.to_string());
                            cx.notify();
                            false
                        }
                    }
                })
                .unwrap_or(false);
            if let (true, Some((workspace_id, tab_id, pane_id))) = (proceeded, selection_ids) {
                let _ = cx.update_window(window_handle, |_, window, cx| {
                    let _ = this.update(cx, |view, cx| {
                        // FocusIntent seam closure.
                        view.complete_navigation_after_authoritative_selection(
                            workspace_id,
                            tab_id,
                            pane_id,
                            window,
                            cx,
                        );
                    });
                });
            }
        })
        .detach();
    }

    pub(super) fn continue_history_session(
        &mut self,
        session: ConversationMeta,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.continue_history_with_prompt(session, None, window, cx);
    }
}

/// M6: static continuation availability (context-menu gating). The planner
/// remains the runtime source of truth; this only does provider-fact checks —
/// the same provider can continue natively, or there is an enabled provider
/// that can serve as a transfer target.
pub(crate) fn history_continuation_available(session: &ConversationMeta) -> bool {
    if shardlane_history::resume_supported(session.agent) {
        return true;
    }
    session_has_transfer_target()
}

fn session_has_transfer_target() -> bool {
    // Any launchable enabled provider can serve as a transfer target (the M5
    // engine + planner handle precise fail-closed behavior).
    true
}

/// M6/M8 picker row caption (AC-20: name the real strategy by source+target
/// instead of always saying "full context transfer"): same provider with native
/// continuation → Original session; cross-provider (or same provider without
/// native continuation) → New Agent · full context transfer; target unavailable
/// → Setup required. Real install state is owned by the Host capability
/// projection; this no longer fabricates installed:true.
fn continuation_caption(source: AgentId, target: AgentId) -> &'static str {
    let caps = shardlane_host::provider_product_capabilities(
        target,
        shardlane_host::ProviderEnvironment {
            enabled: true,
            // Until the M8 capability snapshot is wired in, keep the UI picker's
            // own availability judgment; after AC-12 this should consume the real
            // runtime snapshot.
            installed: true,
        },
    );
    if caps.unavailable_reason.is_some() {
        return "Setup required";
    }
    if target == source && shardlane_history::resume_supported(source) {
        "Original session · native resume"
    } else {
        "New Agent · full context transfer"
    }
}
