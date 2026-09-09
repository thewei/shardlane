//! Adapter contract kit (docs/multiplexer-api.md §9). Every backend adapter
//! must pass these contracts; the Herdr adapter runs the scripted-server suite
//! in unit tests, and live backends (tmux, real Herdr) run
//! [`live_structural_contract`] against an isolated instance.
//!
//! The kit is the seam's regression gate: a backend cannot silently drift the
//! neutral contract, and a capability declared false must degrade through the
//! trait's typed Unsupported defaults.

use super::{
    CreateTab, CreateWorkspace, Multiplexer, MultiplexerConnection, MuxCapabilities, MuxError,
};

/// Facet/coherence contract that needs only the backend handle.
pub fn backend_contract(backend: &dyn Multiplexer) {
    let capabilities = backend.capabilities();
    assert_eq!(
        backend.server_admin().is_some(),
        capabilities.server_admin,
        "backend {} server_admin facet must match its capability",
        backend.id()
    );
}

/// Facet/coherence contract for one opened connection.
pub fn connection_contract(connection: &dyn MultiplexerConnection, capabilities: MuxCapabilities) {
    assert_eq!(
        connection.agent_runtime().is_some(),
        capabilities.agents,
        "agent_runtime facet must match the agents capability"
    );
    if !capabilities.pane_history_read {
        assert!(
            matches!(
                connection.read_pane_history("pane", 10),
                Err(MuxError::Unsupported("pane_history_read"))
            ),
            "pane_history_read=false must degrade to typed Unsupported"
        );
    }
    if !capabilities.server_admin {
        assert!(
            matches!(
                connection.reload_config(),
                Err(MuxError::Unsupported("server_admin"))
            ),
            "server_admin=false must degrade to typed Unsupported"
        );
    }
}

/// Live structural round-trip against a scratch workspace. Requires a backend
/// that tolerates mutations (run only against an isolated instance — never a
/// user-owned runtime).
pub fn live_structural_contract(connection: &dyn MultiplexerConnection) -> Result<(), MuxError> {
    let workspace = connection.create_workspace(&CreateWorkspace {
        cwd: None,
        focus: false,
    })?;
    let workspace_id = workspace.workspace.workspace_id.clone();

    let tab = connection.create_tab(&CreateTab {
        workspace_id: Some(&workspace_id),
        cwd: None,
        focus: false,
    })?;
    let tab_id = tab.tab.tab_id.clone();
    let root_pane_id = tab.root_pane.pane_id.clone();

    connection.rename_tab(&tab_id, "mux-kit")?;
    connection.rename_workspace(&workspace_id, "mux-kit-ws")?;

    let surface = connection.tab_surface_state(&workspace_id, &tab_id)?;
    assert!(
        surface
            .panes
            .iter()
            .any(|pane| pane.pane_id == root_pane_id),
        "created root pane must appear in the tab surface projection"
    );
    let navigation = connection.navigation_state()?;
    assert!(
        navigation
            .workspaces
            .iter()
            .any(|workspace| workspace.workspace_id == workspace_id),
        "created workspace must appear in the navigation projection"
    );

    connection.close_tab(&tab_id)?;
    connection.close_workspace(&workspace_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    use serde_json::{json, Value};

    use super::super::herdr::{instance_listing, HerdrBackend};
    use super::super::registry::MuxRegistry;
    use super::super::{
        CreateTab, CreateWorkspace, InstanceRef, InstanceTarget, Multiplexer,
        MultiplexerConnection, MuxDirection, MuxError, SplitDirection,
    };
    use crate::herdr::{HerdrClient, HerdrError, HerdrSessionListing};

    /// Minimal scripted Herdr socket server: answers each request line with
    /// the canned response for its method, records every request, and closes
    /// the connection (the client opens one connection per RPC).
    struct ScriptedHerdr {
        socket_path: PathBuf,
        requests: Arc<Mutex<Vec<String>>>,
        dir: PathBuf,
    }

    static SCRATCH_COUNTER: AtomicU32 = AtomicU32::new(0);

    impl ScriptedHerdr {
        fn start(responses: BTreeMap<&'static str, Value>) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "shardlane-mux-kit-{}-{}",
                std::process::id(),
                SCRATCH_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create kit scratch dir: {e}"));
            let socket_path = dir.join("kit.sock");
            let listener =
                UnixListener::bind(&socket_path).unwrap_or_else(|e| panic!("bind kit socket: {e}"));
            let expected: BTreeMap<String, Value> = responses
                .into_iter()
                .map(|(method, response)| (method.to_string(), response))
                .collect();
            let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            let recorder = requests.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { break };
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        continue;
                    }
                    let Ok(request) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    let method = request
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    recorder
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push(method.clone());
                    let response = expected.get(&method).cloned().unwrap_or_else(|| {
                        json!({"error": {"code": "unscripted", "message": format!("no canned response for {method}")}})
                    });
                    let _ = writeln!(reader.get_mut(), "{response}");
                }
            });
            Self {
                socket_path,
                requests,
                dir,
            }
        }

        fn client(&self) -> HerdrClient {
            HerdrClient::for_test_socket(self.socket_path.clone())
        }

        fn reference(&self) -> InstanceRef {
            InstanceRef::socket("herdr", self.socket_path.clone())
        }

        fn methods(&self) -> Vec<String> {
            self.requests
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }
    }

    impl Drop for ScriptedHerdr {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn ok_result(payload: Value) -> Value {
        json!({ "result": payload })
    }

    fn workspace_json(id: &str, focused: bool) -> Value {
        json!({
            "workspace_id": id,
            "label": format!("ws-{id}"),
            "focused": focused,
        })
    }

    fn tab_json(id: &str, workspace_id: &str, focused: bool) -> Value {
        json!({
            "tab_id": id,
            "workspace_id": workspace_id,
            "label": format!("tab-{id}"),
            "focused": focused,
        })
    }

    fn pane_json(id: &str, workspace_id: &str, tab_id: &str) -> Value {
        json!({
            "pane_id": id,
            "workspace_id": workspace_id,
            "tab_id": tab_id,
            "focused": true,
        })
    }

    fn layout_json(tab_id: &str, pane_id: &str) -> Value {
        json!({
            "tab_id": tab_id,
            "workspace_id": "w1",
            "area": {"x": 0, "y": 0, "width": 80, "height": 24},
            "panes": [{"pane_id": pane_id, "rect": {"x": 0, "y": 0, "width": 80, "height": 24}, "focused": true}],
            "splits": [],
            "focused_pane_id": pane_id,
            "zoomed": false,
        })
    }

    fn snapshot_responses() -> BTreeMap<&'static str, Value> {
        BTreeMap::from([
            (
                "workspace.list",
                ok_result(json!({"workspaces": [workspace_json("w1", true)]})),
            ),
            (
                "tab.list",
                ok_result(json!({"tabs": [tab_json("t1", "w1", true)]})),
            ),
            (
                "pane.list",
                ok_result(json!({"panes": [pane_json("p1", "w1", "t1")]})),
            ),
            ("agent.list", ok_result(json!({"agents": []}))),
            (
                "pane.layout",
                ok_result(json!({"layout": layout_json("t1", "p1")})),
            ),
        ])
    }

    #[test]
    fn herdr_error_maps_to_mux_error_preserving_semantics() {
        let pairs: Vec<(HerdrError, MuxError)> = vec![
            (
                HerdrError::InstallFailed("x".into()),
                MuxError::InstallFailed("x".into()),
            ),
            (
                HerdrError::SocketUnavailable("s".into(), "d".into()),
                MuxError::SocketUnavailable("s".into(), "d".into()),
            ),
            (HerdrError::Api("a".into()), MuxError::Api("a".into())),
            (
                HerdrError::AgentNotFound("n".into()),
                MuxError::NotFound("n".into()),
            ),
            (
                HerdrError::AgentBlocked("b".into()),
                MuxError::Blocked("b".into()),
            ),
            (
                HerdrError::AgentPromptStalled("p".into()),
                MuxError::PromptStalled("p".into()),
            ),
            (
                HerdrError::ApiTimeout("t".into()),
                MuxError::Timeout("t".into()),
            ),
            (
                HerdrError::DeliveryUncertain("u".into()),
                MuxError::Uncertain("u".into()),
            ),
        ];
        for (herdr, expected) in pairs {
            let mapped: MuxError = herdr.into();
            assert_eq!(mapped.to_string(), expected.to_string());
        }
        assert!(matches!(
            MuxError::from(HerdrError::IncompatibleProtocol {
                min: 20,
                actual: 19
            }),
            MuxError::IncompatibleProtocol {
                min: 20,
                actual: 19
            }
        ));
    }

    #[test]
    fn herdr_backend_facets_match_capabilities() {
        let backend = HerdrBackend::default();
        assert_eq!(backend.id(), "herdr");
        let capabilities = backend.capabilities();
        assert!(capabilities.agents && capabilities.server_admin && capabilities.shared_tui);
        assert!(capabilities.pane_history_read && capabilities.events_push);
        assert!(!capabilities.cross_workspace_tab_move);
        super::backend_contract(&backend);
    }

    #[test]
    fn instance_listing_preserves_session_fields() {
        let listing = instance_listing(
            "herdr",
            HerdrSessionListing {
                name: "main".to_string(),
                running: true,
                is_default: false,
            },
        );
        assert_eq!(listing.backend, "herdr");
        assert_eq!(listing.name, "main");
        assert!(listing.running);
        assert!(!listing.is_default);
        assert!(listing.display_name.is_none());
    }

    #[test]
    fn connection_snapshots_delegate_through_the_adapter() {
        let server = ScriptedHerdr::start(snapshot_responses());
        let connection: Arc<dyn MultiplexerConnection> = Arc::new(server.client());

        let navigation = connection
            .navigation_state()
            .unwrap_or_else(|e| panic!("navigation: {e}"));
        assert_eq!(navigation.focused_workspace_id.as_deref(), Some("w1"));
        assert_eq!(navigation.focused_tab_id.as_deref(), Some("t1"));
        assert_eq!(navigation.workspaces.len(), 1);

        let state = connection
            .visible_state()
            .unwrap_or_else(|e| panic!("visible state: {e}"));
        assert_eq!(state.panes.len(), 1);
        assert_eq!(state.panes[0].pane_id, "p1");
        assert_eq!(state.layouts.len(), 1);

        let panes = connection
            .workspace_panes("w1")
            .unwrap_or_else(|e| panic!("workspace panes: {e}"));
        assert_eq!(panes[0].tab_id.as_deref(), Some("t1"));

        let surface = connection
            .tab_surface_state("w1", "t1")
            .unwrap_or_else(|e| panic!("tab surface: {e}"));
        assert_eq!(surface.focused_pane_id.as_deref(), Some("p1"));

        let agents = connection
            .agents()
            .unwrap_or_else(|e| panic!("agents: {e}"));
        assert!(agents.is_empty());

        let methods = server.methods();
        for expected in [
            "workspace.list",
            "tab.list",
            "pane.list",
            "agent.list",
            "pane.layout",
        ] {
            assert!(
                methods.contains(&expected.to_string()),
                "missing {expected}"
            );
        }
    }

    #[test]
    fn structural_operations_carry_neutral_params_and_payloads() {
        let mut responses = BTreeMap::from([
            (
                "tab.create",
                ok_result(
                    json!({"tab": tab_json("t2", "w1", false), "root_pane": pane_json("p2", "w1", "t2")}),
                ),
            ),
            ("tab.rename", ok_result(json!({}))),
            ("tab.move", ok_result(json!({"type": "ok"}))),
            ("tab.close", ok_result(json!({}))),
            (
                "workspace.create",
                ok_result(json!({
                    "workspace": workspace_json("w9", false),
                    "tab": tab_json("t9", "w9", false),
                    "root_pane": pane_json("p9", "w9", "t9"),
                })),
            ),
            ("workspace.rename", ok_result(json!({}))),
            (
                "pane.split",
                ok_result(json!({"pane": pane_json("p3", "w1", "t1")})),
            ),
            ("pane.send_text", ok_result(json!({}))),
            (
                "pane.read",
                ok_result(json!({"read": {
                    "pane_id": "p1", "workspace_id": "w1", "tab_id": "t1",
                    "source": "recent", "format": "ansi",
                    "text": "\u{1b}[31mhello", "revision": 7, "truncated": false,
                }})),
            ),
            (
                "pane.swap",
                ok_result(json!({"swap": {"layout": layout_json("t1", "p1")}})),
            ),
            (
                "pane.resize",
                ok_result(json!({"resize": {"layout": layout_json("t1", "p1")}})),
            ),
            ("server.reload_config", ok_result(json!({}))),
        ]);
        responses.insert("pane.list", ok_result(json!({"panes": []})));
        responses.insert("agent.list", ok_result(json!({"agents": []})));
        let server = ScriptedHerdr::start(responses);
        let connection: Arc<dyn MultiplexerConnection> = Arc::new(server.client());

        let tab = connection
            .create_tab(&CreateTab {
                workspace_id: Some("w1"),
                cwd: Some("/tmp/proj"),
                focus: false,
            })
            .unwrap_or_else(|e| panic!("create tab: {e}"));
        assert_eq!(tab.tab.tab_id, "t2");
        assert_eq!(tab.root_pane.pane_id, "p2");

        connection
            .rename_tab("t2", "build")
            .unwrap_or_else(|e| panic!("rename tab: {e}"));
        connection
            .move_tab("t2", 0)
            .unwrap_or_else(|e| panic!("move tab: {e}"));
        connection
            .close_tab("t2")
            .unwrap_or_else(|e| panic!("close tab: {e}"));

        let workspace = connection
            .create_workspace(&CreateWorkspace {
                cwd: None,
                focus: false,
            })
            .unwrap_or_else(|e| panic!("create workspace: {e}"));
        assert_eq!(workspace.workspace.workspace_id, "w9");
        connection
            .rename_workspace("w9", "renamed")
            .unwrap_or_else(|e| panic!("rename workspace: {e}"));

        let split = connection
            .split_pane("p1", SplitDirection::Right)
            .unwrap_or_else(|e| panic!("split right: {e}"));
        assert_eq!(split.pane_id, "p3");
        connection
            .split_pane("p1", SplitDirection::Down)
            .unwrap_or_else(|e| panic!("split down: {e}"));
        connection
            .send_text("p1", "cargo test\n")
            .unwrap_or_else(|e| panic!("send text: {e}"));

        let history = connection
            .read_pane_history("p1", 100)
            .unwrap_or_else(|e| panic!("read pane history: {e}"));
        assert_eq!(history.pane_id, "p1");
        assert!(history.text.contains("hello"));

        let swap = connection
            .swap_pane("p1", MuxDirection::Up)
            .unwrap_or_else(|e| panic!("swap: {e}"));
        assert_eq!(swap.layout.tab_id, "t1");
        let resize = connection
            .resize_pane("p1", MuxDirection::Up)
            .unwrap_or_else(|e| panic!("resize: {e}"));
        assert_eq!(resize.layout.tab_id, "t1");
        connection
            .reload_config()
            .unwrap_or_else(|e| panic!("reload config: {e}"));

        let methods = server.methods();
        assert_eq!(
            methods.iter().filter(|m| *m == "pane.split").count(),
            2,
            "both split directions must delegate"
        );

        let requests_text = format!("{:?}", server.methods());
        let _ = requests_text;
    }

    #[test]
    fn herdr_adapter_declares_full_capabilities_so_no_gated_default_is_hit() {
        // Coherence for the production adapter: every gated facet is
        // implemented, so the trait defaults must never fire.
        let server = ScriptedHerdr::start(BTreeMap::from([
            ("server.reload_config", ok_result(json!({}))),
            (
                "pane.read",
                ok_result(json!({"read": {
                    "pane_id": "p1", "workspace_id": "w1", "tab_id": "t1",
                    "source": "recent", "format": "ansi", "text": "x",
                    "revision": 1, "truncated": false,
                }})),
            ),
        ]));
        let connection: Arc<dyn MultiplexerConnection> = Arc::new(server.client());
        super::connection_contract(connection.as_ref(), HerdrBackend::default().capabilities());
    }

    #[test]
    fn registry_routes_by_backend_and_rejects_unknown() {
        let server = ScriptedHerdr::start(BTreeMap::from([(
            "ping",
            ok_result(json!({"protocol": 20, "version": "mux-kit"})),
        )]));
        let registry = MuxRegistry::with_builtins();
        assert_eq!(
            registry.backends().len(),
            4,
            "builtins = herdr + tmux + uuyc + luvus"
        );

        let connection = registry
            .connect_instance(&server.reference())
            .unwrap_or_else(|e| panic!("route herdr ref: {e}"));
        assert!(connection.as_herdr().is_some());

        let unknown = registry.connect_instance(&InstanceRef {
            backend: "no-such-backend".to_string(),
            target: InstanceTarget::Default,
        });
        assert!(matches!(unknown, Err(MuxError::Api(_))));
    }

    /// Capability-degraded backend double proving the trait defaults degrade
    /// to typed Unsupported (kit `connection_contract` exercised on the
    /// `false` path).
    struct DegradedBackend;

    struct DegradedConnection;

    impl Multiplexer for DegradedBackend {
        fn id(&self) -> &'static str {
            "degraded"
        }
        fn capabilities(&self) -> super::super::MuxCapabilities {
            super::super::MuxCapabilities {
                agents: false,
                server_admin: false,
                shared_tui: false,
                pane_history_read: false,
                cross_workspace_tab_move: false,
                events_push: false,
            }
        }
        fn list_instances(&self) -> Option<Vec<super::super::InstanceListing>> {
            None
        }
        fn rename_instance(&self, _instance: &str, _display_name: &str) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("instance_metadata"))
        }
        fn stop_instance(&self, _instance: &str) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("instance_lifecycle"))
        }
        fn delete_instance(&self, _instance: &str) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("instance_lifecycle"))
        }
        fn open_instance(
            &self,
            _reference: &InstanceRef,
        ) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
            Ok(Arc::new(DegradedConnection))
        }
        fn connect_instance(
            &self,
            _reference: &InstanceRef,
        ) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
            Ok(Arc::new(DegradedConnection))
        }
        fn server_admin(&self) -> Option<&dyn super::super::MultiplexerServerAdmin> {
            None
        }
    }

    impl MultiplexerConnection for DegradedConnection {
        fn capabilities(&self) -> super::super::MuxCapabilities {
            super::super::MuxCapabilities {
                agents: false,
                server_admin: false,
                shared_tui: false,
                pane_history_read: false,
                cross_workspace_tab_move: false,
                events_push: false,
            }
        }

        fn ping(&self) -> Result<(), MuxError> {
            Ok(())
        }
        fn protocol(&self) -> Option<u32> {
            None
        }
        fn server_started_with_supplied_config(&self) -> bool {
            false
        }
        fn navigation_state(&self) -> Result<crate::herdr::NavigationState, MuxError> {
            Err(MuxError::Unsupported("snapshots"))
        }
        fn visible_state(&self) -> Result<super::super::MuxState, MuxError> {
            Err(MuxError::Unsupported("snapshots"))
        }
        fn host_bootstrap_state(&self) -> Result<super::super::MuxState, MuxError> {
            Err(MuxError::Unsupported("snapshots"))
        }
        fn workspace_state(&self) -> Result<super::super::MuxState, MuxError> {
            Err(MuxError::Unsupported("snapshots"))
        }
        fn workspace_panes(
            &self,
            _workspace_id: &str,
        ) -> Result<Vec<crate::herdr::Pane>, MuxError> {
            Err(MuxError::Unsupported("snapshots"))
        }
        fn tab_surface_state(
            &self,
            _workspace_id: &str,
            _tab_id: &str,
        ) -> Result<crate::herdr::TabSurfaceState, MuxError> {
            Err(MuxError::Unsupported("snapshots"))
        }
        fn pane_layout(&self, _pane_id: &str) -> Result<crate::herdr::PaneLayout, MuxError> {
            Err(MuxError::Unsupported("snapshots"))
        }
        fn agents(&self) -> Result<Vec<crate::herdr::Agent>, MuxError> {
            Err(MuxError::Unsupported("agents"))
        }
        fn subscribe_events(
            &self,
        ) -> Result<async_channel::Receiver<super::super::MuxEvent>, MuxError> {
            Err(MuxError::Unsupported("events_push"))
        }
        fn subscribe_pane_events(
            &self,
            _pane_ids: &[String],
        ) -> Result<async_channel::Receiver<super::super::MuxEvent>, MuxError> {
            Err(MuxError::Unsupported("events_push"))
        }
        fn create_workspace(
            &self,
            _params: &CreateWorkspace<'_>,
        ) -> Result<crate::herdr::WorkspaceCreatedResult, MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn close_workspace(&self, _workspace_id: &str) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn rename_workspace(&self, _workspace_id: &str, _label: &str) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn move_workspace(
            &self,
            _workspace_id: &str,
            _insert_index: usize,
        ) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn move_workspace_before(
            &self,
            _workspace_id: &str,
            _before_workspace_id: &str,
        ) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn workspace_focus(&self, _workspace_id: &str) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("focus"))
        }
        fn create_tab(
            &self,
            _params: &CreateTab<'_>,
        ) -> Result<crate::herdr::TabCreatedResult, MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn close_tab(&self, _tab_id: &str) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn rename_tab(&self, _tab_id: &str, _label: &str) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn move_tab(&self, _tab_id: &str, _insert_index: usize) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn tab_focus(&self, _tab_id: &str) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("focus"))
        }
        fn split_pane(
            &self,
            _pane_id: &str,
            _direction: super::super::SplitDirection,
        ) -> Result<crate::herdr::Pane, MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn close_pane(&self, _pane_id: &str) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn rename_pane(&self, _pane_id: &str, _label: &str) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn swap_pane(
            &self,
            _pane_id: &str,
            _direction: super::super::MuxDirection,
        ) -> Result<crate::herdr::PaneLayoutActionResult, MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn resize_pane(
            &self,
            _pane_id: &str,
            _direction: super::super::MuxDirection,
        ) -> Result<crate::herdr::PaneLayoutActionResult, MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn toggle_pane_zoom(
            &self,
            _pane_id: &str,
        ) -> Result<crate::herdr::PaneLayoutActionResult, MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn pane_focus(&self, _pane_id: &str) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("focus"))
        }
        fn move_pane_to_tab(
            &self,
            _pane_id: &str,
            _tab_id: &str,
        ) -> Result<crate::herdr::PaneMoveResult, MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn move_pane_to_new_tab(
            &self,
            _pane_id: &str,
            _workspace_id: &str,
        ) -> Result<crate::herdr::PaneMoveResult, MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn set_split_ratio(
            &self,
            _tab_id: &str,
            _path: &[bool],
            _ratio: f64,
        ) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("structural"))
        }
        fn send_text(&self, _pane_id: &str, _text: &str) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("input"))
        }
        fn send_keys(&self, _pane_id: &str, _keys: &[String]) -> Result<(), MuxError> {
            Err(MuxError::Unsupported("input"))
        }
        fn pane_process_info(
            &self,
            _pane_id: &str,
        ) -> Result<crate::herdr::PaneProcessInfo, MuxError> {
            Err(MuxError::Unsupported("process_info"))
        }
    }

    #[test]
    fn capability_degraded_connection_degrades_through_typed_defaults() {
        let backend = DegradedBackend;
        super::backend_contract(&backend);
        let connection = backend
            .open_instance(&InstanceRef::default_instance("degraded"))
            .unwrap_or_else(|e| panic!("open degraded instance: {e}"));
        super::connection_contract(connection.as_ref(), backend.capabilities());
        assert!(connection.agent_runtime().is_none());
        assert!(connection.as_herdr().is_none());
    }

    /// Live contract against the developer's real Herdr default instance.
    /// Opt-in only (it mutates runtime state by creating a scratch workspace):
    /// `cargo test -p shardlane-host mux_live -- --ignored` with
    /// `SHARDLANE_MUX_KIT_LIVE=1`.
    #[test]
    #[ignore]
    fn mux_live_structural_contract() {
        if std::env::var_os("SHARDLANE_MUX_KIT_LIVE").is_none() {
            return;
        }
        let registry = MuxRegistry::with_builtins();
        let connection = registry
            .open_instance(&InstanceRef::default_instance("herdr"))
            .unwrap_or_else(|e| panic!("open default herdr instance: {e}"));
        if let Err(error) = super::live_structural_contract(connection.as_ref()) {
            panic!("live contract failed: {error}");
        }
    }
}
