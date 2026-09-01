# shardlane-history
> L2 | Parent: ../../CLAUDE.md

Read-only Agent history core owned by Shardlane. This crate has no GPUI or Herdr runtime dependency.

Members:
- `src/models.rs` — normalized Agent/session/transcript/search domain types.
- `src/adapters/` — external Agent format boundary; readers only. Supported formats: claude, codex, copilot, cursor, opencode, command-code, kiro, gemini, pi (omp shares the pi parser with the `~/.omp` root), grok, kimi, antigravity (SQLite summaries store; body stays encrypted), dsh (zstd multi-frame event log), qoder (active-leaf JSONL under `~/.qoder/projects`). Shared parsing helpers live in `parse_utils.rs` (`blocks_text`/`MtimeCache` included); read-only SQLite access lives in `sqlite_ro.rs`.
- `src/sources.rs` — `ApplicationConfig`-owned `HistorySourcePolicy`, enabled/disabled default and custom location snapshots, and one adapter-roster generation shared by scanner/watch/detail/export/live routing; it never writes the disposable catalog.
- `src/catalog.rs` — Shardlane-owned SQLite/FTS catalog plus disposable page-addressable transcript cache (`transcript_page_meta` / `transcript_page_cache` / `transcript_message_index`) and CJK-aware short-query policy; never an external Agent database.
- `src/scanner.rs` — parse/index orchestration; mtime-incremental with missing-source reconciliation, and changed-source transcript page-cache prewarming from the same adapter parse pass.
- `src/transfer.rs` — lossless read-only transfer contract: complete-source snapshots with SHA-256/record/bounds metadata, Shardlane-owned 0700/0600 artifact store with TTL/size-bounded cleanup and hash-verified fail-closed reads, and the deterministic full-context briefing builder (no model summarization). Never reuses bounded TranscriptMessage as transfer truth.
- `src/watcher.rs` — file-source dirty signal only; it emits a bounded async wake and never parses or writes the catalog.
- `src/resume.rs` — pure ResumeIntent/command mapping and POSIX quoting; no filesystem/process/runtime ownership.

Rules:
- External Agent files/databases are read-only. No delete/archive/write API belongs here.
- Parser details stay inside adapters and never leak into the Shardlane shell.
- The catalog may write only its own Shardlane database; transcript cache entries are derived/disposable and invalidated by stable source identity + mtime/size. New cache writes are page-addressable; the old full-transcript blob is legacy-read-only and may only be consumed once for migration into page cache. If indexing already parses a changed source, reuse that exact transcript to prewarm the page cache instead of reparsing it when the user opens History.
- Host CLI discovery/project validation belongs to `crates/herdr-gui/src/agent_cli.rs`; Continue runtime execution belongs to the Herdr integration boundary, not this crate.
- Add a synthetic adapter contract test for every supported source-format change.
- Unknown or unreliable resume semantics remain unsupported rather than guessed.
