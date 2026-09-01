// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.

//! Transport-neutral live semantic source layer. `LiveSession` consumes only byte chunks delivered by
//! `LiveTransport`; source differences such as files/databases/compressed
//! frames/protocol streams all converge inside transport implementations, and
//! new providers no longer touch LiveSession or Chat.
//!
//! [INPUT]: cursor::LiveCursor (AppendLog's verdicts and offset bookkeeping);
//! zero decoding knowledge.
//! [OUTPUT]: The LiveTransport trait, TransportChunk, AppendLogTransport,
//!           and the construction surface used by open_with_transport.
//! [POS]: `AppendLogTransport` is a verbatim extraction of the existing
//! append-only JSONL semantics (Claude/Codex/Pi/Omp): metadata verdict +
//! range read, `consumed` counts complete lines only, same-length in-place
//! rewrites are not detected (recorded limitation CHAT-A19 of
//! chat-provider-expansion). The planned SnapshotJournal / SqliteChange /
//! CompressedFrame / ProtocolEvent each implement this trait in Waves 2/3;
//! the session confirms bytes entering the pipeline via `commit_consumed`,
//! and the transport advances its offset accordingly (the partial-line
//! buffer is held by the session and not counted as consumed).

use crate::live::cursor::LiveCursor;
use anyhow::Result;
use std::io::{Read, Seek, SeekFrom};

/// Delivery of one `poll`. Bytes carried by `Append`/`Replace` must be fed
/// into the decoding pipeline immediately, then confirmed with
/// `commit_consumed(len)`.
#[derive(Debug)]
pub enum TransportChunk {
    /// No new content: duplicate wakes are naturally idempotent, zero reads.
    Unchanged,
    /// Only grew: new bytes (partial-line safe; the tail line may be
    /// incomplete).
    Append(Vec<u8>),
    /// Truncate/replace: the whole new content; the session must rebuild its
    /// decoding state wholesale and never splice across generations.
    Replace(Vec<u8>),
}

/// Transport policy for a live semantic source. Implementations own all I/O
/// and source-specific consistency verdicts; decoding and report bookkeeping
/// live in `LiveSession` (provider agnostic).
pub trait LiveTransport: Send {
    /// One-shot hydration read (the only full read allowed at open).
    fn hydrate(&mut self) -> Result<Vec<u8>>;
    /// Poll the source once and read increments as needed.
    fn poll(&mut self) -> Result<TransportChunk>;
    /// The session confirms `bytes` have entered the decoding pipeline
    /// (partial-line buffers are not counted).
    fn commit_consumed(&mut self, bytes: u64);
}

/// append-only JSONL file tailing (existing semantics of
/// Claude/Codex/Pi/Omp).
pub struct AppendLogTransport {
    path: String,
    cursor: LiveCursor,
}

impl AppendLogTransport {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            cursor: LiveCursor::new(),
        }
    }
}

impl LiveTransport for AppendLogTransport {
    fn hydrate(&mut self) -> Result<Vec<u8>> {
        Ok(std::fs::read(&self.path)?)
    }

    fn poll(&mut self) -> Result<TransportChunk> {
        let size = std::fs::metadata(&self.path)?.len();
        match self.cursor.classify(size) {
            crate::live::cursor::TailVerdict::Unchanged => Ok(TransportChunk::Unchanged),
            crate::live::cursor::TailVerdict::Shrunk => {
                // Truncate/replace: zero the cursor and hand the whole new
                // content to the session for a rebuild.
                let bytes = std::fs::read(&self.path)?;
                self.cursor.reset();
                Ok(TransportChunk::Replace(bytes))
            }
            crate::live::cursor::TailVerdict::Appended { from, to } => {
                Ok(TransportChunk::Append(read_range(&self.path, from, to)?))
            }
        }
    }

    fn commit_consumed(&mut self, bytes: u64) {
        self.cursor.advance(bytes);
    }
}

fn read_range(path: &str, from: u64, to: u64) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(from))?;
    let mut buffer = vec![0u8; (to - from) as usize];
    file.read_exact(&mut buffer)?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Transport contract: hydrate full, poll three-state verdicts, commit
    /// advances the offset.
    #[test]
    fn append_log_transport_classifies_and_advances() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("transport.jsonl");
        std::fs::write(&path, b"line-1\n")?;
        let mut transport = AppendLogTransport::new(path.to_str().unwrap_or(""));

        let hydrated = transport.hydrate()?;
        assert_eq!(hydrated, b"line-1\n".to_vec());
        transport.commit_consumed(hydrated.len() as u64);

        // No new bytes.
        assert!(matches!(transport.poll()?, TransportChunk::Unchanged));

        // Append: only the new range is delivered; after commit it converges
        // back to Unchanged.
        std::fs::write(&path, b"line-1\nline-2\n")?;
        match transport.poll()? {
            TransportChunk::Append(bytes) => {
                assert_eq!(bytes, b"line-2\n".to_vec());
                transport.commit_consumed(bytes.len() as u64);
            }
            other => panic!("expected append, got {other:?}"),
        }
        assert!(matches!(transport.poll()?, TransportChunk::Unchanged));

        // Partial line: the session commits the whole block (already "seen";
        // the partial-line buffer lives on the session side) → the next poll
        // starts after it and never re-delivers the partial line.
        std::fs::write(&path, b"line-1\nline-2\nhalf")?;
        match transport.poll()? {
            TransportChunk::Append(bytes) => {
                assert_eq!(bytes, b"half".to_vec());
                transport.commit_consumed(bytes.len() as u64);
            }
            other => panic!("expected append, got {other:?}"),
        }
        assert!(matches!(transport.poll()?, TransportChunk::Unchanged));

        // Shrank: rebuild wholesale.
        std::fs::write(&path, b"new\n")?;
        match transport.poll()? {
            TransportChunk::Replace(bytes) => {
                assert_eq!(bytes, b"new\n".to_vec());
                transport.commit_consumed(bytes.len() as u64);
            }
            other => panic!("expected replace, got {other:?}"),
        }
        assert!(matches!(transport.poll()?, TransportChunk::Unchanged));
        Ok(())
    }
}
