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
//!
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
//!
//! 满载鲁棒性契约（2026-09-19，W9 诊断）：herdr 0.9.1 的 events.subscribe
//! 注册为异步生效——订阅被 ack 后约 1s 内触发的变更事件可能不推送（探针实
//！ 测：注册后 0s 触发 2/2 丢失，≥1.5s 后触发 4/4 送达）。本文件的断言均按
//! "变更 → 最终送达"契约书写：逐帧窗口放宽到 15s，结构性事件在必要时重触发。

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
    // 15s：满载下隔离 herdr server 与 remote hub 的启动/排队可远超 5s。
    let ready = match tokio::time::timeout(Duration::from_secs(15), ws.next()).await {
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
        let frame = match tokio::time::timeout(Duration::from_secs(15), ws.next()).await {
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

    // herdr 0.9.1 注册延迟实测 ~1-1.5s：触发前先让订阅注册稳定落地，
    // 常规路径一轮即过；下面 3 轮重试只是满载下的安全网。
    tokio::time::sleep(Duration::from_millis(1500)).await;

    let mut saw_structural = false;
    // herdr 0.9.1: 订阅注册异步生效（见文件头），首触发的结构事件可能不
    // 推送。契约是"变更 → 结构事件最终送达"：每轮先触发一次 tab create，
    // 再最多等 4 帧 × 10s；未送达则用新的 tab create 重触发（每次触发都会
    // 产生新的 tab.created 事件）。
    'structural: for _attempt in 0..3 {
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
        for _ in 0..4 {
            let frame = match tokio::time::timeout(Duration::from_secs(10), ws.next()).await {
                Ok(Some(Ok(frame))) => frame,
                Ok(Some(Err(error))) => panic!("event frame error: {error}"),
                Ok(None) => panic!("stream closed before event"),
                Err(_) => break,
            };
            let frame = text_of(frame);
            if frame.contains(r#""kind":"workspace.updated""#)
                || frame.contains(r#""kind":"project.updated""#)
            {
                saw_structural = true;
                break 'structural;
            }
        }
    }
    assert!(
        saw_structural,
        "structural event must arrive after tab create (herdr 0.9.1 async registration; retried 3x)"
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
    // 10s：满载下 axum 握手与鉴权回包也可能排队超过 5s（W9 满载实测）。
    let result = tokio::time::timeout(
        Duration::from_secs(10),
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
    // 15s：满载下 hub 启动与首帧推送可远超 5s（同 structural 用例）。
    match tokio::time::timeout(Duration::from_secs(15), ws.next()).await {
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

    // Close → the registry reclaims. 50×100ms：满载下 WS 关闭传播可超 2s。
    let _ = ws.close(None).await;
    for _ in 0..50 {
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
