// SPDX-License-Identifier: MIT
// Incremental scan flow retains upstream MIT-derived semantics.
//! [INPUT]: A constructed adapter slice and the Shardlane-owned
//! HistoryCatalog.
//! [OUTPUT]: Incremental/full scan reports, FTS metadata, the transcript
//! page-cache, and missing-source cleanup.
//! [POS]: Background indexing coordinator of the history core; reads no
//! configuration, creates no runtime, performs no external writes.

use crate::adapters::{units_from_messages, AgentHistoryAdapter};
use crate::catalog::HistoryCatalog;
use crate::models::{ParsedTranscript, SessionFileRef, SessionMeta};
use anyhow::Result;
use std::collections::HashSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub discovered: usize,
    pub parsed: usize,
    pub prewarmed: usize,
    pub cache_evicted: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub errors: Vec<String>,
}

const LEGACY_PAGE_CACHE_BACKFILL_LIMIT: usize = 2;
const LEGACY_PAGE_CACHE_BACKFILL_BYTES: i64 = 8 * 1024 * 1024;

pub fn scan(
    adapters: &[Box<dyn AgentHistoryAdapter>],
    catalog: &mut HistoryCatalog,
    full: bool,
) -> Result<ScanReport> {
    let known = catalog.known_files()?;
    let mut report = ScanReport::default();
    let mut seen_paths = HashSet::new();
    let mut discovery_failed = false;

    for adapter in adapters {
        let mut references = match adapter.list_session_files() {
            Ok(references) => references,
            Err(error) => {
                report.errors.push(format!(
                    "{} discovery failed: {error}",
                    adapter.agent().display_name()
                ));
                // Never purge the catalog on a round with incomplete
                // enumeration: missing entries in seen_paths do not mean the
                // source files vanished.
                discovery_failed = true;
                continue;
            }
        };
        references.sort_by_key(|reference| std::cmp::Reverse(reference.mtime_ms));
        let discovered = references.len();
        report.discovered += discovered;
        for reference in &references {
            seen_paths.insert(reference.file_path.clone());
        }
        let changed = references
            .into_iter()
            .filter(|reference| {
                full || known
                    .get(&reference.file_path)
                    .is_none_or(|mtime| *mtime != reference.mtime_ms)
            })
            .collect::<Vec<_>>();
        report.unchanged += discovered.saturating_sub(changed.len());
        if changed.is_empty() {
            continue;
        }
        let quick = adapter.quick_meta(&changed);

        for reference in changed {
            let quick = quick
                .as_ref()
                .and_then(|quick| quick.get(&reference.file_path));
            let outcome = prewarm_transcript(
                adapter.as_ref(),
                catalog,
                &reference,
                |parsed_meta| match quick {
                    Some(quick) => adapter.merge_quick_meta(parsed_meta, quick),
                    None => parsed_meta,
                },
                |catalog, meta, transcript| {
                    let units = units_from_messages(&transcript.mainline);
                    catalog.write_session(meta, reference.mtime_ms, &units)
                },
            );
            match outcome {
                Ok(()) => report.parsed += 1,
                Err(PrewarmStepError::Parse(error)) => report.errors.push(format!(
                    "{} parse failed for {}: {error}",
                    adapter.agent().display_name(),
                    reference.file_path
                )),
                Err(PrewarmStepError::Persist(error)) => report.errors.push(format!(
                    "{} catalog write failed for {}: {error}",
                    adapter.agent().display_name(),
                    reference.file_path
                )),
                // The transcript is indexed even when its page-cache insert
                // fails; a later prewarm round rebuilds the cache.
                Err(PrewarmStepError::CacheWrite(error)) => {
                    report.parsed += 1;
                    report.errors.push(format!(
                        "{} transcript page-cache write failed for {}: {error}",
                        adapter.agent().display_name(),
                        reference.file_path
                    ));
                }
            }
        }
    }

    // When a source is temporarily unreadable (e.g. an Agent CLI is writing
    // its database), a mistaken purge would delete that Agent's entire index,
    // FTS, and page cache; any round with a failed enumeration skips cleanup.
    report.removed = if discovery_failed {
        0
    } else {
        catalog.remove_missing(&seen_paths)?
    };

    if !full {
        for (meta, reference) in catalog.uncached_transcript_sources(
            LEGACY_PAGE_CACHE_BACKFILL_LIMIT,
            LEGACY_PAGE_CACHE_BACKFILL_BYTES,
        )? {
            let Some(adapter) = adapters
                .iter()
                .find(|adapter| adapter.agent() == meta.agent)
            else {
                continue;
            };
            // The legacy loop receives its meta already resolved and performs
            // no catalog write; only the page cache is (re)built.
            match prewarm_transcript(
                adapter.as_ref(),
                catalog,
                &reference,
                |_| meta.clone(),
                |_, _, _| Ok(()),
            ) {
                Ok(()) => report.prewarmed += 1,
                Err(PrewarmStepError::Parse(error)) => report.errors.push(format!(
                    "{} legacy transcript prewarm parse failed for {}: {error}",
                    adapter.agent().display_name(),
                    reference.file_path
                )),
                Err(PrewarmStepError::Persist(error)) => report.errors.push(format!(
                    "{} legacy transcript catalog write failed for {}: {error}",
                    adapter.agent().display_name(),
                    reference.file_path
                )),
                Err(PrewarmStepError::CacheWrite(error)) => report.errors.push(format!(
                    "{} legacy transcript page-cache prewarm failed for {}: {error}",
                    adapter.agent().display_name(),
                    reference.file_path
                )),
            }
        }
    }

    // Progressively backfill Description for legacy rows (excerpted from the
    // first FTS messages); bounded batches until no rows are missing.
    match catalog.backfill_session_descriptions() {
        Ok(_) => {}
        Err(error) => report
            .errors
            .push(format!("description backfill failed: {error}")),
    }

    report.cache_evicted = catalog.prune_transcript_page_cache()?;
    Ok(report)
}

/// Failure modes of [`prewarm_transcript`]. The two scan loops count
/// successes differently, so the failure kind stays typed until the caller
/// formats its own scan-error line.
enum PrewarmStepError {
    Parse(anyhow::Error),
    Persist(anyhow::Error),
    CacheWrite(anyhow::Error),
}

/// Shared parse → meta-resolution → catalog persist → transcript page-cache
/// sequence for both scan loops (changed sources and legacy prewarm).
/// `resolve_meta` maps the parsed meta onto the meta that gets persisted;
/// `persist` is the loop-specific catalog write (a pass-through for the
/// legacy prewarm loop, whose meta is already resolved). `Ok` guarantees the
/// transcript reached the page cache.
fn prewarm_transcript(
    adapter: &dyn AgentHistoryAdapter,
    catalog: &mut HistoryCatalog,
    reference: &SessionFileRef,
    resolve_meta: impl FnOnce(SessionMeta) -> SessionMeta,
    persist: impl FnOnce(&mut HistoryCatalog, &SessionMeta, &ParsedTranscript) -> Result<()>,
) -> Result<(), PrewarmStepError> {
    let mut transcript = adapter
        .parse_transcript(reference)
        .map_err(PrewarmStepError::Parse)?;
    let meta = resolve_meta(transcript.meta.clone());
    transcript.meta = meta.clone();
    persist(catalog, &meta, &transcript).map_err(PrewarmStepError::Persist)?;
    catalog
        .cache_transcript(&meta.key, reference, &transcript)
        .map_err(PrewarmStepError::CacheWrite)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::claude::ClaudeAdapter;
    use crate::models::{AgentId, ParsedSession, ParsedTranscript, SessionFileRef};
    use std::fs;
    use std::path::PathBuf;

    /// Adapter whose enumeration fails: the root exists but is unreadable, so
    /// the scanner must skip this round's cleanup.
    struct FailingDiscoveryAdapter;

    impl AgentHistoryAdapter for FailingDiscoveryAdapter {
        fn agent(&self) -> AgentId {
            AgentId::ClaudeCode
        }

        fn detect(&self) -> bool {
            true
        }

        fn list_session_files(&self) -> Result<Vec<SessionFileRef>> {
            Err(anyhow::anyhow!("simulated unreadable source"))
        }

        fn parse_session(&self, _reference: &SessionFileRef) -> Result<ParsedSession> {
            Err(anyhow::anyhow!("unused in this test"))
        }

        fn parse_transcript(&self, _reference: &SessionFileRef) -> Result<ParsedTranscript> {
            Err(anyhow::anyhow!("unused in this test"))
        }

        fn watch_paths(&self) -> Vec<PathBuf> {
            Vec::new()
        }

        fn with_custom_root(&self, _root: PathBuf) -> Box<dyn AgentHistoryAdapter> {
            Box::new(Self)
        }

        fn data_roots(&self) -> Vec<PathBuf> {
            Vec::new()
        }
    }

    const DEMO_JSONL: &str = concat!(
        r#"{"type":"user","cwd":"/work/demo","timestamp":"2026-08-01T01:00:00Z","message":{"content":"hello history"}}"#,
        "\n",
        r#"{"type":"assistant","cwd":"/work/demo","timestamp":"2026-08-01T01:00:01Z","message":{"id":"m1","model":"claude-test","content":[{"type":"text","text":"hello back"}]}}"#,
        "\n",
    );

    #[test]
    fn discovery_failure_skips_catalog_purge() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let project = temp.path().join("project");
        fs::create_dir_all(&project)?;
        fs::write(project.join("session.jsonl"), DEMO_JSONL)?;
        let real: Vec<Box<dyn AgentHistoryAdapter>> = vec![Box::new(ClaudeAdapter::with_root(
            temp.path().to_path_buf(),
        ))];
        let mut catalog = HistoryCatalog::memory()?;
        scan(&real, &mut catalog, false)?;
        assert_eq!(catalog.list_sessions(100)?.len(), 1);

        let failing: Vec<Box<dyn AgentHistoryAdapter>> = vec![Box::new(FailingDiscoveryAdapter)];
        let report = scan(&failing, &mut catalog, false)?;
        assert!(!report.errors.is_empty());
        assert_eq!(report.removed, 0);
        // Source unreadable ≠ source gone: already-indexed sessions must
        // remain.
        assert_eq!(catalog.list_sessions(100)?.len(), 1);
        Ok(())
    }

    #[test]
    fn missing_root_still_purges() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let project = temp.path().join("project");
        fs::create_dir_all(&project)?;
        fs::write(project.join("session.jsonl"), DEMO_JSONL)?;
        let root = temp.path().to_path_buf();
        let adapters: Vec<Box<dyn AgentHistoryAdapter>> =
            vec![Box::new(ClaudeAdapter::with_root(root.clone()))];
        let mut catalog = HistoryCatalog::memory()?;
        scan(&adapters, &mut catalog, false)?;
        assert_eq!(catalog.list_sessions(100)?.len(), 1);

        // The user deleted the root directory: a legitimate empty set, so the
        // cleanup semantics stay correct.
        std::fs::remove_dir_all(&root)?;
        let report = scan(&adapters, &mut catalog, false)?;
        assert_eq!(report.removed, 1);
        assert_eq!(catalog.list_sessions(100)?.len(), 0);
        Ok(())
    }

    #[test]
    fn second_scan_skips_unchanged_history() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let project = temp.path().join("project");
        fs::create_dir_all(&project)?;
        fs::write(
            project.join("session.jsonl"),
            concat!(
                r#"{"type":"user","cwd":"/work/demo","timestamp":"2026-08-01T01:00:00Z","message":{"content":"hello history"}}"#,
                "\n",
                r#"{"type":"assistant","cwd":"/work/demo","timestamp":"2026-08-01T01:00:01Z","message":{"id":"m1","model":"claude-test","content":[{"type":"text","text":"hello back"}]}}"#,
                "\n",
            ),
        )?;
        let adapters: Vec<Box<dyn AgentHistoryAdapter>> = vec![Box::new(ClaudeAdapter::with_root(
            temp.path().to_path_buf(),
        ))];
        let mut catalog = HistoryCatalog::memory()?;

        let first = scan(&adapters, &mut catalog, false)?;
        assert_eq!(first.discovered, 1);
        assert_eq!(first.parsed, 1);
        assert_eq!(first.unchanged, 0);
        let session = catalog
            .list_sessions(1)?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing scanned session"))?;
        let source = catalog
            .transcript_source(&session.key)?
            .ok_or_else(|| anyhow::anyhow!("missing scanned source identity"))?;
        let cached = catalog
            .cached_transcript_window(&session.key, &source, 0, 60)?
            .ok_or_else(|| anyhow::anyhow!("scanner did not prewarm transcript page cache"))?;
        assert_eq!(cached.messages.len(), 2);
        assert_eq!(cached.messages[0].text, "hello history");

        let second = scan(&adapters, &mut catalog, false)?;
        assert_eq!(second.discovered, 1);
        assert_eq!(second.parsed, 0);
        assert_eq!(second.unchanged, 1);
        assert_eq!(catalog.search("hello history", 10)?.len(), 1);
        Ok(())
    }

    #[test]
    fn unchanged_legacy_session_is_backfilled_without_reindexing() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let project = temp.path().join("project");
        fs::create_dir_all(&project)?;
        fs::write(
            project.join("legacy.jsonl"),
            concat!(
                r#"{"type":"user","cwd":"/work/demo","timestamp":"2026-08-01T01:00:00Z","message":{"content":"legacy history"}}"#,
                "\n",
                r#"{"type":"assistant","cwd":"/work/demo","timestamp":"2026-08-01T01:00:01Z","message":{"id":"m1","content":[{"type":"text","text":"legacy response"}]}}"#,
                "\n",
            ),
        )?;
        let adapter = ClaudeAdapter::with_root(temp.path().to_path_buf());
        let reference = adapter
            .list_session_files()?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing legacy source"))?;
        let parsed = adapter.parse_session(&reference)?;
        let mut catalog = HistoryCatalog::memory()?;
        catalog.write_session(&parsed.meta, reference.mtime_ms, &parsed.units)?;
        assert!(catalog
            .cached_transcript_window(&parsed.meta.key, &reference, 0, 60)?
            .is_none());
        let adapters: Vec<Box<dyn AgentHistoryAdapter>> = vec![Box::new(adapter)];

        let report = scan(&adapters, &mut catalog, false)?;
        assert_eq!(report.parsed, 0);
        assert_eq!(report.unchanged, 1);
        assert_eq!(report.prewarmed, 1);
        let cached = catalog
            .cached_transcript_window(&parsed.meta.key, &reference, 0, 60)?
            .ok_or_else(|| anyhow::anyhow!("legacy transcript was not backfilled"))?;
        assert_eq!(cached.messages.len(), 2);
        Ok(())
    }

    #[test]
    #[ignore = "local performance smoke; run explicitly on the development Mac"]
    fn scanner_prewarms_large_transcript_without_interactive_parse_budget_regression() -> Result<()>
    {
        use std::time::Instant;

        let temp = tempfile::tempdir()?;
        let project = temp.path().join("project");
        fs::create_dir_all(&project)?;
        let session_path = project.join("large.jsonl");
        let mut jsonl = String::with_capacity(2_000_000);
        for index in 0..10_000 {
            jsonl.push_str(&format!(
                "{{\"type\":\"user\",\"cwd\":\"/work/demo\",\"timestamp\":\"2026-08-01T01:00:00Z\",\"message\":{{\"content\":\"message-{index}-{}\"}}}}\n",
                "x".repeat(128)
            ));
        }
        fs::write(&session_path, jsonl)?;
        let adapters: Vec<Box<dyn AgentHistoryAdapter>> = vec![Box::new(ClaudeAdapter::with_root(
            temp.path().to_path_buf(),
        ))];
        let mut catalog = HistoryCatalog::memory()?;

        let started = Instant::now();
        let report = scan(&adapters, &mut catalog, false)?;
        let elapsed = started.elapsed();
        let elapsed_ms = elapsed.as_secs_f64() * 1_000.0;
        eprintln!(
            "history scan prewarm: messages=10000 parsed={} {:.2}ms",
            report.parsed, elapsed_ms
        );
        assert_eq!(report.parsed, 1);
        assert!(
            elapsed_ms < 5_000.0,
            "10k-message scan+FTS+page-cache took {elapsed_ms:.2}ms"
        );
        let session = catalog
            .list_sessions(1)?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing perf session"))?;
        let source = catalog
            .transcript_source(&session.key)?
            .ok_or_else(|| anyhow::anyhow!("missing perf source identity"))?;
        let cached = catalog
            .cached_transcript_window(&session.key, &source, 9_940, 60)?
            .ok_or_else(|| anyhow::anyhow!("missing prewarmed perf window"))?;
        assert_eq!(cached.messages.len(), 60);
        Ok(())
    }
}
