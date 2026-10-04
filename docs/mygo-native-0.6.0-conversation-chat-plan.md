# Shardlane MyGo 0.6.0 — Native Conversation & Chat Plan

Status: **planned post-0.5 release**
Target: `0.6.0`
Depends on: 0.5 Agent Workbench & Lifecycle complete
Architecture authority: `docs/client-product-architecture.md`
Execution rules: `docs/mygo-native-execution-rules.md`
Planning review: **reviewed 2026-10-05 against the post-0.4.0 tree** — authority files, MyGo API prerequisites (Tray/Notification where used) and 0.4 prerequisites (launch transaction, integration health, operational status model, Design System v2) all verified present; no stale statements found.

## 1. Version goal

0.6 is the first full semantic Conversation release for the MyGo rewrite.

Goal:

> Provide one Native Conversation surface for live Agents and History-backed conversations,
> with exact session identity, bounded live decoding, safe semantic prompt delivery,
> queue-after-turn behavior, structured interactions, tool activity, continuation, and live handoff.

This is not a Web chat client layered over a terminal. It is a Native presentation over the same
Herdr-owned Agent session used by Terminal.

The core rule is:

```text
Terminal View ─┐
               ├── same Herdr Agent / same provider session
Chat View ─────┘
```

No duplicate runtime, PTY, Agent, or provider session is created merely by opening Chat.

## 2. Original-project behavior authority

Port product semantics from the original implementation, not GPUI mechanics.

| Original authority | Behavior to preserve |
|---|---|
| `crates/shardlane-host/src/conversations.rs` | opaque Conversation identity, stale-occupant protection, provider-neutral item projection |
| `crates/shardlane-host/src/conversation_service.rs` | one semantic read/mutate service for live + History conversations |
| `crates/shardlane-host/src/conversation_delivery.rs` | mutation ledger, per-Conversation serialization, exactly-once/idempotent delivery |
| `crates/shardlane-host/src/conversation_queue.rs` | one follow-up while Working; exact-target settle/revalidate/deliver; blocked/uncertain safety |
| `crates/shardlane-host/src/conversation_interactions.rs` | provider-neutral interaction broker, CAS revision, first-responder wins, occupant-change fail-closed |
| `crates/herdr-history/src/live/*` | live semantic source, incremental append/change/reset decoding, no ANSI/TUI semantics |
| `crates/herdr-gui/src/chat/model.rs` | disposable Chat state, pending echo, Working anti-flicker, queue projection |
| `crates/herdr-gui/src/agent_ui/conversation.rs` | turn/row projection, thinking/tool grouping, visible narration, working/stopped rows |
| `crates/herdr-gui/src/agent_ui/activity.rs` | semantic Tool activity classes and explicit-diff rendering |
| `crates/herdr-gui/src/agent_ui/interaction_view.rs` | Native Question/Permission/Plan interaction presentation |
| `crates/shardlane-host/src/history_continuation.rs` | History Continue already completed in 0.5; reuse identity/transaction results |
| `crates/shardlane-host/src/live_handoff.rs` | safe live Agent-to-Agent handoff with settle/freshness/pending-operation fences |

## 3. Non-negotiable architecture

### 3.1 ConversationService is the application seam

Native UI must not directly:

- call provider session files;
- parse live JSONL;
- call `agent.prompt` directly;
- own queue delivery;
- send guessed terminal keys;
- infer semantic state from terminal bytes;
- resolve provider questions through generic prompts.

The shape is:

```text
MyGo Native Chat
      │
      ▼
internal/app ConversationService
      │
      ├─ Conversation identity/read projection
      ├─ ConversationDeliveryCoordinator
      ├─ FollowUpQueue
      ├─ InteractionBroker
      ├─ HistoryService
      └─ Live semantic source
             │
             ▼
          Herdr runtime / exact provider session source
```

### 3.2 Herdr remains runtime authority

Herdr owns:

- Agent/process lifecycle;
- Pane/Tab/Project runtime state;
- PTYs;
- runtime Agent status;
- terminal IDs;
- typed Agent session identity.

Shardlane Go owns product semantics around Conversation delivery, projection and presentation.

### 3.3 Native UI only

0.6 Chat remains MyGo Native UI.

Do not add:

- React;
- Vite;
- xterm;
- WKWebView/private WebView embedding;
- localhost Chat frontend;
- a second window solely to host Web Chat.

## 4. Conversation identity

### 4.1 Opaque IDs

Use opaque Conversation IDs. Native UI never parses provider paths/native IDs.

Required locator classes:

```text
Live session-exact conversation
History conversation
legacy/read-only live locator only if compatibility requires it
```

### 4.2 Session-exact live identity

A live mutation must bind:

```text
AgentRef + typed AgentSessionInfo fingerprint
```

The fingerprint is derived from:

```text
provider + locator kind + locator source + locator value
```

The exact hash format may mirror the original implementation, but the product requirement is:

> a stale Conversation ID must fail closed if the Pane now hosts a different provider session.

Never retarget a stale Conversation mutation to the replacement occupant.

### 4.3 Public privacy boundary

Path-backed provider session file paths stay internal.

Native/UI DTOs may expose opaque ConversationId and safe provider-native IDs where allowed, but never raw provider file paths as public identity.

## 5. Conversation domain model

Use provider-neutral models independent of MyGo.

Suggested core:

```go
type ConversationSource string // live | history

type ConversationItemKind string // user | assistant | reasoning | tool | activity | meta | compact-summary

type ConversationSummary struct {
    ID           ConversationID
    ProjectID    string
    Source       ConversationSource
    Provider     string
    Title        string
    RuntimePhase agent.AgentRuntimePhase
    Sendability  agent.AgentSendability
    AgentKey     *agent.AgentKey
    UpdatedAtMS  *int64
    Revision     uint64
}

type ConversationItem struct {
    ID          string
    Seq         uint64
    Kind        ConversationItemKind
    Role        string
    Text        string
    Thinking    *string
    ToolCalls   []ConversationToolCall
    TimestampMS *int64
    Model       *string
    Truncated   bool
}

type ConversationWindow struct {
    ConversationID ConversationID
    Revision       uint64
    Items          []ConversationItem
    FirstSeq       *uint64
    LastSeq        *uint64
    HasOlder       bool
    HasNewer       bool
}
```

No MyGo element/view type belongs in these packages.

## 6. Bounded read model

Match the original service contract:

```text
default read half-window = 80
maximum before/after = 200
```

Native presentation may materialize fewer rows when appropriate, but the application service must remain bounded.

Rules:

- never load the unbounded whole live transcript into Native UI state;
- older/newer pagination uses sequence anchors;
- switching Conversation cancels obsolete reads;
- stale generation results cannot apply;
- History and Live share the same `ConversationWindow` shape;
- History remains read-only.

## 7. Live semantic source

### 7.1 No TUI parsing

Live Chat is built from provider semantic sources, never ANSI/grid text.

### 7.2 Capability registry

Preserve the provider capability idea:

```text
AppendLog
HookJournal
None
```

Product exposure remains separate from technical decode support.

Initial required live providers should follow the post-0.5 provider registry. At minimum, do not regress the Stable live providers already supported by the original product.

### 7.3 Incremental decoder contract

A live decoder sync reports:

```text
appended message indices
changed message indices
projection reset
facts update
```

Required behavior:

- initial hydration may read the source once;
- normal append reads only appended bytes;
- duplicate wakes are idempotent;
- partial lines do not emit corrupted rows;
- tool-result backfill updates existing rows;
- truncate/replace causes explicit projection reset;
- arbitrary split incremental decode equals full parse;
- unknown provider records are counted, not guessed.

### 7.4 Wake policy

Prefer event/file wake where available with a bounded timer backstop.

Do not use high-frequency polling.

Wake storms must coalesce.

## 8. ConversationService

Recommended application interface:

```go
type ConversationService interface {
    List(ctx context.Context, query ConversationQuery) ([]ConversationSummary, error)
    Detail(ctx context.Context, id ConversationID, bounds ConversationWindowBounds) (ConversationDetail, error)
    SubmitPrompt(ctx context.Context, id ConversationID, request PromptRequest) (PromptSubmission, error)
    QueueState(ctx context.Context, id ConversationID) (*QueuedFollowUp, error)
    CancelQueued(ctx context.Context, id ConversationID) (string, error)
    ResolveInteraction(ctx context.Context, req InteractionResolveRequest) (InteractionResolution, error)
    DelegateInteractionToTerminal(ctx context.Context, req InteractionDelegateRequest) (InteractionResolution, error)
}
```

UI never chooses send-now vs queue-after-turn itself. It may preview the expected affordance, but `SubmitPrompt` is authoritative.

## 9. Semantic prompt transaction

### 9.1 One authoritative entry point

All semantic prompts — Native Chat, History AlreadyLive, future Remote/Mobile — use the same transaction.

Input:

```text
ConversationId
request_id
text
```

### 9.2 Per-Conversation serialization

The entire decision/mutation transaction is serialized per exact live Agent target.

This prevents:

- an immediate prompt overtaking an older queued prompt;
- two same-id callers both mutating;
- handoff snapshot racing a new prompt.

### 9.3 Mutation ledger

`request_id` is an idempotency key.

Rules:

- same request ID + same logical request replays recorded result;
- same request ID + different body is rejected;
- uncertain delivery creates a tombstone;
- an uncertain prompt is never automatically re-executed;
- mutation accepted + post-read failure remains accepted.

## 10. Prompt disposition

Use the 0.5 Agent sendability SSOT.

```text
idle / done
    → SentNow

working / pending / launch_pending
    → QueuedAfterTurn

blocked / failed
    → NeedsTerminal

unknown
    → fail closed
```

Native composer labels are presentation only. The service decides again at commit time.

## 11. Follow-up queue

### 11.1 One queued semantic follow-up per live Conversation

Initial 0.6 queue contract remains intentionally simple and safe.

Queue item captures:

- request ID;
- ConversationId;
- AgentRef;
- provider;
- native session ID if safe;
- opaque session fingerprint;
- baseline revision;
- text;
- created time;
- queue state.

### 11.2 States

```text
Queued
WaitingForTurnBoundary
Delivering
Delivered
FailedRecoverable
DeliveryUncertain
Cancelled
```

### 11.3 Delivery

```text
queue accepted
→ exact-target wait
→ occupant identity revalidation
→ revision/readiness check
→ atomic claim
→ exactly one semantic prompt
→ reconcile result
```

### 11.4 Safety

- Blocked retains the queue; it does not send.
- Timeout retains the queue.
- Identity change fails closed and returns recoverable text.
- Definite rejection returns recoverable text.
- Delivery uncertainty never auto-retries and does not pretend the text was unsent.
- cancel/edit is available only before delivery commit.
- cancel-A/enqueue-B races cannot let an old worker send B.
- two delivery workers: exactly one owns the claim.

## 12. Pending submission echo

When `SentNow` is accepted, Native Chat may show a temporary local User row immediately.

Pending echo contains:

```text
text
baseline message count/seq
exact Conversation/Agent binding
```

When the provider semantic source later emits the matching User message after the baseline, the pending row is consumed exactly once.

Rules:

- no duplicate visible user row;
- rebind to another Agent drops unrelated pending echo;
- same-pane History Continue rebind may preserve it when identity proves the same target;
- do not use terminal echo text as confirmation.

## 13. Native Chat timeline model

0.6 reuses the 0.4/0.5 Design System and ToolCard primitives.

### 13.1 Turn boundary

A Text-kind User message opens a new turn.

### 13.2 Row types

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
InteractionCard
QueuedFollowUpCard
PendingSubmissionRow
```

### 13.3 Narration stays visible

Assistant text is never hidden behind a turn-level “Worked” fold.

Narration between tool calls is progress information and remains visible.

### 13.4 Thinking

One consolidated thinking block per turn.

Presentation policy:

- streaming thinking defaults expanded;
- settled thinking defaults collapsed;
- user toggle is presentation-only;
- measurable duration may label `Thought for Xs`;
- do not invent reasoning text when provider does not expose it.

### 13.5 Tool grouping

Consecutive runs of 2+ tool rows may compact into one expandable ToolGroup.

The tool semantic classes remain:

```text
Command
FileRead
FileSearch
FileChange
WebSearch
Plan
Generic
```

Only explicit diff data renders as a diff. Never synthesize a diff from arbitrary output.

### 13.6 Working indicator

Herdr runtime status is authoritative.

The last turn is immediately considered in-progress while Working, but the visible `Working…` row uses approximately a 500 ms presentation delay so near-instant turns do not flash.

Do not delay underlying state or queue semantics.

### 13.7 Failed run

A failed last turn remains visible and gets an explicit stopped/failed row. Do not delete the partial transcript.

## 14. Minimal row updates

Live appends should not rebuild the whole Conversation tree when avoidable.

Use stable row fingerprints/keys and compute minimal replacement range:

```text
pure append
→ insert tail rows

mid-stream tool result / thinking merge
→ replace minimal changed range

projection reset
→ replace bounded projection intentionally
```

This is a presentation optimization; semantic state remains immutable/reconciled.

## 15. Composer

### 15.1 Modes

```text
Send
Send after turn
Needs Terminal
Unknown / refresh required
```

### 15.2 Send

Idle/Done + non-empty draft.

### 15.3 Send after turn

Working-family state.

Submitting enqueues one semantic follow-up. Do not inject mid-turn text.

### 15.4 Needs Terminal

Blocked/Failed.

Composer must not offer ordinary Send.

Primary action:

```text
Open Terminal
```

### 15.5 Unknown

Fail closed.

Show a refresh/reconnect explanation rather than pretending sendable.

## 16. Queued follow-up UI

Show one compact queued card near the composer/timeline tail.

States:

```text
Queued
Waiting for turn
Delivering
Failed — recover text
Delivery uncertain — inspect before retry
```

Before commit the user may cancel and recover draft text.

After commit/uncertainty the UI must not expose a misleading Cancel/Retry that could duplicate delivery.

## 17. Structured interactions

### 17.1 Preferred path

Use the provider-neutral `ConversationInteractionBroker` semantics.

Interaction kinds:

```text
Question
Permission
PlanApproval
Authentication
Other
```

States:

```text
Pending
Resolving
Resolved
Rejected
Cancelled
NativeFallback
Expired
```

### 17.2 Responses

```text
Choice
MultiChoice
Text
Allow(scope)
Deny
Cancel
DelegateToTerminal
```

### 17.3 CAS safety

Resolution includes the interaction revision.

Exactly one resolver wins.

Reject:

- stale revision;
- already resolved;
- occupant changed;
- provider bridge unavailable;
- invalid response.

Never convert a structured provider question into an ordinary Agent prompt.

### 17.4 Terminal fallback

`DelegateToTerminal` is explicit and updates the interaction state to NativeFallback before navigating the user to the Terminal.

Do not guess terminal keys.

### 17.5 Legacy live approval fallback

The older decoder-based approval path may only remain as a compatibility fallback for providers where:

- the adapter produces a verified stable approval request;
- option key bytes are provider-derived, not invented;
- a guard proves the terminal menu/grid still matches before sending.

Prefer the structured InteractionBroker wherever available. Do not expand the legacy path to new providers.

## 18. Interaction card UI

Native interaction cards reuse the 0.4 Design System.

Question:

- single/multi choice;
- optional custom text;
- Continue/Cancel.

Permission / Plan:

- Allow once;
- Allow for session when provider supports it;
- Reject;
- Open in Terminal.

Authentication/Other:

- only expose responses proven by provider metadata;
- otherwise Delegate to Terminal.

While resolving:

- disable duplicate actions;
- show in-flight state;
- apply only matching generation/revision result.

## 19. Live Handoff

0.6 closes the handoff deferred by 0.5.

### 19.1 Meaning

Move full semantic context from one live Agent to a new Agent/provider while leaving the source untouched.

### 19.2 Preconditions

- exact source Agent/session identity;
- source has a semantic source;
- no unresolved earlier Conversation operation;
- source not Blocked/Failed;
- target provider/integration available;
- canonical launch transaction available.

### 19.3 Working source

There is no unsafe `Handoff now` in v1.

Working source behavior:

```text
Handoff after current turn
→ exact-target settle wait
→ identity revalidation
→ source freshness barrier
→ snapshot
→ target launch
→ exactly one initial briefing
```

### 19.4 Freshness

A stable file/stat alone is not automatically equivalent to a verified completed-turn flush.

Preserve explicit fidelity result when the source format can only prove weaker freshness.

### 19.5 Source protection

The source Agent is never automatically stopped/closed/mutated by handoff.

### 19.6 Failure classes

Keep distinct UI outcomes:

```text
SourceUnresolved
WaitFailed
SourceIdentityChanged
SourceBusy
SourceBlocked
WaitForSourceFlush
SourceHasPendingOperation
SnapshotFailed
TransferFailed
CreatedNeedsAttention
```

If target creation committed, preserve/navigate to that target instead of retrying the whole operation.

## 20. Conversation route and binding

Recommended route:

```text
/conversation/{conversation_id}
```

Opening an Agent's Chat resolves to its exact session ConversationId.

History Continue that reuses/creates a live Agent routes to the resulting live Conversation.

Local navigation remains client-owned. Do not mutate Herdr global focus merely to show Chat.

## 21. Terminal / Chat mode switching

Each selected Agent supports two presentation modes:

```text
Terminal
Chat
```

Rules:

- switching mode never creates/stops the Agent;
- hidden Chat must not consume terminal keyboard input;
- hidden Terminal remains runtime-owned but expensive rendering should be minimized according to MyGo constraints;
- blocked interaction may route Chat → Terminal;
- returning to Chat reconciles from semantic Conversation state, not terminal screen contents.

## 22. Conversation presentation state

Disposable client state may include:

```text
active ConversationId
load generation/cancel
bounded window
expanded thinking turns
expanded tool groups
expanded context boundaries
pending local submission
queued follow-up view
active interactions
composer draft
working-indicator presentation timer
scroll anchor
```

It must not include runtime lifecycle authority.

## 23. Scroll behavior

### Initial open

Land at the latest bounded tail.

### Live append while at bottom

Follow tail.

### User scrolled away from bottom

Do not yank scroll position. Show a compact “new activity” affordance.

### Loading older rows

Preserve visual anchor after prepend.

### Conversation switch

Cancel previous load/watch and restore new Conversation's own presentation state where appropriate.

## 24. Performance constraints

- no transcript parsing in render;
- no filesystem/subprocess/RPC in render;
- incremental decoder normal append reads only appended bytes;
- bounded Conversation windows;
- virtualized/native list rendering;
- stable row keys;
- minimal row splice for live updates;
- no full-window repaint timer for Working animation;
- one centralized anti-flicker timer/state;
- wake storm coalescing;
- obsolete Conversation jobs cancelled;
- hidden Conversation route performs no expensive row construction;
- logs never include full prompt/transcript/tool output.

## 25. Privacy/logging

Allowed logs:

- ConversationId/AgentKey if not provider-secret;
- operation/request ID;
- counts;
- seq/revision;
- durations;
- state transitions;
- error classes.

Never log:

- prompt body;
- assistant response body;
- thinking;
- tool input/output;
- raw provider source;
- terminal contents;
- provider secrets/credentials.

## 26. Atomic implementation plan

### C0 — Conversation identity/domain

| ID | Task | Verify |
|---|---|---|
| CONV-01 | ConversationId + locator types | encode/decode table tests |
| CONV-02 | session fingerprint + stale occupant validation | replacement occupant tests |
| CONV-03 | provider alias/session source projection | original fixture parity |
| CONV-04 | provider-neutral Conversation models | JSON/model tests |
| CONV-05 | live/history summary projection | fixture tests |

### C1 — read service

| ID | Task | Verify |
|---|---|---|
| CONV-06 | ConversationWindow bounds/clamp | boundary tests |
| CONV-07 | History Conversation read adapter | HistoryService integration |
| CONV-08 | Live Conversation source resolver | typed id/path tests |
| CONV-09 | ConversationService Detail | bounded window tests |
| CONV-10 | cancellation/latest-generation application | slow/fast deterministic tests |

### C2 — live decoder

| ID | Task | Verify |
|---|---|---|
| CONV-11 | LiveCapability registry | provider table tests |
| CONV-12 | transport-neutral live interface | fake transport tests |
| CONV-13 | append cursor/partial-line behavior | byte-tail tests |
| CONV-14 | Claude live decoder | full == incremental fixture |
| CONV-15 | Codex live decoder | full == incremental fixture |
| CONV-16 | other Stable providers required by current exposure | fixture parity |
| CONV-17 | tool-result/streaming changed-row updates | delta tests |
| CONV-18 | projection reset/truncate | reset tests |
| CONV-19 | wake/coalescing + timer backstop | deterministic wake tests |

### C3 — prompt delivery core

| ID | Task | Verify |
|---|---|---|
| CONV-20 | MutationLedger | replay/conflict/uncertain tests |
| CONV-21 | per-Conversation serialization | concurrent ordering tests |
| CONV-22 | SubmitPrompt SentNow | exactly-once fake Herdr test |
| CONV-23 | NeedsTerminal | zero mutation test |
| CONV-24 | unknown fail-closed | zero mutation test |
| CONV-25 | accepted mutation + degraded enrichment | committed-result test |

### C4 — follow-up queue

| ID | Task | Verify |
|---|---|---|
| CONV-26 | one-item queue state model | pure state tests |
| CONV-27 | enqueue while Working | acceptance test |
| CONV-28 | exact-target wait + identity revalidation | replacement occupant test |
| CONV-29 | atomic delivery claim | concurrent worker test |
| CONV-30 | cancel-before-delivery | race tests |
| CONV-31 | cancel-A/enqueue-B isolation | race test |
| CONV-32 | blocked/timeout retention | queue tests |
| CONV-33 | definite failure recovery | draft recovery test |
| CONV-34 | uncertain delivery tombstone | never-auto-retry test |

### C5 — interaction broker

| ID | Task | Verify |
|---|---|---|
| CONV-35 | interaction models | serialization/table tests |
| CONV-36 | publish/snapshot broker | pure service tests |
| CONV-37 | CAS resolve first-winner | concurrent resolver test |
| CONV-38 | occupant-change cancellation | stale identity test |
| CONV-39 | expiry/cancel/bridge disconnect | lifecycle tests |
| CONV-40 | DelegateToTerminal | exact state + navigation intent test |

### C6 — timeline presentation model

| ID | Task | Verify |
|---|---|---|
| CHAT-01 | turn derivation | original fixture parity |
| CHAT-02 | ConversationRow projection | golden row tests |
| CHAT-03 | thinking consolidation/toggle model | pure tests |
| CHAT-04 | tool run compaction | 1 vs 2+ tool tests |
| CHAT-05 | response footer/in-progress rules | row tests |
| CHAT-06 | Working anti-flicker | deterministic clock test |
| CHAT-07 | failed turn marker | row test |
| CHAT-08 | row fingerprint/minimal splice | append/change/reset tests |
| CHAT-09 | pending submission reconciliation | baseline/order tests |

### C7 — Native Chat UI

| ID | Task | Verify |
|---|---|---|
| CHAT-10 | `/conversation/{id}` route | Router/headless test |
| CHAT-11 | Native virtualized timeline | headless large-list test |
| CHAT-12 | User/Assistant/Context blocks | render smoke |
| CHAT-13 | Thinking block | toggle/render tests |
| CHAT-14 | ToolCard/ToolGroup | expand/collapse render tests |
| CHAT-15 | Working/Stopped rows | state render tests |
| CHAT-16 | scroll anchor/follow-tail | deterministic scroll-state tests |
| CHAT-17 | Terminal/Chat mode switch | same-session/no-launch test |

### C8 — Composer and queue UI

| ID | Task | Verify |
|---|---|---|
| CHAT-18 | Native Composer state | headless input tests |
| CHAT-19 | Ready → Send | one request ID / pending echo test |
| CHAT-20 | Working → Send after turn | queue card test |
| CHAT-21 | Blocked → Open Terminal | zero-prompt test |
| CHAT-22 | Unknown fail-closed UI | headless state test |
| CHAT-23 | queued cancel/recover | state/action test |
| CHAT-24 | uncertain/failed queue presentation | no unsafe retry test |

### C9 — interaction UI

| ID | Task | Verify |
|---|---|---|
| CHAT-25 | Native InteractionCard base | render smoke |
| CHAT-26 | Question single/multi/custom text | headless actions |
| CHAT-27 | Permission/Plan actions | response tests |
| CHAT-28 | in-flight/duplicate-submit guard | async deterministic test |
| CHAT-29 | stale revision result | refresh/error presentation test |
| CHAT-30 | DelegateToTerminal | route/state test |

### C10 — live handoff

| ID | Task | Verify |
|---|---|---|
| HANDOFF-01 | source exact-identity resolver | id/path fixtures |
| HANDOFF-02 | source sendability eligibility | state matrix |
| HANDOFF-03 | source Conversation reservation | prompt/handoff race test |
| HANDOFF-04 | settle cycles | busy/settled tests |
| HANDOFF-05 | source freshness fence | verified/stale fixtures |
| HANDOFF-06 | pending-operation fence | queue/ledger tests |
| HANDOFF-07 | transfer + canonical launch | exactly-one target/briefing test |
| HANDOFF-08 | CreatedNeedsAttention | committed target preservation |
| HANDOFF-09 | Native Handoff dialog/action | headless state test |

### C11 — package closure

| ID | Task | Verify |
|---|---|---|
| CHAT-31 | no render-time Conversation IO audit | source/static audit |
| CHAT-32 | long-conversation performance | bounded window/list benchmark |
| CHAT-33 | live append soak | decoder + UI update soak |
| CHAT-34 | queue concurrency race pass | `go test -race` |
| CHAT-35 | interaction concurrency race pass | `go test -race` |
| CHAT-36 | 0.6 real-app acceptance | focused manual/automated acceptance |

## 27. Required package gate

Before producing 0.6 test build:

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go test -race ./internal/conversation/... ./internal/agent/... ./internal/app/...
GOTOOLCHAIN=go1.27.1 go tool mygo build
cd ..
git diff --check
```

Forbidden-surface audit:

- no React/Vite/xterm Chat;
- no WebView Chat;
- no terminal-output semantic parser;
- no generic-prompt interaction fallback;
- no guessed PTY-key interaction path;
- no global Herdr focus mutation for Chat navigation;
- no second Agent/session/runtime;
- no auto retry after delivery uncertainty.

## 28. Focused real-app acceptance

1. Open a live Agent in Terminal, switch to Chat, confirm same Agent remains running.
2. Initial Chat opens at bounded tail.
3. Live assistant text/tool results append without full transcript reload.
4. Streaming thinking/tool updates modify existing rows correctly.
5. Instant turn does not flash Working row; long turn does.
6. Idle prompt sends exactly once.
7. Working prompt becomes exactly one queued follow-up.
8. Queue waits for turn boundary, revalidates occupant and sends once.
9. Blocked Agent never accepts ordinary semantic Send; Open Terminal works.
10. Unknown Agent state fails closed.
11. Prompt delivery uncertainty never auto-retries.
12. Pending local User echo disappears exactly once when provider source confirms it.
13. User scrolled upward is not yanked to bottom by new activity.
14. Thinking/tool groups expand/collapse without changing semantic state.
15. Structured question resolves through provider bridge.
16. Two simultaneous interaction responders: only one wins.
17. Stale interaction revision is rejected and UI reconciles.
18. DelegateToTerminal changes interaction state then navigates locally.
19. History Continue result can open the resulting live Conversation.
20. Live Handoff from settled source creates exactly one target and one briefing.
21. Working-source Handoff waits for turn; blocked source refuses truthfully.
22. Source occupant change aborts Handoff.
23. Target CreatedNeedsAttention is preserved/navigable and never blindly retried.
24. Closing/replacing Agent invalidates stale Conversation mutation IDs.
25. Terminal behavior/IME/copy/paste remains unchanged.

## 29. Definition of Done

0.6 is complete only when:

- [ ] one Conversation domain serves History and Live sources;
- [ ] live Conversation mutations are session-exact and stale-occupant safe;
- [ ] live semantic decoding is incremental and provider-source based;
- [ ] Chat never infers semantics from terminal output;
- [ ] Native Chat uses bounded/virtualized timeline rendering;
- [ ] Assistant narration stays visible while tool activity remains compact;
- [ ] thinking/tool/context expansion is presentation-only;
- [ ] Working indicator anti-flicker works without polling/repaint loops;
- [ ] prompt request IDs are idempotent at the application service layer;
- [ ] SentNow / QueuedAfterTurn / NeedsTerminal / Unknown are truthful;
- [ ] queued delivery is exactly-once and identity revalidated;
- [ ] uncertainty is never auto-retried;
- [ ] structured interactions use CAS/occupant-safe resolution;
- [ ] blocked interactions can explicitly DelegateToTerminal;
- [ ] live Handoff uses source reservation + freshness + pending-operation fences;
- [ ] source Agent remains untouched by Handoff;
- [ ] committed target failures surface CreatedNeedsAttention rather than blind retry;
- [ ] full tests/race tests/build pass;
- [ ] no WebView Chat was introduced.

## 30. 0.7 boundary — Status Center & Agent Quick Navigation

After 0.6, the client has the full core daily workflow. Before adding broader desktop tools, 0.7 closes the always-visible operational entry point:

- right-side titlebar Status Center;
- Needs Attention / Review / Working aggregation;
- direct Agent → Interaction / Terminal / Chat / Workbench routing;
- explicit Mark reviewed;
- MyGo cross-platform Tray projection using the same snapshot/actions.

Plan: `docs/mygo-native-0.7.0-status-center-plan.md`.
Prompt: `docs/prompts/mygo-native-0.7.0-status-center-implementation-prompt.md`.

0.8 then closes Project-grouped History management, Usage and Agent Inspector; Desktop supporting surfaces move to 0.9+.
