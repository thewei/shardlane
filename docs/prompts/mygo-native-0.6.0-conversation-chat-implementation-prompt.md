# Shardlane MyGo 0.6.0 Native Conversation & Chat Implementation Prompt

Continue the Shardlane MyGo migration after 0.5 Agent Workbench & Lifecycle is complete.

Repository:

`/Users/wilson/Workspaces/wh-studio/herdr-client`

Use the existing Devspace workspace and branch `rewrite/mygo`.
Preserve user/other-agent changes. Do not reset, stage, commit, or push unless explicitly requested.

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
10. `next/CLAUDE.md`

Then inspect the original Rust behavior authorities named in the 0.6 plan, especially:

- `crates/shardlane-host/src/conversations.rs`
- `crates/shardlane-host/src/conversation_service.rs`
- `crates/shardlane-host/src/conversation_delivery.rs`
- `crates/shardlane-host/src/conversation_queue.rs`
- `crates/shardlane-host/src/conversation_interactions.rs`
- `crates/herdr-history/src/live/mod.rs`
- `crates/herdr-history/src/live/registry.rs`
- `crates/herdr-history/src/live/transport.rs`
- `crates/herdr-history/src/live/wake.rs`
- `crates/herdr-gui/src/chat/model.rs`
- `crates/herdr-gui/src/agent_ui/conversation.rs`
- `crates/herdr-gui/src/agent_ui/activity.rs`
- `crates/herdr-gui/src/agent_ui/interaction_view.rs`
- `crates/shardlane-host/src/live_handoff.rs`

Port product behavior, not GPUI implementation details.

## Mission

Implement **0.6 Native Conversation & Chat**.

The invariant is:

```text
Terminal and Chat are two views over the same Herdr Agent/provider session.
```

Opening Chat must never create a duplicate Agent or provider session.

## Architecture rules

- Herdr remains runtime/PTTY/Agent authority.
- Go ConversationService owns product Conversation semantics.
- Native UI does not call provider files, Herdr prompt RPCs, queue workers, or interaction bridges directly.
- Native UI never parses ANSI/TUI output for Conversation semantics.
- No WebView/React/Vite/xterm Chat.
- No global Herdr focus RPC navigation.
- No generic prompt response to a structured provider interaction.
- No guessed PTY keys for Question/Permission/Plan interactions.
- No automatic mutation retry after a request may have committed.

## P0 — Conversation identity first

Implement opaque Conversation IDs and session-exact live identity.

A live mutation must prove the current typed Agent session matches the ID fingerprint.

If a Pane has a replacement occupant, stale mutation fails closed and instructs the UI to reopen/reconcile.

Never expose provider session file paths as public Conversation identity.

## P1 — provider-neutral Conversation models

Implement Summary / Item / Window / Identity / Mutation / Detail models.

Item kinds:

```text
User
Assistant
Reasoning
Tool
Activity
Meta
CompactSummary
```

History and Live must converge to the same bounded `ConversationWindow` shape.

No MyGo types in domain/application packages.

## P2 — bounded reads

Use the original bounded service contract:

```text
default before/after = 80
maximum = 200
```

Cancel obsolete Conversation loads and reject stale generation results.

Never materialize unbounded live transcripts in Native UI state.

## P3 — incremental live semantic decoder

Do not parse the terminal.

Use provider semantic source capabilities and transport-neutral incremental decoding.

Required properties:

- full initial hydration only once;
- normal append reads appended bytes only;
- partial-line safe;
- duplicate wakes idempotent;
- tool-result backfill changes existing rows;
- truncate/replace emits projection reset;
- full parse equals incremental decode at arbitrary splits;
- wake storms coalesce;
- bounded timer is only a backstop.

Keep provider technical live capability separate from product exposure.

## P4 — one semantic prompt transaction

Implement one authoritative `SubmitPrompt` used by every client path.

Input:

```text
ConversationId
request_id
text
```

Transaction:

```text
validate non-empty
→ acquire per-Conversation serialization
→ claim/check MutationLedger request_id
→ resolve exact current Agent occupant
→ validate session fingerprint
→ classify sendability
→ SentNow / QueuedAfterTurn / NeedsTerminal / fail closed
→ record exact accepted/uncertain outcome
```

`request_id` replay must never re-execute a committed or uncertain prompt.

## P5 — follow-up queue

Exactly one queued semantic follow-up per live Conversation for 0.6.

When Working:

```text
accept queue
→ exact-target wait
→ revalidate occupant identity
→ atomic delivery claim
→ exactly one semantic prompt
```

Safety:

- Blocked retains item;
- timeout retains item;
- occupant change returns recoverable text;
- definite failure returns recoverable text;
- uncertain delivery never retries automatically;
- cancellation only before delivery commit;
- cancel A / enqueue B race cannot let old worker send B;
- two workers => one claimant.

Run race tests.

## P6 — Native timeline model

Port the original presentation semantics:

```text
UserPrompt
ContextBoundary
TurnThinking
ToolActivity
ToolGroup
Answer
ResponseFooter
WorkingIndicator
TurnStopped
```

Rules:

- Text-kind User starts turn.
- Assistant narration is never hidden in a turn-level fold.
- One consolidated thinking block per turn.
- Streaming thinking defaults expanded; settled defaults collapsed.
- 2+ consecutive tools may compact into one ToolGroup.
- explicit diffs only; never synthesize arbitrary-output diffs.
- Working row uses about 500 ms anti-flicker presentation delay.
- failed last turn stays visible with a stopped marker.

Build stable row keys and minimal splice planning so tail appends do not rebuild the whole bounded timeline.

## P7 — pending user echo

After a SentNow commit, a local pending User row may appear immediately.

Reconcile it only when the provider semantic source emits a matching User row after the submission baseline.

No terminal-echo confirmation.

No duplicate visible message.

## P8 — Native Composer

Presentation modes:

```text
Ready               → Send
Working              → Send after turn
Blocked/Failed       → Open Terminal
Unknown              → fail closed / refresh
```

The UI may display these modes, but the service makes the authoritative commit-time decision again.

Do not add mid-turn injection.

## P9 — queued follow-up UI

Render one queued item with truthful states:

```text
queued
waiting for turn
delivering
failed recoverable
delivery uncertain
```

Before commit: Cancel can recover draft.

After commit/uncertainty: do not expose a false Cancel/Retry that can duplicate delivery.

## P10 — structured interaction broker

Port provider-neutral interactions:

```text
Question
Permission
PlanApproval
Authentication
Other
```

Lifecycle:

```text
Pending
Resolving
Resolved
Rejected
Cancelled
NativeFallback
Expired
```

Responses:

```text
Choice
MultiChoice
Text
Allow(scope)
Deny
Cancel
DelegateToTerminal
```

Resolution is revision-CAS guarded and exact-occupant validated.

Exactly one resolver wins.

Never answer a structured interaction with ordinary `SubmitPrompt`.

`DelegateToTerminal` must first record NativeFallback, then navigate locally to the Terminal.

## P11 — legacy approval fallback

Do not expand the old TUI-grid approval path.

Only retain it where an existing provider adapter supplies verified stable option/key semantics and a guard proves the current terminal state still matches.

Prefer structured InteractionBroker for all new/provider-capable paths.

Never invent approval keys.

## P12 — Native Chat route/UI

Route:

```text
/conversation/{conversation_id}
```

Use official MyGo Native collection/scroll primitives.

Requirements:

- latest bounded tail on open;
- virtualized/bounded timeline;
- User/Assistant/Thinking/Tool/Context/Interaction/Queue rows;
- follow tail only when user is already near bottom;
- user scrolled upward is not yanked;
- prepend older rows preserves visual anchor;
- Conversation switch cancels old jobs;
- hidden Chat does not build expensive content.

Reuse 0.4 Design System and 0.5 provider/status/Agent components.

## P13 — Terminal / Chat same-session switch

Each live Agent can switch presentation mode between Terminal and Chat.

Switching:

- must not create/stop Agent;
- must not mutate Herdr global focus;
- must not send terminal input to hidden Terminal from Chat;
- must reconcile Chat from semantic source after returning from Terminal.

Blocked interaction routing to Terminal must preserve the same Agent/session identity.

## P14 — live Handoff

Now complete the handoff deferred by 0.5.

Required transaction:

```text
reserve source Conversation
→ exact occupant/status read
→ reject unresolved older operation
→ if Working: wait exact source turn boundary
→ reject Blocked/Failed
→ verify occupant again
→ source freshness fence
→ semantic source snapshot
→ canonical target launch
→ exactly one full-context briefing + optional instruction
→ reconcile target
```

No `Handoff now` that snapshots an in-flight turn.

Source Agent remains untouched.

Preserve distinct failure classes, especially:

- source changed;
- source busy;
- source blocked;
- source not flushed;
- source has pending operation;
- target CreatedNeedsAttention.

Never blindly retry a committed target.

## P15 — performance and rendering

- no IO in render;
- bounded Conversation windows;
- incremental append bytes only;
- stable keys/minimal row splice;
- virtualized Native timeline;
- centralized Working anti-flicker;
- wake storm coalescing;
- no full-window animation loop;
- no hidden-route heavy rendering;
- stale async results cannot apply;
- logs do not contain Conversation bodies/tool output.

## Task order

Follow the atomic IDs in `docs/mygo-native-0.6.0-conversation-chat-plan.md`:

```text
CONV-01..10  identity/read
CONV-11..19  live decoder
CONV-20..25  prompt transaction
CONV-26..34  queue
CONV-35..40  interactions
CHAT-01..09  presentation model
CHAT-10..17  Native timeline
CHAT-18..24  composer/queue UI
CHAT-25..30  interaction UI
HANDOFF-01..09 live handoff
CHAT-31..36  closure/performance/package
```

Each task should normally fit 30 minutes–4 hours. Split larger tasks before implementation.

## Verification cadence

Per atomic task:

```text
focused go test
git diff --check
```

Do not full-build every task.

At package gate:

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go test -race ./internal/conversation/... ./internal/agent/... ./internal/app/...
GOTOOLCHAIN=go1.27.1 go tool mygo build
cd ..
git diff --check
```

## Required final audit

Report DONE / PARTIAL / NOT DONE for:

- opaque/session-exact Conversation IDs;
- stale occupant protection;
- History + Live bounded reads;
- live incremental decoder;
- semantic prompt idempotency;
- SentNow;
- QueuedAfterTurn;
- NeedsTerminal;
- follow-up queue cancellation/recovery;
- delivery uncertainty safety;
- pending echo reconciliation;
- Native timeline;
- Working anti-flicker;
- Tool/Thinking rows;
- Native Composer;
- InteractionBroker;
- Question/Permission/Plan UI;
- DelegateToTerminal;
- Terminal/Chat same-session switching;
- Live Handoff;
- CreatedNeedsAttention handling;
- large-conversation performance;
- Remote/Mobile integration (expected later unless explicitly expanded).

Also prove:

- no WebView Chat;
- no terminal-output semantic parsing;
- no generic-prompt interaction fallback;
- no guessed PTY-key interaction path;
- no global Herdr focus navigation;
- no duplicate Agent/session/runtime;
- no automatic retry after delivery uncertainty;
- full tests/race tests/build pass.

Proceed without asking for confirmation.
