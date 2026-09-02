//! Remote API server: dedicated thread + independent Tokio runtime +
//! loopback listener.
//!
//! [INPUT]: Depends on crate::{config,auth,error,state}, shardlane-host DTOs
//! (HostInfo/HostCapabilities/HOST_API_VERSION) and diagnostics (lag_log),
//! plus axum/tokio (serving and lifecycle)
//! [OUTPUT]: Exposes RemoteServerOptions/RemoteServerHandle/
//! spawn_remote_server and the single `/api/v2` surface: hello/bootstrap,
//! Herdr workspace/tab/pane control (re-homed from the retired `/api/v1`
//! compatibility routes, 2026-09-01 v1 clearance), the events WebSocket at
//! `/api/v2/events`, semantic Conversation/History, and shared Herdr TUI
//! routes; build_router mounts authentication uniformly and stamps
//! `x-request-id` on every API response (D12). Every handler decodes bodies
//! through decode_body and runs Herdr calls through run_herdr (D01).
//! [POS]: The network entry of shardlane-remote; zero coupling with the GPUI
//! executor — the main thread interacts only via spawn/stop handles; binds
//! 127.0.0.1 and never 0.0.0.0

use crate::auth::require_bearer;
use crate::bootstrap::{build_bootstrap, connect_herdr, map_bootstrap_error, BootstrapError};
use crate::config::{ListenerMode, RemoteConfig, MIN_MOBILE_API_VERSION, REMOTE_API_VERSION};
use crate::error::ApiError;
use crate::events::{is_ping_frame, WsFrame, WS_IDLE_TIMEOUT};
use crate::state::RemoteState;
use axum::extract::rejection::JsonRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::middleware::from_fn_with_state;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use shardlane_host::herdr::{HerdrClient, HerdrError};
use shardlane_host::{AgentRuntime, HostCapabilities, HostInfo, HOST_API_VERSION};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Debug)]
pub enum RemoteServerError {
    /// Config did not enable loopback (callers should check
    /// is_loopback_ready first).
    NotEnabled,
    /// Enabled but missing token/identity (incomplete config, fail-closed).
    MissingCredentials,
    /// Bind failed (port occupied etc.).
    Bind(String),
    /// The server thread failed to become ready (panic or startup timeout).
    Startup(String),
}

impl std::fmt::Display for RemoteServerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotEnabled => formatter.write_str("remote server is not enabled for loopback"),
            Self::MissingCredentials => {
                formatter.write_str("remote server requires an access token")
            }
            Self::Bind(message) => write!(formatter, "remote server bind failed: {message}"),
            Self::Startup(message) => write!(formatter, "remote server failed to start: {message}"),
        }
    }
}

impl std::error::Error for RemoteServerError {}

/// Internal (non-wire) message sent over the startup ready channel: the
/// failure class is decided where it happens (bind vs generic startup), so
/// the receiving side never string-sniffs.
enum StartupOutcome {
    Ready(std::net::SocketAddr),
    Bind(String),
    Startup(String),
}

/// Spawn input. settings_path is the absolute path of
/// `~/.shardlane/config.json` (the read-only source of the bootstrap
/// projection); herdr_socket_override lets tests inject an isolated
/// instance.
pub struct RemoteServerOptions {
    /// Host shared live-session manager (GUI-injected; None = transient
    /// reads).
    pub conversation_sessions: Option<std::sync::Arc<shardlane_host::ConversationSessionManager>>,
    /// Process-level per-instance Herdr TUI manager registry (GUI-injected;
    /// None = RemoteState owns one, loopback tests only). Desktop GPUI and
    /// Remote viewers attach the same per-instance child.
    pub shared_tui: Option<std::sync::Arc<shardlane_host::shared_tui::TuiManagerRegistry>>,
    /// Process-level delivery coordinator (AC-05 + R2-03: GUI-injected;
    /// None = RemoteState owns one, loopback tests only). Once injected,
    /// Desktop and Remote share the same queue and the same Host delivery
    /// worker.
    pub delivery_coordinator:
        Option<std::sync::Arc<shardlane_host::ConversationDeliveryCoordinator>>,
    pub config: RemoteConfig,
    pub host_name: String,
    pub host_version: String,
    pub settings_path: PathBuf,
    pub herdr_socket_override: Option<PathBuf>,
    /// W4: path to the exported Mobile Web bundle (`dist/`). When present, Axum serves
    /// these static files at root, making the Remote API server a same-origin host for
    /// the Mobile PWA — zero CORS, zero separate serve-web proxy.
    pub web_bundle_path: Option<PathBuf>,
}

/// Running handle: addr for tests/logs; stop() shuts down gracefully and
/// joins the thread; web_connections() exposes pairing-list snapshots
/// (consumed by the GUI).
#[derive(Debug)]
pub struct RemoteServerHandle {
    pub addr: std::net::SocketAddr,
    state: Arc<RemoteState>,
    stop_tx: watch::Sender<bool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl RemoteServerHandle {
    /// Pairing-list snapshot (currently online client connections).
    pub fn web_connections(&self) -> Vec<crate::state::WebConnection> {
        self.state.web_connections_snapshot()
    }

    /// Mark a connection as kicked (the WS loop closes it after detection).
    pub fn kick_web_connection(&self, id: u64) {
        self.state.kick_web_connection(id);
    }

    /// Request graceful shutdown and wait for the thread to exit
    /// (idempotent). Injected process-level owners (held by the GUI) are not
    /// destroyed with Remote shutdown.
    pub fn stop(mut self) {
        let _ = self.stop_tx.send(true);
        if self.state.tui_owned {
            self.state.tui_registry.stop_all_managers();
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for RemoteServerHandle {
    fn drop(&mut self) {
        // Ensure the stop signal is sent even without an explicit stop
        // (leak protection; does not wait for join).
        let _ = self.stop_tx.send(true);
        if self.state.tui_owned {
            self.state.tui_registry.stop_all_managers();
        }
    }
}

pub fn spawn_remote_server(
    options: RemoteServerOptions,
) -> Result<RemoteServerHandle, RemoteServerError> {
    if !options.config.enabled
        || !matches!(
            options.config.listener_mode,
            ListenerMode::Loopback | ListenerMode::LocalNetwork
        )
    {
        return Err(RemoteServerError::NotEnabled);
    }
    if !options.config.is_loopback_ready() {
        return Err(RemoteServerError::MissingCredentials);
    }

    let port = options.config.port;
    let bind_addr = options.config.bind_address();
    let mut state = RemoteState::new(
        options.config,
        options.host_name,
        options.host_version,
        options.settings_path,
        options.herdr_socket_override,
    );
    state.conversation_sessions = options.conversation_sessions;
    if let (Some(tui), Some(delivery)) = (options.shared_tui, options.delivery_coordinator) {
        state.adopt_shared_owners(tui, delivery);
    }
    let state = Arc::new(state);
    // B5: event hub (1 subscription socket → broadcast → N WS clients),
    // exiting with the stop signal.
    let (stop_tx, stop_rx) = watch::channel(false);
    // A5: remember the shutdown signal for lazily-spawned per-instance hubs,
    // then start the DEFAULT hub eagerly (existing behavior).
    let _ = state.event_stop.set(stop_rx.clone());
    let event_sender = crate::events::spawn_event_hub(state.clone(), stop_rx.clone());
    state.set_event_sender(event_sender);
    let app = build_router(state.clone(), options.web_bundle_path);
    // D06: the startup outcome crosses the thread boundary as a typed enum
    // so a bind failure is classified at the source — never re-derived by
    // sniffing the message text for "bind failed".
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<StartupOutcome>(1);

    let thread_state = state.clone();
    let thread = std::thread::Builder::new()
        .name("shardlane-remote".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready_tx.send(StartupOutcome::Startup(format!(
                        "runtime build failed: {error}"
                    )));
                    return;
                }
            };
            let mut stop_rx = stop_rx;
            runtime.block_on(async move {
                let tui_manager = thread_state.tui_registry.clone();
                let mut tui_stop = stop_rx.clone();
                let tui_reaper = tokio::spawn(async move {
                    let mut interval = tokio::time::interval(Duration::from_secs(30));
                    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    loop {
                        tokio::select! {
                            _ = interval.tick() => tui_manager.reap_idle_all(),
                            changed = tui_stop.changed() => {
                                if changed.is_err() || *tui_stop.borrow() {
                                    break;
                                }
                            }
                        }
                    }
                });
                let listener = match tokio::net::TcpListener::bind((bind_addr, port)).await {
                    Ok(listener) => listener,
                    Err(error) => {
                        tui_reaper.abort();
                        let _ =
                            ready_tx.send(StartupOutcome::Bind(format!("bind failed: {error}")));
                        return;
                    }
                };
                let addr = listener
                    .local_addr()
                    .unwrap_or_else(|_| std::net::SocketAddr::new(bind_addr, port));
                if ready_tx.send(StartupOutcome::Ready(addr)).is_err() {
                    tui_reaper.abort();
                    return; // the caller gave up (dropped)
                }
                let serve = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
                )
                .with_graceful_shutdown(async move {
                    let _ = stop_rx.changed().await;
                });
                if let Err(error) = serve.await {
                    shardlane_host::diagnostics::lag_log(format_args!(
                        "remote.server serve exited: {error}"
                    ));
                }
                tui_reaper.abort();
            });
        })
        .map_err(|error| RemoteServerError::Startup(format!("thread spawn failed: {error}")))?;

    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(StartupOutcome::Ready(addr)) => Ok(RemoteServerHandle {
            addr,
            state,
            stop_tx,
            join: Some(thread),
        }),
        Ok(StartupOutcome::Bind(message)) => {
            let _ = thread.join();
            Err(RemoteServerError::Bind(message))
        }
        Ok(StartupOutcome::Startup(message)) => {
            let _ = thread.join();
            Err(RemoteServerError::Startup(message))
        }
        Err(_) => {
            let _ = stop_tx.send(true);
            let _ = thread.join();
            Err(RemoteServerError::Startup(
                "server did not become ready in time".into(),
            ))
        }
    }
}

/// v1 API clearance (2026-09-01): the former `/api/v1` compatibility routes
/// are gone. Mobile-consumed endpoints were re-homed under `/api/v2` with
/// identical semantics; the rest (v1 hello/bootstrap, projects/{id}/agents,
/// agents/{ref}/output, agents/{ref}/prompt, v1 history sessions) were
/// deleted outright. Route and endpoint assembly (the auth middleware covers
/// API endpoints; the static web bundle requires no auth — the token travels
/// in the URL fragment `#pair=<token>`, which browsers never send to the
/// server).
fn build_router(state: Arc<RemoteState>, web_bundle_path: Option<PathBuf>) -> Router {
    let api = Router::new()
        .route("/api/v2/hello", get(hello))
        .route("/api/v2/bootstrap", get(bootstrap))
        // Agent keys (re-homed from /api/v1/agents/{agent_ref}/keys)
        .route("/api/v2/agents/{agent_ref}/keys", post(agent_keys))
        // Workspace management (re-homed from /api/v1/workspaces…)
        .route("/api/v2/workspaces", post(create_workspace))
        .route("/api/v2/workspaces/{workspace_id}", delete(close_workspace))
        .route(
            "/api/v2/workspaces/{workspace_id}/rename",
            post(rename_workspace),
        )
        .route(
            "/api/v2/workspaces/{workspace_id}/move",
            post(move_workspace),
        )
        // Tab management (re-homed from /api/v1/tabs…)
        .route("/api/v2/tabs", post(create_tab))
        .route("/api/v2/tabs/{tab_id}", delete(close_tab))
        .route("/api/v2/tabs/{tab_id}/rename", post(rename_tab))
        .route("/api/v2/tabs/{tab_id}/move", post(move_tab))
        // Pane operations (re-homed from /api/v1/panes/{pane_id}…)
        .route("/api/v2/panes/{pane_id}/split", post(split_pane))
        .route("/api/v2/panes/{pane_id}", delete(close_pane))
        .route("/api/v2/panes/{pane_id}/zoom", post(toggle_pane_zoom))
        .route("/api/v2/panes/{pane_id}/resize", post(resize_pane))
        .route("/api/v2/panes/{pane_id}/swap", post(swap_pane))
        .route("/api/v2/panes/{pane_id}/move", post(move_pane))
        .route("/api/v2/panes/{pane_id}/rename", post(rename_pane))
        // Pane I/O (any pane, not just agents; re-homed from /api/v1)
        .route("/api/v2/panes/{pane_id}/output", get(pane_output))
        .route("/api/v2/panes/{pane_id}/keys", post(pane_keys))
        .route("/api/v2/panes/{pane_id}/text", post(pane_text))
        .route("/api/v2/panes/{pane_id}/process", get(pane_process))
        // Agent start (re-homed from /api/v1/agents/start)
        .route("/api/v2/agents/start", post(start_agent))
        // Agent authority (re-homed from /api/v1/panes/{pane_id}/agent)
        .route(
            "/api/v2/panes/{pane_id}/agent",
            post(report_pane_agent).delete(clear_pane_agent),
        )
        // Events WebSocket (re-homed from /api/v1/events; identical frame
        // protocol and subprotocol auth).
        .route("/api/v2/events", get(events_ws))
        // v2 semantic Conversation API.
        .route(
            "/api/v2/projects/{project_id}/conversations",
            get(crate::conversations::project_conversations),
        )
        .route(
            "/api/v2/agents/{agent_ref}/conversation",
            get(crate::conversations::agent_conversation),
        )
        .route(
            "/api/v2/conversations/{conversation_id}",
            get(crate::conversations::get_conversation),
        )
        .route(
            "/api/v2/conversations/{conversation_id}/window",
            get(crate::conversations::conversation_window),
        )
        .route(
            "/api/v2/conversations/{conversation_id}/prompt",
            post(crate::conversations::prompt_conversation),
        )
        .route(
            "/api/v2/conversations/{conversation_id}/continue",
            post(crate::conversations::continue_conversation),
        )
        .route(
            "/api/v2/conversations/{conversation_id}/queue",
            get(crate::conversations::conversation_queue_state),
        )
        .route(
            "/api/v2/conversations/{conversation_id}/interactions/resolve",
            post(crate::conversations::resolve_conversation_interaction),
        )
        .route(
            "/api/v2/conversations/{conversation_id}/interactions/{interaction_id}/delegate",
            post(crate::conversations::delegate_conversation_interaction),
        )
        .route(
            "/api/v2/history/search",
            get(crate::conversations::search_conversations),
        )
        // One Host-owned shared Herdr TUI session. The route is explicit and
        // never maps a Project/Tab/Pane into a private Terminal process.
        .route("/api/v2/tui/session", post(crate::tui::open_session))
        .route(
            "/api/v2/tui/session/{id}",
            get(crate::tui::get_session).delete(crate::tui::close_session),
        )
        .route(
            "/api/v2/tui/session/{id}/resize",
            post(crate::tui::resize_session),
        )
        .route(
            "/api/v2/tui/session/{id}/input",
            post(crate::tui::input_session),
        )
        .route(
            "/api/v2/tui/session/{id}/stream",
            get(crate::tui::stream_session),
        )
        .route("/api/v2/instances", get(crate::instances::list_instances))
        .route(
            "/api/v2/instances/{id}/bootstrap",
            get(crate::instances::instance_bootstrap),
        )
        .route(
            "/api/v2/instances/{id}/rename",
            post(crate::instances::rename_instance),
        )
        // D12: innermost layer so every API response is stamped (ApiError
        // envelopes set the header themselves from their own request id; this
        // layer fills the rest). Additive contract — see request_id_header.
        .layer(from_fn_with_state(state.clone(), request_id_header))
        .layer(from_fn_with_state(state.clone(), require_bearer))
        .layer(axum::middleware::from_fn(
            crate::bootstrap::instance_scope_middleware,
        ))
        .layer(axum::middleware::from_fn(crate::cors::cors_guard));

    if let Some(bundle_path) = web_bundle_path {
        if bundle_path.is_dir() {
            use tower_http::services::ServeDir;
            let serve = ServeDir::new(&bundle_path).append_index_html_on_directories(true);
            let bp = bundle_path.clone();
            Router::new()
                .merge(api.with_state(state))
                .nest_service("/_expo", ServeDir::new(bundle_path.join("_expo")))
                .nest_service("/assets", ServeDir::new(bundle_path.join("assets")))
                .fallback_service(serve.fallback(axum::routing::get(
                    move |uri: axum::http::Uri| {
                        let bp2 = bp.clone();
                        async move { serve_spa_page(bp2, uri).await }
                    },
                )))
        } else {
            api.fallback(not_found).with_state(state)
        }
    } else {
        api.fallback(not_found).with_state(state)
    }
}

/// Expo Router SPA fallback: static-exported pages map by first path segment.
/// The retired `/pane/*` route is intentionally not a normal product entry.
async fn serve_spa_page(
    bundle_path: PathBuf,
    uri: axum::http::Uri,
) -> axum::response::Response<axum::body::Body> {
    use axum::response::IntoResponse;
    let path = uri.path().trim_start_matches('/');
    let first = path.split(['/', '?']).next().unwrap_or("");
    let candidates: &[&str] = match first {
        "agent" => &["agent/[agentId].html"],
        "history" => &["history/[sessionKey].html", "history.html", "index.html"],
        "new-agent" => &["new-agent.html", "index.html"],
        // Shared Herdr TUI viewer, remote control tree, and pairing page.
        "tui" => &["tui.html", "index.html"],
        "control" => &["control.html", "index.html"],
        "connect" => &["connect.html", "index.html"],
        _ => &["index.html"],
    };
    for candidate in candidates {
        let file_path = bundle_path.join(candidate);
        if let Some(contents) = cached_spa_page(file_path).await {
            return (
                [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                contents,
            )
                .into_response();
        }
    }
    (axum::http::StatusCode::NOT_FOUND, "Page not found").into_response()
}

/// D27: SPA fallback page cache, keyed by absolute candidate file path.
/// Cache lifetime: populated on first request and never invalidated for the
/// process lifetime — the web bundle is a static export, so on-disk changes
/// are only picked up after an app restart. Bounded by construction: at most
/// the fixed candidate set (~7 files, positive and negative entries) per
/// bundle path. A read failure is cached too (negative entry) so a missing
/// candidate is not re-read from disk on every navigation.
static SPA_PAGE_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<PathBuf, Option<Vec<u8>>>>,
> = std::sync::OnceLock::new();

async fn cached_spa_page(file_path: PathBuf) -> Option<Vec<u8>> {
    let cache =
        SPA_PAGE_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    {
        let guard = cache.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(cached) = guard.get(&file_path) {
            return cached.clone();
        }
    }
    let contents = tokio::fs::read(&file_path).await.ok();
    let mut guard = cache.lock().unwrap_or_else(|poison| poison.into_inner());
    // or_insert: a concurrent first request may have won the race; the
    // contents for the same static path are identical either way.
    guard.entry(file_path).or_insert(contents).clone()
}

#[derive(Clone, Debug, Serialize)]
pub struct HelloResponse {
    product: &'static str,
    host: HostInfo,
    remote_api_version: u32,
    min_mobile_api_version: u32,
    capabilities: HostCapabilities,
}

pub fn hello_response(state: &RemoteState) -> HelloResponse {
    HelloResponse {
        product: "shardlane",
        host: HostInfo {
            host_id: state.host_id().to_string(),
            name: state.host_name.clone(),
            version: state.host_version.clone(),
            api_version: HOST_API_VERSION,
        },
        remote_api_version: REMOTE_API_VERSION,
        min_mobile_api_version: MIN_MOBILE_API_VERSION,
        // Single capability authority (crate::bootstrap::host_capabilities):
        // the bootstrap projection declares the identical set. The B4 prompt
        // endpoint has landed, so agent_control=true.
        capabilities: crate::bootstrap::host_capabilities_for(state),
    }
}

async fn hello(State(state): State<Arc<RemoteState>>) -> Json<HelloResponse> {
    Json(hello_response(&state))
}

async fn not_found(State(state): State<Arc<RemoteState>>) -> Response {
    ApiError::not_found("unknown endpoint", state.next_request_id()).into_response()
}

/// spawn_blocking wrapper: synchronous HerdrClient calls do not block the
/// tokio worker and do not slow the GPUI thread (the remote thread pool is
/// isolated from the GUI).
/// Instance-scoped variant used by the `/instances/{id}/bootstrap` route.
pub(crate) async fn blocking_projection_scoped(
    state: &Arc<RemoteState>,
    instance: Option<String>,
    work: impl FnOnce(
            &Arc<RemoteState>,
            Option<String>,
        ) -> Result<shardlane_host::dto::HostBootstrap, BootstrapError>
        + Send
        + 'static,
) -> Result<shardlane_host::dto::HostBootstrap, ApiError> {
    let state = state.clone();
    let request_id = state.next_request_id();
    match tokio::task::spawn_blocking(move || work(&state, instance)).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(map_bootstrap_error(error, request_id)),
        Err(join_error) => Err(ApiError::internal(
            format!("projection script failed: {join_error}"),
            request_id,
        )),
    }
}

async fn blocking_projection<T, F>(state: &Arc<RemoteState>, work: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&Arc<RemoteState>) -> Result<T, BootstrapError> + Send + 'static,
{
    let state = state.clone();
    let request_id = state.next_request_id();
    match tokio::task::spawn_blocking(move || work(&state)).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(map_bootstrap_error(error, request_id)),
        Err(join_error) => Err(ApiError::internal(
            format!("projection script failed: {join_error}"),
            request_id,
        )),
    }
}

async fn bootstrap(State(state): State<Arc<RemoteState>>) -> Response {
    // Instance scope read on the async side; build_bootstrap_for threads it
    // into the blocking projection (task locals do not cross spawn_blocking).
    let instance = crate::bootstrap::current_instance_scope();
    match blocking_projection(&state, move |state| {
        crate::bootstrap::build_bootstrap_for(state, instance.as_deref())
    })
    .await
    {
        Ok(bootstrap) => Json(bootstrap).into_response(),
        Err(error) => error.into_response(),
    }
}

// ==================== D01: shared handler plumbing ====================

/// D12: stamp `x-request-id` on every API response. Error envelopes
/// (ApiError) set the header from their own request id before this layer
/// runs, keeping header and body correlated; every other response receives a
/// fresh transport id here. Additive contract: mobile/remote clients may use
/// it for log correlation but must not require it.
async fn request_id_header(
    State(state): State<Arc<RemoteState>>,
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Response {
    let mut response = next.run(request).await;
    if !response.headers().contains_key("x-request-id") {
        let request_id = state.next_request_id();
        if let Ok(value) = axum::http::HeaderValue::from_str(&request_id) {
            response.headers_mut().insert("x-request-id", value);
        }
    }
    response
}

/// D01: the single decode point for request bodies. A malformed body gets
/// the unified ApiError envelope (400 invalid_request) instead of axum's
/// bare text 422 rejection, correlated with the handler's request id.
pub(crate) fn decode_body<T>(
    body: Result<Json<T>, JsonRejection>,
    request_id: &str,
) -> Result<T, ApiError>
where
    T: serde::de::DeserializeOwned,
{
    match body {
        Ok(Json(value)) => Ok(value),
        Err(rejection) => Err(ApiError::invalid_request(
            format!("malformed request body: {rejection}"),
            request_id.to_string(),
        )),
    }
}

/// D01: the single execution path for one synchronous Herdr operation —
/// connect_herdr + spawn_blocking + the unified error mapping — so handlers
/// keep only their success projection. `op` names the operation in the
/// join-failure envelope (spawn task panicked/cancelled; never a Herdr
/// outcome).
pub(crate) async fn run_mux<T, F>(
    state: &Arc<RemoteState>,
    op: &'static str,
    request_id: String,
    work: F,
) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(
            &dyn shardlane_host::mux::MultiplexerConnection,
        ) -> Result<T, shardlane_host::mux::MuxError>
        + Send
        + 'static,
{
    let work_state = state.clone();
    // Instance scope (multi-instance): `?instance=<registry id>` selects the
    // instance for this request. Read BEFORE spawn_blocking — task-locals do
    // not cross that boundary.
    let instance = crate::bootstrap::current_instance_scope();
    match tokio::task::spawn_blocking(move || {
        let connection = crate::bootstrap::connect_instance_for(&work_state, instance.as_deref())?;
        work(connection.as_ref())
    })
    .await
    {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(map_mux_error(error, request_id)),
        Err(join_error) => Err(ApiError::internal(
            format!("{op} failed: {join_error}"),
            request_id,
        )),
    }
}

/// D01: uniform success/error response projection for handlers whose work
/// went through run_herdr.
pub(crate) fn json_or_error<T: Serialize>(result: Result<T, ApiError>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => error.into_response(),
    }
}

const DEFAULT_OUTPUT_LINES: u32 = 200;
const MAX_OUTPUT_LINES: u32 = 1000;

// ==================== Unified Herdr error mapping ====================

/// Unified mapping of Herdr errors → stable envelope.
/// AgentNotFound (structured code) → 404; timeout semantics → 504; others →
/// 503 runtime unavailable.
/// Neutral-seam counterpart of [`map_herdr_error`]: identical wire mapping
/// over `MuxError`. Arms must not drift from the Herdr version.
pub(crate) fn map_mux_error(error: shardlane_host::mux::MuxError, request_id: String) -> ApiError {
    match &error {
        shardlane_host::mux::MuxError::NotFound(message) => {
            ApiError::not_found(message.clone(), request_id)
        }
        shardlane_host::mux::MuxError::Timeout(message) => {
            ApiError::timeout(message.clone(), request_id)
        }
        shardlane_host::mux::MuxError::SocketUnavailable(path, reason) => {
            ApiError::host_unavailable(
                format!("herdr socket unavailable at {path}: {reason}"),
                request_id,
            )
        }
        // R2-05: the mutation may already be accepted — a stable wire state
        // that idempotency tombstones; never collapse it into a generic
        // transient failure.
        shardlane_host::mux::MuxError::Uncertain(message) => {
            ApiError::delivery_uncertain(message.clone(), request_id)
        }
        shardlane_host::mux::MuxError::Api(message)
            if message.to_lowercase().contains("timeout")
                || message.to_lowercase().contains("timed out") =>
        {
            ApiError::timeout(message.clone(), request_id)
        }
        _ => ApiError::runtime_unavailable(format!("{error}"), request_id),
    }
}

pub(crate) fn map_herdr_error(error: HerdrError, request_id: String) -> ApiError {
    match &error {
        HerdrError::AgentNotFound(message) => ApiError::not_found(message.clone(), request_id),
        HerdrError::Api(message)
            if message.to_lowercase().contains("timeout")
                || message.to_lowercase().contains("timed out") =>
        {
            ApiError::timeout(message.clone(), request_id)
        }
        HerdrError::SocketUnavailable(path, reason) => ApiError::host_unavailable(
            format!("herdr socket unavailable at {path}: {reason}"),
            request_id,
        ),
        // R2-05: the mutation may already be accepted — a stable wire state
        // that idempotency tombstones; never collapse it into a generic
        // transient failure.
        HerdrError::DeliveryUncertain(message) => {
            ApiError::delivery_uncertain(message.clone(), request_id)
        }
        _ => ApiError::runtime_unavailable(format!("{error}"), request_id),
    }
}

// ==================== B4.5: terminal keys write path ====================

/// RuntimeAgent → AgentSummary: prefer associations from a fresh bootstrap
/// (project/tab projection), falling back to a direct runtime conversion
/// (project_id encoded from the runtime workspace) when projection fails.
fn agent_summary_from_runtime(
    runtime: &shardlane_host::RuntimeAgent,
    bootstrap: Option<&shardlane_host::HostBootstrap>,
) -> shardlane_host::AgentSummary {
    let projected = bootstrap
        .and_then(|bootstrap| bootstrap.agents.iter().find(|agent| agent.id == runtime.id));
    let project_id = projected
        .map(|agent| agent.project_id.clone())
        .unwrap_or_else(|| {
            shardlane_host::project_id_for_runtime_workspace(&runtime.runtime_workspace_id)
        });
    shardlane_host::AgentSummary {
        id: runtime.id.clone(),
        project_id,
        tab_id: shardlane_host::TabId::new(runtime.tab_id.clone()),
        pane_id: shardlane_host::PaneId::new(runtime.pane_id.clone()),
        name: runtime.name.clone(),
        kind: runtime.kind.clone(),
        title: runtime.title.clone(),
        status: runtime.status,
        // RuntimeAgent predates typed AgentSessionInfo. Never manufacture a
        // Conversation id from the pane target; only the Host projection that
        // proved a typed session identity may expose one.
        conversation_id: projected.and_then(|agent| agent.conversation_id.clone()),
        revision: runtime.revision,
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct KeysBody {
    pub keys: Vec<String>,
}

async fn agent_keys(
    State(state): State<Arc<RemoteState>>,
    Path(agent_ref): Path<String>,
    body: Result<Json<KeysBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if agent_ref.trim().is_empty() {
        return ApiError::invalid_request("agent_ref must not be empty", request_id)
            .into_response();
    }
    if body.keys.is_empty() {
        return Json(serde_json::json!({ "ok": true })).into_response();
    }

    let keys = body.keys;
    json_or_error(
        run_mux(&state, "send_keys", request_id, move |client| {
            client
                .agent_runtime()
                .ok_or(shardlane_host::mux::MuxError::Unsupported("agents"))?
                .send_runtime_agent_keys(&shardlane_host::AgentRef::new(agent_ref), &keys)
        })
        .await
        .map(|()| serde_json::json!({ "ok": true })),
    )
}

// ==================== Workspace management ====================

#[derive(Clone, Debug, Deserialize)]
pub struct CreateWorkspaceBody {
    pub cwd: Option<String>,
}

async fn create_workspace(
    State(state): State<Arc<RemoteState>>,
    body: Result<Json<CreateWorkspaceBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    let cwd = body.cwd;
    json_or_error(
        run_mux(&state, "create_workspace", request_id, move |client| {
            client
                .create_workspace(&shardlane_host::mux::CreateWorkspace {
                    cwd: cwd.as_deref(),
                    focus: true,
                })
                .map(|created| {
                    serde_json::json!({
                        "workspace_id": created.workspace.workspace_id,
                        "tab_id": created.tab.tab_id,
                        "pane_id": created.root_pane.pane_id,
                    })
                })
        })
        .await,
    )
}

async fn close_workspace(
    State(state): State<Arc<RemoteState>>,
    Path(workspace_id): Path<String>,
) -> Response {
    let request_id = state.next_request_id();
    if workspace_id.trim().is_empty() {
        return ApiError::invalid_request("workspace_id must not be empty", request_id)
            .into_response();
    }
    json_or_error(
        run_mux(&state, "close_workspace", request_id, move |client| {
            client
                .close_workspace(&workspace_id)
                .map(|()| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

#[derive(Clone, Debug, Deserialize)]
pub struct RenameBody {
    pub label: String,
}

async fn rename_workspace(
    State(state): State<Arc<RemoteState>>,
    Path(workspace_id): Path<String>,
    body: Result<Json<RenameBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if workspace_id.trim().is_empty() {
        return ApiError::invalid_request("workspace_id must not be empty", request_id)
            .into_response();
    }
    json_or_error(
        run_mux(&state, "rename_workspace", request_id, move |client| {
            client
                .rename_workspace(&workspace_id, &body.label)
                .map(|()| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

#[derive(Clone, Debug, Deserialize)]
pub struct MoveBody {
    pub insert_index: usize,
}

async fn move_workspace(
    State(state): State<Arc<RemoteState>>,
    Path(workspace_id): Path<String>,
    body: Result<Json<MoveBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if workspace_id.trim().is_empty() {
        return ApiError::invalid_request("workspace_id must not be empty", request_id)
            .into_response();
    }
    json_or_error(
        run_mux(&state, "move_workspace", request_id, move |client| {
            client
                .move_workspace(&workspace_id, body.insert_index)
                .map(|()| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

// ==================== Tab management ====================

#[derive(Clone, Debug, Deserialize)]
pub struct CreateTabBody {
    pub workspace_id: Option<String>,
    pub cwd: Option<String>,
    /// Logical New Agent operation id. Legacy callers may omit it; the
    /// transport envelope id then scopes this one request without claiming
    /// response-loss retries are deduplicable.
    #[serde(default)]
    pub request_id: Option<String>,
}

async fn create_tab(
    State(state): State<Arc<RemoteState>>,
    body: Result<Json<CreateTabBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    let operation_id = body
        .request_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| request_id.clone());
    let idempotency_key = format!("tab-create:{operation_id}");
    let idempotency_fingerprint = format!(
        "tab|workspace={}|cwd={}",
        body.workspace_id.as_deref().unwrap_or_default(),
        body.cwd.as_deref().unwrap_or_default()
    );
    let result = state
        .mutations
        .execute(idempotency_key, idempotency_fingerprint, || {
            let state = state.clone();
            let body = body.clone();
            let error_request_id = request_id.clone();
            async move {
                run_mux(&state, "create_tab", error_request_id, move |client| {
                    client
                        .create_tab(&shardlane_host::mux::CreateTab {
                            workspace_id: body.workspace_id.as_deref(),
                            cwd: body.cwd.as_deref(),
                            focus: true,
                        })
                        .map(|created| {
                            serde_json::json!({
                                "tab_id": created.tab.tab_id,
                                "pane_id": created.root_pane.pane_id,
                            })
                        })
                })
                .await
            }
        })
        .await;
    json_or_error(result)
}

async fn close_tab(State(state): State<Arc<RemoteState>>, Path(tab_id): Path<String>) -> Response {
    let request_id = state.next_request_id();
    if tab_id.trim().is_empty() {
        return ApiError::invalid_request("tab_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "close_tab", request_id, move |client| {
            client
                .close_tab(&tab_id)
                .map(|()| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

async fn rename_tab(
    State(state): State<Arc<RemoteState>>,
    Path(tab_id): Path<String>,
    body: Result<Json<RenameBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if tab_id.trim().is_empty() {
        return ApiError::invalid_request("tab_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "rename_tab", request_id, move |client| {
            client
                .rename_tab(&tab_id, &body.label)
                .map(|()| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

async fn move_tab(
    State(state): State<Arc<RemoteState>>,
    Path(tab_id): Path<String>,
    body: Result<Json<MoveBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if tab_id.trim().is_empty() {
        return ApiError::invalid_request("tab_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "move_tab", request_id, move |client| {
            client
                .move_tab(&tab_id, body.insert_index)
                .map(|()| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

// ==================== Pane operations ====================

#[derive(Clone, Debug, Deserialize)]
pub struct SplitPaneBody {
    pub direction: String,
}

async fn split_pane(
    State(state): State<Arc<RemoteState>>,
    Path(pane_id): Path<String>,
    body: Result<Json<SplitPaneBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "split_pane", request_id, move |client| {
            match body.direction.as_str() {
                "right" => client
                    .split_pane(&pane_id, shardlane_host::mux::SplitDirection::Right)
                    .map(|pane| serde_json::json!({ "pane_id": pane.pane_id })),
                "down" => client
                    .split_pane(&pane_id, shardlane_host::mux::SplitDirection::Down)
                    .map(|pane| serde_json::json!({ "pane_id": pane.pane_id })),
                other => Err(shardlane_host::mux::MuxError::Api(format!(
                    "direction must be \"right\" or \"down\", got \"{other}\""
                ))),
            }
        })
        .await,
    )
}

async fn close_pane(
    State(state): State<Arc<RemoteState>>,
    Path(pane_id): Path<String>,
) -> Response {
    let request_id = state.next_request_id();
    if pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "close_pane", request_id, move |client| {
            client
                .close_pane(&pane_id)
                .map(|()| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

async fn toggle_pane_zoom(
    State(state): State<Arc<RemoteState>>,
    Path(pane_id): Path<String>,
) -> Response {
    let request_id = state.next_request_id();
    if pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "toggle_pane_zoom", request_id, move |client| {
            client
                .toggle_pane_zoom(&pane_id)
                .map(|action| serde_json::json!({ "zoomed": action.layout.zoomed }))
        })
        .await,
    )
}

#[derive(Clone, Debug, Deserialize)]
pub struct ResizePaneBody {
    pub direction: String,
}

async fn resize_pane(
    State(state): State<Arc<RemoteState>>,
    Path(pane_id): Path<String>,
    body: Result<Json<ResizePaneBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "resize_pane", request_id, move |client| {
            let direction: shardlane_host::mux::MuxDirection = body
                .direction
                .parse()
                .map_err(shardlane_host::mux::MuxError::Api)?;
            client
                .resize_pane(&pane_id, direction)
                .map(|_| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

#[derive(Clone, Debug, Deserialize)]
pub struct SwapPaneBody {
    pub direction: String,
}

async fn swap_pane(
    State(state): State<Arc<RemoteState>>,
    Path(pane_id): Path<String>,
    body: Result<Json<SwapPaneBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "swap_pane", request_id, move |client| {
            let direction: shardlane_host::mux::MuxDirection = body
                .direction
                .parse()
                .map_err(shardlane_host::mux::MuxError::Api)?;
            client
                .swap_pane(&pane_id, direction)
                .map(|_| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

#[derive(Clone, Debug, Deserialize)]
pub struct MovePaneBody {
    pub destination: String,
    pub workspace_id: Option<String>,
    pub tab_id: Option<String>,
}

async fn move_pane(
    State(state): State<Arc<RemoteState>>,
    Path(pane_id): Path<String>,
    body: Result<Json<MovePaneBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "move_pane", request_id, move |client| {
            match body.destination.as_str() {
                "new_tab" => {
                    let workspace_id = body.workspace_id.as_deref().ok_or_else(|| {
                        shardlane_host::herdr::HerdrError::Api(
                            "workspace_id required for new_tab destination".into(),
                        )
                    })?;
                    client
                        .move_pane_to_new_tab(&pane_id, workspace_id)
                        .map(|result| serde_json::json!({ "pane_id": result.pane.pane_id }))
                }
                "tab" => {
                    let tab_id = body.tab_id.as_deref().ok_or_else(|| {
                        shardlane_host::mux::MuxError::Api(
                            "tab_id required for tab destination".into(),
                        )
                    })?;
                    client
                        .move_pane_to_tab(&pane_id, tab_id)
                        .map(|result| serde_json::json!({ "pane_id": result.pane.pane_id }))
                }
                other => Err(shardlane_host::mux::MuxError::Api(format!(
                    "destination must be \"new_tab\" or \"tab\", got \"{other}\""
                ))),
            }
        })
        .await,
    )
}

async fn rename_pane(
    State(state): State<Arc<RemoteState>>,
    Path(pane_id): Path<String>,
    body: Result<Json<RenameBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "rename_pane", request_id, move |client| {
            client
                .rename_pane(&pane_id, &body.label)
                .map(|()| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

// ==================== Pane I/O ====================

fn parse_output_lines(query: &HashMap<String, String>, request_id: &str) -> Result<u32, ApiError> {
    match query.get("lines") {
        None => Ok(DEFAULT_OUTPUT_LINES),
        Some(raw) => match raw.parse::<u32>() {
            Ok(value) if (1..=MAX_OUTPUT_LINES).contains(&value) => Ok(value),
            Ok(_) => Err(ApiError::invalid_request(
                format!("lines must be between 1 and {MAX_OUTPUT_LINES}"),
                request_id.to_string(),
            )),
            Err(_) => Err(ApiError::invalid_request(
                "lines must be an integer",
                request_id.to_string(),
            )),
        },
    }
}

async fn pane_output(
    State(state): State<Arc<RemoteState>>,
    Path(pane_id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let request_id = state.next_request_id();
    let lines = match parse_output_lines(&query, &request_id) {
        Ok(lines) => lines,
        Err(error) => return error.into_response(),
    };
    if pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "pane_output", request_id, move |client| {
            client
                .read_pane_history(&pane_id, lines)
                .map(|history| serde_json::json!({ "text": history.text }))
        })
        .await,
    )
}

async fn pane_keys(
    State(state): State<Arc<RemoteState>>,
    Path(pane_id): Path<String>,
    body: Result<Json<KeysBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    if body.keys.is_empty() {
        return Json(serde_json::json!({ "ok": true })).into_response();
    }
    json_or_error(
        run_mux(&state, "pane_keys", request_id, move |client| {
            client
                .send_keys(&pane_id, &body.keys)
                .map(|()| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

#[derive(Clone, Debug, Deserialize)]
pub struct PaneTextBody {
    pub text: String,
}

async fn pane_text(
    State(state): State<Arc<RemoteState>>,
    Path(pane_id): Path<String>,
    body: Result<Json<PaneTextBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "pane_text", request_id, move |client| {
            client
                .send_text(&pane_id, &body.text)
                .map(|()| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

async fn pane_process(
    State(state): State<Arc<RemoteState>>,
    Path(pane_id): Path<String>,
) -> Response {
    let request_id = state.next_request_id();
    if pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "pane_process", request_id, move |client| {
            client.pane_process_info(&pane_id).map(|info| {
                serde_json::json!({
                    "pane_id": info.pane_id,
                    "shell_pid": info.shell_pid,
                    "tty": info.tty,
                    "foreground_processes": info.foreground_processes.iter().map(|p| {
                        serde_json::json!({
                            "pid": p.pid,
                            "name": p.name,
                            "cwd": p.cwd,
                            "cmdline": p.cmdline,
                        })
                    }).collect::<Vec<_>>(),
                })
            })
        })
        .await,
    )
}

// ==================== Agent start ====================

#[derive(Clone, Debug, Deserialize)]
pub struct StartAgentBody {
    pub name: String,
    pub kind: String,
    pub pane_id: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub timeout_ms: Option<u64>,
    /// R4-P0-06: logical client mutation id — the start transaction runs
    /// through the Remote idempotency cache, so an ambiguous-response retry
    /// reuses the recorded outcome instead of creating a second Agent.
    #[serde(default)]
    pub request_id: Option<String>,
}

async fn start_agent(
    State(state): State<Arc<RemoteState>>,
    body: Result<Json<StartAgentBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if body.pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    if body.name.trim().is_empty() {
        return ApiError::invalid_request("name must not be empty", request_id).into_response();
    }
    if body.kind.trim().is_empty() {
        return ApiError::invalid_request("kind must not be empty", request_id).into_response();
    }
    let start_state = state.clone();
    // Missing request ids are legacy input, not permission to share one
    // global empty key.  The per-envelope id is unique and therefore avoids
    // replaying a previous New Agent or reporting a false fingerprint conflict.
    let operation_id = body
        .request_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| request_id.clone());
    let idempotency_key = format!("agent-start:{operation_id}");
    let idempotency_fingerprint = format!(
        "start|kind={}|pane={}|name={}|args={:?}|timeout_ms={:?}",
        body.kind, body.pane_id, body.name, body.args, body.timeout_ms
    );
    let result = state
        .mutations
        .execute(idempotency_key, idempotency_fingerprint, || {
            let start_state = start_state.clone();
            let body = body.clone();
            let operation_request_id = request_id.clone();
            async move {
                let operation_request_id_for_join = operation_request_id.clone();
                let executed = tokio::task::spawn_blocking(move || {
                    start_agent_blocking(start_state, body, operation_request_id_for_join)
                })
                .await
                .map_err(|error| {
                    ApiError::internal(format!("start join: {error}"), operation_request_id)
                })?;
                executed
            }
        })
        .await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => error.into_response(),
    }
}

/// R4-P0-06: the blocking start transaction, returning a JSON value so the
/// idempotency cache can replay it verbatim. Runs on a dedicated
/// spawn_blocking thread — no block_in_place wrapper is needed (and it would
/// be misleading on a blocking pool thread).
fn start_agent_blocking(
    start_state: Arc<RemoteState>,
    body: StartAgentBody,
    request_id: String,
) -> Result<serde_json::Value, ApiError> {
    let client: HerdrClient =
        connect_herdr(&start_state).map_err(|error| map_herdr_error(error, request_id.clone()))?;
    let pane_id = body.pane_id.clone();
    let runtime = client
        .start_runtime_agent(&shardlane_host::RuntimeAgentStartRequest {
            name: body.name,
            kind: body.kind,
            pane_id: shardlane_host::PaneId::new(pane_id.clone()),
            args: body.args,
            timeout_ms: body.timeout_ms,
        })
        .map_err(|error| map_herdr_error(error, request_id.clone()))?;
    let created_error = |phase: &str, detail: String| {
        ApiError::agent_created_with_target(
            detail,
            request_id.clone(),
            crate::error::AgentCreatedTarget {
                agent_ref: pane_id.clone(),
                tab_id: runtime.tab_id.clone(),
                pane_id: pane_id.clone(),
                phase: phase.to_string(),
            },
        )
    };
    // AC-17: v1 start is a single-shot mutation (no blind retries); after
    // success it must pass typed readiness/identity verification to count
    // as complete — the same verification semantics as the canonical
    // launch transaction, not a bare SPI passthrough.
    use shardlane_host::AgentLaunchRuntime as _;
    client.wait_agent_idle(&pane_id, 45_000).map_err(|error| {
        created_error(
            "not_ready",
            format!("agent created in pane {pane_id}, but readiness confirmation failed: {error}"),
        )
    })?;
    let occupant = client.agent_by_pane(&pane_id).map_err(|error| {
        created_error(
            "not_ready",
            format!("agent created in pane {pane_id}, but identity lookup failed: {error}"),
        )
    })?;
    let verified = occupant
        .filter(|agent| agent.pane_id.as_deref() == Some(pane_id.as_str()))
        .and_then(|agent| agent.agent_session)
        .is_some_and(|session| {
            // R2-11: normalize BOTH sides to typed AgentId. Herdr reports
            // "claude" while AgentId::ClaudeCode.as_str() is
            // "claude-code" — raw string comparison breaks Claude.
            let session_agent = shardlane_host::agent_id_from_herdr_kind(&session.agent);
            let requested_agent = runtime
                .kind
                .as_deref()
                .and_then(shardlane_host::agent_id_from_herdr_kind);
            session_agent.is_some_and(|session_agent| Some(session_agent) == requested_agent)
        });
    if !verified {
        return Err(created_error(
            "not_ready",
            format!("agent created in pane {pane_id}, but typed identity verification failed"),
        ));
    }
    // Project the v1 summary once; the idempotency cache replays this
    // value verbatim for an exact retry of the same logical request.
    let bootstrap = build_bootstrap(&start_state).ok();
    let summary = agent_summary_from_runtime(&runtime, bootstrap.as_ref());
    serde_json::to_value(summary).map_err(|error| {
        ApiError::internal(format!("serialize start summary: {error}"), request_id)
    })
}

// ==================== Agent authority ====================

#[derive(Clone, Debug, Deserialize)]
pub struct ReportAgentBody {
    pub agent: String,
}

async fn report_pane_agent(
    State(state): State<Arc<RemoteState>>,
    Path(pane_id): Path<String>,
    body: Result<Json<ReportAgentBody>, JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    if pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "report_pane_agent", request_id, move |client| {
            client
                .agent_runtime()
                .ok_or(shardlane_host::mux::MuxError::Unsupported("agents"))?
                .report_pane_agent(&pane_id, &body.agent)
                .map(|()| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

async fn clear_pane_agent(
    State(state): State<Arc<RemoteState>>,
    Path(pane_id): Path<String>,
) -> Response {
    let request_id = state.next_request_id();
    if pane_id.trim().is_empty() {
        return ApiError::invalid_request("pane_id must not be empty", request_id).into_response();
    }
    json_or_error(
        run_mux(&state, "clear_pane_agent", request_id, move |client| {
            client
                .agent_runtime()
                .ok_or(shardlane_host::mux::MuxError::Unsupported("agents"))?
                .clear_pane_agent_authority(&pane_id)
                .map(|()| serde_json::json!({ "ok": true }))
        })
        .await,
    )
}

// ==================== B5: events WebSocket ====================

/// Per-connection bounded send queue: overflow means resync+disconnect (slow
/// consumers recover on their own).
const WS_SEND_QUEUE_CAPACITY: usize = 128;
// The idle timeout and application-level ping sniffing are shared with the
// TUI stream socket: crate::events::{WS_IDLE_TIMEOUT, is_ping_frame}.

async fn events_ws(
    State(state): State<Arc<RemoteState>>,
    connect_info: axum::extract::ConnectInfo<std::net::SocketAddr>,
    ws: WebSocketUpgrade,
) -> Response {
    // A5: `?instance=<session>` selects the instance whose event hub this
    // socket streams; absent = the default instance.
    let instance = crate::bootstrap::current_instance_scope();
    // Echo the protocol selected by the browser subprotocol auth channel
    // (some browsers refuse the connection without the echo).
    ws.protocols([crate::cors::BEARER_SUBPROTOCOL])
        .on_upgrade(move |socket| async move {
            serve_event_socket(socket, state, connect_info.0, instance).await
        })
        .into_response()
}

async fn serve_event_socket(
    socket: WebSocket,
    state: Arc<RemoteState>,
    peer: std::net::SocketAddr,
    instance: Option<String>,
) {
    // Pairing-list ground truth: register connection → remove at end of
    // lifecycle (including any break path).
    let connection_id = state.register_web_connection(peer.to_string());
    let result = serve_event_socket_inner(socket, &state, connection_id, instance).await;
    state.drop_web_connection(connection_id);
    result
}

async fn serve_event_socket_inner(
    socket: WebSocket,
    state: &Arc<RemoteState>,
    connection_id: u64,
    instance: Option<String>,
) {
    use futures_util::{SinkExt, StreamExt};
    // A5: subscribe to the requested instance's hub. Non-default instances
    // spawn their hub lazily on first subscriber (shutdown via event_stop).
    let broadcast_rx = if instance.is_none() {
        state.subscribe_events()
    } else {
        let key = instance.clone().unwrap_or_else(|| "default".to_string());
        let stop = state.event_stop.get().cloned();
        let state_for_hub = state.clone();
        state.subscribe_instance_events(&key, move || {
            let state = state_for_hub.clone();
            let stop = stop?;
            Some(crate::events::spawn_event_hub_for(state, stop, instance))
        })
    };
    let Some(mut broadcast_rx) = broadcast_rx else {
        // Hub unavailable (thread spawn failure etc.): declare resync and
        // disconnect; the client reconnects.
        let mut socket = socket;
        let _ = socket.send(Message::text(WsFrame::Resync.to_wire())).await;
        let _ = socket.send(Message::Close(None)).await;
        return;
    };

    let (mut ws_tx, mut ws_rx) = socket.split();
    let (queue_tx, mut queue_rx) = tokio::sync::mpsc::channel::<String>(WS_SEND_QUEUE_CAPACITY);
    let writer = tokio::spawn(async move {
        while let Some(frame) = queue_rx.recv().await {
            if ws_tx.send(Message::text(frame)).await.is_err() {
                break;
            }
        }
        let _ = ws_tx.send(Message::Close(None)).await;
        let _ = ws_tx.close().await;
    });

    // The ready first frame (not queued competitively; it takes one queue
    // slot directly to preserve ordering).
    let _ = queue_tx
        .try_send(
            WsFrame::Ready {
                remote_api_version: REMOTE_API_VERSION,
            }
            .to_wire(),
        )
        .inspect_err(|error| {
            shardlane_host::diagnostics::lag_log(format_args!(
                "remote.ws ready enqueue failed: {error}"
            ));
        });

    let mut kick_check = tokio::time::interval(std::time::Duration::from_secs(1));
    kick_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = kick_check.tick() => {
                if state.take_kicked(connection_id) {
                    // D21: Resync is best-effort — when the send queue is full
                    // (exactly the slow-consumer scenario) the frame is
                    // dropped. The kick/disconnect itself is the authoritative
                    // signal: the client reconnects and re-pulls a full
                    // bootstrap.
                    let _ = queue_tx.try_send(WsFrame::Resync.to_wire());
                    break;
                }
            }
            frame = broadcast_rx.recv() => {
                match frame {
                    Ok(frame) => {
                        if queue_tx.try_send(frame).is_err() {
                            // Queue full / write end closed: the slow-consumer
                            // protocol. The Resync is best-effort (the queue
                            // is full precisely in this scenario); the
                            // disconnect is the authoritative signal — the
                            // client recovers by reconnecting and re-pulling
                            // a full bootstrap.
                            let _ = queue_tx.try_send(WsFrame::Resync.to_wire());
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        // One connection fell behind the broadcast capacity:
                        // same protocol as a slow consumer (best-effort
                        // Resync; the disconnect is authoritative).
                        shardlane_host::diagnostics::lag_log(format_args!(
                            "remote.ws subscriber lagged skipped={skipped}"
                        ));
                        let _ = queue_tx.try_send(WsFrame::Resync.to_wire());
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            inbound = tokio::time::timeout(WS_IDLE_TIMEOUT, ws_rx.next()) => {
                match inbound {
                    Ok(Some(Ok(message))) => {
                        state.touch_web_connection(connection_id);
                        match message {
                            Message::Text(text) => {
                                if is_ping_frame(&text)
                                    && queue_tx.try_send(WsFrame::Pong.to_wire()).is_err()
                                {
                                    break;
                                }
                            }
                            Message::Close(_) => break,
                            _ => {}
                        }
                    }
                    Ok(Some(Err(_))) | Ok(None) => break,
                    Err(_elapsed) => {
                        // Idle timeout: the client did not ping as agreed;
                        // reclaim the connection.
                        break;
                    }
                }
            }
        }
    }
    drop(queue_tx);
    let _ = writer.await;
}
