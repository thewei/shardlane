# live/ — Claude/Codex/Pi incremental semantic sources

Semantic source layer for Phase-1 live Chat: exact binding + incremental tail
+ line-by-line decoding of Claude Code / Codex / Pi (including the isomorphic
Omp fork) session files running inside the Herdr TUI.

## Ownership

- This module is only responsible for: byte cursors after `(agent,
  native_session_id)` exact source binding, partial-line buffering, append
  decoding, truncate/replace resets, generation guards, and idempotent upsert
  change records.
- **Interpretation rules are not re-implemented here**: the line-level
  interpretation state machines (`ClaudeSession` / `CodexSession` /
  `PiSession`) live in `src/adapters/{claude,codex,pi}.rs`; full parsing and
  live increments share the same `feed_line` path, so
  `parse_full(fixture) == incremental feed + settle` holds by construction
  (contract tests in `tests.rs`).
- The render/GUI layer may only consume `LiveSnapshot` (`messages` + `facts`
  + `generation`); `open`/`sync` I/O must happen in a background task, and
  results may only be applied after generation validation.

## Hard boundaries

- Never starts/stops any provider process; no TUI/ANSI screen inference; no
  writes to any external Agent file.
- Source files must come from `HistoryCatalog::session_source_by_native` (or
  an equivalent exact lookup); cwd/mtime heuristic guessing is forbidden.
- Normal appends read only `[consumed, size)` bytes; duplicate FS wakes
  coalesce into `Unchanged` at `classify` and are never consumed twice.
- Same-length in-place rewrites are not detected (recorded limitation);
  shrinking always rebuilds wholesale as a truncate/replace (generation +1)
  and never splices across generations.
- `settle()` interprets a trailing partial line as a complete line,
  equivalent to full parsing; never settles while the file keeps growing
  (waits for the next newline).
