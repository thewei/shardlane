//! B2 integration tests: loopback server skeleton (hello/auth/error
//! envelope).
//!
//! [INPUT]: Depends on the crate public API and a tokio TcpStream minimal
//! HTTP client (zero extra dependencies)
//! [OUTPUT]: Verifies B2 acceptance: no token 401, wrong token 401, correct
//! token 200 hello shape assertions, unknown endpoint 404 envelope, and
//! spawn rejection for a disabled config
//! [POS]: Automated acceptance of the B2 batch of remote-r6-implementation.md;
//! extended from B3 on with bootstrap etc.

use shardlane_remote::config::{ListenerMode, RemoteConfig};
use shardlane_remote::{spawn_remote_server, RemoteServerOptions};
use std::path::PathBuf;

fn test_config(port: u16) -> RemoteConfig {
    RemoteConfig {
        enabled: true,
        listener_mode: ListenerMode::Loopback,
        port,
        host_id: Some("shardlane-host-test".into()),
        access_token: Some("unit-test-token-0123456789abcdef".into()),
        web_url: None,
    }
}

fn spawn(port: u16) -> shardlane_remote::RemoteServerHandle {
    let options = RemoteServerOptions {
        conversation_sessions: None,
        shared_tui: None,
        delivery_coordinator: None,
        config: test_config(port),
        host_name: "test-mac".into(),
        host_version: "0.1.11".into(),
        settings_path: PathBuf::from("/nonexistent/settings.json"),
        herdr_socket_override: None,
        web_bundle_path: None,
    };
    spawn_remote_server(options).unwrap_or_else(|error| panic!("spawn failed: {error}"))
}

/// Minimal HTTP/1.1 GET (loopback, no TLS, no chunked — axum returns
/// content-length for small JSON). Returns (status, body).
async fn http_get(addr: std::net::SocketAddr, path: &str, token: Option<&str>) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    let auth = match token {
        Some(token) => format!("Authorization: Bearer {token}\r\n"),
        None => String::new(),
    };
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Connection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .unwrap_or_else(|e| panic!("write: {e}"));
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .unwrap_or_else(|e| panic!("read: {e}"));
    let text = String::from_utf8_lossy(&raw).to_string();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("malformed response: {text}"));
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    (status, body)
}

/// Minimal HTTP/1.1 POST with a JSON body (loopback, no TLS). Returns
/// (status, body).
async fn http_post(
    addr: std::net::SocketAddr,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    let auth = match token {
        Some(token) => format!("Authorization: Bearer {token}\r\n"),
        None => String::new(),
    };
    let body = body.unwrap_or_default();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .unwrap_or_else(|e| panic!("write: {e}"));
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .unwrap_or_else(|e| panic!("read: {e}"));
    let text = String::from_utf8_lossy(&raw).to_string();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("malformed response: {text}"));
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    (status, body)
}

#[tokio::test]
async fn hello_requires_bearer_token() {
    let handle = spawn(0);
    let (status, body) = http_get(handle.addr, "/api/v2/hello", None).await;
    assert_eq!(status, 401, "no token must be rejected: {body}");
    let envelope: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("envelope not JSON: {e}"));
    assert_eq!(envelope["error"]["code"], "unauthorized");
    assert!(envelope["error"]["request_id"].as_str().is_some());
    assert!(envelope["error"]["message"].as_str().is_some());

    let (status, body) = http_get(handle.addr, "/api/v2/hello", Some("wrong-token")).await;
    assert_eq!(status, 401, "wrong token must be rejected: {body}");
    handle.stop();
}

#[tokio::test]
async fn hello_returns_contract_shape_with_valid_token() {
    let handle = spawn(0);
    let (status, body) = http_get(
        handle.addr,
        "/api/v2/hello",
        Some("unit-test-token-0123456789abcdef"),
    )
    .await;
    assert_eq!(status, 200, "valid token must pass: {body}");
    let hello: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("hello not JSON: {e}"));
    assert_eq!(hello["product"], "shardlane");
    assert_eq!(hello["remote_api_version"], 3);
    assert_eq!(hello["min_mobile_api_version"], 2);
    assert_eq!(hello["host"]["host_id"], "shardlane-host-test");
    assert_eq!(hello["host"]["name"], "test-mac");
    assert_eq!(hello["host"]["version"], "0.1.11");
    assert_eq!(hello["host"]["api_version"], 1);
    assert_eq!(hello["capabilities"]["agent_control"], true);
    assert_eq!(hello["capabilities"]["scripts"], false);
    assert_eq!(hello["capabilities"]["terminal_text"], true);
    assert_eq!(hello["capabilities"]["terminal_stream"], true);
    assert_eq!(hello["capabilities"]["conversation_view"], true);
    assert_eq!(hello["capabilities"]["conversation_live"], true);
    assert_eq!(hello["capabilities"]["history_continue"], true);
    assert_eq!(hello["capabilities"]["herdr_tui"], true);
    handle.stop();
}

#[tokio::test]
async fn retired_v1_routes_return_envelope_404() {
    // v1 API clearance (2026-09-01): the retired /api/v1 surface must answer
    // with the unified 404 envelope, not a route match.
    let handle = spawn(0);
    let token = Some("unit-test-token-0123456789abcdef");
    for path in [
        "/api/v1/hello",
        "/api/v1/bootstrap",
        "/api/v1/events",
        "/api/v1/projects/prj_1_x/agents",
        "/api/v1/agents/w1:p1/output",
        "/api/v1/agents/w1:p1/prompt",
        "/api/v1/agents/start",
        "/api/v1/agents/w1:p1/keys",
        "/api/v1/workspaces",
        "/api/v1/workspaces/w1/rename",
        "/api/v1/workspaces/w1/move",
        "/api/v1/tabs",
        "/api/v1/tabs/w1:t1",
        "/api/v1/panes/w1:p1",
        "/api/v1/panes/w1:p1/split",
        "/api/v1/panes/w1:p1/output",
        "/api/v1/panes/w1:p1/agent",
        "/api/v1/history/sessions",
        "/api/v1/history/sessions/some-key",
    ] {
        let (status, body) = http_get(handle.addr, path, token).await;
        assert_eq!(status, 404, "retired v1 GET {path} must 404: {body}");
        let envelope: serde_json::Value =
            serde_json::from_str(&body).unwrap_or_else(|e| panic!("envelope not JSON: {e}"));
        assert_eq!(envelope["error"]["code"], "not_found", "{path}: {body}");
    }
    // Deleted v1 POST routes: the path no longer matches anything, so the
    // method-specific 405 cannot appear — only the fallback 404.
    let (status, body) = http_post(
        handle.addr,
        "/api/v1/agents/start",
        token,
        Some(r#"{"name":"a","kind":"pi","pane_id":"w1:p1"}"#),
    )
    .await;
    assert_eq!(status, 404, "retired v1 POST must 404: {body}");
    handle.stop();
}

/// D12: every API response carries `x-request-id`; on error envelopes it
/// equals the body's request_id.
#[tokio::test]
async fn responses_carry_x_request_id_header() {
    let handle = spawn(0);
    let token = "unit-test-token-0123456789abcdef";
    for path in ["/api/v2/hello", "/api/v1/nope"] {
        let (status, body) = http_get(handle.addr, path, Some(token)).await;
        assert!(status == 200 || status == 404, "{path}: {body}");
        // The minimal client cannot read headers; re-issue over the raw
        // helper and inspect the head.
        let raw = http_get_raw(handle.addr, path, Some(token)).await;
        let (head, raw_body) = raw
            .split_once("\r\n\r\n")
            .unwrap_or_else(|| panic!("malformed response: {raw}"));
        let header = head
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                (key.trim().to_lowercase() == "x-request-id").then(|| value.trim().to_string())
            })
            .unwrap_or_else(|| panic!("{path} must carry x-request-id: {head}"));
        assert!(!header.is_empty(), "{path}: empty x-request-id");
        if status == 404 {
            let envelope: serde_json::Value =
                serde_json::from_str(raw_body).unwrap_or_else(|e| panic!("envelope not JSON: {e}"));
            assert_eq!(
                envelope["error"]["request_id"], header,
                "envelope id must equal the header: {raw_body}"
            );
        }
    }
    handle.stop();
}

/// Raw HTTP/1.1 GET returning the full response text (headers + body).
async fn http_get_raw(addr: std::net::SocketAddr, path: &str, token: Option<&str>) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    let auth = match token {
        Some(token) => format!("Authorization: Bearer {token}\r\n"),
        None => String::new(),
    };
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n{auth}Connection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .unwrap_or_else(|e| panic!("write: {e}"));
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .unwrap_or_else(|e| panic!("read: {e}"));
    String::from_utf8_lossy(&raw).to_string()
}

#[tokio::test]
async fn v2_hello_is_the_same_authenticated_contract() {
    let handle = spawn(0);
    let (status, body) = http_get(
        handle.addr,
        "/api/v2/hello",
        Some("unit-test-token-0123456789abcdef"),
    )
    .await;
    assert_eq!(status, 200, "v2 hello must pass: {body}");
    let hello: serde_json::Value = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(error) => panic!("v2 hello JSON: {error}"),
    };
    assert_eq!(hello["remote_api_version"], 3);
    assert_eq!(hello["capabilities"]["conversation_view"], true);
    assert_eq!(hello["capabilities"]["herdr_tui"], true);
    handle.stop();
}

#[tokio::test]
async fn shared_tui_route_is_authenticated_and_bounded_to_one_session_id() {
    let handle = spawn(0);
    let (status, body) = http_get(handle.addr, "/api/v2/tui/session", None).await;
    assert_eq!(status, 401, "TUI route must require bearer auth: {body}");

    let (status, body) = http_get(
        handle.addr,
        "/api/v2/tui/session/unknown",
        Some("unit-test-token-0123456789abcdef"),
    )
    .await;
    assert_eq!(
        status, 404,
        "unknown TUI id must not spawn a process: {body}"
    );

    // D23: only live stream sockets may register in the web-connection
    // registry — a rejected stream (unknown session) registers nothing. The
    // live register/touch/drop lifecycle is exercised for the shared
    // registry by the events-socket test in loopback_events.rs.
    assert!(
        handle.web_connections().is_empty(),
        "failed stream upgrades must not appear in the pairing list"
    );
    handle.stop();
}

#[tokio::test]
async fn unknown_endpoint_returns_envelope_404() {
    let handle = spawn(0);
    let (status, body) = http_get(
        handle.addr,
        "/api/v1/nope",
        Some("unit-test-token-0123456789abcdef"),
    )
    .await;
    assert_eq!(status, 404, "unknown path must 404: {body}");
    let envelope: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("envelope not JSON: {e}"));
    assert_eq!(envelope["error"]["code"], "not_found");
    handle.stop();
}

#[tokio::test]
async fn disabled_config_refuses_to_spawn() {
    let mut config = test_config(0);
    config.enabled = false;
    let error = match spawn_remote_server(RemoteServerOptions {
        conversation_sessions: None,
        shared_tui: None,
        delivery_coordinator: None,
        config,
        host_name: "test-mac".into(),
        host_version: "0.1.11".into(),
        settings_path: PathBuf::from("/nonexistent/settings.json"),
        herdr_socket_override: None,
        web_bundle_path: None,
    }) {
        Ok(_) => panic!("disabled config must refuse spawn"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        shardlane_remote::RemoteServerError::NotEnabled
    ));

    let mut tokenless = test_config(0);
    tokenless.access_token = None;
    let error = match spawn_remote_server(RemoteServerOptions {
        conversation_sessions: None,
        shared_tui: None,
        delivery_coordinator: None,
        config: tokenless,
        host_name: "test-mac".into(),
        host_version: "0.1.11".into(),
        settings_path: PathBuf::from("/nonexistent/settings.json"),
        herdr_socket_override: None,
        web_bundle_path: None,
    }) {
        Ok(_) => panic!("tokenless config must refuse spawn"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        shardlane_remote::RemoteServerError::MissingCredentials
    ));
}

#[tokio::test]
async fn bind_failure_reports_typed_bind_error() {
    // D06: a bind failure (port occupied) must surface as
    // RemoteServerError::Bind — classified at the bind site via the typed
    // ready-channel outcome, not by sniffing the startup message text.
    let blocker =
        std::net::TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| panic!("blocker bind: {e}"));
    let port = blocker
        .local_addr()
        .unwrap_or_else(|e| panic!("blocker local addr: {e}"))
        .port();
    let error = match spawn_remote_server(RemoteServerOptions {
        conversation_sessions: None,
        shared_tui: None,
        delivery_coordinator: None,
        config: test_config(port),
        host_name: "test-mac".into(),
        host_version: "0.1.11".into(),
        settings_path: PathBuf::from("/nonexistent/settings.json"),
        herdr_socket_override: None,
        web_bundle_path: None,
    }) {
        Ok(handle) => {
            handle.stop();
            panic!("spawn on occupied port {port} must fail");
        }
        Err(error) => error,
    };
    assert!(
        matches!(error, shardlane_remote::RemoteServerError::Bind(_)),
        "occupied port must map to Bind, got: {error}"
    );
    drop(blocker);
}

#[tokio::test]
async fn global_history_search_answers_without_herdr_runtime() {
    // D04: a global search (no project_id) must not build the Host bootstrap,
    // so it answers from the local history catalog even with no Herdr runtime
    // reachable; the project-scoped search still requires it and fails
    // closed with host_unavailable.
    let settings = tempfile::Builder::new()
        .prefix("shardlane-remote-search-")
        .tempdir_in("/tmp")
        .unwrap_or_else(|e| panic!("tempdir: {e}"));
    let settings_path = settings.path().join("settings.json");
    std::fs::write(&settings_path, "{}").unwrap_or_else(|e| panic!("seed settings: {e}"));
    let options = RemoteServerOptions {
        conversation_sessions: None,
        shared_tui: None,
        delivery_coordinator: None,
        config: test_config(0),
        host_name: "test-mac".into(),
        host_version: "0.1.11".into(),
        settings_path,
        herdr_socket_override: Some(PathBuf::from("/tmp/shardlane-remote-no-such.sock")),
        web_bundle_path: None,
    };
    let handle =
        spawn_remote_server(options).unwrap_or_else(|error| panic!("spawn failed: {error}"));
    let token = "unit-test-token-0123456789abcdef";

    let (status, body) = http_get(
        handle.addr,
        "/api/v2/history/search?q=anything",
        Some(token),
    )
    .await;
    assert_eq!(
        status, 200,
        "global search must not require the Herdr runtime: {body}"
    );
    let page: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("page not JSON: {e}"));
    assert!(page["conversations"].as_array().is_some());
    assert_eq!(page["next_cursor"], serde_json::Value::Null);

    let (status, body) = http_get(
        handle.addr,
        "/api/v2/history/search?q=anything&project_id=prj_1_bogus",
        Some(token),
    )
    .await;
    assert_eq!(
        status, 503,
        "project-scoped search still needs the bootstrap: {body}"
    );
    let envelope: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("envelope not JSON: {e}"));
    assert_eq!(envelope["error"]["code"], "host_unavailable");
    handle.stop();
}

#[tokio::test]
async fn interaction_resolve_rejects_malformed_body_with_envelope() {
    // D13: the interaction handler must decode through the shared
    // Result<Json<T>, JsonRejection> pattern, so a malformed body gets the
    // unified ApiError envelope (400 invalid_request) instead of axum's bare
    // text 422 rejection.
    let handle = spawn(0);
    let (status, body) = http_post(
        handle.addr,
        "/api/v2/conversations/conv_1_x/interactions/resolve",
        Some("unit-test-token-0123456789abcdef"),
        Some("{not json"),
    )
    .await;
    assert_eq!(status, 400, "malformed body must be our envelope: {body}");
    let envelope: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|e| panic!("envelope not JSON: {e}"));
    assert_eq!(envelope["error"]["code"], "invalid_request");
    handle.stop();
}
