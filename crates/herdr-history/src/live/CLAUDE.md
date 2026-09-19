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
- `journal.rs` 是 provider 中立的 Hook Journal 解码器（2026-09-19，
  `LiveCapability::HookJournal`）：消费 Host hook 适配层写入的归一化
  JSONL（版本化 `v` 字段，未知版本/kind 一律计入 unknown——fail-closed），
  只投影 user_prompt/assistant_message 为消息；journal 文件由
  shardlane-host `agent_hooks::adapter` 拥有，本模块只读不写。轮转
  （8MiB 上限）会重置该会话的 Chat 可见历史——durable 全量 transcript
  由规划中的 AntigravityAdapter v2 承担，不在本层拼接双代。
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
- Facts ride along on every content-carrying sync (`LiveSync.facts`, #2
  2026-09-19) so ephemeral projections (pending approvals) reach consumers
  in incremental mode too: `facts.pending_approval` (`LiveApproval`) is
  decoded by the codex adapter from rollout `exec_approval_request` /
  `apply_patch_approval_request` event_msg rows and cleared by the next
  turn-progress evidence. Option keys come only from the verified Codex TUI
  keymap contract (vendor defaults plus CODEX_HOME config.toml overrides);
  adapters that cannot derive options/keys stably must not emit an approval
  at all — never-blind-send, no menu guessing.
