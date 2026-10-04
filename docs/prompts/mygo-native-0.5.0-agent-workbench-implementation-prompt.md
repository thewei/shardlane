# Shardlane MyGo 0.5.0 Agent Workbench Implementation Prompt

Continue the Shardlane MyGo migration after **0.4 Operational Closure & UI System** is complete.

Repository:

`/Users/wilson/Workspaces/wh-studio/herdr-client`

Use the existing Devspace workspace and branch `rewrite/mygo`.
Do not reset, stage, commit, or push unless explicitly requested.

## Read before implementation

1. `AGENTS.md`
2. `CLAUDE.md`
3. `.agents/skills/herdr-client-development/SKILL.md`
4. `docs/client-product-architecture.md`
5. `docs/performance-engineering.md`
6. `docs/mygo-native-execution-rules.md`
7. `docs/mygo-native-migration-roadmap.md`
8. `docs/mygo-native-0.4.0-closure-plan.md`
9. `docs/mygo-native-0.5.0-agent-workbench-plan.md`
10. `next/CLAUDE.md`

Then inspect the original Rust behavior authorities named in the 0.5 plan, especially:

- `crates/herdr-gui/src/status.rs`
- `crates/shardlane-host/src/attention.rs`
- `crates/herdr-gui/src/agent_panel.rs`
- `crates/herdr-gui/src/agent_switcher.rs`
- `crates/herdr-gui/src/sidebar/pane_rows.rs`
- `crates/herdr-gui/src/status_bar.rs`
- `crates/herdr-gui/src/shell_navigation.rs`
- `crates/shardlane-host/src/conversation_queue.rs`
- `crates/shardlane-host/src/agent_service.rs`
- `crates/shardlane-host/src/history_continuation.rs`
- `crates/shardlane-host/src/live_handoff.rs`
- `crates/herdr-gui/src/agent_ui/activity.rs`

Port product semantics, not GPUI mechanics.

## Mission

Implement **0.5 Agent Workbench & Lifecycle**.

Do not start full Chat in this version.

The required end state is:

```text
Herdr Agent projection
→ one Go Agent lifecycle projection
→ client unread/review policy
→ one AgentCardModel
→ Sidebar + Header Overview + /agents + MRU switcher
→ safe Agent actions / History Continue
→ authoritative reconciliation
```

## Non-negotiable architecture

- Herdr remains the sole runtime authority.
- MyGo owns presentation only.
- Project/Tab/Pane/Agent navigation is client-local.
- Never reintroduce `workspace.focus`, `tab.focus`, or `pane.focus` for normal navigation.
- Do not create a client PTY, second Agent runtime, or second Pane layout authority.
- Do not infer Agent state/tool activity from terminal output.
- Do not use generic prompt text or guessed keys to resolve blocked interactions.
- History/Chat remain Native UI; no WebView.
- Integration/Hook health is not runtime Agent status.

## P0 — model Agent state correctly

Implement separate dimensions:

```text
RuntimePhase
Operational Attention
Unread / ReviewPending
Sendability
IntegrationHealth
```

Do not collapse them into one enum.

Use stable key:

```text
(instance_id, terminal_id)
```

Pane/Tab/Project IDs are navigation locators, not primary Agent identity.

Port the original sendability classifier exactly:

```text
working/pending/launch_pending → MidTurn
blocked/failed                 → NeedsTerminal
idle/done                      → Sendable
unknown                        → fail closed
```

## P1 — unread/review lifecycle

Implement the original marker behavior:

- no marker from initial projection;
- attention transition off-screen → unread;
- `done` → review-pending;
- visiting Agent clears unread only;
- explicit Mark reviewed clears review + unread;
- `done → working` clears review;
- Agent release removes markers.

One transition classifier must drive all consumers.

## P2 — AgentCardModel and shared components

Build one immutable UI-independent `AgentCardModel`.

Create shared Native card variants:

- compact row;
- standard Agent card;
- switcher row.

Use the 0.4 Design System and ProviderBadge/StatusGlyph primitives.

Card state actions:

- Working/Launching → Open Agent;
- Blocked/Failed → Open Terminal;
- Done + ReviewPending → Open Agent + Mark reviewed;
- Idle/Reviewed Done → Open Agent;
- CreatedNeedsAttention → open committed target + exact failure detail; no blind retry.

Do not add a fake quick-prompt field in 0.5.

## P3 — Agent Directory and Header Overview

Port the original useful AgentDirectory concept as presentation state only.

- merge reconciled live Agent cards;
- clean vanished entries;
- no watcher for inactive sessions solely for overview;
- actionable sorting: NeedsAttention → ReadyForReview → Working → Idle, unread first;
- Header counts: needs attention / review pending / working;
- no token usage in primary status chrome.

Header filters:

```text
Working
Needs attention
Review
Idle
All
```

Default: Working.

Rows can jump to their Agent and Mark reviewed.

## P4 — Native `/agents` Workbench

Add a real virtualized Native Agents page.

Use shared AgentCardModel and official MyGo list primitives.

Filters:

```text
All / Attention / Review / Working / Idle
```

No IO/RPC from render.

## P5 — Sidebar Agent rows

Replace raw string status rendering with shared compact Agent rows.

- provider mark;
- title fallback;
- unread;
- review chip;
- shared status glyph;
- local navigation.

No duplicate status parser.

## P6 — MRU Agent switcher

Restore original behavior:

- Ctrl-Tab opens/cycles;
- current observed instance only;
- max 10 MRU live Agents;
- release Ctrl commits;
- Esc cancels and restores previous selection/focus;
- reverse cycling supported;
- mouse hover/click supported;
- vanished Agent cancels safely;
- commit uses local navigation only.

## P7 — notifications

Port only the original meaningful transition set:

```text
working → done    Finished / ready for review
blocked → done    Finished / ready for review
working → blocked Needs attention
working → idle    Ready
```

No initial-projection or same-status notifications.
Obey notification settings and inactive-window gating.
Notification click navigates to Agent locally.

If the official platform/framework notification path is unavailable, keep in-app markers fully functional and document OS notification delivery as deferred instead of inventing a private platform framework.

## P8 — History Continue / Resume

Implement the exact original planner order:

```text
AlreadyLive
→ NativeResume
→ ContextTransfer
→ NeedsProjectSelection
→ Unsupported
```

AlreadyLive:
- exact typed identity match;
- no new Agent;
- jump to existing Agent;
- optional instruction uses canonical semantic prompt.

NativeResume:
- same provider exact resumable session;
- canonical safe launch transaction.

ContextTransfer:
- exact provider source;
- bounded transfer artifact;
- canonical safe launch;
- exactly one initial briefing.

NeedsProjectSelection:
- no mutation before Project choice.

CreatedNeedsAttention:
- preserve committed target;
- navigate to it;
- show exact phase/detail;
- no whole-operation Retry button.

## P9 — defer Live Handoff execution

Do not expose live Handoff until the Conversation delivery coordinator/freshness/pending-operation transaction is ported in 0.6.

You may port pure types/eligibility helpers only if required by 0.5 Agent cards.

Do not infer source freshness from terminal output.

## P10 — performance and dynamic rendering

- Agent cards consume reconciled state only;
- no per-card polling;
- marker stores bounded to live Agents;
- Agent page virtualized;
- one event burst → one Agent projection application;
- status updates do not rebuild unrelated hidden routes unnecessarily;
- no render-time filesystem/subprocess/SQLite/Herdr RPC;
- keep Working anti-flicker presentation centralized;
- logs must not contain prompts, transcripts or terminal output.

## Focused validation

Use atomic tasks from `docs/mygo-native-0.5.0-agent-workbench-plan.md`.

Per task:

```text
focused go test
git diff --check
```

At package gate:

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go test -race ./internal/agent/... ./internal/app/... ./internal/nativeui/...
GOTOOLCHAIN=go1.27.1 go tool mygo build
cd ..
git diff --check
```

## Final acceptance matrix

Before handing back, report DONE / PARTIAL / NOT DONE for:

- AgentKey/runtime phase;
- sendability;
- unread markers;
- review-pending;
- Mark reviewed;
- AgentCardModel;
- Sidebar Agent rows;
- Header Agent Overview;
- `/agents` page;
- MRU switcher;
- notifications;
- AlreadyLive Continue;
- NativeResume;
- ContextTransfer;
- NeedsProjectSelection;
- CreatedNeedsAttention;
- Live Handoff (expected deferred unless all Conversation prerequisites are complete);
- Native Chat (expected deferred to 0.6).

Also prove:

- no global Herdr focus RPC was reintroduced;
- no terminal-output Agent-state inference was introduced;
- no duplicate Agent launch/Continue path exists;
- no WebView exists;
- full tests/race tests/build pass.

Proceed without asking for confirmation. Split any task expected to exceed four hours before implementing it.
