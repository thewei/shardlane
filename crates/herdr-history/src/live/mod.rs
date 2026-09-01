// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.
//! [INPUT]: Depends on adapters' ClaudeSession/CodexSession/PiSession (the
//!           line-level interpretation states shared with full parsing),
//!           models::{AgentId, SessionFileRef, TranscriptMessage}, and
//!           live::cursor.
//! [OUTPUT]: Exposes LiveSession / LiveSync / LiveChange / LiveSnapshot /
//!           LiveFacts / LiveDecoder; provides DecoderUpdate and
//!           LiveDecoderState within the crate.
//! [POS]: Phase-1 live semantic source: exact (agent, native_id) binding +
//!        incremental byte tail + line-by-line decoding of Claude/Codex/Pi
//!        session files running inside the Herdr TUI. Interpretation rules
//!        share the same state machine with the full adapters
//!        (`parse_full(fixture) == incremental feed(settle)` holds by
//!        construction); no GUI, no process lifecycle, no TUI/ANSI
//!        inference. Render paths must not call this module's I/O entries
//!        (open/sync); they may only consume fetched snapshots.

use crate::adapters::claude::ClaudeSession;
use crate::adapters::codex::CodexSession;
use crate::adapters::command_code::CommandCodeSession;
use crate::adapters::cursor::CursorSession;
use crate::adapters::kimi::KimiSession;
use crate::adapters::pi::PiSession;
use crate::models::{AgentId, SessionFileRef, TranscriptMessage};
use anyhow::{bail, Result};
use std::collections::HashSet;

pub(crate) mod cursor;
pub mod registry;
#[cfg(test)]
mod tests;
pub mod transport;
pub mod wake;

/// Line-level changes reported by the decoder (snapshot index space;
/// idempotent upsert semantics).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DecoderUpdate {
    /// Message indices newly appended this round.
    pub appended: Vec<usize>,
    /// Message indices updated in place this round (tool result back-fill,
    /// round merging, etc.).
    pub changed: Vec<usize>,
    /// Projection invalidated wholesale (e.g. a Command Code lineage change):
    /// the visible sequence is no longer a superset of the old one, upsert
    /// cannot express shrinking, and consumers must replace with the complete
    /// projection.
    pub projection_reset: bool,
}

/// Provider-agnostic snapshot of parsed facts (the comparison surface of the
/// full/incremental equivalence contract).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LiveFacts {
    pub title: String,
    pub cwd: String,
    pub git_branch: Option<String>,
    pub model: Option<String>,
    pub source: Option<String>,
    pub tokens_used: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub unknown_lines: u32,
    pub session_id: Option<String>,
}

/// Provider decoder interface sharing the same `feed_line` state machine as
/// full parsing.
pub(crate) trait LiveDecoderState: Send {
    fn fresh() -> Self
    where
        Self: Sized;
    fn feed_line(&mut self, line: &str);
    fn count_unknown_line(&mut self);
    fn snapshot_messages(&self) -> Vec<TranscriptMessage>;
    fn take_update(&mut self) -> DecoderUpdate;
    fn facts(&self) -> LiveFacts;
    /// Total message count (including the pending projection unflushed during
    /// live; consistent with the snapshot_messages length).
    fn message_count(&self) -> usize;
    /// Clone a single message (the content channel of incremental deltas;
    /// `message_count` index space).
    fn message_at(&self, index: usize) -> Option<TranscriptMessage>;
}

impl LiveDecoderState for ClaudeSession {
    fn fresh() -> Self {
        // The live mainline view matches the History detail: no subagent
        // sidechain lines.
        Self::new(false)
    }

    fn feed_line(&mut self, line: &str) {
        Self::feed_line(self, line)
    }

    fn count_unknown_line(&mut self) {
        Self::count_unknown_line(self)
    }

    fn snapshot_messages(&self) -> Vec<TranscriptMessage> {
        Self::snapshot_messages(self)
    }

    fn take_update(&mut self) -> DecoderUpdate {
        std::mem::take(&mut self.update)
    }

    fn facts(&self) -> LiveFacts {
        LiveFacts {
            title: self.resolved_title(),
            cwd: self.cwd.clone(),
            git_branch: self.git_branch.clone(),
            model: self.model.clone(),
            source: None,
            tokens_used: self.tokens_used,
            created_at: self.created_at,
            updated_at: self.updated_at,
            unknown_lines: self.unknown_lines,
            session_id: None,
        }
    }

    fn message_count(&self) -> usize {
        Self::live_message_count(self)
    }

    fn message_at(&self, index: usize) -> Option<TranscriptMessage> {
        Self::live_message_at(self, index)
    }
}

impl LiveDecoderState for CodexSession {
    fn fresh() -> Self {
        Self::new()
    }

    fn feed_line(&mut self, line: &str) {
        Self::feed_line(self, line)
    }

    fn count_unknown_line(&mut self) {
        Self::count_unknown_line(self)
    }

    fn snapshot_messages(&self) -> Vec<TranscriptMessage> {
        Self::snapshot_messages(self)
    }

    fn take_update(&mut self) -> DecoderUpdate {
        std::mem::take(&mut self.update)
    }

    fn facts(&self) -> LiveFacts {
        LiveFacts {
            title: crate::adapters::parse_utils::title_from_messages(&self.snapshot_messages())
                .unwrap_or_else(|| crate::models::UNTITLED.to_string()),
            cwd: self.cwd.clone(),
            git_branch: self.git_branch.clone(),
            model: self.model.clone(),
            source: self.source.clone(),
            tokens_used: self.tokens_used,
            created_at: self.created_at,
            updated_at: self.updated_at,
            unknown_lines: self.unknown_lines,
            session_id: None,
        }
    }

    fn message_count(&self) -> usize {
        Self::live_message_count(self)
    }

    fn message_at(&self, index: usize) -> Option<TranscriptMessage> {
        Self::live_message_at(self, index)
    }
}

impl LiveDecoderState for PiSession {
    fn fresh() -> Self {
        Self::new()
    }

    fn feed_line(&mut self, line: &str) {
        Self::feed_line(self, line)
    }

    fn count_unknown_line(&mut self) {
        Self::count_unknown_line(self)
    }

    fn snapshot_messages(&self) -> Vec<TranscriptMessage> {
        Self::snapshot_messages(self)
    }

    fn take_update(&mut self) -> DecoderUpdate {
        std::mem::take(&mut self.update)
    }

    fn facts(&self) -> LiveFacts {
        LiveFacts {
            title: crate::adapters::parse_utils::title_from_messages(&self.snapshot_messages())
                .unwrap_or_else(|| crate::models::UNTITLED.to_string()),
            cwd: self.cwd.clone(),
            git_branch: None,
            model: self.model.clone(),
            source: None,
            tokens_used: self.tokens_used.unwrap_or(0),
            created_at: self.created_at,
            updated_at: self.last_ts,
            unknown_lines: self.unknown_lines,
            session_id: self.session_id.clone(),
        }
    }

    fn message_count(&self) -> usize {
        Self::live_message_count(self)
    }

    fn message_at(&self, index: usize) -> Option<TranscriptMessage> {
        Self::live_message_at(self, index)
    }
}

impl LiveDecoderState for CommandCodeSession {
    fn fresh() -> Self {
        crate::adapters::command_code::CommandCodeSession::fresh()
    }

    fn feed_line(&mut self, line: &str) {
        // live feeds line by line: line parsing happens inside ingest_row (no
        // EOF concept).
        if line.trim().is_empty() {
            return;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(row) => {
                let timestamp = row
                    .get("timestamp")
                    .and_then(serde_json::Value::as_str)
                    .map(crate::adapters::parse_utils::iso_ms)
                    .unwrap_or(0);
                self.ingest_row(&row, timestamp);
            }
            Err(_) => self.unknown_lines += 1,
        }
    }

    fn count_unknown_line(&mut self) {
        self.unknown_lines += 1;
    }

    fn snapshot_messages(&self) -> Vec<TranscriptMessage> {
        self.messages.clone()
    }

    fn take_update(&mut self) -> DecoderUpdate {
        std::mem::take(&mut self.update)
    }

    fn facts(&self) -> LiveFacts {
        LiveFacts {
            title: self
                .title
                .clone()
                .or_else(|| crate::adapters::parse_utils::title_from_messages(&self.messages))
                .unwrap_or_else(|| crate::models::UNTITLED.to_string()),
            cwd: self
                .header
                .as_ref()
                .map_or_else(String::new, |h| h.cwd.clone()),
            git_branch: None,
            model: self.model.clone(),
            source: Some("command-code".to_string()),
            tokens_used: self.tokens_used.unwrap_or(0),
            created_at: self.header.as_ref().map_or(0, |h| h.created_at),
            updated_at: self.updated_at,
            unknown_lines: self.unknown_lines,
            session_id: self.header.as_ref().map(|h| h.id.clone()),
        }
    }

    fn message_count(&self) -> usize {
        self.messages.len()
    }

    fn message_at(&self, index: usize) -> Option<TranscriptMessage> {
        self.messages.get(index).cloned()
    }
}

impl LiveDecoderState for CursorSession {
    fn fresh() -> Self {
        CursorSession::fresh()
    }

    fn feed_line(&mut self, line: &str) {
        CursorSession::feed_line(self, line)
    }

    fn count_unknown_line(&mut self) {
        CursorSession::count_unknown_line(self)
    }

    fn snapshot_messages(&self) -> Vec<TranscriptMessage> {
        let mut messages = self.messages.clone();
        if let Some(mut pending) = self
            .pending
            .as_ref()
            .and_then(crate::adapters::cursor::pending_message)
        {
            pending.seq = messages.len() as i64;
            messages.push(pending);
        }
        messages
    }

    fn take_update(&mut self) -> DecoderUpdate {
        std::mem::take(&mut self.update)
    }

    fn facts(&self) -> LiveFacts {
        LiveFacts {
            title: crate::adapters::parse_utils::title_from_messages(&self.messages)
                .unwrap_or_else(|| crate::models::UNTITLED.to_string()),
            // cwd is decoded by the History layer from the transcript path's
            // project slug; line-level content carries no cwd.
            cwd: String::new(),
            git_branch: None,
            model: None,
            source: None,
            tokens_used: 0,
            created_at: self.created_at,
            updated_at: self.updated_at,
            unknown_lines: self.unknown_lines,
            session_id: None,
        }
    }

    fn message_count(&self) -> usize {
        self.live_message_count()
    }

    fn message_at(&self, index: usize) -> Option<TranscriptMessage> {
        self.live_message_at(index)
    }
}

impl LiveDecoderState for KimiSession {
    fn fresh() -> Self {
        KimiSession::fresh()
    }

    fn feed_line(&mut self, line: &str) {
        KimiSession::feed_line(self, line)
    }

    fn count_unknown_line(&mut self) {
        KimiSession::count_unknown_line(self)
    }

    fn snapshot_messages(&self) -> Vec<TranscriptMessage> {
        self.messages.clone()
    }

    fn take_update(&mut self) -> DecoderUpdate {
        std::mem::take(&mut self.update)
    }

    fn facts(&self) -> LiveFacts {
        // wire.jsonl carries no title/cwd/model: the title falls back to the
        // first user message; cwd is resolved by the History layer through
        // session_index; the state.json sidecar is not a live source
        // (plan §7.2: it only affects metadata and is never a transcript
        // source).
        LiveFacts {
            title: crate::adapters::parse_utils::title_from_messages(&self.messages)
                .unwrap_or_else(|| crate::models::UNTITLED.to_string()),
            cwd: String::new(),
            git_branch: None,
            model: None,
            source: None,
            tokens_used: 0,
            created_at: 0,
            updated_at: 0,
            unknown_lines: self.unknown_lines,
            session_id: None,
        }
    }

    fn message_count(&self) -> usize {
        self.messages.len()
    }

    fn message_at(&self, index: usize) -> Option<TranscriptMessage> {
        self.messages.get(index).cloned()
    }
}

/// Phase-1 supported provider decoders (Omp is an isomorphic fork of Pi and
/// shares the same interpretation state).
pub struct LiveDecoder {
    agent: AgentId,
    state: Box<dyn LiveDecoderState>,
}

impl LiveDecoder {
    /// The capability registry (PEX-1) is the sole authority for live
    /// capability; the decoder admits providers based on it and no longer
    /// keeps its own list.
    pub fn supports(agent: AgentId) -> bool {
        registry::capabilities(agent)
            .is_some_and(|caps| caps.live == registry::LiveCapability::AppendLog)
    }

    pub fn new(agent: AgentId) -> Option<Self> {
        let state: Box<dyn LiveDecoderState> = match agent {
            AgentId::ClaudeCode => Box::new(ClaudeSession::fresh()),
            AgentId::Codex => Box::new(CodexSession::fresh()),
            AgentId::Pi | AgentId::Omp => Box::new(PiSession::fresh()),
            AgentId::CommandCode => Box::new(CommandCodeSession::fresh()),
            AgentId::Cursor => Box::new(CursorSession::fresh()),
            AgentId::Kimi => Box::new(KimiSession::fresh()),
            _ => return None,
        };
        Some(Self { agent, state })
    }

    fn reset(&mut self) {
        // Only called for supported providers; keep the old state instead of
        // panicking when unsupported.
        if let Some(fresh) = Self::new(self.agent) {
            self.state = fresh.state;
        }
    }

    fn feed_line(&mut self, line: &str) {
        self.state.feed_line(line);
    }

    fn count_unknown_line(&mut self) {
        self.state.count_unknown_line();
    }

    fn take_update(&mut self) -> DecoderUpdate {
        self.state.take_update()
    }

    fn facts(&self) -> LiveFacts {
        self.state.facts()
    }

    fn snapshot_messages(&self) -> Vec<TranscriptMessage> {
        self.state.snapshot_messages()
    }

    fn message_count(&self) -> usize {
        self.state.message_count()
    }

    fn message_at(&self, index: usize) -> Option<TranscriptMessage> {
        self.state.message_at(index)
    }
}

/// The result of one sync/settle. `appended`/`changed` are upsert hints;
/// consumers may rebuild the full view via `LiveSession::snapshot()` at any
/// time without breaking correctness. The incremental content itself is
/// carried by `appended_messages`/`changed_messages` (cloned per variant
/// entry, never a full clone); a full snapshot is provided only on Reset
/// (the `snapshot` field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSync {
    pub change: LiveChange,
    pub generation: u64,
    /// Message indices newly appended this round (one-to-one with
    /// `appended_messages`).
    pub appended: Vec<usize>,
    /// Message indices updated in place this round (excluding indices already
    /// covered by `appended`).
    pub changed: Vec<usize>,
    /// Message contents for the `appended` indices (same order).
    pub appended_messages: Vec<TranscriptMessage>,
    /// Message contents for the `changed` indices (same order).
    pub changed_messages: Vec<TranscriptMessage>,
    /// The complete new snapshot on Reset; always None for
    /// Appended/Unchanged.
    pub snapshot: Option<LiveSnapshot>,
    pub lines_fed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveChange {
    /// No new bytes: duplicate FS wakes converge here and are never consumed
    /// twice.
    Unchanged,
    /// Append-only: decoded incrementally.
    Appended,
    /// Truncate/replace: decoding state rebuilt, snapshot reflects the new
    /// content.
    Reset,
}

impl LiveSync {
    fn unchanged(generation: u64) -> Self {
        Self {
            change: LiveChange::Unchanged,
            generation,
            appended: Vec::new(),
            changed: Vec::new(),
            appended_messages: Vec::new(),
            changed_messages: Vec::new(),
            snapshot: None,
            lines_fed: 0,
        }
    }

    pub fn is_unchanged(&self) -> bool {
        self.change == LiveChange::Unchanged
    }
}

/// Current semantic snapshot of the session (safe for the render layer to
/// hold; contains no I/O handles).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSnapshot {
    pub messages: Vec<TranscriptMessage>,
    pub facts: LiveFacts,
    pub generation: u64,
}

/// Live incremental decoding session for one exact provider session (PEX-2:
/// source differences converge in `LiveTransport`; the session itself is
/// provider/transport agnostic).
///
/// Lifecycle: `open` (one-shot hydration, the only allowed full read) →
/// repeated `sync` (the transport delivers incremental bytes) → `settle`
/// when a terminal view is needed (interpret the trailing partial line as a
/// complete line, equivalent to full parsing).
pub struct LiveSession {
    source: SessionFileRef,
    decoder: LiveDecoder,
    transport: Box<dyn transport::LiveTransport>,
    /// Partial-line buffer for a tail without `\n` (not counted as consumed
    /// by the transport; committed together after the newline).
    partial: Vec<u8>,
    generation: u64,
    /// Number of messages already reported to the consumer: anything after
    /// them is delivered as `appended`.
    reported_len: usize,
    /// One-shot `initial_delivery` flag (explicit initial installation of the
    /// open-hydrated content).
    initial_delivered: bool,
    /// Content signature of the tail message after the last delivery.
    /// Claude's same-id assistant continuation lines only extend the pending
    /// projection and produce no decoder bookkeeping, so this signature is
    /// the only way to detect them (the backstop of incremental semantics).
    tail: Option<TailSignature>,
}

/// Cheap content signature of the tail message (length tuple, no string
/// clones).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TailSignature {
    text_len: usize,
    thinking_len: usize,
    tool_calls: usize,
}

impl LiveSession {
    /// Whether this provider has a live decoder (Phase-1:
    /// Claude/Codex/Pi/Omp).
    pub fn supports(agent: AgentId) -> bool {
        LiveDecoder::supports(agent)
    }

    /// Bind the exact source and hydrate existing content (the default
    /// AppendLog transport). Callers must obtain `source` via an exact
    /// catalog lookup (`HistoryCatalog::session_source_by_native`), never by
    /// guessing from cwd/mtime.
    pub fn open(source: SessionFileRef) -> Result<Self> {
        let transport = transport::AppendLogTransport::new(&source.file_path);
        Self::open_with_transport(source, Box::new(transport))
    }

    /// Open with an explicit transport (PEX-2: once the future
    /// SnapshotJournal / SqliteChange / CompressedFrame / ProtocolEvent each
    /// implement `LiveTransport`, they enter here and the session and Chat no
    /// longer perceive source differences).
    pub fn open_with_transport(
        source: SessionFileRef,
        mut transport: Box<dyn transport::LiveTransport>,
    ) -> Result<Self> {
        let Some(decoder) = LiveDecoder::new(source.agent) else {
            bail!(
                "live decoding is not supported for agent {}",
                source.agent.as_str()
            );
        };
        let bytes = transport.hydrate()?;
        let mut session = Self {
            source,
            decoder,
            transport,
            partial: Vec::new(),
            generation: 0,
            reported_len: 0,
            initial_delivered: false,
            tail: None,
        };
        session.feed_bytes(&bytes, false);
        // Clear hydration-period decoder bookkeeping; pre-advance the report
        // bookkeeping — `sync` keeps its "report only new items" contract,
        // and the hydrated content is delivered once explicitly by
        // `initial_delivery`.
        let _ = session.decoder.take_update();
        session.reported_len = session.decoder.message_count();
        session.tail = session.tail_signature();
        Ok(session)
    }

    pub fn source(&self) -> &SessionFileRef {
        &self.source
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Current semantic snapshot (no I/O; callable from render/state threads
    /// at any time).
    pub fn snapshot(&self) -> LiveSnapshot {
        LiveSnapshot {
            messages: self.decoder.snapshot_messages(),
            facts: self.decoder.facts(),
            generation: self.generation,
        }
    }

    /// Poll the transport and incrementally consume the delivered bytes.
    /// Returns `Unchanged` when nothing was read; duplicate FS wakes are
    /// naturally idempotent.
    pub fn sync(&mut self) -> Result<LiveSync> {
        match self.transport.poll()? {
            transport::TransportChunk::Unchanged => Ok(LiveSync::unchanged(self.generation)),
            transport::TransportChunk::Replace(bytes) => {
                // Truncate/replace: rebuild wholesale, never splice across
                // generations (the partial-line buffer belongs to the old
                // generation).
                self.decoder.reset();
                self.partial.clear();
                self.generation += 1;
                let lines_fed = self.feed_bytes(&bytes, false);
                Ok(self.report_feed(lines_fed, true))
            }
            transport::TransportChunk::Append(bytes) => {
                let lines_fed = self.feed_bytes(&bytes, false);
                Ok(self.report_feed(lines_fed, false))
            }
        }
    }

    /// Initial delivery: hand the open-hydrated existing content to the
    /// consumer as one batch of appended upserts. After open, `sync` reports
    /// only new items (delivering nothing while Unchanged), so the consumer
    /// must call this explicitly to complete the initial installation or it
    /// stays stuck on Connecting (root cause confirmed in two acceptance
    /// rounds). One-shot: repeated calls return Unchanged; no I/O.
    pub fn initial_delivery(&mut self) -> LiveSync {
        if self.initial_delivered {
            return LiveSync::unchanged(self.generation);
        }
        self.initial_delivered = true;
        let total = self.decoder.message_count();
        let appended: Vec<usize> = (0..total).collect();
        let appended_messages: Vec<TranscriptMessage> = appended
            .iter()
            .filter_map(|index| self.decoder.message_at(*index))
            .collect();
        LiveSync {
            change: LiveChange::Appended,
            generation: self.generation,
            appended,
            changed: Vec::new(),
            appended_messages,
            changed_messages: Vec::new(),
            snapshot: None,
            lines_fed: 0,
        }
    }

    /// Terminal view: interpret the trailing partial line as a complete line
    /// (file finished-writing semantics); the result matches full parsing
    /// exactly. No I/O.
    pub fn settle(&mut self) -> LiveSync {
        let lines_fed = self.feed_bytes(&[], true);
        if lines_fed == 0 {
            return LiveSync::unchanged(self.generation);
        }
        self.report_feed(lines_fed, false)
    }

    /// Turn this feed's decoding results into a content-carrying LiveSync and
    /// advance the report bookkeeping.
    ///
    /// The delivered index set = decoder bookkeeping (appended/changed,
    /// idempotent upsert; indices may repeat across batches) ∪ the not-yet-
    /// delivered message range (e.g. a new pending projection). The delivery
    /// semantics are index-idempotent upsert, so duplicate delivery is
    /// harmless. Tail-signature backstop: same-id assistant continuation
    /// lines only extend the pending projection and produce no bookkeeping;
    /// when there is no bookkeeping and no new range, report as changed.
    fn report_feed(&mut self, lines_fed: usize, reset: bool) -> LiveSync {
        let update = self.decoder.take_update();
        if update.projection_reset {
            // Projection replacement (lineage change): deliver the complete
            // new projection; report bookkeeping advances directly to the new
            // projection length.
            self.tail = self.tail_signature();
            self.reported_len = self.decoder.message_count();
            return LiveSync {
                change: LiveChange::Reset,
                generation: self.generation,
                appended: Vec::new(),
                changed: Vec::new(),
                appended_messages: Vec::new(),
                changed_messages: Vec::new(),
                snapshot: Some(self.snapshot()),
                lines_fed,
            };
        }
        let total = self.decoder.message_count();
        let mut appended = update.appended;
        let mut changed = update.changed;
        // Set lookup keeps the fill loop linear in the reported range even
        // when the decoder's bookkeeping lists are long.
        let booked: HashSet<usize> = appended.iter().chain(changed.iter()).copied().collect();
        appended.extend((self.reported_len..total).filter(|index| !booked.contains(index)));
        // Tail-signature backstop: pending continuation (same-id assistant
        // lines) produces no bookkeeping.
        let tail_now = self.tail_signature();
        if appended.is_empty() && changed.is_empty() && total > 0 && self.tail != tail_now {
            changed.push(total - 1);
        }
        appended.sort_unstable();
        appended.dedup();
        changed.sort_unstable();
        changed.dedup();
        appended.retain(|index| *index < total);
        changed.retain(|index| *index < total);
        let appended_messages = appended
            .iter()
            .filter_map(|index| self.decoder.message_at(*index))
            .collect();
        let changed_messages = changed
            .iter()
            .filter_map(|index| self.decoder.message_at(*index))
            .collect();
        self.tail = tail_now;
        self.reported_len = total;
        let snapshot = reset.then(|| self.snapshot());
        LiveSync {
            change: if reset {
                LiveChange::Reset
            } else {
                LiveChange::Appended
            },
            generation: self.generation,
            appended,
            changed,
            appended_messages,
            changed_messages,
            snapshot,
            lines_fed,
        }
    }

    fn tail_signature(&self) -> Option<TailSignature> {
        let total = self.decoder.message_count();
        let message = self.decoder.message_at(total.checked_sub(1)?)?;
        Some(TailSignature {
            text_len: message.text.len(),
            thinking_len: message.thinking.as_deref().map_or(0, str::len),
            tool_calls: message.tool_calls.len(),
        })
    }

    /// Feed bytes: split complete lines by `\n` (CRLF normalized, invalid
    /// UTF-8 counted as unknown) and keep the trailing partial line for
    /// later; with `settle=true` the remaining partial line is interpreted as
    /// a complete line. Chunk bytes are committed whole upon entering the
    /// pipeline (equivalent to the old `consumed_total` bookkeeping: the
    /// partial line is already "seen", the next poll starts after it and
    /// never re-delivers).
    fn feed_bytes(&mut self, bytes: &[u8], settle: bool) -> usize {
        self.transport.commit_consumed(bytes.len() as u64);
        self.partial.extend_from_slice(bytes);
        let mut lines_fed = 0;
        while let Some(position) = self.partial.iter().position(|&byte| byte == b'\n') {
            let mut line: Vec<u8> = self.partial.drain(..=position).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            feed_line_bytes(&mut self.decoder, line);
            lines_fed += 1;
        }
        if settle && !self.partial.is_empty() {
            let rest = std::mem::take(&mut self.partial);
            feed_line_bytes(&mut self.decoder, rest);
            lines_fed += 1;
        }
        lines_fed
    }
}

fn feed_line_bytes(decoder: &mut LiveDecoder, line: Vec<u8>) {
    match String::from_utf8(line) {
        Ok(text) => decoder.feed_line(&text),
        Err(_) => decoder.count_unknown_line(),
    }
}
