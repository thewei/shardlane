//! Remote HTTP/WS adapter for the Host-owned shared Herdr TUI session.
//!
//! [INPUT]: `shardlane_host::shared_tui` (the sole TUI process owner: one
//! child + one PTY + fan-out), `crate::bootstrap::connect_herdr` (the
//! protocol gate), `RemoteState` (the authenticated Herdr socket boundary),
//! and `crate::tui_input` (semantic input encoding).
//! [OUTPUT]: HTTP lifecycle handlers and a base64 byte-stream WebSocket for
//! the shared Herdr TUI; viewers are only subscribers and own no process. The
//! stream WebSocket also accepts the same input requests as HTTP `/input`
//! (`{"data": ...}` / semantic events) as low-latency inbound frames — mobile
//! scroll/key batches skip per-report HTTP setup.
//! [POS]: After the A01 convergence this module no longer spawns/owns the
//! Herdr TUI child process; the process owner is
//! `shardlane_host::shared_tui::TuiManager` (the GUI injects the
//! process-level instance; Desktop GPUI and this Remote adapter attach the
//! same session). Stream viewers register/touch/drop in the shared
//! web-connection registry like the events socket (D23), so the pairing list
//! counts them.

use crate::bootstrap::connect_herdr;
use crate::cors::BEARER_SUBPROTOCOL;
use crate::error::ApiError;
use crate::events::{is_ping_frame, WS_IDLE_TIMEOUT};
use crate::state::RemoteState;
use crate::tui_input::{encode_request, TuiInputRequest};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Json, Path, State};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use shardlane_host::shared_tui::{
    validate_remote_size, HerdrTuiSession, TuiError, TuiEvent, DEFAULT_COLS, DEFAULT_ROWS,
};
use shardlane_host::HerdrTuiSessionSummary;
use std::sync::Arc;

const MIN_TUI_PROTOCOL: u32 = 20;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct OpenSessionRequest {
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    /// Multi-instance: the Project registry id whose Herdr instance the TUI
    /// child attaches to. `None` = the default instance.
    #[serde(default)]
    pub instance_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ResizeRequest {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TuiStreamFrame {
    Ready {
        session: HerdrTuiSessionSummary,
        shared: bool,
    },
    Output {
        revision: u64,
        encoding: &'static str,
        data: String,
    },
    Status {
        session: HerdrTuiSessionSummary,
    },
    Resync {
        revision: u64,
    },
    Pong,
}

impl TuiStreamFrame {
    fn wire(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|_| "{\"type\":\"resync\",\"revision\":0}".into())
    }
}

fn map_error(error: TuiError, request_id: String) -> ApiError {
    match error {
        TuiError::HerdrUnavailable(detail) => {
            shardlane_host::diagnostics::lag_log(format_args!(
                "remote.tui Herdr unavailable: {detail}"
            ));
            ApiError::host_unavailable("Herdr runtime is not reachable", request_id)
        }
        TuiError::UnsupportedProtocol(actual) => ApiError::runtime_unavailable(
            format!("Herdr protocol {actual} cannot host the shared TUI"),
            request_id,
        ),
        TuiError::InvalidSession => ApiError::not_found("unknown TUI session", request_id),
        TuiError::Closed => ApiError::runtime_unavailable("TUI session is closed", request_id),
        TuiError::Backpressure => ApiError::timeout("TUI input queue is full", request_id),
        TuiError::InvalidInput(detail) => ApiError::invalid_request(detail, request_id),
        TuiError::Spawn(detail) | TuiError::Io(detail) => {
            shardlane_host::diagnostics::lag_log(format_args!("remote.tui failure: {detail}"));
            ApiError::runtime_unavailable("Herdr TUI failed", request_id)
        }
    }
}

fn session_from_state(
    state: &Arc<RemoteState>,
    id: &str,
) -> Result<Arc<HerdrTuiSession>, TuiError> {
    state
        .tui_registry
        .get_session(id)
        .ok_or(TuiError::InvalidSession)
}

fn protocol_gate_for(state: &RemoteState, session: Option<&str>) -> Result<(), TuiError> {
    let client = match session {
        None => connect_herdr(state),
        Some(session) => shardlane_host::herdr::HerdrClient::bootstrap_for_session(session),
    }
    .map_err(|error| TuiError::HerdrUnavailable(error.to_string()))?;
    let protocol = client.protocol().ok_or_else(|| {
        TuiError::HerdrUnavailable("Herdr did not report a protocol version".into())
    })?;
    if protocol < MIN_TUI_PROTOCOL {
        return Err(TuiError::UnsupportedProtocol(protocol));
    }
    Ok(())
}

pub async fn open_session(
    State(state): State<Arc<RemoteState>>,
    body: Result<Json<OpenSessionRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    // D01: shared body decode → unified ApiError envelope.
    let body = match crate::server::decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    let cols = body.cols.unwrap_or(DEFAULT_COLS);
    let rows = body.rows.unwrap_or(DEFAULT_ROWS);
    // Multi-instance: the instance id IS the Herdr session name.
    let instance_session = match body.instance_id.as_deref() {
        None | Some("default") => None,
        Some(session) => Some(session.to_string()),
    };
    let result = {
        let state = state.clone();
        let instance_session = instance_session.clone();
        tokio::task::spawn_blocking(move || {
            validate_remote_size(cols, rows)?;
            protocol_gate_for(&state, instance_session.as_deref())?;
            let manager = state.tui_manager_for(instance_session.as_deref());
            manager.open(cols, rows, state.herdr_socket_override.as_deref())
        })
        .await
    };
    match result {
        Ok(Ok(session)) => Json(session.summary()).into_response(),
        Ok(Err(error)) => map_error(error, request_id).into_response(),
        Err(error) => {
            ApiError::internal(format!("TUI open failed: {error}"), request_id).into_response()
        }
    }
}

pub async fn get_session(
    State(state): State<Arc<RemoteState>>,
    Path(id): Path<String>,
) -> Response {
    let request_id = state.next_request_id();
    match session_from_state(&state, &id) {
        Ok(session) => Json(session.summary()).into_response(),
        Err(error) => map_error(error, request_id).into_response(),
    }
}

pub async fn close_session(
    State(state): State<Arc<RemoteState>>,
    Path(id): Path<String>,
) -> Response {
    let request_id = state.next_request_id();
    match state.tui_registry.close_session(&id) {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(error) => map_error(error, request_id).into_response(),
    }
}

pub async fn resize_session(
    State(state): State<Arc<RemoteState>>,
    Path(id): Path<String>,
    body: Result<Json<ResizeRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    // D01: shared body decode → unified ApiError envelope.
    let body = match crate::server::decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    match validate_remote_size(body.cols, body.rows)
        .and_then(|()| session_from_state(&state, &id))
        .and_then(|session| session.resize(body.cols, body.rows))
    {
        Ok(summary) => Json(summary).into_response(),
        Err(error) => map_error(error, request_id).into_response(),
    }
}

pub(crate) async fn input_session(
    State(state): State<Arc<RemoteState>>,
    Path(id): Path<String>,
    body: Result<Json<TuiInputRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    // D01: shared body decode → unified ApiError envelope.
    let body = match crate::server::decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    let bytes = match encode_request(body) {
        Ok(bytes) => bytes,
        Err(error) => {
            return map_error(TuiError::InvalidInput(error.to_string()), request_id)
                .into_response();
        }
    };
    match session_from_state(&state, &id).and_then(|session| session.send_bytes(&bytes.0)) {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(error) => map_error(error, request_id).into_response(),
    }
}

pub async fn stream_session(
    State(state): State<Arc<RemoteState>>,
    Path(id): Path<String>,
    connect_info: axum::extract::ConnectInfo<std::net::SocketAddr>,
    ws: WebSocketUpgrade,
) -> Response {
    let request_id = state.next_request_id();
    let session = match session_from_state(&state, &id) {
        Ok(session) => session,
        Err(error) => return map_error(error, request_id).into_response(),
    };
    ws.protocols([BEARER_SUBPROTOCOL])
        .on_upgrade(move |socket| async move {
            serve_socket(socket, session, state, connect_info.0).await
        })
        .into_response()
}

async fn serve_socket(
    socket: WebSocket,
    session: Arc<HerdrTuiSession>,
    state: Arc<RemoteState>,
    peer: std::net::SocketAddr,
) {
    // D23: TUI stream viewers are web connections too — register → touch →
    // drop exactly like the events socket, so the pairing list stops
    // under-reporting connected clients.
    let connection_id = state.register_web_connection(peer.to_string());
    let result = serve_socket_inner(socket, session, &state, connection_id).await;
    state.drop_web_connection(connection_id);
    result
}

async fn serve_socket_inner(
    socket: WebSocket,
    session: Arc<HerdrTuiSession>,
    state: &Arc<RemoteState>,
    connection_id: u64,
) {
    let mut events = session.subscribe();
    let (mut tx, mut rx) = socket.split();
    let ready = TuiStreamFrame::Ready {
        session: session.summary(),
        shared: true,
    };
    if tx.send(Message::text(ready.wire())).await.is_err() {
        return;
    }
    let _ = session.force_redraw();
    loop {
        tokio::select! {
            event = events.recv() => {
                let frame = match event {
                    Ok(TuiEvent::Output {
                        revision, bytes, ..
                    }) => TuiStreamFrame::Output {
                        revision,
                        encoding: "base64",
                        data: base64::engine::general_purpose::STANDARD.encode(bytes),
                    },
                    Ok(TuiEvent::Status { summary }) => TuiStreamFrame::Status { session: summary },
                    Ok(TuiEvent::Wake) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let _ = session.force_redraw();
                        TuiStreamFrame::Resync { revision: session.summary().revision }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                if tx.send(Message::text(frame.wire())).await.is_err() {
                    break;
                }
            }
            inbound = tokio::time::timeout(WS_IDLE_TIMEOUT, rx.next()) => {
                match inbound {
                    Ok(Some(Ok(message))) => {
                        state.touch_web_connection(connection_id);
                        match message {
                            Message::Text(text) => {
                                if is_ping_frame(&text)
                                    && tx.send(Message::text(TuiStreamFrame::Pong.wire())).await.is_err() {
                                    break;
                                }
                                // Low-latency input path: the same untagged request the
                                // HTTP /input endpoint takes (raw `{data}` or a semantic
                                // event), so scroll/key batches skip per-report request
                                // setup. Malformed frames are ignored, never fatal.
                                else if let Ok(request) = serde_json::from_str::<TuiInputRequest>(&text) {
                                    if let Ok(bytes) = encode_request(request) {
                                        let _ = session.send_bytes(&bytes.0);
                                    }
                                }
                            }
                            Message::Close(_) => break,
                            _ => {}
                        }
                    }
                    Ok(Some(Err(_))) | Ok(None) => break,
                    Err(_) => break,
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "tui_tests.rs"]
mod tests;
