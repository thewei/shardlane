//! Hosted terminal transport primitives: the product Herdr TUI is held by the Host-owned shared
//! session (`shardlane_host::shared_tui`) as one child + one PTY; this side only attaches (with a
//! viewer-local Ghostty model); the visible auxiliary tool (Lazygit) may still own an independent PTY/model.
//!
//! [INPUT]: `crate::ghostty` (terminal model/encoding), `portable-pty` (PTY and
//!           controlling-TTY semantics), `shardlane_host::shared_tui` (the A01 shared
//!           Herdr TUI session owner: output fan-out + serial input/resize seam)
//! [OUTPUT]: `ManagedTerminal::attach_shared` (the product Herdr TUI: attach to the shared session,
//!           no spawn; on subscribe, the session's startup replay is injected into this viewer's model
//!           ahead of live events, restoring DECSET modes published during the child's startup),
//!           `ManagedTerminal::host_process` (auxiliary child processes only),
//!           `resize_local` (local reflow), `drain_frames_budgeted` (budgeted ingestion),
//!           `frame_reusing` (RAW/OSC-8 exact reuse) + `take_last_frame_plan` (the B16 row-level
//!           extraction plan), `encode_key/encode_paste/encode_mouse/encode_focus` (the shared
//!           seam marks the paste/focus ordering boundary),
//!           `is_alternate_screen`/BEL counting, host dynamic colors (`set_dynamic_colors`/
//!           `take_pending_color_queries`/`answer_color_queries`, OSC 10/11 emulator
//!           semantics), `TerminalControlInput::{Pty, Shared}`, `TerminalFrameData`,
//!           `TerminalDrainResult`
//! [POS]: The ownership layer between `shell_tui.rs` lifecycle and `main.rs` polling; after A01
//!           the product TUI's process/PTY ownership lives in shardlane-host shared_tui, and this
//!           layer only holds the viewer-local model and forwarding; the old Embedded per-Pane controller
//!           JSON protocol, projection hold, and deep-history reseeding have all been deleted

use crate::ghostty::{
    GhosttyRuntime, GhosttyTerminal, TerminalFrame, TerminalFramePlan, TerminalGridSelection,
    TerminalKey, TerminalModifiers, TerminalMouseAction, TerminalMouseButton,
    TerminalMouseGeometry,
};
use crate::terminal_trace;
use async_channel::{Receiver as WakeReceiver, Sender as WakeSender};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::{
    io::Read,
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, TryRecvError},
        Arc, Mutex,
    },
    thread,
    time::Instant,
};

/// Decoded terminal frame bytes ready to feed into libghostty-vt.
pub struct TerminalFrameData {
    pub ansi_bytes: Vec<u8>,
}

/// Bound each viewer's decoded-frame backlog. A PTY reader/forwarder may block
/// behind a slow GPUI drain, but it must never allocate an unbounded queue while
/// a TUI is producing output faster than the renderer can consume it.
const FRAME_QUEUE_CAPACITY: usize = 128;

pub type TerminalWakeReceiver = WakeReceiver<()>;
/// Cloneable sender side of the terminal poll wake channel: UI paths (e.g. a pended
/// copy-on-select) can wake the poll loop immediately instead of waiting out the
/// adaptive backoff timer.
pub type TerminalWakeSender = WakeSender<()>;

fn terminal_wake_channel() -> (WakeSender<()>, TerminalWakeReceiver) {
    async_channel::bounded(1)
}

fn signal_terminal_wake(wake: &WakeSender<()>) {
    let result = wake.try_send(());
    if terminal_trace::enabled() {
        terminal_trace::event(format_args!(
            "stage=poll.wake_signal outcome={}",
            if result.is_ok() {
                "queued"
            } else {
                "coalesced"
            },
        ));
    }
}

#[derive(Default)]
struct PtyTraceState {
    last_input_id: AtomicU64,
    last_write_us: AtomicU64,
    last_read_id: AtomicU64,
    last_read_us: AtomicU64,
    pending_writes: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TerminalIoTraceSnapshot {
    pub last_input_id: u64,
    pub last_write_us: u64,
    pub last_read_id: u64,
    pub last_read_us: u64,
    pub pending_writes: u64,
}

impl PtyTraceState {
    fn snapshot(&self) -> TerminalIoTraceSnapshot {
        TerminalIoTraceSnapshot {
            last_input_id: self.last_input_id.load(Ordering::Relaxed),
            last_write_us: self.last_write_us.load(Ordering::Relaxed),
            last_read_id: self.last_read_id.load(Ordering::Relaxed),
            last_read_us: self.last_read_us.load(Ordering::Relaxed),
            pending_writes: self.pending_writes.load(Ordering::Relaxed),
        }
    }
}

struct PtyWritePacket {
    trace_id: u64,
    kind: &'static str,
    enqueued_us: u64,
    bytes: Vec<u8>,
}

/// Cloneable input seam for the Host-owned shared Herdr TUI session. Writes
/// and resizes are serialized by the session's own writer thread / PTY lock;
/// dropping this handle only unsubscribes input, never the shared child.
#[derive(Clone)]
pub struct SharedTuiHandle {
    session: Arc<shardlane_host::shared_tui::HerdrTuiSession>,
}

impl SharedTuiHandle {
    fn send_bytes_traced(&self, bytes: &[u8], kind: &'static str) -> Result<u64, String> {
        let trace_id = terminal_trace::next_packet_id();
        let started_at = (trace_id != 0).then(Instant::now);
        let coalescible = !matches!(kind, "paste" | "focus");
        let result = self
            .session
            .send_bytes_with_trace_kind(bytes, trace_id, coalescible)
            .map_err(|error| error.to_string());
        if let Some(started_at) = started_at {
            terminal_trace::event(format_args!(
                "stage=shared.enqueue input_id={trace_id} kind={kind} bytes={} send_us={} outcome={}",
                bytes.len(),
                terminal_trace::elapsed_us(started_at),
                if result.is_ok() { "queued" } else { "rejected" },
            ));
        }
        result.map(|_| trace_id)
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.session
            .resize(cols, rows)
            .map_err(|error| error.to_string())
            .map(|_| ())
    }
}

/// Cloneable hosted-PTY input handle for the primary Herdr or bounded auxiliary child.
#[derive(Clone)]
pub enum TerminalControlInput {
    /// PTY host: raw byte writes to the master fd + TIOCSWINSZ/SIGWINCH resize.
    Pty(PtyHandle),
    /// Host-owned shared Herdr TUI session (A01): input/resize go through the shared session's
    /// serial seam; this side does not own the child process.
    Shared(SharedTuiHandle),
}

/// Cloneable hosted-PTY I/O. `portable-pty` owns the platform-specific PTY/session
/// semantics (including controlling-TTY setup); Shardlane only serializes writes/resizes.
#[derive(Clone)]
pub struct PtyHandle {
    /// Dedicated ordered PTY writer. UI/input paths enqueue bytes and never block on the
    /// kernel PTY write itself; one writer thread preserves terminal byte ordering.
    writer: mpsc::Sender<PtyWritePacket>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    trace: Arc<PtyTraceState>,
}

impl PtyHandle {
    fn send_bytes_traced(&self, bytes: &[u8], kind: &'static str) -> Result<u64, String> {
        let trace_id = terminal_trace::next_packet_id();
        let enqueued_us = if trace_id == 0 {
            0
        } else {
            terminal_trace::now_us()
        };
        if trace_id != 0 {
            let pending = self.trace.pending_writes.fetch_add(1, Ordering::Relaxed) + 1;
            terminal_trace::event(format_args!(
                "stage=pty.enqueue input_id={trace_id} kind={kind} bytes={} pending={pending}",
                bytes.len()
            ));
        }
        self.writer
            .send(PtyWritePacket {
                trace_id,
                kind,
                enqueued_us,
                bytes: bytes.to_vec(),
            })
            .map_err(|_| {
                if trace_id != 0 {
                    self.trace.pending_writes.fetch_sub(1, Ordering::Relaxed);
                }
                "hosted pty writer stopped".to_string()
            })?;
        Ok(trace_id)
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        let master = self.master.lock().map_err(|error| error.to_string())?;
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("pty resize failed: {error}"))
    }
}

impl TerminalControlInput {
    pub fn send_text(&self, text: &str) -> Result<(), String> {
        self.send_text_traced(text, "text").map(|_| ())
    }

    pub fn send_text_traced(&self, text: &str, kind: &'static str) -> Result<u64, String> {
        match self {
            Self::Pty(handle) => handle.send_bytes_traced(text.as_bytes(), kind),
            Self::Shared(handle) => handle.send_bytes_traced(text.as_bytes(), kind),
        }
    }

    pub fn send_bytes(&self, bytes: &[u8]) -> Result<(), String> {
        self.send_bytes_traced(bytes, "raw").map(|_| ())
    }

    pub fn send_bytes_traced(&self, bytes: &[u8], kind: &'static str) -> Result<u64, String> {
        match self {
            Self::Pty(handle) => handle.send_bytes_traced(bytes, kind),
            Self::Shared(handle) => handle.send_bytes_traced(bytes, kind),
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        match self {
            Self::Pty(handle) => handle.resize(cols, rows),
            Self::Shared(handle) => handle.resize(cols, rows),
        }
    }
}

/// Manages a structured Herdr terminal control stream for the primary host or a
/// bounded hosted auxiliary PTY child (for example Lazygit).
pub struct TerminalStream {
    imp: StreamImp,
    pub frames: Receiver<TerminalFrameData>,
    wake: TerminalWakeReceiver,
    /// Clone of the wake channel's sender held next to the receiver so UI paths can
    /// request an immediate poll pass.
    wake_tx: TerminalWakeSender,
    input: TerminalControlInput,
    trace: Arc<PtyTraceState>,
}

enum StreamImp {
    Pty(Option<Box<dyn portable_pty::Child + Send + Sync>>),
    /// Attached viewer of the Host-owned shared Herdr TUI session (A01).
    /// Dropping unsubscribes the forwarder; the shared child's lifecycle
    /// belongs to `shardlane_host::shared_tui::TuiManager`.
    Shared {
        stop: Arc<std::sync::atomic::AtomicBool>,
        session: Arc<shardlane_host::shared_tui::HerdrTuiSession>,
    },
}

/// Forward one shared-session subscription into the viewer-local frame queue.
///
/// The shared PTY reader already publishes an event as soon as bytes arrive.
/// Waiting on `blocking_recv` preserves that edge-triggered wake and avoids a
/// second software frame clock in front of the normal GPUI poll loop. `Wake` is
/// an internal teardown event: only the viewer whose stop flag is set exits;
/// other subscribers continue to observe the same session.
fn forward_shared_events<F>(
    mut events: tokio::sync::broadcast::Receiver<shardlane_host::shared_tui::TuiEvent>,
    tx: mpsc::SyncSender<TerminalFrameData>,
    wake_tx: WakeSender<()>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    trace_state: Arc<PtyTraceState>,
    force_redraw: F,
) where
    F: Fn() + Send + 'static,
{
    'outer: loop {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        match events.blocking_recv() {
            Ok(shardlane_host::shared_tui::TuiEvent::Output {
                bytes,
                published_at,
                ..
            }) => {
                if terminal_trace::enabled() {
                    let read_id = terminal_trace::next_read_id();
                    let read_us = terminal_trace::now_us();
                    trace_state.last_read_id.store(read_id, Ordering::Relaxed);
                    trace_state.last_read_us.store(read_us, Ordering::Relaxed);
                    terminal_trace::event(format_args!(
                        "stage=shared.forward bytes={} read_id={read_id} publish_to_forward_us={}",
                        bytes.len(),
                        u64::try_from(published_at.elapsed().as_micros()).unwrap_or(u64::MAX),
                    ));
                }
                if tx.send(TerminalFrameData { ansi_bytes: bytes }).is_err() {
                    break;
                }
                // One bounded wake is enough for the GPUI poll loop to drain
                // every queued output chunk; do not create one timer per chunk.
                signal_terminal_wake(&wake_tx);
            }
            Ok(shardlane_host::shared_tui::TuiEvent::Status { summary }) => {
                use shardlane_host::HerdrTuiSessionStatus;
                if matches!(
                    summary.status,
                    HerdrTuiSessionStatus::Stopped | HerdrTuiSessionStatus::Failed
                ) {
                    // The shared child ended: surface it as a stream disconnect
                    // so the existing poll loop applies its cooldown/restart state.
                    break 'outer;
                }
            }
            Ok(shardlane_host::shared_tui::TuiEvent::Wake) => {
                // A Wake is only a transport/lifecycle hint. A live viewer must
                // not turn it into a repaint; a dropped viewer exits at the top
                // of the loop once its own stop flag is observed.
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                // Missed output bytes cannot be reconstructed; ask the shared
                // child for a full repaint, matching the previous behavior.
                force_redraw();
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break 'outer,
        }
    }
    drop(tx);
    signal_terminal_wake(&wake_tx);
}

impl TerminalStream {
    /// Host one terminal program (for example the embedded `herdr` client) on a
    /// real platform PTY. `portable-pty` is the terminal-process owner: on Unix it
    /// establishes a new session and controlling TTY before exec, matching a normal
    /// interactive terminal launch rather than Shardlane maintaining a partial PTY shim.
    pub fn spawn_pty(command: &Command, cols: u16, rows: u16) -> Result<Self, String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("open hosted pty: {error}"))?;

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

        let mut child = pair
            .slave
            .spawn_command(builder)
            .map_err(|error| format!("spawn hosted pty child: {error}"))?;
        drop(pair.slave);
        // `portable-pty` children are not killed on drop: if reader/writer setup fails
        // after a successful spawn, kill + reap the child here so the error path cannot
        // leak a zombie process (audit B03).
        let mut reader = match pair.master.try_clone_reader() {
            Ok(reader) => reader,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("clone hosted pty reader: {error}"));
            }
        };
        let mut writer = match pair.master.take_writer() {
            Ok(writer) => writer,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("take hosted pty writer: {error}"));
            }
        };
        let master = Arc::new(Mutex::new(pair.master));
        let trace = Arc::new(PtyTraceState::default());
        let writer_trace = trace.clone();
        let (writer_tx, writer_rx) = mpsc::channel::<PtyWritePacket>();
        thread::spawn(move || {
            while let Ok(packet) = writer_rx.recv() {
                let dequeue_us = if packet.trace_id == 0 {
                    0
                } else {
                    terminal_trace::now_us()
                };
                let write_started = Instant::now();
                let write_result = writer.write_all(&packet.bytes);
                let write_us = terminal_trace::elapsed_us(write_started);
                let flush_started = Instant::now();
                let flush_result = if write_result.is_ok() {
                    writer.flush()
                } else {
                    Ok(())
                };
                let flush_us = terminal_trace::elapsed_us(flush_started);
                if packet.trace_id != 0 {
                    writer_trace.pending_writes.fetch_sub(1, Ordering::Relaxed);
                    if write_result.is_ok() && flush_result.is_ok() {
                        let written_us = terminal_trace::now_us();
                        writer_trace
                            .last_input_id
                            .store(packet.trace_id, Ordering::Relaxed);
                        writer_trace
                            .last_write_us
                            .store(written_us, Ordering::Relaxed);
                    }
                    terminal_trace::event(format_args!(
                        "stage=pty.write input_id={} kind={} bytes={} queue_us={} write_us={} flush_us={} pending={}",
                        packet.trace_id,
                        packet.kind,
                        packet.bytes.len(),
                        dequeue_us.saturating_sub(packet.enqueued_us),
                        write_us,
                        flush_us,
                        writer_trace.pending_writes.load(Ordering::Relaxed),
                    ));
                }
                if write_result.is_err() || flush_result.is_err() {
                    break;
                }
            }
        });
        let input = TerminalControlInput::Pty(PtyHandle {
            writer: writer_tx,
            master,
            trace: trace.clone(),
        });

        let (tx, rx) = mpsc::sync_channel(FRAME_QUEUE_CAPACITY);
        let (wake_tx, wake_rx) = terminal_wake_channel();
        let stream_wake_tx = wake_tx.clone();
        let reader_trace = trace.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 65_536];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(read) => {
                        if terminal_trace::enabled() {
                            let read_id = terminal_trace::next_read_id();
                            let read_us = terminal_trace::now_us();
                            reader_trace.last_read_id.store(read_id, Ordering::Relaxed);
                            reader_trace.last_read_us.store(read_us, Ordering::Relaxed);
                            let input_id = reader_trace.last_input_id.load(Ordering::Relaxed);
                            let last_write_us = reader_trace.last_write_us.load(Ordering::Relaxed);
                            terminal_trace::event(format_args!(
                                "stage=pty.read read_id={read_id} bytes={read} input_id={input_id} since_write_us={}",
                                read_us.saturating_sub(last_write_us),
                            ));
                        }
                        if tx
                            .send(TerminalFrameData {
                                ansi_bytes: buf[..read].to_vec(),
                            })
                            .is_err()
                        {
                            break;
                        }
                        signal_terminal_wake(&wake_tx);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            signal_terminal_wake(&wake_tx);
        });

        Ok(Self {
            imp: StreamImp::Pty(Some(child)),
            frames: rx,
            wake: wake_rx,
            wake_tx: stream_wake_tx,
            input,
            trace,
        })
    }

    /// Attach this surface as a viewer of the Host-owned shared Herdr TUI
    /// session (A01). No child is spawned here: the shared session owns the
    /// single PTY/child; a bounded forwarder thread bridges the session's
    /// output fan-out into this stream's frames channel + wake signal, and
    /// input/resize go back through the session's serialized seams.
    pub fn attach_shared(session: &Arc<shardlane_host::shared_tui::HerdrTuiSession>) -> Self {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(FRAME_QUEUE_CAPACITY);
        let (wake_tx, wake_rx) = terminal_wake_channel();
        let trace = Arc::new(PtyTraceState::default());
        let (events, startup_replay) = session.subscribe_with_startup_replay();
        let forwarder_wake_tx = wake_tx.clone();
        let forwarder_stop = stop.clone();
        let forwarder_session = session.clone();
        let forwarder_trace = trace.clone();
        let replay_tx = tx.clone();
        thread::spawn(move || {
            // The shared child published its startup mode sequences (alternate
            // screen, SGR mouse reporting, bracketed paste) before any viewer
            // could subscribe. Apply that frozen prefix to this viewer's fresh
            // model first; otherwise the model permanently believes those
            // modes are off and wheel/pointer encoding silently degrades.
            if !startup_replay.is_empty()
                && replay_tx
                    .send(TerminalFrameData {
                        ansi_bytes: startup_replay,
                    })
                    .is_ok()
            {
                signal_terminal_wake(&forwarder_wake_tx);
            }
            forward_shared_events(
                events,
                tx,
                forwarder_wake_tx,
                forwarder_stop,
                forwarder_trace,
                move || {
                    let _ = forwarder_session.force_redraw();
                },
            );
        });
        Self {
            imp: StreamImp::Shared {
                stop,
                session: session.clone(),
            },
            frames: rx,
            wake: wake_rx,
            wake_tx,
            input: TerminalControlInput::Shared(SharedTuiHandle {
                session: session.clone(),
            }),
            trace,
        }
    }

    pub fn input_handle(&self) -> TerminalControlInput {
        self.input.clone()
    }

    pub fn wake_receiver(&self) -> TerminalWakeReceiver {
        self.wake.clone()
    }

    /// Sender side of this stream's poll wake channel (audit B20): UI paths can wake the
    /// poll loop immediately, without waiting out the adaptive backoff timer.
    pub fn wake_sender(&self) -> TerminalWakeSender {
        self.wake_tx.clone()
    }

    /// Reader handle to the Host-owned shared Herdr TUI session (None for a
    /// privately hosted PTY child).
    pub(crate) fn shared_session(
        &self,
    ) -> Option<std::sync::Arc<shardlane_host::shared_tui::HerdrTuiSession>> {
        match &self.imp {
            StreamImp::Shared { session, .. } => Some(session.clone()),
            StreamImp::Pty(_) => None,
        }
    }

    fn trace_snapshot(&self) -> TerminalIoTraceSnapshot {
        self.trace.snapshot()
    }
}

impl Drop for TerminalStream {
    fn drop(&mut self) {
        match &mut self.imp {
            StreamImp::Pty(child) => {
                let Some(mut child) = child.take() else {
                    return;
                };
                // `portable-pty` owns platform termination semantics. Signal before
                // handing wait to a background thread so app/Crepus teardown cannot
                // orphan the hosted Herdr client.
                let _ = child.kill();
                thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            StreamImp::Shared { stop, session } => {
                // Unsubscribe only: the shared Herdr TUI child belongs to the
                // Host-level TuiManager and may still be observed by Remote
                // viewers; its idle reaper performs the final stop.
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                // `forward_shared_events` is blocked in `blocking_recv` when
                // the session is idle. Wake all subscribers so this viewer's
                // stop flag is observed immediately, without a polling timer.
                session.wake_subscribers();
            }
        }
    }
}

/// The result of a single drain: whether a frame was consumed, whether a disconnect was observed,
/// whether a full authoritative frame arrived, and the number of BELs observed during this round of
/// live ingestion (consumed by the user feedback layer).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalDrainResult {
    pub consumed: bool,
    pub disconnected: bool,
    /// Number of BELs (0x07 bells) observed during this round of live ingestion.
    pub bells: u64,
    /// PTY bytes and reader chunk counts fed into libghostty-vt this round; used for budget and trace.
    pub bytes: usize,
    pub chunks: usize,
}

/// F48: a receive loop with a byte budget: stop when the budget is exhausted, leaving remaining
/// frames in the channel for the next iteration (progress guarantee: at least one frame is
/// consumed, regardless of whether max_bytes is 0).
fn drain_frame_receiver_budgeted(
    frames: &Receiver<TerminalFrameData>,
    mut consume: impl FnMut(TerminalFrameData),
    max_bytes: usize,
) -> TerminalDrainResult {
    let mut result = TerminalDrainResult::default();
    let mut consumed_bytes = 0usize;
    loop {
        if consumed_bytes > 0 && consumed_bytes >= max_bytes {
            break;
        }
        match frames.try_recv() {
            Ok(frame_data) => {
                let frame_bytes = frame_data.ansi_bytes.len();
                consumed_bytes += frame_bytes;
                result.bytes = result.bytes.saturating_add(frame_bytes);
                result.chunks = result.chunks.saturating_add(1);
                consume(frame_data);
                result.consumed = true;
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                result.disconnected = true;
                break;
            }
        }
    }
    result
}

/// Manages a terminal stream + libghostty-vt model for one hosted surface: the
/// primary Herdr TUI or one bounded auxiliary PTY child.
pub struct ManagedTerminal {
    stream: TerminalStream,
    terminal: GhosttyTerminal,
    cols: u16,
    rows: u16,
    pixel_width: u16,
    pixel_height: u16,
}

impl ManagedTerminal {
    /// Host a terminal program (like the `herdr` TUI) on a local PTY: byte round-trips are isomorphic
    /// to the controller path (frames channel + wake + budgeted ingestion), but the local Ghostty
    /// model is the authority. Only for bounded auxiliary children (Lazygit); the product Herdr TUI
    /// must go through `attach_shared` to consume the Host-owned shared session (the A01 single-child invariant).
    pub fn host_process(
        command: &Command,
        cols: u16,
        rows: u16,
        max_scrollback: usize,
    ) -> Result<Self, String> {
        let runtime = GhosttyRuntime::detect()?;
        let api = runtime.load_api()?;
        let terminal = GhosttyTerminal::new_with_scrollback(api, cols, rows, max_scrollback)?;
        let stream = TerminalStream::spawn_pty(command, cols, rows)?;
        Ok(Self {
            stream,
            terminal,
            cols,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        })
    }

    /// Attach the primary Herdr TUI surface to the Host-owned shared session
    /// (A01): one child + one PTY process-wide; this surface keeps only its
    /// viewer-local Ghostty model/viewport. The shared session's authoritative
    /// geometry is adopted via the caller's resize before first paint.
    pub fn attach_shared(
        session: &Arc<shardlane_host::shared_tui::HerdrTuiSession>,
        cols: u16,
        rows: u16,
        max_scrollback: usize,
    ) -> Result<Self, String> {
        let runtime = GhosttyRuntime::detect()?;
        let api = runtime.load_api()?;
        let terminal = GhosttyTerminal::new_with_scrollback(api, cols, rows, max_scrollback)?;
        let stream = TerminalStream::attach_shared(session);
        Ok(Self {
            stream,
            terminal,
            cols,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        })
    }

    /// F48: budgeted ingestion. VT parsing happens on the UI thread (O(bytes)); when an Agent dumps
    /// several MB at once, a single poll iteration cannot digest it all: stop when the budget is
    /// exhausted, leaving remaining frames in the channel for the next iteration (with a backlog,
    /// consumed=true keeps the active polling cadence).
    /// Consuming at least one frame guarantees progress without relying on the caller to ensure max_bytes > 0.
    pub fn drain_frames_budgeted(&mut self, max_bytes: usize) -> TerminalDrainResult {
        let trace_started = Instant::now();
        let frames = &self.stream.frames;
        let terminal = &mut self.terminal;
        let mut bells = 0_u64;
        let mut result = drain_frame_receiver_budgeted(
            frames,
            |frame_data| {
                terminal.write(&frame_data.ansi_bytes);
                bells += terminal.take_pending_bells();
            },
            max_bytes,
        );
        result.bells = bells;
        if result.consumed && terminal_trace::enabled() {
            let snapshot = self.stream.trace_snapshot();
            let now_us = terminal_trace::now_us();
            terminal_trace::event(format_args!(
                "stage=vt.drain bytes={} chunks={} elapsed_us={} read_id={} input_id={} read_to_drain_us={} pending_writes={}",
                result.bytes,
                result.chunks,
                terminal_trace::elapsed_us(trace_started),
                snapshot.last_read_id,
                snapshot.last_input_id,
                now_us.saturating_sub(snapshot.last_read_us),
                snapshot.pending_writes,
            ));
        }
        result
    }

    /// Extract the current frame; while the OSC-8 semantic stream is unchanged, reuse the previous
    /// frame's link spans for unchanged rows, skipping per-frame per-cell FFI probing after latching.
    pub fn frame_reusing(
        &mut self,
        previous: Option<&TerminalFrame>,
    ) -> Result<TerminalFrame, String> {
        self.terminal.frame_reusing(previous)
    }

    /// Row-level change plan of the most recent extraction (B16) — see
    /// `GhosttyTerminal::take_last_frame_plan`. Callers pair it with the frame returned by
    /// `frame_reusing` to skip deep grid comparisons.
    pub fn take_last_frame_plan(&mut self) -> TerminalFramePlan {
        self.terminal.take_last_frame_plan()
    }

    pub fn input_handle(&self) -> TerminalControlInput {
        self.stream.input_handle()
    }

    pub fn key_encoder_handle(&self) -> Arc<Mutex<crate::ghostty::GhosttyKeyEncoderState>> {
        self.terminal.key_encoder_handle()
    }

    pub fn wake_receiver(&self) -> TerminalWakeReceiver {
        self.stream.wake_receiver()
    }

    /// Sender side of the poll wake channel (see `TerminalStream::wake_sender`).
    pub fn wake_sender(&self) -> TerminalWakeSender {
        self.stream.wake_sender()
    }

    pub fn io_trace_snapshot(&self) -> TerminalIoTraceSnapshot {
        self.stream.trace_snapshot()
    }

    /// Local viewer-model resize (immediate): reflows the Ghostty model to the compensated
    /// transport grid, records the pixel metrics, and returns the reflowed frame. There is no
    /// hold/reseed step — the caller issues the shared session's PTY resize in the same
    /// critical section (`resize_main_terminal_to_size`), and the Herdr TUI repaints
    /// naturally via its own PTY output.
    pub fn resize_local(
        &mut self,
        cols: u16,
        rows: u16,
        pw: u16,
        ph: u16,
    ) -> Result<TerminalFrame, String> {
        self.terminal.resize(cols, rows, pw, ph)?;
        self.cols = cols;
        self.rows = rows;
        self.pixel_width = pw;
        self.pixel_height = ph;
        self.terminal.frame()
    }

    /// The shared session's authoritative grid (`None` for a privately hosted PTY).
    pub fn shared_geometry(&self) -> Option<(u16, u16)> {
        self.stream.shared_session().map(|session| {
            let summary = session.summary();
            (summary.cols, summary.rows)
        })
    }

    /// Adopt the shared session's authoritative grid when another viewer resized
    /// it. Resize applies globally to the one shared child, and that child only
    /// repaints its own grid — a viewer left on a larger local model keeps stale
    /// glyphs beyond the child's width and projects them into the visible pane as
    /// residue. Returns `Some((reflow frame, cols, rows))` when the geometry
    /// actually changed.
    pub fn adopt_shared_geometry(
        &mut self,
        cell_width: f64,
        cell_height: f64,
    ) -> Result<Option<(TerminalFrame, u16, u16)>, String> {
        let Some((cols, rows)) = self.shared_geometry() else {
            return Ok(None);
        };
        if (cols, rows) == (self.cols, self.rows) {
            return Ok(None);
        }
        let clamp_px = |cells: u16, cell: f64| -> u16 {
            (f64::from(cells) * cell)
                .round()
                .clamp(1.0, f64::from(u16::MAX)) as u16
        };
        let pixel_width = clamp_px(cols, cell_width);
        let pixel_height = clamp_px(rows, cell_height);
        self.terminal
            .resize(cols, rows, pixel_width, pixel_height)?;
        self.cols = cols;
        self.rows = rows;
        self.pixel_width = pixel_width;
        self.pixel_height = pixel_height;
        Ok(Some((self.terminal.frame()?, cols, rows)))
    }

    /// Write VT bytes directly to the ghostty model (for pending data).
    pub fn write_bytes(&mut self, data: &[u8]) {
        self.terminal.write(data);
    }

    /// Seed the hosted terminal emulator's dynamic default foreground/background
    /// (OSC 10/11 into the local model). Shardlane is the emulator for the hosted
    /// Herdr TUI child; colors derive from the active Herdr theme.
    pub fn set_dynamic_colors(&mut self, foreground: u32, background: u32) {
        self.terminal.set_dynamic_colors(foreground, background);
    }

    /// Consume the OSC 10/11 color-query bits observed in the child's output.
    pub fn take_pending_color_queries(&mut self) -> u8 {
        self.terminal.take_pending_color_queries()
    }

    /// Answer the child's dynamic-color queries on the PTY input channel, the way a
    /// native terminal emulator reports its configured colors.
    pub fn answer_color_queries(
        &self,
        queries: u8,
        foreground: u32,
        background: u32,
    ) -> Result<(), String> {
        let bytes = crate::ghostty::color_query_report_bytes(queries, foreground, background);
        if bytes.is_empty() {
            return Ok(());
        }
        self.stream
            .input_handle()
            .send_bytes_traced(&bytes, "osc_color_report")
            .map(|_| ())
    }

    /// Whether the terminal is on the alternate screen (less/vim-like TUIs); used by the wheel translation decision.
    pub fn is_alternate_screen(&mut self) -> bool {
        self.terminal.is_alternate_screen()
    }

    /// Number of BELs observed in the live VT stream since the last call; reset to zero on read.
    pub fn take_terminal_bells(&mut self) -> u64 {
        self.terminal.take_pending_bells()
    }

    pub fn selection_text(&mut self, start: (u16, u16), end: (u16, u16)) -> Result<String, String> {
        self.terminal.selection_text(start, end)
    }

    pub fn select_word_at(
        &self,
        point: (u16, u16),
    ) -> Result<Option<TerminalGridSelection>, String> {
        self.terminal.select_word_at(point)
    }

    pub fn select_line_at(
        &self,
        point: (u16, u16),
    ) -> Result<Option<TerminalGridSelection>, String> {
        self.terminal.select_line_at(point)
    }

    pub fn select_word_drag(
        &self,
        anchor: (u16, u16),
        current: (u16, u16),
    ) -> Result<Option<TerminalGridSelection>, String> {
        self.terminal.select_word_drag(anchor, current)
    }

    pub fn select_line_drag(
        &self,
        anchor: (u16, u16),
        current: (u16, u16),
    ) -> Result<Option<TerminalGridSelection>, String> {
        self.terminal.select_line_drag(anchor, current)
    }

    pub fn encode_focus(&mut self, focused: bool) -> Result<Vec<u8>, String> {
        self.terminal.encode_focus(focused)
    }

    pub fn encode_paste(&mut self, text: &str) -> Result<Vec<u8>, String> {
        self.terminal.encode_paste(text)
    }

    pub fn encode_key(
        &mut self,
        key: TerminalKey,
        modifiers: TerminalModifiers,
        unshifted_codepoint: u32,
    ) -> Result<Vec<u8>, String> {
        self.terminal
            .encode_key(key, modifiers, None, unshifted_codepoint)
    }

    pub fn encode_mouse(
        &mut self,
        action: TerminalMouseAction,
        button: Option<TerminalMouseButton>,
        modifiers: TerminalModifiers,
        position: (f32, f32),
        geometry: TerminalMouseGeometry,
        any_button_pressed: bool,
    ) -> Result<Vec<u8>, String> {
        self.terminal.encode_mouse(
            action,
            button,
            modifiers,
            position,
            geometry,
            any_button_pressed,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn drain_observes_disconnect_without_consuming_a_status_probe_frame() {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(TerminalFrameData {
                ansi_bytes: b"first".to_vec(),
            })
            .unwrap_or_else(|error| panic!("{error}"));
        sender
            .send(TerminalFrameData {
                ansi_bytes: b"second".to_vec(),
            })
            .unwrap_or_else(|error| panic!("{error}"));
        drop(sender);

        let mut frames = Vec::new();
        let result = drain_frame_receiver_budgeted(
            &receiver,
            |frame| frames.push(frame.ansi_bytes),
            usize::MAX,
        );

        assert_eq!(frames, vec![b"first".to_vec(), b"second".to_vec()]);
        assert_eq!(
            result,
            TerminalDrainResult {
                consumed: true,
                disconnected: true,
                bells: 0,
                bytes: 11,
                chunks: 2,
            }
        );
    }

    #[test]
    fn budgeted_drain_leaves_backlog_in_channel_and_makes_progress() {
        // F48: three frames, budget 5: the first round consumes only "first" (exactly hitting the budget), the rest stay in the channel.
        let (sender, receiver) = mpsc::channel();
        for text in ["first", "second", "third"] {
            sender
                .send(TerminalFrameData {
                    ansi_bytes: text.as_bytes().to_vec(),
                })
                .unwrap_or_else(|error| panic!("{error}"));
        }

        let mut drained = Vec::new();
        let result =
            drain_frame_receiver_budgeted(&receiver, |frame| drained.push(frame.ansi_bytes), 5);
        assert_eq!(drained, vec![b"first".to_vec()]);
        assert!(result.consumed && !result.disconnected);

        // Second round: "second" (6B) hits the budget and stops; "third" is left for the next round.
        drained.clear();
        let result =
            drain_frame_receiver_budgeted(&receiver, |frame| drained.push(frame.ansi_bytes), 5);
        assert_eq!(drained, vec![b"second".to_vec()]);
        assert!(result.consumed && !result.disconnected);

        // Third round clears the backlog.
        drained.clear();
        let result =
            drain_frame_receiver_budgeted(&receiver, |frame| drained.push(frame.ansi_bytes), 5);
        assert_eq!(drained, vec![b"third".to_vec()]);
        assert!(result.consumed && !result.disconnected);

        // Fourth round: channel empty + disconnect → consumes nothing but reports disconnected.
        drained.clear();
        drop(sender);
        let result =
            drain_frame_receiver_budgeted(&receiver, |frame| drained.push(frame.ansi_bytes), 5);
        assert!(drained.is_empty());
        assert!(result.disconnected && !result.consumed);
    }

    #[test]
    fn terminal_wake_signal_coalesces_until_consumer_drains_it() {
        let (sender, receiver) = terminal_wake_channel();
        signal_terminal_wake(&sender);
        signal_terminal_wake(&sender);

        assert!(receiver.try_recv().is_ok());
        assert!(matches!(
            receiver.try_recv(),
            Err(async_channel::TryRecvError::Empty)
        ));
    }

    #[test]
    fn viewer_frame_queue_has_a_hard_capacity() {
        let (sender, receiver) = mpsc::sync_channel(FRAME_QUEUE_CAPACITY);
        for _ in 0..FRAME_QUEUE_CAPACITY {
            sender
                .try_send(TerminalFrameData {
                    ansi_bytes: vec![b'x'],
                })
                .unwrap_or_else(|error| panic!("frame queue filled too early: {error}"));
        }
        assert!(matches!(
            sender.try_send(TerminalFrameData {
                ansi_bytes: vec![b'x'],
            }),
            Err(mpsc::TrySendError::Full(_))
        ));
        drop(receiver);
    }

    #[test]
    fn shared_event_forwarder_ignores_lifecycle_wakes_and_stops_on_its_own_wake() {
        let (event_sender, event_receiver) = tokio::sync::broadcast::channel(8);
        let (frame_sender, frame_receiver) = mpsc::sync_channel(FRAME_QUEUE_CAPACITY);
        let (wake_sender, _wake_receiver) = terminal_wake_channel();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stop = stop.clone();
        let forwarder = thread::spawn(move || {
            forward_shared_events(
                event_receiver,
                frame_sender,
                wake_sender,
                thread_stop,
                Arc::new(PtyTraceState::default()),
                || {},
            );
        });

        // A transport-only wake must not manufacture a terminal frame for a
        // live viewer. The following output must still arrive in order.
        event_sender
            .send(shardlane_host::shared_tui::TuiEvent::Wake)
            .unwrap_or_else(|error| panic!("wake send failed: {error}"));
        event_sender
            .send(shardlane_host::shared_tui::TuiEvent::Output {
                revision: 1,
                bytes: b"echo".to_vec(),
                published_at: Instant::now(),
            })
            .unwrap_or_else(|error| panic!("output send failed: {error}"));
        let frame = frame_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or_else(|error| panic!("forwarder did not deliver output: {error}"));
        assert_eq!(frame.ansi_bytes, b"echo");
        assert!(matches!(
            frame_receiver.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));

        // Teardown sets this viewer's flag and sends a wake. The blocking
        // receiver must observe it and let the thread terminate promptly.
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        event_sender
            .send(shardlane_host::shared_tui::TuiEvent::Wake)
            .unwrap_or_else(|error| panic!("stop wake send failed: {error}"));
        forwarder
            .join()
            .unwrap_or_else(|_| panic!("forwarder thread panicked"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "local real-device diagnostic; compares shared fan-out wake latency"]
    fn shared_event_forwarder_repeat_input_latency_smoke() {
        let (event_sender, event_receiver) = tokio::sync::broadcast::channel(256);
        let (frame_sender, frame_receiver) = mpsc::sync_channel(FRAME_QUEUE_CAPACITY);
        let (wake_sender, _wake_receiver) = terminal_wake_channel();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_stop = stop.clone();
        let forwarder = thread::spawn(move || {
            forward_shared_events(
                event_receiver,
                frame_sender,
                wake_sender,
                thread_stop,
                Arc::new(PtyTraceState::default()),
                || {},
            );
        });

        // Warm the forwarder into its blocking edge with one throwaway event; readiness
        // is inferred from that first frame (the old ready-channel parameter is gone).
        event_sender
            .send(shardlane_host::shared_tui::TuiEvent::Output {
                revision: 0,
                bytes: b"warm".to_vec(),
                published_at: Instant::now(),
            })
            .unwrap_or_else(|error| panic!("warmup send failed: {error}"));
        let warm = frame_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or_else(|error| panic!("forwarder readiness timeout: {error}"));
        assert_eq!(warm.ansi_bytes, b"warm");
        let mut latency_us = Vec::with_capacity(120);
        for revision in 1..=120_u64 {
            let sent = Instant::now();
            event_sender
                .send(shardlane_host::shared_tui::TuiEvent::Output {
                    revision,
                    bytes: vec![b'x'],
                    published_at: Instant::now(),
                })
                .unwrap_or_else(|error| panic!("output send failed: {error}"));
            frame_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap_or_else(|error| panic!("forwarder output timeout: {error}"));
            latency_us.push(sent.elapsed().as_micros() as u64);
            thread::sleep(Duration::from_millis(1));
        }

        latency_us.sort_unstable();
        let percentile = |p: f64| {
            let index = ((latency_us.len().saturating_sub(1)) as f64 * p).round() as usize;
            latency_us.get(index).copied().unwrap_or(0)
        };
        eprintln!(
            "shared fan-out repeat: packets=120 p50={:.2}ms p95={:.2}ms max={:.2}ms",
            percentile(0.50) as f64 / 1_000.0,
            percentile(0.95) as f64 / 1_000.0,
            latency_us.last().copied().unwrap_or(0) as f64 / 1_000.0,
        );
        assert!(
            percentile(0.95) < 12_000,
            "shared fan-out p95 exceeded one display tick: {}us",
            percentile(0.95)
        );

        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        event_sender
            .send(shardlane_host::shared_tui::TuiEvent::Wake)
            .unwrap_or_else(|error| panic!("stop wake send failed: {error}"));
        forwarder
            .join()
            .unwrap_or_else(|_| panic!("forwarder thread panicked"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn hosted_pty_input_round_trip_reaches_ghostty_model() {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "stty -echo; IFS= read -r line; printf 'ACK:%s\\n' \"$line\"",
        ]);
        let mut terminal = ManagedTerminal::host_process(&command, 80, 24, 1_000)
            .unwrap_or_else(|error| panic!("{error}"));
        terminal
            .input_handle()
            .send_text_traced("probe\n", "test_round_trip")
            .unwrap_or_else(|error| panic!("{error}"));

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let drained = terminal.drain_frames_budgeted(64 * 1024);
            if drained.consumed {
                let frame = terminal
                    .frame_reusing(None)
                    .unwrap_or_else(|error| panic!("{error}"));
                let screen = frame
                    .lines
                    .iter()
                    .flat_map(|line| line.runs.iter())
                    .map(|run| run.text.as_str())
                    .collect::<String>();
                if screen.contains("ACK:probe") {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("hosted PTY round-trip did not reach Ghostty model");
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "local input-latency diagnostic; run explicitly on the development Mac"]
    fn hosted_pty_repeated_text_input_latency_smoke() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "stty raw -echo; exec cat"]);
        let mut terminal = ManagedTerminal::host_process(&command, 120, 40, 1_000)
            .unwrap_or_else(|error| panic!("{error}"));
        let input = terminal.input_handle();
        let mut previous = terminal
            .frame_reusing(None)
            .unwrap_or_else(|error| panic!("{error}"));
        let mut extract_us = Vec::new();
        let started = Instant::now();

        // 100 Hz is intentionally faster than a typical macOS key-repeat stream. Interleave
        // input and model draining so this exercises the same ordered writer -> PTY -> Ghostty
        // path without manufacturing an artificial unread PTY backlog.
        for _ in 0..120 {
            input
                .send_text_traced("x", "repeat_smoke")
                .unwrap_or_else(|error| panic!("{error}"));
            std::thread::sleep(Duration::from_millis(10));
            let drained = terminal.drain_frames_budgeted(64 * 1024);
            if drained.consumed {
                let extract_started = Instant::now();
                previous = terminal
                    .frame_reusing(Some(&previous))
                    .unwrap_or_else(|error| panic!("{error}"));
                extract_us.push(extract_started.elapsed().as_micros() as u64);
            }
        }

        let deadline = Instant::now() + Duration::from_millis(250);
        while Instant::now() < deadline {
            let drained = terminal.drain_frames_budgeted(64 * 1024);
            if !drained.consumed {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            let extract_started = Instant::now();
            previous = terminal
                .frame_reusing(Some(&previous))
                .unwrap_or_else(|error| panic!("{error}"));
            extract_us.push(extract_started.elapsed().as_micros() as u64);
        }

        extract_us.sort_unstable();
        let percentile = |p: f64| {
            let index = ((extract_us.len().saturating_sub(1)) as f64 * p).round() as usize;
            extract_us.get(index).copied().unwrap_or(0)
        };
        eprintln!(
            "repeat input 100Hz: packets=120 elapsed={:.2}s frame_samples={} extract_p50={:.2}ms extract_p95={:.2}ms extract_max={:.2}ms",
            started.elapsed().as_secs_f64(),
            extract_us.len(),
            percentile(0.50) as f64 / 1_000.0,
            percentile(0.95) as f64 / 1_000.0,
            extract_us.last().copied().unwrap_or(0) as f64 / 1_000.0,
        );
        assert!(!extract_us.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn hosted_pty_child_gets_a_controlling_terminal() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "ps -o tty= -p $$"]);
        let stream =
            TerminalStream::spawn_pty(&command, 80, 24).unwrap_or_else(|error| panic!("{error}"));

        let mut output = Vec::new();
        for _ in 0..8 {
            match stream.frames.recv_timeout(Duration::from_millis(500)) {
                Ok(frame) => output.extend(frame.ansi_bytes),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if !output.is_empty() => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            }
        }
        let output = String::from_utf8_lossy(&output);
        assert!(
            !output.trim().is_empty(),
            "hosted PTY child must emit tty state"
        );
        assert!(
            !output.contains("??"),
            "hosted PTY child has no controlling terminal: {output:?}"
        );
    }

    #[test]
    fn base64_decodes_ansi_bytes() {
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode("SGVsbG8=")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(decoded, b"Hello");
    }
}
