# Shardlane MyGo 0.3.0 — Closure Audit

Status: **next-stage audit**
Audit date: 2026-10-05
Branch: `rewrite/mygo`
Architecture authority: `docs/client-product-architecture.md`
Execution rules: `docs/mygo-native-execution-rules.md`

## 1. Executive verdict

0.3.0 has crossed the line from a shell prototype into a real Native application slice:
Settings and read-only History are functional, Native Terminal/runtime ownership is correct,
and the Go domain split is established.

The next risk is no longer “can MyGo implement this app?” The next risk is **fragmentation**:
page-local styling, duplicated state interpretation, incomplete Agent/Hook workflows, and
presentation state that is dynamic but not yet modeled through one shared operational layer.

The recommended next release is therefore **0.4.0 — Operational Closure & UI System**.
It should not start by adding more large surfaces. It should first make the existing surfaces
behave as one product.

## 2. Framework audit

### Current

- Shardlane pin: `github.com/egoist/mygo v0.2.5`.
- Go toolchain: 1.27.1.

### Latest verified upstream

Official MyGo releases list `v0.2.7` as Latest on 2026-10-05.
The relevant changes since 0.2.5 are maintenance/behavior fixes rather than a new component generation:

- 0.2.6: Native UI live-resize fix on Windows and drawing fixes.
- 0.2.7: one frame per Core Animation transaction on macOS; List scroll-thumb/initial-state fixes.

The large Native component expansion (Toolbar, Combobox, SearchField, Form, Outline, Sidebar,
Badge, GridView, status controls, Router, etc.) already landed in the 0.2.5 generation. 0.2.7
makes that foundation safer; it does not require a new application architecture.

### Compatibility probe

A temporary copy of the current 0.3.0 tree was upgraded to MyGo 0.2.7 without touching the repository.
The following all passed:

```text
go mod tidy

go test ./...

go tool mygo build
```

The upgraded temporary app built successfully at approximately the same size as the 0.3.0 package.

**Audit result: upgrade to 0.2.7 is low-risk and should be the first framework task in 0.4.0.**

## 3. UI system audit

### What is good

The Native UI already has:

- `theme.go`;
- `DesignTokens` color vocabulary;
- reusable SVG icon set;
- `iconButton`;
- `navButton`;
- `panelCard`;
- `treeRow`;
- shared row tint behavior;
- macOS vibrancy baseline;
- MyGo headless render tests.

This is a useful foundation and should be evolved rather than replaced.

### What is not closed

The design system currently centralizes mostly **colors**, not full visual metrics or semantics.
A source audit of `next/internal/nativeui` found approximately:

```text
72 direct ui.Text calls
45 direct FontSize calls
33 direct Padding calls
12 direct ui.Button calls
6 direct ui.Badge calls
4 direct Spinner calls
```

Consequences:

- typography hierarchy is page-local;
- spacing/radius choices drift between History, Settings, Sidebar and dialogs;
- loading/error/empty states are reimplemented per page;
- buttons do not have one size/variant contract;
- provider/runtime statuses are interpreted ad hoc;
- future Chat would create a second visual language unless this is fixed first.

### Required Design System v2

Move from “helpers” to a real presentation contract:

```text
DesignTokens
├─ semantic colors
├─ typography scale
├─ spacing scale
├─ radius scale
├─ control heights
├─ icon sizes
└─ density variants

Shared Native components
├─ AppPage / PageHeader
├─ Toolbar / ToolbarIconButton
├─ Section / SectionHeader
├─ Card / SettingsCard / FormRow
├─ ListRow / TreeRow
├─ StatusGlyph / StatusPill
├─ EmptyState
├─ LoadingState
├─ ErrorState
├─ InlineNotice
├─ ProviderBadge
├─ AgentRow
├─ ToolCard
└─ ConversationBlock primitives
```

Page code should choose semantics, not raw sizes/colors.

## 4. Official MyGo component adoption audit

The app should prefer official MyGo components where they already solve the interaction/accessibility problem.

### Adopt in 0.4

- `ui.List` for History session list and future Activity/Agent lists.
- `ui.Toolbar` for workspace/page toolbars where behavior matches.
- `ui.Form` / field primitives for Settings and New Task controls.
- `ui.Collapsible` for History thinking/tool detail disclosure.
- `ui.Toast` for transient successful/failed actions.
- `ui.Breadcrumbs` where it preserves the Shardlane shell geometry.
- official Sidebar primitives where they preserve the canonical hierarchy.

### Spike before replacing custom code

- `ui.Outline` for the Project → Tab → Pane tree.

Shardlane's current tree has semantic selection, custom connector guides, status presentation,
context menus and Herdr-layout semantics. Do not rewrite it merely to use an official component.
Run one focused parity spike; keep the custom tree if Outline cannot preserve the required UX cleanly.

## 5. History state audit

0.3.0 History is real, bounded and read-only. The domain architecture is good.

Two closure issues remain.

### H-01 — list request latest-wins bug

`requestHistoryList` refuses a new request while `hist.loading` is true.
If the user changes provider/search while a request is in flight, the new request can be dropped;
when the old request returns, rows can describe the previous query while the UI shows the new filter.

Required fix:

```text
new list intent
→ cancel previous list context
→ increment generation
→ issue new query immediately
→ only latest generation may apply
```

Do not serialize interactive filtering behind a stale request.

### H-02 — list rendering is not virtualized

The current History query can return 100 rows and constructs them with a plain Column.
Use MyGo 0.2.7 `ui.List` with persistent `ListState`.

This gives:

- bounded row construction;
- correct long-list scrolling;
- better keyboard/accessibility behavior;
- direct benefit from the 0.2.7 List fixes.

History detail remains bounded to ≤60 messages, so it does not need large-list virtualization yet,
but its body must have an explicit scroll/list owner and preserve paging position intentionally.

## 6. Agent launch audit

The Agent package is correctly UI-independent and Start Agent remains disabled, which is safer than exposing a partial transaction.

### Critical finding: LaunchRegistry is not actually concurrent-idempotent

Current flow:

```text
lock
check map
unlock
launch(...)
lock
save outcome
unlock
```

Two concurrent callers with the same RequestID can both pass the first check and both call launch.
A temporary audit test reproduced the issue:

```text
launch called 2 times
```

This must be fixed before Start Agent can be enabled.

Required semantics:

```text
RequestID -> one in-flight owner
         -> duplicates await the same result
         -> completed result replayed
         -> conflicting request shape rejected
```

Do not hold the global registry mutex around the whole network launch; use an in-flight entry/singleflight-style state.

## 7. Runtime status audit

### Current

Herdr already provides dynamic Agent status through the reconciled projection and
`pane.agent_status_changed` subscription.

Native UI currently renders only raw strings through:

```text
statusBadge("working" | "blocked" | "done")
```

This proves dynamic state propagation works, but not product-level status semantics.

### Missing shared operational model

The canonical architecture already defines attention priority:

```text
NeedsAttention > Working > ReadyForReview > Idle > Resolved
```

0.4 should implement one pure shared projection:

```go
type OperationalState int

type OperationalItem struct {
    Kind
    ID
    Provider
    RuntimeState
    AttentionState
    Label
}

type OperationalSummary struct {
    HighestPriority
    NeedsAttentionCount
    WorkingCount
    ReadyForReviewCount
}
```

Inputs remain authoritative data already held in memory:

- Herdr Agent projection;
- future Script/Service projections;
- bounded review receipts if implemented.

Consumers must not reinterpret raw status strings separately:

- Titlebar Activity control;
- Agents section;
- Project aggregate indicator;
- Pane/Agent rows;
- future Activity route;
- future menu-bar item.

### Dynamic rendering rules

- blocked/attention appears immediately;
- working is event-driven and may use a short anti-flicker presentation delay;
- no polling loop for runtime state;
- one event burst causes one reconciled projection application;
- hidden surfaces do not perform expensive rendering;
- no persistent healthy-connection badge in normal chrome;
- offline/reconnecting is actionable and explicit.

## 8. Hook / Integration Health audit

The old Rust product already contains two important concepts that must remain distinct.

### A. Runtime lifecycle status

`working / blocked / idle / done` belongs to Herdr runtime state.

### B. Provider integration / hook installation health

Examples from the existing Rust authorities:

```text
Installed
Outdated
NotInstalled
HerdrManaged
Unsupported
```

And the newer integration strategy registry distinguishes:

```text
HerdrOfficial
HerdrScreenOnly
HerdrScreenWithManagedSessionBridge
ManagedLifecycleBridge
Deferred
```

The MyGo rewrite currently has **no Native integration-health surface**.

### Do not port the old AgentHookSettings screen line-by-line

The newer architecture has moved many providers to official Herdr integration authority.
The Native rewrite should expose one architecture-correct **Provider Integration Health** service, not blindly reinstall a second autonomous hook beside Herdr.

Recommended UI states:

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

Recommended row content:

```text
Provider icon + name
Runtime strategy
Integration status
version/path detail when available
Install / Update / Repair / Refresh action when safe
```

### Dynamic refresh contract

Never run integration probes from render.

Refresh on:

1. Settings → Providers/Integrations route entry;
2. explicit Refresh;
3. after Install/Update/Repair completes;
4. optional application activation if the last audit is stale.

Do not poll every N seconds while idle.

The async shape should mirror the good pattern already proven in the Rust client:

```text
Checking state
→ background audit
→ generation/token guard
→ window update
→ Native list rebuild
```

Actions must immediately transition the affected row to an in-flight state and reconcile from a fresh audit after completion.

## 9. UI/operational features still missing

### Must close in 0.4

- MyGo 0.2.7 upgrade.
- History latest-request-wins behavior.
- History `ui.List` virtualization.
- Design System v2 metrics/components.
- shared semantic status renderer.
- Provider Integration/Hook Health Native surface.
- Activity/operational summary projection and Header/Sidebar display.
- concurrent-safe launch idempotency.
- full New Task Agent launch transaction if all readiness/uncertain-delivery gates are complete.
- consistent toast/error/empty/loading states.

### Nice to close if it does not delay the version

- Gemini History adapter.
- `ui.Outline` Project-tree parity spike.
- provider icon/brand registry shared across History/New Task/Agent/Integration rows.

### Defer to 0.5

- full Native Chat.
- live semantic conversation timeline.
- follow-up queue UI.
- interaction resolution UI.
- context transfer/live handoff UI.

Chat should consume the 0.4 design/status/provider primitives rather than create parallel ones.

## 10. Audit status matrix

| Area | Status | 0.4 action |
|---|---|---|
| MyGo framework | PARTIAL | upgrade 0.2.5 → 0.2.7 |
| Native shell/runtime | GOOD | preserve |
| Design tokens | PARTIAL | add metrics/typography/semantic statuses |
| Shared components | PARTIAL | establish UI kit contract |
| History domain | GOOD | preserve |
| History request concurrency | BUG | latest-wins cancellation |
| History list rendering | PARTIAL | migrate to `ui.List` |
| Settings functionality | GOOD | restyle through shared Form/Card components |
| Agent capabilities | PARTIAL | preserve + integration-health merge |
| Agent idempotency | BUG | concurrent single-owner fix |
| New Task | NOT CLOSED | complete safe transaction before enable |
| Runtime Agent status | PARTIAL | shared OperationalSummary + renderer |
| Hook/integration health | MISSING | Go service + Native dynamic UI |
| Activity surface | MISSING | Native operational projection/surface |
| Chat | DEFERRED | later roadmap superseded the original 0.5 target: 0.5 = Agent Workbench, 0.6 = Native Conversation/Chat |
| WebView | FROZEN | no change |
