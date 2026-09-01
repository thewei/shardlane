//! Web/PWA development integration tests: the CORS allowlist + WS
//! subprotocol auth.
//!
//! [INPUT]: Depends on tests/common (isolated Herdr harness + minimal HTTP
//! client) and tokio-tungstenite (a subprotocol WS client)
//! [OUTPUT]: Verifies the Web gap fixes — preflight short-circuit for
//! allowlisted origins, ACAO echo on ordinary responses, no headers for
//! non-allowlisted origins, and browser-shaped requests (no Authorization
//! header) completing WS auth via Sec-WebSocket-Protocol with the server
//! echoing the selected protocol
//! [POS]: Automated acceptance of the remote-r6 Web development batch
//! (web-pwa-architecture.md §4 dev allowlist)

mod common;

use common::{spawn_against_with, IsolatedHerdr, TOKEN};
use futures_util::StreamExt;
use std::time::Duration;

/// Extract a header value from raw response text (lowercase key match).
fn header_value(raw: &str, name: &str) -> Option<String> {
    let headers = raw.split_once("\r\n\r\n").map(|(head, _)| head)?;
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim().to_lowercase() == name).then(|| value.trim().to_string())
    })
}

#[tokio::test]
async fn preflight_short_circuits_before_auth_with_cors_headers() {
    if !common::herdr_available() {
        eprintln!(
            "skipping: herdr binary not found; set HERDR_BIN or install Herdr with wax to run loopback tests"
        );
        return;
    }
    let herdr = IsolatedHerdr::spawn();
    herdr.seed_workspace();
    let handle = spawn_against_with(&herdr, "shardlane-host-web", "web-test-mac");

    // Browser preflight: no Authorization (per spec), must be 204 + ACAO.
    let (status, raw) = http_request_full(
        handle.addr,
        "OPTIONS",
        "/api/v2/bootstrap",
        Some("http://localhost:8081"),
        None,
    )
    .await;
    assert_eq!(status, 204, "preflight must short-circuit: {raw}");
    assert_eq!(
        header_value(&raw, "access-control-allow-origin").as_deref(),
        Some("http://localhost:8081")
    );
    assert!(raw.to_lowercase().contains("authorization"));

    // Preflight from a non-allowlisted origin: no CORS headers (the browser
    // blocks it), no short-circuit.
    let (status, raw) = http_request_full(
        handle.addr,
        "OPTIONS",
        "/api/v2/bootstrap",
        Some("https://evil.example.com"),
        None,
    )
    .await;
    assert_ne!(
        status, 204,
        "non-whitelisted preflight must not pass: {raw}"
    );
    assert_eq!(header_value(&raw, "access-control-allow-origin"), None);

    handle.stop();
}

#[tokio::test]
async fn whitelisted_origin_gets_acao_on_normal_responses() {
    if !common::herdr_available() {
        eprintln!(
            "skipping: herdr binary not found; set HERDR_BIN or install Herdr with wax to run loopback tests"
        );
        return;
    }
    let herdr = IsolatedHerdr::spawn();
    herdr.seed_workspace();
    let handle = spawn_against_with(&herdr, "shardlane-host-web", "web-test-mac");

    let (status, raw) = http_request_full(
        handle.addr,
        "GET",
        "/api/v2/hello",
        Some("http://127.0.0.1:8090"),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, 200, "{raw}");
    assert_eq!(
        header_value(&raw, "access-control-allow-origin").as_deref(),
        Some("http://127.0.0.1:8090")
    );

    // Non-allowlisted: the response succeeds (the token was carried) but
    // without ACAO — a browser reading it would still be blocked.
    let (status, raw) = http_request_full(
        handle.addr,
        "GET",
        "/api/v2/hello",
        Some("http://192.168.1.5:8090"),
        Some(TOKEN),
    )
    .await;
    assert_eq!(status, 200, "{raw}");
    assert_eq!(header_value(&raw, "access-control-allow-origin"), None);

    handle.stop();
}

#[tokio::test]
async fn browser_ws_authenticates_via_subprotocol_and_echoes_selection() {
    if !common::herdr_available() {
        eprintln!(
            "skipping: herdr binary not found; set HERDR_BIN or install Herdr with wax to run loopback tests"
        );
        return;
    }
    let herdr = IsolatedHerdr::spawn();
    herdr.seed_workspace();
    let handle = spawn_against_with(&herdr, "shardlane-host-web", "web-test-mac");

    // Browser shape: no Authorization header, credentials travel in the
    // Sec-WebSocket-Protocol pair.
    let url = format!("ws://{}/api/v2/events", handle.addr);
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = url
        .as_str()
        .into_client_request()
        .unwrap_or_else(|error| panic!("build request: {error}"));
    if let Ok(value) = format!("shardlane.bearer, {TOKEN}").parse() {
        request
            .headers_mut()
            .insert("sec-websocket-protocol", value);
    }
    let (mut stream, response) = tokio_tungstenite::connect_async(request)
        .await
        .unwrap_or_else(|e| panic!("ws: {e}"));

    // The server must echo the selected subprotocol (a browser
    // compatibility requirement).
    assert_eq!(
        response
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|value| value.to_str().ok()),
        Some("shardlane.bearer")
    );

    let ready = match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
        Ok(Some(Ok(frame))) => frame,
        Ok(Some(Err(error))) => panic!("ready frame error: {error}"),
        Ok(None) => panic!("stream closed before ready"),
        Err(_) => panic!("ready timeout"),
    };
    let ready = ready.into_text().unwrap_or_default();
    assert!(ready.contains(r#""type":"ready""#), "{ready}");

    // A wrong-token subprotocol pair → 401 (handshake failure).
    let mut bad = url
        .as_str()
        .into_client_request()
        .unwrap_or_else(|error| panic!("build bad request: {error}"));
    if let Ok(value) = "shardlane.bearer, wrong-token".parse() {
        bad.headers_mut().insert("sec-websocket-protocol", value);
    }
    let rejected = tokio_tungstenite::connect_async(bad).await;
    assert!(
        rejected.is_err(),
        "wrong subprotocol token must be rejected"
    );

    handle.stop();
}

/// Minimal raw HTTP response (headers need reading; common::http_request only
/// returns the body).
async fn http_request_full(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    origin: Option<&str>,
    token: Option<&str>,
) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    let mut extra = String::new();
    if let Some(origin) = origin {
        extra.push_str(&format!("Origin: {origin}\r\n"));
        if method == "OPTIONS" {
            extra.push_str("Access-Control-Request-Method: GET\r\n");
        }
    }
    if let Some(token) = token {
        extra.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    let request =
        format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n{extra}Connection: close\r\n\r\n");
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
    (status, text)
}
