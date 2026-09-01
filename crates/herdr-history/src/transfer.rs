// SPDX-License-Identifier: MIT
// Lossless transfer snapshot semantics retain upstream-MIT-derived JSONL framing
// conventions where applicable.
//! [INPUT]: An exact `SessionFileRef` (the unified History/live resolution
//! product) and Shardlane's own artifact store root directory.
//! [OUTPUT]: The lossless transfer contract. capture_transfer_snapshot
//! captures whole-source bytes, SHA-256, and record-count/bounds metadata,
//! never reusing bounded TranscriptMessage. TransferArtifactStore persists
//! with 0700/0600 permissions, TTL/total-size-bounded cleanup, and fails
//! closed on corruption. build_transfer_briefing builds a deterministic
//! initial briefing of full context plus optional instructions, with no model
//! summarization.
//! [POS]: plan M5 / audit AF-07. Read-only: never rewrites external Agent
//! history; artifacts land only in Shardlane's own cache directory. File
//! names carry no provider/session secrets.

use crate::models::{AgentId, SessionFileRef};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Default ceiling under which the complete session travels inline in the
/// briefing. Larger payloads always go through a local artifact.
pub const DEFAULT_INLINE_LIMIT_BYTES: u64 = 256 * 1024;

/// Artifact retention bounds: entries older than the TTL or beyond the total
/// size/count budget are removed by cleanup.
pub const DEFAULT_ARTIFACT_TTL_MS: u64 = 24 * 60 * 60 * 1000;
pub const DEFAULT_ARTIFACT_MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
pub const DEFAULT_ARTIFACT_MAX_COUNT: usize = 64;

#[derive(Debug)]
pub enum TransferError {
    SourceUnavailable(String),
    /// The source mutated while being captured; the snapshot is unproven and
    /// must not be presented as complete (AC-15).
    SourceUnstable {
        path: String,
        reason: String,
    },
    UnsupportedSource(String),
    ArtifactWrite(String),
    ArtifactCorrupt {
        path: PathBuf,
        reason: String,
    },
    ArtifactAccess(String),
}

impl fmt::Display for TransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceUnavailable(message) => {
                write!(formatter, "transfer source unavailable: {message}")
            }
            Self::SourceUnstable { path, reason } => write!(
                formatter,
                "transfer source changed during capture ({}): {reason}",
                path
            ),
            Self::UnsupportedSource(message) => {
                write!(formatter, "transfer source unsupported: {message}")
            }
            Self::ArtifactWrite(message) => {
                write!(formatter, "transfer artifact write failed: {message}")
            }
            Self::ArtifactCorrupt { path, reason } => write!(
                formatter,
                "transfer artifact corrupt at {}: {reason}",
                path.display()
            ),
            Self::ArtifactAccess(message) => {
                write!(formatter, "transfer artifact not accessible: {message}")
            }
        }
    }
}

impl std::error::Error for TransferError {}

/// How the exact provider-native source is addressed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferSourceKind {
    /// Newline-delimited records (Claude/Codex/Pi/… JSONL sessions).
    FileRecords,
}

/// Integrity + provenance metadata for one lossless capture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferSnapshotMeta {
    pub provider: AgentId,
    pub native_session_id: String,
    pub source_path: String,
    pub source_kind: TransferSourceKind,
    pub byte_length: u64,
    pub record_count: u64,
    pub mtime_ms: i64,
    pub sha256: String,
    pub captured_at_ms: u64,
}

/// A lossless capture of one complete session source.
#[derive(Clone, Debug)]
pub struct TransferSnapshot {
    pub meta: TransferSnapshotMeta,
    pub payload: TransferPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransferPayload {
    /// Complete payload carried inside the briefing (small sessions only).
    Inline(Vec<u8>),
    /// Payload persisted as a Shardlane-owned local artifact.
    Artifact(TransferArtifactRef),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferArtifactRef {
    pub path: PathBuf,
    pub byte_length: u64,
    pub sha256: String,
    pub captured_at_ms: u64,
}

/// Tunable limits for capture and artifact retention.
#[derive(Clone, Debug)]
pub struct TransferLimits {
    pub inline_limit_bytes: u64,
    pub artifact_ttl_ms: u64,
    pub artifact_max_total_bytes: u64,
    pub artifact_max_count: usize,
}

impl Default for TransferLimits {
    fn default() -> Self {
        Self {
            inline_limit_bytes: DEFAULT_INLINE_LIMIT_BYTES,
            artifact_ttl_ms: DEFAULT_ARTIFACT_TTL_MS,
            artifact_max_total_bytes: DEFAULT_ARTIFACT_MAX_TOTAL_BYTES,
            artifact_max_count: DEFAULT_ARTIFACT_MAX_COUNT,
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn count_records(bytes: &[u8]) -> u64 {
    records_from(
        bytes.iter().filter(|byte| **byte == b'\n').count() as u64,
        bytes.last().copied(),
    )
}

/// The record-count rule shared by the in-memory and streaming capture
/// paths: a trailing newline terminates the last record, a final partial
/// line still counts as one record, and no bytes means no records.
fn records_from(newline_count: u64, last_byte: Option<u8>) -> u64 {
    match last_byte {
        None => 0,
        Some(b'\n') => newline_count,
        Some(_) => newline_count + 1,
    }
}

/// Shardlane-owned local artifact store. Strict permissions (verified, not
/// best-effort — AC-13), bounded by TTL and total size/count with throttled
/// production cleanup (AC-14), and verified by content hash on read.
pub struct TransferArtifactStore {
    root: PathBuf,
}

/// Production cleanup cadence (AC-14): cleanup runs after an artifact write at
/// most this often, so the bounded scan never lands in a render path.
const CLEANUP_THROTTLE_MS: u64 = 60_000;

impl TransferArtifactStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn ensure_root(&self) -> Result<(), TransferError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            use std::os::unix::fs::PermissionsExt as _;
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true).mode(0o700);
            builder
                .create(&self.root)
                .map_err(|error| TransferError::ArtifactWrite(error.to_string()))?;
            // AC-13: hardening failures fail closed — a "secure" artifact under
            // a permissive directory is a lie.
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700)).map_err(
                |error| {
                    TransferError::ArtifactWrite(format!(
                        "artifact root chmod 0700 failed: {error}"
                    ))
                },
            )?;
        }
        #[cfg(not(unix))]
        std::fs::create_dir_all(&self.root)
            .map_err(|error| TransferError::ArtifactWrite(error.to_string()))?;
        Ok(())
    }

    fn cleanup_stamp_path(&self) -> PathBuf {
        self.root.join(".cleanup-stamp")
    }

    /// Throttled production cleanup (AC-14): invoked by `write` at most once
    /// per [`CLEANUP_THROTTLE_MS`], so retention policy is actually enforced
    /// without scanning the directory on every operation.
    fn maybe_cleanup(&self, limits: &TransferLimits) {
        let stamp = self.cleanup_stamp_path();
        let now = now_ms();
        let last = std::fs::read_to_string(&stamp)
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(0);
        if now.saturating_sub(last) < CLEANUP_THROTTLE_MS {
            return;
        }
        let _ = self.cleanup(limits);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            use std::os::unix::fs::PermissionsExt as _;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o600)
                .open(&stamp)
            {
                use std::io::Write as _;
                let _ = writeln!(file, "{now}");
            }
            let _ = std::fs::set_permissions(&stamp, std::fs::Permissions::from_mode(0o600));
        }
        #[cfg(not(unix))]
        {
            let _ = std::fs::write(&stamp, format!("{now}\n"));
        }
    }

    /// Persist one artifact. The filename carries only a hash prefix and a
    /// capture timestamp — never provider names or session ids.
    pub fn write(
        &self,
        bytes: &[u8],
        captured_at_ms: u64,
        limits: &TransferLimits,
    ) -> Result<TransferArtifactRef, TransferError> {
        self.ensure_root()?;
        let sha256 = sha256_hex(bytes);
        let path = self
            .root
            .join(format!("transfer-{}-{}.bin", &sha256[..16], captured_at_ms));
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            use std::os::unix::fs::PermissionsExt as _;
            // AC-13: create with the private mode instead of chmod-after-write.
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .and_then(|mut file| std::io::Write::write_all(&mut file, bytes))
                .map_err(|error| TransferError::ArtifactWrite(error.to_string()))?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
                |error| {
                    TransferError::ArtifactWrite(format!("artifact chmod 0600 failed: {error}"))
                },
            )?;
        }
        #[cfg(not(unix))]
        std::fs::write(&path, bytes)
            .map_err(|error| TransferError::ArtifactWrite(error.to_string()))?;
        let reference = TransferArtifactRef {
            byte_length: bytes.len() as u64,
            sha256,
            captured_at_ms,
            path,
        };
        self.maybe_cleanup(limits);
        Ok(reference)
    }

    /// Re-read and hash-verify an artifact. Any mismatch or missing file fails
    /// closed; there is no partial-fidelity fallback.
    pub fn verify(&self, reference: &TransferArtifactRef) -> Result<Vec<u8>, TransferError> {
        let bytes =
            std::fs::read(&reference.path).map_err(|error| TransferError::ArtifactCorrupt {
                path: reference.path.clone(),
                reason: error.to_string(),
            })?;
        if bytes.len() as u64 != reference.byte_length {
            return Err(TransferError::ArtifactCorrupt {
                path: reference.path.clone(),
                reason: format!(
                    "length drift: expected {} bytes, found {}",
                    reference.byte_length,
                    bytes.len()
                ),
            });
        }
        let actual = sha256_hex(&bytes);
        if actual != reference.sha256 {
            return Err(TransferError::ArtifactCorrupt {
                path: reference.path.clone(),
                reason: "sha-256 mismatch".to_string(),
            });
        }
        Ok(bytes)
    }

    /// Bounded retention cleanup: drop expired entries first, then enforce the
    /// total-size/count budget oldest-first. Returns the number removed.
    pub fn cleanup(&self, limits: &TransferLimits) -> usize {
        let entries = match self.list_artifacts() {
            Some(entries) => entries,
            None => return 0,
        };
        let now = now_ms();
        let mut removed = 0usize;
        // TTL pass.
        for entry in &entries {
            if now.saturating_sub(entry.captured_at_ms) > limits.artifact_ttl_ms
                && std::fs::remove_file(&entry.path).is_ok()
            {
                removed += 1;
            }
        }
        // Budget pass (oldest first).
        let mut remaining: Vec<ArtifactEntry> = entries
            .into_iter()
            .filter(|entry| entry.path.exists())
            .collect();
        remaining.sort_by_key(|entry| entry.captured_at_ms);
        let mut total: u64 = remaining.iter().map(|entry| entry.byte_length).sum();
        while remaining.len() > limits.artifact_max_count || total > limits.artifact_max_total_bytes
        {
            let Some(oldest) = remaining.first() else {
                break;
            };
            if std::fs::remove_file(&oldest.path).is_ok() {
                total = total.saturating_sub(oldest.byte_length);
                remaining.remove(0);
                removed += 1;
            } else {
                break;
            }
        }
        removed
    }

    fn list_artifacts(&self) -> Option<Vec<ArtifactEntry>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&self.root).ok()? {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("bin") {
                continue;
            }
            let name = path.file_name()?.to_string_lossy().into_owned();
            let captured_at_ms = name
                .trim_end_matches(".bin")
                .rsplit('-')
                .next()
                .and_then(|stamp| stamp.parse::<u64>().ok())
                .unwrap_or(0);
            let byte_length = entry.metadata().ok()?.len();
            entries.push(ArtifactEntry {
                path,
                byte_length,
                captured_at_ms,
            });
        }
        Some(entries)
    }
}

struct ArtifactEntry {
    path: PathBuf,
    byte_length: u64,
    captured_at_ms: u64,
}

/// Capture one complete session source losslessly (AC-15): stream the source
/// into the artifact (or memory for inline-sized sources) while hashing and
/// counting records in the same pass, then require the source to be unchanged
/// (size + mtime) across the capture. A mutating source retries once and then
/// fails closed as `SourceUnstable` — never an unproven "complete" snapshot.
/// The bounded presentation transcript is never the fidelity boundary here.
pub fn capture_transfer_snapshot(
    source: &SessionFileRef,
    store: &TransferArtifactStore,
    limits: &TransferLimits,
) -> Result<TransferSnapshot, TransferError> {
    let mut attempt = 0_u8;
    loop {
        attempt += 1;
        match capture_transfer_snapshot_once(source, store, limits) {
            Ok(snapshot) => return Ok(snapshot),
            Err(TransferError::SourceUnstable { .. }) if attempt < 2 => continue,
            Err(error) => return Err(error),
        }
    }
}

fn source_stat(path: &str) -> Result<(u64, i64), TransferError> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| TransferError::SourceUnavailable(format!("{}: {error}", path)))?;
    let mtime_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    Ok((metadata.len(), mtime_ms))
}

fn capture_transfer_snapshot_once(
    source: &SessionFileRef,
    store: &TransferArtifactStore,
    limits: &TransferLimits,
) -> Result<TransferSnapshot, TransferError> {
    use sha2::Digest as _;
    let (size_before, mtime_ms) = source_stat(&source.file_path)?;
    let captured_at_ms = now_ms();

    if size_before <= limits.inline_limit_bytes {
        // Small sources: one memory pass, same stability proof.
        let bytes = std::fs::read(&source.file_path).map_err(|error| {
            TransferError::SourceUnavailable(format!("{}: {error}", source.file_path))
        })?;
        let (size_after, mtime_after) = source_stat(&source.file_path)?;
        if (size_after, mtime_after) != (size_before, mtime_ms) {
            return Err(TransferError::SourceUnstable {
                path: source.file_path.clone(),
                reason: "size/mtime changed during capture".to_string(),
            });
        }
        let sha256 = sha256_hex(&bytes);
        let byte_length = bytes.len() as u64;
        Ok(TransferSnapshot {
            meta: TransferSnapshotMeta {
                provider: source.agent,
                native_session_id: source.native_id.clone(),
                source_path: source.file_path.clone(),
                source_kind: TransferSourceKind::FileRecords,
                byte_length,
                record_count: count_records(&bytes),
                mtime_ms,
                sha256,
                captured_at_ms,
            },
            payload: TransferPayload::Inline(bytes),
        })
    } else {
        // R2-19: a single payload larger than the whole artifact budget can
        // never be retained — reject it explicitly instead of letting the
        // post-write cleanup delete the artifact we just created.
        if size_before > limits.artifact_max_total_bytes {
            return Err(TransferError::ArtifactWrite(format!(
                "transfer payload ({} bytes) exceeds the artifact budget ({} bytes)",
                size_before, limits.artifact_max_total_bytes
            )));
        }
        // Large sources (AC-15): stream source → mode-0600 temporary artifact
        // while hashing/counting, fsync, prove source stability, then atomic
        // rename into the artifact name. The payload is never fully held in
        // memory and never re-read for metadata. Temp names carry an atomic
        // sequence so concurrent captures can never collide (R2-17).
        store.ensure_root()?;
        let mut hasher = sha2::Sha256::new();
        let mut byte_length: u64 = 0;
        let mut newline_count: u64 = 0;
        let mut last_byte: u8 = 0;
        let temp_path = {
            static CAPTURE_SEQUENCE: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let sequence = CAPTURE_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            store.root.join(format!(
                ".capture-{}-{}-{sequence}.tmp",
                captured_at_ms,
                std::process::id()
            ))
        };
        let read_result = (|| -> Result<(), std::io::Error> {
            use std::io::{BufRead as _, BufReader, Write as _};
            let file = std::fs::File::open(&source.file_path)?;
            let mut reader = BufReader::with_capacity(256 * 1024, file);
            #[cfg(unix)]
            let mut out = {
                use std::os::unix::fs::OpenOptionsExt as _;
                std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .mode(0o600)
                    .open(&temp_path)?
            };
            #[cfg(not(unix))]
            let mut out = std::fs::File::create(&temp_path)?;
            loop {
                let chunk = reader.fill_buf()?;
                if chunk.is_empty() {
                    break;
                }
                hasher.update(chunk);
                newline_count += chunk.iter().filter(|byte| **byte == b'\n').count() as u64;
                if let Some(byte) = chunk.last() {
                    last_byte = *byte;
                }
                out.write_all(chunk)?;
                byte_length += chunk.len() as u64;
                let consumed = chunk.len();
                reader.consume(consumed);
            }
            out.sync_all()
        })();
        if let Err(error) = read_result {
            let _ = std::fs::remove_file(&temp_path);
            return Err(TransferError::SourceUnavailable(error.to_string()));
        }
        let (size_after, mtime_after) = match source_stat(&source.file_path) {
            Ok(stat) => stat,
            Err(error) => {
                let _ = std::fs::remove_file(&temp_path);
                return Err(error);
            }
        };
        if (size_after, mtime_after) != (size_before, mtime_ms) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(TransferError::SourceUnstable {
                path: source.file_path.clone(),
                reason: "size/mtime changed during capture".to_string(),
            });
        }
        if size_after != byte_length {
            let _ = std::fs::remove_file(&temp_path);
            return Err(TransferError::SourceUnstable {
                path: source.file_path.clone(),
                reason: format!("read {byte_length} bytes but source reports {size_after}"),
            });
        }
        let digest = hasher.finalize();
        let sha256 = hex_encode(&digest);
        let final_path =
            store
                .root
                .join(format!("transfer-{}-{}.bin", &sha256[..16], captured_at_ms));
        if let Err(error) = std::fs::rename(&temp_path, &final_path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(TransferError::ArtifactWrite(format!(
                "atomic artifact rename failed: {error}"
            )));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&final_path, std::fs::Permissions::from_mode(0o600)).map_err(
                |error| {
                    TransferError::ArtifactWrite(format!("artifact chmod 0600 failed: {error}"))
                },
            )?;
        }
        let record_count = records_from(newline_count, (byte_length != 0).then_some(last_byte));
        store.maybe_cleanup(limits);
        Ok(TransferSnapshot {
            meta: TransferSnapshotMeta {
                provider: source.agent,
                native_session_id: source.native_id.clone(),
                source_path: source.file_path.clone(),
                source_kind: TransferSourceKind::FileRecords,
                byte_length,
                record_count,
                mtime_ms,
                sha256: sha256.clone(),
                captured_at_ms,
            },
            payload: TransferPayload::Artifact(TransferArtifactRef {
                byte_length,
                sha256,
                captured_at_ms,
                path: final_path,
            }),
        })
    }
}

/// Resolve the complete payload bytes for a snapshot: inline directly, or the
/// hash-verified artifact content. Never truncates.
pub fn transfer_payload_bytes(
    snapshot: &TransferSnapshot,
    store: &TransferArtifactStore,
) -> Result<Vec<u8>, TransferError> {
    match &snapshot.payload {
        TransferPayload::Inline(data) => Ok(data.clone()),
        TransferPayload::Artifact(reference) => store.verify(reference),
    }
}

/// Verify a target can access the artifact path with its normal local Agent
/// tooling: the file must exist, be regular, and be readable by the current
/// user. Sandboxed/remote targets that cannot read local files must be marked
/// unavailable by the caller (fail closed; no permission broadening).
pub fn artifact_accessible(reference: &TransferArtifactRef) -> bool {
    match std::fs::metadata(&reference.path) {
        Ok(metadata) => {
            metadata.is_file() && {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt as _;
                    let mode = metadata.permissions().mode();
                    mode & 0o400 != 0
                }
                #[cfg(not(unix))]
                {
                    true
                }
            }
        }
        Err(_) => false,
    }
}

/// Deterministic transfer briefing: complete context plus the optional user
/// instruction, exactly once each. No model summarization intermediary exists
/// in this path.
pub fn build_transfer_briefing(
    snapshot: &TransferSnapshot,
    bytes: &[u8],
    instruction: Option<&str>,
    artifact: Option<&TransferArtifactRef>,
) -> String {
    let mut briefing = String::new();
    briefing.push_str(
        "You are continuing an existing conversation from another coding agent session. ",
    );
    briefing.push_str("The complete prior context follows.\n\n");
    briefing.push_str(&format!(
        "Source: {} session {} ({})\n",
        snapshot.meta.provider.display_name(),
        snapshot.meta.native_session_id,
        match snapshot.meta.source_kind {
            TransferSourceKind::FileRecords => "newline-delimited records",
        }
    ));
    briefing.push_str(&format!(
        "Integrity: sha-256 {}, {} bytes, {} records\n",
        snapshot.meta.sha256, snapshot.meta.byte_length, snapshot.meta.record_count
    ));
    briefing.push_str(
        "Rules: preserve the current workspace; do not redo completed work; \
         continue from the stopping point.\n\n",
    );
    match artifact {
        Some(reference) => {
            briefing.push_str(&format!(
                "Full context artifact: {}\n(sha-256 {}, {} bytes — read this file for the complete session record.)\n",
                reference.path.display(),
                reference.sha256,
                reference.byte_length
            ));
        }
        None => {
            briefing.push_str("Complete prior context:\n\n");
            briefing.push_str(&String::from_utf8_lossy(bytes));
            briefing.push_str("\n\n");
        }
    }
    if let Some(instruction) = instruction.map(str::trim).filter(|text| !text.is_empty()) {
        briefing.push_str(&format!("Continuation instruction: {instruction}\n"));
    }
    briefing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifacts_and_root_are_private_fail_closed_modes() {
        // AC-13: the store creates with private modes; verification is part of
        // the write contract, not a best-effort chmod.
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let reference = store
            .write(b"secret", now_ms(), &TransferLimits::default())
            .unwrap_or_else(|error| panic!("{error}"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let artifact_mode = std::fs::metadata(&reference.path)
                .ok()
                .map(|meta| meta.permissions().mode() & 0o777);
            assert_eq!(artifact_mode, Some(0o600));
            let root_mode = std::fs::metadata(store.root())
                .ok()
                .map(|meta| meta.permissions().mode() & 0o777);
            assert_eq!(root_mode, Some(0o700));
        }
    }

    #[test]
    fn oversized_payload_is_rejected_before_any_artifact_is_written() {
        // R2-19: a payload bigger than the total artifact budget fails with a
        // typed error instead of write-then-self-delete.
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let body = "0123456789".repeat(64);
        let src = source(dir.path(), "huge.jsonl", &body);
        let limits = TransferLimits {
            inline_limit_bytes: 0,
            artifact_max_total_bytes: 32,
            ..TransferLimits::default()
        };
        let error = match capture_transfer_snapshot(&src, &store, &limits) {
            Err(error) => error,
            Ok(_) => panic!("oversized payload must be rejected"),
        };
        assert!(error.to_string().contains("exceeds the artifact budget"));
        assert!(
            store.list_artifacts().unwrap_or_default().is_empty(),
            "nothing may be written for a rejected payload"
        );
    }

    #[test]
    fn large_source_streams_to_a_verified_artifact_with_exact_record_count() {
        // AC-15: the streaming path hashes/counts in one pass, proves source
        // stability, and the artifact verifies without a full-memory payload.
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let body = (0..5_000)
            .map(|index| format!("record-{index}\n"))
            .collect::<String>();
        let src = source(dir.path(), "big.jsonl", &body);
        let limits = TransferLimits {
            inline_limit_bytes: 0,
            ..TransferLimits::default()
        };
        let snapshot = capture_transfer_snapshot(&src, &store, &limits)
            .unwrap_or_else(|error| panic!("{error}"));
        let reference = match &snapshot.payload {
            TransferPayload::Artifact(reference) => reference,
            TransferPayload::Inline(_) => panic!("large source must not stay inline"),
        };
        assert_eq!(snapshot.meta.record_count, 5_000);
        assert_eq!(snapshot.meta.byte_length, body.len() as u64);
        let verified = store
            .verify(reference)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(String::from_utf8_lossy(&verified), body);
    }

    fn source(dir: &Path, name: &str, contents: &str) -> SessionFileRef {
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap_or_else(|error| panic!("write: {error}"));
        SessionFileRef {
            agent: AgentId::ClaudeCode,
            native_id: name.trim_end_matches(".jsonl").to_string(),
            file_path: path.to_string_lossy().into_owned(),
            mtime_ms: 0,
            size: contents.len() as i64,
        }
    }

    #[test]
    fn small_sources_travel_inline_with_exact_bytes_and_deterministic_hash() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let src = source(
            dir.path(),
            "s1.jsonl",
            "{\"type\":\"user\"}\n{\"type\":\"assistant\"}\n",
        );
        let before = std::fs::read(&src.file_path).unwrap_or_default();
        let limits = TransferLimits::default();
        let snapshot = capture_transfer_snapshot(&src, &store, &limits)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(snapshot.payload, TransferPayload::Inline(_)));
        assert_eq!(snapshot.meta.byte_length, before.len() as u64);
        assert_eq!(snapshot.meta.record_count, 2);
        let bytes = transfer_payload_bytes(&snapshot, &store).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(bytes, before, "inline payload must be byte-exact");
        // Deterministic hash across captures.
        let second = capture_transfer_snapshot(&src, &store, &limits)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(snapshot.meta.sha256, second.meta.sha256);
        // Source unchanged.
        assert_eq!(
            std::fs::read(&src.file_path).unwrap_or_default(),
            before,
            "capture must never mutate the provider source"
        );
    }

    #[test]
    fn large_sources_use_artifacts_with_exact_content_and_strict_permissions() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let big: String = "x".repeat(300 * 1024);
        let src = source(dir.path(), "big.jsonl", &big);
        let limits = TransferLimits::default();
        let snapshot = capture_transfer_snapshot(&src, &store, &limits)
            .unwrap_or_else(|error| panic!("{error}"));
        let TransferPayload::Artifact(reference) = &snapshot.payload else {
            panic!("large source must use an artifact");
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&reference.path)
                .unwrap_or_else(|error| panic!("{error}"))
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "artifact files must be 0600");
            let dir_mode = std::fs::metadata(store.root())
                .unwrap_or_else(|error| panic!("{error}"))
                .permissions()
                .mode();
            assert_eq!(dir_mode & 0o777, 0o700, "artifact dirs must be 0700");
        }
        let bytes = store.verify(reference).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(bytes.len(), big.len());
        assert!(artifact_accessible(reference));
        // Filenames never leak provider/session material.
        let name = reference
            .path
            .file_name()
            .unwrap_or_else(|| panic!("artifact has a filename"))
            .to_string_lossy();
        assert!(!name.contains("claude"));
        assert!(!name.contains("big.jsonl"));
    }

    #[test]
    fn corrupt_or_missing_artifacts_fail_closed() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let reference = store
            .write(b"payload", 1_000, &TransferLimits::default())
            .unwrap_or_else(|error| panic!("{error}"));
        std::fs::write(&reference.path, b"tampered").unwrap_or_else(|error| panic!("{error}"));
        let result = store.verify(&reference);
        assert!(matches!(result, Err(TransferError::ArtifactCorrupt { .. })));

        std::fs::remove_file(&reference.path).unwrap_or_else(|error| panic!("{error}"));
        let missing = store.verify(&reference);
        assert!(matches!(
            missing,
            Err(TransferError::ArtifactCorrupt { .. })
        ));
        assert!(!artifact_accessible(&reference));
    }

    #[test]
    fn cleanup_is_bounded_by_ttl_and_budget() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        // 120s old: expired under the test's 60s TTL, but young enough to
        // survive the write-time throttled cleanup (default TTL).
        let expired_captured_at = now_ms().saturating_sub(120_000);
        store
            .write(b"old", expired_captured_at, &TransferLimits::default())
            .unwrap_or_else(|error| panic!("{error}"));
        store
            .write(b"new", now_ms(), &TransferLimits::default())
            .unwrap_or_else(|error| panic!("{error}"));
        let limits = TransferLimits {
            inline_limit_bytes: DEFAULT_INLINE_LIMIT_BYTES,
            artifact_ttl_ms: 60_000,
            artifact_max_total_bytes: DEFAULT_ARTIFACT_MAX_TOTAL_BYTES,
            artifact_max_count: 64,
        };
        let removed = store.cleanup(&limits);
        assert_eq!(removed, 1, "expired artifact removed");
        let remaining = store.list_artifacts().unwrap_or_default();
        assert_eq!(remaining.len(), 1);

        let count_limits = TransferLimits {
            inline_limit_bytes: DEFAULT_INLINE_LIMIT_BYTES,
            artifact_ttl_ms: DEFAULT_ARTIFACT_TTL_MS,
            artifact_max_total_bytes: DEFAULT_ARTIFACT_MAX_TOTAL_BYTES,
            artifact_max_count: 0,
        };
        let removed = store.cleanup(&count_limits);
        assert_eq!(removed, 1, "count budget enforced oldest-first");
        assert_eq!(store.list_artifacts().unwrap_or_default().len(), 0);
    }

    #[test]
    fn briefing_contains_full_context_and_instruction_exactly_once() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let store = TransferArtifactStore::new(dir.path().join("artifacts"));
        let src = source(dir.path(), "s2.jsonl", "COMPLETE-CONTEXT-BODY\n");
        let snapshot = capture_transfer_snapshot(&src, &store, &TransferLimits::default())
            .unwrap_or_else(|error| panic!("{error}"));
        let bytes = transfer_payload_bytes(&snapshot, &store).unwrap_or_else(|e| panic!("{e}"));
        let briefing =
            build_transfer_briefing(&snapshot, &bytes, Some(" Continue with Codex "), None);
        assert!(briefing.contains("COMPLETE-CONTEXT-BODY"));
        assert_eq!(briefing.matches("Continuation instruction:").count(), 1);
        assert_eq!(briefing.matches("Continue with Codex").count(), 1);
        assert!(briefing.contains("do not redo completed work"));

        // Empty instruction is a valid transfer.
        let empty = build_transfer_briefing(&snapshot, &bytes, Some("   "), None);
        assert!(!empty.contains("Continuation instruction:"));

        // Artifact mode references the file with integrity metadata.
        let big: String = "y".repeat(300 * 1024);
        let src = source(dir.path(), "big2.jsonl", &big);
        let snapshot = capture_transfer_snapshot(&src, &store, &TransferLimits::default())
            .unwrap_or_else(|error| panic!("{error}"));
        let TransferPayload::Artifact(reference) = &snapshot.payload else {
            panic!("expected artifact");
        };
        let briefing = build_transfer_briefing(&snapshot, b"", Some("go"), Some(reference));
        assert!(briefing.contains("Full context artifact:"));
        assert!(briefing.contains(&reference.sha256));
    }

    #[test]
    fn record_count_handles_partial_trailing_lines() {
        assert_eq!(count_records(b""), 0);
        assert_eq!(count_records(b"a\n"), 1);
        assert_eq!(count_records(b"a\nb\n"), 2);
        assert_eq!(count_records(b"a\nb"), 2);
    }
}
