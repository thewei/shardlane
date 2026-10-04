# Shardlane MyGo 0.7.0 Status Center Implementation Prompt

Continue the Shardlane MyGo migration after 0.6 Native Conversation & Chat is complete.

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
7. `docs/mygo-native-migration-roadmap.md`
8. `docs/mygo-native-0.5.0-agent-workbench-plan.md`
9. `docs/mygo-native-0.6.0-conversation-chat-plan.md`
10. `docs/reference-magpie-agent-history-usage-audit.md`
11. `docs/mygo-native-0.7.0-status-center-plan.md`
12. `next/CLAUDE.md`

Then inspect the old product semantics:

- `crates/herdr-gui/src/status_bar.rs`
- `crates/herdr-gui/src/agent_panel.rs`
- `crates/herdr-gui/src/header_view.rs`
- `crates/herdr-gui/src/status.rs`
- `crates/herdr-gui/src/agent_switcher.rs`
- `crates/herdr-gui/src/shell_navigation.rs`

Port product behavior, not objc2/GPUI mechanics.

## Mission

Implement **0.7 Status Center & Agent Quick Navigation**.

There must be one immutable `StatusCenterSnapshot` and one `StatusCenterAction` routing contract consumed by:

1. the full in-window right-top Status Center;
2. the macOS/system Tray floating Native quick panel;
3. an optional lightweight native right-click/fallback menu.

Do not create two independent status engines.

## Architecture invariants

- Herdr remains runtime authority.
- Use the 0.5 AgentDirectory/AgentCardModel and 0.6 Conversation/Interaction models.
- Status Center is presentation + navigation, not runtime ownership.
- No Agent status is inferred from terminal output.
- No new polling loop.
- No render-time RPC/filesystem/provider work.
- No global Herdr focus RPC for quick navigation.
- Navigation remains local Shardlane routing.
- Opening/visiting an Agent clears unread only; it does not clear ReviewPending.
- Agent Usage is a secondary dimension and must never drive operational attention priority.
- Missing/partial usage must remain unknown/partial, never be rendered as zero.
- The Tray quick panel must use MyGo Native UI, never WebView.

## P0 — one StatusCenterSnapshot

Implement UI-independent models for:

```text
Connection
Summary counts
Agent items
RecommendedDestination
Revision/fingerprint
```

Summary:

```text
NeedsAttention
ReviewPending
Working
Idle
Total
```

Agent item must preserve stable `(instance_id, terminal_id)` AgentKey and exact navigation locators.

No MyGo types in the model.

## P1 — one destination resolver

Compute `RecommendedDestination` once.

Priority:

```text
pending structured Interaction
→ exact Chat InteractionCard

NativeFallback / blocked / failed / NeedsTerminal
→ Terminal

ReviewPending
→ live Conversation if available, otherwise Agent Workbench

Working
→ live Conversation if semantic source exists, otherwise Agent/Terminal

Idle/other
→ Agent Workbench / owning Agent
```

Do not spread these rules across titlebar, tray and page event handlers.

## P2 — move status to right-side titlebar accessory

Replace the current minimal `titlebarActivity` implementation.

The final titlebar should have a dedicated right-side status trigger.

Display compact counts only for actionable state:

```text
⚠ N
✓ N
⚡ N
```

No permanent Connected badge.
Do not put raw token totals in the titlebar trigger by default; compact Usage belongs inside Agent rows/panels.
Disconnected/reconnecting takes priority over Agent counts.

Reuse Design System v2 and shared StatusGlyph components.

## P3 — Native in-window Status Center

Open from the right-side trigger.

Use the shared compact Agent row/card from 0.5, not a new visual implementation.

Filters:

```text
Attention
Review
Working
Idle
All
```

Initial filter:

```text
Attention if any
else Review if any
else Working if any
else All/Idle
```

Rows show:

- provider mark;
- Agent title;
- project;
- unread;
- Review chip;
- status glyph/text;
- compact model/tokens/estimated-cost/quota metadata when known;
- truthful Usage source/staleness semantics;
- destination affordance when useful.

Provide `Open Agents` to the full `/agents` Workbench.

## P4 — quick jump routing

On row click:

```text
capture exact AgentKey/destination
→ dismiss status panel
→ activate/show owning window
→ confirm Agent still exists
→ refresh current local locators if needed
→ local navigation to destination
→ clear unread
→ keep ReviewPending
```

If Agent vanished:

```text
no mutation
refresh snapshot
Toast: Agent is no longer running
```

Never resolve by display label alone.

## P5 — structured interaction quick jump

A pending Question/Permission/PlanApproval is a higher-specificity destination than generic blocked status.

Click must open the Agent's live Conversation and focus/scroll to the exact InteractionCard.

If interaction state is NativeFallback, route to Terminal.

No generic prompt or guessed keys.

## P6 — review flow

Inline `Mark reviewed` uses the 0.5 marker service.

It must update:

```text
Agent Workbench
Sidebar
Header Status Center
System Tray
```

from one marker change.

Do not clear review on ordinary navigation.

## P7 — Menu Bar / Tray floating Native quick panel

Use MyGo `NewTray`, `Tray.Bounds`, Native `Window`, and dynamic `Menu` APIs. Do not port the old objc2 status bar controller and do not add a WebView quick panel.

On macOS, normal tray/menu-bar click toggles a dedicated Native floating window:

```text
Frameless
AlwaysOnTop
DisableResize
Hidden until first click
Transparent/Vibrancy where supported
positioned from Tray.Bounds
hide on Window.OnBlur
hide on Escape
```

Lifecycle:

```text
tray click
→ visible? hide : compute screen-safe bounds + show/focus
```

The panel reuses the same `StatusCenterSnapshot`, compact Agent rows and `StatusCenterAction` router as the in-window Status Center.

macOS menu-bar title remains status-first:

```text
⚠ N  ✓ N  ⚡ N
```

Do not append token totals to the menu-bar title by default.

An optional right-click/fallback native menu may expose:

```text
Open Status Center
Open Agents
New Agent
History
Search
Settings
Show Shardlane
Quit
```

Tray/panel failure or unsupported placement must not break the in-window Status Center.

## P8 — lightweight Agent Usage projection

Add the shared seam required by the Status Center:

```text
AgentUsageSnapshot
UsageSource
QuotaSnapshot (optional)
```

0.7 needs only compact per-live-Agent facts:

```text
model
tokens when known
estimated cost when known
one relevant quota window when authoritative
source/completeness/as-of
```

Use already available History/session/provider facts or service caches. Do not build a full usage ledger inside 0.7; 0.8 owns the authoritative UsageService.

Rules:

- unknown != zero;
- estimated cost is labeled estimated;
- stale quota carries as-of/stale state;
- Usage never changes NeedsAttention/Review/Working priority;
- no Usage IO from render or panel-open code.

Reference: `docs/reference-magpie-agent-history-usage-audit.md`.

## P9 — update only on semantic change

Derive StatusCenterSnapshot after existing projection/marker/interaction reconciliation.

Compare semantic snapshot/fingerprint.

Only update:

- right-top trigger/panel state;
- tray title/tooltip/menu;

when relevant data changed.

Do not rebuild the tray menu every render/frame.

No polling for status.

## P10 — cross-window/multi-instance readiness

Stable Agent identity is `(instance_id, terminal_id)`.

Support a window registry routing seam so future/multiple observed windows can contribute Agent rows.

Do not start watchers for inactive Herdr instances only to populate Status Center.

Same terminal ID across instances must never collide.

## P11 — keyboard/accessibility

- Status trigger focusable;
- Enter/Space opens;
- Escape closes;
- accessible Agent row label includes title/project/status;
- Mark reviewed is separately focusable/labeled;
- no status is represented only by color;
- add Cmd/Ctrl+Shift+A only if no existing shortcut conflict.

Ctrl-Tab remains the Agent MRU switcher and must not be changed.

## Atomic tasks

Use `STC-01` through `STC-55` from `docs/mygo-native-0.7.0-status-center-plan.md`.

Each task must have one objective and focused test.

Do not run a full build after every task.

Per task:

```text
focused go test
git diff --check
```

At package gate:

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go test -race ./internal/agent/... ./internal/conversation/... ./internal/app/... ./internal/nativeui/...
GOTOOLCHAIN=go1.27.1 go tool mygo build
cd ..
git diff --check
```

## Required acceptance proof

Report DONE / PARTIAL / NOT DONE for:

- StatusCenterSnapshot;
- right-side titlebar status trigger;
- Attention/Review/Working counts;
- Native Status Center panel;
- filters;
- shared Agent card reuse;
- pending Interaction quick jump;
- blocked → Terminal jump;
- Review → Conversation/Agent jump;
- Working → Conversation jump;
- unread semantics;
- Mark reviewed;
- disconnected/reconnect status;
- MyGo Tray;
- menu-bar left-click Native floating quick panel;
- Tray.Bounds/screen-safe panel positioning;
- hide-on-blur/Escape behavior;
- optional right-click/fallback native menu;
- compact Agent Usage/model/token/cost/quota display;
- unknown/partial/stale Usage truthfulness;
- snapshot/no-op update optimization;
- cross-instance AgentKey routing;
- keyboard/accessibility;
- real-app acceptance.

Also prove:

- no global Herdr focus RPC navigation was introduced;
- no status polling was introduced;
- no render-time IO was introduced;
- no duplicated status parser/model was introduced;
- no WebView/React/xterm was introduced, including the tray quick panel;
- Usage does not influence Agent attention priority;
- missing Usage is never rendered as zero;
- Agent review is never cleared by navigation alone;
- full tests/race tests/build pass.

Proceed without asking for confirmation.
