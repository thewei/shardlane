# Shardlane MyGo 0.4.0 — Operational Closure & UI System Plan

Status: **proposed next implementation release**
Target: `0.4.0`
Depends on: audited 0.3.0 Native Settings + History build
Architecture authority: `docs/client-product-architecture.md`
Audit input: `docs/mygo-native-0.3.0-closure-audit.md`

## 1. Version goal

0.4.0 is the **closure release** between “Native History/Settings work” and “Native Chat”.

Its goal is:

> Make every currently visible operational surface use one framework version, one design system,
> one Agent/provider status model, and complete end-to-end actions instead of disabled or informational shells.

0.4.0 should make the app feel like one macOS product before adding Chat complexity.

## 2. Definition of closed loop

A feature is not considered complete because a page/button exists.

Every 0.4 feature must satisfy:

```text
source of truth
→ application/domain service
→ presentation state
→ Native UI
→ user action
→ mutation/background work
→ reconciliation
→ rendered success/error state
→ focused test
```

If one arrow is missing, mark the feature PARTIAL.

## 3. Milestone order

```text
C0 correctness blockers
        ↓
C1 MyGo 0.2.7
        ↓
C2 Design System v2
        ↓
C3 Operational status model
        ↓
C4 Provider Integration / Hooks health
        ↓
C5 New Task / Agent launch closure
        ↓
C6 dynamic-render/performance pass
        ↓
C7 0.4 package gate
```

## C0 — correctness blockers

### CLOSURE-01 — History list latest-wins

Replace `loading => reject new intent` with cancellable list requests.

State additions:

```go
listCancel context.CancelFunc
listGen atomic.Uint64
```

Contract:

- each changed query/provider/filter starts a new request;
- previous list request is cancelled;
- stale generations cannot apply;
- loading belongs to the latest request only.

Verify:

```sh
go test ./internal/nativeui -run TestHistoryListLatestRequestWins
```

### CLOSURE-02 — concurrent Agent idempotency

Implement one in-flight owner per RequestID.

Requirements:

- duplicate equivalent requests wait for same result;
- launch callback called exactly once;
- completed result replays;
- same RequestID + different request body returns conflict;
- cancellation of one waiter does not create a second launch.

Verify with concurrent barrier tests and `-race` for the package.

### CLOSURE-03 — explicit scroll owner for History detail

Ensure bounded detail content is always scrollable and paging preserves intentional position.

## C1 — MyGo 0.2.7 upgrade

### FW-01 — bump framework

Change:

```text
github.com/egoist/mygo v0.2.5
→ github.com/egoist/mygo v0.2.7
```

Run:

```sh
go mod tidy
go test ./...
go tool mygo build
```

### FW-02 — render baseline

Re-run Native render-smoke evidence after the upgrade:

- Workspace;
- History list;
- History detail;
- Settings;
- New Task.

### FW-03 — interaction smoke

Focused real-app smoke:

- live resize on macOS;
- History scrolling;
- list selection;
- Router Back/Forward;
- Terminal unaffected.

Do not combine the framework bump with design-system rewrites in the same task.

## C2 — Design System v2

### DS-01 — full metric tokens

Expand tokens beyond colors:

```go
type TypographyTokens struct {
    Title, Section, Body, BodySmall, Caption, Micro float32
}

type SpaceTokens struct {
    XXS, XS, S, M, L, XL float32
}

type RadiusTokens struct {
    Control, Row, Card, Dialog float32
}

type ControlTokens struct {
    CompactHeight, RegularHeight, IconSmall, IconRegular float32
}
```

No page-local magic numbers for common typography/spacing.

### DS-02 — semantic status palette

Define semantic status, not provider/raw-string colors:

```text
neutral
info
working
success
warning
attention
error
muted
```

Support light/dark themes.

### DS-03 — shared page primitives

Implement:

- `appPage`;
- `pageHeader` v2;
- `sectionHeader`;
- `settingsCard`;
- `formRow`;
- `emptyState`;
- `loadingState`;
- `errorState`;
- `inlineNotice`;
- `actionToolbar`;
- `statusGlyph`;
- `statusPill`;
- `providerBadge`.

### DS-04 — raw-style reduction pass

Migrate History, Settings, New Task, Sidebar and Workspace toolbar.

Acceptance:

- page files should not define common font-size/radius/spacing policy;
- raw MyGo controls are allowed when they are the semantic control itself, but visual variants come from shared primitives;
- one component test covers each reusable state.

### DS-05 — official MyGo component adoption

Use:

- `ui.List` for History;
- `ui.Form`/Field for Settings/New Task;
- `ui.Collapsible` for thinking/details;
- `ui.Toast` for transient action result;
- `ui.Toolbar` where behavior matches.

### DS-06 — Project Outline spike

Prototype official `ui.Outline` with:

- Project/Tab/Pane hierarchy;
- local selection;
- multi-Project disclosure;
- context menus;
- status glyphs;
- connector/disclosure semantics;
- accessibility.

If parity is worse or code becomes less clear, record NO-GO and keep `treeRow`.

## C3 — shared operational status

### OPS-01 — normalize Agent runtime status

Pure parser:

```text
blocked / failed      → NeedsAttention
working / pending     → Working
launch_pending        → Working
idle                  → Idle
done                  → ReadyForReview
unknown               → Idle/Unknown presentation-safe fallback
```

Do not change Herdr authority.

### OPS-02 — OperationalSummary

Pure derivation from the current projection.

Priority:

```text
NeedsAttention > Working > ReadyForReview > Idle > Resolved
```

### OPS-03 — shared status presentation

One `statusGlyph/statusPill` implementation consumed by:

- Agents list;
- Project aggregate;
- Tab/Pane rows;
- Titlebar Activity;
- Activity surface;
- future Chat.

Do not render raw strings independently in page code.

### OPS-04 — Titlebar Activity

Rules:

- attention always visible when actionable;
- ordinary working state appears in Header primarily when Sidebar cannot already show it;
- clicking reveals Activity/Agents section;
- healthy Herdr state does not consume permanent chrome;
- disconnected state is a reconnect action.

### OPS-05 — dynamic update tests

Script event sequences:

```text
idle → working → blocked → working → done
```

Assert all consumers derive the same status without additional polling.

## C4 — Provider Integration / Hook Health

### INT-01 — Go integration strategy registry

Port the current Rust authority as pure data:

```text
HerdrOfficial
HerdrScreenOnly
HerdrScreenWithManagedSessionBridge
ManagedLifecycleBridge
Deferred
```

Every History AgentID must have exactly one explicit strategy.

### INT-02 — official Herdr integration health

Go adapter over:

```sh
herdr integration status
```

Parse target/state/version/path once per audit.

No install from render.

### INT-03 — IntegrationHealthService

Presentation-independent state:

```go
type IntegrationHealthState string

const (
    Checking
    Current
    Outdated
    NotInstalled
    ManagedByHerdr
    ManagedBridgeReady
    Deferred
    Unsupported
    Error
)
```

Service methods:

```go
Audit(ctx) []ProviderIntegrationHealth
RefreshProvider(ctx, provider)
Install(ctx, provider)
Repair(ctx, provider)
```

Only expose an action when its implementation is safe for that strategy.

### INT-04 — Native Providers/Integrations Settings page

Each row:

- provider mark/name;
- product exposure;
- runtime strategy;
- integration health;
- path/version detail;
- Install/Update/Repair/Refresh action.

### INT-05 — dynamic action state

Row lifecycle:

```text
checking
current/not-installed/etc
→ action clicked
→ working spinner + disabled duplicate action
→ action finishes
→ fresh audit
→ reconciled final row
```

Use Toast for transient success/failure, persistent inline detail for unresolved errors.

### INT-06 — Hook status compatibility

Preserve the meaningful legacy statuses:

```text
Installed
Outdated
NotInstalled
HerdrManaged
Unsupported
```

But map them through the newer strategy authority; never install a parallel Shardlane lifecycle hook when Herdr official integration is the authority.

### INT-07 — audit cadence

Audit on:

- route entry;
- manual refresh;
- post-action;
- stale app activation.

No fixed idle polling.

## C5 — New Task / Agent launch closure

### LAUNCH-01 — complete idempotency before transport

Depends on CLOSURE-02.

### LAUNCH-02 — Herdr launch transaction adapter

Port the verified Rust transaction semantics, not the Rust implementation mechanics:

```text
validate
→ prepare target
→ ensure integration
→ create structure without global focus
→ agent.start
→ reconcile uncertain response
→ readiness + identity verification
→ plan-mode post-ready setup when required
→ exactly one semantic initial prompt
→ authoritative projection
```

### LAUNCH-03 — uncertain-delivery reconciliation

Never blindly repeat a mutation after the request may have been written.

### LAUNCH-04 — New Task provider picker

Drive available providers from the shared capability + integration-health model.

UI state must explain:

- ready;
- setup required;
- updating integration;
- unavailable/deferred.

### LAUNCH-05 — enable Start Agent

Only after the full transaction tests pass.

Button lifecycle:

```text
ready
→ launching
→ created/ready
→ navigate locally to new target
```

Failure after commit must present “Agent created but setup/prompt failed” and navigate/reconcile the created Agent instead of encouraging duplicate retry.

## C6 — Dynamic rendering / performance

### RENDER-01 — History list virtualization

Persistent `ui.ListState`, stable item keys, list selection and keyboard behavior.

### RENDER-02 — no render-time IO

Audit Native render paths:

- no filesystem probe;
- no subprocess;
- no SQLite query;
- no Herdr RPC;
- no integration audit.

All IO is service/background state.

### RENDER-03 — visible-only work

Hidden routes must not build large row/block trees.

### RENDER-04 — no-op state updates

Avoid `win.Update` when the derived state did not change materially.

### RENDER-05 — status anti-flicker

Blocked/attention immediate.
Working indicator may delay presentation briefly to avoid flashing on instant operations.
Do not delay underlying state.

### RENDER-06 — render evidence

Render smoke frames for:

- light/dark Sidebar;
- working/blocked/done states;
- integration Checking/Current/NotInstalled/Error;
- History list/detail;
- Settings;
- New Task ready/launching/error.

## C7 — package gate

Before 0.4 test package:

```sh
go test ./...
go test -race ./internal/agent/... ./internal/app/...
git diff --check
go tool mygo build
```

Forbidden-surface audit:

- no React/Vite/xterm;
- no Chat/History WebView;
- no global Herdr focus RPC navigation;
- no duplicate PTY/runtime/layout authority;
- no integration probes from render.

Focused real-app acceptance:

1. Settings/History regressions;
2. History fast filter changes always show latest query;
3. 100+ History rows scroll correctly;
4. runtime Agent status updates dynamically;
5. integration health loads asynchronously;
6. integration action reconciles immediately;
7. New Task launch creates exactly one Agent under duplicate-click/concurrent-submit pressure;
8. blocked/failed/uncertain launch state is truthful;
9. Terminal remains unaffected;
10. logs contain no prompts/transcript/terminal output.

## 4. 0.4 completion criteria

0.4 is complete when:

- [ ] MyGo 0.2.7 is pinned and verified.
- [ ] History list latest-request-wins is fixed.
- [ ] History list uses virtualized/native collection behavior.
- [ ] concurrent duplicate StartAgent request cannot launch twice.
- [ ] Design System v2 owns common metrics and semantic status visuals.
- [ ] History/Settings/New Task/Shell use shared primitives.
- [ ] Agent runtime status is one shared derived model.
- [ ] Header/Sidebar/Project/Pane statuses agree dynamically.
- [ ] Providers/Integrations Native Settings page shows real health.
- [ ] Hook/integration actions have checking/working/success/error reconciliation.
- [ ] no duplicate Herdr lifecycle authority is introduced.
- [ ] Start Agent is enabled only if the full transaction is complete.
- [ ] full tests/race tests/build pass.
- [ ] no WebView is introduced for Chat/History.

## 5. 0.5 boundary — Agent Workbench & Lifecycle

0.5 does **not** jump directly into full Chat. It first turns the 0.4 launch/status primitives into a complete Agent workbench:

- stable Agent identity and one AgentCardModel;
- unread/review-pending semantics;
- process/window Agent directory;
- Header Agent Overview;
- Native `/agents` page;
- Ctrl-Tab Agent MRU switcher;
- meaningful Agent notifications;
- History Continue / AlreadyLive / NativeResume / ContextTransfer.

Plan: `docs/mygo-native-0.5.0-agent-workbench-plan.md`.

Full Native Chat moves to 0.6 so it can reuse the Agent lifecycle instead of becoming the lifecycle engine itself.
