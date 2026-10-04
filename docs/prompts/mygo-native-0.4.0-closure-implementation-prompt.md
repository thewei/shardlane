# Shardlane MyGo 0.4.0 Closure Implementation Prompt

Continue the Shardlane MyGo migration in:

`/Users/wilson/Workspaces/wh-studio/herdr-client`

Use the existing Devspace checkout/workspace and branch `rewrite/mygo`.
Preserve all user/other-agent changes. Do not reset, stage, commit, or push unless explicitly requested.

## Read first

1. `AGENTS.md`
2. `CLAUDE.md`
3. `.agents/skills/herdr-client-development/SKILL.md`
4. `docs/client-product-architecture.md`
5. `docs/performance-engineering.md`
6. `docs/mygo-native-execution-rules.md`
7. `docs/mygo-native-migration-roadmap.md`
8. `docs/mygo-native-0.3.0-audit.md`
9. `docs/mygo-native-0.3.0-closure-audit.md`
10. `docs/mygo-native-0.4.0-closure-plan.md`
11. `next/CLAUDE.md`

Treat `docs/client-product-architecture.md` as the sole architecture authority.

## Mission

Implement **0.4.0 — Operational Closure & UI System**.

Do not start by adding Chat. First make the existing Native application one coherent product:

- latest MyGo framework;
- one Design System;
- one shared runtime status model;
- real provider integration/hook health;
- dynamically reconciled Native UI;
- complete safe New Task launch transaction;
- no half-working visible controls.

## Non-negotiable runtime rules

- Herdr remains the sole authority for runtime Workspaces/Projects, Tabs, Panes/layouts, Agents, PTYs, terminal IDs, process lifecycle and persistence.
- Project/Tab/Pane navigation remains client-local.
- Never reintroduce `workspace.focus`, `tab.focus`, or `pane.focus` for normal Shardlane navigation.
- Never subscribe to global Herdr focus events as navigation authority.
- Terminal geometry remains from Herdr authoritative layouts.
- Do not create client PTYs or a second Pane/runtime model.
- Do not auto-attach with `--takeover`.
- History and Chat stay Native UI; no WebView/WKWebView/private browser embedding.
- Runtime Agent status is not Hook installation health. Keep these as separate models.

## P0 — fix correctness before polish

### 1. History latest-request-wins

Current bug: while a list request is loading, changed search/provider intent can be dropped.

Implement cancellation + generation semantics:

```text
new intent
→ cancel previous list request
→ new generation
→ immediately query latest filters
→ stale result ignored
```

Add a deterministic test that starts a slow first request, changes the filter, resolves the first request late, and proves only the second result appears.

### 2. LaunchRegistry concurrent idempotency

Current implementation is not safe under concurrent duplicate Submit calls. A temporary audit reproduced two launch callback invocations for the same RequestID.

Implement an in-flight entry/single-owner design:

- exactly one launch callback per RequestID;
- equivalent duplicates await/replay the same result;
- same RequestID with a conflicting request body returns conflict;
- do not hold one global mutex for the entire network launch;
- waiter cancellation must not create a second launch.

Run package race tests.

## P1 — upgrade MyGo first, separately

Upgrade:

```text
github.com/egoist/mygo v0.2.5
→ github.com/egoist/mygo v0.2.7
```

This upgrade has already been tested in a temporary copy: `go mod tidy`, `go test ./...`, and `go tool mygo build` all passed.

Still perform it as its own task and re-run render smoke before any UI refactor.

Do not combine framework upgrade and Design System rewrite in one diff.

## P2 — Design System v2

Expand the current `theme.go/components.go` rather than replacing them with a new frontend framework.

Add shared:

- typography tokens;
- spacing tokens;
- radius/control-size tokens;
- semantic status colors;
- `AppPage` / page chrome;
- Section/SectionHeader;
- SettingsCard/FormRow;
- Toolbar/IconButton variants;
- ListRow/TreeRow;
- EmptyState/LoadingState/ErrorState;
- InlineNotice/Toast action wrapper;
- ProviderBadge;
- StatusGlyph/StatusPill;
- AgentRow;
- ToolCard primitives reusable by future Chat.

Migrate existing History, Settings, New Task, Sidebar and Workspace surfaces.

Do not leave common FontSize/Padding/Radius decisions scattered through page files.

## P3 — adopt official MyGo components

Prefer official Native UI behavior:

- `ui.List` for History and operational lists;
- `ui.Form`/Field for Settings/New Task;
- `ui.Collapsible` for thinking/detail disclosures;
- `ui.Toast` for transient action results;
- `ui.Toolbar` where it fits;
- `ui.Breadcrumbs` if shell geometry remains correct.

Run a focused `ui.Outline` parity spike for Project/Tab/Pane. Do not force the migration if custom tree behavior/accessibility/status/context-menu parity is worse.

## P4 — one operational runtime status model

Implement a pure shared projection using the architecture priority:

```text
NeedsAttention > Working > ReadyForReview > Idle > Resolved
```

Herdr remains the source of runtime status.

Normalize raw runtime statuses once, then make these consumers use the same derived status/glyph implementation:

- Titlebar Activity;
- Agents section;
- Project aggregate;
- Tab/Pane/Agent rows;
- future Activity page;
- future Chat.

Blocked/attention renders immediately. Working may use a short presentation-only anti-flicker delay, but the underlying state is not delayed.

No runtime status polling. Continue using Herdr events + bounded reconciliation.

## P5 — Provider Integration / Hook Health

Do NOT port the old Rust Agent Hooks UI line-by-line.

Port the architecture-correct provider integration strategy registry:

```text
HerdrOfficial
HerdrScreenOnly
HerdrScreenWithManagedSessionBridge
ManagedLifecycleBridge
Deferred
```

Build a Go `IntegrationHealthService` that asynchronously audits real state.

Visible Native states:

```text
Checking
Current
Outdated
NotInstalled
ManagedByHerdr
ManagedBridgeReady
Deferred
Unsupported
Error
```

Preserve the meaningful legacy status semantics (Installed/Outdated/NotInstalled/HerdrManaged/Unsupported), but map them through the newer strategy authority so Shardlane never installs a competing lifecycle hook beside Herdr official integration.

Audit on:

- Providers/Integrations Settings route entry;
- manual Refresh;
- after Install/Update/Repair;
- optionally app activation when cache is stale.

Do not poll continuously and never run audit IO in render.

Each provider row must dynamically show:

- provider icon/name;
- strategy;
- status glyph/text;
- version/path when available;
- safe action;
- in-flight spinner/disabled duplicate action;
- reconciled final state.

Use Toast for transient result and persistent inline detail for unresolved errors.

## P6 — close New Task / Agent launch

Only enable Start Agent after the full transaction is ported and tested.

Preserve the verified Rust transaction behavior:

```text
validate
→ target/worktree preparation
→ ensure provider integration
→ create runtime structure without global focus
→ agent.start
→ uncertain-write reconciliation
→ readiness/identity verification
→ provider-specific Plan setup when required
→ exactly one semantic initial prompt
→ authoritative projection
→ local navigation to created target
```

Do not type prompts into a PTY.

If a post-commit phase fails, show truthful “Agent created, but … failed” state and navigate/reconcile the created Agent; do not present a simple Retry that can duplicate the launch.

Drive the provider picker from shared capability + integration health:

```text
Ready
Setup required
Updating
Deferred/unavailable
```

## P7 — dynamic rendering and performance

- use persistent `ui.ListState` for History list;
- explicit scrolling for bounded History detail;
- no filesystem/subprocess/SQLite/RPC work inside view/render functions;
- stale async results cannot apply;
- hidden routes do not build expensive content;
- avoid `win.Update` for materially identical state;
- no fixed high-frequency polling;
- keep logs bounded and content-private.

Add render-smoke evidence for:

- light/dark;
- working/blocked/done;
- provider integration Checking/Current/NotInstalled/Error;
- History list/detail;
- Settings;
- New Task ready/launching/post-commit-error.

## Verification cadence

For each atomic task:

```sh
focused go test
git diff --check
```

Do not full-build every task.

At 0.4 package gate:

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go test -race ./internal/agent/... ./internal/app/...
GOTOOLCHAIN=go1.27.1 go tool mygo build
cd ..
git diff --check
```

Then perform focused real-app acceptance.

## Required audit before handoff

Return a matrix of DONE / PARTIAL / NOT DONE for:

- MyGo 0.2.7;
- Design System v2;
- History latest-wins;
- History List virtualization;
- operational status model;
- Header/Sidebar dynamic status;
- Integration/Hook health;
- integration actions;
- concurrent launch idempotency;
- full Agent launch;
- Start Agent enabled;
- Chat (expected deferred unless explicitly expanded);
- Remote/Mobile;
- Windows/Linux.

Also provide:

- exact tests run;
- package path and SHA-256 if built;
- remaining architectural blockers;
- confirmation that no WebView/global focus/client PTY/runtime authority was introduced.

Proceed without asking for confirmation. Split any task expected to exceed four hours before implementing it.
