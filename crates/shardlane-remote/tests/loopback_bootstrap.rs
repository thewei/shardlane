//! B3 integration tests: bootstrap/agents read paths on an isolated
//! Herdr instance.
//!
//! [INPUT]: Depends on tests/common (isolated Herdr harness + minimal HTTP
//! client)
//! [OUTPUT]: Verifies B3 acceptance: bootstrap contains a real
//! workspace/project/tab/pane, the Project-scoped semantic Conversation list
//! answers, retired v1 read routes (projects/{id}/agents,
//! agents/{ref}/output, agents/{ref}/prompt) 404 with the unified envelope,
//! Herdr down 503, and pane output parameter validation plus runtime error
//! mapping
//! [POS]: Automated acceptance of the B3 batch of remote-r6-implementation.md;
//! does not submit real agent prompts (quota discipline; positive prompts go
//! through B8 manual acceptance)

mod common;

use common::{http_get, spawn_against, IsolatedHerdr, TOKEN};

fn parse_json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap_or_else(|error| panic!("body not JSON ({error}): {body}"))
}

#[tokio::test]
async fn bootstrap_reflects_isolated_herdr_runtime() {
    if !common::herdr_available() {
        eprintln!(
            "skipping: herdr binary not found; set HERDR_BIN or install Herdr with wax to run loopback tests"
        );
        return;
    }
    let herdr = IsolatedHerdr::spawn();
    herdr.seed_workspace();
    let handle = spawn_against(&herdr);

    let (status, body) = http_get(handle.addr, "/api/v2/bootstrap", Some(TOKEN)).await;
    assert_eq!(status, 200, "bootstrap must succeed: {body}");
    let bootstrap = parse_json(&body);

    assert_eq!(bootstrap["host"]["host_id"], "shardlane-host-integration");
    assert_eq!(bootstrap["host"]["api_version"], 1);
    assert_eq!(bootstrap["capabilities"]["agent_control"], true);
    assert_eq!(bootstrap["capabilities"]["conversation_view"], true);
    assert_eq!(bootstrap["capabilities"]["herdr_tui"], true);

    let workspaces = bootstrap["workspaces"]
        .as_array()
        .unwrap_or_else(|| panic!("workspaces"));
    assert!(!workspaces.is_empty(), "default workspace must be present");
    assert_eq!(workspaces[0]["name"], "Default");
    assert_eq!(workspaces[0]["active"], true);

    let projects = bootstrap["projects"]
        .as_array()
        .unwrap_or_else(|| panic!("projects"));
    assert_eq!(
        projects.len(),
        1,
        "seeded runtime workspace must project once: {body}"
    );
    assert_eq!(projects[0]["runtime_available"], true);
    assert!(
        projects[0]["id"]
            .as_str()
            .unwrap_or_default()
            .starts_with("prj_1_"),
        "opaque ProjectId encoding, got: {body}"
    );
    assert!(
        projects[0]["project_path"]
            .as_str()
            .unwrap_or_default()
            .contains("shardlane"),
        "seeded cwd leaks into project path display: {}",
        projects[0]["project_path"]
    );

    let tabs = bootstrap["tabs"]
        .as_array()
        .unwrap_or_else(|| panic!("tabs"));
    assert_eq!(tabs.len(), 1);
    let panes = bootstrap["panes"]
        .as_array()
        .unwrap_or_else(|| panic!("panes"));
    assert!(!panes.is_empty(), "seeded tab must own at least one pane");
    assert!(
        bootstrap["agents"].as_array().is_some(),
        "agents array must exist (may be empty without started agents)"
    );

    // agents list: the Project-scoped semantic Conversation list (v1
    // /projects/{id}/agents was retired in the 2026-09-01 v1 clearance).
    let project_id = projects[0]["id"].as_str().unwrap_or_default().to_string();
    let (conversation_status, conversation_body) = http_get(
        handle.addr,
        &format!("/api/v2/projects/{project_id}/conversations"),
        Some(TOKEN),
    )
    .await;
    assert_eq!(
        conversation_status, 200,
        "semantic conversation list: {conversation_body}"
    );
    assert!(parse_json(&conversation_body)["conversations"]
        .as_array()
        .is_some());

    // Retired v1 read routes must answer the unified 404 envelope.
    let (status, body) = http_get(
        handle.addr,
        &format!("/api/v1/projects/{project_id}/agents"),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, 404, "retired v1 agents list must 404: {body}");
    assert_eq!(parse_json(&body)["error"]["code"], "not_found");

    // output: parameter validation now lives on the re-homed pane output
    // route (same lines bounds, identical envelope).
    let (status, body) = http_get(
        handle.addr,
        "/api/v2/panes/w1:p1/output?lines=99999",
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, 400, "out-of-range lines must 400: {body}");
    assert_eq!(parse_json(&body)["error"]["code"], "invalid_request");

    let (status, body) = http_get(
        handle.addr,
        "/api/v2/panes/w1:p1/output?lines=abc",
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, 400, "non-integer lines must 400: {body}");

    // The retired v1 agent output route (and its per-agent semantics) is
    // gone: the fallback 404 envelope, not a runtime read.
    let (status, body) = http_get(handle.addr, "/api/v1/agents/w1:p1/output", Some(TOKEN)).await;
    assert_eq!(status, 404, "retired v1 agent output must 404: {body}");
    assert_eq!(parse_json(&body)["error"]["code"], "not_found");

    handle.stop();
}

#[tokio::test]
async fn retired_v1_prompt_route_returns_envelope_404() {
    use common::http_request;

    if !common::herdr_available() {
        eprintln!(
            "skipping: herdr binary not found; set HERDR_BIN or install Herdr with wax to run loopback tests"
        );
        return;
    }
    let herdr = IsolatedHerdr::spawn();
    herdr.seed_workspace();
    let handle = spawn_against(&herdr);

    // The PTY-era v1 prompt endpoint (with its dead wait_until/timeout_ms
    // validation, audit D07) was deleted outright in the 2026-09-01 v1
    // clearance: any body, valid or not, gets the fallback 404 envelope.
    for payload in [
        r#"{"text":"continue"}"#,
        r#"{"text":"  "}"#,
        r#"{"text":"hi","wait_until":["exploded"]}"#,
        r#"{"text":"hi","timeout_ms":999999}"#,
        "{not json",
        r#"{"text":"hi","wait_until":["working","idle"],"timeout_ms":5000,"request_id":"abc-123"}"#,
    ] {
        let (status, body) = http_request(
            handle.addr,
            "POST",
            "/api/v1/agents/w1:p1/prompt",
            Some(TOKEN),
            Some(payload),
        )
        .await;
        assert_eq!(
            status, 404,
            "retired v1 prompt must 404 ({payload}): {body}"
        );
        assert_eq!(parse_json(&body)["error"]["code"], "not_found");
    }
    handle.stop();
}

#[tokio::test]
async fn herdr_down_yields_host_unavailable() {
    if !common::herdr_available() {
        eprintln!(
            "skipping: herdr binary not found; set HERDR_BIN or install Herdr with wax to run loopback tests"
        );
        return;
    }
    let herdr = IsolatedHerdr::spawn();
    // No seeding, and herdr killed immediately: the socket exists but no one
    // answers → host_unavailable.
    let handle = spawn_against(&herdr);
    drop(herdr);

    let (status, body) = http_get(handle.addr, "/api/v2/bootstrap", Some(TOKEN)).await;
    assert_eq!(status, 503, "dead herdr must be host_unavailable: {body}");
    assert_eq!(parse_json(&body)["error"]["code"], "host_unavailable");

    let (status, body) = http_get(handle.addr, "/api/v2/panes/w1:p1/output", Some(TOKEN)).await;
    assert_eq!(status, 503, "pane read with dead herdr: {body}");
    assert_eq!(parse_json(&body)["error"]["code"], "host_unavailable");
    handle.stop();
}
