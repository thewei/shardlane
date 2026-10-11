# moshi (Kyutai) chat view reference audit — 2026-10-11

Status: **Reference audit, feeding the Chat View improvement list** (N19 and
beyond). Scope: the chat/conversation UI of moshi — Kyutai Labs' full-duplex
spoken-dialogue AI. Note: `moshi.app` does not resolve; the product demo is
**moshi.chat** and the client source lives in
[`kyutai-labs/moshi`](https://github.com/kyutai-labs/moshi) under `client/`
(MIT, audited via public sources on 2026-10-11). Same Godiff rule applies:
**UI/behavior reference only; no code import** into Shardlane.

## 1. What moshi's client actually is (verified from source)

A Vite + React 18 + TypeScript + Tailwind single-page app whose one page is
`client/src/pages/Conversation/`. It talks to a GPU server over **one
WebSocket** (`/api/chat`) carrying binary Opus audio frames (20 ms, 24 kHz
mono, streamed via opus-recorder's AudioWorklet) plus a small set of typed
control/text messages (`client/src/protocol/types.ts`): `handshake`, `audio`,
`text`, `coloredtext` (text + palette index), `control` (start/endTurn/
pause/restart), `metadata`, `error`, `ping`.

The defining property: **full-duplex voice**. Mic audio is streamed
continuously; interruption ("barge-in") is a *model* behavior — the client
gates nothing (`useUserAudio.ts` just forwards chunks; no mute/pause logic).
Practical latency ≈200 ms. The text transcript is a byproduct of the audio
stream, not the primary medium — the inverse of our situation.

## 2. Functional block map (Conversation.tsx)

One full-screen CSS grid (`main-grid h-screen`), four visible blocks plus
dialogs below the fold:

| Block | Component(s) | Behavior |
|---|---|---|
| Controls row | inline in `Conversation.tsx` | Start/Connect-Disconnect button, colored status dot (green/red), "waiting for a slot" queue message; "Start Over" appears when the conversation ended (persists params to localStorage, reloads page — iOS audio reset) |
| Player panel | `ServerAudio` + `UserAudio` | The AI's voice playback and the mic capture, each with a canvas spectrum visualizer (`AudioVisualizer.tsx`: 140 frequency bins from an `AnalyserNode`, 3 px gradient bars, rAF loop); plus session **recording download** (canvas@30fps + audio destination → MediaRecorder → webm, with an mp4-conversion hint) |
| Transcript panel | `TextDisplay.tsx` | Flat token stream, one `<span>` per word token; **speaker attribution by color only** (11-color diverging palette, index from `coloredtext`); current token `font-bold`; smooth auto-scroll to bottom on every token; no timestamps, no message boundaries, no bubbles |
| Stats panel | `ServerAudioStats` (md+ screens) | Live playback/latency numbers from the server audio path |
| Dialogs | `ModelParams.tsx`, `ServerInfo.tsx` | Sliders for text/audio temperature & top-k, padding multiplier, repeat penalty (+window), image max-side; **all disabled while connected** (set-before-connect contract), Reset/Validate buttons, persisted to localStorage |
| Turn controls | `Controls.tsx` | Manual **EOS/BOS** buttons (end-of-stream / begin-of-stream control frames) — explicit turn-boundary control on a full-duplex stream |

## 3. What is genuinely worth borrowing

Ranked by value to Shardlane's Chat (frontend/src/App.tsx + chat_bridge.go).
All are **client-local** — none need Herdr protocol changes.

1. **Dictation level meter (live mic visualization).** We already have
   voice dictation (Whisper via `chat.transcribe`), but it is
   push-and-wait: while recording, the composer shows no evidence the mic is
   live. Moshi's AnalyserNode → bar spectrum is cheap and makes dictation
   feel responsive and trustworthy. Adoptable inside the WebView with
   `getUserMedia` + `AnalyserNode` during recording only (no new bridge
   surface), or a native meter later. **High value, low cost.**
2. **"Interrupt and send" affordance (the barge-in analog).** Moshi's whole
   UX thesis is *the user can always cut in*. Our send-while-working path
   queues silently. A visible secondary action on the composer —
   "Interrupt & send" (ESC via `agent.send_keys` + immediate send) when
   disposition is working — imports that agency into a turn-based world.
   **High value, small surface** (disposition is already in
   ChatBridgeState).
3. **Streaming micro-feedback on the assistant text.** Current-token bold +
   fade-in of newly appended text (moshi bolds `text[length-1]`). We publish
   snapshots coalesced at 120 ms; adding a CSS animation on the trailing
   chunk of the streaming turn makes the transcript feel alive without any
   protocol work. **Medium value, trivial cost.**
4. **Local turn stats.** Moshi shows live stats panels. Without touching
   provider plan APIs (the recorded usage-meter deferral), we can compute
   per-turn elapsed time and rough tokens/s from text-delta timing
   app-side and surface them in the existing context-ring/model-pill row.
   **Medium value, medium cost.**
5. **Set-before-connect contract for per-conversation controls.** Moshi
   locks its params dialog while streaming and persists to localStorage.
   If we grow per-conversation controls (e.g. model/dir overrides), copy the
   *contract*: editable only while idle, persisted per conversation, Reset
   button. Pattern only — nothing to adopt today.

## 4. What to skip, with reasons

- **Color-only speaker attribution in a flat token stream.** Elegant for
  one voice, wrong for coding agents: our structured turns (markdown, code
  highlight, work blocks, interaction cards) are strictly richer. Moshi's
  own textColor array can drift out of alignment with the text array
  (`useServerText.ts` appends colors only for `coloredtext`) — a latent bug
  our turn model makes impossible.
- **Force smooth auto-scroll on every token.** Moshi always scrolls to the
  bottom, even while the user reads up. Our jump-pill + scroll-lock is the
  better behavior; do not regress toward theirs.
- **Session recording to webm/mp4.** Not applicable to terminal-runtime
  agents; our Export Markdown (shipped) covers the durable-artifact need,
  and logging rules discourage capturing conversation bodies to files.
- **EOS/BOS manual turn buttons.** Meaningful only for a full-duplex audio
  stream. Enter-to-send + Stop=ESC already cover turn boundaries.
- **Model sampling sliders.** Temperature/top-k for a local speech model is
  out of reach for TUI coding agents without inventing Herdr protocol
  methods — forbidden. (Provider-adjacent knobs were already deferred with
  the usage meters.)
- **Queue/slot-waiting page and email auth.** Demo infrastructure for a
  shared GPU; Herdr instance bootstrap states cover our equivalent.

## 5. Lessons from their defects (avoid list)

- Unbounded transcript arrays (`text`/`textColor` never pruned for the life
  of the socket) — we window via `chat.older` paging; keep the DOM bounded.
- Stale counters across reconnects (`totalTextMessages` not reset with
  `text`) — reset projection state atomically on conversation switch (our
  `publishChatBridge` already rebuilds wholesale; keep it that way).
- "Start Over = page reload" as a state reset — fine for a demo, wrong for
  a desktop client holding panes; conversation resets must go through the
  runtime (new/resume via Sessions), never by restarting the UI.
