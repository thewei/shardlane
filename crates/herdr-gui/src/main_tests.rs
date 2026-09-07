use super::{
    agent_notification_kind, agent_reconcile_ready, apply_agent_projection_patch,
    apply_pane_agent_projection_patch, clamped_terminal_font_size, coalesce_terminal_input,
    consume_terminal_scroll_rows, derive_selection_flags, grid_size_for, is_secondary_surface,
    navigation_reconcile_ready, next_terminal_poll_interval, operational_summary,
    pending_copy_is_stale, reconnect_backoff, resolve_navigation_selection,
    resolve_workspace_tab_locally, resolved_theme_preset, responsive_dialog_width,
    should_project_terminal_frame, sidebar_should_auto_collapse, terminal_drain_budget_bytes,
    terminal_focus_report_action, terminal_frame_min_interval_for_activity,
    terminal_frame_retry_interval, wait_for_terminal_poll, AgentNotificationKind, ClientSearchItem,
    ClientSearchTarget, OperationalSummary, TerminalFocusReportAction, TerminalInputCommand,
    TerminalPollTrigger,
};
use crate::herdr::{
    Agent, AgentStatusPatch, HerdrState, LayoutPane, LayoutRect, NavigationState, Pane, PaneLayout,
    Tab, Workspace,
};
use crate::scripts::{ScriptRecord, ScriptRegistry, ScriptStatus};
use crate::search_model::{
    history_message_search_item, parse_client_search_query, ClientProjectSearchScope,
    ClientSearchScope,
};
use crate::search_view::client_picker_width;
use crate::shell_input::terminal_keystroke_blocked;
use crate::shell_input::{route_shell_keystroke, ShellKeyRoute};
use crate::shell_projects::before_target_authoritative_insert;
use crate::terminal_interact::terminal_selection_background;
use crate::terminal_interact::PendingTerminalCopy;
use futures::executor::block_on;
use futures::future::{pending, ready};
use shardlane_history::{AgentId, ConversationMeta, SearchHit};
use std::collections::VecDeque;
use std::time::Duration;

#[test]
fn terminal_mouse_selection_uses_frame_palette_highlight() {
    let frame = crate::ghostty::TerminalFrame {
        selection_color: Some(0x9fcdff),
        ..crate::ghostty::TerminalFrame::default()
    };
    assert_eq!(terminal_selection_background(&frame), 0x9fcdff);
    assert_eq!(
        terminal_selection_background(&crate::ghostty::TerminalFrame::default()),
        crate::terminal_view::TERMINAL_SELECTION_BG
    );
}

#[test]
fn font_size_clamps_to_settings_slider_range() {
    assert_eq!(clamped_terminal_font_size(9.5), 10.0);
    assert_eq!(clamped_terminal_font_size(24.5), 24.0);
    assert_eq!(clamped_terminal_font_size(12.0), 12.0);
    assert_eq!(clamped_terminal_font_size(10.0), 10.0);
    assert_eq!(clamped_terminal_font_size(24.0), 24.0);
}

#[test]
fn operational_summary_is_shared_attention_projection_for_header_and_status_item() {
    let agents = vec![
        Agent {
            agent_status: Some("working".to_string()),
            ..Agent::default()
        },
        Agent {
            agent_status: Some("blocked".to_string()),
            ..Agent::default()
        },
        Agent {
            agent_status: Some("idle".to_string()),
            ..Agent::default()
        },
    ];
    let mut running = ScriptRecord::default();
    running.runtime.status = ScriptStatus::Running;
    let mut starting = ScriptRecord::default();
    starting.runtime.status = ScriptStatus::Starting;
    let mut failed = ScriptRecord::default();
    failed.runtime.status = ScriptStatus::Failed;
    let scripts = ScriptRegistry {
        scripts: vec![running, starting, failed, ScriptRecord::default()],
    };

    assert_eq!(
        operational_summary(&agents, &scripts),
        OperationalSummary {
            blocked_agents: 1,
            working_agents: 1,
            failed_scripts: 1,
            active_scripts: 2,
        }
    );
}

#[test]
fn root_view_mounts_all_gpui_component_overlay_layers() {
    let source = include_str!("main.rs");
    // 2026-08-27: the root context is always "ShardlaneApp" (fixed shortcuts being fully dead in
    // TUI mode); structural anchors updated in sync.
    let Some((_, after_root)) = source.split_once("root.key_context(\"ShardlaneApp\")") else {
        panic!("Shardlane root render chain");
    };
    let Some((root_chain, _)) = after_root.split_once(".on_action(cx.listener(Self::toggle_help))")
    else {
        panic!("Shardlane root action boundary");
    };
    for layer in [
        ".children(Root::render_dialog_layer(window, cx))",
        ".children(Root::render_sheet_layer(window, cx))",
        ".children(Root::render_notification_layer(window, cx))",
    ] {
        assert!(
            root_chain.contains(layer),
            "missing root overlay layer: {layer}"
        );
    }
}

#[test]
fn secondary_surfaces_consistently_hide_runtime_header_controls() {
    assert!(!is_secondary_surface(false, false, false));
    assert!(is_secondary_surface(true, false, false));
    assert!(is_secondary_surface(false, true, false));
    assert!(is_secondary_surface(false, false, true));
    assert!(is_secondary_surface(true, true, true));
}

#[test]
fn responsive_dialog_width_respects_window_fraction_and_bounds() {
    assert_eq!(responsive_dialog_width(360.0, 0.72, 300.0, 720.0), 300.0);
    assert_eq!(responsive_dialog_width(800.0, 0.72, 300.0, 720.0), 576.0);
    assert_eq!(responsive_dialog_width(1600.0, 0.72, 300.0, 720.0), 720.0);
    assert_eq!(client_picker_width(360.0), 320.0);
    // Card width: full viewport width (minus the 24*2 scrims), capped at 680.
    assert_eq!(client_picker_width(800.0), 680.0);
    assert_eq!(client_picker_width(1600.0), 680.0);
}

#[test]
fn agent_notifications_only_cover_meaningful_authoritative_transitions() {
    assert_eq!(
        agent_notification_kind(Some("working"), Some("done")),
        Some(AgentNotificationKind::Finished)
    );
    assert_eq!(
        agent_notification_kind(Some("working"), Some("blocked")),
        Some(AgentNotificationKind::NeedsAttention)
    );
    assert_eq!(
        agent_notification_kind(Some("working"), Some("idle")),
        Some(AgentNotificationKind::Ready)
    );
    assert_eq!(agent_notification_kind(None, Some("working")), None);
    assert_eq!(
        agent_notification_kind(Some("working"), Some("working")),
        None
    );
    assert_eq!(agent_notification_kind(Some("idle"), Some("working")), None);
}

#[test]
fn repeated_agent_projection_patch_is_a_noop_after_first_application() {
    let patch = AgentStatusPatch {
        pane_id: "p1".to_string(),
        workspace_id: "w1".to_string(),
        agent_status: Some(Some("working".to_string())),
        agent: Some(Some("codex".to_string())),
        display_agent: Some(Some("Codex".to_string())),
        title: Some(Some("Implementing".to_string())),
        tab_id: Some(Some("t1".to_string())),
        focused: Some(true),
        ..AgentStatusPatch::default()
    };
    let mut agent = Agent {
        pane_id: Some("p1".to_string()),
        ..Agent::default()
    };
    let mut pane = Pane {
        pane_id: "p1".to_string(),
        terminal_id: None,
        workspace_id: Some("w1".to_string()),
        tab_id: Some("t1".to_string()),
        label: None,
        title: None,
        terminal_title: None,
        cwd: None,
        agent_status: None,
        agent: None,
        focused: true,
        scroll: None,
    };

    assert!(apply_agent_projection_patch(&mut agent, &patch));
    assert!(apply_pane_agent_projection_patch(&mut pane, &patch));
    assert!(!apply_agent_projection_patch(&mut agent, &patch));
    assert!(!apply_pane_agent_projection_patch(&mut pane, &patch));
}

#[test]
fn herdr_focus_facts_never_flip_client_navigation_selection() {
    // The `focused` in pane.updated events is a Herdr runtime fact:
    // client selection is owned by local navigation; events must not flip it back.
    let patch = AgentStatusPatch {
        pane_id: "p1".to_string(),
        workspace_id: "w1".to_string(),
        focused: Some(false),
        ..AgentStatusPatch::default()
    };
    let mut agent = Agent {
        pane_id: Some("p1".to_string()),
        workspace_id: Some("w1".to_string()),
        focused: true,
        ..Agent::default()
    };
    let mut pane = Pane {
        pane_id: "p1".to_string(),
        terminal_id: None,
        workspace_id: Some("w1".to_string()),
        tab_id: Some("t1".to_string()),
        label: None,
        title: None,
        terminal_title: None,
        cwd: None,
        agent_status: None,
        agent: None,
        focused: true,
        scroll: None,
    };

    assert!(!apply_agent_projection_patch(&mut agent, &patch));
    assert!(!apply_pane_agent_projection_patch(&mut pane, &patch));
    assert!(agent.focused);
    assert!(pane.focused);
}

#[test]
fn reconnect_backoff_grows_exponentially_then_caps() {
    // F19/F47: 0.5s → 1s → 2s → 4s → 8s → 15s (capped).
    assert_eq!(reconnect_backoff(0), Duration::from_millis(500));
    assert_eq!(reconnect_backoff(1), Duration::from_secs(1));
    assert_eq!(reconnect_backoff(2), Duration::from_secs(2));
    assert_eq!(reconnect_backoff(3), Duration::from_secs(4));
    assert_eq!(reconnect_backoff(4), Duration::from_secs(8));
    assert_eq!(
        reconnect_backoff(5),
        Duration::from_secs(16).min(Duration::from_secs(15))
    );
    assert_eq!(reconnect_backoff(9), Duration::from_secs(15));
}

#[test]
fn selection_flags_derive_from_ids_as_single_source_of_truth() {
    // F34: the booleans are pure functions of ids — flags must derive consistently and idempotently for any id combination.
    let mut state = HerdrState {
        focused_workspace_id: Some("w2".to_string()),
        focused_tab_id: Some("t2".to_string()),
        focused_pane_id: Some("p2".to_string()),
        workspaces: vec![
            Workspace {
                workspace_id: "w1".to_string(),
                focused: true,
                label: None,
                cwd: Some("/w1".to_string()),
                agent_status: None,
                active_tab_id: None,
                tab_count: None,
                pane_count: None,
                number: None,
            },
            Workspace {
                workspace_id: "w2".to_string(),
                focused: false,
                label: None,
                cwd: Some("/w2".to_string()),
                agent_status: None,
                active_tab_id: None,
                tab_count: None,
                pane_count: None,
                number: None,
            },
        ],
        tabs: vec![
            Tab {
                tab_id: "t1".to_string(),
                workspace_id: Some("w1".to_string()),
                focused: true,
                label: None,
                title: None,
                terminal_title: None,
                agent_status: None,
                pane_count: None,
            },
            Tab {
                tab_id: "t2".to_string(),
                workspace_id: Some("w2".to_string()),
                focused: false,
                label: None,
                title: None,
                terminal_title: None,
                agent_status: None,
                pane_count: None,
            },
        ],
        panes: vec![
            Pane {
                pane_id: "p1".to_string(),
                workspace_id: Some("w1".to_string()),
                focused: true,
                terminal_id: None,
                tab_id: None,
                label: None,
                title: None,
                terminal_title: None,
                cwd: None,
                agent_status: None,
                agent: None,
                scroll: None,
            },
            Pane {
                pane_id: "p2".to_string(),
                workspace_id: Some("w2".to_string()),
                focused: false,
                terminal_id: None,
                tab_id: None,
                label: None,
                title: None,
                terminal_title: None,
                cwd: None,
                agent_status: None,
                agent: None,
                scroll: None,
            },
        ],
        layouts: vec![PaneLayout {
            tab_id: "t2".to_string(),
            zoomed: true,
            focused_pane_id: Some("p1".to_string()),
            panes: vec![
                LayoutPane {
                    pane_id: "p1".to_string(),
                    focused: false,
                    rect: LayoutRect {
                        x: 0,
                        y: 0,
                        width: 40,
                        height: 24,
                    },
                },
                LayoutPane {
                    pane_id: "p2".to_string(),
                    focused: true,
                    rect: LayoutRect {
                        x: 40,
                        y: 0,
                        width: 40,
                        height: 24,
                    },
                },
            ],
            workspace_id: Some("w2".to_string()),
            area: LayoutRect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
            splits: Vec::new(),
        }],
        ..HerdrState::default()
    };

    derive_selection_flags(&mut state);
    assert!(!state.workspaces[0].focused && state.workspaces[1].focused);
    assert!(!state.tabs[0].focused && state.tabs[1].focused);
    assert!(!state.panes[0].focused && state.panes[1].focused);
    // Zoom runtime facts untouched: layout.focused_pane_id remains the server-side zoom target.
    assert_eq!(state.layouts[0].focused_pane_id.as_deref(), Some("p1"));
    // Idempotent: repeated derivation changes nothing.
    let before = state
        .panes
        .iter()
        .map(|pane| pane.focused)
        .collect::<Vec<_>>();
    derive_selection_flags(&mut state);
    assert_eq!(
        state
            .panes
            .iter()
            .map(|pane| pane.focused)
            .collect::<Vec<_>>(),
        before
    );
}

#[test]
fn workspace_tab_memory_beats_herdr_active_tab_with_ownership_check() {
    // F20: remembered t2 (still in w1) wins; remembered t3 (belongs to another workspace) is rejected by the ownership check.
    let workspaces = vec![Workspace {
        workspace_id: "w1".to_string(),
        active_tab_id: Some("t1".to_string()),
        label: None,
        cwd: Some("/w1".to_string()),
        agent_status: None,
        focused: false,
        tab_count: None,
        pane_count: None,
        number: None,
    }];
    let tabs = vec![
        Tab {
            tab_id: "t1".to_string(),
            workspace_id: Some("w1".to_string()),
            label: None,
            title: None,
            terminal_title: None,
            agent_status: None,
            pane_count: None,
            focused: false,
        },
        Tab {
            tab_id: "t2".to_string(),
            workspace_id: Some("w1".to_string()),
            label: None,
            title: None,
            terminal_title: None,
            agent_status: None,
            pane_count: None,
            focused: false,
        },
        Tab {
            tab_id: "t3".to_string(),
            workspace_id: Some("w2".to_string()),
            label: None,
            title: None,
            terminal_title: None,
            agent_status: None,
            pane_count: None,
            focused: false,
        },
    ];
    assert_eq!(
        resolve_workspace_tab_locally(&workspaces, &tabs, "w1", Some("t2")),
        Some("t2".to_string())
    );
    assert_eq!(
        resolve_workspace_tab_locally(&workspaces, &tabs, "w1", Some("t3")),
        Some("t1".to_string()) // ownership check rejects → active_tab_id
    );
    assert_eq!(
        resolve_workspace_tab_locally(&workspaces, &tabs, "w1", None),
        Some("t1".to_string())
    );
}

#[test]
fn terminal_focus_report_only_flips_on_activation_edges() {
    let reported = Some(("p1".to_string(), "term-1".to_string()));
    // Active and reported (live controller): in-app navigation injects no focus escape.
    assert_eq!(
        terminal_focus_report_action(true, reported.as_ref()),
        TerminalFocusReportAction::None
    );
    // Inactive and reported: send Out.
    assert_eq!(
        terminal_focus_report_action(false, reported.as_ref()),
        TerminalFocusReportAction::FocusOut
    );
    // Active and unreported (including no pane / voided after a controller teardown): send In.
    assert_eq!(
        terminal_focus_report_action(true, None),
        TerminalFocusReportAction::FocusIn
    );
    // F18 regression pin: inactive and unreported must produce no action;
    // "active but no pane" stays unreported upstream (no poisoning) and therefore doesn't get stuck on None either.
    assert_eq!(
        terminal_focus_report_action(false, None),
        TerminalFocusReportAction::None
    );
}

#[test]
fn agent_reconcile_coalesces_bursts_but_has_a_max_delay() {
    assert!(!agent_reconcile_ready(
        true,
        Some(Duration::from_millis(120)),
        Some(Duration::from_millis(900)),
        Duration::from_millis(900),
    ));
    assert!(agent_reconcile_ready(
        true,
        Some(Duration::from_millis(300)),
        Some(Duration::from_millis(900)),
        Duration::from_millis(900),
    ));
    assert!(agent_reconcile_ready(
        true,
        Some(Duration::from_millis(10)),
        Some(Duration::from_millis(1_600)),
        Duration::from_millis(1_600),
    ));
    assert!(!agent_reconcile_ready(
        true,
        Some(Duration::from_millis(300)),
        Some(Duration::from_millis(1_600)),
        Duration::from_millis(500),
    ));
}

#[test]
fn navigation_reconcile_waits_for_a_quiet_event_window() {
    assert!(!navigation_reconcile_ready(
        true,
        false,
        Some(Duration::from_millis(120))
    ));
    assert!(navigation_reconcile_ready(
        true,
        false,
        Some(Duration::from_millis(180))
    ));
    assert!(!navigation_reconcile_ready(
        true,
        true,
        Some(Duration::from_secs(1))
    ));
    assert!(!navigation_reconcile_ready(
        false,
        false,
        Some(Duration::from_secs(1))
    ));
}

#[test]
fn hidden_terminal_surface_defers_frame_projection_without_dropping_pending_state() {
    assert!(!should_project_terminal_frame(true, true));
    assert!(should_project_terminal_frame(true, false));
    assert!(!should_project_terminal_frame(false, false));
}

#[test]
fn grid_size_reports_content_exact_pixels_for_both_paths() {
    // Audit B19: the single pixel convention — the pixel fields are content-exact
    // (padding excluded), so the window-derived attach fallback and the steady-state
    // canvas measurement produce the same TerminalSize for the same content area and
    // attach can no longer force an extra SIGWINCH.
    let size = grid_size_for(1013.0, 691.0, 7.2, 18.0);
    assert_eq!(size.0, 140); // floor(1013 / 7.2)
    assert_eq!(size.1, 38); // floor(691 / 18)
    assert_eq!(size.2, 1013.0_f64.round() as u16);
    assert_eq!(size.3, 691.0_f64.round() as u16);

    // Sub-pixel content still clamps to the visible minimums.
    let tiny = grid_size_for(0.4, 0.4, 7.2, 18.0);
    assert_eq!((tiny.0, tiny.1), (1, 1));
    assert_eq!((tiny.2, tiny.3), (1_u16, 1_u16));

    // Grid math is decoupled from the pixel rounding: a larger cell width must
    // shrink cols without changing the reported pixel width.
    let wide = grid_size_for(100.0, 100.0, 20.0, 20.0);
    assert_eq!((wide.0, wide.1), (5, 5));
    assert_eq!((wide.2, wide.3), (100, 100));
}

#[test]
fn pended_terminal_copy_is_discarded_after_a_generation_bump() {
    // Audit B20: ⌘C pends the copy stamped with the terminal generation; a restart
    // between the keystroke and the poll tick invalidates the stale coordinates.
    let copy = PendingTerminalCopy {
        selection: ((0, 0), (4, 0)),
        fallback_text: "hello".to_string(),
        generation: 7,
    };
    assert!(!pending_copy_is_stale(&copy, 7));
    assert!(pending_copy_is_stale(&copy, 8));
}

#[test]
fn terminal_poll_backoff_resets_on_activity_and_caps_by_focus() {
    let active = next_terminal_poll_interval(Duration::from_millis(120), true, false);
    assert_eq!(active, Duration::from_millis(16));

    let focused = next_terminal_poll_interval(Duration::from_millis(32), false, true);
    assert_eq!(focused, Duration::from_millis(48));
    assert_eq!(
        next_terminal_poll_interval(focused, false, true),
        Duration::from_millis(48)
    );

    let background = next_terminal_poll_interval(Duration::from_millis(64), false, false);
    assert_eq!(background, Duration::from_millis(120));
    assert_eq!(
        next_terminal_poll_interval(background, false, false),
        Duration::from_millis(120)
    );
}

#[test]
fn terminal_interactive_output_uses_smaller_vt_drain_budget() {
    assert_eq!(terminal_drain_budget_bytes(true), 64 * 1024);
    assert_eq!(terminal_drain_budget_bytes(false), 256 * 1024);
}

#[test]
fn terminal_interaction_frames_do_not_add_a_second_software_gate() {
    assert_eq!(
        terminal_frame_min_interval_for_activity(true, true),
        Duration::ZERO
    );
    assert_eq!(
        terminal_frame_min_interval_for_activity(true, false),
        Duration::ZERO
    );
    assert_eq!(
        terminal_frame_min_interval_for_activity(false, true),
        Duration::from_millis(16)
    );
    assert_eq!(
        terminal_frame_min_interval_for_activity(false, false),
        Duration::from_millis(100)
    );
}

#[test]
fn pending_terminal_frame_waits_only_for_remaining_budget() {
    assert_eq!(
        terminal_frame_retry_interval(Some(Duration::from_millis(7)), Duration::from_millis(16)),
        Duration::from_millis(9)
    );
    assert_eq!(
        terminal_frame_retry_interval(Some(Duration::from_millis(16)), Duration::from_millis(16)),
        Duration::from_millis(1)
    );
    assert_eq!(
        terminal_frame_retry_interval(Some(Duration::from_millis(25)), Duration::from_millis(16)),
        Duration::from_millis(1)
    );
}

#[test]
fn terminal_output_wakes_poll_without_waiting_for_idle_timer() {
    let (sender, receiver) = async_channel::bounded(1);
    sender
        .try_send(())
        .unwrap_or_else(|error| panic!("{error}"));
    let trigger = block_on(wait_for_terminal_poll(&receiver, pending()));
    assert_eq!(trigger, TerminalPollTrigger::Output);
}

#[test]
fn terminal_poll_timer_remains_idle_fallback() {
    let (_sender, receiver) = async_channel::bounded::<()>(1);
    let trigger = block_on(wait_for_terminal_poll(&receiver, ready(())));
    assert_eq!(trigger, TerminalPollTrigger::Timer);
}

#[test]
fn terminal_poll_observes_closed_output_channel_immediately() {
    let (sender, receiver) = async_channel::bounded::<()>(1);
    drop(sender);
    let trigger = block_on(wait_for_terminal_poll(&receiver, pending()));
    assert_eq!(trigger, TerminalPollTrigger::Closed);
}

#[test]
fn narrow_window_sidebar_auto_collapse_is_ephemeral_layout_policy() {
    assert!(sidebar_should_auto_collapse(360.0));
    assert!(sidebar_should_auto_collapse(480.0));
    assert!(sidebar_should_auto_collapse(720.0));
    assert!(sidebar_should_auto_collapse(979.0));
    assert!(!sidebar_should_auto_collapse(980.0));
    assert!(!sidebar_should_auto_collapse(1280.0));
}

#[test]
fn global_history_search_result_keeps_message_target_and_full_path() {
    let session = ConversationMeta {
        key: "claude:session-1".to_string(),
        id: "session-1".to_string(),
        agent: AgentId::ClaudeCode,
        title: "Fix terminal resize".to_string(),
        project_path: "/Users/test/Workspaces/shardlane".to_string(),
        project_name: "shardlane".to_string(),
        file_path: "/tmp/session-1.jsonl".to_string(),
        created_at: 1,
        updated_at: 2,
        message_count: 3,
        size_bytes: 4,
        git_branch: None,
        model: None,
        tokens_used: None,
        archived: false,
        source: None,
    };
    let item = history_message_search_item(SearchHit {
        session,
        seq: 42,
        role: "assistant".to_string(),
        snippet: "resize the terminal to the actual pane bounds".to_string(),
        timestamp: Some(2),
    });
    assert!(item.matches("terminal pane bounds"));
    assert!(item.detail.contains("/Users/test/Workspaces/shardlane"));
    assert!(matches!(
        item.target,
        ClientSearchTarget::Conversation {
            target_seq: Some(42),
            ..
        }
    ));
}

#[test]
fn navigation_metadata_refresh_preserves_local_selection_until_it_disappears() {
    let navigation = NavigationState {
        focused_workspace_id: Some("w2".to_string()),
        focused_tab_id: Some("w2:t1".to_string()),
        workspaces: vec![
            Workspace {
                workspace_id: "w1".to_string(),
                label: Some("one".to_string()),
                cwd: None,
                agent_status: None,
                active_tab_id: Some("w1:t2".to_string()),
                focused: false,
                tab_count: Some(2),
                pane_count: Some(2),
                number: Some(1),
            },
            Workspace {
                workspace_id: "w2".to_string(),
                label: Some("two".to_string()),
                cwd: None,
                agent_status: None,
                active_tab_id: Some("w2:t1".to_string()),
                focused: true,
                tab_count: Some(1),
                pane_count: Some(1),
                number: Some(2),
            },
        ],
        tabs: vec![
            Tab {
                tab_id: "w1:t1".to_string(),
                workspace_id: Some("w1".to_string()),
                label: None,
                title: None,
                terminal_title: None,
                agent_status: None,
                pane_count: Some(1),
                focused: false,
            },
            Tab {
                tab_id: "w1:t2".to_string(),
                workspace_id: Some("w1".to_string()),
                label: None,
                title: None,
                terminal_title: None,
                agent_status: None,
                pane_count: Some(1),
                focused: false,
            },
            Tab {
                tab_id: "w2:t1".to_string(),
                workspace_id: Some("w2".to_string()),
                label: None,
                title: None,
                terminal_title: None,
                agent_status: None,
                pane_count: Some(1),
                focused: true,
            },
        ],
    };

    assert_eq!(
        resolve_navigation_selection(Some("w1"), Some("w1:t1"), &navigation),
        (Some("w1".to_string()), Some("w1:t1".to_string()))
    );

    let mut without_selected_tab = navigation.clone();
    without_selected_tab
        .tabs
        .retain(|tab| tab.tab_id != "w1:t1");
    assert_eq!(
        resolve_navigation_selection(Some("w1"), Some("w1:t1"), &without_selected_tab),
        (Some("w1".to_string()), Some("w1:t2".to_string()))
    );
    // TUI-only: navigation-state application accepts Herdr's runtime focus ids verbatim (the
    // retired surface wrapper only cloned them back onto themselves), so w2/w2:t1 win over the
    // local w1 selection without any local-preservation branch.
    assert_eq!(
        (
            navigation.focused_workspace_id.as_deref(),
            navigation.focused_tab_id.as_deref()
        ),
        (Some("w2"), Some("w2:t1"))
    );
}

#[test]
fn global_search_query_resolves_agent_filters_without_polluting_history_terms() {
    let query = parse_client_search_query("agent:claude parser error", &[]);
    assert_eq!(query.in_memory_query, "claude parser error");
    assert_eq!(query.history_query, "parser error");
    assert_eq!(query.agent_filter.as_deref(), Some("claude"));
    assert_eq!(query.agents, vec![AgentId::ClaudeCode]);
    assert!(query.history_allowed());
}

#[test]
fn global_search_project_scope_maps_to_one_project_and_filters_project_targets() {
    let projects = vec![ClientProjectSearchScope {
        haystack: "api /work/api herdr-w2".to_string(),
        project_path: "/work/api".to_string(),
    }];
    let query = parse_client_search_query("#project project:api", &projects);
    assert_eq!(query.in_memory_query, "api");
    assert_eq!(query.history_query, "");
    assert_eq!(query.project_filter.as_deref(), Some("api"));
    assert_eq!(query.history_project_paths, vec!["/work/api"]);
    assert_eq!(query.scopes, vec![ClientSearchScope::Project]);
    assert!(query.allows_target(&ClientSearchTarget::Project {
        workspace_id: "herdr-w2".to_string(),
    }));
    assert!(!query.history_allowed());
}

#[test]
fn global_search_unique_hash_prefixes_complete_to_scope_semantics() {
    let project_query = parse_client_search_query("#pro api", &[]);
    assert_eq!(project_query.scopes, vec![ClientSearchScope::Project]);
    assert_eq!(project_query.scope_completion, Some("#project"));
    assert_eq!(project_query.in_memory_query, "api");

    let script_query = parse_client_search_query("#script 5173", &[]);
    assert!(
        script_query.allows_target(&ClientSearchTarget::DetectedService {
            workspace_id: "w1".into(),
            tab_id: "t1".into(),
            pane_id: "p1".into(),
        })
    );

    let ambiguous = parse_client_search_query("#p shell", &[]);
    assert!(ambiguous.scopes.is_empty());
    assert_eq!(ambiguous.scope_completion, None);
    assert_eq!(ambiguous.in_memory_query, "#p shell");
}

#[test]
fn global_search_hash_scopes_filter_targets_without_polluting_search_terms() {
    let query = parse_client_search_query("#history #agent crash", &[]);
    assert_eq!(query.in_memory_query, "crash");
    assert_eq!(query.history_query, "crash");
    assert_eq!(
        query.scopes,
        vec![ClientSearchScope::History, ClientSearchScope::Agent]
    );
    assert!(query.history_allowed());
    assert!(query.allows_target(&ClientSearchTarget::Agent {
        workspace_id: None,
        tab_id: None,
        pane_id: None,
        terminal_id: None,
    }));
    assert!(query.allows_target(&ClientSearchTarget::Conversation {
        session: Box::new(ConversationMeta {
            key: "k".to_string(),
            id: "id".to_string(),
            agent: AgentId::ClaudeCode,
            title: "t".to_string(),
            project_path: "/work/shardlane".to_string(),
            project_name: "shardlane".to_string(),
            file_path: "/tmp/k".to_string(),
            created_at: 0,
            updated_at: 0,
            message_count: 0,
            size_bytes: 0,
            git_branch: None,
            model: None,
            tokens_used: None,
            archived: false,
            source: None,
        }),
        target_seq: None,
    }));

    let pane_query = parse_client_search_query("#pane shell", &[]);
    assert_eq!(pane_query.in_memory_query, "shell");
    assert_eq!(pane_query.scopes, vec![ClientSearchScope::Pane]);
    assert!(pane_query.allows_target(&ClientSearchTarget::Pane {
        workspace_id: Some("w1".to_string()),
        tab_id: Some("t1".to_string()),
        pane_id: "p1".to_string(),
    }));
    assert!(!pane_query.history_allowed());
}

#[test]
fn client_search_is_case_insensitive_and_matches_all_tokens() {
    let item = ClientSearchItem::new(
        "API Server".to_string(),
        "Project · /work/platform".to_string(),
        "w3F",
        gpui_component::IconName::FolderOpen,
        ClientSearchTarget::Project {
            workspace_id: "w3F".to_string(),
        },
    );
    assert!(item.matches("api"));
    assert!(item.matches("API PLATFORM"));
    assert!(item.matches("w3f"));
    assert!(!item.matches("api mobile"));
}

#[test]
fn terminal_input_burst_coalesces_to_bounded_queue_work() {
    let mut queue = VecDeque::new();
    for _ in 0..20_000 {
        coalesce_terminal_input(
            &mut queue,
            TerminalInputCommand::Text {
                pane_id: "p1".to_string(),
                target: "t1".to_string(),
                text: "x".to_string(),
            },
        );
    }
    assert_eq!(queue.len(), 1);
    match queue.front() {
        Some(TerminalInputCommand::Text { text, .. }) => assert_eq!(text.len(), 20_000),
        _ => panic!("expected one coalesced text command"),
    }

    for _ in 0..5_000 {
        coalesce_terminal_input(
            &mut queue,
            TerminalInputCommand::Keys {
                pane_id: "p1".to_string(),
                keys: vec!["left".to_string()],
            },
        );
    }
    assert_eq!(queue.len(), 2);
    match queue.back() {
        Some(TerminalInputCommand::Keys { keys, .. }) => assert_eq!(keys.len(), 5_000),
        _ => panic!("expected one coalesced key batch"),
    }
}

#[test]
fn terminal_input_coalesces_without_reordering_boundaries() {
    let mut queue = VecDeque::new();
    coalesce_terminal_input(
        &mut queue,
        TerminalInputCommand::Text {
            pane_id: "p1".to_string(),
            target: "t1".to_string(),
            text: "a".to_string(),
        },
    );
    coalesce_terminal_input(
        &mut queue,
        TerminalInputCommand::Text {
            pane_id: "p1".to_string(),
            target: "t1".to_string(),
            text: "bc".to_string(),
        },
    );
    coalesce_terminal_input(
        &mut queue,
        TerminalInputCommand::Keys {
            pane_id: "p1".to_string(),
            keys: vec!["left".to_string()],
        },
    );
    coalesce_terminal_input(
        &mut queue,
        TerminalInputCommand::Keys {
            pane_id: "p1".to_string(),
            keys: vec!["right".to_string()],
        },
    );
    coalesce_terminal_input(
        &mut queue,
        TerminalInputCommand::Paste {
            pane_id: "p1".to_string(),
            target: "t1".to_string(),
            text: "paste".to_string(),
        },
    );
    coalesce_terminal_input(
        &mut queue,
        TerminalInputCommand::Text {
            pane_id: "p2".to_string(),
            target: "t2".to_string(),
            text: "z".to_string(),
        },
    );

    match queue.pop_front() {
        Some(TerminalInputCommand::Text {
            pane_id,
            target,
            text,
        }) => {
            assert_eq!(pane_id, "p1");
            assert_eq!(target, "t1");
            assert_eq!(text, "abc");
        }
        _ => panic!("expected coalesced text"),
    }
    match queue.pop_front() {
        Some(TerminalInputCommand::Keys { pane_id, keys }) => {
            assert_eq!(pane_id, "p1");
            assert_eq!(keys, vec!["left".to_string(), "right".to_string()]);
        }
        _ => panic!("expected coalesced keys"),
    }
    match queue.pop_front() {
        Some(TerminalInputCommand::Paste {
            pane_id,
            target,
            text,
        }) => {
            assert_eq!(pane_id, "p1");
            assert_eq!(target, "t1");
            assert_eq!(text, "paste");
        }
        _ => panic!("expected paste boundary"),
    }
    match queue.pop_front() {
        Some(TerminalInputCommand::Text {
            pane_id,
            target,
            text,
        }) => {
            assert_eq!(pane_id, "p2");
            assert_eq!(target, "t2");
            assert_eq!(text, "z");
        }
        _ => panic!("expected trailing text"),
    }
    assert!(queue.is_empty());
}

#[test]
fn precise_terminal_scroll_consumes_only_newly_crossed_rows() {
    let mut residual = 0.0;
    assert_eq!(
        consume_terminal_scroll_rows(&mut residual, 7.2, 18.0, false),
        0
    );
    assert!((residual - 7.2).abs() < f64::EPSILON);
    assert_eq!(
        consume_terminal_scroll_rows(&mut residual, 7.2, 18.0, false),
        0
    );
    assert!((residual - 14.4).abs() < f64::EPSILON);
    assert_eq!(
        consume_terminal_scroll_rows(&mut residual, 7.2, 18.0, false),
        1
    );
    assert!((residual - 3.6).abs() < 0.001);

    // The next event contributes only its own distance. The prior 21.6 px gesture total is
    // not resent as another full row, which was the old runaway-scroll bug.
    assert_eq!(
        consume_terminal_scroll_rows(&mut residual, 7.2, 18.0, false),
        0
    );
    assert!((residual - 10.8).abs() < 0.001);
}

#[test]
fn precise_terminal_scroll_resets_partial_distance_on_direction_change_and_end() {
    let mut residual = 9.0;
    assert_eq!(
        consume_terminal_scroll_rows(&mut residual, -9.0, 18.0, false),
        0
    );
    assert!((residual + 9.0).abs() < f64::EPSILON);
    assert_eq!(
        consume_terminal_scroll_rows(&mut residual, -9.0, 18.0, true),
        -1
    );
    assert_eq!(residual, 0.0);
}

// --- Drag ordering: the Shardlane display order == Herdr's authoritative order ---

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn before_target_authoritative_insert_matches_herdr_remove_insert_semantics() {
    let auth = ids(&["a", "b", "c"]);
    // Forward drag: a(0) lands before c(2); after removing a, c shifts up to 1.
    assert_eq!(before_target_authoritative_insert(&auth, "a", 2), Some(1));
    // Backward drag: c(2) lands before a(0), no shift-up correction.
    assert_eq!(before_target_authoritative_insert(&auth, "c", 0), Some(0));
    // Adjacent forward move: a before b is the original position, letting Herdr accept a no-op.
    assert_eq!(before_target_authoritative_insert(&auth, "a", 1), Some(0));
    // Don't guess when dragged isn't in the authoritative list.
    assert_eq!(before_target_authoritative_insert(&auth, "zz", 1), None);
}

/// P10-1a budget invariant: attach seed ≤ deep-history cap ≤ local scrollback capacity.
/// seed == cap is deliberate (the Herdr `pane.read recent` server-side hard cap measured at
/// ≈999 lines, probed 2026-08-26 on protocol 20; out-of-window replay carries a separate
/// controller-without-barrier duplication risk, see 256f963). If Herdr later offers a paged
/// read window and this constant is relaxed, this assertion is the guardrail of "confirm
/// protocol paging before raising the cap".
/// P10-1a cap one-shot hint state machine: first touch of the cap while capped → show; leaving the cap → never again;
/// uncapped/untouched → no action.
/// P2-1 steering agent pane detection and display name chain.
#[test]
fn steering_agent_pane_detection_and_display_name() {
    use super::herdr::{Agent, Pane};
    use super::steering::{pane_is_agent, steering_agent_display_name};
    let plain = Pane {
        pane_id: "p1".into(),
        ..Default::default()
    };
    assert!(!pane_is_agent(&plain));
    let agent_pane = Pane {
        pane_id: "p2".into(),
        agent: Some("claude".into()),
        ..Default::default()
    };
    assert!(pane_is_agent(&agent_pane));
    // An empty agent string doesn't count (projection not filled).
    let blank = Pane {
        pane_id: "p3".into(),
        agent: Some(String::new()),
        ..Default::default()
    };
    assert!(!pane_is_agent(&blank));

    let agent = Agent {
        terminal_id: "t1".into(),
        pane_id: Some("p2".into()),
        agent: Some("claude".into()),
        title: Some("Fix login bug".into()),
        ..Default::default()
    };
    // Agent projection takes priority (display_agent → agent → name), otherwise fall back to pane.agent.
    assert_eq!(
        steering_agent_display_name(std::slice::from_ref(&agent), &agent_pane),
        "claude"
    );
    let codex = Agent {
        display_agent: Some("Codex".into()),
        ..agent
    };
    assert_eq!(steering_agent_display_name(&[codex], &agent_pane), "Codex");
    assert_eq!(
        steering_agent_display_name(&[], &agent_pane),
        "claude",
        "pane.agent fallback"
    );
}

#[test]
fn terminal_keystroke_blocked_covers_input_surfaces() {
    // P0 regression pin: when an input surface (steering composer / ⌘F find) is focused, keys must
    // be intercepted — GPUI still dispatches keys to observe_keystrokes after the Input's KeyBinding
    // actions; without interception, Enter/Backspace/Esc get encoded into the terminal
    // (double Enter, DEL, and ESC bytes leaking into the agent PTY).
    assert!(terminal_keystroke_blocked(true, false, false));
    assert!(terminal_keystroke_blocked(false, true, false));
    assert!(terminal_keystroke_blocked(false, false, true));
    assert!(!terminal_keystroke_blocked(false, false, false));
}

#[test]
fn successive_shortcut_generations_tombstone_retired_override_chords() {
    // P1-3: the GPUI keymap is append-only. Rebinding bookkeeping must cover the "successive
    // generations" scenario: after default X → override J → override K, J must be recognized as
    // stale for this generation and given a NoAction tombstone (otherwise the old action lingers
    // and J still fires).
    use crate::settings;
    use crate::shortcuts;

    let mut config = settings::ApplicationConfig::default();

    let installed_pairs =
        |config: &settings::ApplicationConfig| crate::dyn_shortcut_install(config).1;
    let active_chords = |config: &settings::ApplicationConfig| {
        shortcuts::resolved_bindings(&config.shortcuts)
            .into_iter()
            .filter(|binding| binding.id == "app.quit")
            .map(|binding| binding.chord)
            .collect::<Vec<_>>()
    };

    // Generation 0: default cmd-q.
    assert_eq!(active_chords(&config), vec!["cmd-q".to_string()]);
    let mut generation = installed_pairs(&config);

    // Generation 1: cmd-q → cmd-j (the default chord enters the mask set).
    config
        .shortcuts
        .overrides
        .insert("app.quit".to_string(), vec!["cmd-j".to_string()]);
    let next = installed_pairs(&config);
    assert_eq!(active_chords(&config), vec!["cmd-j".to_string()]);
    assert!(
        next.iter().any(|(chord, _)| chord == "cmd-q"),
        "retired default must be masked in its own generation"
    );
    generation.extend(next.iter().cloned());

    // Generation 2: cmd-j → cmd-k; the previous generation's active chord cmd-j must be judged stale,
    // suppressing the lingering action with a NoAction tombstone.
    config
        .shortcuts
        .overrides
        .insert("app.quit".to_string(), vec!["cmd-k".to_string()]);
    let next = installed_pairs(&config);
    assert_eq!(active_chords(&config), vec!["cmd-k".to_string()]);
    let stale: Vec<(String, shortcuts::ShortcutScope)> = generation
        .iter()
        .filter(|pair| !next.contains(pair))
        .cloned()
        .collect();
    assert!(
        stale.iter().any(|(chord, _)| chord == "cmd-j"),
        "previous-generation override must be tombstoned, got {stale:?}"
    );
    assert!(
        !stale.iter().any(|(chord, _)| chord == "cmd-k"),
        "current active chord must never be tombstoned"
    );
}

// --- SCT-04 recording-state rebind regression pins ---

/// Recording entry structural anchor: clicking the shortcut badge to enter recording must first
/// suppress all currently effective chords with trailing NoAction bindings (otherwise stray presses
/// during recording would trigger real actions); capture is then taken over by shell_input's observer branch.
#[test]
fn shortcuts_recording_entry_masks_resolved_bindings_with_noaction() {
    let source = include_str!("shortcuts_view.rs");
    let Some((_, after)) = source.split_once("pub(crate) fn begin_shortcut_recording") else {
        panic!("begin_shortcut_recording entry point missing from shortcuts_view.rs");
    };
    let Some((body, _)) = after.split_once("pub(crate) fn finish_shortcut_recording") else {
        panic!("finish_shortcut_recording must follow begin_shortcut_recording");
    };
    assert!(
        body.contains("self.shortcut_recording = Some(id);"),
        "entering recording must record the target shortcut id first"
    );
    assert!(
        body.contains("shortcuts::resolved_bindings(&self.config.shortcuts)"),
        "masks must derive from the live resolved bindings, not compile-time defaults"
    );
    assert!(
        body.contains("gpui::NoAction"),
        "recording entry must convert bindings into NoAction masks"
    );
    assert!(
        body.contains("cx.bind_keys(masks);"),
        "NoAction masks must be installed via cx.bind_keys before capturing keys"
    );
}

/// finish_shortcut_recording writes overrides using the `keystroke_chord(key).to_lowercase()`
/// format. Three representative shapes pin the normalization result:
/// modifier+letter → "cmd-shift-a", function key → "f5", bare key → "escape";
/// the output must be directly consumable by shortcuts::resolved_bindings / dyn_shortcut_install.
#[test]
fn recorded_chord_normalization_matches_override_registry_format() {
    use crepuscularity_gpui::{Keystroke, Modifiers};
    // Keystroke/MODIFIERS are plain structs; no GPUI App needed to construct them.
    let ks = |key: &str, platform: bool, shift: bool| Keystroke {
        key: key.to_string(),
        key_char: None,
        modifiers: Modifiers {
            control: false,
            alt: false,
            shift,
            platform,
            function: false,
        },
    };

    assert_eq!(
        crate::input::keystroke_chord(&ks("a", true, true)).to_lowercase(),
        "cmd-shift-a"
    );
    assert_eq!(
        crate::input::keystroke_chord(&ks("f5", false, false)).to_lowercase(),
        "f5"
    );
    assert_eq!(
        crate::input::keystroke_chord(&ks("escape", false, false)).to_lowercase(),
        "escape"
    );
}

#[test]
fn registry_shortcuts_all_resolve_to_runtime_actions() {
    // audit P1-3: every configurable command id in the Settings panel must resolve to a real
    // GPUI action; otherwise it's a black-hole shortcut with "an option but no implementation".
    for entry in crate::shortcuts::REGISTRY {
        assert!(
            super::registry_key_binding(entry.id, entry.default_chord, entry.scope.gpui_context())
                .is_some(),
            "registry id `{}` has no runtime action mapping",
            entry.id
        );
    }
}

#[test]
fn script_hotkey_beats_input_surface_and_never_routes_to_tui() {
    // audit P1-2: Script custom hotkeys must still fire while the host TUI is active;
    // only the Terminal route may encode bytes to the PTY.
    match route_shell_keystroke(Some("script-1"), false) {
        ShellKeyRoute::Script(id) => assert_eq!(id, "script-1"),
        other => panic!("expected script route, got {other:?}"),
    }
    // A focused input surface doesn't shadow script keys (review R2 P2 semantics).
    match route_shell_keystroke(Some("script-1"), true) {
        ShellKeyRoute::Script(id) => assert_eq!(id, "script-1"),
        other => panic!("expected script route, got {other:?}"),
    }
    assert_eq!(
        route_shell_keystroke::<&str>(None, true),
        ShellKeyRoute::InputSurface
    );
    assert_eq!(
        route_shell_keystroke::<&str>(None, false),
        ShellKeyRoute::Terminal
    );
}

#[test]
fn failed_agents_land_in_needs_attention_like_the_summary_counts() {
    // Audit A01: the status bar snapshot and the summary counts must classify through the
    // same attention_for_raw_status mapping — a failed agent is a NeedsAttention item (the
    // old raw `== "blocked"` comparison dropped it from the menu's attention group).
    let agents = vec![Agent {
        agent_status: Some("failed".to_string()),
        ..Agent::default()
    }];
    let summary = operational_summary(&agents, &ScriptRegistry::default());
    assert_eq!(summary.blocked_agents, 1);
    assert_eq!(summary.working_agents, 0);

    let attention = crate::status::agent_effective_status(&agents[0])
        .map(crate::status::attention_for_raw_status);
    assert_eq!(
        attention,
        Some(crate::status::AttentionLevel::NeedsAttention)
    );
    // The live label source shared by the status bar and the Header's Chat title (audit A20).
    assert_eq!(
        attention.map(|level| level.label()),
        Some("Needs Attention")
    );

    // starting/running count as Working, matching the summary's working bucket.
    assert_eq!(
        crate::status::attention_for_raw_status("running"),
        crate::status::AttentionLevel::Working
    );
    assert_eq!(
        crate::status::attention_for_raw_status("launch_pending"),
        crate::status::AttentionLevel::Working
    );
}

#[test]
fn resolved_theme_preset_is_the_single_auto_switch_aware_rule() {
    // Audit A18: one resolution rule for bootstrap, sync_app_theme_from_herdr, and theme().
    use crate::herdr_tui::HerdrUserConfigSnapshot;
    let herdr_theme = |preset: Option<crate::theme::ThemePreset>| preset.map(|p| p.herdr_theme);

    // Manual mode: theme_name is authoritative regardless of the dark flag.
    let manual = HerdrUserConfigSnapshot {
        theme_name: "one-dark".to_string(),
        ..HerdrUserConfigSnapshot::default()
    };
    assert_eq!(
        herdr_theme(resolved_theme_preset(&manual, false)),
        Some("one-dark")
    );
    assert_eq!(
        herdr_theme(resolved_theme_preset(&manual, true)),
        Some("one-dark")
    );

    // Auto switch: the per-appearance slot wins over the legacy theme_name.
    let auto = HerdrUserConfigSnapshot {
        theme_name: "catppuccin".to_string(),
        theme_auto_switch: true,
        theme_light_name: "catppuccin-latte".to_string(),
        theme_dark_name: "tokyo-night".to_string(),
        ..HerdrUserConfigSnapshot::default()
    };
    assert_eq!(
        herdr_theme(resolved_theme_preset(&auto, true)),
        Some("tokyo-night")
    );
    assert_eq!(
        herdr_theme(resolved_theme_preset(&auto, false)),
        Some("catppuccin-latte")
    );

    // An empty slot falls back to the legacy theme_name; an unknown slot name falls back too.
    let auto_empty_slots = HerdrUserConfigSnapshot {
        theme_auto_switch: true,
        ..HerdrUserConfigSnapshot::default()
    };
    assert_eq!(
        herdr_theme(resolved_theme_preset(&auto_empty_slots, true)),
        Some("catppuccin")
    );
    let auto_unknown_dark = HerdrUserConfigSnapshot {
        theme_auto_switch: true,
        theme_name: "vesper".to_string(),
        theme_dark_name: "not-a-preset".to_string(),
        ..HerdrUserConfigSnapshot::default()
    };
    assert_eq!(
        herdr_theme(resolved_theme_preset(&auto_unknown_dark, true)),
        Some("vesper")
    );
}

#[test]
fn settings_sidebar_orders_providers_before_skill_mobile_and_browser() {
    // Audit E24: Providers decides whether New Task/History can work at all, so it outranks
    // the Skill/Mobile/Browser surfaces in the Settings sidebar order (Skill is the
    // agent-facing installer and shares the agent-ecosystem tier).
    let all = super::SettingsSection::ALL;
    let providers = all
        .iter()
        .position(|s| *s == super::SettingsSection::Providers);
    let mobile = all
        .iter()
        .position(|s| *s == super::SettingsSection::Mobile);
    let browser = all
        .iter()
        .position(|s| *s == super::SettingsSection::Browser);
    let skill = all.iter().position(|s| *s == super::SettingsSection::Skill);
    assert!(
        providers
            .zip(skill)
            .zip(mobile)
            .zip(browser)
            .is_some_and(|(((p, s), m), b)| p < s && p < m && p < b),
        "Providers must be listed before Skill, Mobile, and Browser"
    );
}

#[test]
fn agent_switcher_chords_are_registered_configurable_and_swallow_aware() {
    // Audit E07: the Ctrl-Tab switcher family used to be internal-only bindings — not in the
    // registry, absent from help, and invisible to chord_is_bound's terminal swallow decision.
    for id in [
        "agent.switcher-next",
        "agent.switcher-prev",
        "agent.switcher-confirm",
        "agent.switcher-cancel",
    ] {
        let entry = crate::shortcuts::find_entry(id)
            .unwrap_or_else(|| panic!("{id} missing from the shortcut registry"));
        assert!(!entry.default_chord.is_empty(), "{id} has no default chord");
        // Confirm/Cancel must be scoped to the switcher overlay only; next/prev are App-scope.
        if id.ends_with("confirm") || id.ends_with("cancel") {
            assert_eq!(entry.scope, crate::shortcuts::ShortcutScope::AgentSwitcher);
        } else {
            assert_eq!(entry.scope, crate::shortcuts::ShortcutScope::App);
        }
    }
    let config = crate::shortcuts::ShortcutConfig::default();
    assert!(crate::shortcuts::chord_is_bound("ctrl-tab", &config));
    assert!(crate::shortcuts::chord_is_bound("ctrl-shift-tab", &config));
    // The registry-wide test `registry_shortcuts_all_resolve_to_runtime_actions` pins that
    // every id above resolves to its GPUI action (SwitchAgentNext/… mappings).
}

/// B16: the plan fast path may only replace the full-grid deep comparison when extraction
/// PROVED the rows byte-identical. An unchanged frame then compares equal without walking
/// cells (zero downstream row re-syncs); the same grid with a moved cursor must still
/// compare unequal so the frame is applied; without a plan nothing changes.
#[test]
fn terminal_frame_plan_replaces_deep_grid_compare_only_on_proven_rows() {
    use crate::ghostty::TerminalFramePlan;
    use crate::shell_terminal_stream::terminal_frames_equal_with_plan;

    let grid_line = || crate::ghostty::TerminalLine {
        cells: vec!["x".to_string(); 8],
        ..crate::ghostty::TerminalLine::default()
    };
    let frame = |cursor: Option<(u16, u16)>| crate::ghostty::TerminalFrame {
        lines: vec![grid_line(), grid_line(), grid_line()],
        cursor,
        ..crate::ghostty::TerminalFrame::default()
    };

    // Rows proven unchanged + identical scalars: equal — set_terminal_frame early-returns
    // and the pane is never touched.
    assert!(terminal_frames_equal_with_plan(
        &frame(None),
        &frame(None),
        Some(&TerminalFramePlan::RowsUnchanged)
    ));
    // Rows proven unchanged but the cursor moved: NOT equal — the frame must be applied
    // (no visible frame may be dropped that the deep comparison would have applied).
    assert!(!terminal_frames_equal_with_plan(
        &frame(None),
        &frame(Some((2, 1))),
        Some(&TerminalFramePlan::RowsUnchanged)
    ));
    // Unknown plan / no plan: the deep comparison runs exactly as before, both verdicts.
    assert!(terminal_frames_equal_with_plan(
        &frame(None),
        &frame(None),
        None
    ));
    assert!(!terminal_frames_equal_with_plan(
        &frame(None),
        &frame(Some((2, 1))),
        None
    ));
    assert!(!terminal_frames_equal_with_plan(
        &frame(None),
        &frame(Some((2, 1))),
        Some(&TerminalFramePlan::Unknown)
    ));
}
