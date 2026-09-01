//! Header/titlebar presentation layer: Workspace/Tab breadcrumbs, the single bidirectional
//! Chat⇄Terminal switch, the Chat presentation mode's centered Agent identity+status title,
//! the History secondary surface's Search/Refresh/Copy Markdown consolidation (to the left of the
//! right-panel toggle) with compact-layout fallback, and operation indicators.
//!
//! [INPUT]: Depends on `super` (main.rs)'s ShardlaneApp navigation state, OperationalSummary, picker actions, and gpui-component controls
//! [OUTPUT]: Exposes `ShardlaneApp::window_header`
//! [POS]: One of main.rs's presentation-layer splits; the ui.crepus root template calls it via `{self.window_header(...)}`

use super::*;
use ::gpui::img;
use gpui_component::button::ButtonRounded;
use gpui_component::menu::DropdownMenu as _;
use gpui_component::Disableable as _;

#[allow(clippy::too_many_arguments)]
fn title_chip_children(
    agent_label: impl Into<String>,
    agent_slug: impl Into<String>,
    status_text: impl Into<String>,
    status_color: gpui::Hsla,
    dark: bool,
    hud_open: bool,
    context_badge: Option<String>,
    cache_badge: Option<String>,
    theme: &ContentSurfaceTheme,
) -> gpui::AnyElement {
    let agent_label = agent_label.into();
    let agent_slug = agent_slug.into();
    let status_text = status_text.into();
    let muted_color = theme.muted;
    let border_color = theme.border;
    h_flex()
        .min_w_0()
        .gap(px(8.0))
        .items_center()
        .child(
            crate::assets::agent_brand_icon(agent_slug.as_str(), dark)
                .map(|path| img(path).size(px(16.0)).flex_shrink_0().into_any_element())
                .unwrap_or_else(|| {
                    Icon::new(ComponentIconName::Bot)
                        .with_size(px(14.0))
                        .into_any_element()
                }),
        )
        .child(
            div()
                .flex_shrink_0()
                .font_weight(FontWeight::MEDIUM)
                .text_size(crate::theme::FONT_BODY)
                .child(agent_label),
        )
        .when_some(context_badge, |chip, badge| {
            chip.child(
                div()
                    .flex_shrink_0()
                    .px(px(4.0))
                    .py(px(1.0))
                    .rounded(px(4.0))
                    .bg(border_color.opacity(0.4))
                    .text_size(px(10.0))
                    .font_family("monospace")
                    .text_color(muted_color)
                    .child(badge),
            )
        })
        .when_some(cache_badge, |chip, badge| {
            chip.child(
                div()
                    .flex_shrink_0()
                    .px(px(4.0))
                    .py(px(1.0))
                    .rounded(px(4.0))
                    .bg(border_color.opacity(0.4))
                    .text_size(px(10.0))
                    .font_family("monospace")
                    .text_color(muted_color)
                    .child(badge),
            )
        })
        .child(
            div()
                .flex_shrink_0()
                .text_size(crate::theme::FONT_META)
                .text_color(status_color)
                .child(status_text),
        )
        .when(hud_open, |chip| {
            chip.child(
                div()
                    .flex_shrink_0()
                    .text_size(crate::theme::FONT_META)
                    .child("▾"),
            )
        })
        .into_any_element()
}

impl ShardlaneApp {
    pub(super) fn window_header(
        &self,
        _theme: UiTheme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let project_title = self
            .bound_project()
            .map(|binding| binding.project_name.clone())
            .or_else(|| {
                self.active_workspace()
                    .and_then(|workspace| workspace.label.as_deref().or(workspace.cwd.as_deref()))
                    .and_then(|value| {
                        std::path::Path::new(value)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .map(str::to_string)
                            .or(Some(value.to_string()))
                    })
            })
            .unwrap_or_else(|| "Shardlane".to_string());
        let tab_title = self.active_tab().map(|tab| self.tab_title(tab));
        let secondary_header_title = if self.history.open {
            Some(("History", None))
        } else if self.show_settings {
            Some(("Settings", Some(self.settings_section.label())))
        } else if self.new_agent_open {
            Some(("New Task", None))
        } else {
            None
        };
        let connected = self.status.is_connected();
        let compact_header = sidebar_should_auto_collapse(window.bounds().size.width.to_f64());
        let sidebar_auto_collapsed = self.sidebar_auto_collapsed;
        let secondary_surface =
            is_secondary_surface(self.show_settings, self.history.open, self.new_agent_open);
        let sidebar_visible = self.shell_sidebar_visible();
        // This frame's rendered width (follows the slide animation; same semantics as a sidebar_rendered_width).
        let sidebar_width = self.sidebar_rendered_width as f32;
        let summary = operational_summary(&self.state.agents, &self.scripts);
        // M8: jump targets are chosen by sorting on shared typed attention (NeedsAttention before
        // Working); no second raw-string comparison is maintained.
        let mut attention_targets = self.state.agents.iter().filter_map(|agent| {
            let attention = crate::status::agent_effective_status(agent)
                .map(crate::status::attention_for_raw_status)
                .unwrap_or(crate::status::AttentionLevel::Idle);
            matches!(
                attention,
                crate::status::AttentionLevel::NeedsAttention
                    | crate::status::AttentionLevel::Working
            )
            .then(|| {
                (
                    attention,
                    (
                        agent.workspace_id.clone(),
                        agent.tab_id.clone(),
                        agent.pane_id.clone(),
                    ),
                )
            })
        });
        let needs_attention_target = attention_targets
            .find(|(attention, _)| *attention == crate::status::AttentionLevel::NeedsAttention);
        let summary_agent_target = needs_attention_target
            .or_else(|| attention_targets.next())
            .map(|(_, target)| target);
        let summary_script_id = self
            .scripts
            .scripts
            .iter()
            .find(|script| script.runtime.status == ScriptStatus::Failed)
            .or_else(|| {
                self.scripts.scripts.iter().find(|script| {
                    matches!(
                        script.runtime.status,
                        ScriptStatus::Starting | ScriptStatus::Running
                    )
                })
            })
            .map(|script| script.id.clone());
        let active_project_scripts = self.active_project_scripts();
        let last_run_script = active_project_scripts
            .iter()
            .find(|script| script.last_run_at_ms.is_some())
            .cloned();
        let script_launcher_label = last_run_script
            .as_ref()
            .map(|script| script.name.clone())
            .unwrap_or_else(|| "New Script".to_string());
        let script_launcher_script_id = last_run_script.map(|script| script.id.clone());
        let herdr = cx.entity();
        let sidebar_herdr = herdr.clone();
        let nav_back_herdr = herdr.clone();
        let nav_forward_herdr = herdr.clone();
        let can_nav_back = !self.nav_back_stack.is_empty();
        let can_nav_forward = !self.nav_forward_stack.is_empty();
        let search_header_herdr = herdr.clone();
        let workspace_picker_herdr = herdr.clone();
        let tab_picker_herdr = herdr.clone();
        let agent_summary_herdr = herdr.clone();
        let script_summary_herdr = herdr.clone();
        let reconnect_header_herdr = herdr.clone();
        let history_header_herdr = herdr.clone();
        let history_search_herdr = herdr.clone();
        let history_refresh_herdr = herdr.clone();
        let chat_header_herdr = herdr.clone();
        let script_launcher_herdr = herdr.clone();
        let script_menu_herdr = herdr.clone();
        let script_add_herdr = herdr.clone();
        let right_panel_herdr = herdr.clone();

        // The git snapshot refresh must happen before the cx.theme() borrow (&mut cx and the immutable borrow are mutually exclusive).
        if !secondary_surface {
            self.refresh_git_status(cx);
        }
        let component_theme = cx.theme();
        let content_theme = self.content_surface_theme(window);
        let foreground = content_theme.foreground;
        let muted = content_theme.muted;
        // Overlay semantics: the tint's base color is the foreground, not the text color (layering over an already-transparent color would wash it out twice).
        let toggle_hover = foreground.opacity(0.05);
        let toggle_active = foreground.opacity(crate::theme::WASH_ACTIVE);
        // The Header background matches its page exactly: the History page and the terminal surface
        // both use the terminal palette (the History root background is content_theme.background);
        // the other secondary surfaces (Settings/New Agent) use the app background.
        let terminal_header_bg = if secondary_surface && !self.history.open {
            cx.theme().background
        } else {
            content_theme.background
        };
        let terminal_header_muted = content_theme.muted;
        let terminal_header_button = content_theme.button_variant(cx);
        let terminal_header_danger = content_theme.danger;
        // Audit A29: this palette derives from the Herdr theme's limited token set, which has no
        // separate warning color — the misleading `terminal_header_warning = danger` alias was
        // deleted. "Needs *" and hard failures intentionally share the danger color.
        let terminal_header_success = content_theme.success;
        let terminal_header_activity = content_theme.primary;
        let git_status = if secondary_surface {
            None
        } else {
            self.git_status.clone()
        };
        let project_path_for_info = if secondary_surface {
            None
        } else {
            self.active_workspace()
                .and_then(|workspace| workspace.cwd.clone())
        };
        // When the sidebar is visible, a spacer pushes the title exactly to the content area's left
        // margin (CONTENT_INSET): the TitleBar component carries macOS TITLE_BAR_LEFT_PADDING
        // (mirrored as APP_TITLEBAR_LEFT_INSET=80), the header's left segment adds collapse
        // button(26) + root gap(12), and the title itself has pl(14). Under-counting by 80px would
        // push the title 80px into the content area (measured over three rounds on 08-29: title at
        // 390, target 309). When collapsed, spacer=0.
        // +/- pill: appears only when there are changes; clicking reveals the working directory in Finder.
        let git_pill = git_status
            .as_ref()
            .filter(|snapshot| snapshot.additions > 0 || snapshot.deletions > 0)
            .map(|snapshot| {
                (
                    snapshot.additions,
                    snapshot.deletions,
                    snapshot.files_changed,
                    snapshot.path.clone(),
                )
            });
        // Info popover (EnvironmentPopover semantics): project status + quick actions.
        let info_button = project_path_for_info.clone().map(|path| {
            let snapshot = git_status.clone();
            let working = summary.working_agents;
            let blocked = summary.blocked_agents;
            DropdownButton::new("titlebar-project-info")
                .ghost()
                .xsmall()
                .button(
                    Button::new("titlebar-project-info-button")
                        .custom(terminal_header_button)
                        .xsmall()
                        .icon(Icon::new(ComponentIconName::Info).xsmall())
                        .tooltip("Project status"),
                )
                .dropdown_menu(move |mut menu, _, _| {
                    let branch_line = snapshot
                        .as_ref()
                        .map(|snapshot| {
                            let mut line = format!("⑂ {}", snapshot.branch);
                            let mut remote = Vec::new();
                            if snapshot.ahead > 0 {
                                remote.push(format!("↑{}", snapshot.ahead));
                            }
                            if snapshot.behind > 0 {
                                remote.push(format!("↓{}", snapshot.behind));
                            }
                            if !remote.is_empty() {
                                line.push_str(&format!(" {}", remote.join(" ")));
                            }
                            line
                        })
                        .unwrap_or_else(|| "⑂ not a git repository".to_string());
                    let changes_line = snapshot
                        .as_ref()
                        .map(|snapshot| {
                            format!(
                                "{} files changed (+{} −{})",
                                snapshot.files_changed, snapshot.additions, snapshot.deletions
                            )
                        })
                        .unwrap_or_else(|| "No git status".to_string());
                    let agents_line = if blocked > 0 {
                        format!("{working} working · {blocked} blocked")
                    } else {
                        format!("{working} agents working")
                    };
                    // Status lines (no action items, display only); clicking a PopupMenuItem without disabled has no side effect.
                    menu = menu
                        .item(PopupMenuItem::new(branch_line))
                        .item(PopupMenuItem::new(changes_line))
                        .item(PopupMenuItem::new(agents_line))
                        .separator();
                    let copy_path = path.clone();
                    menu = menu.item(PopupMenuItem::new("Copy Project Path").on_click(
                        move |_, _, app| {
                            app.write_to_clipboard(crepuscularity_gpui::ClipboardItem::new_string(
                                copy_path.clone(),
                            ));
                        },
                    ));
                    let reveal_path = path.clone();
                    menu = menu.item(PopupMenuItem::new("Reveal in Finder").on_click(
                        move |_, _, _| {
                            let _ = std::process::Command::new("open")
                                .arg("-R")
                                .arg(&reveal_path)
                                .spawn();
                        },
                    ));
                    let copy_status = snapshot.clone();
                    menu.item(
                        PopupMenuItem::new("Copy Git Status").on_click(move |_, _, app| {
                            let text = copy_status
                                .as_ref()
                                .map(|snapshot| {
                                    format!(
                                        "{} @ {} · {} files changed (+{} −{})",
                                        snapshot.branch,
                                        snapshot.path,
                                        snapshot.files_changed,
                                        snapshot.additions,
                                        snapshot.deletions
                                    )
                                })
                                .unwrap_or_else(|| "No git status".to_string());
                            app.write_to_clipboard(crepuscularity_gpui::ClipboardItem::new_string(
                                text,
                            ));
                        }),
                    )
                })
        });

        let agent_indicator = if secondary_surface {
            None
        } else if summary.blocked_agents > 0 {
            Some((
                terminal_header_danger,
                format!("{} blocked", summary.blocked_agents),
            ))
        } else if !compact_header && !sidebar_visible && summary.working_agents > 0 {
            Some((
                terminal_header_activity,
                format!("{} working", summary.working_agents),
            ))
        } else {
            None
        };
        let script_indicator = if secondary_surface {
            None
        } else if summary.failed_scripts > 0 {
            Some((
                terminal_header_danger,
                format!("{} failed", summary.failed_scripts),
            ))
        } else if !compact_header && !sidebar_visible && summary.active_scripts > 0 {
            Some((
                terminal_header_success,
                format!("{} active", summary.active_scripts),
            ))
        } else {
            None
        };
        let border_color = foreground.opacity(0.12);
        let script_launcher = {
            let primary_herdr = script_launcher_herdr.clone();
            let primary_script_id = script_launcher_script_id.clone();
            let menu_scripts = active_project_scripts.clone();
            let menu_herdr = script_menu_herdr.clone();
            let add_herdr = script_add_herdr.clone();
            let current_primary_script_id = primary_script_id.clone();

            let primary_click_herdr = primary_herdr.clone();
            let primary_click_script_id = primary_script_id.clone();

            let primary = Button::new("titlebar-script-primary")
                .custom(terminal_header_button)
                .xsmall()
                .rounded(ButtonRounded::None)
                .label(script_launcher_label.clone())
                .tooltip(match primary_script_id.as_ref() {
                    Some(_) => format!("Run {}", script_launcher_label),
                    None => "New Script".to_string(),
                })
                .on_click(move |_, window, app| {
                    if let Some(script_id) = primary_click_script_id.clone() {
                        primary_click_herdr
                            .update(app, |this, cx| this.run_script_id(script_id, window, cx));
                    } else {
                        let add_herdr = primary_click_herdr.clone();
                        window.defer(app, move |window, app| {
                            add_herdr
                                .update(app, |this, cx| this.open_new_script_dialog(window, cx));
                        });
                    }
                });

            let caret_button = Button::new("titlebar-script-caret")
                .custom(terminal_header_button)
                .xsmall()
                .rounded(ButtonRounded::None)
                // Caret segment: a fixed narrow strip (w18) instead of shrinking with content.
                .w(px(18.0))
                .h_full()
                .icon(
                    Icon::new(ComponentIconName::ChevronDown)
                        .with_size(px(11.0))
                        .text_color(muted),
                )
                .tooltip("Scripts")
                .dropdown_menu_with_anchor(gpui::Corner::TopRight, move |mut menu, _, _| {
                    let scripts = menu_scripts.clone();
                    let has_scripts = !scripts.is_empty();
                    for script in scripts {
                        let script_id = script.id.clone();
                        let run_herdr = menu_herdr.clone();
                        let icon_name = scripts::script_icon_name(&script.icon);
                        let label = script
                            .keybinding
                            .as_deref()
                            .map(|keybinding| format!("{}    {}", script.name, keybinding))
                            .unwrap_or_else(|| script.name.clone());
                        let is_primary = current_primary_script_id.as_ref() == Some(&script.id);
                        menu = menu.item(
                            PopupMenuItem::new(label)
                                .icon(Icon::new(icon_name).xsmall())
                                .checked(is_primary)
                                .on_click(move |_, window, app| {
                                    run_herdr.update(app, |this, cx| {
                                        this.run_script_id(script_id.clone(), window, cx)
                                    });
                                }),
                        );
                    }
                    if has_scripts {
                        menu = menu.item(PopupMenuItem::separator());
                    }
                    let add_herdr = add_herdr.clone();
                    menu.item(
                        PopupMenuItem::new("New Workflow…")
                            .icon(Icon::new(ComponentIconName::Plus).xsmall())
                            .on_click(move |_, window, app| {
                                let add_herdr = add_herdr.clone();
                                window.defer(app, move |window, app| {
                                    add_herdr.update(app, |this, cx| {
                                        this.open_new_script_dialog(window, cx)
                                    });
                                });
                            }),
                    )
                });

            // Open-in split button (background_work.rs): h28/r7/border_strong,
            // primary shows text only (user-specified: no icon), caret fixed at w18 + 11px chevron;
            // hover 5% / active 9% neutral wash, menu BelowRight, item = Scripts.
            div()
                .id("titlebar-script-launcher-group")
                .h(px(26.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(foreground.opacity(0.16))
                .overflow_hidden()
                .flex_none()
                .flex()
                .items_center()
                .child(primary)
                .child(div().w(px(1.0)).h_full().flex_none().bg(border_color))
                .child(caret_button)
                .into_any_element()
        };

        // In Chat presentation mode (WorkSurfaceMode::Chat), the Agent identity + authoritative status
        // is shown centered in the Header as the title (notate 2026-08-29: the Pi Idle chip moved into the Header).
        // AnyElement isn't cloneable: only data is carried here; the element is built inside the when_some closure.
        let chat_title = if self.chat.model.mode == crate::chat::WorkSurfaceMode::Chat {
            let dark = content_theme.is_dark;
            let hud_open = self.chat.hud_open;
            let has_insight = self.chat.model.insight.is_some();
            let (agent_label, agent_slug) = self
                .chat
                .model
                .binding
                .as_ref()
                .map(|binding| (binding.agent.display_name(), binding.agent.as_str()))
                .unwrap_or(("Agent", "claude-code"));
            let pending_interaction = self.chat.active_interactions.iter().find(|i| {
                i.state == shardlane_host::conversation_interactions::ConversationInteractionState::Pending
            });
            let (status_text, status_color) = if let Some(pending) = pending_interaction {
                match pending.kind {
                    shardlane_host::conversation_interactions::ConversationInteractionKind::Question => {
                        ("Needs Input", terminal_header_danger)
                    }
                    shardlane_host::conversation_interactions::ConversationInteractionKind::Permission
                    | shardlane_host::conversation_interactions::ConversationInteractionKind::PlanApproval => {
                        ("Needs Approval", terminal_header_danger)
                    }
                    _ => ("Needs Attention", terminal_header_danger),
                }
            } else {
                // Audit A20: route through the single runtime→product status mapping and its
                // live label source (AttentionLevel::label) — this was the third independent
                // hand-written status-string table (besides the status bar and the authoritative
                // attention_for_raw_status).
                let status = self.chat.model.herdr_status.clone().unwrap_or_default();
                let attention = crate::status::attention_for_raw_status(&status);
                let text = attention.label();
                // The level→color arm stays local: this surface paints with the Herdr-derived
                // content palette, while AttentionLevel::color reads the gpui-component theme.
                let color = match attention {
                    crate::status::AttentionLevel::NeedsAttention => terminal_header_danger,
                    crate::status::AttentionLevel::Working => terminal_header_success,
                    crate::status::AttentionLevel::ReadyForReview => terminal_header_activity,
                    crate::status::AttentionLevel::Idle => terminal_header_muted,
                };
                (text, color)
            };
            let context_badge = self
                .chat
                .model
                .insight
                .as_ref()
                .and_then(|i| i.context_used_percent.map(|p| format!("ctx {p:.0}%")));
            let cache_badge = self
                .chat
                .model
                .insight
                .as_ref()
                .and_then(|i| i.cache_hit_percent.map(|p| format!("cache {p:.1}%")));

            Some((
                agent_label.to_string(),
                agent_slug.to_string(),
                status_text,
                status_color,
                dark,
                hud_open,
                has_insight,
                context_badge,
                cache_badge,
            ))
        } else {
            None
        };

        let header = {
            div()
                .relative()
                .w_full()
                .h_full()
                .pr(px(14.0))
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    // The header content segment's background is always painted: starting from the
                    // sidebar's right edge when the sidebar is visible (the traffic lights/sidebar
                    // segment keep the titlebar transparent), or from the traffic lights' right edge
                    // when collapsed — otherwise the root background and page background would form a
                    // horizontal seam at the titlebar's bottom.
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(px(if sidebar_visible {
                            (sidebar_width - APP_TITLEBAR_LEFT_INSET).max(0.0)
                        } else {
                            0.0
                        }))
                        .right_0()
                        .bg(terminal_header_bg),
                )
                .child(
                    // sidebar toggle + navigation back/forward buttons, arranged horizontally
                    h_flex()
                        .flex_none()
                        .items_center()
                        .gap(px(2.0))
                        // sidebar toggle
                        .child(
                            // Sidebar toggle: right of the traffic lights, 26×26 r6 panel-left 14px
                            // tertiary; hover/active = foreground tint (overlay semantics).
                            // Present on all pages; goes through the toggle_sidebar action (persist + notify).
                            div()
                                .id("titlebar-sidebar-toggle")
                                .w(px(26.0))
                                .h(px(26.0))
                                .flex_none()
                                .rounded(px(6.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_default()
                                .hover(move |style| style.bg(toggle_hover))
                                .active(move |style| style.bg(toggle_active))
                                .child(
                                    // panel-left is the single icon used in its resting state (no open variant).
                                    Icon::empty()
                                        .path("icons/panel-left.svg")
                                        .with_size(px(14.0))
                                        .text_color(muted),
                                )
                                .on_click({
                                    let toggle_herdr = sidebar_herdr.clone();
                                    move |_, window, app| {
                                        toggle_herdr.update(app, |this, cx| {
                                            this.toggle_sidebar(&ToggleSidebar, window, cx)
                                        });
                                    }
                                }),
                        )
                        // Back button
                        .child({
                            let nav_color = if can_nav_back { muted } else { muted.opacity(0.35) };
                            div()
                                .id("titlebar-nav-back")
                                .w(px(22.0))
                                .h(px(22.0))
                                .flex_none()
                                .rounded(px(5.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_default()
                                .when(can_nav_back, |el| {
                                    el.hover(move |style| style.bg(toggle_hover))
                                        .active(move |style| style.bg(toggle_active))
                                        .on_click({
                                            let back_herdr = nav_back_herdr.clone();
                                            move |_, window, app| {
                                                back_herdr.update(app, |this, cx| {
                                                    this.navigate_back(&NavigateBack, window, cx)
                                                });
                                            }
                                        })
                                })
                                .tooltip(crate::ui::tooltip::tooltip_fn("Go Back"))
                                .child(
                                    Icon::new(ComponentIconName::ChevronLeft)
                                        .with_size(px(13.0))
                                        .text_color(nav_color),
                                )
                        })
                        // Forward button
                        .child({
                            let fwd_color =
                                if can_nav_forward { muted } else { muted.opacity(0.35) };
                            div()
                                .id("titlebar-nav-forward")
                                .w(px(22.0))
                                .h(px(22.0))
                                .flex_none()
                                .rounded(px(5.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_default()
                                .when(can_nav_forward, |el| {
                                    el.hover(move |style| style.bg(toggle_hover))
                                        .active(move |style| style.bg(toggle_active))
                                        .on_click({
                                            let fwd_herdr = nav_forward_herdr.clone();
                                            move |_, window, app| {
                                                fwd_herdr.update(app, |this, cx| {
                                                    this.navigate_forward(
                                                        &NavigateForward,
                                                        window,
                                                        cx,
                                                    )
                                                });
                                            }
                                        })
                                })
                                .tooltip(crate::ui::tooltip::tooltip_fn("Go Forward"))
                                .child(
                                    Icon::new(ComponentIconName::ChevronRight)
                                        .with_size(px(13.0))
                                        .text_color(fwd_color),
                                )
                        }),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(theme::FONT_BODY)
                        .text_color(foreground)
                        .when_some(chat_title.clone(), |row, chip| {
                            // Chat presentation mode: the Agent identity+status is the centered Header title
                            //(breadcrumbs yield; notate 2026-08-29). M8: the title is clickable, opening the
                            // conversation insight HUD (data from the Host insight projection).
                            let (
                                agent_label,
                                agent_slug,
                                status_text,
                                status_color,
                                dark,
                                hud_open,
                                has_insight,
                                context_badge,
                                cache_badge,
                            ) = chip;
                            let title_entity = chat_header_herdr.clone();
                            let interactive_title = has_insight;
                            let chip: gpui::AnyElement = if interactive_title {
                                h_flex()
                                    .min_w_0()
                                    .gap(px(8.0))
                                    .items_center()
                                    .id("chat-title-hud")
                                    .hover(|hover| hover.bg(gpui::transparent_white()))
                                    .cursor_pointer()
                                    .on_click(move |_, _window, app| {
                                        title_entity.update(app, |this, cx| {
                                            this.chat.hud_open = !this.chat.hud_open;
                                            cx.notify();
                                        });
                                    })
                                    .child(title_chip_children(
                                        agent_label,
                                        agent_slug,
                                        status_text,
                                        status_color,
                                        dark,
                                        hud_open,
                                        context_badge,
                                        cache_badge,
                                        &content_theme,
                                    ))
                                    .into_any_element()
                            } else {
                                h_flex()
                                    .min_w_0()
                                    .gap(px(8.0))
                                    .items_center()
                                    .child(title_chip_children(
                                        agent_label,
                                        agent_slug,
                                        status_text,
                                        status_color,
                                        dark,
                                        hud_open,
                                        context_badge,
                                        cache_badge,
                                        &content_theme,
                                    ))
                                    .into_any_element()
                            };
                            row.child(div().flex_1().min_w_0())
                                .child(chip)
                                .child(div().flex_1().min_w_0())
                        })
                        .when(chat_title.is_none(), |row| {
                            row.when_some(secondary_header_title, |row, (title, section)| {
                                row.child(
                                    h_flex()
                                        .min_w_0()
                                        .pl(px(14.0))
                                        .gap_1()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(title)
                                        .when_some(section, |label, section| {
                                            label.child(
                                                div().text_color(terminal_header_muted).child("/"),
                                            )
                                            .child(
                                                div()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_color(terminal_header_muted)
                                                    .child(section),
                                            )
                                        }),
                                )
                            })
                            .when(!secondary_surface, |row| {
                                // Breadcrumb levels are switcher triggers: the
                                // workspace level opens the SAME panel as the
                                // sidebar footer switcher; the Tab level opens
                                // the matching Tab panel.
                                row.child({
                                    let machines =
                                        crate::switcher_panel::build_picker_machines(self);
                                    let selected_device =
                                        crate::switcher_panel::selected_panel_device(self);
                                    let project_button = Button::new("titlebar-project-picker")
                                        .custom(terminal_header_button)
                                        .xsmall()
                                        .min_w_0()
                                        .max_w(relative(0.42))
                                        .flex_shrink()
                                        .overflow_hidden()
                                        .child(div().min_w_0().truncate().child(project_title.clone()))
                                        .tooltip(format!("Switch workspace · {project_title}"));
                                    crate::switcher_panel::workspace_switcher_panel(
                                        workspace_picker_herdr.clone(),
                                        machines,
                                        selected_device,
                                        gpui::Corner::TopLeft,
                                        "shardlane-header-workspace-popover",
                                        "shardlane-header-ws-filter",
                                        project_button,
                                    )
                                })
                                .when_some(tab_title, |el, title| {
                                    el.child(div().text_color(terminal_header_muted).child("/"))
                                        .child({
                                            let tabs: Vec<crate::switcher_panel::TabRow> = self
                                                .active_workspace_id()
                                                .map(|workspace_id| {
                                                    self.tabs_for_workspace(workspace_id)
                                                        .into_iter()
                                                        .map(|tab| {
                                                            let focused = self
                                                                .state
                                                                .focused_tab_id
                                                                .as_deref()
                                                                == Some(tab.tab_id.as_str());
                                                            (
                                                                tab.tab_id.clone(),
                                                                self.tab_title(&tab),
                                                                focused,
                                                            )
                                                        })
                                                        .collect()
                                                })
                                                .unwrap_or_default();
                                            let tab_button = Button::new("titlebar-tab-picker")
                                                .custom(terminal_header_button)
                                                .xsmall()
                                                .min_w_0()
                                                .max_w(relative(0.42))
                                                .flex_shrink()
                                                .overflow_hidden()
                                                .text_color(terminal_header_muted)
                                                .child(div().min_w_0().truncate().child(title.clone()))
                                                .tooltip(format!("Switch Tab · {title}"));
                                            crate::switcher_panel::tab_switcher_panel(
                                                tab_picker_herdr.clone(),
                                                tabs,
                                                gpui::Corner::TopLeft,
                                                "shardlane-header-tab-popover",
                                                "shardlane-header-tab-filter",
                                                tab_button,
                                            )
                                        })
                                })
                            })
                            .child(div().flex_1().min_w_0())
                        })
                        .when_some(agent_indicator, |row, (color, label)| {
                            row.child(
                                Button::new("titlebar-agent-summary")
                                    .custom(terminal_header_button)
                                    .xsmall()
                                    .text_color(terminal_header_muted)
                                    .icon(
                                        Icon::new(ComponentIconName::Bot)
                                            .xsmall()
                                            .text_color(color),
                                    )
                                    .label(label)
                                    .tooltip(if sidebar_auto_collapsed {
                                        "Open highest-priority Agent"
                                    } else {
                                        "Show Agents"
                                    })
                                    .on_click(move |_, window, app| {
                                        let target = summary_agent_target.clone();
                                        agent_summary_herdr.update(app, |this, cx| {
                                            if sidebar_auto_collapsed {
                                                if let Some((workspace_id, tab_id, pane_id)) =
                                                    target
                                                {
                                                    // FocusIntent seam: the Header summary jump shares
                                                    // the Sidebar's landing; partial attribution degrades
                                                    // to the most specific target.
                                                    if let Some(intent) = FocusIntent::from_targets(
                                                        workspace_id,
                                                        tab_id,
                                                        pane_id,
                                                    ) {
                                                        this.apply_focus_intent(intent, window, cx);
                                                    }
                                                }
                                            } else {
                                                this.reveal_agents_section(cx);
                                            }
                                        });
                                    }),
                            )
                        })
                        .when_some(script_indicator, |row, (color, label)| {
                            row.child(
                                Button::new("titlebar-script-summary")
                                    .custom(terminal_header_button)
                                    .xsmall()
                                    .text_color(terminal_header_muted)
                                    .icon(
                                        Icon::new(ComponentIconName::SquareTerminal)
                                            .xsmall()
                                            .text_color(color),
                                    )
                                    .label(label)
                                    .tooltip(if sidebar_auto_collapsed {
                                        "Open highest-priority BackgroundJob"
                                    } else {
                                        "Show Services"
                                    })
                                    .on_click(move |_, window, app| {
                                        let script_id = summary_script_id.clone();
                                        script_summary_herdr.update(app, |this, cx| {
                                            if sidebar_auto_collapsed {
                                                if let Some(script_id) = script_id {
                                                    this.focus_script_id(script_id, window, cx);
                                                }
                                            } else {
                                                this.reveal_services_section(cx);
                                            }
                                        });
                                    }),
                            )
                        })
                        .when_some(git_pill, |row, (additions, deletions, files, path)| {
                            let reveal_path = path;
                            row.child(
                                Button::new("titlebar-git-changes")
                                    .custom(terminal_header_button)
                                    .xsmall()
                                    .child(
                                        h_flex()
                                            .gap(px(3.0))
                                            .child(
                                                div()
                                                    .text_color(terminal_header_success)
                                                    .child(format!("+{additions}")),
                                            )
                                            .child(
                                                div()
                                                    .text_color(terminal_header_danger)
                                                    .child(format!("−{deletions}")),
                                            ),
                                    )
                                    .tooltip(format!(
                                        "{files} files changed · click to reveal in Finder"
                                    ))
                                    .on_click(move |_, _, _| {
                                        let _ = std::process::Command::new("open")
                                            .arg("-R")
                                            .arg(&reveal_path)
                                            .spawn();
                                    }),
                            )
                        })
                        .when_some(info_button, |row, button| row.child(button)),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_size(theme::FONT_DECORATIVE)
                        .text_color(muted)
                        .when(!compact_header && !secondary_surface, |row| {
                            row.child(script_launcher)
                        })
                        .when(
                            !compact_header
                                && !secondary_surface
                                && self.tui_width_restore_needed(),
                            |mut row| {
                                // Manual width-ownership fallback: re-impose this window's
                                // TUI grid when the automatic activation restore did not
                                // fire (shared session, last activated client wins).
                                let restore_herdr = herdr.clone();
                                row = row.child(
                                    Button::new("titlebar-restore-width")
                                        .custom(terminal_header_button)
                                        .xsmall()
                                        .label("Restore Width")
                                        .tooltip(
                                            "Restore this window's TUI width after another \
                                             client resized it",
                                        )
                                        .on_click(move |_, _, app| {
                                            restore_herdr.update(app, |this, cx| {
                                                this.restore_tui_grid_on_activation(cx)
                                            });
                                        }),
                                );
                                row
                            },
                        )
                        .when(
                            !compact_header
                                && !secondary_surface
                                && (self.focused_chat_agent().is_some()
                                    || self.chat.model.mode == crate::chat::WorkSurfaceMode::Chat),
                            |mut row| {
                                // Single bidirectional toggle: the label points to "the other side" (the Terminal
                                // view shows Chat, the Chat view shows Terminal; notate 2026-08-29).
                                let in_chat = self.chat.model.mode
                                    == crate::chat::WorkSurfaceMode::Chat;
                                let (label, tooltip, icon) = if in_chat {
                                    (
                                        "Terminal",
                                        "Back to the hosted terminal",
                                        Icon::empty()
                                            .path("icons/terminal.svg")
                                            .with_size(px(13.0)),
                                    )
                                } else {
                                    (
                                        "Chat",
                                        "Chat (semantic view of the focused agent)",
                                        Icon::empty()
                                            .path("icons/compose.svg")
                                            .with_size(px(13.0)),
                                    )
                                };
                                let chat_toggle_herdr = chat_header_herdr.clone();
                                row = row.child(
                                    Button::new("titlebar-chat")
                                        .custom(terminal_header_button)
                                        .xsmall()
                                        .icon(icon)
                                        .label(label)
                                        .tooltip(tooltip)
                                        .on_click(move |_, window, app| {
                                            chat_toggle_herdr.update(app, |this, cx| {
                                                this.toggle_chat_surface(window, cx)
                                            });
                                        }),
                                );
                                // M7: Live Handoff entry (Chat mode only; Working sources automatically
                                // use `Handoff after current turn`). The picker directly consumes the
                                // Host capability projection.
                                if in_chat
                                    && self.chat.model.binding.as_ref().is_some_and(
                                        |binding| !binding.source.file_path.is_empty(),
                                    )
                                    && !self.chat.handoff_in_flight
                                {
                                        let source_agent = self
                                            .chat
                                            .model
                                            .binding
                                            .as_ref()
                                            .map(|binding| binding.agent);
                                        let handoff_herdr = chat_header_herdr.clone();
                                        let available_targets: Vec<shardlane_history::AgentId> =
                                            self.config
                                                .providers
                                                .available_choices()
                                                .into_iter()
                                                .filter(|target| Some(*target) != source_agent)
                                                .filter(|target| {
                                                    shardlane_host::provider_product_capabilities(
                                                        *target,
                                                        shardlane_host::ProviderEnvironment {
                                                            enabled: true,
                                                            installed: true,
                                                        },
                                                    )
                                                    .startable
                                                })
                                                .collect();
                                        row = row.child(
                                            crate::composer_chip::ComposerChip::new(
                                                "titlebar-handoff",
                                            )
                                            .icon(
                                                Icon::empty()
                                                    .path("icons/corner-down-right.svg")
                                                    .with_size(px(13.0))
                                                    .into_any_element(),
                                            )
                                            .label("Handoff…")
                                            .dropdown_menu_with_anchor(
                                                gpui::Corner::BottomRight,
                                                move |mut menu, _, _| {
                                                    for target in available_targets.iter().copied()
                                                    {
                                                        let menu_entity = handoff_herdr.clone();
                                                        menu = menu.item(
                                                            PopupMenuItem::new(format!(
                                                                "Hand off to {} (verified up to last flush)",
                                                                target.display_name()
                                                            ))
                                                            .on_click(
                                                                move |_, window, app| {
                                                                    menu_entity.update(
                                                                        app,
                                                                        |this, cx| {
                                                                            this.start_live_handoff(
                                                                                target, window, cx,
                                                                            );
                                                                        },
                                                                    );
                                                                },
                                                            ),
                                                        );
                                                    }
                                                    menu
                                                },
                                            ),
                                        );
                                }
                                // Plan 060 Phase 2: low-exposure identity copy actions
                                // (Chat mode only; pane/tab/native session ids come from
                                //  the binding's exact identity — no guessing from cwd/title).
                                if in_chat {
                                    let binding = self.chat.model.binding.clone();
                                    if let Some(binding) = binding {
                                        let native_id = binding.native_session_id.clone();
                                        let pane_key = binding.pane_key.clone();
                                        let tab_id = self
                                            .state
                                            .agents
                                            .iter()
                                            .find(|a| {
                                                a.pane_id.as_deref()
                                                    == Some(pane_key.as_str())
                                            })
                                            .and_then(|a| a.tab_id.clone());
                                        let source_path = binding.source.file_path.clone();
                                        row = row.child(
                                            Button::new("titlebar-agent-identity")
                                                .custom(terminal_header_button)
                                                .xsmall()
                                                .icon(
                                                    Icon::new(ComponentIconName::Ellipsis)
                                                        .with_size(px(13.0)),
                                                )
                                                .tooltip("Agent identity & debug")
                                                .dropdown_menu_with_anchor(
                                                    gpui::Corner::BottomRight,
                                                    move |mut menu, _, _| {
                                                        if !native_id.is_empty() {
                                                            let id = native_id.clone();
                                                            menu = menu.item(
                                                                PopupMenuItem::new("Copy Agent Session ID")
                                                                    .on_click(move |_, _, app| {
                                                                        app.write_to_clipboard(
                                                                            crepuscularity_gpui::ClipboardItem::new_string(id.clone()),
                                                                        );
                                                                    }),
                                                            );
                                                        }
                                                        if !pane_key.is_empty() {
                                                            let id = pane_key.clone();
                                                            menu = menu.item(
                                                                PopupMenuItem::new("Copy Pane ID")
                                                                    .on_click(move |_, _, app| {
                                                                        app.write_to_clipboard(
                                                                            crepuscularity_gpui::ClipboardItem::new_string(id.clone()),
                                                                        );
                                                                    }),
                                                            );
                                                        }
                                                        if let Some(tab_id) = tab_id.clone() {
                                                            let id = tab_id.clone();
                                                            menu = menu.item(
                                                                PopupMenuItem::new("Copy Tab ID")
                                                                    .on_click(move |_, _, app| {
                                                                        app.write_to_clipboard(
                                                                            crepuscularity_gpui::ClipboardItem::new_string(id.clone()),
                                                                        );
                                                                    }),
                                                            );
                                                        }
                                                        if !source_path.is_empty() {
                                                            let path = source_path.clone();
                                                            menu = menu.item(PopupMenuItem::separator());
                                                            menu = menu.item(
                                                                PopupMenuItem::new("Reveal Session Source")
                                                                    .on_click(move |_, _, _| {
                                                                        let _ = std::process::Command::new("open")
                                                                            .arg("-R")
                                                                            .arg(&path)
                                                                            .spawn();
                                                                    }),
                                                            );
                                                        }
                                                        menu
                                                    },
                                                ),
                                        );
                                    }
                                }
                                row
                            },
                        )
                        .when(self.history.open, |row| {
                            // History secondary surface: Search/Refresh/Copy Markdown fold into the
                            // content Header's (titlebar's) right side, left of the panel toggle
                            //(notate 2026-08-29 H5/H6; the in-page 42px breadcrumb bar was removed).
                            row.child(
                                Button::new("titlebar-history-search")
                                    .custom(terminal_header_button)
                                    .xsmall()
                                    .icon(ComponentIconName::Search)
                                    .tooltip("Search full conversation text")
                                    .on_click(move |_, window, app| {
                                        history_search_herdr.update(app, |view, cx| {
                                            view.open_history_search(window, cx)
                                        });
                                    }),
                            )
                            .child(
                                Button::new("titlebar-history-refresh")
                                    .custom(terminal_header_button)
                                    .xsmall()
                                    .icon(if self.history.loading {
                                        Icon::new(ComponentIconName::LoaderCircle)
                                    } else {
                                        Icon::empty().path("icons/refresh-cw.svg")
                                    })
                                    .tooltip(if self.history.loading {
                                        "History indexing in progress"
                                    } else {
                                        "Refresh history index"
                                    })
                                    .disabled(self.history.loading)
                                    .on_click(move |_, _window, app| {
                                        history_refresh_herdr.update(app, |view, cx| {
                                            if !view.history.loading {
                                                view.refresh_history(true, cx);
                                            }
                                        });
                                    }),
                            )
                            .when_some(
                                self.history.transcript.clone(),
                                |row, transcript| {
                                    row.child(
                                        Button::new("titlebar-history-copy-md")
                                            .custom(terminal_header_button)
                                            .xsmall()
                                            .icon(ComponentIconName::Copy)
                                            .label("Copy Markdown")
                                            .tooltip("Copy full conversation as Markdown")
                                            .on_click(move |_, window, app| {
                                                let md = crate::history::history_export_cached_window_markdown(
                                                    &transcript,
                                                );
                                                app.write_to_clipboard(
                                                    crepuscularity_gpui::ClipboardItem::new_string(
                                                        md,
                                                    ),
                                                );
                                                window.push_notification(
                                                    "Conversation Markdown copied",
                                                    app,
                                                );
                                            }),
                                    )
                                },
                            )
                        })
                        .when(!sidebar_visible, |row| {
                            row.child(
                                Button::new("titlebar-search")
                                    .custom(terminal_header_button)
                                    .xsmall()
                                    .icon(ComponentIconName::Search)
                                    .tooltip("Search (⌘K)")
                                    .on_click(move |_, window, app| {
                                        search_header_herdr.update(app, |this, cx| {
                                            this.open_search(&OpenSearch, window, cx)
                                        });
                                    }),
                            )
                            .child(
                                Button::new("titlebar-history")
                                    .custom(terminal_header_button)
                                    .xsmall()
                                    .icon(
                                        Icon::empty().path("icons/layers.svg").with_size(px(14.0)),
                                    )
                                    .tooltip("History")
                                    .selected(self.history.open)
                                    .on_click(move |_, window, app| {
                                        history_header_herdr.update(app, |this, cx| {
                                            this.toggle_history(&OpenHistory, window, cx)
                                        });
                                    }),
                            )
                        })
                        .when(!connected, |row| {
                            row.child(
                                Button::new("titlebar-reconnect")
                                    .custom(terminal_header_button)
                                    .xsmall()
                                    .text_color(terminal_header_danger)
                                    .icon(
                                        Icon::new(ComponentIconName::TriangleAlert)
                                            .xsmall()
                                            .text_color(terminal_header_danger),
                                    )
                                    .label("Offline")
                                    .tooltip("Reconnect to Herdr")
                                    .on_click(move |_, window, app| {
                                        reconnect_header_herdr.update(app, |this, cx| {
                                            this.refresh(&Refresh, window, cx)
                                        });
                                    }),
                            )
                        })
                        .when(!self.right_panel.open, |row| {
                            // When the right panel is open the header keeps no toggle; the close
                            // button lives in the panel's own header.
                            row.child(self.render_right_panel_toggle_button(
                                right_panel_herdr,
                                toggle_hover,
                                toggle_active,
                                muted,
                            ))
                        }),
                )
        };

        // The Header has no bottom border line: TitleBar builds in border_b_1 while its div background
        // is the sidebar color — a transparent border would leak a 1px sidebar-colored horizontal line
        // at the content segment's bottom edge. border_b_0 eradicates it; the Header and the region
        // below join seamlessly through the same color.
        TitleBar::new()
            .bg(if sidebar_visible {
                component_theme.sidebar
            } else {
                terminal_header_bg
            })
            .border_b_0()
            .child(header)
    }
}
