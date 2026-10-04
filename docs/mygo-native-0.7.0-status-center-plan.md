# Shardlane MyGo 0.7.0 — Status Center & Agent Quick Navigation Plan

Status: **planned post-0.6 release**
Target: `0.7.0`
Depends on: 0.6 Native Conversation & Chat complete
Architecture authority: `docs/client-product-architecture.md`
Execution rules: `docs/mygo-native-execution-rules.md`
Planning review: **reviewed 2026-10-05 against the post-0.4.0 tree** — authority files, MyGo API prerequisites (Tray/Notification where used) and 0.4 prerequisites (launch transaction, integration health, operational status model, Design System v2) all verified present; no stale statements found.

## 1. Version goal

0.7 focuses on one product surface: the **right-side Status Center** and its system-tray projection.

Goal:

> Give the user one always-available place to understand what every Agent is doing,
> what needs attention/review, and jump directly to the exact Agent surface that can resolve it.

This release does not create a new Agent state engine. It consumes the Agent lifecycle, Conversation,
Interaction, review/unread and integration models completed by 0.4–0.6.

The core architecture is:

```text
Herdr / Agent / Conversation facts
              │
              ▼
      AgentDirectory + markers
              │
              ▼
       StatusCenterService
              │
              ▼
      StatusCenterSnapshot
       ┌────────┼──────────────┐
       ▼        ▼              ▼
Window right-top   Menu Bar / Tray   Optional native menu
Status Center      Quick Panel       (right-click/fallback)
       │        │              │
       └────────┴──────┬───────┘
                       ▼
              StatusCenterAction
                 │
                 ▼
         local navigation/router
```

There is one state derivation and one action-routing contract.

## 2. Original-project behavior authority

The new implementation should preserve useful behavior from the Rust product without porting objc2/GPUI mechanics.

| Original authority | Behavior to preserve |
|---|---|
| `crates/herdr-gui/src/status_bar.rs` | blocked/review/working counts, actionable grouping, system status menu, quick Agent jumps; primary status remains attention-first |
| `crates/herdr-gui/src/agent_panel.rs` | process-wide Agent directory projection, filter tabs, actionable sorting, review acknowledgement, cross-window routing concept |
| `crates/herdr-gui/src/header_view.rs` | Header Agent summary control and per-Agent status affordance |
| `crates/herdr-gui/src/status.rs` | shared attention semantics and status glyph vocabulary |
| `crates/herdr-gui/src/status_bar.rs` + `main.rs` | status/tray click is routed as an action, not implemented as direct runtime mutation |
| `crates/herdr-gui/src/agent_switcher.rs` | stable Agent identity + navigation semantics |
| `crates/herdr-gui/src/shell_navigation.rs` | unread/review lifecycle and local navigation |

The 0.7 MyGo implementation should use the framework's Native Tray/Window primitives instead of retaining the old macOS-specific objc2 controller.

External reference: `docs/reference-magpie-agent-history-usage-audit.md` records the Magpie quick-panel/history/usage audit. The key UX borrowed for 0.7 is **tray click → floating quick panel**, implemented with MyGo Native UI rather than Magpie's WebView window.

## 3. Two surfaces, one model

### 3.1 In-window Status Center — primary

Located at the **right side of the application titlebar**.

This is the full interactive product surface.

It may show:

- summary counts;
- status filters;
- Agent rows/cards;
- unread/review state;
- exact recommended destination;
- Mark reviewed;
- Open Agents;
- reconnect state.

### 3.2 macOS Menu Bar Quick Panel — secondary but first-class

A normal click on the MyGo Tray/Menu Bar item toggles a dedicated **Native floating panel**, not merely an `NSMenu`/system menu.

Required characteristics:

```text
Native MyGo UI
frameless
always-on-top
non-resizable
transparent/vibrant material where supported
hide on focus loss
hide on Escape
positioned below the tray icon
content height bounded by available screen work area
```

Implementation primitives already exist in MyGo 0.2.7:

```text
NewTray
Tray.Bounds
NewWindow(WindowOptions{Frameless, AlwaysOnTop, SkipTaskbar, Transparent, Vibrancy, Hidden})
Window.SetBounds / SetPosition
Window.OnBlur
```

Do not add a WebView quick panel.

The panel renders the same Status Center rows/components as the in-window panel, with a compact surface mode.

### 3.3 Optional right-click / fallback native menu

A concise system-native menu may still be used for right-click or as a platform fallback:

- Show Shardlane;
- Open Agents;
- New Agent;
- History;
- Search;
- Settings;
- Quit.

This menu is not the primary Agent-status experience.

### 3.4 Cross-platform rule

The application-level Status Center is mandatory on every platform.

Tray behavior is an adapter:

```text
macOS   → menu bar item + floating Native quick panel
Windows → notification area tray; floating panel when stable, menu fallback otherwise
Linux   → tray/panel where desktop environment/backend supports it, menu fallback otherwise
```

If tray or floating-panel placement is unavailable on one environment, the in-window Status Center remains fully functional.

## 4. Current MyGo baseline to change

The current MyGo titlebar has a minimal `titlebarActivity` that:

- only renders `NeedsAttention`;
- routes to the Workspace/Agents section;
- does not expose Working/Review detail;
- is placed beside the Shardlane brand area rather than a dedicated right-side status accessory;
- does not provide Agent rows or direct target routing.

0.7 replaces that minimal control with a first-class right-side Status Center trigger.

Do not retain two titlebar status implementations.

## 5. StatusCenterSnapshot

The snapshot is immutable, UI-independent and cheap to compare.

Suggested model:

```go
type StatusCenterSnapshot struct {
    Connection ConnectionSummary
    Summary    StatusSummary
    Agents     []StatusAgentItem
    Revision   uint64
}

type StatusSummary struct {
    NeedsAttention int
    ReviewPending  int
    Working        int
    Idle           int
    Total          int
}

type StatusAgentItem struct {
    Key             agent.AgentKey
    Provider        history.AgentID
    ProviderLabel   string
    Title           string
    ProjectID       string
    ProjectName     string
    TabID           string
    PaneID          string
    ConversationID  string
    RuntimePhase    agent.AgentRuntimePhase
    Attention       agent.OperationalState
    Sendability     agent.AgentSendability
    Unread          bool
    ReviewPending   bool
    Interaction     *InteractionSummary
    Usage           *AgentUsageSnapshot
    RecommendedDest StatusDestination
}
```

Do not put MyGo elements, colors, menu handles or platform-native objects in the snapshot.

## 6. Status priority

Use one explicit order everywhere:

```text
Needs Attention
→ Ready for Review
→ Working
→ Idle
→ Resolved / hidden from primary list
```

Within a group:

```text
Unread first
→ most recently changed/active when authoritative timestamp exists
→ title alphabetical as deterministic fallback
```

Do not invent fake recency from render time.

## 7. Header trigger design

### 7.1 Right-side accessory zone

The titlebar layout should end with a stable right-side status zone:

```text
┌──────────────────────────────────────────────────────────────────────┐
│ Shardlane  ‹ ›   Project / Tab                         ⚠ 2  ✓ 1  ⚡ 3 │
└──────────────────────────────────────────────────────────────────────┘
                                                            ▲
                                                      Status Center
```

### 7.2 Visibility rules

When actionable state exists:

```text
⚠ 2   needs attention
✓ 1   ready for review
⚡ 3  working
```

Do not permanently show an `all healthy` badge.

When no actionable Agent exists, preferred normal state is a quiet icon/button without counts.

Disconnected/reconnecting overrides Agent counts with an explicit reconnect/offline affordance.

### 7.3 Usage is secondary, visible information

0.7 intentionally extends the old product: Agent usage is visible in the Status Center and quick panel, but it **never controls the operational priority**.

Primary chrome remains:

```text
Needs Attention / Review / Working
```

Per-Agent secondary metadata may show, when authoritative enough:

```text
model · tokens · estimated cost
```

or an authoritative provider allowance:

```text
5h window 38% used · resets in 2h 14m
```

Rules:

- missing usage is `unknown`, never `0`;
- estimated cost must be labeled/treated as estimated;
- quota may be stale and must carry `as-of` metadata;
- Usage does not turn an Idle Agent into an Attention Agent;
- do not put raw provider credentials/accounts in compact chrome;
- full accounting belongs to 0.8 Usage/Agent Inspector.

## 8. In-window Status Center panel

Preferred width: approximately 360–420 DIP, adjusted through the Design System rather than raw page constants.

Structure:

```text
Agents                                      [Open Agents]
2 need attention · 1 review · 3 working

[Attention] [Review] [Working] [Idle] [All]

Needs attention
┌ Claude · Fix auth                         ⚠ Needs input ┐
│ portal · Sonnet · 38k tok · $0.21 est.       Terminal → │
└───────────────────────────────────────────────────────────┘

Ready for review
┌ Codex · Refactor cache                       ✓ Review    ┐
│ api · GPT · 91k tok                [Mark reviewed]      │
└───────────────────────────────────────────────────────────┘

Working
┌ Claude · Add tests                           ⚡ Working   ┐
│ portal · 5h allowance 38% · resets 2h 14m      Chat →   │
└───────────────────────────────────────────────────────────┘
```

The exact row/card visuals must reuse the 0.5 AgentCardModel components.

Do not build a new Status-Agent row visual system.

## 9. Filter semantics

Filters:

```text
Attention
Review
Working
Idle
All
```

Recommended initial filter:

```text
if NeedsAttention > 0 → Attention
else if ReviewPending > 0 → Review
else if Working > 0 → Working
else → All or Idle
```

This improves on the old fixed default Working while preserving its focus on active work.

Filter choice is presentation state and must not mutate runtime state.

Optionally retain the user's filter during one app session; do not persist it unless later product evidence justifies persistence.

## 10. Recommended destination model

The user asked for status rows to jump directly to the corresponding panel. That must be a **pure routing decision**, not page-specific conditionals spread across the menu.

```go
type StatusDestinationKind string

const (
    DestInteraction StatusDestinationKind = "interaction"
    DestTerminal    StatusDestinationKind = "terminal"
    DestConversation StatusDestinationKind = "conversation"
    DestAgent       StatusDestinationKind = "agent"
    DestProject     StatusDestinationKind = "project"
)
```

Suggested routing priority:

### Pending structured interaction

```text
Question / Permission / PlanApproval pending
→ Conversation Chat
→ scroll/focus exact InteractionCard
```

If the interaction is already `NativeFallback`:

```text
→ Terminal
```

### Blocked / Failed / NeedsTerminal

```text
→ owning Terminal/Pane
```

Do not route to a composer that cannot safely resolve it.

### Ready for Review

Preferred:

```text
live Conversation available
→ Chat / latest settled turn

otherwise
→ Agent Workbench card
```

`Mark reviewed` remains explicit; merely jumping does not acknowledge review.

### Working

Preferred:

```text
live semantic Conversation available
→ Chat

otherwise
→ owning Agent/Terminal surface
```

### Idle / Other

```text
→ Agent Workbench / owning Agent
```

This destination is computed once in application/presentation projection and shared by window Status Center and Tray.

## 11. StatusCenterAction

Both window panel and tray emit the same action enum.

Suggested actions:

```go
type StatusCenterAction struct {
    Kind         StatusActionKind
    AgentKey     *agent.AgentKey
    ProjectID    string
    TabID        string
    PaneID       string
    ConversationID string
    InteractionID string
}
```

Kinds:

```text
OpenAgentDestination
OpenAgentsWorkbench
MarkReviewed
OpenNewAgent
OpenHistory
OpenSettings
OpenSearch
ShowMainWindow
Reconnect
Quit
```

The action router owns window activation/local navigation.

Status UI must not call Herdr focus RPCs or mutate runtime directly.

## 12. Quick jump contract

One row click should be one semantic quick jump.

Transaction:

```text
click Agent row
→ snapshot exact AgentKey/destination
→ dismiss menu/panel
→ show/activate owning window
→ resolve Agent still exists
→ refresh navigation locators when needed
→ navigate locally to destination
→ clear unread because Agent was visited
→ DO NOT clear ReviewPending
```

If the Agent disappeared between snapshot and click:

```text
→ no runtime mutation
→ refresh Status Center
→ small toast: Agent is no longer running
```

No silent jump to a different Pane with the same label.

## 13. Cross-window / multi-instance routing

Preserve the original product concept: a Status Center can eventually show Agents contributed by more than one open window/observed instance.

Use stable identity:

```text
(instance_id, terminal_id)
```

Do not use only Pane ID as process-wide identity.

Routing:

```text
AgentKey
→ find owning observed window/instance
→ activate/show that window
→ local navigation
```

Do not create background watchers for every inactive Herdr instance solely to populate the menu.

Until multi-window observation is implemented, the model naturally contains the current observed instance only.

## 14. Review behavior

Status Center must preserve the 0.5 distinction:

```text
visit/open Agent
→ clears Unread
→ does NOT clear ReviewPending

Mark reviewed
→ clears ReviewPending + Unread
```

The panel row may provide an inline check action.

The Tray menu may expose `Mark reviewed` only if the menu API remains understandable; otherwise quick-jump and in-window acknowledgment are sufficient.

Do not overload a row click with review acknowledgment.

## 15. Status Center and 0.6 Interaction state

The Status Center should become the highest-level attention entrance for structured interactions.

Examples:

```text
Claude · Fix auth
Permission required
→ jump to exact Permission card
```

```text
Codex · Run migration
Needs Terminal
→ jump to terminal
```

Priority:

```text
Pending structured interaction
> generic blocked
> review pending
> working
```

But runtime and interaction states remain separate models internally.

## 16. Connection state

The Status Center owns the user-visible connection problem entry point.

### Connected

Do not show a permanent green “Connected” badge.

### Reconnecting

Show:

```text
Reconnecting to Herdr…
```

Disable Agent actions whose exact target cannot currently be validated.

### Disconnected

Show:

```text
Herdr disconnected
[Reconnect]
```

Keep the last known Agent snapshot visually distinguishable/stale only if product policy chooses to retain it; never present stale rows as live actionable facts.

Recommended first implementation: suppress Agent mutation actions while disconnected.

## 17. macOS Menu Bar Quick Panel & Tray design

MyGo provides `NewTray`, `Tray.Bounds`, tray click listeners, dynamic Menu, and Native Window positioning.

### 17.1 Left click = floating Status Quick Panel

On macOS, normal tray/menu-bar click toggles a dedicated Native UI window anchored beneath the menu-bar item.

Suggested size:

```text
width: 360–420 DIP
height: content-driven, bounded to screen work area
corner radius/material: macOS popover-like visual through Native Window/Vibrancy support
```

Lifecycle:

```text
tray click
→ if panel visible: hide
→ else read Tray.Bounds
→ compute screen-safe panel bounds
→ update panel snapshot
→ show + focus

panel blur / Escape
→ hide
```

The quick panel must not appear in Dock/taskbar as a normal document window.

### 17.2 Quick-panel content

The panel reuses `StatusCenterSnapshot` and compact Agent rows.

Header:

```text
Shardlane
2 need attention · 1 review · 3 working
```

Agent row secondary line includes Usage when available:

```text
portal · Sonnet · 38k tok · $0.21 est.
```

A quota-capable provider may instead show the most relevant allowance window.

The panel supports the exact same recommended destination routing as the in-window Status Center.

### 17.3 Menu-bar title / icon

On macOS the title may remain status-first:

```text
⚠ 2  ✓ 1  ⚡ 3
```

If no actionable state exists, title should be empty so the menu bar stays quiet.

Do **not** append raw token totals to the menu-bar title by default; usage belongs inside the panel where the Agent/session context is visible.

A later preference may pin a compact quota/usage cell, but that is not required for 0.7.

### 17.4 Tooltip

Examples:

```text
Shardlane — 2 need attention, 1 for review, 3 working
Shardlane — Ready
Shardlane — Herdr disconnected
```

One summary formatter drives titlebar/tray wording where appropriate.

### 17.5 Optional right-click native menu

Suggested structure:

```text
Open Status Center
Open Agents
New Agent
History
Search
Settings
────────────────────
Show Shardlane
Quit
```

Do not duplicate detailed Agent grouping into both a system menu and the floating panel unless platform fallback requires it.

### 17.6 Usage source in 0.7

0.7 needs only a lightweight Agent usage projection, sourced from data already available through History/session metadata or provider capability services.

Suggested:

```go
type AgentUsageSnapshot struct {
    AgentKey   agent.AgentKey
    Model      string
    Provider   string
    Tokens     *int64
    CostUSD    *float64
    Quota      *QuotaSnapshot
    Source     UsageSource
    Complete   bool
    UpdatedAt  int64
}
```

The richer Usage ledger/domain is a 0.8 responsibility. 0.7 must not create a second incompatible accounting model.

## 18. Tray snapshot update strategy

Do not rebuild the platform tray menu on every frame or pointer event.

Maintain:

```text
last StatusCenterSnapshot fingerprint
```

Update tray only when relevant snapshot state actually changes:

- Agent enters/leaves;
- status changes;
- unread/review changes;
- interaction attention changes;
- connection state changes;
- title/project identity changes relevant to menu labels.

Navigation selection alone should not force a tray rebuild unless it changes unread/review state.

## 19. Dynamic rendering

### In-window panel

The panel consumes already-derived snapshot rows.

No render-time:

- Herdr RPC;
- filesystem IO;
- provider inspection;
- History parse;
- Conversation load;
- Integration health probe.

### Agent update

```text
Herdr event / Conversation interaction update
→ normal reconciliation
→ AgentDirectory / marker update
→ derive StatusCenterSnapshot
→ compare fingerprint
→ update affected Native view/tray only when changed
```

No status polling timer.

## 20. Visual system

Reuse 0.4 Design System v2 and 0.5 Agent components.

Required shared pieces:

- `StatusGlyph`;
- `ProviderBadge/mark`;
- compact Agent row;
- Review chip;
- unread dot;
- filter segmented/tabs;
- EmptyState;
- InlineNotice;
- Toolbar/IconButton;
- Toast.

The right-side trigger should look like a macOS toolbar accessory, not a web dashboard badge cluster.

The system quick panel should borrow Magpie's compact menu-bar panel interaction pattern, but render with MyGo Native UI and Shardlane's Design System.

Suggested normal density:

```text
compact 28–30 DIP trigger height
small semantic glyphs
muted normal state
attention color only when attention exists
```

## 21. Empty states

### No Agents

```text
No active Agents
Start an Agent to see live status here.
[New Agent]
```

### Filter empty

```text
No Agents need attention
```

Do not remove the whole Status Center merely because one filter is empty.

## 22. Keyboard/accessibility

Required:

- trigger is keyboard focusable;
- Enter/Space opens;
- Escape closes;
- Up/Down navigates rows where the chosen Native primitive supports it;
- Agent row has semantic accessible label containing Agent title + project + status;
- inline Mark reviewed has its own label;
- no color-only status distinction.

Suggested shortcut if available without conflict:

```text
Cmd/Ctrl + Shift + A → open Agent Status Center
```

Do not reuse Ctrl-Tab; that remains the 0.5 MRU Agent switcher.

## 23. Relationship to Agent MRU switcher

They solve different tasks.

### Ctrl-Tab Agent Switcher

```text
fast keyboard cycling among recent live Agents
```

### Status Center

```text
inspect global state
find attention/review/working Agents
jump intentionally to a specific resolution surface
```

Both consume AgentDirectory/AgentCardModel, but they maintain separate presentation state.

## 24. Relationship to `/agents`

The Status Center is a quick overview, not the full management page.

The panel must always provide:

```text
Open Agents
```

which routes to `/agents` preserving an appropriate filter when practical:

```text
Attention panel → /agents?filter=attention
Review panel    → /agents?filter=review
Working panel   → /agents?filter=working
```

If Router query state is undesirable, pass a typed navigation intent instead of encoding business state in strings.

## 25. Atomic implementation plan

### S0 — snapshot/action domain

| ID | Task | Verify |
|---|---|---|
| STC-01 | StatusCenterSnapshot models | pure model tests |
| STC-02 | StatusAgentItem from AgentCardModel | fixture projection tests |
| STC-03 | summary counts | state matrix |
| STC-04 | actionable sorting | deterministic sort tests |
| STC-05 | recommended destination resolver | status/interaction table tests |
| STC-06 | StatusCenterAction model/router contract | pure routing tests |

### S1 — titlebar trigger

| ID | Task | Verify |
|---|---|---|
| STC-07 | dedicated right-side titlebar accessory zone | headless layout test |
| STC-08 | compact summary trigger | render smoke |
| STC-09 | quiet/no-agent state | render smoke |
| STC-10 | disconnected/reconnect state | action test |
| STC-11 | remove old minimal `titlebarActivity` path | static regression |

### S2 — in-window Status Center

| ID | Task | Verify |
|---|---|---|
| STC-12 | Native menu/panel open/close state | headless interaction |
| STC-13 | filters | pure + headless tests |
| STC-14 | compact Agent rows reuse | component test |
| STC-15 | Attention/Review/Working grouping | snapshot tests |
| STC-16 | inline Mark reviewed | marker lifecycle test |
| STC-17 | Open Agents action | router test |
| STC-18 | empty/error/reconnecting views | render smoke |

### S3 — quick destination routing

| ID | Task | Verify |
|---|---|---|
| STC-19 | pending Interaction → Chat interaction anchor | routing test |
| STC-20 | NeedsTerminal → Terminal | zero-prompt routing test |
| STC-21 | Review → live Conversation/Agent | routing test |
| STC-22 | Working → Chat when live semantic source exists | routing test |
| STC-23 | fallback → Agent Workbench/Pane | routing test |
| STC-24 | vanished Agent click fails safely | stale target test |
| STC-25 | visit clears unread only | marker test |
| STC-26 | review not cleared by navigation | regression test |

### S4 — tray / menu-bar floating panel

| ID | Task | Verify |
|---|---|---|
| STC-27 | MyGo Tray lifecycle service | fake/platform seam test |
| STC-28 | Native floating quick-panel window | window-options test |
| STC-29 | position panel from Tray.Bounds + screen bounds | geometry tests |
| STC-30 | tray left-click toggle | click lifecycle test |
| STC-31 | hide on blur / Escape | focus/input test |
| STC-32 | tray summary title/tooltip formatter | golden tests |
| STC-33 | optional right-click/fallback native menu | menu tests |
| STC-34 | tray unavailable fallback | service test |

### S5 — lightweight Agent usage projection

| ID | Task | Verify |
|---|---|---|
| STC-35 | UsageSource/AgentUsageSnapshot shared domain seam | model tests |
| STC-36 | live Agent ↔ session usage identity join | fixture identity tests |
| STC-37 | token/model compact formatter | golden tests |
| STC-38 | estimated cost optional formatter | unknown/estimated tests |
| STC-39 | optional quota snapshot cache/projection | stale/as-of tests |
| STC-40 | StatusCenterSnapshot usage integration | projection tests |

### S6 — multi-window/process routing

| ID | Task | Verify |
|---|---|---|
| STC-41 | AgentKey → owning window registry lookup | fixture test |
| STC-42 | activate/show owner before navigation | adapter test |
| STC-43 | same terminal ID across two instances | collision test |
| STC-44 | inactive/unobserved instance not falsely watched | source/static audit |

### S7 — dynamic/performance/accessibility

| ID | Task | Verify |
|---|---|---|
| STC-45 | snapshot changes only on relevant semantic changes | equality tests |
| STC-46 | no render-time IO | static/source audit |
| STC-47 | no status polling | watcher/timer audit |
| STC-48 | keyboard open/navigate/close | headless/input test |
| STC-49 | accessible labels/no color-only state | accessibility test |
| STC-50 | large Agent list panel performance | virtualized/list benchmark |
| STC-51 | quick-panel positioning across display edges | geometry test |

### S8 — package acceptance

| ID | Task | Verify |
|---|---|---|
| STC-52 | titlebar + quick-panel visual evidence light/dark | render smoke |
| STC-53 | tray/status/usage lifecycle soak | scripted status churn |
| STC-54 | real app quick-jump + menu-bar-panel acceptance | focused real app |
| STC-55 | 0.7 package build | full test/build gate |

## 26. Focused acceptance scenarios

1. No Agents → quiet right-side status button.
2. One Agent Working → Working count appears and row jumps to its Chat/Agent surface.
3. Working → Blocked → trigger immediately promotes Needs Attention.
4. Pending structured permission → row jumps to exact InteractionCard.
5. Blocked without structured interaction → row jumps to Terminal.
6. Working → Done → Review count appears.
7. Opening the Review Agent clears unread but leaves ReviewPending.
8. Mark reviewed clears Review count in Header, panel and tray together.
9. Agent starts Working again → stale ReviewPending disappears.
10. Agent released → row disappears everywhere.
11. Two Projects with same Agent display name route by AgentKey, not label.
12. Same terminal ID in two instances does not collide.
13. Agent disappears between menu open and click → no incorrect jump.
14. Herdr disconnects → stale actions disabled; Reconnect is visible.
15. Reconnect restores status from authoritative projection.
16. Menu-bar click opens a Native floating panel below the tray icon.
17. Clicking the menu-bar icon again, Escape, or focus loss hides the panel.
18. Quick panel remains screen-safe near display edges.
19. Agent rows show usage/model/quota only when known; missing values never appear as zero.
20. Tray title/tooltip/panel update on semantic change but not every frame.
21. Tray/quick-panel Agent click shows/activates main window and reaches correct destination.
22. `Open Agents` reaches the full Agent Workbench.
23. Ctrl-Tab MRU still works independently.
24. No global Herdr focus RPC occurs during any quick jump.

## 27. Performance targets

- deriving summary from current Agent directory should be sub-millisecond for typical Agent counts;
- panel open performs zero network/filesystem/subprocess work;
- floating quick-panel placement is pure geometry over Tray.Bounds/screen bounds;
- tray/quick-panel rebuild only after semantic snapshot change;
- usage/quota refresh happens in services, never on panel open/render;
- no idle polling solely for the Status Center;
- list rendering remains virtualized/bounded for large Agent sets;
- status churn must not reconstruct hidden heavy routes;
- no global window repaint loop for Working glyph animation.

## 28. Definition of Done

0.7 is complete when:

- [ ] the application titlebar has one right-side Status Center trigger;
- [ ] old minimal titlebar activity control is removed/replaced;
- [ ] Needs Attention / Review / Working counts use one shared snapshot;
- [ ] panel rows reuse the shared AgentCardModel/compact Agent component;
- [ ] filters work;
- [ ] pending structured interaction quick-jumps to the exact Chat interaction;
- [ ] blocked/failed quick-jumps to the Terminal;
- [ ] review/working rows jump to the correct Agent/Conversation surface;
- [ ] navigation clears unread but never implicitly clears ReviewPending;
- [ ] Mark reviewed updates every status surface;
- [ ] MyGo Tray uses the same snapshot/action system;
- [ ] macOS tray click opens a MyGo Native floating quick panel under the menu-bar item;
- [ ] quick panel hides on focus loss/Escape and stays within the active display work area;
- [ ] Agent rows expose model/token/cost/quota usage when known without changing attention priority;
- [ ] missing/stale Usage is represented truthfully;
- [ ] tray unavailable never breaks the in-window Status Center;
- [ ] cross-instance identity is collision-safe;
- [ ] no status polling/render-time IO exists;
- [ ] no global Herdr focus navigation is introduced;
- [ ] tests/build/real-app acceptance pass.

## 29. Next boundary — 0.8 History Management, Usage & Agent Inspector

The Magpie reference audit identified a higher-value closure milestone before Desktop Tools:

```text
0.8 — History Management, Usage & Agent Inspector
Project-grouped History
Trash / Restore / Purge with capability gating
Session / Project / Agent usage
Provider/model/quota attribution
Agent Inspector
```

Plan: `docs/mygo-native-0.8.0-history-usage-agent-inspector-plan.md`.

Desktop Tools move to 0.9+.

Remote/Mobile API v2 and platform/cutover can reuse the same History/Usage/Agent services after they are stable.

