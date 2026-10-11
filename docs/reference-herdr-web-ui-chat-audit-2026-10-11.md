# herdr-web-ui Chat reference audit — 2026-10-11

Status: **Reference audit, feeding roadmap N19** (Chat presentation on the
official `ui.WebView`). Scope: Chat View features, communication model, and
API/protocol shapes of `examples/herdr-web-ui` (v0.4.6), audited for reuse as
*design and protocol thinking* — the Godiff rule applies: **UI/behavior
reference only; do not copy or import its implementation** into Shardlane.
The project is MIT-licensed and lives outside this repository at
`~/Workspaces/wh-studio/examples/herdr-web-ui`.

## 1. What herdr-web-ui actually is (verified)

A browser "lens" over agent TUIs: a React 18 + Vite client (`src/`) talks to a
Bun bridge (`server/index.ts`) over REST + one WebSocket; the bridge talks to
the Herdr daemon over newline-JSON Unix-socket RPC (`server/herdr/client.ts`)
and runs `herdr terminal attach` in a Node-PTY sidecar for terminals. The
defining property: **the bridge owns no conversation state** — Herdr and the
agent CLIs do. The chat view is a ~2 s poller of a structured conversation
endpoint whose server *parses each agent's native transcript store* (Claude
JSONL, Codex rollouts, omp/omo/pi/devin/opencode SQLite…), falling back to
scrollback heuristics.

Two consequences worth internalizing for N19:

- There is **no token streaming anywhere**. "Freshness" = 2 s polling +
  immediate re-poll on agent-status edges + `pane-status` WS pushes. Long
  transcripts are made cheap by *render-side* engineering (below), not by
  deltas.
- Inputs are deliberately conservative terminal writes: bracketed paste via
  herdr `agent.prompt`, per-pane serialized, with settle delays, and a
  server-side queued-send state machine that **never auto-resumes after
  disconnect** (held/uncertain states instead).

## 2. Communication model

| Link | Mechanism | Notes |
|---|---|---|
| Browser → bridge | REST `/api/*` (≈40 routes, `shared/protocol.ts:56-214` is the whole contract) + one WS `/ws` + SSE only for the multi-PC machine list | Chat reads are HTTP polls with ETag/304; writes are WS frames |
| Send chat message | WS `{type:"submit", id, pane_id, text, payload, delivery: "immediate"\|"queue"}` → ack-correlated `{type:"submit-result", …}` | `submit-result` is *bridge acceptance*, never agent acknowledgement |
| Assistant output | `GET /api/pane/conversation?pane_id=…[&before\|since\|from]` poll, cursor = byte offset into the transcript stream; `history_id` changes → HTTP 409 `history_changed` → client drops pages and reloads | Whole turns, not deltas |
| Agent status | herdr `events.subscribe` (only the collector may use it) → WS `pane-status {pane_id, agent_status, background_tasks?}` broadcast | `AgentStatus = blocked\|done\|idle\|working\|unknown` |
| Interactions | `GET /api/pane/prompt` (parsed from the pane's visible TUI text) + `POST /api/pane/prompt/answer` → converted to `send_keys` arrow navigation; 409 `prompt_changed` when the screen moved on | **Screen scraping** — Shardlane's structured `InteractionBroker` (0.6 §17) is the better owner; do not copy this part |
| Queued sends | `PendingMessage {id, text, state: queued\|sending\|held\|uncertain}` + `pending-action {steer\|discard}`; turn gate dispatches when the agent goes working→idle | Disconnect/lease loss ⇒ held/uncertain, with copy-out escape hatch |
| Terminal | WS `pty-data` with cumulative `pty-ack` flow control (pause 256 KiB / resume 64 KiB), 256 KB replay tail for late joiners, `observe` role | Shardlane replaces this wholesale with the official MyGo Terminal attach |

Bridge → Herdr RPC inventory worth knowing: `session.snapshot`,
`pane.*` (read/send_text/send_keys/split/close/…), `agent.start`,
`agent.prompt`, `agent.rename`, `worktree.*`, `events.subscribe`.
Socket discovery: `HERDR_SOCKET` env else `~/.config/herdr/herdr.sock`;
`ping` → `{version, protocol, capabilities}`. One connection per RPC call;
pooling forbidden. Auth (browser↔bridge only): paired-device cookie /
`HERDR_WEB_TOKEN` Bearer / loopback; mutating POSTs need same-origin +
machine headers; herdr socket itself is trusted local IPC.

## 3. Protocol shapes worth referencing

The wire contract is hand-written in `shared/protocol.ts` (156 exported
types). Key shapes (abridged):

```ts
interface ConversationResponse {
  history_id?: string;
  source: "claude-transcript" | "codex-transcript" | … | "scrollback";
  turns: ConversationTurn[];
  metadata?: { model: string | null; reasoning_effort: string | null;
               context?: { used: number; window: number | null } };
  cursor?: string | null;                       // byte offset, paging key
  abandoned?: { count: number; branches: number; summary: string | null };
}
interface ConversationTurn { role: "user" | "assistant"; ts: string | null;
                             end_ts?: string; parts: ConversationPart[] }
type ConversationPart =
  | { kind: "text"; text: string; phase?: "commentary" | "final_answer" }
  | { kind: "thinking"; text: string }
  | { kind: "tool"; name: string; summary: string; input: string; output: string;
      error?: boolean; output_ref?: string;            // lazy whole-output fetch
      images?: { media_type: string; ref: string }[] }
  | { kind: "image"; media_type: string; ref: string }
  | { kind: "compact"; text: string }                  // compaction summaries
  | { kind: "notice"; text: string; source?: string }
  | { kind: "task_result"; tasks: OmoTaskResult[] }    // subagent results
  | { kind: "skill"; skill: SkillActivity };

interface InteractivePrompt {
  id: string; agent: string;
  kind: "question" | "approval" | "plan" | "menu";
  title: string; question: string; body: string | null;
  options: InteractivePromptOption[];                  // descriptions, (Recommended) tags
  multi_select: boolean; custom_option_index: number | null;
  queued?: "collapsed" | "open"; steps?: InteractivePromptStep[];
}
interface PromptAnswer { pane_id: string; prompt_id: string;
                         option_index?: number; option_indices?: number[];
                         custom_text?: string }
```

Takeaway for the N19 bridge: this turn/part taxonomy is the *right grain* for
a chat transcript (text/thinking/tool/image/notice/task_result), and the
`cursor`+`history_id`+409 triad is the right history-integrity model. In
Shardlane these shapes must be produced by the existing Go domain
(`internal/agent`, Herdr runtime, `internal/history` catalog,
`InteractionBroker`) and exposed through the bound bridge — not re-implemented
in TypeScript.

## 4. Chat View feature inventory (frontend, `src/`)

**Transcript rendering**
- Structured turns → `ChatView.tsx` (memoized) with per-turn `Turn` (line
  refs: `src/components/ChatView.tsx:461,531`).
- **Work-block folding** — tools + thinking + narration collapse under one
  live/settled header: "Working… / Needs you / Worked for 7s · 1 edit";
  open while live, fold when settled; failure counts survive folding
  (`src/lib/workBlocks.ts`, `ChatView.tsx:319`).
- **Semantic tool rows**: human verbs ("Ran bun test", `src/lib/toolVerbs.ts`),
  edits as inline diffs, Codex patches as per-file diffs, commands with cwd,
  todos/checklists as icon lists, subagent task cards with meta (agent,
  duration, turns, tokens) (`ChatView.tsx:129-299`).
- Truncated tool output lazily fetched ("Show the whole output (12k
  characters)", `useWholeOutput` → `/tool-output?ref=`).
- Collapsible thinking (settings-gated), skill-activity chips, goal tracker
  card, quiet "notice" dividers, compaction `<details>`.
- Markdown: hand-rolled parser (`src/lib/markdown.ts`) + @tanstack/highlight
  (28 langs) with caps (100 KB chat / 2 KB sync / 20k line elements) and
  **>2 KB blocks highlighted in a Web Worker**; a growing code block keeps old
  colors while the new tail stays plain; KaTeX lazy-loaded on first
  expression; long code folds instead of inner scroll; copy per code block.
- Per-turn keys `paneId:historyId:role:ts` + FNV-1a content-hash
  `turnRevision` as RenderBoundary reset — unchanged turns skip re-render
  across polls; per-turn error boundary (one bad turn renders an inline
  notice, never a blank app).
- Scroll: sticky-bottom within 48 px, "New messages" pill when scrolled up,
  scroll anchoring on prepend ("Earlier messages" + auto-load within 400 px),
  beginning-of-conversation endcap, abandoned-branch honesty banner.

**Composer** (`src/components/Composer.tsx`)
- Auto-growing multi-line textarea with a draggable/keyboard-resizable grip
  (persisted height, capped at half the viewport).
- **One Send/Stop button**: working + empty → Stop (injects Escape into the
  pty = interrupt); typing swaps to Send.
- Attachments: picker + paste + drag-drop → upload → editable `@path ` mention,
  thumbnails in the sent message; max 4/action, 20 000-char cap.
- Slash-command autocomplete grouped by source with local usage-frequency
  memory; `@` file completion (150 ms debounce); ARIA listbox.
- Queued sends (delivery:"queue") with editable held rows + "Send now"/
  "Discard"; server-owned pending queue mirrors states incl. **"uncertain"**
  ("Not confirmed. Check the terminal before sending again").
- Per-pane durable drafts with a 120 s cross-tab send lease and prefix-only
  settle (edits during send are preserved and flagged); restored cancelled
  send refills the box for edit-resend.
- Agent-suggested next message as ghost text (Tab to fill); context-window
  SVG ring (red < 20%); model pill (provider mark, model, reasoning-effort
  dot, measured to fit); usage meters (plan %, "Resets in 2d"); background-
  tasks chip; voice dictation (insert-at-caret, never auto-send); quick-reply
  buttons; terminal-only command hints as status notes.
- **IME correctness**: composition guards (`isComposing || keyCode === 229`)
  before send and in prompt custom fields; Enter vs Shift+Enter configurable.

**Interactions** (`PromptCard.tsx` + `src/lib/promptAnswer.ts`)
- Option buttons with number keycaps + "(Recommended)", multi-select +
  Submit(n), custom-text option, approvals/plans require a Confirm step,
  per-occurrence remount on re-prompt, 409 → re-read, `role="alert"`.
- Rendered via **portal into a dock above the composer** (own `aria-live`),
  outside the scroll log. Answering **by typing in the composer**: "2"/"y"/
  label maps to an option (`answerFromText`).
- Focus hand-off after answering decided by input modality (keyboard/mouse
  yes, touch no).

**States & a11y**
- Empty (agent mark + greeting over the composer), loading, disconnected
  ("Reconnecting… message held here, never queued"), per-pane chat crash
  boundary ("The chat can't be shown. The terminal still works.").
- `role="log" aria-live="polite"` transcript; `role="status"`/`role="alert"`
  semantics; `prefers-reduced-motion` respected; tokens-only theming
  (`data-theme` overrides, no color literals) + density + chat font settings.
- Not-while-open notification guard, system notifications + web push +
  in-app cards + alert sounds with cross-tab de-duplication.

**Session model**: a conversation = one herdr pane running an agent TUI;
selection is `{machineId, paneId}`; header has a per-pane remembered
Chat | Terminal segmented switch; sidebar shows agent marks + rolled-up
status. Notably **absent**: unread counters, review states, retry/edit-resend
buttons, agent handoffs (Shardlane already owns richer product semantics
here).

## 5. Mapping to Shardlane N19 (WebView Chat)

**Transfer as-is (frontend, in the chat WebView):**
work-block folding with live/settled titles; semantic tool rendering + lazy
whole-output; one Send/Stop button + interrupt semantics; held/uncertain
queue UX with "Send now"/"Discard"; PromptCard-style interaction dock above
the composer + typed-answer mapping; dual copy (MD/TXT) + per-code-block
copy; sticky-bottom + "New messages" pill + scroll anchoring; durable drafts
with send lease; `@path` attachment mentions; IME guards; context ring +
model pill; a11y roles and tokens-only theming; per-turn content-hash render
skips; per-turn error boundary.

**Replace with the framework / existing domain (do not port):**
- WS + REST + SSE + 2 s polling → one bound service (`mygo.Bind`) with
  `*mygo.Channel` streaming for deltas/reconciliation and typed events for
  status/interactions. Streaming becomes *possible*; whether token deltas or
  turn-granular updates flow is the bridge's design choice in Phase 1 —
  turn-granular + status-edge pushes is the proven baseline, deltas are the
  upgrade.
- Transcript-store parsing in TS → conversation data produced by the Go
  domain (`internal/agent` + Herdr runtime; `internal/history` for the
  read-only catalog) and exposed via bridge methods. No new parsers, no
  socket access from the page.
- Screen-scraped `InteractivePrompt` → Shardlane's structured
  `InteractionBroker` (0.6 §17) over the bridge: better source of truth, no
  TUI text parsing.
- Node-PTY sidecar + flow control → official MyGO Terminal attach (unchanged
  N19 boundary; the chat WebView never touches PTYs).
- Cursor/history paging → keep the `cursor` + `history_id` + 409-reload
  *model*, implemented over the Go catalog's own pagination.

**Deliberately not adopted:** multi-PC/SSH machine relay and web-push
(device management is out of Shardlane scope); voice dictation and usage
meters only if a later task asks for them (they need provider-backed
endpoints Shardlane does not expose).

## 6. Suggested feature cut for the first WebView Chat (ordered)

1. Structured turns + work-block folding + semantic tool rows + lazy output.
2. Composer: one Send/Stop, IME guards, durable drafts, queue-with-confirm on
   disconnect.
3. Interaction dock (from InteractionBroker) above the composer + typed
   answer mapping.
4. Sticky-bottom/"New messages"/scroll anchoring; per-turn memo/hash + error
   boundary.
5. Markdown + code (highlight with caps, copy, fold) + KaTeX lazy-load.
6. Status edge → immediate reconcile (typed events), context ring + model
   pill; dual copy.
7. `@path` attachments (paste/drop) if the launch service already accepts
   them; otherwise defer.

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
