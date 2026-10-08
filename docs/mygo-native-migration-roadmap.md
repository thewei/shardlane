# Shardlane → MyGo Native UI Migration Roadmap

Status: **Active migration implementation plan**
Architecture source of truth: `docs/client-product-architecture.md`
Execution rules: `docs/mygo-native-execution-rules.md`
Branch: `rewrite/mygo`

This document is the repository-owned migration handoff and task ledger. It defines execution order and verification, not product/runtime ownership.

## 1. Final target

```text
Shardlane Desktop
│
├─ MyGo Native UI
│  ├─ native macOS-style shell
│  ├─ ui.Router
│  ├─ Sidebar / Project tree / Toolbar / dialogs
│  ├─ Native Terminal
│  ├─ Search
│  ├─ History list + detail       ← Native UI
│  ├─ New Task
│  ├─ Chat / Conversation         ← Native UI
│  ├─ Settings
│  ├─ Git Workbench / Files / Services / local preview chrome
│  └─ bounded diagnostics
│
├─ Go Application / Domain Services
│  ├─ SettingsService
│  ├─ HistoryService
│  ├─ AgentService
│  ├─ ConversationService
│  ├─ SearchService
│  └─ Remote Host API v2
│
├─ Go Integration
│  └─ Herdr adapter / platform transport
│
└─ Herdr
   └─ sole runtime / PTY / process / Pane-layout / persistence authority
```

### History/Chat presentation decision

Current decision: **Native UI only**.

WebView work for History or Chat is frozen until MyGo provides an official supported native-tree WebView capability and a later explicit product decision enables it.

## 2. Target Go package shape

```text
(repository root)
├─ main.go
├─ internal/
│  ├─ applog/
│  ├─ herdr/
│  ├─ app/
│  │  ├─ settings_service.go
│  │  ├─ history_service.go
│  │  ├─ agent_service.go
│  │  ├─ conversation_service.go
│  │  └─ search_service.go
│  ├─ settings/
│  ├─ history/
│  ├─ agent/
│  ├─ conversation/
│  ├─ remote/
│  ├─ platform/
│  └─ nativeui/
│     ├─ shell/titlebar/sidebar/project tree
│     ├─ router
│     ├─ theme/icons/components
│     ├─ terminal/runtime/watcher/dialogs
│     └─ page_*.go
└─ tests/
   ├─ contracts/
   ├─ fixtures/
   └─ acceptance/
```

## 3. Completed foundation

| ID | Status | Capability | Quick evidence |
|---|---|---|---|
| BASE-01 | ✅ | MyGo Native main window | app build/smoke |
| BASE-02 | ✅ | `ui.Router` | nativeui router tests |
| BASE-03 | ✅ | workspace/new-task/search/history/settings routes | nativeui tests |
| BASE-04 | ✅ | Herdr instance discovery/bootstrap | herdr tests |
| BASE-05 | ✅ | protocol 22 gate | herdr tests |
| BASE-06 | ✅ | `session.snapshot` projection | projection tests |
| BASE-07 | ✅ | `events.subscribe` reconciliation | event tests |
| BASE-08 | ✅ | client-local Project/Tab/Pane selection + basic mutations | local-selection/action tests |
| BASE-09 | ✅ | Native Terminal direct Pane attach | smoke |
| BASE-10 | ✅ | authoritative multi-Pane layout mapping | nativeui geometry test |
| BASE-11 | ✅ | visibility-scoped terminal attachments | nativeui/runtime tests |
| BASE-12 | ✅ | design tokens/icons/components | nativeui tests |
| BASE-13 | ✅ | Project→Tab→Pane hierarchy guides | nativeui tests |
| BASE-14 | ✅ | macOS vibrancy / neutral visual baseline | real-app smoke |
| BASE-15 | ✅ | bounded structured logging | applog tests |
| BASE-16 | ✅ | UI file decomposition | source layout |
| BASE-17 | ✅ | app/DMG build | MyGo build |
| BASE-18 | ✅ | current-runtime Search | nativeui search tests |

## 4. Milestone graph

```text
M1 Shell Parity ──┐
                  ├──> M3 History ──┐
M2 Settings ──────┘                 │
                                    ├──> M5 Conversation / Chat
M4 Agent Launch / New Task ─────────┘
                                    │
                                    ▼
                         0.7 Status Center
                                    │
                                    ▼
                    0.8 History / Usage / Inspector
                           ┌────────┴────────┐
                           ▼                 ▼
                    M6 Workspace Intelligence
                           │
                           ▼
                    0.10 Git Workbench
                           │
                           ├──────────────► M7 Remote API v2
                           │
                           └────────┬────────┘
                                    ▼
                           M8 Cross Platform
                                    │
                                    ▼
                           M9 Final Cutover
```

Critical path:

```text
History → 0.4 Closure → 0.5 Agent Workbench/Lifecycle → 0.6 Conversation/Chat → 0.7 Status Center → 0.8 History/Usage/Agent Inspector → 0.9 Workspace Intelligence/Desktop Experience → 0.10 Git Workbench/Unified Primary Surface → Remote/Platform → Cutover
```

## Current planned next release — 0.10.0 Git Workbench & Unified Primary Surface

0.9 has an implementation pass and historical closure audit, but the post-implementation source audit found package-level features that are not fully wired. 0.10 therefore begins with RealityMatrix reconciliation, removes Lazygit, then establishes the unified Terminal / Diff Review / Commit workspace center and the Native Git Workbench.

Audit: `docs/reference-godiff-shardlane-0.10-audit.md`.
Plan: `docs/mygo-native-0.10.0-git-workbench-primary-surface-plan.md`.
Prompt: `docs/prompts/mygo-native-0.10.0-git-workbench-primary-surface-implementation-prompt.md`.

## Historical bridge — 0.4.0 Closure

0.3.0 completed the planned Native Settings + read-only Native History MVP. 0.4 was the closure pass before Native Chat:

- upgrade MyGo 0.2.5 → current 0.2.7;
- fix History latest-request-wins behavior and virtualize the list;
- make concurrent Agent launch idempotency actually single-owner;
- establish Design System v2 and shared Native components;
- introduce one OperationalSummary/status renderer for Header/Sidebar/Project/Pane/Agent consumers;
- restore provider Hook/Integration health as an architecture-correct Native dynamic surface;
- complete the full safe New Task launch transaction before enabling Start Agent.

Audit: `docs/mygo-native-0.3.0-closure-audit.md`.
Plan: `docs/mygo-native-0.4.0-closure-plan.md`.
Prompt: `docs/prompts/mygo-native-0.4.0-closure-implementation-prompt.md`.

0.5 is the Agent Workbench/Lifecycle release; full Native Chat moves to 0.6. See `docs/mygo-native-0.5.0-agent-workbench-plan.md`.

# M1 — Native Shell Parity

## Current audited task status — 2026-10-05

| Task | Status | Evidence |
|---|---|---|
| SHELL-00 local semantic selection | ✅ DONE | focus RPCs removed; external focus churn regression tests |
| SHELL-01 Sidebar section parity | ✅ DONE | Recent section + shell skeleton tests |
| SHELL-02 Project/Tab expand-collapse | ✅ DONE | independent presentation-state test |
| SHELL-05 shortcut registry | ✅ DONE | registry + Back/Forward tests |
| SHELL-06 unified shell state | ✅ DONE | lifecycle classification tests |
| SHELL-07 Recent presentation model | ✅ DONE | Native Sidebar fixture |
| SET-01 Settings Store | ✅ DONE | memory-store tests |
| SET-02 atomic FileStore | ✅ DONE | roundtrip/corrupt/future-schema/0600 tests |
| HIST-01 domain models | ✅ DONE | provider/wire-model tests |
| HIST-02 project-key normalization | ✅ DONE | table tests |
| HIST-03 source locator | ✅ DONE | measured Herdr id/path tests |
| HIST-04 Claude adapter | ✅ DONE | Rust fixture-parity test |
| 0.2.0 package audit | ✅ DONE | full tests/build/isolated app smoke |

Audit report: `docs/mygo-native-0.2.0-audit.md`.
Next version plan: `docs/mygo-native-0.3.0-plan.md`.

## SHELL-00 — Client-local semantic selection
**Objective:** keep Project/Tab/Pane navigation local to one Shardlane window while Herdr global focus remains bootstrap/fallback information only.

**Rules:**
- never use `workspace.focus`, `tab.focus`, or `pane.focus` for ordinary navigation;
- never subscribe to global focus events as navigation drivers;
- preserve local selection while selected runtime objects still exist;
- fall back only when selected objects disappear;
- terminal geometry remains Herdr-layout-owned.

**Fast verify:**
```sh
go test ./internal/nativeui -run 'TestLocalSelection|TestVisiblePaneGeometryUsesLocalSelectedTab'
go test ./internal/herdr -run TestEventSubscriptionsExcludeGlobalFocusChurn
```

## SHELL-01 — Sidebar section parity
**Objective:** canonical order: New Task / Search / History / Agents / Recent / Projects / footer Workspace + Settings.

**Output:** Sidebar shell with explicit Recent section and empty state.

**Fast verify:**
```sh
go test ./internal/nativeui -run TestSidebar
```

## SHELL-02 — Project tree expand/collapse
**Objective:** expansion is client presentation state, independent from runtime focus.

**Output:** multiple Projects can remain expanded.

**Fast verify:** headless test opens A+B, focuses A, asserts B stays expanded.

## SHELL-03 — Project tree state styling
**Objective:** reusable hover/selected/focused/status appearance.

**Fast verify:** component/headless state test.

## SHELL-04 — Context-menu parity
Project: New Tab / Rename / Close.
Tab: Rename / Close / move/reorder where protocol supports.
Pane: Split / Zoom / Rename / Close / move/swap where protocol supports.

**Fast verify:**
```sh
go test ./internal/nativeui -run TestNativeSidebarContextMenus
```

## SHELL-05 — Shortcut registry
Move route/action shortcuts out of page code.

Minimum:
- ⇧⌘N New Task
- ⌘K Search
- ⌘, Settings
- ⌘R Refresh
- ⌘[ / ⌘] Back/Forward

**Fast verify:** shortcut table test.

## SHELL-06 — Unified shell load/error state
Define `loading / ready / degraded / disconnected / error`.

**Fast verify:** fake runtime → state mapping test.

## SHELL-07 — Recent presentation model
Add presentation DTO only:
```go
type RecentItem struct {
    ID, Title, Subtitle, Kind string
    UpdatedAt time.Time
}
```

**Fast verify:** Sidebar headless fixture.

## SHELL-08 — Safe window-state restoration
Restore position only when intersecting an available display; otherwise center primary.

**Fast verify:** pure placement tests.

### M1 local exit
```sh
go test ./internal/nativeui
```

# M2 — Settings

## SET-01 — SettingsStore interface
```go
type SettingsStore interface {
    Load(context.Context) (Settings, error)
    Save(context.Context, Settings) error
}
```
**Verify:** in-memory implementation test.

## SET-02 — File store
MyGo user-data directory; versioned schema; atomic write; corrupt-file safe fallback.

**Verify:** temp-dir roundtrip/corruption tests.

## SET-03 — General settings
Appearance/window behavior.

## SET-04 — Terminal settings
Font/size/line-height/scrollback/Option-as-Alt.

## SET-05 — Apply terminal presentation settings
Must not replace Herdr terminal ID or PTY authority.

**Verify:** mapping + terminal identity tests.

## SET-06 — Shortcut preferences
Presentation/action mapping only.

### M2 local exit
```sh
go test ./internal/settings/... ./internal/nativeui -run 'Settings|Shortcut|TerminalSetting'
```

# M3 — History

Goal: replace `shardlane-history` behavior incrementally. History stays read-only and Native UI.

Performance contract:
- page-addressable cache;
- bounded visible transcript;
- 60 messages max materialized;
- 12-message overlap;
- collapsed long-body previews;
- cancellable obsolete loads;
- no external source mutation.

## HIST-01 — Domain models
Session / Message / Provider / Tool event / Usage / Project key.

**Verify:** JSON/fixture decode tests.

## HIST-02 — Project-key normalization
Pin existing Rust behavior with table fixtures.

## HIST-03 — Source locator
Locate provider history stores without parsing transcripts.

## HIST-04 — Claude adapter
Source → normalized transcript.

## HIST-05 — Codex adapter

## HIST-06 — Gemini adapter

## HIST-07 — Catalog schema/read path
Read-only list/filter/order semantics.

## HIST-08 — History search
Pin existing search result behavior.

## HIST-09 — HistoryService
UI-independent:
```go
ListHistory(projectKey, query, limit)
OpenConversation(id, window)
Search(query, scope)
```

## HIST-10 — Native `/history`
Search/filter/list/loading/error/empty.

**Verify:** MyGo headless UI.

## HIST-11 — History detail presentation model
Normalized blocks, bounded window.

## HIST-12 — Native `/history/{id}`
Native message/tool blocks, bounded rendering.

## HIST-13 — Resume intent
Produce continuation plan only; execution still goes through Agent/Conversation service.

## HIST-14 — Recent provider
Feed Sidebar Recent from HistoryService metadata only; no transcript parse.

### M3 local exit
```sh
go test ./internal/history/...
go test ./internal/nativeui -run 'History|Recent'
```

# M4 — Agent Launch / New Task

## AGENT-01 — Provider capability model
Pin current Host provider capability matrix.

## AGENT-02 — StartAgent request model
instance/project/provider/cwd/prompt/request identity.

## AGENT-03 — Idempotency/request identity
Duplicate request ID must not create duplicate Agent.

## AGENT-04 — Herdr launch adapter
Protocol only; no UI state.

## AGENT-05 — Readiness gate
Bounded identity/readiness retry, preserving current Host semantics.

## AGENT-06 — Uncertain-delivery reconciliation
After socket write + lost response: query state, reconcile identity, never blindly retry.

## AGENT-07 — Native New Task form
Project/provider/cwd/prompt/resume source.

## AGENT-08 — Enable Start Agent
One UI action → exactly one app-service transaction.

## AGENT-09 — Agent state projection
working / blocked / done / errored.

### M4 local exit
```sh
go test ./internal/agent/...
go test ./internal/nativeui -run 'NewTask|Agent'
```

## Version bridge — 0.5 Agent Workbench & Lifecycle

Before M5 Conversation/Chat, complete the Agent-facing client semantics inherited from the original product:

- stable Agent identity and AgentCardModel;
- runtime phase + attention + sendability + unread/review as separate axes;
- Sidebar/Header/Agents-page card reuse;
- Header Agent Overview filters and actionable counts;
- Ctrl-Tab MRU Agent switcher;
- explicit Mark reviewed lifecycle;
- meaningful transition notifications;
- History Continue planner/execution: AlreadyLive → NativeResume → ContextTransfer → NeedsProjectSelection → Unsupported.

Plan: `docs/mygo-native-0.5.0-agent-workbench-plan.md`.
Prompt: `docs/prompts/mygo-native-0.5.0-agent-workbench-implementation-prompt.md`.

Live Handoff closed 2026-10-05 (see M5 below); Native Chat shipped across the 0.6 slices (History binding, live decoder, pending echo, queue-worker delivery with the shared per-Conversation reservation coordinator). **Delivery ledger persistence closed 2026-10-05**: `LaunchService.AttachLedgerPath` persists every ledger mutation atomically (temp+rename) to `follow-up-ledger.json` in the user-data directory via the queue's `OnChanged` hook, and restores it on startup — a crash mid-delivery restarts its item as the DeliveryUncertain tombstone ("outcome unknown across restart — inspect before retry"), never a silent resend; the M5 ledger surface is complete for the session contract.

# M5 — Conversation / Chat (0.6.0)

Current presentation decision: **Native UI only**.

Same-Session rule: Terminal and Chat are two views over one Herdr-owned Agent session.

0.6 implementation authority: `docs/mygo-native-0.6.0-conversation-chat-plan.md`.
Implementation prompt: `docs/prompts/mygo-native-0.6.0-conversation-chat-implementation-prompt.md`.

0.6 must preserve the original Host semantic contracts: session-exact Conversation identity, bounded live semantic decoding, one idempotent prompt transaction, Working follow-up queue, structured InteractionBroker resolution, and source-safe Live Handoff. Chat must never become a second Agent/runtime authority.

## CHAT-01 — Conversation models
Conversation / Turn / Message / Tool event / Interaction / Queue item.

## CHAT-02 — Conversation read service
History + live projection.

## CHAT-03 — Incremental live decoder/projection
No TUI/ANSI semantic parsing.

## CHAT-04 — Prompt transaction
Use canonical Herdr semantic prompt path.

## CHAT-05 — Continue transaction
Pin existing continue fixture.

## CHAT-06 — Follow-up queue

## CHAT-07 — Interaction resolution
Blocked approval/question must use exact provider interaction bridge; never generic prompt/guessed PTY keys.

## CHAT-08 — Delegation

## CHAT-09 — Presentation block model
```text
TextBlock
MarkdownBlock
ToolBlock
InteractionBlock
UsageBlock
StatusBlock
```
Independent of UI toolkit.

## CHAT-10 — Native Chat list
Bounded visible message window.

## CHAT-11 — Native composer
Prompt / queued follow-up / interaction affordances.

## CHAT-12 — Native tool cards and interaction cards

## CHAT-13 — Context transfer / live handoff

## CHAT-14 — Large conversation performance
Bounded materialization + cancellation tests.

### M5 local exit
```sh
go test ./internal/conversation/...
go test ./internal/nativeui -run Chat
```

## Version bridge — 0.7 Status Center & Agent Quick Navigation

Before broader management/tools, close the always-visible Agent operational entry point:

- one StatusCenterSnapshot shared by window Header and system Tray/Menu Bar;
- right-side titlebar Agent status counts;
- macOS Menu Bar left-click → MyGo Native floating quick panel;
- Attention / Review / Working / Idle filters;
- compact Agent model/token/cost/quota Usage when known;
- quick navigation to exact Interaction / Terminal / Conversation / Agent destination;
- explicit Mark reviewed;
- no duplicated status model or WebView quick panel.

Reference audit: `docs/reference-magpie-agent-history-usage-audit.md`.
Plan: `docs/mygo-native-0.7.0-status-center-plan.md`.
Prompt: `docs/prompts/mygo-native-0.7.0-status-center-implementation-prompt.md`.

## Version bridge — 0.8 History Management, Usage & Agent Inspector

Before the larger Workspace Intelligence release, close the management/inspection layer identified by the Magpie reference audit:

- History grouped by normalized Project;
- History search/filter before grouping;
- provider-capability-gated session Trash/Restore/Purge;
- live/recent/stale-source delete guards;
- provider-neutral UsageService with source/completeness semantics;
- Agent/Session/Project/Provider/Model usage summaries;
- estimated-cost and optional quota/allowance projection;
- Native Usage page;
- Agent Inspector combining runtime, provider/model, integration health, usage/quota and session links;
- Status Center consuming the authoritative UsageService.

Shardlane remains a Herdr client/product; the Magpie-style mandatory model gateway is explicitly not part of 0.8.

Reference audit: `docs/reference-magpie-agent-history-usage-audit.md`.
Plan: `docs/mygo-native-0.8.0-history-usage-agent-inspector-plan.md`.
Prompt: `docs/prompts/mygo-native-0.8.0-history-usage-agent-inspector-implementation-prompt.md`.

# M6 — Workspace Intelligence & Desktop Experience (0.9.0)

0.9 replaces the former thin Desktop Tools milestone with one large local-development workflow release. It migrates retained Rust product capabilities and incorporates the strongest compatible patterns from `penso/herdr-gpui` and `yetone/magpie` without importing their architecture wholesale.

Release-blocking pillars:

```text
Command Center
Project-scoped Native Right Panel
Files
Scripts / Services / Ports
Lazygit
local Preview workflow
Git branch/diff/ahead-behind intelligence
Desktop attention + local Terminal file drop
Diagnostics / bounded Logs
official MyGo updater + release notes
Sidebar density / high contrast
New Task Presets
```

Capability-gated enhancements:

```text
read-only GitHub PR readiness through an approved auth adapter
semantic Terminal Find through a pinned Herdr API
worktree mutation only through verified Herdr runtime APIs
embedded Preview only after official MyGo Native-tree WebView support
```

Explicit non-goals remain mandatory model gateway, titlebar Tabs, general browser tabs, remote file transfer/Teleport, and a client-owned Project process runtime.

Reference audit: `docs/reference-magpie-herdr-gpui-shardlane-0.9-audit.md`.
Plan: `docs/mygo-native-0.9.0-workspace-intelligence-desktop-experience-plan.md`.
Prompt: `docs/prompts/mygo-native-0.9.0-workspace-intelligence-desktop-experience-implementation-prompt.md`.

## 0.10.0 — Git Workbench & Unified Primary Surface

0.10 is a corrective/product iteration over the actual 0.9 implementation tree. It removes the unfinished Lazygit target and establishes one center-content owner inside `/workspace`:

```text
WorkspacePrimarySurface
├─ Terminal
├─ Diff Review
└─ Commit

Right Panel
├─ Changes
├─ Files
└─ Services
```

Release-blocking scope:

```text
0.9 RealityMatrix corrections
remove Lazygit
Godiff-inspired Sidebar + Workspace Header visual language
changed-file tree + bidirectional Diff synchronization
virtualized unified/split native Diff
syntax + bounded word-level highlight
selected-file Commit with stale/index-preservation fences
local branch list/switch + New Branch
Files no-render-IO correction
real Script persistence/execution closure
real Services/port wiring
Preview manager / File Drop / Terminal Find wiring
accurate updater and package-version closure
```

Godiff is a behavioral/UI reference only under the audited snapshot; its `internal/*` code is not copied or imported.

Audit: `docs/reference-godiff-shardlane-0.10-audit.md`.
Plan: `docs/mygo-native-0.10.0-git-workbench-primary-surface-plan.md`.
Prompt: `docs/prompts/mygo-native-0.10.0-git-workbench-primary-surface-implementation-prompt.md`.

## N15 — MyGo v0.2.15 capability integration (2026-10-06)

Pin bump: `github.com/egoist/mygo` v0.2.9 → v0.2.15 in `go.mod` (the version authority). Inherited by the upgrade itself: macOS KVO resize/fullscreen crash fix, per-size window-zoom painting, GPU burst under CPU frame cost, Core-Animation continuous corners, CSS-like pressed-hover semantics, and the framework-level Enter/Escape-during-IME-composition guard.

| Task | Capability | Landing |
|---|---|---|
| N15-01 | Notification identity: stable per-agent ID + per-workspace Group; `App.OnNotificationClick` routes clicks (cross-run) to the owning pane via the workbench card path; unknown/ended agents fail closed to a toast | `nativeui/agent_notifications.go`, `workbench.go`, `main.go` |
| N15-02 | Terminal transparency: Settings → Terminal "Transparent background" (opt-in, default off); `terminal.Options.Transparent` + translucent pane-card paper so the window gradient reads through | `settings/settings.go`, `nativeui/terminal.go`, `gorex_style.go`, `page_settings.go` |
| N15-03 | IME composing guard: chat Send and New-Task Start stay disabled while `Composing()` | `chat_ui.go`, `page_new_task.go` |
| N15-04 | External-link failure surfacing: preview `openExternal` seam reports errors; failures toast in the main window and log `scheme://host` only (never full URLs — query strings can carry tokens) | `preview/preview.go`, `right_panel.go` |
| N15-05 | P3 wide-gamut accents: `ui.Oklch` family, each entry hill-climbed so its sRGB fallback stays within a rounding step of the original hex | `nativeui/theme.go` |
| N15-06 | Liquid Glass (`plugins/glass`) on the two floating palettes: Command Center and Agent Switcher frost the dimmed page instead of a flat panel | `command_center_ui.go`, `switcher.go` |
| N15-08 | Panel slide motion: Sidebar and Right Panel collapse/expand with a 200 ms EaseOut width slide driven by `Animate`; a persistent keyed host clips content at its open width (no mid-slide text re-wrap) and unmounts it once fully closed (hidden surfaces do no presentation work); Reduce Motion lands widths instantly (framework) — also the headless-test seam. Terminal cards re-flow per frame (same class as live pane drag, bounded duration) | `nativeui/panel_motion.go` (new), `shell.go`, `right_panel.go`; 3 headless tests in `panel_motion_test.go` |

Deferred: headless-terminal `Resize` + `Snapshot` (v0.2.11) for Remote/mobile session-screen serving — a Herdr-boundary capability (the server keeps a session's screen for attaching windows); design against the live Herdr protocol before any client work. Tooltip/toast theming inherits the framework `Inverse` defaults via `ui.LightTheme()`/`ui.DarkTheme()` — revisit only if a palette drift appears.

Fast verification: `GOTOOLCHAIN=go1.27.1 go test ./...`; `GOTOOLCHAIN=go1.27.1 go tool mygo build`. Known environmental flake: `applog/TestCaptureStderrRoutesFd2IntoFile` (2 s marker deadline) can false-FAIL under full-suite parallel load; it passes isolated `-count=3` on the upgraded tree — rerun isolated before diagnosing (same convention as the `shared_tui` contention detector).

# M7 — Remote / Mobile API v2

Keep current wire compatibility; reuse Go app services.

## API-01 DTO package
## API-02 `/hello`
## API-03 `/bootstrap`
## API-04 Workspace endpoints
## API-05 Tab endpoints
## API-06 Pane structural endpoints
## API-07 Pane compatibility I/O
## API-08 Agent endpoints
## API-09 Conversation endpoints
## API-10 Interaction endpoints
## API-11 History search
## API-12 Events WebSocket
## API-13 TUI compatibility endpoints if still required
## API-14 Instance endpoints
## API-15 Golden fixture suite

**Fast verify per endpoint:** exact Rust fixture JSON parity.

### M7 local exit
```sh
go test ./internal/remote/... -run Golden
```

# M8 — Cross Platform

## PLAT-01 — Platform seams
No OS-specific behavior in domain packages.

## PLAT-02 — macOS Herdr transport
Unix socket.

## PLAT-03 — Linux Herdr transport
Unix socket.

## PLAT-04 — Windows Herdr transport
Official Herdr Windows pipe/transport.

## PLAT-05 — Linux Native UI compile
## PLAT-06 — Windows Native UI compile

## PLAT-07 — Terminal capability matrix
keyboard/mouse/resize/selection/clipboard/IME.

## PLAT-08 — Packaging
macOS / Windows / Linux release artifact generation.

# M9 — Final Cutover

No feature is declared migrated because the app merely launches.

## FINAL-01 — Feature parity matrix
Every retained feature must be DONE, EXPLICITLY REMOVED, or DEFERRED BY PRODUCT DECISION.

## FINAL-02 — Full tests
```sh
go test ./...
```

## FINAL-03 — Release builds
All promised targets.

## FINAL-04 — Contract parity
Remote fixtures + Mobile schemas + History fixtures + Conversation fixtures.

## FINAL-05 — macOS full real-app acceptance
Window lifecycle, multi-monitor, Router, Sidebar, Terminal, multi-Pane, IME/CJK, clipboard, selection, mouse, History, Chat, New Task, tools, logs.

## FINAL-06 — Windows full acceptance
## FINAL-07 — Linux full acceptance

## FINAL-08 — Performance gate
Cold launch, idle CPU/RSS, terminal throughput, route latency, History query/open, Chat projection/render, event reconciliation.

## FINAL-09 — Soak/leak gate
Repeated routing, Pane churn, Agent cycles, reconnect, log rotation; no process/goroutine/attachment leak.

## FINAL-10 — MyGo becomes default client
## FINAL-11 — Remove Rust GUI
Only after FINAL-01..10.

## FINAL-12 — Remove Rust compatibility implementations
Incrementally:
```text
remote → host → history → obsolete workspace deps
```

# Active next tasks

## Version plan chain (planning complete 2026-10-05)

The planned migration releases carry atomic task
ledger, verification gates and a definition of done, and each has a matching
implementation prompt:

| Version | Plan | Implementation prompt | Depends on |
|---|---|---|---|
| 0.5.0 Agent Workbench & Lifecycle | `docs/mygo-native-0.5.0-agent-workbench-plan.md` | `docs/prompts/mygo-native-0.5.0-agent-workbench-implementation-prompt.md` | 0.4. **Implemented 2026-10-05**: workbench domain/markers/directory, `/agents` + Header overview, MRU switcher, notifications, and full History Continue incl. the **ContextTransfer bounded briefing builder** reading the source window from the history page cache (tested at both service and transaction level); 0.5.0 DMG `2cdb44df…` |
| 0.6.0 Native Conversation & Chat | `docs/mygo-native-0.6.0-conversation-chat-plan.md` | `docs/prompts/mygo-native-0.6.0-conversation-chat-implementation-prompt.md` | 0.5. **Core implemented 2026-10-05**: `internal/conversation` (identity/fingerprint, bounded models, disposition, follow-up queue, interaction CAS broker, timeline turns) + `/chat` bound to real HistoryService data via `bindChatConversation` (latest-generation wins, stale results cannot apply, CONV-07/09 tested). **Live decoding implemented 2026-10-05**: `internal/history/live.go` capability registry + transport-neutral incremental decoder (CONV-11..19 — append cursor/partial-line, Claude/Codex full==incremental fixture parity, tool-result backfill changed-row deltas, truncate/reset, wake coalescing + backstop, all race-tested); Chat binds live agents through the measured `agent_session` identity (projection-mapped, protocol 22 schema verified) → `ResolveLiveSource` → decoder pump with generation guard; provisional streaming tail renders uncommitted. **Pending submission echo implemented 2026-10-05 (§12/CHAT-09)**: `conversation.PendingEcho` + `ReconcilePendingEcho` (baseline/order evidence, trimmed-text User rows only — terminal echo never confirms, consumed exactly once, idempotent); wired into the SentNow commit lane and the live sync fold with pane-pinned rebind retention (same-pane rebind keeps, cross-pane rebind and history rebind drop); table + Shell-level deterministic tests green. **Live Handoff implemented 2026-10-05 (§19/HANDOFF-01..09)**: `agent/live_handoff.go` ports the audited Host engine — source reservation held across fences (HANDOFF-03 gate/serialization tests), exact-identity read via typed `agent_session` (HANDOFF-01), sendability-SSOT eligibility + blocked/unknown refusal (HANDOFF-02), bounded settle cycles with busy/wait-failed/settled-to-blocked classes (HANDOFF-04), freshness fence with VerifiedFlush/StableStat fidelity and fail-closed stale sources (HANDOFF-05), pending-operation fence over `FollowUpQueue.HasUnresolvedForConversation` (HANDOFF-06, ledger state-matrix test), exactly-one target/briefing through the canonical transaction with sha-pinned briefing and untouched source (HANDOFF-07), CreatedNeedsAttention/committed-target preservation taxonomy (HANDOFF-08), Native action + headless outcome-state mapping with client-local committed-pane navigation (HANDOFF-09); app entry `LaunchService.HandoffLiveAgent` with per-conversation reservation registry and `SourceAgent`/`WaitAgentSettled` Herdr adapters. **Queue-worker delivery wired 2026-10-05 (§11)**: `conversation/delivery.go` — `DeliveryWorker` owns the §11.3 pipeline (exact-target wait → occupant identity revalidation → atomic claim → exactly one semantic prompt → reconcile; blocked/timeout retain, identity change and definite rejection fail closed recoverable, uncertainty tombstones forever), `Reservations` is the per-Conversation coordinator shared by the worker, prompts and the live handoff; `LaunchService` owns the production ledger (`EnqueueFollowUp`/`Queue`/`PendingOperations` as the handoff seam, `herdrDeliveryTransport` over the verified prompt path, idempotent `StartFollowUpWorker`/`StopFollowUpWorker` on window attach/close); the chat composer's QueuedAfterTurn now enqueues in the real ledger (identity-proven at enqueue) and its cancel cancels the ledger item; **ledger persistence 2026-10-05**: `FollowUpQueue.OnChanged` + `Snapshot`/`Restore` and `LaunchService.AttachLedgerPath` (atomic JSON persistence in user-data, restart load with the crash-mid-delivery Delivering→DeliveryUncertain remap) wired from the desktop entrypoint; 10 deterministic worker tests (happy/retain/identity-fail-closed/tombstone/rejection/claim-lost incl. cancel-A/enqueue-B, reservation-held gate, concurrent exactly-one) + app ledger/transport tests (round-trip, crash remap, tombstone fence) + headless composer wiring test, all race-green |
| 0.7.0 Status Center & Agent Quick Navigation | `docs/mygo-native-0.7.0-status-center-plan.md` | `docs/prompts/mygo-native-0.7.0-status-center-implementation-prompt.md` | 0.6. **Implemented 2026-10-05**: shared `StatusCenterSnapshot`, `/status-center` page with filters/recommended-destination rows, titlebar counts open the panel. **macOS Tray projection implemented 2026-10-05 (§17-18)**: `nativeui/tray_model.go` pure projection (`TrayCounts`/`TrayTitle` status-first `⚠ ✓ ⚡` title, empty when quiet; `TrayToolTip` shared summary wording incl. "Herdr disconnected"; `TrayFingerprint` covering §18 trigger set with normalized entry order) + `nativeui/tray.go` fingerprint-gated controller over the MyGo `NewTray` API (`SetTitle`/`SetToolTip`, template glyph, click routed as the same action as the titlebar trigger — no direct runtime mutation); wired through `applyProjection` → `updateTray`, created in `WhenReady`, in-window Status Center stays the fallback when tray creation fails; 6 deterministic tests (title/tooltip tables, fingerprint stability, controller gating, Shell-level integration, icon PNG). **AgentUsageSnapshot implemented 2026-10-05 (§17.6/§17.2)**: `agent/usage.go` lightweight per-Agent projection from History/session metadata only (model + provider-reported tokens; `CostUSD`/`Quota` typed per the plan but stay nil until a provable provider fact exists — "Unknown stays None"), `FormatUsageLine` renders the §17.2 secondary line ("Sonnet · 38k tok · $0.21 est.", quota window replaces tokens when provable, quiet when fact-less) + compact `FormatTokenCount`; projection + formatter table-tested; consumed by the 0.8 usage-facts display. **Usage secondary line wired 2026-10-05 (§17.2)**: `nativeui/usage_rows.go` — the Shell caches the per-Agent §17.6 snapshot (`usageByAgent`) refreshed on the dispatch lane from the read-only catalog via exact `agent_session` identity matching (id/native-id and path/file-path kinds; throttled to one minute, forced on quick-panel open), and `statusCenterRow` renders `FormatUsageLine` as the row's muted secondary caption — quick panel and status center included, zero render-time IO; matcher/throttle/render tests green. **Floating Status Quick Panel implemented 2026-10-05 (§17.1/§17.2)**: `nativeui/quick_panel_model.go` — pure `QuickPanelBounds` (tray-anchored, work-area-clamped, flip-above fallback, degenerate-safe) and `QuickPanelHeader` reading the one shared `attentionSegments` source (tray "for review" / panel "review" wording, Ready/disconnected states); `nativeui/quick_panel.go` — `QuickPanelView` renders the snapshot's actionable rows through the same `statusCenterRow` recommended-destination routing (idle rows stay in-window), Escape hides; `ToggleQuickPanel` is the tray click action per the §17.1 lifecycle (visible→hide, else anchor via `Tray.Bounds` + `Screen.DisplayNearestPoint` work area → show+focus; in-window `/status-center` stays the fallback without a panel); `main.go` creates the frameless, SkipTaskbar, AlwaysOnTop vibrancy window and `AttachQuickPanel` wires blur→hide; 5 deterministic tests (geometry table, header/tray segment parity, view render incl. idle-exclusion, toggle fallback + nil-safety, relevance matrix) |
| 0.8.0 History Management, Usage & Agent Inspector | — | `docs/prompts/mygo-native-0.8.0-history-usage-agent-inspector-implementation-prompt.md` | 0.7. **Core implemented 2026-10-05**: `/inspector/{pane}` read-only inspector (status dimensions + bound conversation window), `/history-projects` grouped management view; 0.8.0 DMG `b124b0a3…`. **Usage facts implemented 2026-10-05**: the inspector consumes the §17.6 `AgentUsageSnapshot` (model/provider-reported tokens from the bound conversation's History metadata — metadata only, no render-time IO, no transcript parse) in a "Usage facts" card with the compact summary line, exact totals, provenance source, and the explicit "No provider-reported facts" quiet state; headless render tests green. **Per-account usage aggregator ported 2026-10-05**: `agent/usage_ledger.go` mirrors the audited Host facts layer — `UsageWindow`/`AccountUsage` (provider + provable local account + local day, only provider-reported token facts > 0) / `UsageSnapshot` with `TokensToday`, `MaskAccountLabel` (4-char masked id, display-name fallback, raw ids never surfaced), provable Codex account identity from `CODEX_HOME/auth.json` (nil when unprovable — never guessed), and a `UsageAggregator` with a JSON snapshot cache, mtime+count invalidation fingerprint (`SessionMeta.MtimeMS` now mapped from the catalog) and the one-minute throttle; 6 deterministic tests (grouping/day, throttle + invalidation, cache persistence, source-error retention, mask table, auth.json). **Inspector/history-projects page tests 2026-10-05**: headless render tests for the read-only inspector (four status dimensions from the card, off-screen unread markers, location facts, bounded conversation window, unbound-quiet rule, no composer) and the grouped management view (project grouping with counts, unknown-path fallback, cached dispatch-lane refresh with single-inflight + load-once discipline — the page no longer queries per render frame) |
| 0.9.0 Workspace Intelligence & Desktop Experience | `docs/mygo-native-0.9.0-workspace-intelligence-desktop-experience-plan.md` | `docs/prompts/mygo-native-0.9.0-workspace-intelligence-desktop-experience-implementation-prompt.md` | 0.8. **Opened 2026-10-05 — P0 preflight done**: MyGo pinned at 0.2.7 (`App.Dock.SetBadge`/`Badge` present for §P12; no `plugins/updater` in the core module — §P18 needs the separate plugin dependency, gated); all four local provider CLIs verified present for Agent-capability checks — `claude 2.1.285`, `codex-cli 0.160.0`, `pi 1.0.2`, `agy 1.2.16` (agy = the registry's Antigravity Preview provider; matches the 0.5 capability table, no registry drift). **P1 Command Center implemented 2026-10-05**: `internal/commandcenter` — the ranked palette index (exact > prefix > substring > fuzzy subsequence, keywords counted at class, stable section/title ordering, Navigation scope strict for Cmd+P vs All for Cmd+Shift+P, unavailable actions never surface, index immutable/pure) with 5 table tests; `nativeui/command_center_ui.go` — the palette overlay reusing the switcher modal pattern (`SearchField` owns Enter/Changed; arrows select, Enter commits, Esc closes), `buildCommandCenterActions` derives the §P2 Action Registry from the snapshot (projects/tabs/panes from the projection, agents from the workbench, history from the loaded cache, app surfaces) with zero IO on open, and `executeCommandCenterResult` revalidates every typed target (pane/tab/project/agent) against the projection before client-local navigation — stale targets surface recoverable text; headless tests drive the real shortcuts (`Cmd+Shift+P`/`Cmd+P`), keys, query narrowing and stale-target fail-closed. **P9 gitintel implemented 2026-10-05**: `internal/gitintel` — bounded (5s-deadline) `git` pipeline (root → status --porcelain --branch → numstat) into the immutable per-root `Snapshot` (branch, ahead/behind, changed files with numstat, dirty, generated-at; no auto-network in v1), stale-while-refresh cache with single-inflight-per-root `SnapshotFor` and pure `Cached` for renders, fail-closed non-repo snapshots; 6 tests over real temp repositories (branch/dirty/numstat, ahead-behind via a bare origin, fail-closed, stale-while-refresh with an injected clock, injected-runner error) — race-green. **P3 Native Right Panel & P4 Files implemented 2026-10-05 (WIX-040..066)**: `internal/filesview` — direct directory listing, stable sort (directories first, then case-insensitive Unicode), symlink no-recursive-follow, text/binary classification, 1 MiB preview cap, format size helpers with 5 unit tests; `nativeui/right_panel.go` — frameless collapsible tool area (default width 320, bounded to 240..500) strictly anchored to `selectedTabCWD()` (WIX-041), Surface switcher between Files / Services / Lazygit, lazy directory expansion tree (no repository-wide scan on open), file preview modal with copy path & reveal in Finder (`open -R`), toggled via titlebar button or `⌥⌘B` shortcut, registered in Command Center; 4 headless UI tests pass. **P5 Scripts & P6 Services/Ports implemented 2026-10-05 (WIX-070..105)**: `internal/scripts` — atomic JSON store for project script definitions (CRUD, one-shot vs resident service classification, Herdr terminal execution), unit tested; `internal/services` — port intelligence observing local listening TCP sockets (`lsof -Pan -iTCP -sTCP:LISTEN`) with 10s caching, PID→ports mapping, loopback preview intent generation (`http://127.0.0.1:<port>`), tested with simulated lsof output; integrated into right panel Services surface view. **P12 Desktop Attention implemented 2026-10-05 (WIX-170..175)**: `nativeui/dock.go` — pure `DockBadgeText` projecting `NeedsAttention + ReviewPending` to macOS `App.Dock.SetBadge` (Working agents strictly excluded from badge count per invariant; zero clears badge to empty string), `dockController` with change deduplication, hooked into `applyProjection` and attached to `mygo.App.Dock` on startup in `main.go`; tested with 3 deterministic unit tests covering full status matrix, deduplication, and projection integration. **P19 Sidebar Density & High Contrast implemented 2026-10-05 (WIX-240..250)**: `internal/settings` extended with `Density` ("compact" / "default" / "comfortable") and `HighContrast` (bool); `nativeui/theme.go` provides `sidebarItemSpacing()` controlling row height/gap and `designTokensWithContrast()` strengthening border subtle/tree line/text muted contrast in both light/dark themes while keeping terminal program output intact; `page_settings.go` provides interactive segment & checkbox controls; tested with roundtrip mutation, spacing ordering, and contrast token assertions. **P8 Local Preview Window implemented 2026-10-05 (WIX-120..127)**: `internal/preview` — dedicated project-scoped MyGo WebView Window strictly bound to approved loopback targets (`localhost`, `127.0.0.1`, `[::1]` + ports), `IsLoopbackURL` and `NormalizePreviewTarget` validate loopback addresses, `OnWillNavigate` intercepts external navigation (`e.PreventDefault()`) and diverts to system browser via `openExternalBrowser` preventing generic browser bloat; integrated into right panel Services listening ports; unit tested. **P20 Task Presets implemented 2026-10-05 (WIX-250..256)**: `internal/presets` — adapts Magpie profiles safely as New Task Presets storing only Shardlane-owned fields (Name, Provider, PromptTemplate, Mode), no API keys or credentials stored; atomic JSON store (`task-presets.json`) with built-in default templates (Feature Scaffold, Bug Investigation, Architecture Review); integrated into `page_new_task.go` form with interactive preset buttons and autofill, strictly enforcing that preset selection never launches automatically; unit & headless UI tested. **P16 Diagnostics & P17 Sanitized Export implemented 2026-10-05 (WIX-200..223)**: `internal/diagnostics` — safe `Snapshot` containing environment, Go/MyGo/Shardlane versions, and Herdr protocol; bounded `LogEntry` viewer reading up to 5,000 entries from `applog` with in-memory level and search filtering; strict sanitization via `SanitizeText` masking API keys/tokens/passwords (`[REDACTED_CREDENTIAL]`) and normalizing home paths to `~`; `ExportArchive` bundle with zero terminal text, zero transcripts, and zero secrets; integrated into `/settings/diagnostics` settings section with snapshot facts, filterable log scroll, and one-click sanitized export to clipboard; 4 unit tests + 2 headless UI tests pass. **P13 File Drop & P15 Terminal Find implemented 2026-10-05 (WIX-180..194)**: `nativeui/file_drop.go` — `SafeShellQuote` enforces max 256 paths, 64 KiB text limit, POSIX single-quote escaping, rejects any control characters (`\n`, `\r`, `\t`, `\x00`), never appends execution newlines; 3 unit tests; `internal/herdr/search_gate.go` — audited against Protocol 22 `pane.copy_search` schema with typed request/response models and RPC adapter, preserving invariant that no client-side VT shadow index is created; 2 unit tests. **0.9.0 Closure Audit Completed 2026-10-05**: created `docs/mygo-native-0.9.0-closure-audit.md` with complete 26-item verification matrix (all delivered capabilities `DONE`, `gh` auth / worktree / updater plugin / activity documented as `DEFERRED_PROTOCOL_GAP`), 9 hard architecture invariants proved, 16 Go packages 100% passing tests under `-race`, clean DMG package built (`405e2f4b…`) |
| 0.10.0 Git Workbench & Unified Primary Surface | `docs/mygo-native-0.10.0-git-workbench-primary-surface-plan.md` | `docs/prompts/mygo-native-0.10.0-git-workbench-primary-surface-implementation-prompt.md` | 0.9 actual-tree reality audit. Removes Lazygit; introduces one Terminal/Diff/Commit primary-surface owner; native Changes/Diff/Commit/branch workflows; and repairs inherited Files/Services/Scripts/Preview/FileDrop/TerminalFind/updater/versioning gaps before closure. |

Planning review notes (2026-10-05, post-0.4 closure increment):

- all three plans verified against the current tree: every cited Rust
  authority file exists, the MyGo 0.2.7 `Tray` and `Notification` APIs the
  0.7/0.5 plans rely on are confirmed present, and the plans' assumed 0.4
  prerequisites (launch transaction, integration health service, shared
  operational status model, Design System v2) are all landed and audited;
- no stale statements found (no references to the superseded
  "Start Agent disabled" state);
- the plans chain correctly: 0.5 §24 hands its primitives to 0.6, 0.6 §30 to
  0.7, and 0.7 §29 opens the 0.8 boundary (History management, Usage, Agent
  Inspector — implementation prompt already drafted).

## Completed 2026-10-05 — 0.4.0 (evidence; full matrix in `docs/mygo-native-0.4.0-audit.md`)

| Task | Status | Quick evidence |
|---|---|---|
| CLOSURE-01 History latest-wins | ✅ | cancel+generation latest intent; `TestHistoryListLatestRequestWins` (-race) |
| CLOSURE-02 Concurrent launch idempotency | ✅ | single in-flight owner per RequestID; barrier/conflict/waiter-cancel tests, `-race` clean |
| CLOSURE-03 Detail scroll owner | ✅ | `ui.Scroll` + `TrackScroll` + paging intents; `TestHistoryDetailScrollIntentsFollowPaging` |
| FW-01/02/03 MyGo 0.2.7 | ✅ | pin bumped; tidy/tests/build pass; render baseline + isolated interaction smoke |
| DS-01..04 Design System v2 | ✅ | `design_system.go` tokens + semantic statuses; `ds_components.go` primitives; History/Settings/New Task/Sidebar/Titlebar migrated |
| DS-05 official components | ✅ | `ui.List` (History), `ui.Collapsible` (thinking), `ui.Toast` (integration actions), Router/Breadcrumbs retained |
| DS-06 Outline spike | ✅ spike / NO-GO for 0.4 | `TestOutlineSpikeParity` passes; migration deferred to 0.5 to avoid a second expansion state machine mid-release |
| OPS-01..05 Operational status | ✅ | `operational.go` shared model; Titlebar attention + Sidebar rows consume it; OPS-05 dynamic sequence test; no polling |
| INT-01..07 Integration health | ✅ | strategy registry + `herdr integration status` adapter + `IntegrationHealthService`; Providers Settings page with strategy-gated actions, spinner/reconcile/Toast; route-entry/refresh/post-action cadence only |
| LAUNCH-01..03 transaction + harness | 🟡 | phase machine + uncertain-delivery fake-runtime harness ported and tested; Herdr transport adapter (agent.start/prompt/readiness) NOT ported — needs live protocol verification |
| LAUNCH-04 New Task picker | ✅ | capability + integration-health driven Ready/Setup/Updating/Deferred; `TestNewTaskProviderPickerStates` |
| LAUNCH-05 Start Agent | ❌ NOT DONE (by gate) | stays disabled with truthful copy until the verified transport lands |
| RENDER-01/02/03/06 | ✅ | `ui.List` virtualization; no render-path IO (audited); hidden-route behavior unchanged; render frames for all required states |

Deferred by plan: Chat (0.5), Remote/Mobile, Windows/Linux, Gemini adapter.

## Completed 2026-10-05 — 0.3.0 (evidence; full matrix in `docs/mygo-native-0.3.0-audit.md`)

| Task | Status | Quick evidence |
|---|---|---|
| SET-03 SettingsService | ✅ | `internal/app/settings_service.go`; load-failure keeps defaults, serialized updates, validation |
| SET-04 FileStore injection | ✅ | `main.go` resolves MyGo `PathUserData`; proven by the isolated launch smoke |
| SET-05/06 General+Terminal settings UI | ✅ | `page_settings.go` real controls; `TestSettingsGeneralPageUpdatesService`, `TestSettingsTerminalPageUpdatesService` |
| SET-07 Terminal presentation apply | ✅ | `terminalOptionsFromSettings` mapping test; new-Pane-only application |
| SET-08 Restart persistence | ✅ | `TestSettingsServiceSurvivesRestartOverFileStore` |
| HIST-05 Codex adapter | ✅ | `internal/history/codex.go` + `codex_blocks.go`; rollout fixture parity tests |
| HIST-07A/B/C SQLite catalog + identity + page cache | ✅ | `catalog*.go` (modernc.org/sqlite); window/invalidation/corrupt-miss tests |
| HIST-08 FTS/search | ✅ | trigram FTS5 + LIKE fallback; `TestCatalogSearchTermsAndScopes` |
| HIST-09A/B/C Scanner + HistoryService | ✅ | `scanner.go`, `service.go`; changed/missing/unreadable-root, bounded windows, anchor jumps |
| HIST-10/11/12 Native History list + detail | ✅ | `page_history.go`, `history_blocks.go`; headless UI tests + rendered-frame evidence |
| HIST-14 Sidebar Recent from service | ✅ | `feedRecentFromHistory` metadata-only feed; rendered in list evidence |
| AGENT-01 Capability model (prework) | ✅ | `internal/agent/capabilities.go`; Rust-authority parity table tests |
| AGENT-02 StartAgent DTO (prework) | ✅ | `internal/agent/launch.go`; explicit targets + validation + wire round-trip |
| AGENT-03 Idempotency (prework) | ✅ | `LaunchRegistry`: duplicate RequestID never re-launches |

Deferred by plan: HIST-06 Gemini (optional, skipped to protect the MVP),
AGENT-06A uncertain-delivery harness, Start Agent enablement, Chat.

## Completed since 2026-10-04 (evidence)

| Task | Status | Quick evidence |
|---|---|---|
| SHELL-01 Recent section parity | ✅ | `go test ./internal/nativeui -run TestSidebarRecentSectionRendersAndRoutes` |
| SHELL-02 Project tree expand/collapse | ✅ | `TestProjectAndTabExpansionArePresentationState` (expansion independent from runtime focus) |
| SHELL-03 Project tree state styling | ✅ | `TestRowTintSelectionOutranksHover` + `TestTreeRowStatePresentation` (`components.go` `rowTint`) |
| SHELL-05 Shortcut registry | ✅ | `shortcuts.go` table; `TestShortcutRegistryCoversRequiredShellActions`, `TestShortcutBackForwardUsesRouterHistory` |
| SHELL-06 Unified shell load/error state | ✅ | `shell_state.go` (`loading/ready/degraded/disconnected/error`); `TestClassifyShellState`, `TestShellStateMapsShellFields` |
| SHELL-07 Recent presentation model | ✅ | `RecentItem` in `sidebar_models.go`; covered by the Recent fixture test |
| SHELL-08 Safe window-state restoration | ✅ | `main.go` `StateKey: "main"`; placement owned by MyGo `window_state.go` (restore only on display intersection, else center primary) |
| SET-01 SettingsStore interface | ✅ | `internal/settings` `SettingsStore`; `go test ./internal/settings` |
| SET-02 File store | ✅ | `internal/settings/file_store.go` versioned + atomic + corrupt-safe; `go test ./internal/settings` |
| HIST-01 History domain models | ✅ | `internal/history/models.go`; `TestHistoryModelJSONUsesContractSlugs`, `TestAgentIDsMatchRustContract` |
| HIST-02 Project-key normalization | ✅ | `internal/history/path_key.go`; `TestNormalizePathKeyMatchesComponentCleanupWithoutCanonicalize` |
| HIST-03 Source locator | ✅ | `internal/history/locator.go`; `TestResolveSessionSourceLocatorMatchesMeasuredHerdrShapes` |
| HIST-04 Claude adapter | ✅ | `internal/history/claude.go` + `claude_blocks.go`; `TestClaudeAdapterParsesMainlineThinkingAndToolOutput`, `TestClaudeAdapterSkipsKnownMetadataAndCountsUnknown` |

M1 local exit (`go test ./internal/nativeui`) passes; full `go test ./...` and
`go tool mygo build` (Shardlane.app + DMG) pass. Selection semantics moved to
client-local presentation state in `nativeui/selection.go` (Herdr `focused_*`
fields are bootstrap/fallback facts only).

## Next queue

| Order | Task | Can parallelize |
|---:|---|---|
| 1 | LAUNCH transport: verified Herdr `agent.start`/prompt/readiness adapter (live schema) | after 0.4 |
| 2 | LAUNCH-05 enable Start Agent behind LaunchRegistry + transport | after 1 |
| 3 | ui.Outline production migration (spike passed) | yes (0.5) |
| 4 | HIST-06 Gemini adapter | yes |
| 5 | Full agent_titles heuristic port (rename labels) | yes |
| 6 | HIST-13 Resume intent | after HIST-09 |

## Per-task handoff template

```text
Task:
Objective:
Inputs / old behavior reference:
Output:
Non-goals:
Dependencies:
Files expected:
Fast verification:
Completion evidence:
Status: TODO | DOING | DONE | BLOCKED
Notes:
```

## Success definition

The migration is complete only when:
- MyGo is the default desktop client;
- History/New Task/Chat are Native UI and use Go domain services;
- Herdr remains the sole runtime authority;
- Remote/Mobile contracts remain compatible;
- promised macOS/Windows/Linux gates pass;
- logs remain bounded/private;
- performance and soak gates pass;
- Rust compatibility implementations can be removed safely.
