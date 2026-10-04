# Reference Audit — yetone/magpie for Shardlane

Status: **architecture/reference audit**
Audit date: 2026-10-05
Reference repository: `https://github.com/yetone/magpie`
Reference commit inspected: `b96d55f` (2026-10-05 10:08 +08:00)

## 1. Why this project is relevant

Magpie is not the same product as Shardlane, but it overlaps with several product surfaces we need:

- macOS menu-bar quick access;
- Agent/provider/model inventory;
- session/history browsing;
- session usage/accounting;
- provider quota/allowance health;
- provider/gateway health and routing diagnostics;
- session deletion/recovery;
- project/folder grouping;
- per-session continuation/resume.

The useful reference is not “copy Magpie's gateway.” The useful reference is its separation of:

```text
session identity
usage attribution
provider/model attribution
quota/health
history management
quick-panel presentation
```

Shardlane must preserve its own runtime architecture:

> Herdr remains the sole runtime authority. Shardlane may borrow Magpie's management, accounting and presentation patterns, but must not become a second Agent runtime or silently insert a mandatory model gateway.

## 2. Source areas inspected

Key Magpie sources reviewed:

```text
internal/gui/app.go
internal/gui/traymenu.go
internal/gui/trayusage.go
internal/gui/trayusage_tray.go
internal/gui/sessions.go
internal/gui/sessions_manage.go
internal/gui/assets/sessions.js
internal/gui/assets/sessions.css

internal/sessions/manage.go
internal/sessions/stats.go
internal/sessions/transcript.go

internal/usage/usage.go
internal/usage/ledger.go
internal/usage/direct.go

internal/gateway/*
internal/provider/*
```

## 3. Menu-bar quick panel findings

Magpie distinguishes between:

1. a system tray/menu-bar item;
2. a compact floating panel shown under the tray icon;
3. a persistent main application window.

The quick panel is not merely an `NSMenu`.

Its desktop implementation uses a dedicated frameless floating window with behavior equivalent to:

```text
frameless
always-on-top
non-resizable
hide on Escape
hide on focus loss
translucent background
rounded macOS panel
positioned below the tray icon
```

The panel size is fitted to content and repositioned relative to the tray.

### Shardlane adoption

Adopt the interaction model, not the WebView implementation.

MyGo 0.2.7 already provides the primitives Shardlane needs:

```text
NewTray
Tray.Bounds
WindowOptions.Frameless
WindowOptions.AlwaysOnTop
WindowOptions.SkipTaskbar
WindowOptions.Transparent
WindowOptions.Vibrancy
Window.OnBlur
Window.SetBounds / SetPosition
```

Therefore Shardlane should build its Status Quick Panel as a **MyGo Native UI window**, positioned from `Tray.Bounds()`.

Do not add a WebView quick panel.

## 4. History / Sessions findings

Magpie's Sessions management UI has several useful product behaviors.

### 4.1 Grouping by project/folder

Sessions are grouped by their working directory.

The group row shows:

- folder/project name;
- path;
- session count;
- disclosure state.

The latest session's folder is opened initially.

Filtering applies before grouping, so a search only shows groups containing matching sessions.

### Shardlane adoption

Shardlane already has:

```text
SessionMeta.ProjectPath
SessionMeta.ProjectName
project_key
sessions_project_key index
```

Use the canonical Shardlane `project_key` as grouping identity, not raw path string equality.

Suggested hierarchy:

```text
History
├─ portal
│  ├─ session A
│  ├─ session B
│  └─ session C
├─ herdr
│  └─ session D
└─ No Project
   └─ session E
```

The UI may display the path as secondary text but must group by normalized project identity.

## 5. Session management / Trash findings

Magpie's delete behavior is intentionally reversible.

### 5.1 Delete means “move to app trash”

A deleted session's full source set is moved to an application-owned trash directory.

A manifest records:

- Agent/provider;
- session ID;
- title;
- project/CWD;
- original paths;
- names in trash;
- deletion time;
- size.

### 5.2 Active-session guard

Magpie refuses deletion when a session file has been written recently, because the Agent may still be writing it.

### 5.3 Restore

Restore moves every recorded item back to its original path.

### 5.4 Purge

Permanent deletion is explicit:

```text
Delete forever
Empty trash
```

Trash is not automatically erased by default.

### 5.5 Capability gating

Magpie only allows deletion for providers whose complete session source set it understands.

Database-backed or otherwise unsafe providers stay read-only.

### Shardlane architecture consequence

Shardlane's current History package has a deliberate contract:

> provider-owned history is read-only.

Do not silently weaken that contract globally.

Instead introduce an explicit management capability:

```go
type SessionManagementCapability string

const (
    SessionReadOnly   SessionManagementCapability = "read-only"
    SessionTrashable  SessionManagementCapability = "trashable"
    SessionRestorable SessionManagementCapability = "restorable"
    SessionPurgeable  SessionManagementCapability = "purgeable"
)
```

A provider adapter may expose Delete only if it can enumerate **all files/directories belonging to that session** and safely restore them.

Unknown/incomplete providers remain read-only.

## 6. Proposed Shardlane Trash model

Use a Shardlane-owned location under user data:

```text
<trash-root>/history/<provider>/<trash-key>/
├─ manifest.json
└─ files/
```

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
    DeletedAt     int64
    TotalBytes    int64
    SourceItems   []TrashSourceItem
}

type TrashSourceItem struct {
    OriginalPath string
    StoredName   string
    Kind         string // file | directory
    SizeBytes    int64
}
```

### Delete transaction

```text
resolve exact session
→ verify provider capability
→ reject live/running Agent match
→ verify source identity still matches indexed source
→ enumerate complete source set
→ recent-write safety check
→ create manifest/trash directory
→ move source items
→ if partial failure: rollback moved items
→ update History catalog
→ publish History changed event
```

### Restore transaction

```text
load manifest
→ verify all trash items exist
→ detect original-path collisions
→ restore atomically as far as platform allows
→ on partial failure: preserve recoverability
→ rescan History
```

Do not overwrite a new provider session that appeared at the same original path.

### Purge

Permanent deletion only acts inside the Shardlane-owned trash root.

Never let a purge request accept arbitrary filesystem paths.

## 7. Usage model findings

Magpie has a strong accounting separation worth borrowing.

One usage record can distinguish:

- Agent;
- Session;
- Provider;
- Model requested;
- Model served;
- Input tokens;
- Output tokens;
- Cache reads;
- Cache writes;
- Reasoning tokens;
- duration;
- TTFT;
- first text latency;
- status/error;
- request ID;
- provider account/key identity;
- caller/client key identity;
- estimated cost.

Summaries are then grouped by:

```text
Agent
Model
Provider
Provider account/key
Caller key
Session
Time period
```

Session stats additionally group by project/folder and day.

## 8. What Shardlane should adopt from Usage

Shardlane should add a provider-neutral Usage domain, but it must record **source/confidence** because we do not necessarily proxy every model request.

Suggested model:

```go
type UsageSource string

const (
    UsageProviderReported UsageSource = "provider-reported"
    UsageSessionDerived   UsageSource = "session-derived"
    UsageGatewayObserved  UsageSource = "gateway-observed"
    UsageEstimated        UsageSource = "estimated"
)

type UsageTotals struct {
    Calls       int64
    Errors      int64
    Input       int64
    Output      int64
    CacheRead   int64
    CacheWrite  int64
    Reasoning   int64
    CostUSD     *float64
    DurationMS  int64
    TTFTMS      *int64
}

type SessionUsage struct {
    Agent       history.AgentID
    NativeID    string
    ProjectKey  string
    Provider    string
    Model       string
    Totals      UsageTotals
    Source      UsageSource
    Complete    bool
    UpdatedAt   int64
}
```

### Initial source priority

For Shardlane:

```text
provider/session reported usage
→ session-derived usage
→ optional gateway-observed usage
→ estimated only when explicitly marked
```

Never display missing usage as zero.

## 9. Agent usage display

Usage is a **secondary Agent dimension**, not the runtime status authority.

Agent state remains:

```text
RuntimePhase
Attention
Unread / ReviewPending
Sendability
IntegrationHealth
```

Usage is added alongside it:

```text
AgentUsageSnapshot
```

Suggested compact data:

```go
type AgentUsageSnapshot struct {
    AgentKey       agent.AgentKey
    SessionID      string
    Model          string
    Provider       string
    Tokens         int64
    Input          int64
    Output         int64
    CacheRead      int64
    CostUSD        *float64
    Quota          *QuotaSnapshot
    Source         UsageSource
    Complete       bool
    UpdatedAt      int64
}
```

### Status Center row example

```text
Claude · Fix auth                         ⚠ Needs input
portal · claude-sonnet · 38k tok · $0.21
                                            Terminal →
```

Or when subscription quota is authoritative:

```text
Claude · Fix auth                         ⚡ Working
portal · Sonnet · 38% 5h window · resets 2h 14m
                                                Chat →
```

Status priority is never derived from cost/token usage.

## 10. Quota / allowance findings

Magpie models subscription quota windows separately from usage ledger totals.

Useful fields include:

- provider/account identity;
- percentage used/left;
- named window;
- reset time;
- balance;
- stale/as-of metadata;
- error.

### Shardlane adoption

Add an optional provider capability:

```go
type QuotaWindow struct {
    Name       string
    UsedPct    *float64
    LeftPct    *float64
    ResetsAt   *int64
    Balance    string
}

type QuotaSnapshot struct {
    Provider   string
    Account    string
    Windows    []QuotaWindow
    AsOf       int64
    Stale      bool
    Error      string
}
```

Do not make quota polling part of Agent render.

Refresh through an application service with cache/staleness policy.

## 11. Gateway architecture findings

Magpie is intentionally a model gateway:

```text
Agent
→ Magpie gateway
→ provider/account/key/routing group
→ model
```

It therefore has authoritative per-request accounting and can:

- route/fail over providers;
- enforce gateway-key limits;
- attribute provider keys/accounts;
- measure request latency;
- translate compatible APIs;
- record exact request-level usage.

### What Shardlane should NOT adopt now

Do not make Shardlane's desktop client a mandatory proxy for all Agent model traffic.

Do not:

- move vendor credentials into Shardlane as a prerequisite for running Agents;
- make Agent startup depend on a Shardlane gateway;
- override Herdr/provider runtime ownership;
- implement model routing groups inside the UI layer;
- claim exact request cost when we only have partial session-derived usage.

### What to borrow architecturally

Borrow the boundaries:

```text
ProviderIdentity
AccountIdentity
ModelIdentity
UsageLedger
QuotaService
HealthService
Attribution
```

These can later support an optional Gateway/Proxy module without coupling the core desktop app to it.

## 12. Provider health / Agent Inspector

Magpie's provider/gateway model suggests a richer Agent Inspector for Shardlane.

The Inspector should combine independent facts:

```text
Agent runtime
Conversation/session identity
Provider/model
Integration health
Usage
Quota
Last activity
Project
Terminal/Chat destinations
History identity
```

Suggested sections:

```text
Agent
  Claude Code · Working
  Portal / main

Model
  Anthropic / Claude Sonnet

Usage
  38k tokens
  28k input · 10k output
  12k cache read
  $0.21 estimated
  Session-derived

Allowance
  5-hour window: 38% used
  resets in 2h 14m

Integration
  Managed by Herdr · Current

Session
  Native session id …
  Started …
  Last activity …

Actions
  Open Chat
  Open Terminal
  Open History
  Mark reviewed
```

Not every provider will supply every field.

Unknown must render as unknown, not zero/healthy.

## 13. History UI findings to adopt

### Top-level grouping

Primary grouping:

```text
Project
→ Sessions
```

Secondary filters:

```text
Provider
Model
Date
Status/read state if useful
Search
```

### Session row

Useful compact metadata:

```text
Title
Agent/provider icon
updated time
message count
model
tokens
estimated cost when known
branch
short description
```

### Project header

Useful fields:

```text
Project name
path
session count
aggregate tokens/cost when available
latest activity
```

### Trash

A dedicated Trash mode/view:

```text
Trash (N)
```

Each item:

```text
Title
Agent
Project
Deleted time
Size
[Restore]
[Delete Forever]
```

Top action:

```text
Empty Trash
```

## 14. Privacy and accounting rules

Do not copy Magpie's ability to log bodies unless Shardlane explicitly creates such a feature later.

Default Shardlane Usage records must not persist:

- prompts;
- assistant text;
- thinking;
- tool inputs/outputs;
- credentials;
- auth headers.

Usage should be numeric/identity metadata only.

Provider keys/accounts must use safe stable IDs/display names and must never persist raw credential values in usage records.

## 15. Adopt / adapt / reject matrix

| Magpie concept | Shardlane decision | Reason |
|---|---|---|
| Tray click → floating quick panel | **ADOPT** | strong macOS workflow; implement Native MyGo window |
| Session grouping by CWD | **ADAPT** | use Shardlane normalized project_key |
| Search-before-grouping | **ADOPT** | predictable History behavior |
| Trash + restore | **ADOPT with capability gate** | safe only when complete source set is known |
| Delete active-session guard | **ADOPT** | prevents provider-store corruption |
| Explicit Delete Forever / Empty Trash | **ADOPT** | clear destructive boundary |
| Automatic trash expiration | **REJECT initially** | safer default is no auto purge |
| Session tokens/cost/model metadata | **ADOPT** | improves History and Agent inspection |
| Usage ledger dimensions | **ADOPT domain model** | useful for reporting/inspection |
| Provider quotas/allowances | **ADAPT optional capability** | provider support varies |
| Provider health/test latency | **ADAPT** | merge with IntegrationHealth, do not poll in render |
| Mandatory model gateway | **REJECT for core** | conflicts with current architecture/product scope |
| Provider credential ownership in desktop gateway | **DEFER** | separate future product decision |
| Routing groups/model failover | **DEFER** | not required for Herdr client migration |
| Request/response body capture | **REJECT by default** | privacy and product scope |
| Gateway caller-key limits | **REFERENCE ONLY** | useful later for Remote/Team gateway, not Agent runtime |
| Per-request exact usage when gateway-observed | **OPTIONAL FUTURE** | only authoritative when Shardlane actually observes request |

## 16. Roadmap impact

### 0.7 — Status Center & Menu-Bar Quick Panel

Add:

- macOS tray left-click floating Native panel;
- right-click/lightweight native tray menu if useful;
- Agent usage summary in StatusCenterSnapshot;
- compact per-Agent usage/quota line;
- exact quick navigation;
- no WebView;
- no Usage-driven Agent status priority.

### 0.8 — History Management, Usage & Agent Inspector

Add:

- project-grouped History;
- Search/filter/group UI;
- Session management capabilities;
- Trash/Restore/Purge;
- active-session deletion guard;
- UsageService;
- session/project/Agent usage summaries;
- optional quota capability;
- Agent Inspector;
- provider/model/health attribution;
- Usage and History integration.

### 0.9+

Desktop Tools and broader compatibility follow afterward.

## 17. Reference rule

Magpie is a reference implementation, not a new architectural authority.

When Magpie behavior conflicts with:

```text
docs/client-product-architecture.md
Herdr runtime authority
Shardlane local-navigation semantics
privacy/logging rules
provider-source safety
```

Shardlane's architecture wins.
