# Shardlane Architecture — Canonical Source of Truth

Status: **Canonical / as-built + approved-next architecture**
Baseline: 2026-08-25
Runtime baseline: Herdr 0.8.x. Current transitional code still accepts socket protocol 19; the approved TUI-only cutover raises the minimum fully supported protocol to 20 because native shell → hosted TUI navigation uses protocol-20 focus operations. Known wrapper contracts remain version-checked rather than assuming unlimited forward compatibility.
UI baseline: GPUI 0.2.2, gpui-component 0.5.1 + gpui-component-assets 0.5.1

This file is the single architectural source of truth for Shardlane. Other documents may define interaction details, execution order, progress evidence, or handoff context, but they must not redefine ownership.

## 1. Product identity

**Shardlane is the product, application, and client brand.**

**Herdr is the backend/runtime used by Shardlane.** Herdr remains authoritative for:

- runtime workspaces, which Shardlane presents to users as **Projects**;
- tabs;
- panes and pane layouts;
- terminal sessions and scrollback;
- Agent runtime state;
- process lifecycle;
- persistence and runtime reconciliation.

Herdr technical names are not branding leftovers. Real Herdr APIs, protocol methods, CLI commands, runtime types, errors, socket paths, and integration modules must continue to use the Herdr name when they describe the backend factually.

Shardlane owns:

- **Workspaces**, the ordered/color-coded client contexts that group Projects;
- **Project metadata persistence**: project-to-workspace assignment (`project_paths`), manual path overrides (`project_path_overrides`), and project display preferences. This metadata is the client-side source of truth for how Projects are organized, independent of Herdr runtime lifecycle;
- native macOS presentation and navigation;
- transient pointer/selection/IME/menu state;
- local appearance and interaction preferences;
- the client-side view/projection cache;
- read-only browsing/indexing of external coding-agent conversation history;
- Shardlane-owned search/index storage for that read-only history;
- Script semantic definitions and UI projection while the Herdr protocol has no protocol-native Script resource. Script Tabs/Panes/PTYs/processes remain Herdr-owned.

Shardlane must never become a second runtime authority.

**Approved Next — Agent Provider integration coverage.** Every Provider already present in `shardlane-history::AgentId::ALL` must also have an explicit Herdr runtime-integration strategy. Shardlane may provision Herdr's official Agent integrations and may install narrow provider-native hooks/plugins for capability gaps, but those adapters must report lifecycle/session identity back into Herdr (`pane.report_agent`, `pane.report_agent_session`, `pane.release_agent`) rather than maintaining a parallel client-side Agent-status monitor. When Herdr intentionally keeps lifecycle authority on screen-manifest detection, Shardlane must not override it with an incomplete custom lifecycle source. The per-Provider coverage matrix is maintained in the internal engineering archive (not published with this repository).

**Implemented (2026-08-30) — Agent-first semantic service convergence.** Desktop, Remote and Mobile converged on one UI-independent Host application path for normal Agent/Conversation semantics. `AgentRuntime` remains the low-level Herdr SPI; product-level Agent launch, Conversation query/prompt, safe queued working follow-up, History continuation and Context Transfer/Handoff belong to `shardlane-host`. Normal semantic Agent operations use Herdr Agent APIs rather than PTY command/prompt typing. History continuation plans one of `AlreadyLive`, exact same-Provider `NativeResume`, or lossless `ContextTransfer`; different-Provider continuation and Live Handoff reuse the same transfer engine, and presentation-bounded/truncated transcript Markdown is never a silent transfer fallback. Raw Terminal input remains an explicitly separate terminal-only capability. Host semantic mutations return exact identities without implicitly moving another client's UI; Desktop applies its existing `FocusIntent` only after success. Landing evidence: `HostConversationService` (query/prompt/continue), `run_agent_launch` (canonical launch transaction), `ConversationSessionManager` (one subscribed live session owner), `ConversationFollowUpQueue` (safe `Send after turn`), `run_context_transfer` + `TransferArtifactStore` (lossless transfer), `plan_history_continuation` (AlreadyLive → NativeResume → ContextTransfer), `run_live_handoff`, `provider_product_capabilities`, and `AgentSessionInsight`; the PTY-typed launch/resume, the 12K Markdown continuation fork, and the GUI/Remote Continue transaction mirrors are deleted. Verification levels for M1–M9 are recorded in the internal engineering archive.

**Done 2026-08-27 — product pruning.** Notes, Bookmarks, and Annotation have been deleted outright: modules, routes, actions, config writers, assets, feature-only tests, and the annotation-only native capture/image dependencies (objc2-core-foundation/core-graphics/image-io/screen-capture-kit) are gone from the codebase. Existing user-created files/packages are not destructively erased by startup migration. Browser/right panel, Activity, Scripts/Services, Shortcuts, History, New Agent, Remote/Mobile, and Workspace/Project organization remain retained capabilities unless a later product decision explicitly changes them.

**Approved Next (2026-08-30) — Same-Session / Dual-Surface Chat interactions.** Terminal View and Chat View are two product surfaces over the **same Herdr-owned live Agent session**. The native Provider CLI/TUI remains running inside Herdr. Conversation read projection combines the existing read-only transcript/live decoder, narrow Provider hook/plugin/extension events, and Herdr's exact Agent/session/lifecycle state. Ordinary Chat Composer messages continue through the canonical Host Conversation transaction and Herdr `agent.prompt`; a question/approval that belongs to the current in-flight turn is a separate Host-owned `ConversationInteraction` and may be resolved only through an exact Provider hook/plugin response bridge. `blocked` must never be answered by converting the choice into a generic prompt or guessed PTY keys. Companion Shardlane hooks/plugins must coexist with Herdr integrations and must not steal lifecycle authority from Herdr. Provider-native App Server/RPC/native server transports are optional only after a measured **Same-Session Gate** proves one Agent core/session, single-writer ownership, retained native TUI, safe fallback, and acceptable performance. **ACP is explicitly frozen as of 2026-08-30: no ACP dependency, adapter, client/server/proxy, capability field, Host DTO, feature flag, or preparatory runtime code may be added until a later explicit product decision unfreezes it. Passing the Same-Session Gate does not itself authorize ACP work.** The decision/evidence/execution set for this direction is maintained in the internal engineering archive (not published).

### Gradual data ownership evolution (approved direction)

**Core principle**: Shardlane persists project/workspace organizational metadata as the stable source of truth. Herdr remains the sole session/runtime authority (PTY, process, scrollback, tabs, panes).

#### Current state (Phase 0 — as-built)

```text
┌────────────────────────────────────────────────────────────────────┐
│                        Shardlane (client)                          │
│                                                                    │
│  config.json                                                       │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │ workspaces: [                                                │  │
│  │   { id, name, color, project_paths: ["/path/a", "/path/b"] }│  │
│  │ ]                                                            │  │
│  │ project_path_overrides: { "runtime-ws-id" → "/manual/path" }│  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                    │
│  ProjectIndex (disposable, rebuilt on every render cycle)          │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │ Merges: Herdr runtime workspaces                             │  │
│  │       + Script registry project_paths                          │  │
│  │       + path_overrides                                       │  │
│  │ Produces: ProjectProjection[] with key, label, path, tabs,   │  │
│  │           panes, agents, scripts                               │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                    │
│  Workspace resolution pipeline:                                    │
│  1. active_client_workspace_runtime_ids_with_overrides()           │
│     ↳ Determines which Herdr runtime workspace IDs belong to the  │
│       currently active Shardlane Workspace                         │
│  2. visible_sidebar_projects()                                     │
│     ↳ Produces VisibleSidebarProject[] for the active workspace    │
│  3. project_owner_workspace_id_for_config()                        │
│     ↳ Given a path, finds which Shardlane Workspace owns it       │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
                            │ socket RPC
                            ▼
┌────────────────────────────────────────────────────────────────────┐
│                        Herdr (runtime)                             │
│                                                                    │
│  workspace.list → [{workspace_id, cwd?, label?, agent_status?}]   │
│  tab.list      → [{tab_id, workspace_id?, title?}]                │
│  pane.list     → [{pane_id, workspace_id?, cwd?, ...}]            │
│  agent.list    → [{terminal_id, workspace_id?, cwd?, state?}]     │
│  events stream → workspace.created, pane.updated, agent.updated…  │
│                                                                    │
│  Session truth: PTY lifecycle, scrollback, process PID, terminal   │
│  control channels, pane layout, tab ordering                       │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
```

#### Reconciliation algorithm (current)

When Shardlane connects/reconnects or receives events:

```text
On bootstrap / reconnect:
  1. Fetch workspace.list, tab.list, pane.list, agent.list from Herdr
  2. Rebuild ProjectIndex from runtime state + ScriptRegistry
  3. For each runtime workspace:
     a. Resolve effective_path = path_overrides[runtime_id] ?? runtime.cwd ?? pane.cwd
     b. Match effective_path against WorkspaceConfig.project_paths
     c. Unmatched → falls to first (Default) WorkspaceConfig
  4. Filter by active_workspace → visible projects

On Herdr runtime workspace created:
  - New runtime workspace auto-appears in Default workspace (no explicit path)
  - User can "Move to Workspace" or "Set Project Path" to assign it

On Herdr runtime workspace deleted:
  - ProjectIndex rebuild removes it from live projection
  - WorkspaceConfig.project_paths entries remain (orphaned but harmless)
  - path_overrides entry remains until user clears it
  - No active disruption: project simply disappears from sidebar

On reconnect after crash:
  - Herdr may have different runtime workspace IDs (not stable across restarts)
  - Shardlane's project_paths are path-based, not ID-based → survive runtime changes
  - path_overrides are ID-based → may become stale (handled: stale overrides are harmless)
```

#### Edge cases and invariants

| Scenario | Behavior |
|----------|----------|
| Two runtime workspaces share same path | Both appear in same Shardlane Workspace; distinct sidebar rows |
| User sets override path to match another workspace's path | Both runtime workspaces appear in the owner workspace |
| Herdr workspace's cwd changes at runtime | Next ProjectIndex rebuild picks up new path; may shift workspace |
| User deletes a workspace that owns projects | Projects fall back to Default; config.project_paths cleared |
| Config file manually edited externally | File watcher detects change, triggers reload and re-render |
| Herdr disconnects | Sidebar shows stale cached ProjectIndex; reconnect triggers full rebuild |

#### Phase 1 — Orphan cleanup and path stability (next)

**Goal**: Prevent "ghost" projects and make workspace assignment deterministic after Herdr restarts.

Steps:
1. On reconnect, sweep `project_path_overrides`: remove entries whose `runtime_workspace_id` no longer exists in `workspace.list` AND whose path is not in any `WorkspaceConfig.project_paths`.
2. On reconnect, sweep `WorkspaceConfig.project_paths`: for each path, if no runtime workspace resolves to it AND no override points to it, mark it as "dormant" (keep in config, hide from sidebar unless user shows archived projects).
3. Add `last_seen_at_ms` to config path entries for garbage-collection heuristics.

Verification: unit tests for sweep logic; manual smoke: kill Herdr, restart with different workspace IDs, confirm sidebar stabilizes.

#### Phase 2 — Session restore intent (future, pending Herdr API)

**Goal**: When Herdr restarts fresh, Shardlane can recreate runtime workspaces from its persistent config.

Prerequisites:
- Herdr `workspace.create(cwd)` API stable and idempotent
- Herdr `tab.create(workspace_id)` API available
- Clear semantics for "create workspace with specific cwd" vs "reattach to existing"

Steps:
1. On connect, if Shardlane has `WorkspaceConfig.project_paths` entries that have no matching runtime workspace:
   a. Prompt user: "Restore sessions for [path1, path2]?" (or auto-restore based on preference)
   b. Call `workspace.create(cwd=path)` for each
   c. Store new `runtime_workspace_id` in `path_overrides` for immediate correlation
2. Respect `one_shot` Scripts and restore them if configured

Verification: integration test with mock Herdr; manual smoke: fresh Herdr instance + existing config.

#### Phase 3 — Full Shardlane-primary model (long-term)

**Goal**: Shardlane defines the project catalog; Herdr sessions are ephemeral materializations.

```text
┌──────────────────────────────────────────────────────────────┐
│                    Shardlane (primary)                        │
│                                                              │
│  Project Registry (persistent)                               │
│  ┌────────────────────────────────────────────────────────┐  │
│  │ projects: [                                            │  │
│  │   { id, name, path, workspace_id, icon?,               │  │
│  │     session_intent: { auto_start, shell, env } }       │  │
│  │ ]                                                      │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                              │
│  Session Manager                                             │
│  ┌────────────────────────────────────────────────────────┐  │
│  │ For each Project with session_intent:                  │  │
│  │   if materialized → track runtime_workspace_id         │  │
│  │   if not materialized → create on demand via Herdr API │  │
│  │   if runtime disappears → mark as suspended            │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                              │
└──────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌──────────────────────────────────────────────────────────────┐
│              Herdr (ephemeral session runtime)                │
│  Creates/destroys sessions on Shardlane's request            │
│  Still owns: PTY, process, scrollback, pane layout           │
└──────────────────────────────────────────────────────────────┘
```

This phase requires significant Herdr protocol evolution and is explicitly deferred.

#### Migration safety rules

- Each phase is independently mergeable; the system is usable if any future phase never ships.
- Phase 0 (current) is the production baseline; no regression from today's behavior.
- Config schema changes use `version` field and additive-only fields with `#[serde(default)]`.
- `project_path_overrides` keyed by runtime ID are inherently ephemeral; code must tolerate stale entries gracefully (filter, ignore, eventually GC — never crash).
- Path normalization uses the cached `normalized_project_path()` function; equality checks use `project_paths_match()` which handles trailing slashes, canonicalization, and case sensitivity.

#### Herdr change detection (sync resilience)

Shardlane cannot assume Herdr state is stable between event-stream reconnects. Defensive behaviors:

1. **Full rebuild on reconnect**: `visible_state()` after event-stream reconnection triggers a complete `ProjectIndex::build()` and sidebar re-render.
2. **No runtime ID persistence for display**: Sidebar renders from `ProjectIndex` rebuilt per frame; stale runtime IDs in `path_overrides` are filtered out gracefully.
3. **Event-driven incremental updates**: `workspace.created`, `workspace.deleted`, `pane.updated` patch the in-memory `HerdrState` and trigger incremental sidebar notifications without full `visible_state()` calls.
4. **Graceful degradation**: If Herdr is unreachable, the sidebar shows "Reconnecting" state; no workspace management operations crash; config saves continue to work for offline changes.

## 2. Non-goals

V1 does not add:

- a replacement PTY/session manager;
- a second pane-layout model independent of Herdr;
- direct external-terminal orchestration for Herdr runtime sessions;
- general-purpose browsing as a product surface;
- plugin/marketplace UI;
- cloud accounts;
- telemetry.

One narrow exception exists: the right panel ships a local web preview
surface (WKWebView) for developer workflows already rooted in a Project —
previewing a local dev-server port, rendered documents, and similar
localhost artifacts. It is an inspect surface, not a browser product: no
tabs/history/bookmarks beyond the per-surface Omnibox, no remote browsing
norms, and one-click hand-off to the system default browser for anything
outside the local preview scope. PTY/process/agent authority never moves
into it.

If a runtime capability is missing from the Herdr protocol, the preferred fix is to extend Herdr rather than bypass it inside Shardlane.

## 3. Layer model

Implementation decisions follow this ownership ladder:

1. **Herdr runtime/protocol API** — workspace/tab/pane/layout/process/runtime authority.
2. **vendored libghostty-vt** — terminal parser, grid, selection, formatting, mouse/focus/paste encoding, hyperlinks, viewport semantics.
3. **gpui-component 0.5.1 + gpui-component-assets 0.5.1** — standard native desktop components, interaction behavior, and the matching default `IconName` asset bundle.
4. **GPUI 0.2.2** — lower-level native window/input/render primitives when the component library has no suitable owner.
5. **Proven compatible implementation pattern** — evidence only; never a permanent product identity or alternate architecture.
6. **Custom Shardlane code** — only when the earlier layers cannot own the capability cleanly.

Custom code must have a concrete reason to exist: missing API, incompatible ABI, ownership conflict, or measured limitation.

## 4. Workspace and Project model

Shardlane adds one client-owned grouping layer above the Herdr runtime without duplicating runtime authority:

```text
Shardlane Workspace
  └─ Project  ← projection of one Herdr runtime workspace
      ├─ Tab
      │   └─ Pane
      ├─ Script
      ├─ Agent
      └─ History Conversation
```

A **Workspace** is a Shardlane-owned context. It has client-owned identity, name, order, color, active state, and Project membership. Deleting or reordering a Workspace must never close or delete a Herdr runtime resource. Exactly one Workspace is active; when only one exists the compact Workspace switcher is hidden.

A **Project** is the user-facing projection of a Herdr runtime workspace. Herdr's real `Workspace` type, `workspace_id`, `workspace.*` protocol methods, and runtime events remain technically named Workspace at the integration boundary. Shardlane code above that boundary uses Project terminology whenever it describes the product/domain concept. `ProjectIndex` is a disposable derived index that correlates runtime workspace identity, project path, Tabs, Panes, Agents, and Scripts; it is not a second source of truth.

Current projection policy:

- all Herdr runtime workspaces are projected as lightweight **Project** Sidebar/search metadata;
- all Tab metadata is projected globally so any manually expanded Project can show its Tabs without changing runtime focus;
- Shardlane owns its ephemeral selected Project/Tab after startup; Herdr global focus is only an initialization/fallback signal and must not steal the current client view while the selected objects still exist;
- Herdr focus events are therefore not subscribed as shell-navigation drivers; navigation selection is fully client-owned: Project/Tab/Pane selection and directional pane navigation update the local projection only and never emit Herdr `workspace.focus`/`tab.focus`/`pane.focus`/`pane.focus_direction`; `pane.updated` focus facts patch Agent/Pane metadata but never flip the client selection; structural layout results/events keep geometry/zoom facts while preserving the local selection, falling back to Herdr's focused Pane only when the local selection no longer exists. Herdr-side focus churn (including an explicit shared-TUI action switching Tabs/Panes) must not implicitly rewrite Shardlane's semantic navigation;
- global navigation reconciliation refreshes only Project/Tab metadata;
- pane create/close/exit/move events refresh the visible Tab surface only when they affect the currently selected Project; unrelated Project pane churn must not touch the current terminal surface;
- layout events carry their own snapshot and update only the selected Tab when applicable;
- structural commands consume authoritative Herdr return payloads/events directly when those already contain the created/moved/focused Pane, Tab, runtime Workspace, or layout; Shardlane fetches only metadata that is genuinely absent rather than following every mutation with `visible_state()`;
- `visible_state()` remains appropriate for bootstrap, explicit manual refresh, and exceptional recovery, not as the default completion path for high-frequency Pane/Tab/Project actions;
- global Agent state is projected independently for navigation and status; protocol-19 `pane.updated` payloads patch known Agent/Pane status metadata directly and list-wide `agent.list` remains a fallback only when the event cannot identify an existing Agent projection;
- high-frequency terminal output does not rebuild the entire shell projection;
- the native shell owns one hosted Herdr TUI transport slot (plus the bounded
  Lazygit auxiliary slot); it does not create writable terminal controllers per
  visible Pane. Remote clients use the separate Host-owned shared TUI session
  route and disclose its global focus/resize semantics.

The expensive terminal/render path must remain visibility-scoped.

Replaceable client background work follows owner lifetime. When a GPUI job becomes obsolete because the user selected another object or left the surface, Shardlane keeps its job handle and drops it to cancel the obsolete work. `.detach()` is reserved for work that intentionally outlives the initiating UI state.

## 5. Terminal architecture

**Current — Herdr TUI is the sole normal terminal surface.** This means the hosted Herdr TUI is the sole **normal Terminal implementation/surface**; it is not a claim that no other Agent *presentation* may exist. An approved-next semantic Chat View (see §6) renders the same Herdr-owned Agent session without becoming a second terminal implementation. Landed 2026-08-27, with the bounded auxiliary-tool (Lazygit) exception landed 2026-08-29: Shardlane keeps its retained native UI/UX and product features while deleting the direct Embedded per-Pane terminal controller/render path. The normal work surface owns exactly one **primary** `herdr` TUI process behind one local PTY, one private Ghostty terminal-emulator model, and one GPUI presentation/input bridge. While a visible Right Panel is explicitly on Lazygit, Shardlane may own **at most one ephemeral auxiliary tool process/PTY** with its own private Ghostty model and session state; it is bound to the active Project's resolved Git root, is stopped on hide/close/Project switch, never occupies Herdr's global terminal slot, and never creates a per-Project process fleet. A detected Lazygit older than the certified minimum may still use the stable CLI startup arguments, but receives no Shardlane overlay and is visibly marked “update recommended”; missing CLI remains a blocking native state. Native Sidebar/Search/History/New Agent/Activity/notification navigation converges on one Herdr focus-intent seam; native secondary surfaces may cover/block terminal input without tearing down the primary host. Remote clients retain independent navigation and move the Mac TUI only through an explicit cross-client action. The cutover raises the minimum fully supported Herdr socket protocol to 20 because native shell → TUI focus relies on protocol-20 `workspace.focus`/`tab.focus`/`pane.focus`/`agent.focus`; older servers receive an explicit upgrade state, never an Embedded fallback. `TerminalSurfaceMode`, per-Pane `terminal session control --takeover`, `PaneTerminalSlot`/`pane_terminals`, Embedded native pane layout rendering, deep-history reseed/scrollbar/controller branches, and mode-switch recovery are deleted. `portable-pty`, Ghostty models, IME, selection/copy/paste, mouse/trackpad encoding, terminal appearance, and host restart/failure UX remain private hosted-surface infrastructure; only the primary host participates in Herdr runtime focus. Ordinary hosted-TUI right-click is owned by Shardlane's native GPUI terminal menu (Copy/Paste/Select All only): Right press/motion is withheld from the PTY so Herdr's TUI context menu cannot open underneath it; if this interception proves correctness-breaking in real use, revert to Herdr's menu wholesale rather than supporting two right-click modes.

Terminal ownership is split deliberately in the current transitional implementation:

- `herdr.rs` — Herdr socket/projection wrappers and structural runtime operations;
- `terminal_stream.rs` — persistent Herdr terminal-control transport and `ManagedTerminal` lifecycle;
- `ghostty.rs` — vendored libghostty-vt FFI and terminal semantics;
- `terminal_view.rs` — paint, hitboxes, selection presentation, scrollbar, and links;
- `input.rs` — keyboard/input translation.

The vendored libghostty-vt binary is the ABI authority. New FFI declarations must be verified against the bundled artifact rather than assumed from an unrelated upstream header revision.

Terminal geometry has one client-owned coordinate source derived from GPUI's resolved terminal-font metrics. Grid sizing, Herdr cell-pixel resize, text-run placement, cursor, selection, mouse hit-testing, links and IME/candidate bounds must consume that same geometry; font-size ratios and ordinary flowing text layout are not authoritative terminal coordinates. Styled frame runs retain their Ghostty grid column/span so GPUI paint remains anchored to terminal cells, including across wide-character spacer columns.

Terminal polling is controller-scoped rather than process-global. A poll loop must terminate when its controller/token ownership becomes stale; it must not remain as a dormant periodic wakeup. Persistent controller output provides a coalesced wake signal so decoded terminal frames interrupt idle backoff immediately; the adaptive timer remains a fallback for local housekeeping. Active input/output/scroll/VT work uses the low-latency cadence, while idle focused and background panes back off independently and reset immediately when activity returns.

**Removed 2026-08-27 — the former dual Embedded/Herdr-TUI surfaces (historical).**
The old `Embedded`/`Herdr TUI (Experimental)` settings selector, per-Pane
controllers, and mode-switch lifecycle no longer exist. This historical note is
kept only to explain the deletion; current native terminal ownership is the
single hosted Herdr TUI described above. The user's normal Herdr config remains
the runtime source of truth, and hosted-child failure is recoverable presentation
failure rather than proof that the Herdr runtime itself is offline.

Historical Embedded mechanics (path deleted 2026-08-27): full visible-row/cell `TerminalFrame` extraction was the former correctness baseline. Dirty-row/incremental projection is an **Approved Next** optimization and may only replace it after the exact vendored Ghostty ABI/damage contract is verified and invalidation behavior is regression-tested.

Visible-pane attach starts the Herdr terminal controller and reads retained ANSI history concurrently, then feeds that retained history into the same Ghostty model before the live controller frames take over. This reduces serialized attach latency without changing scrollback semantics. Deep history beyond the seeded window loads on demand: scrolling to the top of the local scrollback triggers a re-read of `pane.read(source=recent)` with a quadrupled line budget and a whole-model reseed on a background thread (generation-guarded against concurrent grid changes, viewport anchored by distance-from-bottom, growth capped by the local Ghostty scrollback capacity and marked exhausted when Herdr retention stops growing). Reseeding is a rebuild, not a prepend, so it never violates history ordering. A true two-phase “recent viewport first, arbitrary older retained history later” page-in and true unbounded remote scrollback remain a **Protocol Gap**: protocol 19 exposes no offset/range read and the vendored libghostty-vt exposes no prepend/import-scrollback ABI.

Window/pane resize follows a hold-frame contract: the local Ghostty model reflows immediately, but frame projection is held until the first authoritative full frame after the Herdr `terminal.resize` RPC (probe-verified: Herdr pushes a reflowed full frame on resize even for an idle shell, so the hold window is one round trip) or a 200 ms fallback timeout. The resize RPC itself is trailing-edge debounced (~40 ms) so drag gestures do not issue a SIGWINCH storm; local geometry and painting stay immediate. This removes the “locally mis-reflowed frame followed by the correct Herdr frame” discontinuity while keeping the shell path effectively instant.

## 6. Native shell architecture

Shardlane has one canonical native shell and one canonical Sidebar.

**Approved Next — Activity and operational status.** Activity is a global Shardlane presentation over the existing global Herdr Agent index plus Shardlane Script/Service runtime projections. It is not scoped to the active Shardlane Workspace and it must not create a second Herdr event subscription, polling loop, terminal controller, or durable runtime-state database. The existing reconciled Herdr event stream remains authoritative. Shardlane may persist only bounded user-handling receipts such as “this exact completion revision was reviewed”. Operational status is modeled on two axes: runtime lifecycle and user attention. The shared attention priority is `NeedsAttention > Working > ReadyForReview > Idle > Resolved`; blocked Agents and failed Services therefore outrank working items because they require user action. The title-bar Activity control, Project rows, Agent/Pane rows, and collapsed Agents/Services headers must consume one shared aggregation/status-glyph implementation instead of separately interpreting raw strings. The Activity control lives beside the title-bar Sidebar toggle and may activate a Sidebar Activity route that replaces the normal Projects/Agents/Services body without introducing an alternate Sidebar.

**Removed 2026-08-27 — Project Notes and Bookmarks.** Both product branches were deleted by the TUI-only convergence: modules, sidebar/header entries, actions, shortcut entries, config fields, bundled note-editor web assets, and their tests are gone. Shardlane must not reintroduce them. Previously created user files on disk are not destructively erased; cleanup of old local data remains a separate explicit user action if ever needed. The right-panel local web preview (Browser) is unaffected.

**Current — Browser Profiles (retained).** The built-in macOS browser remains a native WKWebView surface whose identity is explicit Browser Sessions created with a Browser Profile configuration. Shardlane-owned Browser Profiles use WebKit website data stores for cookie/cache/site-data isolation; Shardlane must not read or copy external browser credential/cookie databases.

**Removed 2026-08-27 — Annotation.** The DOM-annotation bridge and the Notate-style global ScreenCaptureKit capture/mark/export domain were deleted by the TUI-only convergence, including the bundled `annotation-bridge` asset, the Screen Recording Info.plist usage description, and the annotation-only `objc2-image-io`/`objc2-screen-capture-kit`/`objc2-core-graphics`/`objc2-core-foundation` dependencies. Do not reintroduce them; Browser keeps its WKWebView infrastructure untouched.

**Approved Next — Shortcut Registry.** Application shortcuts have one registry that owns stable command ID, platform default/user binding, scope, conflict semantics and Terminal-consumption policy. macOS keeps Cmd defaults while future Linux/Windows profiles map the same Action IDs to Ctrl; explicit user overrides remain unchanged. Tooltips/menu labels consume the resolved registry binding. Terminal input must not depend on a second manually maintained list of global shortcuts to decide which keystrokes to swallow. Text inputs, Terminal, Browser and other focused surfaces receive explicit scopes; only commands allowed in the active scope may preempt the focused surface. The hosted TUI and the observer derive their swallow decisions from this one resolved registry — never from a second hand-maintained chord list.

**Approved Next — Agent Chat semantic view and shared Agent UI.** The hosted Herdr TUI remains the sole Terminal implementation while an alternate **Chat View** presents the same Herdr-owned Agent session as a provider-neutral semantic work timeline (user prompt, assistant answer, reasoning, tool activity, file-change/diff, settled-work folding, response footer). The Chat View is a semantic sidecar projection, not a runtime: Claude Code, Codex, and Pi CLI processes continue to run only inside the Herdr TUI; Chat never launches a second provider process, never parses TUI/ANSI screen text for semantics, and never becomes a second conversation authority. The live semantic source is the provider's own persisted session file, decoded incrementally by a dedicated live-decoder seam in `shardlane-history` (`live/`) that reuses the existing Claude/Codex/Pi adapter interpretation rules; exact `(provider, native session id)` correlation comes from Herdr `AgentSessionInfo.value` matched through the existing History resume-identity rules, never from cwd/mtime guessing. Ordinary prompts are submitted only through the Herdr `agent.prompt` seam in verified sendable states; blocked/unsupported interactions explicitly fall back to Terminal. Agent status (working/blocked/idle/done) remains Herdr-authoritative and may render a Working state before the provider file appends. Agent-facing presentation primitives are owned by one shared `agent_ui` module (`AgentComposer` extracted from New Agent, shared conversation/activity/Markdown presentation): New Agent, History Detail, and Chat consume the same Composer and conversation visual language while keeping separate lifecycles — New Agent owns launch configuration, History owns its read-only catalog/paging/cache/search/Continue semantics, Chat owns its live source binding, incremental tail, generation guards, follow-tail, and prompt interaction. This direction adapts proven author-owned presentation work for transcript folding, work/activity presentation, incremental Markdown, streaming performance discipline, and composer polish; adapted portions are author-owned code intentionally relicensed for Shardlane, not GPL-derived third-party imports, and any genuinely third-party material (e.g. Lucide) keeps its required notice in `THIRD_PARTY_NOTICES.md`. No provider daemon/runtime, input engine, or menu system from that earlier work is part of this direction. Treat remaining stages as in-progress rather than landed.

The shell uses gpui-component/GPUI for:

- root/window structure;
- title bar;
- resizable layout;
- dialogs and input;
- menus/context menus;
- searchable lists;
- settings controls;
- native focus and pointer behavior.

Global navigation Search, Project picker, and Tab picker share one root-owned searchable-list overlay. Entry points must not duplicate picker implementations or rely on title-bar-specific hit-testing behavior. The overlay owns picker focus/lifetime, is rendered above the canonical shell, and uses the searchable input itself as the primary visual anchor rather than duplicating a second internal title bar. Its width/height adapt to the host window while retaining bounded desktop caps; result rows consume the List's actual available width (`w_full`) instead of a guessed picker pixel width, and every List item uses one fixed height because gpui-component List requires uniform row height. List-native empty states explain unmatched queries, keyboard hints use lightweight keycap affordances, and clicking the backdrop closes the picker while clicks inside the palette stop propagation. Global Search treats Workspace and Project as distinct scopes: `#workspace` / `workspace:` target Shardlane-owned Workspace contexts, while `#project` / `project:` target Projects; `#script` includes both durable Scripts and read-only detected services. Unique hash-prefixes complete semantically (`#pro` → `#project`, `#work` → `#workspace`) and surface the canonical completion in result metadata, while ambiguous prefixes such as `#p` remain literal rather than being guessed. A Workspace search scope expands to the Project paths owned by that Workspace before History metadata/message-body FTS runs off the UI thread after a short debounce. Runtime Project paths resolve from the best available authoritative Herdr projection (`Workspace.cwd`, then Pane cwd, then Agent cwd) because Herdr 0.8.x runtime workspace metadata may omit cwd. Message-body hits retain the indexed `seq`, so selecting a result opens detail-only History around the matched message without parsing every transcript during search. Project and Tab pickers never query History.

Application typography is semantic and centralized in `theme.rs`: app title, section title, list title, body, description, metadata, and decorative text. Fixed application UI must use those tokens rather than introducing page-local pixel font sizes. Terminal glyph size remains a separate user-configurable terminal setting and does not inherit fixed application typography.

The native Header is visually continuous with the shell instead of being a third chrome surface. While the Sidebar is visible, the Header segment above it uses exactly the Sidebar surface color and has no visible bottom divider; from the authoritative Sidebar width boundary to the right edge, the Header uses the content background and may keep one subtle bottom divider. The Sidebar toggle lives at the trailing edge of the left Header segment, while Workspace/Tab breadcrumbs start in the content segment. Collapsing or auto-collapsing the Sidebar converts the full Header to the content surface. Header operational state is contextual rather than permanent: a shared pure `OperationalSummary` derives blocked/working Agent counts and failed/active Script counts from the existing in-memory projections. Blocked/failed attention may remain visible regardless of Sidebar state; ordinary working/active state is only a Header fallback when the Sidebar is hidden, so the Header does not duplicate an already-visible Sidebar. Operational indicators are actions: with a visible Sidebar they reveal the corresponding section, while auto-collapsed layouts jump directly to the highest-priority projected Agent/Script. Healthy Herdr connection state occupies no persistent chrome; offline state is an explicit reconnect action. Auto-collapsed layouts must not render a dead Sidebar-toggle affordance; they expose compact Search/History Header fallbacks instead. This summary is presentation-only and is the future source for the macOS status item as well.

There is no alternate/reference-branded Sidebar mode. The global `History` entry remains a permanent library surface for every indexed Conversation, including projects that are not currently represented by a Herdr Workspace, but it is a compact icon action beside the global Search field rather than a full-height Sidebar row. History owns its own Refresh action; the Sidebar does not duplicate History refresh/reconnect chrome. Workspaces are the primary Sidebar hierarchy: the `Workspaces` and `Agents` section headers remain fixed while their item regions scroll independently, and each section can be collapsed without changing Herdr runtime state. `New Workspace` and `New Script` are trailing `+` actions on their respective section headers, not navigation rows; each button must stop event propagation so creation never toggles the section disclosure. Expanded empty sections render concise semantic empty states instead of blank space. Collapsed Agent/Script headers may expose attention/activity aggregates only when that section is hidden, and the Script list uses a viewport-relative bounded maximum height so it cannot consume the Workspaces/Agents area on short windows. Each Workspace is a folder-like disclosure node; any manually expanded Workspace renders its projected live Tabs/Panes and then matching read-only historical Conversations directly as Workspace-level secondary rows rather than behind another History disclosure. A multi-Pane Tab projects its Pane children from Herdr `pane.list`/`pane.layout`; rows follow authoritative spatial layout order, focused/zoomed state comes from the live Tab surface, and preload/in-flight state is presentation-only. Historical rows are ordered by conversation update time, query only the currently requested bounded metadata window, reveal 10 sessions initially and 10 more per explicit request, and open the canonical History surface in detail-only mode. The History transcript is a single-column, full-width chronological timeline rather than left/right chat bubbles. On wide windows the History library may present conversation list and detail side by side; below the compact-width threshold it becomes list-first navigation and opens the selected detail full-width with an explicit Back action. Hidden compact-layout detail must not be constructed when only the list is visible. The top-level `History` entry is the complete-library entry point and shows the History list plus detail; entering from a projected historical item or global conversation search hides the library list, and only the top-level History entry switches the surface back to list+detail library mode. Live Tabs/Panes remain ahead of historical rows because protocol 19 exposes Agent `revision`/`state_change_seq` but no wall-clock activity timestamp comparable with History `updated_at`; Shardlane must not invent a cross-domain time ordering. When Herdr exposes a live Agent with the same stable native session identity, the Workspace projection hides the historical duplicate and the live Tab/Pane becomes the visible item; this is projection deduplication only and never deletes the History catalog record. History matching uses canonical project paths and remains Shardlane presentation state, not Herdr runtime state. Tabs and History rows that carry a known Agent use that Agent's brand mark. Agent rows expose Herdr Agent status, and a Workspace may surface an aggregate working/blocked indicator when a child Agent needs attention. Sidebar state is limited to width, collapsed state, per-Workspace disclosure/history reveal count, section disclosure, and navigation state.

When both `Workspaces` and `Agents` are expanded, gpui-component owns the vertical resize interaction between them. Shardlane persists only the preferred Workspaces section height and clamps it against the current window height so Agents retains a usable minimum region on smaller displays; section size is presentation state, not Herdr runtime state.

Visual details may evolve, but they must evolve inside the single Shardlane Sidebar ownership seam rather than by adding a second implementation.

**Current (2026-09-01) — Project Tab placement.** `terminal.tab_bar_placement` (`sidebar` default | `native`) chooses the presentation owner of the active Project's Tab list. `native` renders a gpui-component Tab strip above the hosted TUI/Chat surface (`shell_tabs.rs`): clicking switches through the canonical `FocusIntent` seam, and the strip's menu/drag/close actions reuse the exact Sidebar Tab action paths (`tab.create`/`tab.move`/`tab.rename`/`tab.close`; `tab.moved` remains the correction channel). The strip is presentation-only — it never creates a second runtime, a second Tab model, or a second navigation implementation; Herdr's Tab order and lifecycle stay authoritative, the hosted TUI's own chrome stays cropped by the chrome projection, and while `native` is active the canonical Sidebar keeps Projects without a per-Tab subtree (a Project click focuses the Project). Terminal geometry (`terminal_size`/`terminal_canvas_origin`) subtracts the strip's fixed height so the hosted grid and hit-testing stay consistent.

On macOS, Shardlane intentionally uses GPUI 0.2.2's default `MetalRenderer`. The `crepuscularity-gpui` feature named `macos` is not a generic macOS-support switch: it enables GPUI's alternate `macos-blade` renderer, and Shardlane has no product capability that requires that renderer. Keep the renderer choice explicit in `Cargo.toml` and verify it with Cargo's feature graph when changing GPUI/Crepuscularity dependencies.

GPUI 0.2.2 still predates Zed's screen-change fix `46eb9e5` / PR #38269. Shardlane must not compensate by manually invoking AppKit lifecycle callbacks such as `viewDidChangeBackingProperties` from GPUI bounds observers: that crosses GPUI's renderer ownership boundary and was observed to terminate the native app immediately after a display transition. Prefer the default renderer baseline now, and remove this dependency limitation by upgrading to a GPUI release that contains the upstream fix when such a crates.io release is available.

## 7. Agent history boundary

`shardlane-history` is a read-only, UI-independent history core.

It owns:

- normalized session/transcript/search models;
- source adapters for supported coding agents;
- the adapter source contract (`data_roots`, custom-root normalization, separator-safe
  ownership and longest-root routing);
- incremental scanning;
- Shardlane-owned SQLite/FTS catalog state;
- file-source dirty signaling;
- pure resume intent/argument semantics.

It does not own:

- host executable discovery;
- project-path validation side effects;
- Herdr runtime execution;
- external Agent database/file mutation.

Host Agent CLI validation remains in the Shardlane host integration layer. Continue/resume execution must transition through Herdr.

History source choices are user-owned `ApplicationConfig` data, not catalog state. A
`HistorySourcePolicy` is projected into one `HistoryAdapterRoster` generation; scanner,
watcher, transcript/detail, export, and live-source binding consume that same snapshot.
The catalog remains disposable derived data, so deleting `history.sqlite3` cannot remove
custom locations or disabled-source choices. Replacing a policy rebuilds the roster and
watcher before a generation-guarded background rescan; it never mutates external Agent
files or creates a second runtime authority.

History presentation bounds how many variable-height transcript messages it constructs at once, and replaceable source/page work is cancelled when selection changes. Very large sources use a Shardlane-owned disposable **page-addressable** transcript cache keyed by direct session identity plus source agent/native id/path/mtime/size. The cache stores normalized transcript metadata, fixed message pages, and a `seq → message_index` lookup so History/search-target navigation can load only the current bounded window. `HistoryUiState` must not retain an entire cached transcript after selection. The former whole-transcript blob is legacy-read-only and may only be consumed once for migration into page cache; new cache writes do not duplicate a full blob. Changed sources are prewarmed during the same background adapter parse already required for scanner/FTS indexing, so Shardlane must not parse a newly indexed source again merely because the user opens it. Corrupt/stale entries are disposable misses and rebuild from the read-only adapter source. Unchanged legacy rows that predate prewarming are migrated progressively under a strict per-scan count/byte budget; this backfill writes only the derived page cache and must not reindex unchanged FTS/session metadata. Anything still uncached may take the complete adapter parser compatibility path once, but the resulting full object is reduced to the requested window and dropped after the page cache is written. This cache never mutates external Agent history stores and does not misuse the FTS `UNINDEXED session_key` as a transcript index. Sidebar/search metadata may preload independently, but metadata loading must not implicitly select or parse a transcript; transcript cache lookup/parsing begins only when the user opens/selects a conversation. History metadata refresh is stale-while-revalidate: an already rendered project/history projection remains visible while the background index refresh runs, and Sidebar entities are notified only when visible metadata actually changes. A same-project refresh must never clear cached rows merely to show a loading state.

History session metadata keeps both the source/display fact `project_path` and a normalized stable `project_key`. `project_key` uses the same canonical-path/component normalization semantics as the client Project key and is indexed with `updated_at`; Project History queries and scoped History search filter on this key directly. Older catalogs are backfilled once on normal catalog open. The History catalog may persist this derived lookup key, but it does not become a second Project/runtime authority: live runtime identity still belongs to Herdr and `ProjectIndex` remains a disposable client projection. Shardlane Workspace History scope is derived by expanding that Workspace's owned Project paths; the History catalog does not persist Workspace ownership.

## 8. Persistence

Shardlane local application data lives under `~/.shardlane/`. Code refers to this ownership root as `app_data_dir`; it contains configuration, Shardlane-owned semantic data, and disposable client indexes/caches rather than only settings.

`~/.shardlane/config.json` is the configuration-domain SSOT. Local configuration may contain only client-owned preferences such as:

- global appearance policy (`system | light | dark`) and application color scheme;
- sidebar width/collapse state;
- Project/Agent/Service disclosure state;
- ordered Shardlane Workspace definitions, active Workspace, Workspace names/colors, and Project membership;
- terminal font/render preferences and Terminal theme (`follow-app` or independent palette);
- copy-on-select and Agent system-notification preference;
- Browser/right panel preferences;
- small global defaults that do not belong to a Project asset;
- window opacity;
- Projects/Agents/Services section heights;
- always-on-top state;
- pinned Sidebar tab IDs (`ui.sidebar.pinned_tabs`) — session-scoped UI
  preference storing Herdr runtime tab IDs; a stale sweep on every
  navigation reconcile drops IDs absent from the live Herdr snapshot so
  restarts cannot accumulate dead IDs (mirrors the `project_path_overrides`
  GC contract).

**File-first durable-data rule.** Shardlane-owned user data should remain reconstructible from human-inspectable files. The existing Workspace/Sidebar state already satisfies this rule because it lives in `config.json`; do not split it into extra files without a concrete ownership/write-conflict reason. Larger independent domains use separate files/directories where ownership demands it: Browser Profile metadata may use `browser-profiles.json`, and Shortcut overrides may use `shortcuts.json`. (The former Notes/Bookmarks/Annotation file stores were removed with those product branches on 2026-08-27.) All aggregate writes are revision-guarded and atomic; external edits are validated before replacing the last-known-good in-memory projection. Runtime/search indexes may be in-memory or disposable caches and must always rebuild from the user files. A database may still exist for a domain-specific **derived index/cache** such as the existing History search/page index, but it must not become the source of truth for Workspace/Sidebar assets.

Script definitions currently live in `~/.shardlane/scripts.json` (migrated once from `tasks.json` on load) as a documented protocol-gap bridge. `ScriptDefinition` contains only current durable Shardlane semantics (`id`, stable `project_path`, name, icon, optional keybinding, multiline command, kind, `one_shot`, `close_on_complete`, optional `last_run_at_ms`). The product is unreleased: do not add legacy `workspace_path`, Script `cwd`, fallback identity chains, aliases, or migrations. `ScriptRecord` may additionally persist the minimum Herdr correlation needed for recovery (`workspace_id`, `tab_id`, `pane_id`); those IDs are disposable runtime correlation only and must never override a mismatching `project_path`. `ScriptRuntimeProjection` contains status, PID, listening ports, startup timestamp, and transient error; it is explicitly not serialized and is rebuilt from Herdr/process observation after load. Runtime-only changes must not rewrite `scripts.json`. When Herdr gains protocol-native Script identity/lifecycle, migrate this semantic persistence to Herdr rather than preserving a parallel Script authority.

Local feature storage is separated by ownership rather than accumulated into `config.json`: bounded Activity acknowledgement receipts live in a dedicated local state file. Browser cookies/cache/site data remain WebKit-managed inside the selected `WKWebsiteDataStore`, not Shardlane JSON. The former Notes/Bookmark/Annotation stores (`projects/<key>/notes/`, bookmark overlays, `~/.shardlane/annotations/`) are no longer read or written — the product branches were removed 2026-08-27, and pre-existing user files on disk are left untouched rather than erased by startup.

Runtime workspace/tab/pane/process state belongs to Herdr and must not be mirrored as a competing durable model. On macOS, whole-window opacity, always-on-top, and forced Light/Dark appearance are applied through one narrow native-window adapter at GPUI's raw AppKit handle boundary; `System` clears the explicit `NSAppearance` so AppKit follows macOS, while `Light`/`Dark` use the generated objc2 AppKit Aqua/DarkAqua bindings. Agent system notifications use a separate narrow UserNotifications adapter and may call `UNUserNotificationCenter` only when `NSBundle.mainBundle.bundleIdentifier` is the packaged Shardlane identifier `dev.shardlane.app`; raw `crepus dev` / `target/debug/shardlane` processes have no registered application bundle and therefore treat system notification presentation/authorization as a no-op. Individual Shardlane surfaces remain opaque and consume semantic component theme tokens rather than independently simulating whole-window appearance. AppKit lifecycle/render callbacks remain GPUI-owned.

A future macOS menu-bar status item is an approved native presentation surface, not a new runtime/state owner. Implement it in a narrow AppKit adapter using generated objc2 AppKit bindings (`NSStatusBar`/`NSStatusItem`/`NSStatusBarButton`, `NSMenu`/`NSMenuItem`, `NSImage`, and `NSApplication`) and feed it from the same `OperationalSummary` plus bounded Workspace detail already used by the main window. The menu is operational rather than archival: Agent attention/working state is the primary content, Workspaces and active/failed Scripts are bounded secondary sections or submenus, and History remains in the application. Agent/Workspace/Script menu actions must activate Shardlane and reuse existing focus/navigation routes rather than creating a parallel navigation path. Keep the menu bounded with `Show All…` fallbacks, use a stable template status icon instead of continuous animation, and represent changing state in menu content or a minimal attention affordance so idle CPU remains effectively zero.

Shardlane should not add a permanent full-width bottom status bar by default. It permanently costs Terminal rows and would duplicate Workspace/Tab breadcrumbs, Sidebar Agent/Script state, and contextual Header operational indicators. If an in-app bottom status surface is introduced, it must be a contextual status shelf that is absent in the normal steady state and appears only for actionable app-wide conditions such as reconnecting/offline state, a failed background operation, or attention that would otherwise be hidden with the Sidebar collapsed. It must not become another location for persistent navigation metadata.

## 9. Packaging and identity

Canonical application identity:

- product/app name: **Shardlane**;
- Cargo package: `shardlane`;
- binary: `shardlane`;
- macOS bundle: `Shardlane.app`;
- bundle identifier: `dev.shardlane.app`;
- history crate package: `shardlane-history`.

The single packaging configuration is `[package.metadata.bundle]` in the root
`Cargo.toml`. It owns the user-facing bundle name, reverse-DNS identifier,
version inheritance, and app icon source (`assets/app-icon/`). `cargo-bundle`
converts the icon source into `Contents/Resources/Shardlane.icns` and writes
the matching `CFBundleIconFile` entry; scripts must not duplicate these values.

`scripts/package-macos.sh` is the reproducible local/release orchestration
entrypoint: it builds, bundles, signs, validates the bundle, and optionally
installs it. `--universal` builds `aarch64-apple-darwin` and
`x86_64-apple-darwin`, merges the executables with `lipo`, recursively verifies
bundle Mach-O members, and only then signs the final app. The Mobile Web
`dist/` directory is a cross-repository packaging input rather than a Cargo
resource: `scripts/bundle-web.sh` copies it to
`Contents/Resources/mobile-web/` after the app bundle exists. At runtime,
`web_bundle_path()` prefers `SHARDLANE_WEB_BUNDLE` for development and then
the packaged Resources path; the Remote server serves that static directory
alongside `/api/v1` without moving runtime ownership into Web code.

`scripts/archive-macos.sh` is the single ZIP/checksum boundary after packaging:
it verifies the already-signed `.app`, rechecks Universal 2 members when the
`universal2` label is requested, derives the archive name from the bundle name,
and writes a basename-only SHA-256 manifest. The `v*` GitHub Release workflow
uses this entrypoint to publish one `Shardlane-macos-universal2.zip` artifact
with ad-hoc signing until Developer ID and notarization credentials exist.

Release artifacts use the Shardlane name only.

Project-level license metadata has been removed. Required third-party notices remain isolated in `THIRD_PARTY_NOTICES.md` and in source/assets where upstream licenses require retention.

## 10. Herdr protocol gaps

A protocol gap is a capability Shardlane cannot implement authoritatively without new Herdr support.

Known gaps remain backend facts, not branding concerns. Herdr protocol 20 still exposes `tab.move(tab_id, insert_index)` for authoritative in-Workspace ordering without a destination Workspace, so cross-Workspace Tab drag must remain unsupported until Herdr adds that ownership transfer primitive.

The native surface uses one hosted Herdr TUI child behind one PTY/Ghostty model;
Herdr remains authoritative for PTY/process lifetime, focus, input, resize, and
retained-history data. The Remote API exposes one additional Host-owned shared
TUI session slot for authenticated viewers, never a per-Pane or per-client
fleet. Its raw byte stream is rendered by a real VT owner; Mobile does not parse
ANSI or infer Conversation semantics from it. Explicit TUI actions may change
the global Herdr focus/Tab/resize state and can therefore reflow the Mac and
other viewers; semantic Mobile navigation remains client-local. The public
contract omits PTY fd/PID/socket/config details. Deep history beyond any bounded
hydrated window remains a Protocol Gap until Herdr exposes range/page reads that
can be safely merged without violating ordering.

History continuation does not require a second runtime: the current flow can create/focus Herdr workspace/tab/pane state and send the resume command through Herdr.

Scripts likewise require no second runtime. For a materialized Project, Shardlane prefers the focused/first Herdr Pane and uses `pane.split` to create a dedicated Script Pane inside the existing Project Tab; only when no suitable Project Pane exists may it create a fallback Tab. It waits for the Script Pane shell to become ready, sends the command through Herdr, observes runtime state with `pane.process_info`, and Stop/Restart/Delete/one-shot completion close only the Script Pane. The shell PID itself is not a running Script process; runtime status is derived from non-shell foreground processes. Stable Script ownership is always resolved from `project_path`, with Herdr IDs treated only as verified correlation. Listening-port discovery is a read-only macOS projection over Herdr-owned foreground PIDs. The Sidebar **Services** section contains persisted `service` Scripts plus ordinary Herdr Panes projected as read-only detected Services when their foreground process owns a listening TCP port. Detected Services are never persisted, started, stopped, restarted, or deleted by Shardlane; selecting one only focuses its Herdr Project/Tab/Pane. Service disclosure is presentation-only and does not trigger probing. Discovery is scoped to Projects in the active Shardlane Workspace, bounded to a finite Project/Pane set, refreshes independently on a slow ~10-second cadence or relevant wake, excludes panes already correlated to durable Scripts, and batches listening-port inspection into one `lsof` invocation for candidate PIDs. Managed Script health keeps its separate ~1.5-second cadence only while materialized Scripts exist. Global Activity may aggregate all durable Script runtime projections and all already-discovered Services, but it must not automatically widen unmanaged Service discovery from the active Shardlane Workspace to every Workspace. Any wider discovery policy requires an explicit bounded sweep budget, slower inactive-Workspace cadence, and measured CPU/wakeup evidence first. A detached/background server that is not represented by Herdr `pane.process_info` foreground processes is outside this projection. The missing backend capability is stable protocol-native Script identity/lifecycle (a Herdr-side script lifecycle surface or equivalent), not PTY/process execution.

## 11. Remote Host and mobile clients

Remote/mobile support is an **Approved Next** presentation and transport
extension with a **Current** loopback/local-network semantic Conversation
foundation. The UI-independent `shardlane-host` crate owns opaque client IDs,
bounded Live/History DTOs, provider-neutral normalization, the shared
Continue transaction seam, narrow service interfaces, and the Agent runtime
SPI; `shardlane-remote` implements the authenticated HTTP/WebSocket adapter
over the existing Herdr boundary. Remote API v2 exposes Conversation list,
bounded window/detail, prompt, History search, and Host-owned Continue; v1
control/history routes remain compatibility-only. The Mac Settings → Mobile
surface provides pairing QR/deep-link guidance, access-token rotation, and
live Web connection projection. The Expo/React Native tree in `herdr-mobile`
is the second client, and its Web export can be served from the packaged Mac
app when `dist/` is embedded. Public Relay/cloud infrastructure and
production-grade public TLS remain future work. Runtime ownership is
unchanged.

Shardlane is evolving from one native GUI client into a Mac **Host/Core + clients** architecture:

```text
                         ┌─ macOS GPUI client
Herdr Runtime ← Shardlane Host/Core
                         └─ Remote API / Protocol ← mobile/other clients
```

The Host/Core is the shared application-service boundary for Workspace, Project, Agent, Script, History, and Terminal operations. The macOS GPUI client and the React Native Mobile client must call the same application services; Remote API handlers must never drive GPUI presentation state or expose a generic Herdr RPC passthrough.

Remote clients own independent semantic navigation state and use explicit Workspace/Project/Tab/Pane/Agent/Script identifiers. A mobile semantic selection must not change the Mac user's selected Project/Tab/Pane. The explicit shared Herdr TUI surface is the documented exception: opening it may drive the existing global focus/Tab/resize state, so the Mac TUI can change and the UI must disclose that behavior.

Agent semantics are the primary mobile control path. Remote/mobile Agent and
History screens now consume one provider-neutral semantic Conversation shape;
Live and History differ only by controller/lifecycle. Prompt and Continue
mutations target explicit opaque Conversation/Agent identities through Host;
Mobile never reads provider files, parses TUI/ANSI, or launches a provider.
Herdr TUI remains the sole normal Terminal implementation. Batch 0 evidence
(`docs/remote-tui-protocol-probe-2026-08-29.md`) found global focus and no
client-local lease/focus/resize/reconnect isolation in installed protocol 20.
Product accepts those shared/global semantics: the Host exposes one bounded
shared TUI session to authenticated clients. The Remote implementation advertises
`herdr_tui=true`; runtime/operator and native VT verification remain open. No
remote per-Pane fallback is allowed.

The application contract is transport-independent. Approved connection sequence is:

1. loopback HTTP/WebSocket for Host contract development and tests;
2. authenticated TLS LAN direct connection for normal same-network use;
3. Tailscale/WireGuard overlay as the preferred early public-remote path for dogfood/private beta;
4. SSH port forwarding as developer/advanced fallback rather than the default product connection;
5. a Shardlane-owned outbound Relay as the long-term default public-internet path behind NAT/CGNAT;
6. WebRTC/QUIC/direct-path optimization only after measurements justify the complexity.

Transport/Relay infrastructure is never a Workspace/Project/Agent/Script runtime authority. Remote client state is disposable and recovers through a bounded Host bootstrap after reconnect. Reliable background push is deferred to the future Relay/cloud phase and must not force cloud dependencies into the initial Host API.

Detailed transport/security decisions live in `remote-client-architecture.md`; executable sequencing lives in the internal engineering archive.

## 12. Engineering workflow

Before implementation:

1. read `CLAUDE.md`, `AGENTS.md`, and this file;
2. inspect Git state and preserve user/staged changes;
3. identify the owning layer before writing code;
4. prefer the mature-owner ladder over hand-written substitutes;
5. keep the change bounded and verifiable.

Before completion:

```sh
cargo fmt -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
git diff --check
git diff --cached --check
```

Packaging work must additionally build and structurally verify `Shardlane.app`.

## 13. Definition of architectural correctness

A change is architecturally correct when all are true:

- Shardlane remains the client/product identity;
- Herdr remains the sole backend/runtime authority;
- real Herdr technical names remain accurate rather than being cosmetically renamed;
- terminal semantics live in Ghostty when Ghostty provides them;
- standard UI behavior uses gpui-component when available;
- external Agent history remains read-only;
- history continuation returns through Herdr;
- only visible runtime data drives expensive terminal/render work;
- the shell has one canonical Sidebar implementation;
- custom code has a documented reason to exist;
- unstable external ABI assumptions have targeted verification;
- workspace CI exercises all packages;
- macOS bundle identity is Shardlane-only;
- remote clients, transports, and Relay infrastructure never become a second runtime authority;
- semantic remote operations use explicit targets and do not implicitly steal another client's navigation/focus; an explicit shared TUI action may mutate Herdr's global focus/resize and is disclosed;
- documentation and code describe the same ownership model.
