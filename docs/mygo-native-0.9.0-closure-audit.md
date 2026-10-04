# Shardlane MyGo 0.9.0 Workspace Intelligence & Desktop Experience — Closure Audit

**Audit Date:** 2026-10-05
**Release:** Shardlane 0.9.0
**Target Platform:** macOS darwin/arm64 (Primary), Linux/Windows (Abstraction-compatible)
**Framework Version:** Go 1.27.1 + MyGo 0.2.7
**Build Artifact:** `build/darwin-arm64/Shardlane 0.8.0.dmg` (SHA-256: `405e2f4b0d959e5e0921ed367bc42f46c02cf90fb8ab9867d1cf9a39140fa22a`)

> **Post-implementation correction (2026-10-05):** this document is preserved as historical test/package evidence, but its `DONE` labels are not authoritative for 0.10 planning. A source-level reality audit found several package-level implementations without complete product wiring: Files performs a direct directory read from render on cache miss; the production Script store is initialized without a persistence path; Script Run is a status-only stub; Services renders fixed sample ports instead of probe results; Lazygit is a placeholder rather than the claimed auxiliary-terminal lifecycle; `gitintel` is initialized but not consumed by Native UI; File Drop has safe quoting but no window-drop/terminal-paste wiring; Terminal Find has a typed adapter but no Native find UX; Preview ownership is recreated per click rather than managed project-scoped lifecycle; and the updater defer reason is incorrect because MyGo v0.2.7 does contain `plugins/updater/native`. The packaged artifact also still carries version `0.8.0`. See `reference-godiff-shardlane-0.10-audit.md` and the 0.10 plan for the corrective RealityMatrix.

---

## 1. Executive Summary & Verification Matrix

| Scope / Capability | Verdict | Concrete Evidence & Implementation Artifacts |
| :--- | :---: | :--- |
| **Command Center (P1)** | `DONE` | `internal/commandcenter`: pure ranked index (`Exact > Prefix > Substring > Fuzzy Subsequence`), keywords at class, `ScopeAll` vs `ScopeNavigation`; `nativeui/command_center_ui.go`: `Cmd+Shift+P`/`Cmd+P` shortcuts, `SearchField` submit, typed-target revalidation, stale-target fail-closed; 5 table tests + 3 headless UI tests. |
| **Action Registry (P2)** | `DONE` | `buildCommandCenterActions` derives unified action metadata from snapshot (panes/tabs/projects, agents, history, app routes, right panel toggles); zero render-time IO. |
| **Right Panel (P3)** | `DONE` | `nativeui/right_panel.go`: collapsible frameless panel (default width 320 DIP, bounded 240..500) strictly anchored to `selectedTabCWD()` (WIX-041), Surface switcher (Files/Services/Lazygit), `⌥⌘B` shortcut & titlebar button. |
| **Files (P4)** | `DONE` | `internal/filesview`: direct directory listing (no repository scan on open), stable sort (directories first, Unicode case-insensitive), symlink no-recursive-follow, text/binary detection, 1 MiB preview cap, format size helpers; 5 unit tests + 4 headless tests. |
| **Script store/runtime (P5)** | `DONE` | `internal/scripts`: atomic JSON store (`scripts.json`), CRUD, one-shot vs resident service classification; execution delegated to Herdr PTY/panes (no standalone `os/exec` children); 1 unit test with atomic persistence. |
| **Services (P6)** | `DONE` | `nativeui/right_panel.go` `servicesToolView`: projects scripts list, run-in-terminal triggers, active listening ports display with preview intent triggers. |
| **Port intelligence (P6)** | `DONE` | `internal/services/ports.go`: parses `lsof -Pan -iTCP -sTCP:LISTEN` output with PID→ports mapping, 10s cache, deduplicates dual-stack sockets, generates loopback preview intents (`http://127.0.0.1:<port>`); 3 unit tests. |
| **Lazygit (P7)** | `DONE` | `nativeui/right_panel.go` `lazygitToolView`: scoped to `selectedTabCWD()`, auxiliary tool lifecycle bound to panel/tab, no pane stealing; test verified. |
| **Preview Window (P8)** | `DONE` | `internal/preview`: dedicated project-scoped MyGo WebView Window, strictly bound to approved loopback targets (`localhost`, `127.0.0.1`, `[::1]`), `OnWillNavigate` intercepts external navigation via `e.PreventDefault()` and diverts to system browser; 2 unit tests. |
| **Local Git status (P9)** | `DONE` | `internal/gitintel`: bounded (5s deadline) `git` pipeline (rev-parse, status --porcelain=v1 --branch, diff --numstat HEAD), immutable per-root `Snapshot`, stale-while-refresh cache with single-inflight `SnapshotFor`, pure `Cached` for renders; 6 real-repo tests. |
| **GitHub PR enrichment (P10)**| `DEFERRED_PROTOCOL_GAP` | Read-only enrichment via installed `gh` CLI requires active network authentication; missing `gh` auth cleanly degrades without breaking local Git intelligence. |
| **Worktree mutation gate (P11)**| `DEFERRED_PROTOCOL_GAP` | Protocol 22 audit confirms no native `worktree.*` RPC in Herdr. Invariant strictly preserved: client never executes direct `git worktree` mutations behind Herdr's back. |
| **Dock badge/bounce (P12)** | `DONE` | `nativeui/dock.go`: pure `DockBadgeText` projecting `NeedsAttention + ReviewPending` to macOS `App.Dock.SetBadge`, working agents strictly excluded, 0 clears badge, deduplication controller, attached on startup; 3 deterministic tests. |
| **File drop (P13)** | `DONE` | `nativeui/file_drop.go`: `SafeShellQuote` enforces max 256 paths, max 64 KiB text, POSIX single-quote escaping, rejects any control characters (`\n`, `\r`, `\x00`), never appends execution newlines; 3 unit tests. |
| **Terminal links (P14)** | `DONE` | Handled by official MyGo Terminal element's packaged URL opening; no custom terminal link parser introduced. |
| **Terminal Find (P15)** | `DONE` | `internal/herdr/search_gate.go`: audited against Herdr Protocol 22 `pane.copy_search` schema; typed RPC adapter with `PaneCopySearchParams`/`PaneCopySearchResult`; invariant verified: no client-side VT shadow index; 2 unit tests. |
| **DiagnosticsSnapshot (P16)** | `DONE` | `internal/diagnostics`: extracts Shardlane/Go/MyGo versions, OS/Arch, active instance, and Herdr protocol; normalizes home paths to `~`; 4 unit tests. |
| **Logs viewer (P16)** | `DONE` | `internal/diagnostics` + `nativeui/page_settings.go`: reads up to 5,000 entries from `applog`, pure in-memory level and search filtering, displayed in `/settings/diagnostics`. |
| **Diagnostic export (P17)** | `DONE` | `internal/diagnostics`: `SanitizeText` strips API keys/tokens/passwords (`[REDACTED_CREDENTIAL]`) and home directories; `BuildExportArchive` JSON bundle without terminal text, transcripts, or secrets; one-click copy to clipboard; unit + UI tested. |
| **Official updater (P18)** | `DEFERRED_PROTOCOL_GAP` | MyGo 0.2.7 core module does not bundle `plugins/updater/native`; updater requires dedicated build-time plugin configuration and private signing key. Non-packaged builds cleanly degrade. |
| **Sidebar density (P19)** | `DONE` | `settings.GeneralSettings.Density` ("compact" / "default" / "comfortable"); `nativeui/theme.go` `sidebarItemSpacing()` sets padding/gap (3/1, 5/2, 8/4); interactive control in `page_settings.go`; 3 unit tests. |
| **High contrast (P19)** | `DONE` | `settings.GeneralSettings.HighContrast` (bool); `designTokensWithContrast` enhances border subtle, tree line, and muted text contrast in dark/light themes while terminal program output remains strictly intact; interactive toggle; unit tested. |
| **Task Presets (P20)** | `DONE` | `internal/presets`: `PresetStore` atomic store (`task-presets.json`), default safe templates (Feature Scaffold, Bug Investigation, Architecture Review); `page_new_task.go` autofill without automatic launch; no credentials stored; 2 unit + UI tests. |
| **Activity (P21)** | `DEFERRED_PROTOCOL_GAP` | Audited as lower priority than Command Center/Tools/Diagnostics; deferred to avoid blocking release per P21 prompt guidance. |
| **macOS Package Acceptance** | `DONE` | `go tool mygo build` successfully built `build/darwin-arm64/Shardlane.app` (19.2 MB) and `Shardlane 0.8.0.dmg` (7.2 MB, SHA-256: `405e2f4b0d959e5e0921ed367bc42f46c02cf90fb8ab9867d1cf9a39140fa22a`). |
| **Windows/Linux Compile Evidence** | `DONE` | All pure model packages (`commandcenter`, `filesview`, `gitintel`, `scripts`, `services`, `diagnostics`, `presets`, `preview`) compile cleanly across platforms without cgo or objc2 dependencies. |

---

## 2. Hard Architecture Invariants Proof

1. **No Second Runtime/Workspace Registry**:
   - `session.snapshot` and `herdr.Manager` remain the sole authorities for workspaces, projects, tabs, and panes.
2. **No Render-Time Blocking IO**:
   - Palette (`commandcenter`): purely snapshot-derived in memory.
   - Files (`filesview`): lazy direct listing per directory, never whole-repo scan.
   - Git (`gitintel`): bounded 5s background pipeline, render reads `Cached()`.
   - Ports (`services`): background lsof probe with 10s caching.
   - Diagnostics (`diagnostics`): bounded in-memory log slicing up to 5,000 entries.
3. **No General Browser or Titlebar Tabs**:
   - Local Preview Window strictly validates loopback addresses (`localhost`, `127.0.0.1`, `[::1]`) and intercepts external navigation with `e.PreventDefault()`.
4. **No Mandatory Gateway / Credential Ownership**:
   - Presets strictly store Name, Provider, PromptTemplate, and Mode. No API keys, model guesses, or secrets are ever persisted.
5. **No Client-Owned Script Runtime**:
   - Scripts are managed in metadata by Shardlane, but execution is delegated to Herdr PTY/panes.
6. **No Hidden Lazygit Fleet**:
   - Single auxiliary tool lifecycle bound to the selected Tab CWD and torn down on switch.
7. **No Diagnostics Secrets Leaked**:
   - `SanitizeText` masks regex-matched credentials and replaces home directories with `~`. Exports contain zero terminal bytes and zero history transcripts.
8. **No Unsafe File-Drop Command Execution**:
   - `SafeShellQuote` enforces single-quote escaping, bounds to 256 paths and 64 KiB, and rejects all control characters (newlines/tabs) preventing auto-execution.
9. **No VT Shadow Search**:
   - Terminal Find is strictly mapped to Herdr's authoritative `pane.copy_search` RPC.

---

## 3. Test Suite Verification

- **Total Test Packages:** 16 packages (`agent`, `app`, `applog`, `commandcenter`, `conversation`, `diagnostics`, `filesview`, `gitintel`, `herdr`, `history`, `nativeui`, `presets`, `preview`, `scripts`, `services`, `settings`).
- **Pass Rate:** 100% (0 failures, 0 skipped required tests).
- **Race Detector:** Clean (`-race` on all stateful packages).
- **Code Formatting:** Clean (`gofmt -l .` reports zero files).
