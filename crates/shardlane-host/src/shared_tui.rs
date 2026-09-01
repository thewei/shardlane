//! Host-owned shared Herdr TUI child/session: the single process owner for
//! the product Herdr TUI across every attached surface.
//!
//! [INPUT]: `crate::herdr::herdr_tui_command` (the normal Herdr TUI child
//! process command), `portable-pty` (PTY/controlling-TTY semantics),
//! `crate::dto`'s
//! `HerdrTuiSessionSummary`/`HerdrTuiMode`/`HerdrTuiSessionStatus`, and
//! `tokio::sync::broadcast` (output fan-out).
//! [OUTPUT]: `HerdrTuiSession` (one child + one PTY + authoritative geometry +
//! output fan-out + startup replay prefix (the mode-setting bytes published
//! before subscribing, see `subscribe_with_startup_replay`) + serialized
//! input seam + resize seam + lifecycle) and `TuiManager` (the process-level
//! single-slot owner: open/restart/get/close/reap).
//! [POS]: the sole Herdr TUI process owner after the A01 convergence. The
//! Desktop GPUI and Remote/Mobile viewers only attach to this session (each
//! holding a viewer-local VT model/viewport) and must no longer spawn their
//! own Herdr TUI client. Herdr's global focus/Tab/resize semantics are
//! exactly why a single client/session is shared.

use crate::dto::{HerdrTuiMode, HerdrTuiSessionStatus, HerdrTuiSessionSummary};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::Path as FsPath;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

static SHARED_TRACE_ENABLED: OnceLock<bool> = OnceLock::new();
static SHARED_TRACE_START: OnceLock<Instant> = OnceLock::new();

fn shared_trace_enabled() -> bool {
    *SHARED_TRACE_ENABLED.get_or_init(|| std::env::var_os("SHARDLANE_TERMINAL_TRACE").is_some())
}

fn shared_trace_now_us() -> u64 {
    u64::try_from(
        SHARED_TRACE_START
            .get_or_init(Instant::now)
            .elapsed()
            .as_micros(),
    )
    .unwrap_or(u64::MAX)
}

pub const DEFAULT_COLS: u16 = 120;
pub const DEFAULT_ROWS: u16 = 32;
/// Owner-level bounds are deliberately wide: the trusted Desktop viewer's raw
/// window grid may exceed the tighter Remote HTTP request bounds below.
const MIN_COLS: u16 = 10;
const MAX_COLS: u16 = 1000;
const MIN_ROWS: u16 = 2;
const MAX_ROWS: u16 = 1000;
/// Tighter bounds for untrusted Remote HTTP size requests (open/resize).
pub const REMOTE_MIN_COLS: u16 = 20;
pub const REMOTE_MAX_COLS: u16 = 400;
pub const REMOTE_MIN_ROWS: u16 = 4;
pub const REMOTE_MAX_ROWS: u16 = 200;
const INPUT_QUEUE_CAPACITY: usize = 128;
/// The queue is bounded by bytes as well as packet count. Adjacent PTY byte
/// packets are coalesced before the writer takes its next item, so a held key
/// or a precise-wheel burst does not lose input merely because it produced more
/// than `INPUT_QUEUE_CAPACITY` tiny packets. The byte cap remains the memory /
/// remote-abuse guard; a genuinely megabyte-sized backlog still reports
/// `Backpressure` instead of blocking the UI thread.
const INPUT_QUEUE_MAX_BYTES: usize = 1024 * 1024;
const OUTPUT_QUEUE_CAPACITY: usize = 256;
/// The Herdr TUI child emits its whole initialization burst (alternate screen,
/// SGR mouse reporting, bracketed paste, focus reporting DECSETs) before any
/// viewer can subscribe, and a broadcast channel has no replay: without this
/// buffer every viewer-local Ghostty model permanently believes those modes
/// are off, which silently kills wheel/pointer encoding on the hosted surface.
/// The prefix is frozen at the first subscription and replayed out-of-band to
/// each new viewer model before any live event.
const STARTUP_REPLAY_CAP_BYTES: usize = 256 * 1024;
const IDLE_LEASE: Duration = Duration::from_secs(300);
const STARTUP_GRACE: Duration = Duration::from_secs(1);
/// CR-08: bounded wait for the old TUI generation to exit during restart.
/// Five seconds leaves headroom for a real Herdr child under a busy test/app
/// host while remaining a strict barrier: no successor may spawn after the
/// deadline unless the OS reap signal has already arrived.
const RESTART_REAP_TIMEOUT: Duration = Duration::from_secs(5);

/// Per-write input cap: matches the Remote `tui_input` encoding boundary so
/// an oversized write cannot occupy the input queue for a long time.
pub const MAX_INPUT_BYTES: usize = 64 * 1024;

struct TuiInputPacket {
    trace_id: u64,
    enqueued_us: u64,
    coalescible: bool,
    coalesced_packets: u32,
    bytes: Vec<u8>,
}

struct InputQueueState {
    packets: VecDeque<TuiInputPacket>,
    queued_bytes: usize,
    closed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputQueueError {
    Closed,
    Full,
}

/// Non-blocking producer / blocking writer queue for the shared TUI PTY.
///
/// A `sync_channel(128)` looks bounded, but it makes packet count—not terminal
/// bytes—the admission decision. macOS key repeat and precision trackpads
/// naturally generate hundreds of one-byte/small packets while the writer is
/// briefly descheduled, so the old channel rejected valid user input. This
/// queue keeps both safety properties: producers never wait for the PTY writer,
/// adjacent packets are merged in-order, and a hard byte budget still rejects a
/// pathological backlog.
struct TuiInputQueue {
    state: Mutex<InputQueueState>,
    changed: Condvar,
}

impl TuiInputQueue {
    fn new() -> Self {
        Self {
            state: Mutex::new(InputQueueState {
                packets: VecDeque::new(),
                queued_bytes: 0,
                closed: false,
            }),
            changed: Condvar::new(),
        }
    }

    /// Enqueue without waiting. Adjacent byte packets can share one write
    /// boundary: the PTY consumes one ordered byte stream, and the boundaries
    /// carry no terminal semantics. Trace metadata follows the newest packet so
    /// diagnostics still identify the freshest user action.
    fn try_push(&self, mut packet: TuiInputPacket) -> Result<(), InputQueueError> {
        let packet_bytes = packet.bytes.len();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.closed {
            return Err(InputQueueError::Closed);
        }
        let next_bytes = state.queued_bytes.saturating_add(packet_bytes);
        if next_bytes > INPUT_QUEUE_MAX_BYTES {
            return Err(InputQueueError::Full);
        }

        let can_coalesce = state
            .packets
            .back()
            .is_some_and(|last| last.coalescible && packet.coalescible);
        if can_coalesce {
            let last = state
                .packets
                .back_mut()
                .unwrap_or_else(|| unreachable!("coalescing requires a queue tail"));
            last.bytes.extend_from_slice(&packet.bytes);
            last.trace_id = packet.trace_id;
            last.enqueued_us = packet.enqueued_us;
            last.coalesced_packets = last
                .coalesced_packets
                .saturating_add(packet.coalesced_packets);
        } else {
            // Keep the packet-count bound as a second invariant even though
            // normal producers coalesce into the tail while the writer drains.
            if state.packets.len() >= INPUT_QUEUE_CAPACITY {
                return Err(InputQueueError::Full);
            }
            packet.coalesced_packets = packet.coalesced_packets.max(1);
            state.packets.push_back(packet);
        }
        state.queued_bytes = next_bytes;
        self.changed.notify_one();
        Ok(())
    }

    fn recv(&self) -> Option<TuiInputPacket> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        loop {
            if let Some(packet) = state.packets.pop_front() {
                state.queued_bytes = state.queued_bytes.saturating_sub(packet.bytes.len());
                return Some(packet);
            }
            if state.closed {
                return None;
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(|poison| poison.into_inner());
        }
    }

    fn close(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.closed = true;
        self.changed.notify_all();
    }
}

#[derive(Debug)]
pub enum TuiError {
    HerdrUnavailable(String),
    UnsupportedProtocol(u32),
    Spawn(String),
    InvalidSession,
    Closed,
    Io(String),
    Backpressure,
    InvalidInput(String),
}

impl std::fmt::Display for TuiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HerdrUnavailable(_) => formatter.write_str("Herdr runtime unavailable"),
            Self::UnsupportedProtocol(actual) => {
                write!(
                    formatter,
                    "Herdr protocol {actual} does not support the shared TUI"
                )
            }
            Self::Spawn(message) | Self::Io(message) => formatter.write_str(message),
            Self::InvalidSession => formatter.write_str("unknown TUI session"),
            Self::Closed => formatter.write_str("TUI session is closed"),
            Self::Backpressure => formatter.write_str("TUI input queue is full"),
            Self::InvalidInput(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for TuiError {}

/// Shared-session fan-out event. `Output` bytes are raw PTY output; every
/// attached viewer feeds them into its own local VT model.
#[derive(Clone, Debug)]
pub enum TuiEvent {
    Output {
        revision: u64,
        bytes: Vec<u8>,
        /// Monotonic publish instant for local latency diagnostics. Events
        /// never cross the Remote wire, so no wall-clock conversion is needed
        /// and no user text is recorded here.
        published_at: Instant,
    },
    Status {
        summary: HerdrTuiSessionSummary,
    },
    /// Internal lifecycle wake used to release a blocking desktop viewer
    /// forwarder when that viewer is dropped. It is never serialized to a
    /// Remote client and carries no runtime state.
    #[doc(hidden)]
    Wake,
}

struct TuiState {
    status: HerdrTuiSessionStatus,
    cols: u16,
    rows: u16,
    revision: u64,
    last_activity: Instant,
}

/// The only authoritative signal for releasing a TUI slot.  Session status is
/// presentation state and may move to `Stopped` when the PTY reader reaches
/// EOF before the OS child has actually been reaped; the child-owning waiter
/// marks this state only after `Child::wait()` (or `try_wait`) returns.
struct ChildReapState {
    done: Mutex<bool>,
    changed: Condvar,
}

impl ChildReapState {
    fn new() -> Self {
        Self {
            done: Mutex::new(false),
            changed: Condvar::new(),
        }
    }

    fn mark_done(&self) {
        let mut done = self
            .done
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if !*done {
            *done = true;
            self.changed.notify_all();
        }
    }

    fn is_done(&self) -> bool {
        *self
            .done
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn wait(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut done = self
            .done
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        while !*done {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            if remaining.is_zero() {
                return false;
            }
            let result = self.changed.wait_timeout(done, remaining);
            done = match result {
                Ok((guard, _)) => guard,
                Err(poison) => poison.into_inner().0,
            };
        }
        true
    }

    fn wait_forever(&self) {
        let mut done = self
            .done
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        while !*done {
            done = self
                .changed
                .wait(done)
                .unwrap_or_else(|poison| poison.into_inner());
        }
    }
}

/// A single shared Herdr TUI child and PTY. Every attached surface (Desktop
/// GPUI viewer, Remote Web/Mobile viewer) subscribes to this object; no viewer
/// receives a private runtime, child, or viewport.
pub struct HerdrTuiSession {
    id: String,
    state: Mutex<TuiState>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    input: Mutex<Option<Arc<TuiInputQueue>>>,
    child: Arc<Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>>,
    reap: Arc<ChildReapState>,
    events: broadcast::Sender<TuiEvent>,
    startup_replay: Mutex<StartupReplay>,
    trace_last_input_id: AtomicU64,
    trace_last_write_us: AtomicU64,
}

/// Output bytes published before the first subscription (capped). See
/// [`STARTUP_REPLAY_CAP_BYTES`].
enum StartupReplay {
    Capturing(Vec<u8>),
    Frozen(Arc<[u8]>),
}

impl HerdrTuiSession {
    fn summary_locked(&self, state: &TuiState) -> HerdrTuiSessionSummary {
        HerdrTuiSessionSummary {
            id: self.id.clone(),
            mode: HerdrTuiMode::Shared,
            status: state.status,
            cols: state.cols,
            rows: state.rows,
            revision: state.revision,
        }
    }

    pub fn summary(&self) -> HerdrTuiSessionSummary {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.last_activity = Instant::now();
        self.summary_locked(&state)
    }

    pub fn is_running(&self) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        matches!(
            state.status,
            HerdrTuiSessionStatus::Starting | HerdrTuiSessionStatus::Running
        )
    }

    fn touch(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.last_activity = Instant::now();
    }

    /// Subscribe to the shared output/status fan-out. Holding a subscription
    /// counts as an active viewer for idle-reaping purposes.
    pub fn subscribe(&self) -> broadcast::Receiver<TuiEvent> {
        self.touch();
        self.events.subscribe()
    }

    /// Subscribe and additionally return the frozen startup byte prefix
    /// (possibly empty once drained). A fresh viewer must apply the replay to
    /// its model **before** consuming any live event: the prefix carries the
    /// child's startup DECSET mode sequences (alternate screen, SGR mouse
    /// reporting, bracketed/focus modes) that were published before any
    /// subscriber existed and are therefore absent from the broadcast stream.
    /// Screen-content convergence after the replay stays owned by the
    /// caller's existing `force_redraw` handshake.
    pub fn subscribe_with_startup_replay(&self) -> (broadcast::Receiver<TuiEvent>, Vec<u8>) {
        self.touch();
        let replay = self.freeze_startup_replay();
        (self.events.subscribe(), replay.to_vec())
    }

    /// Freeze the capturing replay buffer at first use and hand every caller
    /// the same immutable prefix.
    fn freeze_startup_replay(&self) -> Arc<[u8]> {
        let mut guard = self
            .startup_replay
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if matches!(*guard, StartupReplay::Capturing(_)) {
            let frozen = match std::mem::replace(
                &mut *guard,
                StartupReplay::Frozen(Arc::from(Vec::new())),
            ) {
                StartupReplay::Capturing(buffer) => Arc::from(buffer),
                StartupReplay::Frozen(_) => unreachable!("matched Capturing above"),
            };
            *guard = StartupReplay::Frozen(frozen);
        }
        match &*guard {
            StartupReplay::Frozen(bytes) => bytes.clone(),
            StartupReplay::Capturing(_) => unreachable!("frozen above"),
        }
    }

    /// Live viewer count (subscriptions). Used by the manager's idle reaper.
    pub fn viewer_count(&self) -> usize {
        self.events.receiver_count()
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    fn set_status(&self, status: HerdrTuiSessionStatus) {
        let summary = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if state.status == status {
                return;
            }
            state.status = status;
            state.revision = state.revision.saturating_add(1);
            state.last_activity = Instant::now();
            self.summary_locked(&state)
        };
        let _ = self.events.send(TuiEvent::Status { summary });
    }

    fn publish_output(&self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        {
            let mut replay = self
                .startup_replay
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if let StartupReplay::Capturing(buffer) = &mut *replay {
                let remaining = STARTUP_REPLAY_CAP_BYTES.saturating_sub(buffer.len());
                let take = bytes.len().min(remaining);
                buffer.extend_from_slice(&bytes[..take]);
            }
        }
        let revision = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state.revision = state.revision.saturating_add(1);
            state.last_activity = Instant::now();
            state.revision
        };
        let _ = self.events.send(TuiEvent::Output {
            revision,
            bytes,
            published_at: Instant::now(),
        });
    }

    /// Wakes all subscribers without changing runtime revision/state. The
    /// Desktop viewer uses this during teardown to interrupt a blocking
    /// receive; other viewers ignore this internal event and keep receiving.
    pub fn wake_subscribers(&self) {
        let _ = self.events.send(TuiEvent::Wake);
    }

    pub fn send_bytes(&self, data: &[u8]) -> Result<(), TuiError> {
        self.send_bytes_with_trace(data, 0)
    }

    /// Enqueues bytes with an optional viewer-local trace id. The trace id
    /// is for the native Desktop diagnostic seam only; Remote callers keep
    /// using `send_bytes`, and the runtime protocol or payload is unchanged.
    pub fn send_bytes_with_trace(&self, data: &[u8], trace_id: u64) -> Result<(), TuiError> {
        self.send_bytes_with_trace_kind(data, trace_id, true)
    }

    /// Enqueue bytes with an explicit coalescing boundary.  The desktop path
    /// marks paste/focus reports as non-coalescible because they are protocol
    /// transactions; ordinary text/key/mouse packets may merge while retaining
    /// their exact byte order.  Remote callers use `send_bytes_with_trace`,
    /// which keeps the legacy all-bytes contract.
    pub fn send_bytes_with_trace_kind(
        &self,
        data: &[u8],
        trace_id: u64,
        coalescible: bool,
    ) -> Result<(), TuiError> {
        if data.is_empty() {
            return Ok(());
        }
        if data.len() > MAX_INPUT_BYTES {
            return Err(TuiError::Backpressure);
        }
        if !self.is_running() {
            return Err(TuiError::Closed);
        }
        let queue = self
            .input
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
            .ok_or(TuiError::Closed)?;
        self.touch();
        let packet = TuiInputPacket {
            trace_id,
            enqueued_us: (trace_id != 0 && shared_trace_enabled())
                .then(shared_trace_now_us)
                .unwrap_or(0),
            coalescible,
            coalesced_packets: 1,
            bytes: data.to_vec(),
        };
        match queue.try_push(packet) {
            Ok(()) => Ok(()),
            Err(InputQueueError::Closed) => Err(TuiError::Closed),
            Err(InputQueueError::Full) => Err(TuiError::Backpressure),
        }
    }

    // C22: the `send_input` convenience wrapper over `send_bytes` was deleted
    // (zero callers — every surface sends bytes directly).

    /// Resize the shared PTY. This is the single authoritative geometry seam:
    /// any attached viewer may request a resize, and it applies globally to
    /// the one Herdr TUI client (accepted product behavior).
    pub fn resize(&self, cols: u16, rows: u16) -> Result<HerdrTuiSessionSummary, TuiError> {
        validate_size(cols, rows)?;
        if !self.is_running() {
            return Err(TuiError::Closed);
        }
        let master = self
            .master
            .lock()
            .map_err(|error| TuiError::Io(error.to_string()))?;
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| TuiError::Io(format!("TUI resize failed: {error}")))?;
        let summary = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            state.cols = cols;
            state.rows = rows;
            state.revision = state.revision.saturating_add(1);
            state.last_activity = Instant::now();
            self.summary_locked(&state)
        };
        let _ = self.events.send(TuiEvent::Status {
            summary: summary.clone(),
        });
        Ok(summary)
    }

    /// Deterministic full repaint: TIOCSWINSZ only raises SIGWINCH when the
    /// size actually changes, so when the requested size already matches we
    /// nudge one row and back to force the child to repaint its whole surface
    /// for freshly attached viewers.
    pub fn force_redraw(&self) -> Result<(), TuiError> {
        let summary = self.summary();
        let nudged_rows = summary.rows.saturating_sub(1).max(MIN_ROWS);
        if nudged_rows != summary.rows {
            self.resize(summary.cols, nudged_rows)?;
            self.resize(summary.cols, summary.rows)?;
        } else {
            self.resize(summary.cols, summary.rows)?;
        }
        Ok(())
    }

    pub fn stop(&self) -> bool {
        self.stop_with_reap(Some(RESTART_REAP_TIMEOUT))
    }

    /// Bounded blocking wait for the child's OS exit (P0-09 reaper support).
    pub fn wait_for_exit(&self, timeout: Duration) -> bool {
        self.reap.wait(timeout)
    }

    /// CR-08 (final): stop with a bounded termination barrier and REPORT
    /// whether the old OS child was proven exited. A restart that cannot
    /// prove the old generation dead must fail closed instead of spawning a
    /// successor alongside a potentially-alive child.
    pub fn stop_with_reap(&self, reap_deadline: Option<Duration>) -> bool {
        let input = self
            .input
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        if let Some(input) = input {
            input.close();
        }
        let child = self
            .child
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take();
        let mut proven_exited = self.reap.is_done();
        if let Some(mut child) = child {
            proven_exited = false;
            let _ = child.kill();
            if let Some(deadline_duration) = reap_deadline {
                let deadline = Instant::now() + deadline_duration;
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => {
                            self.reap.mark_done();
                            proven_exited = true;
                            break;
                        }
                        Ok(None) if Instant::now() >= deadline => {
                            // Not proven dead within the barrier: report it.
                            // The background wait below remains the reaper of
                            // last resort, but no successor may spawn yet.
                            proven_exited = false;
                            break;
                        }
                        Ok(None) => thread::sleep(Duration::from_millis(5)),
                        Err(_) => {
                            // A wait error proves nothing about liveness.
                            proven_exited = false;
                            break;
                        }
                    }
                }
            }
            if !proven_exited {
                let reap = Arc::clone(&self.reap);
                thread::spawn(move || {
                    if child.wait().is_ok() {
                        reap.mark_done();
                    }
                });
            }
        }
        self.set_status(HerdrTuiSessionStatus::Stopped);
        proven_exited
    }

    fn is_idle(&self) -> bool {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.last_activity.elapsed() >= IDLE_LEASE && self.events.receiver_count() == 0
    }
}

/// P0-09: explicit slot lifecycle. `Terminating` means the previous child has
/// NOT been proven exited — no successor may spawn until the background
/// reaper confirms death, preserving the one-live-child invariant even when a
/// child ignores the kill.
#[derive(Clone)]
enum TuiSlot {
    Empty,
    Running(Arc<HerdrTuiSession>),
    Terminating(Arc<HerdrTuiSession>),
}

/// Process-level single-slot owner of the shared Herdr TUI session. One Host
/// process holds one `TuiManager`; Desktop and Remote viewers attach to the
/// session it holds instead of spawning their own Herdr child.
pub struct TuiManager {
    session: Mutex<TuiSlot>,
}

impl Default for TuiManager {
    fn default() -> Self {
        Self {
            session: Mutex::new(TuiSlot::Empty),
        }
    }
}

/// Arm the Empty transition for a Terminating slot: once the OS confirms the
/// old child exited, the slot becomes spawnable again.
fn arm_termination_reap(slot: Arc<HerdrTuiSession>, manager: Weak<TuiManager>) {
    thread::spawn(move || {
        // The first bounded wait is only a responsiveness budget.  If it
        // expires, keep this owner thread parked until the child-owning
        // `wait()` reports actual OS reap; releasing the slot on timeout would
        // recreate the old-child/new-child overlap.
        if !slot.wait_for_exit(Duration::from_secs(30)) {
            slot.reap.wait_forever();
        }
        if let Some(manager) = manager.upgrade() {
            let mut guard = manager
                .session
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if matches!(&*guard, TuiSlot::Terminating(current) if Arc::ptr_eq(current, &slot)) {
                *guard = TuiSlot::Empty;
            }
        }
    });
}

/// Shared stop+rearm sequence (C19): stop the slot's child under the bounded
/// reap barrier; a proven exit releases the slot to Empty, an unproven one
/// keeps the session visible as `Terminating` and hands the final release to
/// the armed background reaper. Consumes the slot guard: the unproven arm
/// must drop it before arming the reaper.
fn stop_slot_and_rearm(
    mut guard: std::sync::MutexGuard<'_, TuiSlot>,
    session: Arc<HerdrTuiSession>,
    manager: &Arc<TuiManager>,
) -> bool {
    let proven = session.stop_with_reap(Some(RESTART_REAP_TIMEOUT));
    if proven {
        *guard = TuiSlot::Empty;
    } else {
        *guard = TuiSlot::Terminating(session.clone());
        drop(guard);
        arm_termination_reap(session, Arc::downgrade(manager));
    }
    proven
}

/// Process-level registry of per-instance TUI managers: exactly one
/// `TuiManager` (⇒ at most one hosted TUI child) per Herdr session, shared by
/// every viewer — desktop windows and Remote/mobile clients attach as
/// broadcast subscribers of the same child. Keys are session names; herdr's
/// own `default` session uses the same key as any other.
#[derive(Default)]
pub struct TuiManagerRegistry {
    managers: Mutex<std::collections::HashMap<String, Arc<TuiManager>>>,
}

impl TuiManagerRegistry {
    fn key(session: Option<&str>) -> String {
        session.map_or_else(|| "default".to_string(), str::to_string)
    }

    /// Returns (creating when absent) the manager owning `session`'s TUI child.
    pub fn get_or_create(self: &Arc<Self>, session: Option<&str>) -> Arc<TuiManager> {
        let key = Self::key(session);
        self.get_or_create_keyed(&key)
    }

    /// Registry key variant for B1: bridge-scoped managers use a device-scoped
    /// key so same-named sessions on different machines stay distinct.
    pub fn get_or_create_keyed(self: &Arc<Self>, key: &str) -> Arc<TuiManager> {
        let mut managers = self
            .managers
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        managers
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(TuiManager::default()))
            .clone()
    }

    /// Looks a TUI session up across every instance's manager (remote client
    /// session ids are process-unique `tui-<uuid>` strings).
    pub fn get_session(&self, id: &str) -> Option<Arc<HerdrTuiSession>> {
        let managers = self
            .managers
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        managers.values().find_map(|manager| manager.get(id))
    }

    /// Closes a session id wherever it lives. `Ok(())` when no manager holds it.
    pub fn close_session(self: &Arc<Self>, id: &str) -> Result<(), TuiError> {
        let managers = self
            .managers
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let holder = managers
            .values()
            .find_map(|manager| manager.get(id).map(|_| manager.clone()));
        match holder {
            Some(manager) => manager.close(id),
            None => Ok(()),
        }
    }

    /// Stops every manager's child. Only for registries the caller OWNS
    /// (loopback tests); an injected registry's children belong to the GUI.
    pub fn stop_all_managers(self: &Arc<Self>) {
        let managers = self
            .managers
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        for manager in managers.values() {
            manager.stop_all();
        }
    }

    /// Reaps idle managers across every instance (remote reaper cadence).
    pub fn reap_idle_all(&self) {
        let managers = self
            .managers
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        for manager in managers.values() {
            manager.reap_idle();
        }
    }
}

impl TuiManager {
    /// Idempotent open: return the existing running shared session, or spawn
    /// exactly one new child. A slot still `Terminating` (previous child not
    /// proven exited) fails closed instead of overlapping children (P0-09).
    pub fn open(
        self: &Arc<Self>,
        cols: u16,
        rows: u16,
        socket_override: Option<&FsPath>,
    ) -> Result<Arc<HerdrTuiSession>, TuiError> {
        validate_size(cols, rows)?;
        let mut guard = self
            .session
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match guard.clone() {
            TuiSlot::Running(existing) if existing.is_running() => {
                existing.touch();
                return Ok(existing);
            }
            TuiSlot::Terminating(_) => {
                return Err(TuiError::Spawn(
                    "the previous Herdr TUI child has not been confirmed exited; retry shortly"
                        .to_string(),
                ));
            }
            TuiSlot::Running(dead) => {
                // A child that reached EOF still needs a PROVEN exit before a
                // successor spawns.
                let proven = dead.stop_with_reap(Some(RESTART_REAP_TIMEOUT));
                if !proven {
                    *guard = TuiSlot::Terminating(dead.clone());
                    drop(guard);
                    arm_termination_reap(dead, Arc::downgrade(self));
                    return Err(TuiError::Spawn(
                        "the previous Herdr TUI child did not exit within the barrier; \
                         retry shortly"
                            .to_string(),
                    ));
                }
            }
            TuiSlot::Empty => {}
        }
        let session = spawn_tui_session(cols, rows, socket_override)?;
        *guard = TuiSlot::Running(session.clone());
        Ok(session)
    }

    /// True restart: prove the old generation exited (bounded barrier), then
    /// spawn the fresh generation. Fails closed while a child's death is
    /// unproven (P0-09: max one live Herdr TUI child at every instant).
    pub fn restart(
        self: &Arc<Self>,
        cols: u16,
        rows: u16,
        socket_override: Option<&FsPath>,
    ) -> Result<Arc<HerdrTuiSession>, TuiError> {
        validate_size(cols, rows)?;
        let mut guard = self
            .session
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let TuiSlot::Terminating(_) = &*guard {
            return Err(TuiError::Spawn(
                "the previous Herdr TUI child has not been confirmed exited; retry shortly"
                    .to_string(),
            ));
        }
        if let TuiSlot::Running(existing) = guard.clone() {
            let proven = existing.stop_with_reap(Some(RESTART_REAP_TIMEOUT));
            if !proven {
                *guard = TuiSlot::Terminating(existing.clone());
                drop(guard);
                arm_termination_reap(existing, Arc::downgrade(self));
                return Err(TuiError::Spawn(format!(
                    "the previous Herdr TUI child did not exit within {}ms; \
                     restart aborted to preserve the one-child invariant",
                    RESTART_REAP_TIMEOUT.as_millis()
                )));
            }
        }
        let session = spawn_tui_session(cols, rows, socket_override)?;
        *guard = TuiSlot::Running(session.clone());
        Ok(session)
    }

    pub fn get(&self, id: &str) -> Option<Arc<HerdrTuiSession>> {
        let guard = self
            .session
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let session = match &*guard {
            TuiSlot::Running(session) => Some(session.clone()),
            _ => None,
        }?;
        if session.id == id {
            session.touch();
            Some(session)
        } else {
            None
        }
    }

    pub fn close(self: &Arc<Self>, id: &str) -> Result<(), TuiError> {
        let guard = self
            .session
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let Some(session) = (match &*guard {
            TuiSlot::Running(session) => Some(session.clone()),
            _ => None,
        }) else {
            return Err(TuiError::InvalidSession);
        };
        if session.id != id {
            return Err(TuiError::InvalidSession);
        }
        // Closing is a release request, not permission for one viewer to tear
        // down a session that other attached viewers still observe.
        if session.events.receiver_count() > 0 {
            session.touch();
            return Ok(());
        }
        stop_slot_and_rearm(guard, session, self);
        Ok(())
    }

    pub fn reap_idle(self: &Arc<Self>) {
        let guard = self
            .session
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let idle = match &*guard {
            TuiSlot::Running(session) if session.is_idle() => Some(session.clone()),
            _ => None,
        };
        if let Some(session) = idle {
            stop_slot_and_rearm(guard, session, self);
        }
    }

    pub fn stop_all(self: &Arc<Self>) {
        let mut guard = self
            .session
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        match guard.clone() {
            TuiSlot::Running(session) => {
                stop_slot_and_rearm(guard, session, self);
            }
            TuiSlot::Terminating(session) => {
                // The first stop already owns the child waiter.  A repeated
                // shutdown request may kill again, but it must not release the
                // slot until that same waiter proves the OS child is reaped.
                if session.reap.is_done() {
                    *guard = TuiSlot::Empty;
                }
            }
            TuiSlot::Empty => {}
        }
    }
}

pub(crate) fn validate_size(cols: u16, rows: u16) -> Result<(), TuiError> {
    if (MIN_COLS..=MAX_COLS).contains(&cols) && (MIN_ROWS..=MAX_ROWS).contains(&rows) {
        Ok(())
    } else {
        Err(TuiError::Io(format!(
            "TUI size must be {MIN_COLS}..={MAX_COLS} columns and {MIN_ROWS}..={MAX_ROWS} rows"
        )))
    }
}

/// Bounds check for untrusted Remote HTTP size requests (open/resize).
pub fn validate_remote_size(cols: u16, rows: u16) -> Result<(), TuiError> {
    if (REMOTE_MIN_COLS..=REMOTE_MAX_COLS).contains(&cols)
        && (REMOTE_MIN_ROWS..=REMOTE_MAX_ROWS).contains(&rows)
    {
        Ok(())
    } else {
        Err(TuiError::Io(format!(
            "TUI size must be {REMOTE_MIN_COLS}..={REMOTE_MAX_COLS} columns and \
             {REMOTE_MIN_ROWS}..={REMOTE_MAX_ROWS} rows"
        )))
    }
}

fn spawn_tui_session(
    cols: u16,
    rows: u16,
    socket_override: Option<&FsPath>,
) -> Result<Arc<HerdrTuiSession>, TuiError> {
    let mut command = crate::herdr::herdr_tui_command().map_err(TuiError::Spawn)?;
    if let Some(socket) = socket_override {
        command.env("HERDR_SOCKET_PATH", socket);
    }
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| TuiError::Spawn(format!("open TUI pty: {error}")))?;
    let mut builder = CommandBuilder::new(command.get_program());
    builder.env_clear();
    builder.args(command.get_args());
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => builder.env(key, value),
            None => builder.env_remove(key),
        }
    }
    if let Some(cwd) = command.get_current_dir() {
        builder.cwd(cwd);
    }
    // R7-P0-05: own the child immediately after OS spawn. If any subsequent
    // fallible setup step fails, the guard kills and waits the child so it is
    // never silently orphaned before a HerdrTuiSession takes ownership.
    let raw_child = pair
        .slave
        .spawn_command(builder)
        .map_err(|error| TuiError::Spawn(format!("spawn TUI child: {error}")))?;
    drop(pair.slave);
    let mut guard = UnpublishedTuiChild::new(raw_child);

    // Startup probe: must run while child is held by the guard.
    wait_for_child_start(guard.as_mut())?;
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| TuiError::Spawn(format!("clone TUI reader: {error}")))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|error| TuiError::Spawn(format!("take TUI writer: {error}")))?;
    let master = Arc::new(Mutex::new(pair.master));
    let input_queue = Arc::new(TuiInputQueue::new());
    let writer_queue = Arc::clone(&input_queue);
    let (events, _) = broadcast::channel::<TuiEvent>(OUTPUT_QUEUE_CAPACITY);
    let id = format!("tui-{}", uuid::Uuid::new_v4());
    // Disarm: transfer child ownership into the session's Arc<Mutex<Option<...>>>.
    let child = Arc::new(Mutex::new(Some(guard.disarm())));
    let reap = Arc::new(ChildReapState::new());
    let session = Arc::new_cyclic(|weak: &Weak<HerdrTuiSession>| {
        let writer_weak = weak.clone();
        thread::spawn(move || {
            while let Some(packet) = writer_queue.recv() {
                let write_started = Instant::now();
                let queue_us = if packet.trace_id != 0 && shared_trace_enabled() {
                    shared_trace_now_us().saturating_sub(packet.enqueued_us)
                } else {
                    0
                };
                if writer
                    .write_all(&packet.bytes)
                    .and_then(|_| writer.flush())
                    .is_err()
                {
                    if let Some(session) = writer_weak.upgrade() {
                        session.set_status(HerdrTuiSessionStatus::Failed);
                    }
                    break;
                }
                if shared_trace_enabled() {
                    let written_us = shared_trace_now_us();
                    if let Some(session) = writer_weak.upgrade() {
                        // A remote/untraced packet must break the attribution chain; otherwise
                        // the next PTY read could be incorrectly reported as the previous local
                        // key-repeat packet's response.
                        session
                            .trace_last_input_id
                            .store(packet.trace_id, Ordering::Relaxed);
                        session
                            .trace_last_write_us
                            .store(written_us, Ordering::Relaxed);
                    }
                    if packet.trace_id != 0 {
                        let write_us =
                            u64::try_from(write_started.elapsed().as_micros()).unwrap_or(u64::MAX);
                        crate::diagnostics::lag_log(format_args!(
                            "terminal.shared.write input_id={} bytes={} coalesced={} queue_us={} write_flush_us={write_us}",
                            packet.trace_id,
                            packet.bytes.len(),
                            packet.coalesced_packets,
                            queue_us,
                        ));
                    }
                }
            }
        });
        let reader_weak = weak.clone();
        thread::spawn(move || {
            let mut buffer = [0u8; 65_536];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if let Some(session) = reader_weak.upgrade() {
                            if shared_trace_enabled() {
                                let input_id = session.trace_last_input_id.load(Ordering::Relaxed);
                                if input_id != 0 {
                                    let since_write_us = shared_trace_now_us().saturating_sub(
                                        session.trace_last_write_us.load(Ordering::Relaxed),
                                    );
                                    crate::diagnostics::lag_log(format_args!(
                                        "terminal.shared.read bytes={read} input_id={input_id} since_write_us={since_write_us}",
                                    ));
                                }
                            }
                            session.publish_output(buffer[..read].to_vec());
                        } else {
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => {
                        if let Some(session) = reader_weak.upgrade() {
                            session.set_status(HerdrTuiSessionStatus::Failed);
                        }
                        return;
                    }
                }
            }
            if let Some(session) = reader_weak.upgrade() {
                session.set_status(HerdrTuiSessionStatus::Stopped);
            }
        });
        HerdrTuiSession {
            id,
            state: Mutex::new(TuiState {
                status: HerdrTuiSessionStatus::Running,
                cols,
                rows,
                revision: 0,
                last_activity: Instant::now(),
            }),
            master,
            input: Mutex::new(Some(input_queue)),
            child,
            reap,
            events,
            startup_replay: Mutex::new(StartupReplay::Capturing(Vec::new())),
            trace_last_input_id: AtomicU64::new(0),
            trace_last_write_us: AtomicU64::new(0),
        }
    });
    Ok(session)
}

/// Do not return a successful session for a Herdr client that exits during
/// startup (for example when the socket/runtime attach fails). The grace
/// window is bounded and runs in the blocking open worker, so callers can
/// surface a truthful error before publishing a session id.
fn wait_for_child_start(child: &mut dyn portable_pty::Child) -> Result<(), TuiError> {
    let deadline = Instant::now() + STARTUP_GRACE;
    loop {
        match child
            .try_wait()
            .map_err(|error| TuiError::Spawn(format!("probe TUI child: {error}")))?
        {
            Some(status) => {
                return Err(TuiError::Spawn(format!(
                    "Herdr TUI exited during startup: {status:?}"
                )))
            }
            None if Instant::now() >= deadline => return Ok(()),
            None => thread::sleep(Duration::from_millis(10)),
        }
    }
}

/// R7-P0-05: RAII guard that owns an OS child from the moment it is spawned
/// until it is either transferred into a published `HerdrTuiSession` (via
/// `disarm()`) or dropped on setup failure (which kills and background-reaps
/// the child so it is never silently orphaned).
struct UnpublishedTuiChild {
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
}

impl UnpublishedTuiChild {
    fn new(child: Box<dyn portable_pty::Child + Send + Sync>) -> Self {
        Self { child: Some(child) }
    }

    fn as_mut(&mut self) -> &mut dyn portable_pty::Child {
        self.child
            .as_mut()
            .unwrap_or_else(|| unreachable!("UnpublishedTuiChild already disarmed"))
            .as_mut()
    }

    /// Transfer ownership into the caller (a fully constructed HerdrTuiSession).
    /// After this call the guard no longer kills on drop.
    fn disarm(mut self) -> Box<dyn portable_pty::Child + Send + Sync> {
        self.child
            .take()
            .unwrap_or_else(|| unreachable!("UnpublishedTuiChild already disarmed"))
    }
}

impl Drop for UnpublishedTuiChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Setup failed before publication: kill and background-reap so the
            // process is never orphaned.  A wait error here proves neither exit
            // nor success, so we accept the child may linger briefly as a zombie
            // rather than blocking the caller.
            let _ = child.kill();
            thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // R7-P0-05: mock child for testing UnpublishedTuiChild RAII behavior
    // without requiring a real PTY spawn.
    #[derive(Debug)]
    struct MockChild {
        kill_called: Arc<Mutex<bool>>,
        wait_called: Arc<Mutex<bool>>,
    }

    impl MockChild {
        fn new() -> (Self, Arc<Mutex<bool>>, Arc<Mutex<bool>>) {
            let kill = Arc::new(Mutex::new(false));
            let wait = Arc::new(Mutex::new(false));
            (
                Self {
                    kill_called: Arc::clone(&kill),
                    wait_called: Arc::clone(&wait),
                },
                kill,
                wait,
            )
        }
    }

    impl portable_pty::Child for MockChild {
        fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
            Ok(None)
        }

        fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
            *self
                .wait_called
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = true;
            Ok(portable_pty::ExitStatus::with_exit_code(0))
        }

        fn process_id(&self) -> Option<u32> {
            None
        }
    }

    impl portable_pty::ChildKiller for MockChild {
        fn kill(&mut self) -> std::io::Result<()> {
            *self
                .kill_called
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = true;
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
            Box::new(CloneKillerStub)
        }
    }

    #[derive(Debug)]
    struct CloneKillerStub;
    impl portable_pty::ChildKiller for CloneKillerStub {
        fn kill(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
            Box::new(CloneKillerStub)
        }
    }

    type MockChildResult = (
        Box<dyn portable_pty::Child + Send + Sync>,
        Arc<Mutex<bool>>,
        Arc<Mutex<bool>>,
    );

    fn boxed_mock() -> MockChildResult {
        let (mock, kill, wait) = MockChild::new();
        (Box::new(mock), kill, wait)
    }

    // R7-P0-05: dropping an armed guard kills and background-reaps the child.
    #[test]
    fn unpublished_child_guard_kills_on_drop() {
        let (child, kill_called, _wait_called) = boxed_mock();
        let guard = UnpublishedTuiChild::new(child);
        drop(guard);
        // Give the background reap thread a moment to record.
        thread::sleep(Duration::from_millis(50));
        assert!(
            *kill_called
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
            "dropping armed guard must call kill on the child"
        );
    }

    // R7-P0-05: disarming the guard prevents kill on drop.
    #[test]
    fn unpublished_child_guard_does_not_kill_after_disarm() {
        let (child, kill_called, _wait_called) = boxed_mock();
        let guard = UnpublishedTuiChild::new(child);
        let _transferred = guard.disarm();
        // Give any background thread a moment (none should be spawned).
        thread::sleep(Duration::from_millis(50));
        assert!(
            !*kill_called
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
            "disarmed guard must not call kill"
        );
    }

    #[test]
    fn size_validation_is_bounded() {
        assert!(validate_size(DEFAULT_COLS, DEFAULT_ROWS).is_ok());
        assert!(validate_size(MIN_COLS - 1, DEFAULT_ROWS).is_err());
        assert!(validate_size(DEFAULT_COLS, MAX_ROWS + 1).is_err());
    }

    #[test]
    fn remote_size_bounds_are_tighter_than_owner_bounds() {
        // The untrusted Remote request bounds must stay inside the owner bounds
        // so a remote viewer can never widen/height beyond what the PTY owner
        // accepts, while the trusted desktop grid may exceed the remote bounds.
        assert!(validate_size(REMOTE_MAX_COLS, REMOTE_MAX_ROWS).is_ok());
        assert!(validate_size(REMOTE_MIN_COLS, REMOTE_MIN_ROWS).is_ok());
        assert!(validate_size(REMOTE_MAX_COLS + 1, REMOTE_MAX_ROWS).is_ok());
        assert!(validate_remote_size(REMOTE_MAX_COLS + 1, REMOTE_MAX_ROWS).is_err());
        assert!(validate_remote_size(REMOTE_MIN_COLS - 1, REMOTE_MIN_ROWS).is_err());
        assert!(validate_remote_size(REMOTE_MAX_COLS, REMOTE_MAX_ROWS + 1).is_err());
        assert!(validate_remote_size(DEFAULT_COLS, DEFAULT_ROWS).is_ok());
    }

    #[test]
    fn mode_and_status_use_stable_snake_case_wire_values() {
        let summary = HerdrTuiSessionSummary {
            id: "tui-1".into(),
            mode: HerdrTuiMode::Shared,
            status: HerdrTuiSessionStatus::Running,
            cols: 80,
            rows: 24,
            revision: 1,
        };
        let value = serde_json::to_value(summary)
            .unwrap_or_else(|error| panic!("summary must serialize: {error}"));
        assert_eq!(value["mode"], "shared");
        assert_eq!(value["status"], "running");
    }

    #[test]
    fn child_reap_signal_does_not_complete_before_actual_wait_notification() {
        let reap = Arc::new(ChildReapState::new());
        let waiter = Arc::clone(&reap);
        let handle = thread::spawn(move || waiter.wait(Duration::from_millis(20)));
        assert!(!handle
            .join()
            .unwrap_or_else(|_| panic!("reap waiter panicked")));
        assert!(!reap.is_done());
        reap.mark_done();
        assert!(reap.wait(Duration::from_millis(20)));
        assert!(reap.is_done());
    }

    #[test]
    fn input_queue_coalesces_ordered_burst_without_dropping_bytes() {
        let queue = TuiInputQueue::new();
        for index in 0..2_000_u64 {
            queue
                .try_push(TuiInputPacket {
                    trace_id: index + 1,
                    enqueued_us: index,
                    coalescible: true,
                    coalesced_packets: 1,
                    bytes: vec![b'x'],
                })
                .unwrap_or_else(|error| panic!("burst packet {index} rejected: {error:?}"));
        }

        let packet = queue
            .recv()
            .unwrap_or_else(|| panic!("coalesced burst packet missing"));
        assert_eq!(packet.bytes.len(), 2_000);
        assert!(packet.bytes.iter().all(|byte| *byte == b'x'));
        assert_eq!(packet.coalesced_packets, 2_000);
        assert_eq!(packet.trace_id, 2_000);
        queue.close();
        assert!(queue.recv().is_none());
    }

    #[test]
    fn input_queue_retains_a_hard_byte_budget() {
        let queue = TuiInputQueue::new();
        let chunk = vec![b'x'; INPUT_QUEUE_MAX_BYTES / 16];
        for _ in 0..16 {
            queue
                .try_push(TuiInputPacket {
                    trace_id: 0,
                    enqueued_us: 0,
                    coalescible: true,
                    coalesced_packets: 1,
                    bytes: chunk.clone(),
                })
                .unwrap_or_else(|error| panic!("budget-sized packet rejected: {error:?}"));
        }
        assert_eq!(
            queue.try_push(TuiInputPacket {
                trace_id: 0,
                enqueued_us: 0,
                coalescible: true,
                coalesced_packets: 1,
                bytes: vec![b'y'],
            }),
            Err(InputQueueError::Full)
        );
        queue.close();
        assert_eq!(
            queue.try_push(TuiInputPacket {
                trace_id: 0,
                enqueued_us: 0,
                coalescible: true,
                coalesced_packets: 1,
                bytes: vec![b'z'],
            }),
            Err(InputQueueError::Closed)
        );
    }

    #[test]
    fn input_queue_keeps_paste_and_focus_boundaries() {
        let queue = TuiInputQueue::new();
        for (bytes, coalescible) in [
            (b"a".as_slice(), true),
            (b"paste".as_slice(), false),
            (b"focus".as_slice(), false),
            (b"b".as_slice(), true),
        ] {
            queue
                .try_push(TuiInputPacket {
                    trace_id: 0,
                    enqueued_us: 0,
                    coalescible,
                    coalesced_packets: 1,
                    bytes: bytes.to_vec(),
                })
                .unwrap_or_else(|error| panic!("boundary packet rejected: {error:?}"));
        }
        assert_eq!(queue.recv().map(|packet| packet.bytes), Some(b"a".to_vec()));
        assert_eq!(
            queue.recv().map(|packet| packet.bytes),
            Some(b"paste".to_vec())
        );
        assert_eq!(
            queue.recv().map(|packet| packet.bytes),
            Some(b"focus".to_vec())
        );
        assert_eq!(queue.recv().map(|packet| packet.bytes), Some(b"b".to_vec()));
        queue.close();
    }

    /// Startup mode bytes published before any subscriber exists must still
    /// reach late viewers: the hosted Herdr TUI emits its DECSET init burst
    /// (alternate screen, SGR mouse reporting) at exec time, before
    /// `TerminalStream` can subscribe, and a broadcast channel drops those
    /// bytes for everyone. The replay prefix is what lets a fresh viewer model
    /// learn the live modes (wheel/pointer/alt-screen encoding depends on
    /// them). Runs the real `herdr` TUI child when the CLI is installed.
    #[test]
    fn startup_replay_prefix_reaches_late_subscribers_and_freezes() {
        if crate::herdr::herdr_cli_path().is_none() {
            eprintln!("skipping: herdr CLI not installed");
            return;
        }
        let manager = Arc::new(TuiManager::default());
        let session = manager
            .open(80, 24, None)
            .unwrap_or_else(|error| panic!("open shared session: {error}"));
        let marker = b"\x1b[?1049h\x1b[?1006hreplay-marker".to_vec();
        session.publish_output(marker.clone());
        let (_rx, replay) = session.subscribe_with_startup_replay();
        assert!(
            replay.ends_with(&marker),
            "replay must carry bytes published before the first subscription"
        );
        assert!(
            replay.len() <= STARTUP_REPLAY_CAP_BYTES,
            "replay prefix must stay bounded"
        );

        // The prefix freezes at first subscription: later output stays
        // live-only, and later viewers receive the identical frozen prefix.
        let (_, replay_again) = session.subscribe_with_startup_replay();
        assert_eq!(replay_again, replay);
        session.publish_output(b"post-freeze".to_vec());
        let (_, replay_third) = session.subscribe_with_startup_replay();
        assert_eq!(
            replay_third, replay,
            "post-freeze bytes must never enter the replay prefix"
        );
        manager.stop_all();
    }

    /// A01 invariant: repeated opens attach one shared child/session, restart
    /// replaces it with exactly one new generation, and stop clears the slot.
    /// Runs the real `herdr` TUI child when the CLI is installed; otherwise
    /// skips (loopback/CI environments without Herdr).
    #[test]
    fn open_is_idempotent_and_restart_replaces_the_single_child() {
        if crate::herdr::herdr_cli_path().is_none() {
            eprintln!("skipping: herdr CLI not installed");
            return;
        }
        let manager = Arc::new(TuiManager::default());
        let first = manager
            .open(90, 28, None)
            .unwrap_or_else(|error| panic!("first open: {error}"));
        let second = manager
            .open(120, 40, None)
            .unwrap_or_else(|error| panic!("second open: {error}"));
        assert_eq!(
            first.id(),
            second.id(),
            "open must reuse the running shared session instead of a second child"
        );
        // Two subscribed viewers observe the same session object.
        let _viewer_a = first.subscribe();
        let _viewer_b = first.subscribe();
        assert_eq!(first.viewer_count(), 2);

        let next = manager
            .restart(90, 28, None)
            .unwrap_or_else(|error| panic!("restart: {error}"));
        assert_ne!(
            first.id(),
            next.id(),
            "restart must replace the shared child generation"
        );
        assert!(
            !first.is_running(),
            "the replaced generation must be stopped for every viewer"
        );
        manager.stop_all();
        assert!(!next.is_running());
    }

    /// Diagnostic for the real desktop failure mode: a held key/momentum wheel
    /// can enqueue hundreds of tiny packets before the Herdr TUI writer gets a
    /// scheduling slice. The old fixed `sync_channel(128)` rejected the tail,
    /// which is observable as dropped repeat input. Keep this ignored because
    /// it launches the real Herdr child; the queue contract now requires every
    /// 2,000-byte burst packet to be admitted without backpressure.
    #[test]
    #[ignore = "local native burst diagnostic; launches the real Herdr TUI child"]
    fn shared_tui_burst_input_backpressure_smoke() {
        if crate::herdr::herdr_cli_path().is_none() {
            eprintln!("skipping: herdr CLI not installed");
            return;
        }
        let manager = Arc::new(TuiManager::default());
        let session = manager
            .open(120, 40, None)
            .unwrap_or_else(|error| panic!("open: {error}"));
        let mut accepted = 0usize;
        let mut rejected = 0usize;
        let started = Instant::now();
        for _ in 0..2_000 {
            match session.send_bytes(b"x") {
                Ok(()) => accepted += 1,
                Err(TuiError::Backpressure) => rejected += 1,
                Err(error) => panic!("unexpected burst input error: {error}"),
            }
        }
        eprintln!(
            "shared burst input: accepted={accepted} rejected={rejected} enqueue_ms={}",
            started.elapsed().as_secs_f64() * 1_000.0
        );
        assert_eq!(accepted, 2_000);
        assert_eq!(rejected, 0);
        manager.stop_all();
    }
}
