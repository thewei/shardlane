# Shardlane MyGo 0.3.0 — Code & Package Audit

Status: **audited test build**
Audit date: 2026-10-05
Branch: `rewrite/mygo`
Architecture authority: `docs/client-product-architecture.md`
Execution rules: `docs/mygo-native-execution-rules.md`
Scope plan: `docs/mygo-native-0.3.0-plan.md`

## 1. Verdict

**0.3.0 delivers the planned Native Settings and read-only Native History workflow on the 0.2.0 foundation. It remains an internal test build.**

New in this version:

- Settings are real: `SettingsService` (`internal/app`) persists through the
  versioned atomic `FileStore` in the MyGo user-data directory; General
  (appearance, restore-window) and Terminal (font family/size/line height,
  scrollback, Option-as-Alt) are live Native UI backed by the service, and
  terminal presentation options map onto newly attached Herdr Panes without
  touching Herdr terminal identity.
- History is real and read-only: Codex rollout adapter (Rust fixture parity),
  SQLite/FTS5 catalog (`modernc.org/sqlite`, pure Go) with source-identity
  invalidation, page-addressable transcript cache (64-message pages,
  seq→index), background scanner (missing root = empty; unreadable root
  aborts without cleanup), and a `HistoryService` (`List/Recent/Open/Search`)
  with bounded 60-message windows, 12-message overlap paging, anchor-seq
  search jumps, and cache-miss parse-once fallback.
- Native `/history` list (search, provider filter, loading/error/empty,
  bounded rows) and `/history/{id}` detail (USER/ASSISTANT/SYSTEM blocks,
  tool cards, thinking collapsed by default, load earlier/later) render from
  the service; Sidebar Recent is fed from session metadata only.
- Agent 0.3 prework landed UI-independent: provider capability projection
  (`internal/agent`, Rust-authority parity), StartAgent request DTO with
  explicit targets, and the idempotency `LaunchRegistry` (one RequestID → at
  most one launch). Start Agent remains disabled in the UI.
- Chat remains out of scope by plan.

## 2. Audit matrix

| Area | Result | Evidence / current state |
|---|---|---|
| 0.2.0 shell/terminal foundation | ✅ PASS | unchanged; full suite green |
| Settings storage core | ✅ PASS (0.2.0) | `internal/settings` versioned atomic store |
| SettingsService (SET-03) | ✅ PASS | load-failure keeps defaults; serialized updates; validation ranges |
| FileStore injection (SET-04) | ✅ PASS | `main.go` resolves MyGo `PathUserData`; isolated smoke created/used it |
| General settings UI (SET-05) | ✅ PASS | headless UI: appearance segmented + restore toggle persist through service |
| Terminal settings UI (SET-06) | ✅ PASS | headless UI: font/size/line-height/scrollback/option-as-alt persist |
| Terminal presentation apply (SET-07) | ✅ PASS | `terminalOptionsFromSettings` mapping test; applied to newly attached Panes only |
| Preferences survive restart (SET-08) | ✅ PASS | relaunch-over-same-FileStore integration test |
| Codex adapter (HIST-05) | ✅ PASS | rollout fixture parity: meta/tool output resolution/tokens/fallback/reasoning hosts |
| SQLite catalog + identity (HIST-07A/B) | ✅ PASS | schema mirrors Rust authority; upsert; identity-invalidates pages together |
| Page cache + seq index (HIST-07C) | ✅ PASS | 64-message pages; window reads bounded; corrupt payload = disposable miss |
| FTS/search (HIST-08) | ✅ PASS | trigram FTS5 + short-term LIKE fallback; single non-CJK char = no search; scoped filters |
| Scanner (HIST-09A) | ✅ PASS | changed/unchanged/missing-root/unreadable-root/cancellation tests |
| HistoryService (HIST-09B/C) | ✅ PASS | list/recent metadata-only; open window ≤ 60; anchor-seq jump; cache-miss parse-once |
| Native History list (HIST-10) | ✅ PASS | real sessions, search, provider filter, loading/error/empty states (headless + rendered frames) |
| Detail block model (HIST-11) | ✅ PASS | 1,800 chars/18 lines message, 700 chars/10 lines thinking bounds, Unicode-safe |
| Native detail + paging (HIST-12A/B) | ✅ PASS | USER/ASSISTANT/tool blocks; earlier/later with 12 overlap; bounded-window footer |
| Cancellation on switch (HIST-12C) | ✅ PASS | generation guard + context cancel; stale results dropped (deterministic tests) |
| Sidebar Recent real data (HIST-14) | ✅ PASS | fed from `Recent()` metadata only; no transcript parse; bounded to 8 |
| Gemini adapter (HIST-06) | ❌ NOT DONE (deferred by plan) | optional only if it did not delay the MVP; it did |
| Agent capability model (AGENT-01) | ✅ PASS (prework) | Rust-authority parity table tests |
| StartAgent DTO (AGENT-02) | ✅ PASS (prework) | explicit targets, wire round-trip, validation |
| Idempotency (AGENT-03) | ✅ PASS (prework) | `LaunchRegistry`: duplicate RequestID never re-launches |
| Uncertain-delivery harness (AGENT-06A) | ❌ NOT DONE (deferred) | needs the fake Herdr socket harness |
| Start Agent enabled | ❌ NOT DONE (by plan) | full launch transaction not ported; UI stays disabled |
| Chat / Conversation | ❌ NOT DONE (by plan) | 0.4+ |
| Remote/Mobile v2 | ❌ NOT DONE | Rust remains compatibility authority |
| Windows/Linux | ❌ NOT DONE | macOS arm64 only |

## 3. Verification performed

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...      # PASS (app, applog, agent, herdr, history, nativeui, settings)
GOTOOLCHAIN=go1.27.1 go tool mygo build # PASS
git diff --check                        # PASS
```

### Isolated real-app smoke (temporary HOME, seeded provider sources)

Launched `Shardlane.app` (0.3.0) via LaunchServices with a temporary `HOME`
seeded with two Codex rollout fixtures under `~/.codex/sessions`. Ground
truth after launch:

1. `history.sqlite3` created under the isolated MyGo user-data directory;
2. both seeded sessions indexed with correct titles/project names/message
   counts, ordered by `updated_at`;
3. FTS populated (4 units), page cache populated (2 pages), seq index present;
4. message search (`MATCH 'smoke'`) returns the expected snippet;
5. process stayed alive; the user's running instance was untouched.

### Privacy / bounded-log audit

- the isolated session log is 819 bytes;
- transcript bodies, prompts, and titles-seeded-as-content do not appear in
  logs (grep over the isolated log returned zero matches);
- scanner logging carries operation names, session keys, and counts only.

### Forbidden-surface audit

- no WebView/WKWebView/xterm/React/Vite anywhere under `next/`;
- no `workspace.focus` / `tab.focus` / `pane.focus` client navigation (only
  the negative tests that pin their exclusion);
- no global focus-event subscriptions (negative test pins the exclusion);
- no `--takeover` in automatic attach (negative test pins it);
- no client-owned PTY/runtime (only `exec` use is Herdr CLI discovery in the
  adapter).

### Native UI render evidence

`SHARDLANE_RENDER_SMOKE=<dir> go test -run TestRenderSmokeEvidence` renders
real frames of workspace/history list/history detail/settings General/
settings Terminal from the actual presentation code (headless, no screen
capture). Frames confirmed: list rows with provider badge/project/timestamp,
detail blocks with tool cards and bounded-window footer, Sidebar Recent fed
from the service, and all five Terminal settings controls with live values.

Manual-only (not machine-verifiable here): real-app visual click-through of
History/Settings and terminal font application on new Panes; the Computer
Use connector was unavailable in this session and the native input fallback
was deliberately not used.

## 4. Test package

- DMG: `next/build/darwin-arm64/Shardlane 0.3.0.dmg` (~7.0 MB)
- App: `next/build/darwin-arm64/Shardlane.app` (~18.5 MB)
- SHA-256: `75d36f6465afef2369696053d37144ae5d3d167d0aef206ccd9ad7f0b99eacab`
- version `0.3.0`, identifier `com.whstudio.shardlane.next`, macOS 13.0+,
  ad-hoc signature, not notarized.

## 5. What to test manually in 0.3.0

1. Settings → General appearance switching (System/Light/Dark) and relaunch persistence;
2. Settings → Terminal edits surviving relaunch and applying to newly split Panes;
3. `/history` list against real Claude/Codex sources: search, provider filter, refresh;
4. `/history/{id}` detail: thinking disclosure, tool cards, earlier/later paging;
5. Sidebar Recent routing into `/history/{id}`;
6. corrupting `settings.json` and confirming startup falls back to defaults;
7. the standing 0.2.0 terminal/shell checklist (unchanged).

## 6. Release recommendation

**Distribute 0.3.0 only as an internal test build.** The Rust client remains
the shipped default. 0.4+ candidates: Gemini adapter, full Agent launch
transaction (readiness + uncertain delivery) behind the idempotency seam,
Chat.
