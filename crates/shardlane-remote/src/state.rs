//! Remote API service state shared across handlers (lock-free read paths +
//! atomic request sequence).
//!
//! [INPUT]: Depends on crate::config (RemoteConfig); from B3 on, depends on
//! shardlane-host::herdr's HerdrClient and config.json reading, plus
//! shardlane-history (the shared HistoryCatalog handle, D14)
//! [OUTPUT]: Exposes RemoteState (config/host identity/request sequence/
//! later Herdr handle slots, cached history catalog), next_request_id, and
//! the web-connection registry (register/touch/drop) that is the pairing
//! list's ground truth for BOTH the events socket and the TUI stream socket
//! (D23)
//! [POS]: The shared AppState of server.rs (Arc-distributed); carries only
//! the snapshot material the remote API needs and never writes back into
//! GUI-owned state

use crate::config::RemoteConfig;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// One established remote client connection (v1: a WS event-stream
/// connection; the ground truth for the pairing list).
#[derive(Clone, Debug)]
pub struct WebConnection {
    pub id: u64,
    /// Peer address (ip:port; under loopback deployment, usually the local
    /// reverse proxy/browser).
    pub addr: String,
    pub connected_at: Instant,
    pub last_seen: Instant,
}

pub struct RemoteState {
    /// Config snapshot already validated by is_loopback_ready (includes the
    /// token).
    pub config: RemoteConfig,
    /// Host shared live-session manager (injected by the GUI; loopback tests
    /// may leave it unset). When unset, live queries use bounded transient
    /// reads with identical semantics.
    pub conversation_sessions: Option<std::sync::Arc<shardlane_host::ConversationSessionManager>>,
    /// Host semantic follow-up queue (queueing and delivery of AlreadyLive +
    /// Working instructions). Used for view/cancel/edit projections;
    /// queueing and delivery are owned by the `delivery` coordinator.
    pub follow_up_queue: std::sync::Arc<shardlane_host::ConversationFollowUpQueue>,
    /// R2-03/CR-01: the Host-owned queueing delivery coordinator. Remote
    /// prompt/Continue items queued here are scheduled for delivery by it
    /// inside the Host process, independent of any Desktop window or Remote
    /// connection.
    pub delivery: std::sync::Arc<shardlane_host::ConversationDeliveryCoordinator>,
    /// Display host name (defaults to the machine name; returned by hello).
    pub host_name: String,
    /// Shardlane app version (returned by hello; clients use it to judge
    /// upgrades).
    pub host_version: String,
    /// settings config.json path (read by the B3 bootstrap projection;
    /// read-only + mtime checks).
    pub settings_path: PathBuf,
    /// Explicit Herdr socket override (test injection); None → herdr.rs
    /// default discovery (HERDR_SOCKET_PATH env var or the standard path
    /// under $HOME).
    pub herdr_socket_override: Option<PathBuf>,
    /// Backend-neutral Multiplexer registry (docs/multiplexer-api.md): the
    /// sole assembly point for instance connections. Remote handlers resolve
    /// instances through it; the Herdr backend is the builtin registration.
    pub mux_registry: std::sync::Arc<shardlane_host::mux::MuxRegistry>,
    /// Per-instance Herdr TUI managers (multi-instance model): at most one
    /// hosted TUI child per Herdr instance, shared by every viewer. Normally
    /// injected by the GUI so the desktop window and Remote/mobile viewers
    /// attach the same child; loopback tests may let RemoteState own one.
    pub tui_registry: Arc<shardlane_host::shared_tui::TuiManagerRegistry>,
    /// Whether this RemoteState owns `tui` (only then may server teardown
    /// stop the shared session; an injected manager belongs to the GUI).
    pub(crate) tui_owned: bool,
    /// A03: the request_id idempotency cache of semantic mutations
    /// (prompt/continue). An exact replay of the same key returns the first
    /// response; concurrent duplicates merge into one execution; bounded by
    /// TTL + capacity.
    pub mutations: crate::idempotency::MutationCache,
    /// D14: one shared HistoryCatalog handle for the whole process, opened
    /// lazily against [`RemoteState::history_db_path`] and reused by the
    /// history/conversation handlers instead of a fresh SQLite connection per
    /// request. Calls run on spawn_blocking threads, so the std Mutex is held
    /// only for the duration of synchronous queries — never across an await.
    /// WAL mode keeps this connection consistent with the Host processes'
    /// own catalog connections. Open failures are not cached; the next
    /// request retries.
    history_catalog: Mutex<Option<shardlane_history::HistoryCatalog>>,
    /// Event hub broadcast sender (B5; loaded by spawn_remote_server, left
    /// empty when the hub is unavailable).
    events: OnceLock<tokio::sync::broadcast::Sender<String>>,
    /// A5: per-instance event hubs for NON-default instances (key = session
    /// name), lazily spawned when a client streams that instance's events.
    pub(crate) event_hubs:
        std::sync::Mutex<std::collections::HashMap<String, tokio::sync::broadcast::Sender<String>>>,
    /// Shared shutdown signal for lazily-spawned hubs (set at server start).
    pub(crate) event_stop: std::sync::OnceLock<tokio::sync::watch::Receiver<bool>>,
    /// Remote client connection registry (WS lifecycle-driven; the data
    /// source of the pairing list).
    connections: Mutex<(Vec<WebConnection>, u64)>,
    /// Connection ids the GUI marked for disconnection (the WS loop checks
    /// and closes them proactively each round).
    kicked: Mutex<HashSet<u64>>,
    request_seq: AtomicU64,
}

impl RemoteState {
    pub fn new(
        config: RemoteConfig,
        host_name: String,
        host_version: String,
        settings_path: PathBuf,
        herdr_socket_override: Option<PathBuf>,
    ) -> Self {
        let delivery = shardlane_host::ConversationDeliveryCoordinator::new(
            std::sync::Arc::new(shardlane_host::ConversationFollowUpQueue::new()),
            herdr_socket_override.clone(),
        );
        Self {
            conversation_sessions: None,
            follow_up_queue: std::sync::Arc::clone(delivery.queue()),
            delivery,
            config,
            host_name,
            host_version,
            settings_path,
            herdr_socket_override,
            mux_registry: std::sync::Arc::new(shardlane_host::mux::MuxRegistry::with_builtins()),
            tui_registry: Arc::new(shardlane_host::shared_tui::TuiManagerRegistry::default()),
            tui_owned: true,
            mutations: crate::idempotency::MutationCache::new(),
            history_catalog: Mutex::new(None),
            events: OnceLock::new(),
            event_hubs: std::sync::Mutex::new(std::collections::HashMap::new()),
            event_stop: std::sync::OnceLock::new(),
            connections: Mutex::new((Vec::new(), 0_u64)),
            kicked: Mutex::new(HashSet::new()),
            request_seq: AtomicU64::new(1),
        }
    }

    /// The TUI manager owning `session`'s child (created when absent).
    pub(crate) fn tui_manager_for(
        &self,
        session: Option<&str>,
    ) -> Arc<shardlane_host::shared_tui::TuiManager> {
        self.tui_registry.get_or_create(session)
    }

    /// Adopt process-level shared state injected by the Host application
    /// (GUI): the shared Herdr TUI manager (A01) and the process-wide delivery
    /// coordinator (AC-05 + R2-03). Injected owners outlive this RemoteState
    /// and must not be torn down with the remote server.
    pub fn adopt_shared_owners(
        &mut self,
        tui_registry: Arc<shardlane_host::shared_tui::TuiManagerRegistry>,
        delivery: std::sync::Arc<shardlane_host::ConversationDeliveryCoordinator>,
    ) {
        self.tui_registry = tui_registry;
        self.tui_owned = false;
        self.follow_up_queue = std::sync::Arc::clone(delivery.queue());
        self.delivery = delivery;
    }

    /// In-process monotonic request correlation id (shared by error envelopes
    /// and diagnostics).
    pub fn next_request_id(&self) -> String {
        let seq = self.request_seq.fetch_add(1, Ordering::Relaxed);
        format!("req-{seq}")
    }

    /// Stable host_id for hello (config already ran ensure_identity; the
    /// fallback empty string only appears on test paths that connect without
    /// generating an identity).
    pub fn host_id(&self) -> &str {
        self.config.host_id.as_deref().unwrap_or("")
    }

    /// Single definition of the semantic-history SQLite path: it lives next
    /// to config.json (the settings file's directory). Used by the v2
    /// Conversation service construction and the shared catalog handle.
    pub fn history_db_path(&self) -> PathBuf {
        self.settings_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("history.sqlite3")
    }

    /// D14: the shared HistoryCatalog handle, opened on first use against
    /// [`RemoteState::history_db_path`] and cached for the process lifetime.
    /// The guard must be held only for synchronous queries (spawn_blocking
    /// context), never across an await. An open failure is returned (and not
    /// cached) so callers keep their per-call-site semantics: the additive
    /// history projection skips, the global search maps to `internal`.
    pub(crate) fn history_catalog(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<shardlane_history::HistoryCatalog>>, String> {
        let mut guard = self
            .history_catalog
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if guard.is_none() {
            *guard = Some(
                shardlane_history::HistoryCatalog::open(&self.history_db_path())
                    .map_err(|error| error.to_string())?,
            );
        }
        Ok(guard)
    }

    /// Event broadcast sender (loaded by B5 spawn_remote_server; used for WS
    /// subscriptions).
    pub fn set_event_sender(&self, sender: tokio::sync::broadcast::Sender<String>) {
        let _ = self.events.set(sender);
    }

    pub fn subscribe_events(&self) -> Option<tokio::sync::broadcast::Receiver<String>> {
        self.events.get().map(|sender| sender.subscribe())
    }

    /// Health probe of the event broadcast sender (whether the hub thread
    /// loaded it).
    pub fn events_available(&self) -> bool {
        self.events.get().is_some()
    }

    /// A5: lazily spawns (or reuses) the event hub for a NON-default instance
    /// and subscribes to it. `spawn` runs the CLI/subscription work — call it
    /// from the async side; the hub thread itself does the blocking.
    pub(crate) fn subscribe_instance_events(
        &self,
        key: &str,
        spawn: impl FnOnce() -> Option<tokio::sync::broadcast::Sender<String>>,
    ) -> Option<tokio::sync::broadcast::Receiver<String>> {
        let mut hubs = self
            .event_hubs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(sender) = hubs.get(key) {
            return Some(sender.subscribe());
        }
        let sender = spawn()?;
        hubs.insert(key.to_string(), sender.clone());
        Some(sender.subscribe())
    }

    /// Register a client connection (on WS establishment); returns the
    /// connection id for touch/drop.
    pub fn register_web_connection(&self, addr: String) -> u64 {
        let mut guard = self
            .connections
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let (list, next_id) = &mut *guard;
        *next_id += 1;
        let id = *next_id;
        let now = Instant::now();
        list.push(WebConnection {
            id,
            addr,
            connected_at: now,
            last_seen: now,
        });
        id
    }

    /// Refresh a connection's activity time (on receiving an inbound frame;
    /// the ground truth of ping keep-alive).
    pub fn touch_web_connection(&self, id: u64) {
        let mut guard = self
            .connections
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(record) = guard.0.iter_mut().find(|record| record.id == id) {
            record.last_seen = Instant::now();
        }
    }

    /// Remove a connection (on WS close).
    pub fn drop_web_connection(&self, id: u64) {
        let mut guard = self
            .connections
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        guard.0.retain(|record| record.id != id);
    }

    /// Connection snapshot (consumed by the pairing list UI).
    pub fn web_connections_snapshot(&self) -> Vec<WebConnection> {
        self.connections
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .0
            .clone()
    }

    /// The GUI marks a connection as "kicked" (the WS loop detects and closes
    /// it on its next round).
    pub fn kick_web_connection(&self, id: u64) {
        self.kicked
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(id);
        self.drop_web_connection(id);
    }

    /// Called by the WS loop: check and consume the kick mark.
    pub fn take_kicked(&self, id: u64) -> bool {
        self.kicked
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(&id)
    }
}

impl std::fmt::Debug for RemoteState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteState")
            .field("host_id", &self.config.host_id)
            .field("port", &self.config.port)
            .field("connections", &self.web_connections_snapshot().len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_are_sequential() {
        let state = RemoteState::new(
            RemoteConfig::default(),
            "mac".into(),
            "0.0.0".into(),
            PathBuf::from("/tmp/none.json"),
            None,
        );
        assert_eq!(state.next_request_id(), "req-1");
        assert_eq!(state.next_request_id(), "req-2");
    }
}
