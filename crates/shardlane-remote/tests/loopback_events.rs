//! B5 integration tests: the events WebSocket (ready/events/ping-pong/auth).
//!
//! [INPUT]: Depends on tests/common (isolated Herdr harness) and
//! tokio-tungstenite (the WS client)
//! [OUTPUT]: Verifies B5 acceptance: ready upon connect, structural change
//! event broadcast, application-level ping/pong, and tokenless handshake
//! rejection
//! [POS]: Automated acceptance of the B5 batch of remote-r6-implementation.md;
//! slow-consumer/resync paths are guaranteed by queue-semantics unit tests
//! and the broadcast Lagged branch

mod common;

use common::{spawn_against, IsolatedHerdr, TOKEN};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;

async fn ws_connect(
    addr: std::net::SocketAddr,
    token: Option<&str>,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!("ws://{addr}/api/v2/events");
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = url
        .as_str()
        .into_client_request()
        .unwrap_or_else(|error| panic!("build ws request: {error}"));
    if let Some(token) = token {
        if let Ok(value) = format!("Bearer {token}").parse() {
            request.headers_mut().insert("Authorization", value);
        }
    }
    let (stream, _response) = tokio_tungstenite::connect_async(request)
        .await
        .unwrap_or_else(|error| panic!("ws connect: {error}"));
    stream
}

fn text_of(message: tokio_tungstenite::tungstenite::Message) -> String {
    match message {
        tokio_tungstenite::tungstenite::Message::Text(text) => text.to_string(),
        other => panic!("expected text frame, got: {other:?}"),
    }
}

#[tokio::test]
async fn events_socket_ready_ping_pong_and_structural_event() {
    if !common::herdr_available() {
        eprintln!(
            "skipping: herdr binary not found; set HERDR_BIN or install Herdr with wax to run loopback tests"
        );
        return;
    }
    let herdr = IsolatedHerdr::spawn();
    herdr.seed_workspace();
    let handle = spawn_against(&herdr);

    let mut ws = ws_connect(handle.addr, Some(TOKEN)).await;

    // The ready first frame.
    let ready = match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
        Ok(Some(Ok(frame))) => frame,
        Ok(Some(Err(error))) => panic!("ready frame error: {error}"),
        Ok(None) => panic!("stream closed before ready"),
        Err(_) => panic!("ready frame timeout"),
    };
    let ready = text_of(ready);
    assert!(ready.contains(r#""type":"ready""#), "{ready}");
    assert!(ready.contains(r#""remote_api_version":3"#), "{ready}");

    // Application-level ping → pong.
    if let Err(error) = ws
        .send(tokio_tungstenite::tungstenite::Message::text(
            r#"{"type":"ping"}"#,
        ))
        .await
    {
        panic!("send ping: {error}");
    }
    // The hub's health_changed/structural events may interleave with the pong
    // (legitimate ordering); skip non-pong frames.
    let mut pong = String::new();
    for _ in 0..8 {
        let frame = match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
            Ok(Some(Ok(frame))) => frame,
            Ok(Some(Err(error))) => panic!("pong frame error: {error}"),
            Ok(None) => panic!("stream closed before pong"),
            Err(_) => panic!("pong frame timeout"),
        };
        let frame = text_of(frame);
        if frame.contains(r#""type":"pong""#) {
            pong = frame;
            break;
        }
    }
    assert!(pong.contains(r#""type":"pong""#), "pong never arrived");

    // Structural change: the CLI creates a tab → the hub broadcasts
    // workspace/project.updated.
    let cwd = herdr.home.to_string_lossy().into_owned();
    let cli_home = herdr.home.clone();
    let cli_socket = herdr.socket.clone();
    let trigger = tokio::task::spawn_blocking(move || {
        let output = std::process::Command::new(common::herdr_binary_for_tests())
            .args(["tab", "create", "--cwd", &cwd])
            .env("HOME", &cli_home)
            .env("HERDR_SOCKET_PATH", &cli_socket)
            .output()
            .unwrap_or_else(|error| panic!("herdr tab create: {error}"));
        assert!(
            output.status.success(),
            "tab create failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    });
    if let Err(error) = trigger.await {
        panic!("cli join: {error}");
    }

    let mut saw_structural = false;
    for _ in 0..6 {
        let frame = match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
            Ok(Some(Ok(frame))) => frame,
            Ok(Some(Err(error))) => panic!("event frame error: {error}"),
            Ok(None) => panic!("stream closed before event"),
            Err(_) => panic!("event frame timeout"),
        };
        let frame = text_of(frame);
        if frame.contains(r#""kind":"workspace.updated""#)
            || frame.contains(r#""kind":"project.updated""#)
        {
            saw_structural = true;
            break;
        }
    }
    assert!(
        saw_structural,
        "structural event must arrive after tab create"
    );

    let _ = ws.close(None).await;
    handle.stop();
}

#[tokio::test]
async fn events_socket_requires_token() {
    if !common::herdr_available() {
        eprintln!(
            "skipping: herdr binary not found; set HERDR_BIN or install Herdr with wax to run loopback tests"
        );
        return;
    }
    let herdr = IsolatedHerdr::spawn();
    herdr.seed_workspace();
    let handle = spawn_against(&herdr);

    let url = format!("ws://{}/api/v2/events", handle.addr);
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let request = url
        .as_str()
        .into_client_request()
        .unwrap_or_else(|error| panic!("build ws request: {error}"));
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(request),
    )
    .await;
    match result {
        Err(_timeout) => panic!("handshake should fail fast, not hang"),
        Ok(Err(_rejected)) => {} // 401 → tungstenite handshake error
        Ok(Ok((_stream, _response))) => panic!("unauthenticated ws must be rejected"),
    }
    handle.stop();
}

#[tokio::test]
async fn web_connection_registry_tracks_socket_lifecycle() {
    if !common::herdr_available() {
        eprintln!(
            "skipping: herdr binary not found; set HERDR_BIN or install Herdr with wax to run loopback tests"
        );
        return;
    }
    let herdr = IsolatedHerdr::spawn();
    herdr.seed_workspace();
    let handle = spawn_against(&herdr);

    assert!(
        handle.web_connections().is_empty(),
        "no connections before any WS client"
    );

    let mut ws = ws_connect(handle.addr, Some(TOKEN)).await;
    // The ready frame arriving = registration has happened.
    match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
        Ok(Some(Ok(_))) => {}
        other => panic!("ready frame expected, got: {other:?}"),
    }
    let connections = handle.web_connections();
    assert_eq!(connections.len(), 1, "one live web connection");
    assert!(
        connections[0].addr.starts_with("127.0.0.1:"),
        "peer addr captured: {}",
        connections[0].addr
    );

    // Close → the registry reclaims.
    let _ = ws.close(None).await;
    for _ in 0..20 {
        if handle.web_connections().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        handle.web_connections().is_empty(),
        "connection removed after close"
    );
    handle.stop();
}
