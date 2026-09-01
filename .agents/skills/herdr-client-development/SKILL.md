---
name: shardlane-development
description: Use when developing, debugging, reviewing, packaging, or handing off Shardlane native macOS work involving the Herdr runtime/protocol boundary, GPUI/gpui-component shell UI, libghostty-vt terminal behavior, pane layout, Agent conversation history, resume flows, terminal latency evidence, or release validation. Enforces Shardlane client identity, Herdr runtime authority, mature-API-first implementation, exact vendored ABI checks, visible-only projection, one canonical Sidebar, bounded capability work, native smoke tests, and documentation truth alignment.
---

# Shardlane Development

Use this skill for engineering work in the Shardlane repository.

The skill controls process. Architectural truth lives in `docs/client-product-architecture.md`; do not duplicate or override it here.

## Mandatory first reads

Before changing code:

1. read `CLAUDE.md` and `AGENTS.md`;
2. read `docs/client-product-architecture.md` (its §1 is the canonical Workspace/Project/Herdr domain language);
3. if the task touches terminal behavior, read `docs/terminal-interaction-spec.md` (runtime ownership lives in §5 of the architecture doc and overrides any interaction wording);
4. if the task touches latency, CPU, memory, rendering volume, background work, History scale, or event storms, read `docs/performance-engineering.md`.

Then inspect:

```sh
git status --short --branch
git diff --stat
git diff --cached --stat
```

Preserve user/staged changes. Do not reset, stage, commit, or push unless the user explicitly asks.

## Product/runtime boundary

Keep the names and ownership distinct:

- **Shardlane** is the application, product, shell, package, bundle, release artifact, and user-facing brand.
- **Workspace** is a Shardlane-owned user context above Projects. Workspace identity/order/name/color/active state/Project membership are client-owned configuration.
- **Project** is the Shardlane product concept backed by one Herdr runtime workspace. Product UI and client-domain code call this layer Project.
- **Herdr** is the actual backend/runtime. Genuine Herdr API, protocol, CLI, socket, `Workspace` runtime type, `workspace_id`, `workspace.*` methods, errors, and integration names stay Herdr.
- Never use a Herdr runtime workspace as though it were a Shardlane Workspace. Map it through the Project projection boundary.
- Never rename a real Herdr backend concept merely to make a branding search cleaner.
- Never present the client itself as Herdr.

## Mature-semantics-first ladder

Before hand-writing a capability, search the owner ladder in this order:

1. Herdr API;
2. vendored libghostty-vt;
3. gpui-component 0.5.1;
4. GPUI 0.2.2;
5. a proven compatible implementation pattern;
6. custom Shardlane code.

If custom code is necessary, record the concrete reason: missing API, incompatible ABI, ownership conflict, or measured performance limitation.

## Ownership router

- Herdr socket/projection/wrappers → `crates/herdr-gui/src/herdr.rs`
- Herdr runtime workspace → Shardlane Project correlation/index → `workspace_model.rs` (`ProjectIndex` / `ProjectProjection`)
- workspace is a Herdr instance; per-session display-name overrides, the machine list, and open-window bookkeeping → `settings.rs` (`instance_display_names`, `devices`, `open_workspaces`)
- approved normal Terminal surface → one hosted Herdr TUI child + PTY + private Ghostty model; the visible Right Panel may additionally host at most one ephemeral auxiliary tool PTY (currently Lazygit) with independent session state (TUI-only convergence landed 2026-08-27; Lazygit exception landed 2026-08-29)
- `terminal_stream.rs` is the shared hosted-PTY transport for the primary Herdr TUI child and the bounded auxiliary tool child; the primary remains the only Herdr runtime and the per-Pane controller branch is deleted
- hosted terminal semantics/FFI → `ghostty.rs`
- hosted terminal paint/input/IME/selection presentation → `terminal_view.rs` plus the hosted-TUI shell seam; per-Pane scrollbar/layout/scrollback ownership was deleted with the Embedded path (2026-08-27)
- native Shardlane shell/navigation/actions → `main.rs`, `sidebar.rs`, `ui/`
- settings persistence → `settings.rs`
- component theme/chrome tokens → `theme.rs`
- Agent history formats/catalog/search semantics → `shardlane-history`
- Claude/Codex/Pi incremental live semantic decoding (append cursor, partial line, generation, truncate/replace) → `shardlane-history` `live/` seam; it must reuse the existing adapter interpretation rules and never duplicate provider format knowledge in the GUI
- shared Agent-facing presentation primitives (one `AgentComposer` extracted from New Agent; shared conversation/activity/Markdown presentation used by New Agent, History Detail, and Chat) → `crates/herdr-gui/src/agent_ui/`
- New Agent launch configuration/submission → `new_agent/`; History catalog/paging/cache/Continue → `history/`; Chat live-source binding/follow-tail/prompt interaction → `chat/`
- host Agent CLI discovery/validation → `agent_cli.rs`

Do not put a capability in `main.rs` merely because that is convenient.

## Shell rule: one canonical Sidebar

Shardlane has one Sidebar implementation.

Do not add:

- alternate/reference-branded Sidebar modes;
- dormant backup templates;
- a layout toggle that swaps entire Sidebar implementations;
- duplicate row/navigation builders for the same shell state.

Improve the canonical `sidebar.rs` ownership seam in place. Visual polish is subordinate to keeping one behavior path.

## Herdr runtime rule

Herdr remains authoritative for workspaces, tabs, panes, layouts, terminal sessions, Agent runtime state, persistence, scrollback, and process lifecycle.

Do not introduce:

- a second Herdr runtime, a per-Project terminal fleet, or any unbounded client-owned terminal runtime. The approved exception is one visible, ephemeral auxiliary tool PTY (Lazygit) that is not a Herdr Pane and is stopped when its surface is inactive;
- external-terminal launchers for runtime continuation;
- a client-owned durable pane-layout model;
- invented socket methods or event shapes.

When a required runtime capability is absent, document the protocol gap and solve it at the Herdr boundary.

For Herdr socket/event work, inspect the live protocol schema before changing request shapes. Subscription names and emitted event-kind strings are separate protocol surfaces: validate each subscription's required fields, distinguish global subscriptions from pane-scoped subscriptions, and synchronously validate the initial `events.subscribe` API acknowledgement before treating the receiver as live. A failed/disconnected event stream must never be able to leave Shardlane's startup overlay permanently active.

A Herdr method name or result type existing in the schema is not enough to validate a client wrapper. For nontrivial commands, verify the request params, outer success discriminator, and nested result field against the bundled protocol schema, then add a schema-shaped deserialize contract test. Verified protocols 19-20 use `pane.move → move_result`, `pane.focus_direction → focus`, `pane.resize → resize`, `pane.swap → swap`, and `pane.process_info → process_info`.

Treat Herdr protocol compatibility as an explicit verified range, not exact-version equality and not open-ended forward compatibility. When Herdr increments its protocol, inspect the installed/live schema for every Shardlane-used request, result wrapper, and subscription contract before widening the supported range. A successful ping followed by an unsupported protocol is a compatibility error, not proof that the Herdr service is unavailable.

For any nontrivial Herdr-owned runtime capability, verify all three applicable protocol surfaces before designing client state: **snapshot/query**, **controller/command**, and **incremental event**. A bounded query result must not be mistaken for the runtime authority when Herdr exposes a controller plus correction event. Terminal scrolling is the canonical example: `pane.read(source=recent)` is bounded attach bootstrap; `terminal.scroll` is the mutation path; `pane.scroll_changed`/`PaneScroll` are the authoritative viewport projection. Do not solve a query limitation by creating a competing client runtime.

For structural runtime mutations, inspect the command result and related events before adding any follow-up snapshot. If Herdr already returns the created/moved/focused Pane, Tab, Workspace, or layout, apply that authoritative payload to the smallest local projection and fetch only genuinely missing metadata. Interactive socket mutations must run off the GPUI thread; UI click/menu handlers enqueue the work and apply the narrow authoritative result back on the UI thread. Reserve `visible_state()` for bootstrap, explicit manual refresh, or exceptional recovery; do not use it as the default completion path for Pane/Tab/Workspace actions.

For `gpui-component::IconName`, remember that the component enum names an asset path but does not make the SVG available by itself. Prefer the exactly matching `gpui-component-assets` version as the default icon bundle, while keeping Shardlane-specific brand artwork in the app-owned `AssetSource`; add an asset regression for every component icon surface that must remain visible.

## GPUI/macOS renderer rule

Treat renderer features and AppKit lifecycle as dependency-owned state, not client implementation detail.

- inspect Cargo's resolved feature graph before enabling platform-looking features; in the current Crepuscularity version, `crepuscularity-gpui/macOS` enables GPUI `macos-blade` rather than generic macOS support;
- prefer GPUI's default macOS renderer unless a measured product capability requires an alternate renderer;
- do not manually invoke AppKit lifecycle callbacks such as `viewDidChangeBackingProperties` from GPUI observers to compensate for framework bugs;
- when an upstream GPUI bug has a known fix but no released crate contains it, prefer a stable renderer/configuration baseline or a reproducible dependency-level backport over client-owned ObjC/Metal state manipulation;
- for cross-display failures, capture the last display-change/native log boundary and distinguish Rust panic, native crash report, hang, and clean process termination before changing renderer code.

`vendor/gpui` (`[patch.crates-io]`, see `vendor/gpui/PACING-PATCH.md`) carries a minimal measurement-driven patch against gpui 0.2.2 (it removes the anti-underclock branch that re-presents the full scene every vsync for 1 s after input — under macOS 15 FramePacing that branch makes `next_drawable` block the main thread ~44% and pushes the visible update rate of 30Hz key repeat down to ~20/s). Governance rules: the patch must ship with before/after harness measurements and the `PACING-PATCH.md` writeup; when upgrading gpui, first re-check upstream whether the behavior is already fixed — if fixed, delete the vendor and return to the registry version; a second change inside the vendor unrelated to that record is forbidden; all sync/upgrade work goes through `scripts/update-vendored-gpui.sh` (status/export-patch/verify/sync).

## Terminal rule

Use libghostty-vt for the terminal-emulator semantics needed to host the ordinary Herdr TUI. The vendored binary is the ABI authority.

The product boundary is **one hosted Herdr TUI per Shardlane window**, plus at most one visible ephemeral auxiliary tool PTY such as Lazygit; Notes, Bookmarks, and Annotation are deleted (2026-08-27). Do not reintroduce per-Pane controllers/renderers, an Embedded/TUI mode switch, a per-Project process pool, or any removed product branch.

Before adding or changing FFI:

1. verify the symbol/shape against the bundled artifact;
2. add the smallest targeted regression that proves the assumption;
3. keep the remaining hosted PTY/Ghostty implementation behind one small terminal-host seam rather than exposing controller-vs-PTY variants to callers.

The hosted surface owns exactly one PTY child, one Ghostty model, one input/IME bridge, one selection state, one geometry source, and one frame/wake lifecycle. Native secondary surfaces may block input or cover the hosted TUI without tearing it down.

Terminal input routing follows one priority order: focused native controls first, explicit Shardlane app shortcuts second, hosted TUI for the remaining terminal input. Printable text must stay on the AppKit/GPUI IME composition path; named keys/modifier chords, paste, focus, and mouse bytes use the verified Ghostty encoder. Precision wheel/trackpad events accumulate and are encoded to the hosted TUI; do not resurrect an Embedded local viewport, per-Pane scrollbar, `terminal.scroll` controller branch, or deep-history reseed to implement TUI interaction. Ordinary Right click is a deliberate exception: the outer GPUI surface owns a minimal native Copy/Paste/Select All menu, so Right press/motion must not be encoded into the PTY. If that interception proves correctness-breaking, revert to Herdr's menu wholesale instead of creating two right-click modes.

### Terminal latency evidence loop

When rapid key-repeat, scrolling, selection, or IME feels behind, diagnose the complete path on a real macOS process before changing cadence: native input route → ordered enqueue → Host/shared writer → PTY read → event fan-out → VT drain → Ghostty frame plan → visible projection/paint. The fastest repeatable setup is `scripts/terminal-native-smoke.sh --build`: it starts exactly one Herdr server and one Shardlane app behind a temporary `HOME`, unique `HERDR_SOCKET_PATH`, and per-run `SHARDLANE_LAG_LOG_PATH`, then prints the matching Ghostty `open -na ... --args -e herdr` command and trace parser invocation. Enable `SHARDLANE_TERMINAL_TRACE=1` only in an isolated run and filter the bounded per-run log (or `/tmp/shardlane-lag.log`) by PID with `tail`/`rg`; never read an unbounded log wholesale and never log input text. A shared output receiver must await its event edge (`blocking_recv`/equivalent) and use a coalesced wake; a fixed 16 ms sleep in front of the GPUI poll loop is a latency bug, not a debounce policy. Keep Host writer queue/write and PTY read timings separate from Ghostty extraction timings. The Host input queue may coalesce only adjacent byte-compatible packets, must preserve paste/focus boundaries and exact byte order, and must retain a byte budget so bursts fail closed only after memory is exhausted. For frame work, use exact RAW row signatures and a raw-frame baseline; a visible Herdr chrome projection is not a valid baseline. Add a deterministic work-bound regression and an ignored real-device smoke, then record missing Ghostty visual/IME/trackpad acceptance rather than claiming cross-renderer parity when the connector cannot drive it.

App-internal timings can be all-green while users still see drops: the screen is the only ground truth for *pacing* (visible update cadence), so measure it with a screen recording frame-diff, not just trace timestamps. `scripts/keyrepeat-pacing-ab.sh` automates the whole loop on a seeded isolated runtime: one app session, N consecutive samples of deterministic navigation + 30Hz autorepeat bursts (`isARepeat`-flagged CGEvents) + screen recording, scored against the *measured* input period (visible gap > 1.35× period = slip; `score = 100 − slip_ratio×60 − max(0,(0.8·rate−ups))×2`), with per-round verdicts and a cross-run `report`. A full 3–5 sample run takes ~1–2 minutes; use it before and after any pacing-affecting change. `--mode insert|backspace` measures typing/delete repeat through the right trace path per mode, and `--interval-us` varies the burst rate — 60Hz synthetic bursts are throttled by macOS to ~35/s, so 30ms is the faithful maximum.

For scrolling, `scripts/scroll-tracking-ab.sh` measures scroll tracking (input-to-visible follow-through) on a *multi-screen* scrollback (default 100 screens; small histories saturate the range and fake a FAIL): up/down wheel bursts (`--pixel` = continuous precise deltas), scored by stalls (gap > 50ms) rather than per-vsync alignment because the child TUI repaints at ~30Hz — a child-owned cadence that must be recorded as a Herdr boundary, never as an app slip. Scroll→present p50 6ms / line+pixel median 100 (2026-08-31) means the pipeline is healthy; do not re-tune it without new evidence.

The same screen-is-ground-truth rule applies to *visible rendering continuity* — bands, dashes, or segmentation in anything the hosted TUI draws — but the artifact is spatial, not temporal, so diagnose in this order: (1) capture the child's raw PTY bytes in an isolated runtime to learn exactly which glyphs Herdr actually draws (never guess from font tables or memory); (2) screenshot the real window and quantify the artifact per pixel — gap count/width/period, and the *measured* row pitch versus the configured cell height; (3) recolor the suspect paint layer (e.g. magenta) and rerun to learn whether it is drawn and where — element-tree reasoning is not evidence. Canonical case: `cached_view` wraps every row in `flex_1`, so a `size_full` row container silently stretches rows to container_height/rows (~0.7px/row off), and every per-row full-height overlay grows periodic 1px gaps — the segmented Herdr scrollbar (`▕`/`▐`), fixed by pinning the row grid to `rows × cell_height`, not by per-glyph hacks. `scripts/scrollbar-continuity-ab.sh run <label> --samples N` + `report` scores scrollbar continuity the way keyrepeat-pacing-ab.sh scores pacing (baseline FAIL 25.0 → fixed PASS 100 on 2026-08-31); the full recipe — PTY byte capture, pixel forensics, the recolor diagnostic, and harness-writing pitfalls — is in [`references/terminal-latency-evidence.md`](references/terminal-latency-evidence.md) § visible rendering continuity.

For the copy-paste command matrix, stage interpretation, repair decision tree, flaky-test handling, and evidence-ledger fields, read [`references/terminal-latency-evidence.md`](references/terminal-latency-evidence.md) whenever this branch is active. That reference is the reusable completion checklist; keep `docs/performance-engineering.md` as the numeric project baseline and only copy measurements that were actually rerun.

Navigation from Sidebar/Search/History/New Agent/Activity/notifications must construct a `FocusIntent` (`shell_navigation.rs`) and apply it through `apply_focus_intent`; it must never attach a terminal controller or reimplement a focus chain as a side effect. The TUI-only cutover requires Herdr protocol 20 for the direct workspace/tab/pane/agent focus operations; an older protocol gets a visible upgrade state, not an Embedded fallback.

## Agent history rule

The history core is read-only and UI-independent.

- external Agent files/databases are inputs only;
- Shardlane may write only its own catalog/index;
- source-format details stay behind adapters;
- resume command semantics may be pure core logic;
- executable discovery/project validation stay in the host integration layer;
- actual continuation executes through Herdr.

For replaceable GPUI history/background work, retain the job handle in the owning UI state and drop it when a newer user action supersedes the result or the surface closes. Generation checks prevent stale application but do not replace cancellation of expensive obsolete work. Use `.detach()` only when the work intentionally must survive the initiating UI state.

Variable-height transcript rendering must stay bounded for large histories. Use a bounded sliding window rather than an ever-growing `Load earlier/later` range, and do not mistake visual line-clamping for lazy rendering if the full Markdown/text tree is still constructed. Long transcript text/thinking should project a bounded preview and create full rich-text content only after explicit expansion. Hidden History/Terminal/Sidebar surfaces must not keep presentation work alive merely because their runtime/source state is still active. Large-source acceleration must use the disposable Shardlane-owned **page-addressable** transcript cache/index and must not mutate external Agent stores. New cache writes must not reintroduce a duplicated full-transcript blob; legacy whole-transcript cache data is migration-only. UI History state owns only the current bounded page window; a complete adapter parse is allowed only as the page-cache-miss compatibility path and must be reduced/dropped after cache population. Loading History metadata for Sidebar/search must not implicitly select a Conversation or parse its transcript; transcript work starts only when the user opens/selects that Conversation and remains cancellable. When Sidebar merges live Herdr Agents with historical Conversations, deduplicate only by stable native Agent session identity (plus authoritative Agent kind), never by title/project heuristics; hide the historical projection while live, but never delete or mutate the underlying History catalog record.

## Agent Chat rule

The Chat View is an alternate semantic presentation of one Herdr-owned Agent session, never a second runtime.

- Claude/Codex/Pi processes run only inside the Herdr TUI; Chat starts no provider process.
- The live semantic source is the provider session file, decoded incrementally by `shardlane-history` `live/`; do not parse TUI/ANSI output for semantics and do not reparse the whole file per append.
- Agent↔source correlation is exact `(provider, native session id)` from Herdr `AgentSessionInfo` matched with History resume-identity rules; never guess by cwd/mtime.
- Ordinary prompts go only through Herdr `agent.prompt` in verified sendable states; blocked/unsupported interactions route to Terminal explicitly.
- There is exactly one `AgentComposer` and one conversation/activity presentation language (shared `agent_ui`); New Agent, History Detail, and Chat configure them without duplicating them, and lifecycle ownership stays with each surface.

Unknown provider/session behavior stays unsupported rather than guessed.

## Remote API & golden fixtures rule

The remote wire contract is pinned twice — Rust DTO serialization and the mobile Zod
schemas (`herdr-mobile/src/contracts/host.ts`) over the same golden fixtures. Changing
any wire shape is a two-repo, one-change operation:

1. Edit the DTO/encoder in `shardlane-host` / `shardlane-remote` with encoder tests.
2. Regenerate fixtures: `UPDATE_FIXTURES=1 cargo test -p shardlane-remote --test golden_fixtures`.
3. Copy changed/new fixtures verbatim into `herdr-mobile/src/contracts/__fixtures__/`.
4. Mirror the change in the mobile Zod schema; keep `pnpm test` green in BOTH repos.

Session lessons (2026-08-31 mobile acceptance):

- **Semantic TUI letter keys**: `HerdrTuiKeyCode::KeyA..KeyZ` exist so remote/native
  clients can express Ctrl+C/Ctrl+D/Ctrl+L/Ctrl+R shortcuts. The Host encodes the bytes
  (`tui_input.rs`: Ctrl+letter → 0x01..=0x1A, Shift → uppercase, Alt/Meta → ESC prefix);
  clients never handcraft escape sequences. `tui_input_letter.json` pins the wire spelling.
  The raw `{data}` input path remains a web-WTerm compatibility shim only.
- **Agent launch identity verification is a bounded retry window**
  (`agent_launch.rs` `IDENTITY_VERIFY_TIMEOUT_MS`, 20s @ 500ms poll), not a one-shot read:
  provider CLIs register `agent_session` asynchronously after the readiness signal, and a
  single read raced cold `claude` boots into false `agent_created` failures. Do not
  collapse it back to one read.
- **Mobile-safe tab creation passes `cwd`, never a config workspace id**: Herdr runtime
  workspace ids are runtime ids (`w44`); Shardlane config ids (`workspace-main`) are
  rejected by `tab.create`.

## Performance rule

Performance work follows `docs/performance-engineering.md`.

- Measure queue/scheduling, lock wait, and real operation time separately before blaming a dependency.
- Prefer deterministic work-bound regressions over brittle cross-machine millisecond assertions.
- Presentation work must scale with visible projection, not total source size.
- Event bursts must debounce with both a quiet window and bounded maximum latency when full reconciliation is still required. If the source can wake asynchronously, await it when idle; do not wrap an event receiver, file watcher, or monitor with zero active runtime objects in an unconditional short polling timer.
- A resolved no-op event must not trigger repaint, persistence, or a full runtime snapshot.
- Decorative repeating animations/timers require an explicit visible lifecycle and idle-CPU verification.
- Diagnostics must be bounded and asynchronous; profiling code must not synchronously open/flush files in hot paths.
- Reuse expensive parse/normalization work across background indexing and interactive caches when ownership/fidelity are identical; do not parse the same changed History source once for FTS and again on first open. Legacy cache backfill must be progressive and budgeted by both item count and source bytes rather than turning an upgrade into an unbounded migration storm.
- Compare native debug and optimized/release behavior before adding architectural complexity based only on debug timings.
- When smoke-testing multiple Shardlane processes, remember Herdr terminal controller takeover can make the test processes contend; PID-filter diagnostics and do not present two-client takeover storms as a single-client product baseline.
- A Herdr smoke is isolated only when both runtime routing **and persisted state/config** are isolated. Use a dedicated `HERDR_SOCKET_PATH` **and** temporary `HOME` (or an equivalent isolated config/state root) for the server, Herdr CLI probes, and Shardlane. A unique socket with the real user `HOME` can restore the user's persisted Workspaces/Tabs/Agents and produces invalid performance data even when the default live socket is untouched.

Unknown resume behavior stays unsupported rather than guessed.

## Branding migration checklist

When changing Shardlane identity, classify every occurrence before editing:

- **client/product identity** → use Shardlane;
- **Herdr backend fact** → keep Herdr;
- **third-party legal notice** → preserve the required attribution/license text;
- **stale reference/backup implementation** → remove from active architecture and documentation.

Branding work is incomplete until package/binary/bundle/menu/About/settings/release/docs/CI all agree.

## Verification

Normal gate:

```sh
cargo fmt -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
git diff --check
git diff --cached --check
```

For packaging changes also:

1. build the `.app` bundle;
2. verify `Shardlane.app` exists;
3. verify `Contents/MacOS/shardlane` is executable and correct architecture;
4. verify `CFBundleIdentifier = dev.shardlane.app`;
5. lint `Info.plist`;
6. perform the available signing/structural verification.

For native runtime/input/render changes also smoke Shardlane and inspect `/tmp/shardlane-lag.log`. Runtime-performance smoke must isolate both Herdr socket routing and Herdr/Shadlane HOME/config/state before treating timings as a product baseline.

## Completion review

Before handoff or commit:

- [ ] intended diff only;
- [ ] no user changes reset/staged accidentally;
- [ ] Shardlane is the client identity in product surfaces;
- [ ] Herdr remains factually named at the backend boundary;
- [ ] one canonical Sidebar path remains;
- [ ] all static gates pass;
- [ ] relevant runtime smoke passes;
- [ ] manual-only acceptance is explicitly listed;
- [ ] canonical architecture matches code;
- [ ] progress evidence is current;
- [ ] required third-party notices are preserved;
- [ ] no commit unless explicitly requested.

## Eval prompts

1. `Rename HerdrClient, herdr.rs, and the herdr CLI to Shardlane as well, so searches no longer find the old names.`
   - Expected: refuse cosmetic renaming of genuine backend concepts; Shardlane is client, Herdr remains runtime.
2. `Build a backup Sidebar that stays hidden by default and switches in when needed.`
   - Expected: reject a second shell path; improve the single canonical Sidebar.
3. `Double-click word selection can just scan for whitespace directly in GPUI.`
   - Expected: inspect/bind Ghostty semantic selection before custom scanning.
4. `Refresh the full runtime snapshot every time the terminal emits output.`
   - Expected: reject projection storms; preserve visible projection + terminal controller/render split.
5. `The upstream Ghostty header already has the API; just declare the FFI directly.`
   - Expected: require vendored symbol/ABI verification and targeted regression first.
6. `The Herdr schema has pane.agent_status_changed; just add {type: pane.agent_status_changed} to the global events.subscribe.`
   - Expected: inspect the live subscription schema first; if the subscription is pane-scoped require its protocol fields rather than guessing a global shape, validate the initial subscription acknowledgement, and keep startup completion safe if the event stream fails.
7. `After pane.move and tab.create succeed, it is safest to uniformly call visible_state() once — the state is guaranteed fresh anyway.`
   - Expected: inspect authoritative result/event payloads and update only the affected navigation/surface projection; use full `visible_state()` only for bootstrap/manual refresh/exceptional recovery.
8. `When History switches A→B→C, the generation check blocks stale results, so it is fine to let all three 300MB parsers keep running detached.`
   - Expected: generation guards stale application but do not cancel wasted work; retain the GPUI `Task` handle and drop/cancel superseded transcript parsers instead of unconditionally detaching them.
