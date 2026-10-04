# Shardlane MyGo 0.8.0 History, Usage & Agent Inspector Implementation Prompt

Continue the Shardlane MyGo migration after 0.7 Status Center is complete.

Repository:

`/Users/wilson/Workspaces/wh-studio/herdr-client`

Use the existing Devspace workspace and branch `rewrite/mygo`.
Preserve all user/other-agent changes. Do not reset, stage, commit or push unless explicitly requested.

## Read first

1. `AGENTS.md`
2. `CLAUDE.md`
3. `.agents/skills/herdr-client-development/SKILL.md`
4. `docs/client-product-architecture.md`
5. `docs/performance-engineering.md`
6. `docs/mygo-native-execution-rules.md`
7. `docs/reference-magpie-agent-history-usage-audit.md`
8. `docs/mygo-native-0.7.0-status-center-plan.md`
9. `docs/mygo-native-0.8.0-history-usage-agent-inspector-plan.md`
10. `next/CLAUDE.md`

Also inspect the current `internal/history`, `internal/agent`, IntegrationHealth and Status Center implementation before changing contracts.

## Mission

Implement **0.8 History Management, Usage & Agent Inspector**.

Borrow useful product patterns from Magpie, but preserve Shardlane's canonical architecture.

Herdr remains runtime authority. Shardlane must not become a mandatory model gateway.

## P0 — grouped History

Use existing normalized `project_key` and catalog index.

Pipeline:

```text
query/filter
→ session sort
→ group by project_key
→ Native expandable groups
```

Project header:

```text
name
path
session count
latest activity
aggregate tokens/cost when known
```

Session row:

```text
provider/title/time
short description
model · tokens · estimated cost · messages
```

Search/filter before grouping.

Do not parse transcripts from render or merely to build group headers.

## P1 — explicit History management capability

Current History is read-only. Do not globally make it writable.

Add provider-specific capability:

```text
ReadOnly
Trashable
Restorable
Purgeable
```

Delete is visible only when the adapter can enumerate the session's complete source set and restore it safely.

Database-backed/unknown providers remain read-only.

## P2 — Trash transaction

Use Shardlane-owned versioned Trash manifests under app user data.

Delete transaction must:

```text
resolve SessionKey
→ verify management capability
→ reject exact live Agent/session
→ revalidate indexed source identity
→ enumerate complete source set
→ reject recently-written source
→ move every item to Trash
→ persist manifest as progress is made
→ rollback partial failure
→ invalidate/rescan derived catalog
```

Never accept arbitrary source paths from Native UI.

Initial recent-write guard target: 60 seconds.

Do not overwrite new provider data on Restore.

## P3 — Trash UI

Add History `Trash (N)` mode.

Each trash row:

```text
Title
Provider
Project
Deleted time
Size
Restore
Delete Forever
```

Top action:

```text
Empty Trash
```

Delete Forever and Empty Trash require explicit confirmation.

No automatic trash purge in 0.8.

## P4 — Usage domain

Create `internal/usage` independent of MyGo.

Every normalized observation records its source/confidence:

```text
ProviderReported
SessionDerived
GatewayObserved
Estimated
```

Totals may include:

```text
calls/errors
input/output
cache read/write
reasoning
duration/TTFT
cost estimate
```

Missing fields remain unknown.

Never turn unknown into zero.

## P5 — usage collectors

Start with session/provider formats Shardlane already parses.

Claude and Codex are required first.

Extend additional providers task-by-task with fixture parity.

Do not create a gateway just to get usage.

If a future optional gateway source exists, it feeds the same Usage domain as another source.

## P6 — dedupe/accounting safety

Do not double count the same model call observed through multiple sources.

Use exact IDs/checkpoints when available.

If dedupe cannot be proven, keep sources separate/partial instead of guessing.

Rescanning the same provider source must be idempotent.

## P7 — usage aggregation

Required dimensions:

```text
Agent
live Agent/session
History Session
Project
Provider
Model
Today / 7d / 30d / All
```

Use a Shardlane-owned SQLite index/cache, not render-time transcript scans.

## P8 — cost

Cost is estimated unless an authoritative billing source exists.

Display:

```text
$0.21 est.
```

Never claim charged/billed exactness from a catalog estimate.

An unknown/unpriced cost is `—`, not `$0.00`.

## P9 — quota

Add optional provider `QuotaService` capability.

Quota windows may expose:

```text
used/left percentage
reset time
balance
account safe display identity
as-of/stale/error
```

Cache and refresh in the application service.

No provider call from render.

No high-frequency polling.

## P10 — Native Usage page

Add Usage page with:

```text
[Today] [7d] [30d] [All]
Agent / Project / Provider / Model filters
```

Summary:

```text
Tokens
Estimated cost
Calls
Errors
```

Groups:

```text
By Agent
By Project
By Session
By Provider
By Model
```

Do not invent a per-request ledger unless request-level observations actually exist.

## P11 — Agent Inspector

Create one Native Agent Inspector composing:

```text
runtime phase
attention/sendability
unread/review
provider/model
integration health
usage
quota
session identity
project
History/Conversation links
```

Actions:

```text
Open Chat
Open Terminal
Open History
Mark reviewed
Refresh Usage
Refresh Integration
```

No credential management inside Agent Inspector.

## P12 — Status Center usage closure

Replace any provisional 0.7 usage lookup with the authoritative UsageService.

Status Center remains compact:

```text
portal · Sonnet · 38k tok · $0.21 est.
```

or quota:

```text
5h window 38% · resets 2h 14m
```

Usage never changes Agent attention priority.

## P13 — gateway reference boundary

Do not implement these as part of 0.8:

```text
mandatory model proxy
protocol translation
provider failover/routing groups
provider credential vault required for Agents
client gateway-key billing limits
```

Use Magpie only as a reference for provider/model/account/usage/quota/health separation.

## P14 — privacy/security

Usage storage/logs must never contain:

```text
prompt body
assistant body
thinking
tool input/output
API key
auth header
environment dump
```

Trash/Purge paths must be canonical and confined to the Shardlane trash root.

Provider source mutation happens only through the explicit HistoryManagementService transaction.

## Atomic tasks

Use the IDs in `docs/mygo-native-0.8.0-history-usage-agent-inspector-plan.md`:

```text
HMG-01..28  grouped History + Trash
USG-01..29  Usage / pricing / quota / Native page
INS-01..12  Agent Inspector + Status Center closure
HUI-01..06  security/performance/package closure
```

Each task must have focused verification and normally fit 30 minutes–4 hours.

Per task:

```text
focused go test
git diff --check
```

At package gate:

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go test -race ./internal/history/... ./internal/usage/... ./internal/agent/... ./internal/app/... ./internal/nativeui/...
GOTOOLCHAIN=go1.27.1 go tool mygo build
cd ..
git diff --check
```

## Required final audit

Report DONE / PARTIAL / NOT DONE for:

- project-grouped History;
- grouped search/filter behavior;
- management capability gating;
- live/recent delete guards;
- Trash;
- Restore;
- Delete Forever;
- Empty Trash;
- Usage domain;
- Claude usage;
- Codex usage;
- usage dedupe/idempotency;
- Agent summary;
- Session summary;
- Project summary;
- Provider/Model summary;
- cost estimate;
- quota capability;
- Native Usage page;
- Agent Inspector;
- Status Center authoritative usage;
- gateway/proxy work (expected deferred).

Also prove:

- no unsupported provider can be deleted;
- no live Agent session can be deleted;
- no restore overwrites existing provider data;
- no purge can escape Shardlane trash root;
- no Usage body/content logging exists;
- no mandatory model gateway was introduced;
- no global Herdr focus/runtime authority regression exists;
- full tests/race tests/build pass.

Proceed without asking for confirmation.
