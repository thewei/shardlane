# Shardlane MyGo 0.8.0 — History Management, Usage & Agent Inspector Plan

Status: **planned post-0.7 release**
Target: `0.8.0`
Depends on: 0.7 Status Center & Agent Quick Navigation complete
Architecture authority: `docs/client-product-architecture.md`
Reference audit: `docs/reference-magpie-agent-history-usage-audit.md`

## 1. Version goal

0.8 turns History and Agent status from “browse and inspect lightly” into a complete management/insight workflow.

Goal:

> Group History by Project, safely manage removable sessions through Trash/Restore/Purge,
> expose trustworthy Agent/Session/Project usage, and provide one Agent Inspector combining
> runtime, integration, model, usage, quota and session identity without changing Herdr ownership.

0.8 borrows heavily from the product ideas observed in `yetone/magpie`, but it does **not** make Shardlane a mandatory model gateway.

## 2. Scope

Required:

```text
Project-grouped History
History search/filter/group state
Session management capability model
Trash / Restore / Delete Forever / Empty Trash
active/live-session deletion protection
Usage domain + UsageService
Session / Project / Agent usage aggregation
Provider/model attribution
optional quota/allowance capability
Agent Inspector
Status Center usage backed by the same service
```

Deferred:

```text
mandatory model proxy/gateway
provider routing groups/failover
credential vault as a prerequisite for Agents
request/response body capture
team billing enforcement
Desktop Files/Lazygit/Preview
```

## 3. Reference behavior from Magpie

The implementation should consult the reference audit and the inspected Magpie source areas:

```text
internal/gui/sessions_manage.go
internal/gui/assets/sessions.js
internal/sessions/manage.go
internal/sessions/stats.go
internal/usage/usage.go
internal/usage/ledger.go
internal/gui/trayusage.go
internal/gateway/*
```

Adopt observable management/accounting behavior where it fits Shardlane. Do not copy its Wails/WebView mechanics or gateway ownership model.

## 4. Architecture

```text
MyGo Native UI
│
├─ History
│  ├─ Project groups
│  ├─ Session detail
│  └─ Trash
│
├─ Usage
│  ├─ Agent
│  ├─ Project
│  ├─ Session
│  ├─ Provider
│  └─ Model
│
├─ Agent Inspector
│
└─ Status Center (compact usage consumer)
        │
        ▼
internal/app
├─ HistoryService              existing read/query contract
├─ HistoryManagementService    explicit mutation capability
├─ UsageService                numeric/accounting projection
├─ QuotaService                optional provider capability
└─ AgentInspectorService       composition only
        │
        ├───────────────┬────────────────┬────────────────┐
        ▼               ▼                ▼                ▼
internal/history    internal/usage    internal/agent   integrations
```

Rules:

- `nativeui` never moves/deletes provider files directly;
- Usage collection does not run from render;
- Trash mutations are separate from read-only History parsing;
- Agent Inspector composes existing services; it owns no runtime state;
- Herdr remains runtime/process authority.

## 5. History grouped by Project

Shardlane already owns normalized `project_key`, `project_name`, and a SQLite index on `project_key`.

### 5.1 Group identity

Group by:

```text
NormalizedProjectKey(SessionMeta.ProjectPath)
```

Display:

```text
ProjectName
ProjectPath
```

Fallback:

```text
No Project
```

Do not group by raw path spelling when normalized project identity is available.

### 5.2 Filter before grouping

Order:

```text
HistoryQuery
→ filter search/provider/model/date/project
→ sort sessions
→ group filtered sessions by project
→ render group tree/list
```

This avoids empty groups and matches the useful Magpie Sessions behavior.

### 5.3 Project-group header

Suggested fields:

```text
Project name
path
session count
latest activity
aggregate tokens when available
aggregate estimated cost when available
```

Usage aggregates are secondary and must never block group rendering.

### 5.4 Session row

Suggested row:

```text
provider mark  Session title                     2h ago
               short description
               Sonnet · 38k tok · $0.21 est. · 42 messages
```

Optional metadata:

```text
branch
model
size
message count
usage source indicator when needed
```

## 6. History presentation state

Client-local state:

```go
type HistoryPresentationState struct {
    Query            string
    Provider         history.AgentID
    Model            string
    ProjectKey       string
    DateRange        DateRange
    ExpandedProjects map[string]bool
    SelectedSession  string
    TrashVisible     bool
}
```

Rules:

- latest-request-wins/cancellation remains mandatory;
- project disclosure is presentation-only;
- search should not mutate catalog/source;
- returning from detail preserves useful group/filter state where Router permits it.

## 7. Session management capability

The current History domain is read-only by design.

0.8 does **not** change every adapter to writable. It introduces an explicit capability seam.

Suggested interface:

```go
type SessionManagementProvider interface {
    Agent() history.AgentID
    Capability() SessionManagementCapability
    ResolveSourceSet(ctx context.Context, session history.SessionMeta) (SessionSourceSet, error)
}

type SessionSourceSet struct {
    SessionKey string
    NativeID   string
    Items      []SessionSourceItem
}
```

Capability states:

```text
ReadOnly
Trashable
Restorable
Purgeable
```

Only show Delete for a session when its provider/source is proven Trashable.

## 8. Which providers are manageable

Do not decide manageability from Agent name alone.

A provider is Trashable only when the adapter can prove:

1. exact session identity;
2. complete file/directory source set;
3. no shared provider database row that cannot be isolated safely;
4. restoration paths;
5. active-session safety.

Initial candidates should be determined by implementation evidence/fixtures.

Unknown/database-backed sources remain read-only.

## 9. Active/live deletion safety

Before moving anything:

### Gate 1 — live Agent identity

If the session maps to a currently live Agent/provider session:

```text
Delete refused: session is active
```

This is stronger than an mtime heuristic.

### Gate 2 — recent-write safety window

Even without a live Agent match, refuse if any source item was modified inside a conservative recent-write window.

Initial target:

```text
60 seconds
```

Make the duration one tested constant, not page logic.

### Gate 3 — source identity revalidation

The source identity indexed by History must still match the source being deleted.

If it changed:

```text
refresh required
```

No stale mutation.

## 10. Trash layout

Use a Shardlane-owned root under application user data.

```text
trash/history/<provider>/<trash-key>/
├─ manifest.json
└─ files/
```

Manifest versioned from day one.

Suggested manifest:

```go
type TrashManifest struct {
    Version       int
    Key           string
    Provider      history.AgentID
    NativeID      string
    SessionKey    string
    Title         string
    ProjectPath   string
    ProjectKey    string
    DeletedAtMS   int64
    TotalBytes    int64
    Items         []TrashSourceItem
}

type TrashSourceItem struct {
    OriginalPath string
    StoredName   string
    Kind         SourceItemKind
    SizeBytes    int64
}
```

Never accept arbitrary paths from Native UI for Trash/Purge operations.

## 11. Delete transaction

```text
request SessionKey
→ load authoritative SessionMeta
→ resolve management adapter/capability
→ reject live session
→ revalidate source identity
→ resolve complete source set
→ recent-write guard over every source item
→ allocate unique trash key
→ create versioned manifest root
→ move items one by one, persisting manifest progress
→ if move fails: rollback already moved items
→ mark/rescan catalog
→ publish History change
```

Cross-volume moves need an explicitly tested safe copy/verify/remove path; do not silently fall back to an unsafe copy+delete.

## 12. Restore transaction

```text
load TrashManifest by key
→ verify key resolves inside Shardlane trash root
→ verify stored items exist
→ check original path collisions
→ restore all items
→ if partial failure: keep manifest recoverable
→ remove trash record only after successful restore
→ rescan History
```

Collision:

```text
A new source already exists at original path
→ refuse restore
→ show exact conflict
```

Never overwrite provider data.

## 13. Purge transaction

Purge accepts only a Trash key.

```text
Trash key
→ resolve canonical trash directory
→ prove directory is inside trash root
→ delete that trash entry
```

Actions:

```text
Delete Forever
Empty Trash
```

Both require explicit destructive confirmation.

Initial default:

> Shardlane never auto-purges History Trash.

A retention policy can be a later opt-in setting.

## 14. Catalog behavior for Trash

The catalog is disposable derived state.

When a source moves to Trash:

- remove/hide it from normal History results;
- preserve Trash metadata from the manifest, not from stale catalog rows;
- invalidate transcript pages/FTS for the live source;
- rescan after restore.

Do not index transcript content inside Trash by default.

## 15. Native Trash UI

History toolbar includes:

```text
Trash (N)
```

Trash page/mode:

```text
Trash
3 sessions
                                  [Empty Trash]

Claude · Fix auth
portal · deleted 2h ago · 1.8 MB
                           [Restore] [Delete Forever]
```

Requirements:

- virtualized/bounded list;
- restore result returns to normal History optionally highlighting restored session;
- permanent deletion requires confirmation;
- provider/path details available under disclosure for debugging;
- no transcript body required to render Trash.

## 16. Usage domain

Create `next/internal/usage` independent of MyGo.

### 16.1 Source/confidence

```go
type Source string

const (
    ProviderReported Source = "provider-reported"
    SessionDerived   Source = "session-derived"
    GatewayObserved  Source = "gateway-observed"
    Estimated        Source = "estimated"
)
```

This prevents partial session-derived numbers from pretending to be an exact provider bill.

### 16.2 Totals

```go
type Totals struct {
    Calls       int64
    Errors      int64
    Input       int64
    Output      int64
    CacheRead   int64
    CacheWrite  int64
    Reasoning   int64
    DurationMS  int64
    TTFTMS      *int64
    CostUSD     *float64
    Unpriced    int64
}
```

### 16.3 Observation

```go
type Observation struct {
    Time        int64
    Agent       history.AgentID
    SessionID   string
    ProjectKey  string
    Provider    string
    Model       string
    Requested   string
    Served      string
    Totals      Totals
    Source      Source
    Complete    bool
}
```

No prompt/response bodies.

## 17. Usage collectors

### Required baseline — provider/session adapters

Extend History/provider parsing so adapters may emit normalized usage observations when their source format contains authoritative token/model information.

Do not force every parser to provide values.

### Optional provider API / integration source

If a provider integration exposes quota/account usage safely, normalize through `QuotaService`.

### Future optional gateway source

If Shardlane later gains an optional gateway/proxy module, it may contribute `GatewayObserved` records through the same UsageService.

The 0.8 domain must not require this source.

## 18. Usage aggregation dimensions

Required summary dimensions:

```text
Agent
Live AgentKey/session
History Session
Project
Provider
Model
Time period
```

Initial periods:

```text
Today
7 days
30 days
All
```

Do not read/parse transcripts on every aggregate query. Usage observations belong in a derived Shardlane-owned catalog/index.

## 19. Usage storage

Prefer extending or siblinging the existing disposable SQLite catalog rather than creating an append-only file that UI must rescan.

Suggested tables:

```text
usage_observations
- source_key / stable observation key
- timestamp
- agent
- session_id
- project_key
- provider
- model
- requested_model
- served_model
- calls/errors
- input/output/cache_read/cache_write/reasoning
- duration_ms/ttft_ms
- cost_usd
- source
- complete
- source_identity

usage_session_summary
usage_project_summary (optional materialized/cache)
```

Source identity must make rescans idempotent.

## 20. Usage deduplication

A provider session may be visible through more than one telemetry path.

Never blindly add:

```text
session-derived usage + gateway-observed usage
```

when they describe the same call/session.

Define source precedence and dedupe keys before combining observations.

Initial safe rule:

- exact provider/gateway request ID match when available;
- exact Agent + session + provider/model + token checkpoint identity when adapter proves it;
- otherwise keep sources separate and mark totals partial instead of guessing dedupe.

## 21. Cost model

Cost is an estimate unless Shardlane has authoritative billing information.

Suggested price precedence:

```text
explicit Shardlane provider/model override
→ provider catalog tariff
→ known model catalog tariff
→ unknown/unpriced
```

Keep price source metadata internally.

UI copy:

```text
$0.21 est.
```

not:

```text
$0.21 charged
```

## 22. Quota / allowance service

Provider quota is separate from Usage totals.

```go
type QuotaWindow struct {
    Name       string
    UsedPct    *float64
    LeftPct    *float64
    ResetsAtMS *int64
    Balance    string
}

type Snapshot struct {
    Provider string
    Account  string
    Windows  []QuotaWindow
    AsOfMS   int64
    Stale    bool
    Error    string
}
```

Refresh policy:

```text
cached result first
background refresh when stale
explicit Refresh
post-auth/integration change
```

No render-time provider request.

No high-frequency polling.

## 23. Status Center integration

0.7 defines the compact usage seam. 0.8 becomes its authoritative service implementation.

```text
UsageService
→ AgentUsageSnapshot
→ StatusCenterSnapshot.Agent.Usage
```

Status Center uses a bounded compact projection only.

Example:

```text
Claude · Fix auth                         ⚠ Needs input
portal · Sonnet · 38k tok · $0.21 est.
```

Full detail belongs in Agent Inspector/Usage page.

## 24. Agent Inspector

Add a Native Agent detail surface.

Recommended route/intention:

```text
/agents/{agent-key}/inspect
```

If Router encoding an AgentKey becomes awkward, route through typed local navigation state rather than exposing unsafe runtime identifiers in URL strings.

### 24.1 Sections

```text
Agent
Model & Provider
Usage
Allowance
Integration
Session
Project
Actions
```

### 24.2 Agent

```text
provider mark
semantic title
RuntimePhase
Attention
Sendability
Unread / ReviewPending
last authoritative status change
```

### 24.3 Model & Provider

```text
provider
model
requested model when known
served model when known
integration strategy
```

Do not imply model routing when source metadata cannot prove it.

### 24.4 Usage

```text
session tokens
input/output/cache/reasoning
estimated cost
calls/errors when available
duration/TTFT when available
Usage source
Complete / partial
```

### 24.5 Allowance

Show provider quota window only if available.

Always show stale/as-of state truthfully.

### 24.6 Integration

Reuse 0.4 IntegrationHealthService:

```text
Current
ManagedByHerdr
Outdated
NotInstalled
Error
```

### 24.7 Session

```text
opaque/native session identity where safe
started/updated time
History session link
Conversation link
```

No raw provider source path as primary public identity.

### 24.8 Actions

State-safe actions only:

```text
Open Chat
Open Terminal
Open History
Mark reviewed
Refresh Usage
Refresh Integration
```

Do not add provider credential mutation into Agent Inspector.

## 25. Native Usage page

0.8 should include a focused Usage page rather than hiding every metric inside Agent cards.

Suggested top controls:

```text
[Today] [7d] [30d] [All]
Agent ▾  Project ▾  Provider ▾  Model ▾
```

Summary:

```text
Tokens
Estimated cost
Calls
Errors
```

Sections:

```text
By Agent
By Project
By Session
By Model
By Provider
```

A full request ledger is optional and should appear only when request-level observations actually exist.

Do not invent request rows from transcript turns.

## 26. History ↔ Usage interaction

History session row may show its own usage.

Clicking usage metadata may open Usage scoped to that session.

Usage session row may open History detail.

Both use typed Session/Conversation identity; do not join by title.

## 27. Provider / gateway reference boundary

Magpie's provider gateway is useful as an architectural reference for:

```text
ProviderIdentity
AccountIdentity
ModelIdentity
Usage attribution
Quota
Health/latency
Request identity
```

But Shardlane 0.8 does not implement:

```text
mandatory proxy
API protocol translation
routing groups
provider-key failover
caller-key billing limits
LAN gateway auth
```

Those may become a separate future product initiative after the client migration is complete.

## 28. Health and latency

Integration health and provider usage health should remain separate:

```text
IntegrationHealth
- installed/current/outdated/managed/error

ProviderHealth
- reachable/latency/rate-limited/quota/error
```

If provider health testing is added:

- explicit Refresh or bounded stale refresh;
- one small provider-supported test request only when product policy permits;
- never run from render;
- never let health probes block Agent launch unless the provider policy explicitly requires it.

## 29. Privacy

Default Usage storage must not persist:

```text
prompt bodies
assistant bodies
thinking
Tool inputs/outputs
credentials
API keys
auth headers
environment dumps
```

Allowed:

```text
stable safe IDs
provider/model names
numeric token/cost/latency fields
timestamps
status/error classes
source/confidence
```

## 30. Logging

Never log Trash manifest contents wholesale if they contain user paths unnecessarily.

Log:

```text
operation
SessionKey/TrashKey
provider
counts
bytes
durations
error class
```

Do not log transcript content.

## 31. Atomic task plan

### H0 — grouped History

| ID | Task | Verify |
|---|---|---|
| HMG-01 | HistoryGroup model/project grouping | normalized-path tests |
| HMG-02 | filter-before-group projection | search/provider fixtures |
| HMG-03 | project aggregate metadata | pure aggregate tests |
| HMG-04 | Native expandable project group row | headless UI |
| HMG-05 | grouped virtualized session list | large-list test |
| HMG-06 | preserve group/filter state across detail | Router/state test |

### H1 — management capability

| ID | Task | Verify |
|---|---|---|
| HMG-07 | SessionManagementCapability/provider interface | capability table tests |
| HMG-08 | source-set resolver first provider | fixture completeness test |
| HMG-09 | additional proven provider adapters | provider fixture tests |
| HMG-10 | live Agent/session guard | exact identity test |
| HMG-11 | recent-write guard | deterministic clock/fs test |
| HMG-12 | source identity revalidation | stale source test |

### H2 — Trash core

| ID | Task | Verify |
|---|---|---|
| HMG-13 | versioned TrashManifest | roundtrip tests |
| HMG-14 | trash-root path confinement | traversal/security tests |
| HMG-15 | delete/move transaction | temp source test |
| HMG-16 | partial-move rollback | injected failure test |
| HMG-17 | cross-volume strategy | platform/fake fs tests |
| HMG-18 | restore transaction | roundtrip test |
| HMG-19 | restore collision refusal | conflict test |
| HMG-20 | purge one | confinement test |
| HMG-21 | empty trash | multi-entry test |
| HMG-22 | catalog invalidation/rescan | integration test |

### H3 — Trash UI

| ID | Task | Verify |
|---|---|---|
| HMG-23 | Delete action only when capability allows | headless state test |
| HMG-24 | delete confirmation + active refusal | headless action test |
| HMG-25 | Trash view/list | render smoke |
| HMG-26 | Restore action | UI/service test |
| HMG-27 | Delete Forever confirmation | destructive-flow test |
| HMG-28 | Empty Trash confirmation | destructive-flow test |

### U0 — usage domain

| ID | Task | Verify |
|---|---|---|
| USG-01 | Source/Totals/Observation models | table tests |
| USG-02 | usage identity/dedupe-key model | collision tests |
| USG-03 | SQLite usage schema | migration/temp-db tests |
| USG-04 | idempotent observation upsert | rescan tests |
| USG-05 | aggregation period helpers | date/timezone tests |

### U1 — collectors

| ID | Task | Verify |
|---|---|---|
| USG-06 | Claude usage extraction | fixture parity |
| USG-07 | Codex usage extraction | fixture parity |
| USG-08 | additional provider usage adapters | fixture tests |
| USG-09 | source completeness/confidence | partial/complete tests |
| USG-10 | source dedupe/precedence | mixed-source fixtures |

### U2 — summaries/service

| ID | Task | Verify |
|---|---|---|
| USG-11 | UsageService Agent summary | aggregate tests |
| USG-12 | Session summary | aggregate tests |
| USG-13 | Project summary | aggregate tests |
| USG-14 | Provider/model summary | aggregate tests |
| USG-15 | period/filter query | query tests |
| USG-16 | live AgentKey → session usage join | typed identity tests |
| USG-17 | unknown never renders as zero | presentation-model test |

### U3 — pricing/quota

| ID | Task | Verify |
|---|---|---|
| USG-18 | PriceSource/effective-price model | precedence tests |
| USG-19 | estimated cost calculation | pricing fixtures |
| USG-20 | unpriced behavior | explicit unknown tests |
| USG-21 | QuotaSnapshot models | table tests |
| USG-22 | first provider quota adapter | fake/cache tests |
| USG-23 | stale/as-of cache behavior | deterministic time test |

### U4 — Native Usage UI

| ID | Task | Verify |
|---|---|---|
| USG-24 | Usage route/page | headless UI |
| USG-25 | period/filter controls | interaction tests |
| USG-26 | summary cards | render smoke |
| USG-27 | Agent/Project/Session groups | list tests |
| USG-28 | Provider/Model groups | list tests |
| USG-29 | History ↔ Usage typed navigation | routing tests |

### A0 — Agent Inspector

| ID | Task | Verify |
|---|---|---|
| INS-01 | AgentInspectorSnapshot composition | fixture test |
| INS-02 | model/provider section | unknown/known tests |
| INS-03 | usage section | partial/full tests |
| INS-04 | quota section | stale/error tests |
| INS-05 | integration section reuse | service composition test |
| INS-06 | session/history links | exact identity tests |
| INS-07 | Native Inspector surface | headless UI |
| INS-08 | safe action routing | status matrix |

### A1 — Status Center authoritative usage

| ID | Task | Verify |
|---|---|---|
| INS-09 | replace provisional 0.7 usage source with UsageService | projection test |
| INS-10 | compact token/cost formatter | golden tests |
| INS-11 | compact quota formatter | golden/stale tests |
| INS-12 | no usage-driven attention regression | operational test |

### C0 — closure

| ID | Task | Verify |
|---|---|---|
| HUI-01 | no History mutation from render/UI layer | static audit |
| HUI-02 | no arbitrary-path purge API | security audit |
| HUI-03 | no transcript content in Usage storage/logs | privacy audit |
| HUI-04 | History/Usage large-data performance | benchmark |
| HUI-05 | 0.8 real-app acceptance | focused acceptance |
| HUI-06 | 0.8 package gate | full tests/build |

## 32. Focused acceptance

1. History displays sessions grouped under normalized Projects.
2. Search only shows matching sessions/groups.
3. Provider/model/date filters compose correctly.
4. Project group shows accurate session count/latest activity.
5. Session usage shows known model/tokens without parsing during render.
6. Read-only provider sessions have no Delete action.
7. Trashable inactive session moves to Shardlane Trash and disappears from normal History.
8. Live Agent session cannot be deleted.
9. Recently written source cannot be deleted.
10. Partial move failure restores the original source set.
11. Restore returns all source items to original paths.
12. Restore refuses path collision.
13. Delete Forever removes only inside the Shardlane trash root.
14. Empty Trash requires confirmation.
15. No Trash action accepts an arbitrary user path from UI.
16. Usage Today/7d/30d/All aggregates correctly.
17. Agent/Project/Session totals agree with underlying observations.
18. Same session rescan does not duplicate usage.
19. Partial usage is labeled partial/unknown, not zero.
20. Unpriced model cost is unknown, not `$0.00`.
21. Provider quota displays reset/as-of/stale state truthfully.
22. Status Center Agent usage matches UsageService for the same live session.
23. Agent Inspector combines runtime, model, usage, quota and integration without issuing render-time IO.
24. No provider source is modified unless the explicit management transaction was invoked.
25. No prompt/transcript body enters Usage storage/logs.

## 33. Performance targets

- grouped History projection: target < 20 ms for 1,000 session metadata rows;
- warm History list query: preserve < 50 ms target;
- Usage summary warm query: target < 50 ms for normal local dataset;
- Status Center compact usage lookup: no synchronous DB query during render;
- Trash listing uses manifest metadata only;
- no transcript parse to render History group headers or Trash;
- quota refresh never blocks UI thread;
- catalog/source rescan remains background/cancellable.

## 34. Definition of Done

0.8 is complete when:

- [ ] History is grouped by normalized Project;
- [ ] project/session metadata includes useful usage where known;
- [ ] session management is capability-gated;
- [ ] Trash/Delete is reversible for supported providers;
- [ ] live/recent/stale-source safety guards exist;
- [ ] Restore and Purge are path-safe;
- [ ] Trash never auto-purges by default;
- [ ] Usage domain records source/confidence;
- [ ] Usage aggregation works by Agent/Session/Project/Provider/Model/period;
- [ ] usage rescans are idempotent/deduplicated conservatively;
- [ ] estimated/unpriced cost is truthful;
- [ ] quota is optional and staleness-aware;
- [ ] Native Usage page works;
- [ ] Agent Inspector works;
- [ ] 0.7 Status Center consumes the authoritative UsageService;
- [ ] Shardlane is not turned into a mandatory model gateway;
- [ ] provider credentials are not stored in usage records;
- [ ] no transcript bodies are stored in usage records;
- [ ] full tests/security checks/build pass.

## 35. 0.9 boundary — Workspace Intelligence & Desktop Experience

After 0.8, the next release is intentionally a larger desktop-workflow milestone:

```text
0.9 — Workspace Intelligence & Desktop Experience
Command Center
Files / Services / Lazygit / local Preview
Git workspace intelligence + optional read-only PR enrichment
Desktop attention / Terminal file-drop ergonomics
Diagnostics / bounded Logs
official MyGo updater / release notes
Sidebar density / high contrast
New Task Presets
```

Reference audit: `docs/reference-magpie-herdr-gpui-shardlane-0.9-audit.md`.
Plan: `docs/mygo-native-0.9.0-workspace-intelligence-desktop-experience-plan.md`.
Prompt: `docs/prompts/mygo-native-0.9.0-workspace-intelligence-desktop-experience-implementation-prompt.md`.

The provider/gateway ideas that remain intentionally deferred stay outside this release; 0.9 does not become a mandatory model gateway or Remote/Teleport program.
