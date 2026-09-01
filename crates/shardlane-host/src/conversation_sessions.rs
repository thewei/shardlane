//! Host-owned bounded Live Conversation session manager.
//!
//! [INPUT]: an exact `SessionFileRef` (produced by the GUI's
//! ensure_chat_source / the Host's live resolution), or a non-destructive
//! shared read request for an already resolved source.
//! [OUTPUT]: exactly one incremental `LiveSession` state per observed live
//! semantic source within the process: the GUI Chat worker holds the lease
//! and drives sync; Remote/other readers read the same state via
//! `shared_snapshot` (non-destructive). With no subscribers there is no
//! longer a per-request full hydrate: the source is opened once outside the
//! map lock, briefly retained as a shared-only entry and incrementally
//! synced, then reaped after idle expiry (the A08/AC-22 "one incremental
//! LiveSession per actively observed exact source across ALL Host clients"
//! invariant).
//! [POS]: plan M3 / audit AF-05 + A08/AC-22. No GPUI/HTTP dependencies; the
//! lease (`LiveLease`) decrements on Drop; incremental sync is driven only
//! by the lease holder or the shared-only reaping path, and readers must not
//! drain someone else's delta.

use crate::conversation_service::ConversationServiceError;
use shardlane_history::models::SessionFileRef;
use shardlane_history::{HistoryCatalog, LiveSession, LiveSnapshot, LiveSync};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a shared-only (unsubscribed) entry stays retained after its last
/// shared read before the bounded reaper removes it. Mobile polls every 2s, so
/// a 30s retain covers the active-viewer case without a standing fleet.
const DEFAULT_SHARED_RETAIN: Duration = Duration::from_secs(30);

/// Exact source identity of one live semantic session.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct LiveSessionKey {
    pub agent: shardlane_history::AgentId,
    pub file_path: String,
}

impl LiveSessionKey {
    fn from_source(source: &SessionFileRef) -> Self {
        Self {
            agent: source.agent,
            file_path: source.file_path.clone(),
        }
    }
}

struct ManagedSession {
    live: LiveSession,
    subscribers: usize,
    /// Last non-destructive shared read; drives shared-only expiry.
    last_shared_read: Instant,
}

/// Per-session interior ownership (CR-11): the global map lock covers only
/// lookup/insert/reap; all LiveSession filesystem I/O happens under the
/// per-session mutex so one slow source cannot stall other conversations.
type SharedSession = Arc<Mutex<ManagedSession>>;

fn lock_sessions(
    sessions: &Mutex<HashMap<LiveSessionKey, SharedSession>>,
) -> std::sync::MutexGuard<'_, HashMap<LiveSessionKey, SharedSession>> {
    sessions.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Shared, thread-safe owner of subscribed live semantic sessions.
pub struct ConversationSessionManager {
    history_db_path: PathBuf,
    sessions: Mutex<HashMap<LiveSessionKey, SharedSession>>,
    shared_retain: Duration,
}

impl ConversationSessionManager {
    pub fn new(history_db_path: impl Into<PathBuf>) -> Self {
        Self {
            history_db_path: history_db_path.into(),
            sessions: Mutex::new(HashMap::new()),
            shared_retain: DEFAULT_SHARED_RETAIN,
        }
    }

    /// Test seam: a short shared-only retain window.
    #[cfg(test)]
    fn with_shared_retain(history_db_path: PathBuf, shared_retain: Duration) -> Self {
        Self {
            history_db_path,
            sessions: Mutex::new(HashMap::new()),
            shared_retain,
        }
    }

    /// CR-15: an actual manager-owned timer that reaps idle shared-only
    /// sessions, so retention does not depend on future manager operations.
    /// Call once from the process owner (GUI main).
    pub fn spawn_shared_reaper(self: &Arc<Self>) {
        let manager = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("shardlane-live-session-reaper".to_string())
            .spawn(move || loop {
                std::thread::sleep(manager.shared_retain);
                // R4-P1: reap through the try_lock-based helper — a session
                // mid-sync must never hijack the GLOBAL map lock (C05); a busy
                // session is definitionally in use and is re-checked next cycle.
                let mut sessions = lock_sessions(&manager.sessions);
                manager.reap_shared_only_locked(&mut sessions);
            });
        if let Err(error) = spawned {
            // A silently missing reaper would make shared-only sessions
            // leak for the whole process lifetime.
            crate::diagnostics::lag_log(format_args!(
                "shared live-session reaper failed to spawn: {error}"
            ));
        }
    }

    /// History catalog path this manager's transient reads share with the
    /// conversation service (diagnostics/tests).
    pub fn history_db_path(&self) -> &std::path::Path {
        self.history_db_path.as_path()
    }

    fn open_session(source: &SessionFileRef) -> Result<LiveSession, ConversationServiceError> {
        LiveSession::open(source.clone())
            .map_err(|error| ConversationServiceError::Runtime(error.to_string()))
    }

    /// Remove idle shared-only entries. Must be called with the sessions lock
    /// held; performs no I/O beyond map mutation.
    fn reap_shared_only_locked(&self, sessions: &mut HashMap<LiveSessionKey, SharedSession>) {
        // R4-P1: a session mid-sync holds its per-session mutex; blocking on
        // it here would hold the GLOBAL map lock hostage. A busy session is
        // definitionally in use this cycle — keep it and re-check next reap.
        let now = Instant::now();
        sessions.retain(|_, managed| {
            let keep = |managed: &ManagedSession| {
                managed.subscribers > 0
                    || now.duration_since(managed.last_shared_read) < self.shared_retain
            };
            match managed.try_lock() {
                Ok(managed) => keep(&managed),
                Err(std::sync::TryLockError::Poisoned(poison)) => keep(&poison.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) => true,
            }
        });
    }

    /// Acquire a subscriber lease on the exact source. While any lease is held,
    /// this process keeps exactly one incremental session for the source and
    /// the lease holder is the only driver of its incremental `sync`.
    pub fn subscribe(
        self: &Arc<Self>,
        source: SessionFileRef,
    ) -> Result<LiveLease, ConversationServiceError> {
        let key = LiveSessionKey::from_source(&source);
        {
            let sessions = lock_sessions(&self.sessions);
            if let Some(managed) = sessions.get(&key) {
                // A shared-only retained entry is adopted instead of rehydrated.
                let mut managed = managed.lock().unwrap_or_else(|poison| poison.into_inner());
                managed.subscribers += 1;
                return Ok(LiveLease {
                    key,
                    manager: Arc::downgrade(self),
                });
            }
        }
        // Hydrate outside the map lock (A08): a slow source open must not
        // stall unrelated sessions' reads.
        let live = Self::open_session(&source)?;
        let mut sessions = lock_sessions(&self.sessions);
        let session = sessions.entry(key.clone()).or_insert_with(|| {
            Arc::new(Mutex::new(ManagedSession {
                live,
                subscribers: 0,
                last_shared_read: Instant::now(),
            }))
        });
        session
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .subscribers += 1;
        Ok(LiveLease {
            key,
            manager: Arc::downgrade(self),
        })
    }

    /// Initial semantic delivery for a freshly subscribed source. Mirrors the
    /// GUI worker's contract: without an explicit initial delivery the first
    /// sync is `Unchanged` and the surface would appear stuck connecting.
    pub fn initial_delivery(
        &self,
        lease: &LiveLease,
    ) -> Result<LiveSync, ConversationServiceError> {
        let session = self.leased_session(lease)?;
        let mut managed = session.lock().unwrap_or_else(|poison| poison.into_inner());
        Ok(managed.live.initial_delivery())
    }

    fn leased_session(&self, lease: &LiveLease) -> Result<SharedSession, ConversationServiceError> {
        lock_sessions(&self.sessions)
            .get(&lease.key)
            .cloned()
            .ok_or_else(|| ConversationServiceError::Runtime("live lease expired".into()))
    }

    /// Incremental sync on the leased session. Lease holders only.
    pub fn sync(&self, lease: &LiveLease) -> Result<LiveSync, ConversationServiceError> {
        // CR-11: filesystem sync happens under the per-session mutex only.
        let session = self.leased_session(lease)?;
        let mut managed = session.lock().unwrap_or_else(|poison| poison.into_inner());
        managed
            .live
            .sync()
            .map_err(|error| ConversationServiceError::Runtime(error.to_string()))
    }

    /// Bounded snapshot of the leased session (non-destructive).
    pub fn snapshot(&self, lease: &LiveLease) -> Result<LiveSnapshot, ConversationServiceError> {
        let session = self.leased_session(lease)?;
        let managed = session.lock().unwrap_or_else(|poison| poison.into_inner());
        Ok(managed.live.snapshot())
    }

    /// Number of live sessions currently retained (diagnostics/tests).
    pub fn retained_sessions(&self) -> usize {
        lock_sessions(&self.sessions).len()
    }

    /// Whether the exact source currently has a retained session
    /// (diagnostics/tests/operator probes).
    pub fn is_retained(&self, source: &SessionFileRef) -> bool {
        lock_sessions(&self.sessions).contains_key(&LiveSessionKey::from_source(source))
    }

    /// Non-destructive shared read for transient consumers (Remote detail,
    /// insights): serves the observed session's current snapshot without
    /// consuming a lease holder's incremental deltas (A08/AC-22). A source
    /// nobody subscribes to is opened ONCE outside the map lock, retained as a
    /// shared-only entry that syncs incrementally, and reaped after the idle
    /// window — repeated Mobile polling never re-hydrates the full provider
    /// session per request.
    pub fn shared_snapshot(
        &self,
        source: &SessionFileRef,
    ) -> Result<LiveSnapshot, ConversationServiceError> {
        let key = LiveSessionKey::from_source(source);
        {
            // CR-11 (final): the global map lock covers lookup/reap ONLY.
            // Per-session filesystem I/O runs under the per-session mutex
            // after the map lock is released, so one slow source cannot stall
            // unrelated conversations.
            let managed = {
                let mut sessions = lock_sessions(&self.sessions);
                self.reap_shared_only_locked(&mut sessions);
                sessions.get(&key).cloned()
            };
            if let Some(managed) = managed {
                let mut inner = managed.lock().unwrap_or_else(|poison| poison.into_inner());
                inner.last_shared_read = Instant::now();
                if inner.subscribers == 0 {
                    // Shared-only entry: no lease holder drives it, so the
                    // incremental sync is ours (stat + appended bytes; never a
                    // full rehydrate). A failing sync is surfaced as an error
                    // instead of silently serving stale state.
                    inner
                        .live
                        .sync()
                        .map_err(|error| ConversationServiceError::Runtime(error.to_string()))?;
                }
                return Ok(inner.live.snapshot());
            }
        }
        // Miss: hydrate outside the map lock, then insert as shared-only.
        let live = Self::open_session(source)?;
        let snapshot = live.snapshot();
        let mut sessions = lock_sessions(&self.sessions);
        self.reap_shared_only_locked(&mut sessions);
        match sessions.get_mut(&key) {
            // Another thread won the insert race; serve its state instead of
            // keeping two hydrated sessions for one source.
            Some(managed) => {
                let mut inner = managed.lock().unwrap_or_else(|poison| poison.into_inner());
                inner.last_shared_read = Instant::now();
                Ok(inner.live.snapshot())
            }
            None => {
                sessions.insert(
                    key,
                    Arc::new(Mutex::new(ManagedSession {
                        live,
                        subscribers: 0,
                        last_shared_read: Instant::now(),
                    })),
                );
                Ok(snapshot)
            }
        }
    }

    /// Catalog handle sharing the manager's db path (transient readers).
    pub fn open_catalog(&self) -> Result<HistoryCatalog, ConversationServiceError> {
        HistoryCatalog::open(&self.history_db_path)
            .map_err(|error| ConversationServiceError::Runtime(error.to_string()))
    }
}

/// Subscriber lease. Cloning adds a subscriber; dropping the last lease for a
/// source hands the session to the bounded shared-only retention window
/// (A08: an immediately-following Remote poll reuses it instead of
/// rehydrating), and the idle reaper performs the final release.
pub struct LiveLease {
    key: LiveSessionKey,
    manager: std::sync::Weak<ConversationSessionManager>,
}

impl Clone for LiveLease {
    fn clone(&self) -> Self {
        if let Some(manager) = self.manager.upgrade() {
            let sessions = lock_sessions(&manager.sessions);
            if let Some(managed) = sessions.get(&self.key) {
                let mut inner = managed.lock().unwrap_or_else(|poison| poison.into_inner());
                inner.subscribers += 1;
            }
        }
        Self {
            key: self.key.clone(),
            manager: self.manager.clone(),
        }
    }
}

impl Drop for LiveLease {
    fn drop(&mut self) {
        if let Some(manager) = self.manager.upgrade() {
            let sessions = lock_sessions(&manager.sessions);
            if let Some(managed) = sessions.get(&self.key) {
                let mut inner = managed.lock().unwrap_or_else(|poison| poison.into_inner());
                inner.subscribers = inner.subscribers.saturating_sub(1);
                if inner.subscribers == 0 {
                    // Convert to shared-only instead of removing: the bounded
                    // idle reaper owns the final release.
                    inner.last_shared_read = Instant::now();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shardlane_history::AgentId;

    fn claude_source(dir: &std::path::Path, name: &str, contents: &str) -> SessionFileRef {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap_or_else(|error| panic!("write: {error}"));
        SessionFileRef {
            agent: AgentId::ClaudeCode,
            native_id: name.to_string(),
            file_path: path.to_string_lossy().into_owned(),
            mtime_ms: 0,
            size: contents.len() as i64,
        }
    }

    fn fixture_line(text: &str) -> String {
        format!(
            r#"{{"type":"user","timestamp":"2026-08-01T01:00:00Z","message":{{"content":"{text}"}}}}"#,
        )
    }

    fn manager(dir: &std::path::Path) -> Arc<ConversationSessionManager> {
        Arc::new(ConversationSessionManager::new(dir.join("history.sqlite3")))
    }

    #[test]
    fn one_session_per_source_while_subscribed_and_release_on_last_drop() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let source = claude_source(
            dir.path(),
            "s1.jsonl",
            &format!("{}\n", fixture_line("one")),
        );
        let manager = manager(dir.path());

        let first = manager
            .subscribe(source.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(manager.retained_sessions(), 1);
        let second = manager
            .subscribe(source.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            manager.retained_sessions(),
            1,
            "a second subscriber must reuse the shared session"
        );
        let initial = manager
            .initial_delivery(&first)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            !initial.is_unchanged(),
            "fresh subscription must deliver existing content"
        );
        drop(first);
        assert_eq!(manager.retained_sessions(), 1, "lease keeps the session");
        drop(second);
        // A08: the last subscriber hands the session to the bounded shared-only
        // window instead of dropping it; the idle reaper owns the release.
        assert_eq!(manager.retained_sessions(), 1);
        assert!(manager.is_retained(&source));
    }

    #[test]
    fn sync_is_incremental_and_bounded_to_the_leased_source() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = dir.path().join("s2.jsonl");
        std::fs::write(&path, format!("{}\n", fixture_line("first")))
            .unwrap_or_else(|error| panic!("write: {error}"));
        let source = SessionFileRef {
            agent: AgentId::ClaudeCode,
            native_id: "s2".into(),
            file_path: path.to_string_lossy().into_owned(),
            mtime_ms: 0,
            size: 0,
        };
        let manager = manager(dir.path());
        let lease = manager
            .subscribe(source)
            .unwrap_or_else(|error| panic!("{error}"));
        let _ = manager
            .initial_delivery(&lease)
            .unwrap_or_else(|error| panic!("{error}"));

        let unchanged = manager
            .sync(&lease)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(unchanged.is_unchanged());

        std::fs::write(
            &path,
            format!("{}\n{}\n", fixture_line("first"), fixture_line("second")),
        )
        .unwrap_or_else(|error| panic!("append: {error}"));
        let changed = manager
            .sync(&lease)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(!changed.is_unchanged());
        let snapshot = manager
            .snapshot(&lease)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(snapshot
            .messages
            .iter()
            .any(|message| message.text.contains("second")));
    }

    #[test]
    fn shared_reads_do_not_consume_the_subscriber_deltas() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = dir.path().join("s3.jsonl");
        std::fs::write(&path, format!("{}\n", fixture_line("first")))
            .unwrap_or_else(|error| panic!("write: {error}"));
        let source = SessionFileRef {
            agent: AgentId::ClaudeCode,
            native_id: "s3".into(),
            file_path: path.to_string_lossy().into_owned(),
            mtime_ms: 0,
            size: 0,
        };
        let manager = manager(dir.path());
        let lease = manager
            .subscribe(source.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        let _ = manager
            .initial_delivery(&lease)
            .unwrap_or_else(|error| panic!("{error}"));

        std::fs::write(
            &path,
            format!("{}\n{}\n", fixture_line("first"), fixture_line("second")),
        )
        .unwrap_or_else(|error| panic!("append: {error}"));

        // A transient shared read must serve the file content WITHOUT draining
        // the incremental delta the lease holder still needs to observe.
        let shared = manager
            .shared_snapshot(&source)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(manager.retained_sessions(), 1);
        assert!(
            shared
                .messages
                .iter()
                .any(|message| message.text.contains("first")),
            "shared snapshot serves existing state"
        );

        let delta = manager
            .sync(&lease)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            !delta.is_unchanged(),
            "shared reads must not consume the subscriber's delta"
        );
    }

    #[test]
    fn repeated_shared_reads_reuse_one_session_and_expire_when_idle() {
        // A08/AC-22 invariant: away-from-desk Mobile polling (no GUI lease)
        // keeps exactly ONE incremental session per observed source — never a
        // full provider rehydrate per poll — and releases it after idle.
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let source = claude_source(
            dir.path(),
            "s4.jsonl",
            &format!("{}\n", fixture_line("solo")),
        );
        let manager = Arc::new(ConversationSessionManager::with_shared_retain(
            dir.path().join("history.sqlite3"),
            Duration::from_millis(15),
        ));
        for _ in 0..5 {
            let snapshot = manager
                .shared_snapshot(&source)
                .unwrap_or_else(|error| panic!("{error}"));
            assert!(snapshot
                .messages
                .iter()
                .any(|message| message.text.contains("solo")));
        }
        assert!(
            manager.is_retained(&source),
            "repeated polls reuse one retained session"
        );
        assert_eq!(manager.retained_sessions(), 1);

        // Shared-only retention still serves appended content incrementally.
        let path = dir.path().join("s4.jsonl");
        std::fs::write(
            &path,
            format!("{}\n{}\n", fixture_line("solo"), fixture_line("appended")),
        )
        .unwrap_or_else(|error| panic!("append: {error}"));
        let refreshed = manager
            .shared_snapshot(&source)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(refreshed
            .messages
            .iter()
            .any(|message| message.text.contains("appended")));

        // Idle beyond the retain window: the next read reaps the entry.
        std::thread::sleep(Duration::from_millis(30));
        let other = claude_source(
            dir.path(),
            "other.jsonl",
            &format!("{}\n", fixture_line("other")),
        );
        let _ = manager
            .shared_snapshot(&other)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            !manager.is_retained(&source),
            "idle shared-only sessions are reaped"
        );
        assert!(manager.is_retained(&other));
    }

    #[test]
    fn distinct_sources_get_distinct_sessions() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let first = claude_source(dir.path(), "a.jsonl", &format!("{}\n", fixture_line("a")));
        let second = claude_source(dir.path(), "b.jsonl", &format!("{}\n", fixture_line("b")));
        let manager = manager(dir.path());
        let lease_a = manager
            .subscribe(first.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        let lease_b = manager
            .subscribe(second.clone())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(manager.retained_sessions(), 2);
        let first_retained = manager.is_retained(&first);
        let second_retained = manager.is_retained(&second);
        assert!(first_retained && second_retained);
        drop(lease_a);
        drop(lease_b);
        // Both sessions move into the bounded shared-only window (A08); they
        // stay addressable until the idle reaper releases them.
        assert_eq!(manager.retained_sessions(), 2);
    }

    #[test]
    fn leases_on_dropped_managers_are_inert() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let source = claude_source(dir.path(), "c.jsonl", &format!("{}\n", fixture_line("c")));
        let manager = manager(dir.path());
        let lease = manager
            .subscribe(source)
            .unwrap_or_else(|error| panic!("{error}"));
        drop(manager);
        drop(lease);
    }
}
