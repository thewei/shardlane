//! B3 golden fixtures: Rust DTO serialization ↔ fixture files ↔ mobile Zod
//! — a three-way pinned contract.
//!
//! [INPUT]: Depends on shardlane-remote/shardlane-host public DTOs and
//! tests/fixtures/*.json
//! [OUTPUT]: Verifies the wire format of hello/bootstrap/agent_output and the
//! semantic Conversation fixtures is byte-for-byte stable; with
//! UPDATE_FIXTURES=1 the fixtures are rewritten (use when changing the
//! contract intentionally)
//! [POS]: Contract pins of remote-r6-implementation.md B3 and mobile
//! architecture convergence Batch 4; herdr-mobile copies the same fixtures
//! to run Zod parsing tests

use shardlane_host::{
    conversation_id_for_history_key, project_id_for_path, project_id_for_runtime_workspace,
    AgentOutput, AgentRef, AgentStatus, AgentSummary, ConversationIdentity, ConversationItem,
    ConversationItemKind, ConversationSource, ConversationSummary, ConversationToolCall,
    ConversationWindow, HerdrTuiInput, HerdrTuiKeyCode, HerdrTuiMode, HerdrTuiModifiers,
    HerdrTuiSessionStatus, HerdrTuiSessionSummary, HostCapabilities, HostInfo, PaneId, PaneSummary,
    ProjectSummary, TabId, TabSummary, WorkspaceId, WorkspaceSummary, HOST_API_VERSION,
};
use shardlane_remote::{hello_response, RemoteConfig, RemoteState};
use std::path::PathBuf;

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Normalize object keys before rendering so the fixture wire text is stable
/// whether another workspace crate enables serde_json's `preserve_order`.
fn canonical_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            let mut ordered = serde_json::Map::new();
            for (key, value) in entries {
                ordered.insert(key, canonical_json(value));
            }
            serde_json::Value::Object(ordered)
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(canonical_json).collect())
        }
        other => other,
    }
}

fn assert_fixture(name: &str, value: &impl serde::Serialize) {
    let json = serde_json::to_value(value)
        .map(canonical_json)
        .and_then(|value| serde_json::to_string_pretty(&value))
        .unwrap_or_else(|error| panic!("serialize {name}: {error}"));
    let json = format!("{json}\n");
    let path = fixture_path(name);
    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &json).unwrap_or_else(|error| panic!("write {name}: {error}"));
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("read {name} (run with UPDATE_FIXTURES=1 to create): {error}")
    });
    assert_eq!(json, expected, "wire contract drifted for {name}");
}

fn fixture_state() -> RemoteState {
    let config = RemoteConfig {
        host_id: Some("shardlane-host-fixture".into()),
        access_token: Some("fixture-token".into()),
        web_url: None,
        ..RemoteConfig::default()
    };
    RemoteState::new(
        config,
        "Test Mac".into(),
        "0.1.11".into(),
        PathBuf::from("/nonexistent/settings.json"),
        None,
    )
}

#[test]
fn hello_fixture_is_stable() {
    assert_fixture("hello.json", &hello_response(&fixture_state()));
}

#[test]
fn shared_tui_session_fixture_is_stable() {
    assert_fixture(
        "tui_session.json",
        &HerdrTuiSessionSummary {
            id: "tui-fixture-shared".into(),
            mode: HerdrTuiMode::Shared,
            status: HerdrTuiSessionStatus::Running,
            cols: 120,
            rows: 32,
            revision: 3,
        },
    );
}

#[test]
fn shared_tui_input_fixture_is_stable() {
    assert_fixture(
        "tui_input.json",
        &HerdrTuiInput::Key {
            code: HerdrTuiKeyCode::ArrowUp,
            modifiers: HerdrTuiModifiers {
                ctrl: true,
                ..HerdrTuiModifiers::default()
            },
        },
    );
}

/// Letter keys are the semantic-key path for native/mobile terminal control
/// shortcuts (Ctrl+C interrupt, Ctrl+D EOF); the wire spelling must stay
/// pinned for the mobile Zod enum.
#[test]
fn shared_tui_letter_input_fixture_is_stable() {
    assert_fixture(
        "tui_input_letter.json",
        &HerdrTuiInput::Key {
            code: HerdrTuiKeyCode::KeyC,
            modifiers: HerdrTuiModifiers {
                ctrl: true,
                ..HerdrTuiModifiers::default()
            },
        },
    );
}

#[test]
fn bootstrap_fixture_is_stable() {
    let project_id = project_id_for_runtime_workspace("w1");
    let bootstrap = shardlane_host::HostBootstrap {
        host: HostInfo {
            host_id: "shardlane-host-fixture".into(),
            name: "Test Mac".into(),
            version: "0.1.11".into(),
            api_version: HOST_API_VERSION,
        },
        capabilities: HostCapabilities {
            agent_control: true,
            scripts: false,
            history: true,
            terminal_text: true,
            terminal_stream: true,
            conversation_view: true,
            conversation_live: true,
            history_continue: true,
            herdr_tui: true,
        },
        workspaces: vec![WorkspaceSummary {
            id: WorkspaceId::new("workspace-main"),
            name: "Default".into(),
            color: "#7C8CFF".into(),
            active: true,
            project_ids: vec![project_id.clone()],
        }],
        projects: vec![ProjectSummary {
            id: project_id.clone(),
            workspace_id: WorkspaceId::new("workspace-main"),
            label: "herdr-client".into(),
            project_path: Some("/tmp/work/herdr-client".into()),
            runtime_available: true,
            tab_ids: vec![TabId::new("w1:t1")],
            agent_refs: vec![AgentRef::new("w1:p1")],
        }],
        tabs: vec![TabSummary {
            id: TabId::new("w1:t1"),
            project_id: project_id.clone(),
            label: Some("1".into()),
            title: None,
            pane_ids: vec![PaneId::new("w1:p1")],
        }],
        panes: vec![PaneSummary {
            id: PaneId::new("w1:p1"),
            tab_id: TabId::new("w1:t1"),
            title: None,
            cwd: Some("/tmp/work/herdr-client".into()),
            agent_ref: Some(AgentRef::new("w1:p1")),
        }],
        agents: vec![AgentSummary {
            id: AgentRef::new("w1:p1"),
            project_id: project_id.clone(),
            tab_id: TabId::new("w1:t1"),
            pane_id: PaneId::new("w1:p1"),
            name: Some("pi-main".into()),
            kind: Some("pi".into()),
            title: None,
            status: AgentStatus::Working,
            conversation_id: Some(shardlane_host::conversation_id_for_live_agent(
                &AgentRef::new("w1:p1"),
            )),
            revision: 3,
        }],
        conversations: vec![ConversationSummary {
            id: shardlane_host::conversation_id_for_live_agent(&AgentRef::new("w1:p1")),
            project_id: project_id.clone(),
            source: ConversationSource::Live,
            agent_kind: "pi".into(),
            title: "pi-main".into(),
            status: Some(AgentStatus::Working),
            sendable: true,
            live_agent_ref: Some(AgentRef::new("w1:p1")),
            updated_at_ms: None,
            revision: 3,
        }],
    };
    assert_fixture("bootstrap.json", &bootstrap);
}

#[test]
fn agent_output_fixture_is_stable() {
    let output = AgentOutput {
        agent_id: AgentRef::new("w1:p1"),
        text: "…recent agent output…".into(),
        revision: 42,
        truncated: false,
    };
    assert_fixture("agent_output.json", &output);
}

fn semantic_window(id: shardlane_host::ConversationId) -> ConversationWindow {
    ConversationWindow {
        conversation_id: id,
        revision: 9,
        items: vec![
            ConversationItem {
                id: "item-1".into(),
                seq: 1,
                kind: ConversationItemKind::User,
                role: Some("user".into()),
                text: "Inspect the failing test".into(),
                thinking: None,
                tool_calls: Vec::new(),
                timestamp_ms: Some(1_700_000_000_000),
                model: None,
                truncated: false,
            },
            ConversationItem {
                id: "item-2".into(),
                seq: 2,
                kind: ConversationItemKind::Tool,
                role: Some("assistant".into()),
                text: "cargo test".into(),
                thinking: Some("I will inspect the test output".into()),
                tool_calls: vec![ConversationToolCall {
                    id: "tool-1".into(),
                    name: "shell".into(),
                    input_preview: "cargo test".into(),
                    input: None,
                    output: Some("ok".into()),
                    is_error: false,
                }],
                timestamp_ms: Some(1_700_000_000_100),
                model: Some("claude-sonnet".into()),
                truncated: false,
            },
        ],
        first_seq: Some(1),
        last_seq: Some(2),
        has_older: false,
        has_newer: true,
    }
}

#[test]
fn semantic_conversation_fixtures_are_stable() {
    let project_id = project_id_for_path("/tmp/work/demo");
    let live_id = shardlane_host::conversation_id_for_live_agent(&AgentRef::new("pane-1"));
    let live_summary = ConversationSummary {
        id: live_id.clone(),
        project_id: project_id.clone(),
        source: ConversationSource::Live,
        agent_kind: "claude".into(),
        title: "Demo agent".into(),
        status: Some(AgentStatus::Working),
        sendable: true,
        live_agent_ref: Some(AgentRef::new("pane-1")),
        updated_at_ms: None,
        revision: 9,
    };
    assert_fixture(
        "conversation_live.json",
        &serde_json::json!({
            "conversation": live_summary,
            "window": semantic_window(live_id),
        }),
    );

    let history_id = conversation_id_for_history_key("claude-code:session-1");
    let history_summary = ConversationSummary {
        id: history_id.clone(),
        project_id,
        source: ConversationSource::History,
        agent_kind: "claude-code".into(),
        title: "Prior session".into(),
        status: None,
        sendable: false,
        live_agent_ref: None,
        updated_at_ms: Some(1_700_000_000_000),
        revision: 0,
    };
    assert_fixture(
        "conversation_history.json",
        &serde_json::json!({
            "conversation": history_summary,
            "window": semantic_window(history_id),
        }),
    );

    // M6: the continue route answers with the canonical planner result —
    // strategy + exact identity + optional launched target — instead of the
    // old mutation-only contract.
    let continue_response = shardlane_remote::conversations::ConversationContinueResponse {
        accepted: true,
        strategy: shardlane_host::ContinuationStrategy::NativeResume,
        identity: ConversationIdentity {
            conversation_id: shardlane_host::conversation_id_for_live_agent(&AgentRef::new(
                "pane-1",
            )),
            agent_ref: AgentRef::new("pane-1"),
            provider: "claude".into(),
            native_session_id: Some("native-1".into()),
            revision: 10,
        },
        instruction: None,
        launched: Some(shardlane_remote::conversations::LaunchedTarget {
            workspace_id: "workspace-1".into(),
            tab_id: "tab-1".into(),
            pane_id: "pane-1".into(),
        }),
    };
    assert_fixture("conversation_continue.json", &continue_response);
    assert_fixture(
        "conversation_error.json",
        &serde_json::json!({
            "error": {
                "code": "runtime_unavailable",
                "message": "conversation source is unavailable",
                "request_id": "req-fixture"
            }
        }),
    );
}
