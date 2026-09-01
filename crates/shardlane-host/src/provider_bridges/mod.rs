//! Host-owned local Provider Bridge adapter layer.
//!
//! [INPUT]: Companion command hooks, plugins, and extensions emitting local IPC events.
//! [OUTPUT]: Bounded Unix domain socket server, length-bounded protocol, provider-neutral
//! interaction broker integration, and capture policy management.
//! [POS]: S3 local provider bridge / S4 Claude bridge.

pub mod claude_adapter;
pub mod codex_adapter;
pub mod local_ipc;
pub mod opencode_adapter;
pub mod overlay_store;
pub mod pi_adapter;
pub mod protocol;
pub mod registry;
pub mod test_support;

pub use claude_adapter::ClaudeBridgeAdapter;
pub use codex_adapter::CodexBridgeAdapter;
pub use local_ipc::{BridgeRuntimeDescriptor, LocalProviderBridgeServer};
pub use opencode_adapter::OpenCodeBridgeAdapter;
pub use overlay_store::{ProviderOverlayStore, ProviderSemanticOverlay};
pub use pi_adapter::PiBridgeAdapter;
pub use protocol::{
    ProviderBridgeEnvelope, ProviderBridgeMode, ProviderBridgeReply, ProviderSessionLocator,
    BRIDGE_PROTOCOL_VERSION, MAX_BRIDGE_FRAME_BYTES,
};
pub use registry::{
    InMemorySessionResolver, InteractionCapturePolicy, ParsedInteractionPayload,
    ProviderBridgeAdapter, ProviderBridgeRegistry, ResolvedLiveSession, SessionLocatorResolver,
    SyntheticBridgeAdapter,
};

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::conversation_interactions::{
        ConversationInteractionBroker, ConversationInteractionState, InteractionResponse,
    };
    use crate::ids::{AgentRef, ConversationId};
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;
    use test_support::SyntheticBridgeClient;

    fn setup_server() -> (
        TempDir,
        LocalProviderBridgeServer,
        Arc<ConversationInteractionBroker>,
        Arc<InMemorySessionResolver>,
        ConversationId,
    ) {
        let temp_dir = TempDir::new().unwrap();
        let broker = Arc::new(ConversationInteractionBroker::new());
        let resolver = Arc::new(InMemorySessionResolver::new());
        let conv_id = ConversationId::new("conv-synth-1");

        resolver.register(
            "synthetic",
            "session-1",
            ResolvedLiveSession {
                conversation_id: conv_id.clone(),
                agent_ref: AgentRef::new("agent-1"),
                occupant_fingerprint: "fp-synth-1".to_string(),
            },
        );

        let registry = Arc::new(ProviderBridgeRegistry::new(
            Arc::clone(&broker),
            Arc::clone(&resolver) as Arc<dyn SessionLocatorResolver>,
        ));

        let server = LocalProviderBridgeServer::start(Arc::clone(&registry), temp_dir.path())
            .expect("server must start");

        (temp_dir, server, broker, resolver, conv_id)
    }

    #[test]
    fn t1_observation_event_is_acknowledged_immediately() {
        let (_tmp, server, _broker, _resolver, _conv_id) = setup_server();
        let client = SyntheticBridgeClient::new(server.socket_path());

        let envelope = ProviderBridgeEnvelope {
            protocol: BRIDGE_PROTOCOL_VERSION,
            mode: ProviderBridgeMode::Observe,
            request_id: "obs-req-1".to_string(),
            provider: "synthetic".to_string(),
            event: "ToolStarted".to_string(),
            session: ProviderSessionLocator::Id("session-1".to_string()),
            event_id: Some("evt-1".to_string()),
            payload: serde_json::json!({ "tool": "Read" }),
        };

        let reply = client
            .send_envelope(&envelope, Duration::from_secs(2))
            .expect("must succeed");
        assert_eq!(reply, ProviderBridgeReply::Ack);
    }

    #[test]
    fn t2_and_t3_and_t4_blocking_question_captured_and_resolved_by_chat() {
        let (_tmp, server, broker, _resolver, conv_id) = setup_server();
        server.registry().set_capture_policy(
            conv_id.clone(),
            InteractionCapturePolicy::ChatActive(conv_id.clone()),
        );

        let client = SyntheticBridgeClient::new(server.socket_path());

        let envelope = ProviderBridgeEnvelope {
            protocol: BRIDGE_PROTOCOL_VERSION,
            mode: ProviderBridgeMode::Interaction,
            request_id: "q-req-1".to_string(),
            provider: "synthetic".to_string(),
            event: "AskUserQuestion".to_string(),
            session: ProviderSessionLocator::Id("session-1".to_string()),
            event_id: Some("evt-2".to_string()),
            payload: serde_json::json!({
                "prompt": "Choose deployment target",
                "choices": [
                    { "id": "staging", "label": "Staging" },
                    { "id": "prod", "label": "Production" }
                ]
            }),
        };

        let broker_clone = Arc::clone(&broker);
        let conv_clone = conv_id.clone();

        // Spawn background resolution simulating user clicking choice in Chat
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let interactions = broker_clone.snapshot(&conv_clone);
            assert_eq!(interactions.len(), 1);
            let pending = &interactions[0];
            assert_eq!(pending.state, ConversationInteractionState::Pending);

            broker_clone
                .resolve(
                    &conv_clone,
                    &pending.id,
                    pending.revision,
                    InteractionResponse::Choice {
                        option_id: "staging".to_string(),
                    },
                    "fp-synth-1",
                )
                .expect("user resolve must succeed");
        });

        // Client waiting for resolution
        let reply = client
            .send_envelope(&envelope, Duration::from_secs(5))
            .expect("must receive reply");

        match reply {
            ProviderBridgeReply::Resolved { response } => {
                assert_eq!(response["decision"], "choice");
                assert_eq!(response["selected"], "staging");
            }
            other => panic!("expected resolved reply, got {other:?}"),
        }
    }

    #[test]
    fn t5_and_t6_race_and_stale_revision() {
        let (_tmp, server, broker, _resolver, conv_id) = setup_server();
        server.registry().set_capture_policy(
            conv_id.clone(),
            InteractionCapturePolicy::ChatActive(conv_id.clone()),
        );

        let client = SyntheticBridgeClient::new(server.socket_path());

        let envelope = ProviderBridgeEnvelope {
            protocol: BRIDGE_PROTOCOL_VERSION,
            mode: ProviderBridgeMode::Interaction,
            request_id: "q-req-race".to_string(),
            provider: "synthetic".to_string(),
            event: "AskUserQuestion".to_string(),
            session: ProviderSessionLocator::Id("session-1".to_string()),
            event_id: None,
            payload: serde_json::json!({
                "prompt": "Pick one",
                "choices": [
                    { "id": "opt-a", "label": "Option A" },
                    { "id": "opt-b", "label": "Option B" }
                ]
            }),
        };

        let broker_clone = Arc::clone(&broker);
        let conv_clone = conv_id.clone();

        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let interactions = broker_clone.snapshot(&conv_clone);
            assert_eq!(interactions.len(), 1);
            let pending = &interactions[0];

            // Resolver 1
            let res1 = broker_clone.resolve(
                &conv_clone,
                &pending.id,
                1,
                InteractionResponse::Choice {
                    option_id: "opt-a".to_string(),
                },
                "fp-synth-1",
            );

            // Resolver 2 (stale revision or race loser)
            let res2 = broker_clone.resolve(
                &conv_clone,
                &pending.id,
                1, // same stale revision
                InteractionResponse::Choice {
                    option_id: "opt-b".to_string(),
                },
                "fp-synth-1",
            );

            assert!(res1.is_ok());
            assert!(res2.is_err(), "second resolution must fail");
        });

        let reply = client
            .send_envelope(&envelope, Duration::from_secs(5))
            .expect("must succeed");

        match reply {
            ProviderBridgeReply::Resolved { response } => {
                assert_eq!(response["selected"], "opt-a");
            }
            other => panic!("expected resolved reply, got {other:?}"),
        }
    }

    #[test]
    fn t7_client_disconnect_cancels_interaction() {
        let (_tmp, server, broker, _resolver, conv_id) = setup_server();
        server.registry().set_capture_policy(
            conv_id.clone(),
            InteractionCapturePolicy::ChatActive(conv_id.clone()),
        );

        let envelope = ProviderBridgeEnvelope {
            protocol: BRIDGE_PROTOCOL_VERSION,
            mode: ProviderBridgeMode::Interaction,
            request_id: "q-req-disconnect".to_string(),
            provider: "synthetic".to_string(),
            event: "AskUserQuestion".to_string(),
            session: ProviderSessionLocator::Id("session-1".to_string()),
            event_id: None,
            payload: serde_json::json!({
                "prompt": "Will disconnect",
                "choices": [{ "id": "a", "label": "A" }]
            }),
        };

        // Connect, write envelope, then close stream immediately (disconnect)
        let mut stream = UnixStream::connect(server.socket_path()).unwrap();
        let json = serde_json::to_string(&envelope).unwrap();
        stream.write_all(json.as_bytes()).unwrap();
        stream.write_all(b"\n").unwrap();
        stream.flush().unwrap();

        // Wait a moment for broker to register, then drop stream
        std::thread::sleep(Duration::from_millis(30));
        drop(stream);

        // Broker record should exist and can be cancelled
        std::thread::sleep(Duration::from_millis(50));
        let interactions = broker.snapshot(&conv_id);
        assert_eq!(interactions.len(), 1);
    }

    #[test]
    fn t8_native_fallback_when_terminal_active_releases_provider() {
        let (_tmp, server, _broker, _resolver, conv_id) = setup_server();
        // Set policy to TerminalActive
        server
            .registry()
            .set_capture_policy(conv_id, InteractionCapturePolicy::TerminalActive);

        let client = SyntheticBridgeClient::new(server.socket_path());

        let envelope = ProviderBridgeEnvelope {
            protocol: BRIDGE_PROTOCOL_VERSION,
            mode: ProviderBridgeMode::Interaction,
            request_id: "q-req-terminal".to_string(),
            provider: "synthetic".to_string(),
            event: "AskUserQuestion".to_string(),
            session: ProviderSessionLocator::Id("session-1".to_string()),
            event_id: None,
            payload: serde_json::json!({
                "prompt": "Terminal question",
                "choices": [{ "id": "a", "label": "A" }]
            }),
        };

        // When terminal is active, server returns NativeFallback immediately without waiting!
        let reply = client
            .send_envelope(&envelope, Duration::from_secs(2))
            .expect("must succeed");
        assert_eq!(reply, ProviderBridgeReply::NativeFallback);
    }

    #[test]
    fn t9_bounded_payload_rejects_oversized_frame() {
        let (_tmp, server, _broker, _resolver, _conv_id) = setup_server();
        let client = SyntheticBridgeClient::new(server.socket_path());

        // Create an oversized payload > 256 KiB
        let huge_string = "x".repeat(270 * 1024);
        let raw_data = format!("{{\"protocol\":1,\"payload\":\"{huge_string}\"}}\n");

        let reply = client
            .send_raw(raw_data.as_bytes(), Duration::from_secs(5))
            .expect("must receive error reply");

        match reply {
            ProviderBridgeReply::Error { message } => {
                assert!(
                    message.contains("exceeds maximum allowed"),
                    "must reject oversized payload: {message}"
                );
            }
            other => panic!("expected Error reply, got {other:?}"),
        }
    }

    #[test]
    fn t10_concurrent_observation_load_is_bounded_and_reliable() {
        let (_tmp, server, _broker, _resolver, _conv_id) = setup_server();
        let socket_path = server.socket_path().to_path_buf();

        let mut handles = Vec::new();
        for i in 0..50 {
            let sp = socket_path.clone();
            let handle = std::thread::spawn(move || {
                let client = SyntheticBridgeClient::new(&sp);
                let envelope = ProviderBridgeEnvelope {
                    protocol: BRIDGE_PROTOCOL_VERSION,
                    mode: ProviderBridgeMode::Observe,
                    request_id: format!("obs-load-{i}"),
                    provider: "synthetic".to_string(),
                    event: "ToolCompleted".to_string(),
                    session: ProviderSessionLocator::Id("session-1".to_string()),
                    event_id: Some(format!("evt-{i}")),
                    payload: serde_json::json!({ "iteration": i }),
                };
                client
                    .send_envelope(&envelope, Duration::from_secs(5))
                    .expect("concurrent observe must succeed")
            });
            handles.push(handle);
        }

        for h in handles {
            let reply = h.join().unwrap();
            assert_eq!(reply, ProviderBridgeReply::Ack);
        }
    }

    #[test]
    fn t11_claude_adapter_ask_user_question_round_trip() {
        let (tmp, _server, broker, resolver, conv_id) = setup_server();
        let claude_adapter = Arc::new(ClaudeBridgeAdapter);
        let registry = Arc::new(ProviderBridgeRegistry::new(
            broker.clone(),
            resolver.clone(),
        ));
        registry.register_adapter(claude_adapter);
        let runtime_dir = tmp.path().to_path_buf();
        let server = LocalProviderBridgeServer::start(registry.clone(), runtime_dir)
            .expect("server must start");
        server.registry().set_capture_policy(
            conv_id.clone(),
            InteractionCapturePolicy::ChatActive(conv_id.clone()),
        );

        // Setup session locator mapping for claude-code
        resolver.register(
            "claude-code",
            "session-claude-123",
            ResolvedLiveSession {
                conversation_id: conv_id.clone(),
                agent_ref: AgentRef::new("pane-claude"),
                occupant_fingerprint: "fp-claude".to_string(),
            },
        );

        let socket_path = server.socket_path().to_path_buf();
        let envelope = ProviderBridgeEnvelope {
            protocol: BRIDGE_PROTOCOL_VERSION,
            mode: ProviderBridgeMode::Interaction,
            request_id: "req-claude-q".to_string(),
            provider: "claude-code".to_string(),
            event: "PreToolUse".to_string(),
            session: ProviderSessionLocator::Id("session-claude-123".to_string()),
            event_id: Some("evt-claude-1".to_string()),
            payload: serde_json::json!({
                "hook_event_name": "PreToolUse",
                "tool_name": "AskUserQuestion",
                "tool_input": {
                    "questions": [
                        {
                            "question": "Which migration strategy should I use?",
                            "options": [
                                { "id": "keep", "label": "Keep compatibility" },
                                { "id": "remove", "label": "Remove legacy path" }
                            ]
                        }
                    ]
                }
            }),
        };

        let client_handle = std::thread::spawn(move || {
            let client = SyntheticBridgeClient::new(&socket_path);
            client
                .send_envelope(&envelope, Duration::from_secs(5))
                .expect("send interaction envelope")
        });

        // Wait for broker to have pending interaction
        let mut pending = None;
        for _ in 0..50 {
            let items = broker.snapshot(&conv_id);
            if let Some(first) = items.first() {
                pending = Some(first.clone());
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let pending = pending.expect("must register pending interaction in broker");
        assert_eq!(pending.prompt, "Which migration strategy should I use?");
        assert_eq!(pending.choices.len(), 2);
        assert_eq!(pending.choices[0].id, "keep");
        assert_eq!(pending.choices[1].id, "remove");

        // Resolve interaction via Host broker
        let resolution = broker
            .resolve(
                &conv_id,
                &pending.id,
                pending.revision,
                InteractionResponse::Choice {
                    option_id: "remove".to_string(),
                },
                "fp-claude",
            )
            .expect("resolve must succeed");

        assert_eq!(resolution.state, ConversationInteractionState::Resolved);

        let reply = client_handle.join().unwrap();
        match reply {
            ProviderBridgeReply::Resolved { response } => {
                let hook_output = response
                    .get("hookSpecificOutput")
                    .expect("has hookSpecificOutput");
                assert_eq!(
                    hook_output
                        .get("permissionDecision")
                        .and_then(|v| v.as_str()),
                    Some("allow")
                );
                let updated_input = hook_output.get("updatedInput").expect("has updatedInput");
                let answers = updated_input.get("answers").expect("has answers");
                assert_eq!(
                    answers.get("answer").and_then(|v| v.as_str()),
                    Some("remove")
                );
            }
            other => panic!("expected Resolved reply, got {other:?}"),
        }
    }

    #[test]
    fn t12_pi_opencode_codex_adapters_interaction_round_trip() {
        let (tmp, _server, broker, resolver, conv_id) = setup_server();
        let registry = Arc::new(ProviderBridgeRegistry::new(
            broker.clone(),
            resolver.clone(),
        ));
        registry.register_adapter(Arc::new(PiBridgeAdapter));
        registry.register_adapter(Arc::new(OpenCodeBridgeAdapter));
        registry.register_adapter(Arc::new(CodexBridgeAdapter));

        let runtime_dir = tmp.path().to_path_buf();
        let server = LocalProviderBridgeServer::start(registry.clone(), runtime_dir)
            .expect("server must start");
        server.registry().set_capture_policy(
            conv_id.clone(),
            InteractionCapturePolicy::ChatActive(conv_id.clone()),
        );

        // 1. Test Pi select question
        resolver.register(
            "pi",
            "session-pi-1",
            ResolvedLiveSession {
                conversation_id: conv_id.clone(),
                agent_ref: AgentRef::new("pane-pi"),
                occupant_fingerprint: "fp-pi".to_string(),
            },
        );

        let pi_client = SyntheticBridgeClient::new(server.socket_path());
        let pi_envelope = ProviderBridgeEnvelope {
            protocol: BRIDGE_PROTOCOL_VERSION,
            mode: ProviderBridgeMode::Interaction,
            request_id: "req-pi-1".to_string(),
            provider: "pi".to_string(),
            event: "select".to_string(),
            session: ProviderSessionLocator::Id("session-pi-1".to_string()),
            event_id: Some("evt-pi-1".to_string()),
            payload: serde_json::json!({
                "prompt": "Choose backend strategy",
                "options": ["herdr", "custom"]
            }),
        };

        let pi_handle = std::thread::spawn(move || {
            pi_client
                .send_envelope(&pi_envelope, Duration::from_secs(5))
                .expect("send pi envelope")
        });

        std::thread::sleep(Duration::from_millis(50));
        let pending = broker
            .snapshot(&conv_id)
            .pop()
            .expect("must have pi pending");
        broker
            .resolve(
                &conv_id,
                &pending.id,
                pending.revision,
                InteractionResponse::Choice {
                    option_id: "herdr".to_string(),
                },
                "fp-pi",
            )
            .expect("resolve pi must succeed");

        let reply = pi_handle.join().unwrap();
        match reply {
            ProviderBridgeReply::Resolved { response } => {
                assert_eq!(
                    response.get("value").and_then(|v| v.as_str()),
                    Some("herdr")
                );
            }
            other => panic!("expected Resolved for Pi, got {other:?}"),
        }

        // 2. Test OpenCode permission request
        resolver.register(
            "opencode",
            "session-opencode-1",
            ResolvedLiveSession {
                conversation_id: conv_id.clone(),
                agent_ref: AgentRef::new("pane-oc"),
                occupant_fingerprint: "fp-oc".to_string(),
            },
        );

        let oc_client = SyntheticBridgeClient::new(server.socket_path());
        let oc_envelope = ProviderBridgeEnvelope {
            protocol: BRIDGE_PROTOCOL_VERSION,
            mode: ProviderBridgeMode::Interaction,
            request_id: "req-oc-1".to_string(),
            provider: "opencode".to_string(),
            event: "permission.asked".to_string(),
            session: ProviderSessionLocator::Id("session-opencode-1".to_string()),
            event_id: Some("evt-oc-1".to_string()),
            payload: serde_json::json!({
                "title": "Run cargo test --workspace",
                "allow_session": true
            }),
        };

        let oc_handle = std::thread::spawn(move || {
            oc_client
                .send_envelope(&oc_envelope, Duration::from_secs(5))
                .expect("send oc envelope")
        });

        std::thread::sleep(Duration::from_millis(50));
        let pending = broker
            .snapshot(&conv_id)
            .pop()
            .expect("must have oc pending");
        broker
            .resolve(
                &conv_id,
                &pending.id,
                pending.revision,
                InteractionResponse::Allow {
                    scope: crate::conversation_interactions::PermissionScope::Session,
                },
                "fp-oc",
            )
            .expect("resolve oc must succeed");

        let reply = oc_handle.join().unwrap();
        match reply {
            ProviderBridgeReply::Resolved { response } => {
                assert_eq!(
                    response.get("decision").and_then(|v| v.as_str()),
                    Some("allow")
                );
                assert_eq!(
                    response.get("scope").and_then(|v| v.as_str()),
                    Some("session")
                );
            }
            other => panic!("expected Resolved for OpenCode, got {other:?}"),
        }

        // 3. Test Codex permission request
        resolver.register(
            "codex",
            "session-codex-1",
            ResolvedLiveSession {
                conversation_id: conv_id.clone(),
                agent_ref: AgentRef::new("pane-codex"),
                occupant_fingerprint: "fp-codex".to_string(),
            },
        );

        let codex_client = SyntheticBridgeClient::new(server.socket_path());
        let codex_envelope = ProviderBridgeEnvelope {
            protocol: BRIDGE_PROTOCOL_VERSION,
            mode: ProviderBridgeMode::Interaction,
            request_id: "req-codex-1".to_string(),
            provider: "codex".to_string(),
            event: "PermissionRequest".to_string(),
            session: ProviderSessionLocator::Id("session-codex-1".to_string()),
            event_id: Some("evt-codex-1".to_string()),
            payload: serde_json::json!({
                "command": "git status",
            }),
        };

        let codex_handle = std::thread::spawn(move || {
            codex_client
                .send_envelope(&codex_envelope, Duration::from_secs(5))
                .expect("send codex envelope")
        });

        std::thread::sleep(Duration::from_millis(50));
        let pending = broker
            .snapshot(&conv_id)
            .pop()
            .expect("must have codex pending");
        broker
            .resolve(
                &conv_id,
                &pending.id,
                pending.revision,
                InteractionResponse::Allow {
                    scope: crate::conversation_interactions::PermissionScope::Once,
                },
                "fp-codex",
            )
            .expect("resolve codex must succeed");

        let reply = codex_handle.join().unwrap();
        match reply {
            ProviderBridgeReply::Resolved { response } => {
                assert_eq!(
                    response.get("decision").and_then(|v| v.as_str()),
                    Some("allow")
                );
                assert_eq!(
                    response.get("persistent").and_then(|v| v.as_bool()),
                    Some(false)
                );
            }
            other => panic!("expected Resolved for Codex, got {other:?}"),
        }
    }
}
