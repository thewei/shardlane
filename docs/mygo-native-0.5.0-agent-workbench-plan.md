# Shardlane MyGo 0.5.0 — Agent Workbench & Lifecycle Plan

Status: **planned post-0.4 release**
Target: `0.5.0`
Depends on: 0.4 Operational Closure & UI System complete
Architecture authority: `docs/client-product-architecture.md`
Execution rules: `docs/mygo-native-execution-rules.md`
Planning review: **reviewed 2026-10-05 against the post-0.4.0 tree** — authority files, MyGo API prerequisites (Tray/Notification where used) and 0.4 prerequisites (launch transaction, integration health, operational status model, Design System v2) all verified present; no stale statements found.

## 1. Version goal

0.5.0 is the Agent-focused release between 0.4 closure and full Native Chat.

Goal:

> Turn live Herdr Agents into first-class Shardlane work objects with one lifecycle model,
> one card model, one process/window directory projection, review/attention markers,
> fast Agent switching, safe Continue/Resume flow, and truthful dynamic actions.

0.5 is **not** “make an Agent card”. A feature counts as migrated only when the entire loop closes:

```text
Herdr/runtime fact
→ Go Agent projection
→ client-owned marker/policy
→ shared AgentCardModel
→ Native UI
→ user action
→ semantic service transaction
→ authoritative reconciliation
→ updated Agent card/status
```

Full Chat/Conversation timeline is deferred to 0.6 so it can consume the Agent primitives created here.

## 2. Original-project behavior authority

The 0.5 implementation must refer back to these existing Rust product semantics instead of inventing a new Agent model:

| Original authority | Behavior to preserve |
|---|---|
| `crates/herdr-gui/src/status.rs` | raw Agent status → attention mapping; transition classifier; review semantics |
| `crates/shardlane-host/src/attention.rs` | completion attention dedupe/suppression policy where authoritative completion facts exist |
| `crates/herdr-gui/src/agent_panel.rs` | process-wide Agent directory, overview filters, actionable sorting, Mark reviewed |
| `crates/herdr-gui/src/agent_switcher.rs` | Ctrl-Tab MRU Agent switcher, max 10, release-to-commit, Esc cancel |
| `crates/herdr-gui/src/sidebar/pane_rows.rs` | provider mark + Pane/Agent role + compact runtime status |
| `crates/herdr-gui/src/status_bar.rs` | blocked/review/working summary; no token/usage badge in primary Agent status chrome |
| `crates/herdr-gui/src/shell_navigation.rs` | unread/review marker lifecycle; status patch reconciliation; explicit review ack |
| `crates/shardlane-host/src/conversation_queue.rs` | single Agent sendability classifier and fail-closed prompt disposition |
| `crates/shardlane-host/src/agent_service.rs` | high-level Agent query/read/wait/semantic prompt service boundary |
| `crates/shardlane-host/src/history_continuation.rs` | Continue planner: AlreadyLive → NativeResume → ContextTransfer → NeedsProjectSelection → Unsupported |
| `crates/shardlane-host/src/live_handoff.rs` | live handoff safety/failure semantics; execution deferred to 0.6 unless prerequisites are complete |
| `crates/herdr-gui/src/agent_ui/activity.rs` | semantic tool-activity classification shared by History/Chat; do not infer activity from terminal output |
| `crates/herdr-gui/src/main.rs` | meaningful Agent notification transition rules |

Port observable product behavior, not GPUI mechanics.

## 3. The five separate Agent state axes

Do not collapse these into one status field.

### 3.1 Runtime phase — Herdr authoritative

```go
type AgentRuntimePhase string

const (
    AgentLaunching AgentRuntimePhase = "launching"
    AgentWorking   AgentRuntimePhase = "working"
    AgentBlocked   AgentRuntimePhase = "blocked"
    AgentIdle      AgentRuntimePhase = "idle"
    AgentDone      AgentRuntimePhase = "done"
    AgentFailed    AgentRuntimePhase = "failed"
    AgentUnknown   AgentRuntimePhase = "unknown"
)
```

Inputs include the authoritative Herdr fields such as `agent_status`, `custom_status`, and typed `launch_pending`.

### 3.2 Operational attention — shared 0.4 projection

```text
NeedsAttention > Working > ReadyForReview > Idle > Resolved
```

This is a presentation/priority projection, not runtime authority.

### 3.3 Client-owned markers

```go
type AgentMarkers struct {
    Unread        bool
    ReviewPending bool
}
```

Rules from the original client:

- initial projection does not manufacture unread/review transitions;
- `working → done` starts review-pending;
- `blocked → done` starts review-pending;
- transition to `done/blocked/failed` can create unread when that Agent is not currently selected/seen;
- selecting/visiting the Agent clears unread;
- visiting the Agent does **not** automatically clear review-pending;
- explicit `Mark reviewed` clears review-pending and unread;
- `done → working` clears stale review-pending;
- Agent release removes both markers.

### 3.4 Sendability — mutation safety

Port the original single classifier rather than deriving button state from `AttentionLevel`:

```text
working / pending / launch_pending  → MidTurn
blocked / failed                    → NeedsTerminal
idle / done                         → Sendable
unknown / absent                    → Unknown (fail closed)
```

```go
type AgentSendability string

const (
    Sendable      AgentSendability = "sendable"
    MidTurn       AgentSendability = "mid-turn"
    NeedsTerminal AgentSendability = "needs-terminal"
    Unknown       AgentSendability = "unknown"
)
```

This state is the future Chat/Follow-up safety source of truth.

### 3.5 Integration health — 0.4 service

Provider Hook/Integration health remains independent:

```text
Current / Outdated / NotInstalled / ManagedByHerdr / Deferred / Error ...
```

A live working Agent may have a separate historical/setup health fact. Do not replace runtime state with integration health or vice versa.

## 4. Stable Agent identity

Use a typed client key:

```go
type AgentKey struct {
    InstanceID string
    TerminalID string
}
```

Why:

- `terminal_id` is the stable primary live identity in the original client;
- `pane_id` may change/move and is a navigation locator, not the durable card identity;
- different Herdr instances may reuse terminal identifiers, so instance identity participates in the process-wide key.

Keep navigation locators separately:

```text
Workspace/Project ID
Tab ID
Pane ID
```

No Agent UI action may restore Herdr global-focus RPC navigation. Navigation remains client-local.

## 5. AgentCardModel

All Agent presentations consume one immutable card model.

Suggested shape:

```go
type AgentCardModel struct {
    Key             AgentKey
    Provider        history.AgentID
    ProviderLabel   string
    Title           string
    ProjectID       string
    ProjectName     string
    TabID           string
    PaneID          string
    RuntimePhase    AgentRuntimePhase
    Attention       OperationalState
    Sendability     AgentSendability
    Unread          bool
    ReviewPending   bool
    NeedsInput      bool
    ConversationID  string
    Revision        int64
}
```

Do not put presentation-only colors or MyGo elements in this model.

### Title/identity fallback

Preserve the old client intent:

```text
Agent title
→ Agent name
→ provider display name
→ "Agent"
```

Do not expose raw terminal/pane IDs as primary user-facing titles unless all semantic identity is unavailable.

## 6. Agent card variants

One model, several Native presentations.

### 6.1 Compact row

Used by Sidebar and Header overview.

Content:

```text
provider mark | title | unread dot | Review chip | project | status glyph + status text
```

### 6.2 Standard Agent card

Used by `/agents` Agent Workbench.

Content:

- provider mark + title;
- project/path context;
- runtime status;
- unread/review markers;
- primary action based on state;
- secondary local navigation action;
- optional conversation/session identity detail under disclosure.

### 6.3 Switcher row

Used by Ctrl-Tab overlay.

Content:

- provider mark;
- title;
- compact status;
- project when disambiguation is needed.

### 6.4 No independent Chat-card implementation

0.6 Chat consumes the same provider mark/status/tool primitives. It must not create a second Agent card/status system.

## 7. State-driven Agent card actions

Actions are derived from safe semantic facts.

### Launching / Working

Primary:

```text
Open Agent
```

Future Chat may additionally show `Send after turn`; 0.5 does not add a fake quick-prompt field.

### Blocked / Failed

Primary:

```text
Open Terminal
```

Reason: original sendability says `NeedsTerminal`. Do not send a generic prompt and do not guess PTY keys.

### Done + ReviewPending

Actions:

```text
Open Agent
Mark reviewed
```

Opening clears unread only. `Mark reviewed` is explicit.

### Done + already reviewed / Idle

Primary:

```text
Open Agent
```

### CreatedNeedsAttention after committed launch/continue

Show the committed Agent card and exact failure phase/detail.

Primary action navigates to the created Agent. Do not offer a blind full retry that could duplicate the Agent.

## 8. Agent transition policy

Port the original pure transition behavior:

```text
previous unknown          → no synthetic transition marker
same status               → no transition
working → done            → attention + review starts
blocked → done            → attention + review starts
working → blocked         → attention
working → idle            → ready notification only
working/done/etc → failed → attention
 done → working           → review clears
release                   → markers removed
```

All consumers derive from the same transition result:

- unread;
- review-pending;
- Header summary;
- Agent card;
- notification eligibility.

## 9. AgentDirectory

Port the useful original concept as a presentation projection, not runtime authority.

```go
type AgentDirectory struct {
    entries map[AgentKey]AgentCardModel
}
```

### Ownership

- written only from reconciled Herdr Agent projections plus client-owned markers;
- contains no runtime lifecycle logic;
- does not start background watchers for every inactive Herdr session;
- current single-window app initially reflects the actively observed instance;
- when true multi-window/multi-instance observation exists, each observed window/instance contributes its own entries without changing the model.

### Sorting

Preserve actionable-first behavior:

```text
NeedsAttention
ReadyForReview
Working
Idle
```

Then:

```text
Unread first
identity/title alphabetical
```

## 10. Header Agent Overview

Restore the original useful overview, using the 0.4 Design System.

Header summary contains counts only for actionable Agent state:

```text
blocked / needs attention
review pending
working
```

Do not put token/usage facts into the primary status control.

Popover filters:

```text
Working
Needs attention
Review
Idle
All
```

Default: `Working`.

Rows use `AgentCardModel` and support:

- local/cross-window Agent jump;
- Mark reviewed when applicable;
- consistent status glyph/provider mark.

## 11. `/agents` Agent Workbench

Add a real Native route for durable Agent management.

Suggested layout:

```text
Agents
[All] [Attention] [Review] [Working] [Idle]

┌ Agent Card ──────────────────────────────┐
│ Claude Code · Fix auth                   │
│ Project Portal                           │
│ ⚠ Needs input                            │
│                         [Open Terminal]  │
└──────────────────────────────────────────┘
```

Requirements:

- uses `ui.List` or another official virtualized collection;
- stable AgentKey row identity;
- filter state client-local;
- no runtime query from render;
- no transcript parsing;
- state changes update cards through the existing event/reconciliation stream.

## 12. Sidebar Agents section

Replace raw status rows with compact shared Agent card rows.

Rules:

- provider mark;
- title fallback;
- unread marker;
- review marker;
- one shared status glyph;
- click navigates locally;
- blocked click lands on the owning Terminal/Pane;
- no duplicated status mapper.

## 13. Agent MRU Switcher

Restore the old Ctrl-Tab Agent switcher after cards/directory are stable.

Scope for 0.5:

- current observed instance only, matching the original switcher's scope;
- MRU list max 10 live Agent identities;
- opening snapshots the current live ordering;
- Ctrl-Tab cycles forward;
- reverse shortcut cycles backward;
- releasing Ctrl commits;
- Esc restores the previous selection/focus;
- mouse hover changes highlighted row;
- click commits;
- vanished Agent fails closed/cancels;
- commit uses local Shardlane navigation, never Herdr global focus RPC.

## 14. Review / unread lifecycle

### Unread

A status transition worthy of attention while the Agent is not the current semantic selection adds unread.

Landing on/selecting the Agent clears unread.

### Review pending

A `done` transition creates review-pending.

Review-pending survives simply opening/focusing the Agent.

It clears only by:

1. explicit `Mark reviewed`;
2. a new working turn;
3. Agent release.

This preserves the old product semantics that “I saw the pane” and “I reviewed the result” are different actions.

## 15. Notifications

Restore the original narrow transition notifications if the platform/plugin path is officially supported.

Exact initial transition set:

```text
working → done   = Finished — ready for review
blocked → done   = Finished — ready for review
working → blocked = Needs attention
working → idle    = Ready
```

Rules:

- no notification for initial snapshot;
- no notification for same-status echo;
- obey `agent_notifications` setting;
- only escalate when the window/app is not already actively showing the event in-band;
- click routes locally to the owning Agent/Pane;
- markers remain the in-app source of truth even when OS notification delivery is unavailable.

Do not add arbitrary notifications for every status change.

## 16. Attention completion policy

The original Host contains content-signature dedupe and parent-alive suppression for completion attention.

Port the pure policy module in 0.5, but **activate each rule only when its required authoritative fact exists**.

- Do not derive completion signatures from terminal output.
- Do not invent a parent Agent relationship from titles/project structure.
- If semantic completion content / parent identity is not available before 0.6 Conversation projection, keep those inputs unavailable and rely on the transition/unread/review flow.

This preserves the architecture without fabricating facts.

## 17. History Continue / Resume

0.5 should close the Agent-facing continuation workflow because 0.4 already establishes safe Agent launch and 0.3 provides real History.

Planner order must match the original Host exactly:

```text
1. AlreadyLive
2. NativeResume
3. ContextTransfer
4. NeedsProjectSelection
5. Unsupported
```

### AlreadyLive

Exact typed identity match and same provider:

- do not create a duplicate Agent;
- jump to the existing live Agent;
- optional instruction uses the canonical semantic prompt transaction.

### NativeResume

Same provider + exact provider-native resumable session:

- use provider resume args;
- launch through the canonical Agent launch transaction;
- no fake transfer requirement.

### ContextTransfer

Different provider or no exact native resume:

- requires exact provider source;
- use bounded transfer artifact semantics from the original implementation;
- launch through canonical Agent launch;
- one initial briefing/prompt only.

### NeedsProjectSelection

Resolve Project before any runtime mutation.

### CreatedNeedsAttention

If runtime creation committed but setup/briefing later failed:

- surface the created Agent card;
- navigate to it;
- show exact phase/detail;
- never offer a blind whole-operation retry.

## 18. Live Handoff boundary

Do **not** automatically include full live Handoff execution in 0.5.

Reason: the original Handoff transaction depends on Conversation delivery coordination, exact source settle/freshness, pending-operation fences and blocked-state semantics. Those belong naturally with the 0.6 Native Conversation service.

0.5 may port pure eligibility/result models if they are needed by Agent cards, but it must not expose a Handoff action until the semantic transaction is complete.

## 19. Tool/activity boundary

The old `agent_ui/activity.rs` defines useful shared tool classes:

```text
Command
FileRead
FileSearch
FileChange
WebSearch
Plan
Generic
```

Port the pure classifier/visual component only if needed for History/0.6 preparation.

0.5 Agent cards must **not** infer “current activity” from terminal bytes/output.

Live tool activity belongs to semantic Conversation data in 0.6.

## 20. Atomic implementation plan

### A0 — domain and identity

| ID | Task | Fast verification |
|---|---|---|
| AGW-01 | typed AgentKey + runtime phase parser | table tests |
| AGW-02 | single sendability classifier | Rust behavior parity tests |
| AGW-03 | transition classifier | transition matrix tests |
| AGW-04 | AgentCardModel projection | fixture snapshot tests |

### A1 — client markers

| ID | Task | Fast verification |
|---|---|---|
| AGW-05 | unread marker store | transition/selection tests |
| AGW-06 | review-pending store | done/review/new-turn/release tests |
| AGW-07 | explicit Mark reviewed action | service/state tests |
| AGW-08 | stale marker cleanup | vanished Agent tests |

### A2 — Agent directory and cards

| ID | Task | Fast verification |
|---|---|---|
| AGW-09 | AgentDirectory projection | merge/remove/sort tests |
| AGW-10 | shared compact Agent row | headless UI |
| AGW-11 | shared standard AgentCard | headless state variants |
| AGW-12 | Sidebar Agents migration | headless UI |
| AGW-13 | `/agents` Native route | headless filter/list tests |
| AGW-14 | Header overview popover | filter/count/action tests |

### A3 — dynamic lifecycle

| ID | Task | Fast verification |
|---|---|---|
| AGW-15 | apply status transition once per event reconciliation | scripted event tests |
| AGW-16 | blocked/failed → Open Terminal action | headless action test |
| AGW-17 | done → Review state | transition/render test |
| AGW-18 | CreatedNeedsAttention card state | transaction-result fixture |
| AGW-19 | runtime status anti-flicker integration | deterministic timer test |

### A4 — Agent switcher

| ID | Task | Fast verification |
|---|---|---|
| AGW-20 | MRU ordering max 10 | pure ordering tests |
| AGW-21 | open/cycle/reverse/cancel state machine | pure state tests |
| AGW-22 | Native overlay rendering | headless UI |
| AGW-23 | Ctrl release commit + Esc restore | focused input test |
| AGW-24 | local navigation commit | no-global-focus regression |

### A5 — notification/attention

| ID | Task | Fast verification |
|---|---|---|
| AGW-25 | original notification transition classifier | table tests |
| AGW-26 | notification setting + inactive-window gate | service tests |
| AGW-27 | notification click → Agent navigation | platform adapter test |
| AGW-28 | attention dedupe policy pure port | original policy parity tests |

### A6 — History Continue

| ID | Task | Fast verification |
|---|---|---|
| AGW-29 | exact History/live identity matcher | id/path fixture tests |
| AGW-30 | read-only continuation planner | five-strategy table tests |
| AGW-31 | AlreadyLive execution | no-new-Agent test |
| AGW-32 | NativeResume execution | provider resume fixture |
| AGW-33 | ContextTransfer transaction | transfer + launch fixture |
| AGW-34 | NeedsProjectSelection flow | zero-mutation test |
| AGW-35 | CreatedNeedsAttention reconciliation | committed-target test |
| AGW-36 | Native History Continue UI/action | headless UI |

### A7 — package closure

| ID | Task | Fast verification |
|---|---|---|
| AGW-37 | no render-time Agent IO audit | source/static audit |
| AGW-38 | large Agent list virtualized rendering | render/list test |
| AGW-39 | lifecycle soak test | scripted status churn |
| AGW-40 | 0.5 real-app acceptance | focused manual/automated acceptance |

## 21. 0.5 performance rules

- Agent cards derive from already-reconciled projection; no card triggers RPC/file IO.
- one Herdr event burst → one reconciled Agent projection update.
- no per-card timer except centralized presentation anti-flicker state.
- MRU ordering is O(number of live Agents), bounded to displayed 10.
- `/agents` uses virtualized/native list behavior for large live sets.
- marker stores are bounded to live Agent identities and cleaned on release.
- no continuous poll for runtime status.
- Header count derivation is pure and cheap.
- process-wide directory does not create watchers for inactive sessions solely to populate cards.

## 22. 0.5 verification gate

Before package:

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go test -race ./internal/agent/... ./internal/app/... ./internal/nativeui/...
GOTOOLCHAIN=go1.27.1 go tool mygo build
cd ..
git diff --check
```

Focused real-app acceptance:

1. launch two or more Agents;
2. working status updates all visible card consumers consistently;
3. working → blocked produces Needs Attention and Open Terminal;
4. working → done produces ReviewPending;
5. opening Agent clears unread but not ReviewPending;
6. Mark reviewed clears review state everywhere;
7. done → working clears stale review;
8. closing/releasing Agent removes card + markers;
9. Header counts match Agent Workbench filters;
10. Ctrl-Tab cycles live Agents, release commits, Esc cancels;
11. no navigation emits Herdr global focus RPC;
12. History Continue reuses AlreadyLive Agent instead of duplicating it;
13. NativeResume creates exactly one target;
14. ContextTransfer creates exactly one target and one initial briefing;
15. CreatedNeedsAttention never offers unsafe blind retry;
16. no runtime state is inferred from terminal output;
17. Terminal behavior remains unchanged.

## 23. 0.5 Definition of Done

0.5 is complete only when:

- [ ] every live Agent has one stable AgentCardModel;
- [ ] runtime phase, attention, markers, sendability and integration health remain separate dimensions;
- [ ] unread/review semantics match the original client;
- [ ] one shared card/status system drives Sidebar, Header and `/agents`;
- [ ] Header Agent Overview filters and counts are correct;
- [ ] Ctrl-Tab Agent MRU switcher works;
- [ ] blocked/failed actions route to Terminal, not generic prompt;
- [ ] explicit Mark reviewed is implemented;
- [ ] meaningful Agent notifications are restored or explicitly platform-deferred;
- [ ] History Continue implements AlreadyLive/NativeResume/ContextTransfer/NeedsProjectSelection safely;
- [ ] no duplicate Agent is created by Continue or launch retry;
- [ ] no Agent UI owns runtime lifecycle;
- [ ] no Chat/WebView implementation is introduced;
- [ ] full tests/race tests/build pass.

## 24. 0.6 boundary — Native Conversation / Chat

0.6 may begin full Native Chat only after 0.5 closes Agent lifecycle.

It will reuse:

- AgentKey / AgentCardModel;
- runtime status and sendability;
- provider marks;
- unread/review semantics;
- ToolCard/activity primitives;
- safe launch;
- History Continue identities;
- Agent navigation/switcher;
- 0.4 Integration Health and Design System.

0.6 then owns the missing Conversation-specific work:

```text
semantic timeline
live decoder
composer
Send now / Send after turn
follow-up queue
blocked interaction resolution
Tool activity stream
context transfer / live handoff
conversation handoff
```

Plan: `docs/mygo-native-0.6.0-conversation-chat-plan.md`.
Prompt: `docs/prompts/mygo-native-0.6.0-conversation-chat-implementation-prompt.md`.

This ordering keeps Chat as another surface over an already-correct Agent lifecycle instead of making Chat the state engine.
