// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.
//! [INPUT]: No external dependencies (pure byte-cursor logic, no I/O).
//! [OUTPUT]: Provides live/transport.rs with LiveCursor (consumed byte
//!           offset) and TailVerdict (unchanged / appended range / truncate
//!           reset) classification.
//! [POS]: Offset bookkeeping and append detection for the append-only tail
//!        (the partial-line buffer is held by the session and only counted at
//!        commit_consumed, so `consumed` is the total bytes fed into the
//!        decoder; same-length in-place rewrites are not detected — session
//!        files are append-only JSONL and content-hashing on every wake would
//!        violate the "zero full reads for normal appends" constraint; a
//!        recorded limitation, CHAT-A19).

/// Byte cursor for an append-only JSONL source.
#[derive(Debug, Default)]
pub(crate) struct LiveCursor {
    /// Bytes that have entered the decoding pipeline.
    consumed: u64,
}

/// One poll's verdict on the source file's growth state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TailVerdict {
    /// Size did not pass the consumed position: no new bytes; duplicate FS
    /// wakes are naturally idempotent here.
    Unchanged,
    /// Only grew: only the `[from, to)` range needs reading
    /// (from = total consumed).
    Appended { from: u64, to: u64 },
    /// Shrunk: truncate/replace; cursor and decoder state must be rebuilt
    /// wholesale.
    Shrunk,
}

impl LiveCursor {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn classify(&self, size: u64) -> TailVerdict {
        if size > self.consumed {
            TailVerdict::Appended {
                from: self.consumed,
                to: size,
            }
        } else if size < self.consumed {
            TailVerdict::Shrunk
        } else {
            TailVerdict::Unchanged
        }
    }

    /// Book-keep bytes that entered the decoder (complete lines include the
    /// newline; partial lines consumed by settle do not).
    pub(crate) fn advance(&mut self, bytes: u64) {
        self.consumed += bytes;
    }

    pub(crate) fn reset(&mut self) {
        self.consumed = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_distinguishes_unchanged_append_and_shrink() {
        let mut cursor = LiveCursor::new();
        assert_eq!(cursor.classify(0), TailVerdict::Unchanged);
        cursor.advance(10);
        assert_eq!(cursor.classify(10), TailVerdict::Unchanged);
        assert_eq!(
            cursor.classify(20),
            TailVerdict::Appended { from: 10, to: 20 }
        );
        assert_eq!(cursor.classify(9), TailVerdict::Shrunk);
        cursor.reset();
        assert_eq!(cursor.classify(5), TailVerdict::Appended { from: 0, to: 5 });
    }
}
