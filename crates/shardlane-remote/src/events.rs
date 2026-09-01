//! Event hub: 1 subscription socket thread → broadcast channel → N
//! WebSocket clients.
//!
//! [INPUT]: Depends on shardlane-host (subscribe_events/HerdrEvent/
//! ProjectIndex/host_agent_status/project_id codecs) and diagnostics
//! (lag_log), plus tokio broadcast
//! [OUTPUT]: Exposes the WsFrame wire format and spawn_event_hub
//! (reconnect + agent.updated/conversation.updated 100ms keep-latest
//! coalescing + host.health_changed lifecycle), plus the event sender
//! loaded into RemoteState
//! [POS]: The event dispatch core of shardlane-remote; events are change
//! hints, not a second source of truth — client recovery always goes
//! through a bootstrap re-pull (resync semantics), with no persistent
//! replay

use crate::bootstrap::connect_herdr;
use crate::state::RemoteState;
use serde::Serialize;
use serde_json::json;
use shardlane_host::herdr::HerdrEvent;
use shardlane_host::project_index::ProjectIndex;
use shardlane_host::{
    conversation_id_for_live_agent, conversation_id_for_live_session,
    project_id_for_runtime_workspace, AgentRef,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Broadcast channel capacity: overflow drops frames (events are hints; slow
/// consumers recover via resync).
pub const EVENT_BROADCAST_CAPACITY: usize = 256;
/// agent.updated coalescing window: status storms for the same pane
/// keep-latest within this window.
const AGENT_EVENT_COALESCE_WINDOW: Duration = Duration::from_millis(100);
/// pane.output_changed coalescing window: terminal output changes are
/// high-frequency — 100ms keep-latest.
const PANE_OUTPUT_COALESCE_WINDOW: Duration = Duration::from_millis(100);
/// Subscription poll period (drives coalescer flushing).
const HUB_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// Reconnect backoff base step (0.5s→15s cap, same family as the desktop
/// reconnect_backoff).
const RECONNECT_BASE: Duration = Duration::from_millis(500);
const RECONNECT_CAP: Duration = Duration::from_secs(15);

/// WS wire format (v1): ready / event / resync / pong.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsFrame {
    Ready {
        remote_api_version: u32,
    },
    Event {
        kind: String,
        payload: serde_json::Value,
    },
    Resync,
    Pong,
}

/// Client application-level pings are expected every 30s; the server closes
/// after 120s without any inbound frame. Single authority shared by the
/// events socket (server.rs) and the TUI stream socket (tui.rs).
pub(crate) const WS_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// Single authority for application-level ping sniffing, shared by the
/// events socket and the TUI stream socket: the client sends
/// `{"type":"ping"}` (with or without a space after the colon) and expects a
/// pong frame.
pub(crate) fn is_ping_frame(text: &str) -> bool {
    text.contains(r#""type":"ping""#) || text.contains(r#""type": "ping""#)
}

impl WsFrame {
    pub fn to_wire(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"type":"resync"}"#.to_string())
    }
}

/// Map one Herdr event to remote frames (pure function; unit tests pin the
/// semantics). Compatibility events carry workspace/project/agent/pane
/// hints; a typed session change additionally emits a semantic
/// `conversation.updated` invalidation hint.
pub fn map_herdr_event(event: &HerdrEvent, index: Option<&ProjectIndex>) -> Vec<WsFrame> {
    if let Some(scroll) = event.pane_scroll_patch() {
        return vec![WsFrame::Event {
            kind: "pane.output_changed".into(),
            payload: json!({
                "pane_id": scroll.pane_id,
                "workspace_id": scroll.workspace_id,
            }),
        }];
    }
    if let Some(patch) = event.agent_status_patch() {
        if patch.pane_id.is_empty() {
            return Vec::new();
        }
        let mut payload = json!({
            "agent_ref": patch.pane_id,
            "workspace_id": patch.workspace_id,
        });
        if let Some(status) = patch.agent_status.flatten() {
            payload["status"] = json!(status);
        }
        if let Some(project) = index.and_then(|index| index.for_runtime_id(&patch.workspace_id)) {
            if let Some(runtime_id) = &project.runtime_workspace_id {
                payload["project_id"] =
                    json!(project_id_for_runtime_workspace(runtime_id).as_str());
            }
        }
        let mut frames = vec![WsFrame::Event {
            kind: "agent.updated".into(),
            payload,
        }];
        // A typed session appearing/clearing is a semantic Conversation change:
        // clients must drop the live surface rather than retain a stale identity.
        // AC-03: a new session carries the session-exact v2 id; a cleared
        // session (Some(None)) has no typed session and falls back to the v1
        // pane id as a mere invalidation hint; None = unchanged, no notify.
        if let Some(session) = patch.agent_session.as_ref() {
            let conversation_id = match session {
                Some(session) => {
                    conversation_id_for_live_session(&AgentRef::new(&patch.pane_id), session)
                }
                None => conversation_id_for_live_agent(&AgentRef::new(&patch.pane_id)),
            };
            frames.push(WsFrame::Event {
                kind: "conversation.updated".into(),
                payload: json!({
                    "conversation_id": conversation_id,
                    "revision": 0,
                }),
            });
        }
        return frames;
    }
    if event.refreshes_navigation_projection() {
        let mut frames = Vec::new();
        if let Some(workspace_id) = event.affected_workspace_id() {
            frames.push(WsFrame::Event {
                kind: "workspace.updated".into(),
                payload: json!({ "workspace_id": workspace_id }),
            });
            frames.push(WsFrame::Event {
                kind: "project.updated".into(),
                payload: json!({
                    "project_id": project_id_for_runtime_workspace(&workspace_id).as_str()
                }),
            });
        }
        return frames;
    }
    Vec::new()
}

/// Generic event coalescer: keep-latest within the window for the same key.
#[derive(Default)]
struct EventCoalescer {
    pending: HashMap<String, (Instant, String)>,
}

impl EventCoalescer {
    fn hold(&mut self, key: &str, frame: WsFrame, window: Duration) {
        self.pending
            .insert(key.to_string(), (Instant::now() + window, frame.to_wire()));
    }

    fn flush_due(&mut self, send: &dyn Fn(String)) {
        let now = Instant::now();
        let due: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, (deadline, _))| *deadline <= now)
            .map(|(key, _)| key.clone())
            .collect();
        for key in due {
            if let Some((_, frame)) = self.pending.remove(&key) {
                send(frame);
            }
        }
    }
}

/// D18: single dispatch of one Herdr event → remote frames, shared by the
/// pane-scroll drain loop and the main subscription loop. High-frequency
/// kinds keep-latest in the coalescer; everything else emits immediately.
/// Sending to a broadcast channel with no subscribers fails silently —
/// events are hints, not guaranteed delivery.
fn dispatch_event(
    event: &HerdrEvent,
    index: Option<&ProjectIndex>,
    coalescer: &mut EventCoalescer,
    tx: &tokio::sync::broadcast::Sender<String>,
) {
    for frame in map_herdr_event(event, index) {
        match &frame {
            WsFrame::Event { kind, payload } if kind == "agent.updated" => {
                let agent_ref = payload["agent_ref"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                coalescer.hold(&agent_ref, frame, AGENT_EVENT_COALESCE_WINDOW);
            }
            WsFrame::Event { kind, payload } if kind == "pane.output_changed" => {
                let pane_id = payload["pane_id"].as_str().unwrap_or_default().to_string();
                coalescer.hold(
                    &format!("pane_output:{pane_id}"),
                    frame,
                    PANE_OUTPUT_COALESCE_WINDOW,
                );
            }
            _ => {
                let _ = tx.send(frame.to_wire());
            }
        }
    }
}

/// Start the event hub thread (exits with the stop signal). Returns the
/// broadcast sender, loaded into RemoteState by spawn_remote_server.
pub fn spawn_event_hub(
    state: Arc<RemoteState>,
    stop: watch::Receiver<bool>,
) -> tokio::sync::broadcast::Sender<String> {
    spawn_event_hub_for(state, stop, None)
}

/// A5: instance-scoped variant — the hub thread subscribes to THAT Herdr
/// instance's socket (`session == None` = default instance).
pub fn spawn_event_hub_for(
    state: Arc<RemoteState>,
    stop: watch::Receiver<bool>,
    session: Option<String>,
) -> tokio::sync::broadcast::Sender<String> {
    let (tx, _rx) = tokio::sync::broadcast::channel::<String>(EVENT_BROADCAST_CAPACITY);
    let hub_tx = tx.clone();
    let emit = |frame: WsFrame, tx: &tokio::sync::broadcast::Sender<String>| {
        // send failing with no subscribers is normal (events are hints, not
        // guaranteed delivery).
        let _ = tx.send(frame.to_wire());
    };

    let thread = std::thread::Builder::new()
        .name("shardlane-remote-events".to_string())
        .spawn(move || {
            let mut backoff_step: u32 = 0;
            loop {
                if *stop.borrow() {
                    break;
                }
                let subscription = crate::bootstrap::connect_herdr_for(&state, session.as_deref())
                    .and_then(|client| client.subscribe_events().map(|rx| (client, rx)));
                match subscription {
                    Ok((_client, rx)) => {
                        backoff_step = 0;
                        emit(
                            WsFrame::Event {
                                kind: "host.health_changed".into(),
                                payload: json!({ "connected": true }),
                            },
                            &hub_tx,
                        );
                        // Projection snapshot: used for agent→project
                        // association (staleness acceptable — hint
                        // semantics).
                        let snapshot = connect_herdr(&state)
                            .ok()
                            .and_then(|c| c.host_bootstrap_state().ok());
                        let index = snapshot.as_ref().map(ProjectIndex::build_from_state);

                        // pane scroll subscription: needs the concrete
                        // pane_id list.
                        let pane_rx = snapshot.as_ref().and_then(|s| {
                            let pane_ids: Vec<String> =
                                s.panes.iter().map(|p| p.pane_id.clone()).collect();
                            if pane_ids.is_empty() {
                                shardlane_host::diagnostics::lag_log(format_args!(
                                    "remote.events pane_scroll: no panes to subscribe"
                                ));
                                return None;
                            }
                            shardlane_host::diagnostics::lag_log(format_args!(
                                "remote.events pane_scroll: subscribing {} panes",
                                pane_ids.len()
                            ));
                            let result = connect_herdr(&state)
                                .ok()
                                .and_then(|c| c.subscribe_pane_events(&pane_ids).ok());
                            if result.is_none() {
                                shardlane_host::diagnostics::lag_log(format_args!(
                                    "remote.events pane_scroll: subscribe_pane_events failed"
                                ));
                            }
                            result
                        });

                        let mut coalescer = EventCoalescer::default();
                        loop {
                            if *stop.borrow() {
                                return;
                            }
                            if let Some(ref pane_rx) = pane_rx {
                                while let Ok(event) = pane_rx.try_recv() {
                                    // D19: no per-event logging here — pane
                                    // events fire at terminal-output
                                    // frequency, so an unconditional lag_log
                                    // would be a hot-path formatting cost.
                                    dispatch_event(&event, index.as_ref(), &mut coalescer, &hub_tx);
                                }
                            }
                            match rx.try_recv() {
                                Ok(event) => {
                                    dispatch_event(&event, index.as_ref(), &mut coalescer, &hub_tx);
                                }
                                Err(_) => {
                                    // async-channel 2.x's TryRecvError is Empty
                                    // only; disconnection is judged by
                                    // sender_count==0 (the reader thread exiting
                                    // drops the senders).
                                    if rx.sender_count() == 0 {
                                        break;
                                    }
                                }
                            }
                            coalescer.flush_due(&|frame| {
                                let _ = hub_tx.send(frame);
                            });
                            std::thread::sleep(HUB_POLL_INTERVAL);
                        }
                    }
                    Err(error) => {
                        shardlane_host::diagnostics::lag_log(format_args!(
                            "remote.events subscribe failed: {error}"
                        ));
                    }
                }
                emit(
                    WsFrame::Event {
                        kind: "host.health_changed".into(),
                        payload: json!({ "connected": false }),
                    },
                    &hub_tx,
                );
                let delay = RECONNECT_BASE
                    .saturating_mul(1u32 << backoff_step.min(5))
                    .min(RECONNECT_CAP);
                // Sleep in segments to stay responsive to stop.
                let mut remaining = delay;
                while remaining > Duration::ZERO {
                    if *stop.borrow() {
                        return;
                    }
                    let step = remaining.min(Duration::from_millis(250));
                    std::thread::sleep(step);
                    remaining = remaining.saturating_sub(step);
                }
                backoff_step += 1;
            }
        });
    if let Err(error) = thread {
        shardlane_host::diagnostics::lag_log(format_args!(
            "remote.events hub thread spawn failed: {error}"
        ));
        // The sender remains usable but forever silent: WS clients wait for
        // events after ready, and a missing health event is an acceptable
        // degradation (bootstrap backstops).
    }
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn herdr_event(event: &str, data: serde_json::Value) -> HerdrEvent {
        HerdrEvent {
            event: event.to_string(),
            data,
        }
    }

    #[test]
    fn agent_status_event_maps_to_agent_updated() {
        let event = herdr_event(
            "pane_agent_status_changed",
            json!({
                "pane_id": "w1:p1",
                "workspace_id": "w1",
                "agent_status": "working"
            }),
        );
        let frames = map_herdr_event(&event, None);
        assert_eq!(frames.len(), 1);
        let wire = frames[0].to_wire();
        assert!(wire.contains(r#""kind":"agent.updated""#), "{wire}");
        assert!(wire.contains(r#""agent_ref":"w1:p1""#), "{wire}");
        assert!(wire.contains(r#""status":"working""#), "{wire}");
    }

    #[test]
    fn typed_agent_session_change_also_invalidates_semantic_conversation() {
        let event = herdr_event(
            "pane_agent_status_changed",
            json!({
                "pane_id": "w1:p1",
                "workspace_id": "w1",
                "agent_status": "working",
                "agent_session": {
                    "agent": "claude",
                    "kind": "id",
                    "source": "herdr:claude",
                    "value": "native-1"
                }
            }),
        );
        let frames = map_herdr_event(&event, None);
        assert_eq!(frames.len(), 2);
        let wire = frames[1].to_wire();
        assert!(wire.contains(r#""kind":"conversation.updated""#), "{wire}");
        // AC-03: a typed session event carries the session-exact v2 id.
        assert!(wire.contains(r#""conversation_id":"conv_2_"#), "{wire}");
        assert!(wire.contains(r#""revision":0"#), "{wire}");
    }

    #[test]
    fn clearing_agent_session_also_invalidates_semantic_conversation() {
        let event = herdr_event(
            "pane_agent_status_changed",
            json!({
                "pane_id": "w1:p1",
                "workspace_id": "w1",
                "agent_session": null
            }),
        );
        let frames = map_herdr_event(&event, None);
        assert_eq!(frames.len(), 2);
        assert!(frames[1]
            .to_wire()
            .contains(r#""kind":"conversation.updated"#));
    }

    #[test]
    fn navigation_event_maps_to_workspace_and_project() {
        let event = herdr_event(
            "workspace_created",
            json!({ "workspace": { "workspace_id": "w7" } }),
        );
        let frames = map_herdr_event(&event, None);
        assert_eq!(frames.len(), 2);
        let wire = frames.iter().map(WsFrame::to_wire).collect::<Vec<_>>();
        assert!(wire[0].contains(r#""kind":"workspace.updated""#));
        assert!(wire[1].contains(r#""kind":"project.updated""#));
        assert!(wire[1].contains("prj_1_"), "{}", wire[1]);
    }

    #[test]
    fn pane_scroll_event_maps_to_output_changed() {
        let event = herdr_event(
            "pane_scroll_changed",
            json!({
                "pane_id": "w1:p2",
                "workspace_id": "w1",
                "scroll": { "total_rows": 100, "viewport_rows": 24 }
            }),
        );
        let frames = map_herdr_event(&event, None);
        assert_eq!(frames.len(), 1);
        let wire = frames[0].to_wire();
        assert!(wire.contains(r#""kind":"pane.output_changed""#), "{wire}");
        assert!(wire.contains(r#""pane_id":"w1:p2""#), "{wire}");
    }

    #[test]
    fn unrelated_event_maps_to_nothing() {
        let event = herdr_event(
            "layout_updated",
            json!({ "layout": { "workspace_id": "w1" } }),
        );
        assert!(map_herdr_event(&event, None).is_empty());
    }

    #[test]
    fn coalescer_keeps_latest_per_agent() {
        let mut coalescer = EventCoalescer::default();
        coalescer.hold(
            "a",
            WsFrame::Event {
                kind: "agent.updated".into(),
                payload: json!({"status":"working"}),
            },
            AGENT_EVENT_COALESCE_WINDOW,
        );
        std::thread::sleep(Duration::from_millis(30));
        coalescer.hold(
            "a",
            WsFrame::Event {
                kind: "agent.updated".into(),
                payload: json!({"status":"done"}),
            },
            AGENT_EVENT_COALESCE_WINDOW,
        );
        let delivered = std::sync::Mutex::new(Vec::new());
        let record = |frame: String| {
            if let Ok(mut guard) = delivered.lock() {
                guard.push(frame);
            }
        };
        coalescer.flush_due(&record);
        assert!(
            delivered.lock().map(|g| g.is_empty()).unwrap_or(false),
            "window not elapsed yet"
        );
        std::thread::sleep(AGENT_EVENT_COALESCE_WINDOW);
        coalescer.flush_due(&record);
        let guard = delivered.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(guard.len(), 1, "only latest survives");
        assert!(guard[0].contains(r#""status":"done""#));
    }
}
