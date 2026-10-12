# Shardlane Performance Engineering

Status: **active engineering contract**

Architecture source of truth: `client-product-architecture.md`
Terminal execution guide: `docs/terminal-interaction-spec.md` (current terminal = hosted Herdr TUI)


## 1. Goal

Shardlane performance work must reduce measured user-visible latency or bounded background cost without weakening terminal correctness, Herdr runtime ownership, history fidelity, or recoverability.

Do not accept “feels faster” as evidence. Every performance change should leave at least one of:

- a deterministic work-bound regression test;
- a decomposed latency log;
- a native debug/release smoke measurement;
- a profiler sample identifying the changed hot path.

## 2. Core rules

### 2.1 Visible-only work

Only visible product surfaces should perform presentation work.

A hidden Terminal may continue consuming Herdr/PTY output into Ghostty state, but must not extract `TerminalFrame`, update `TerminalPane`, or trigger root layout merely to keep an invisible surface painted. When an overlay such as History, Settings, Help, Search, About, Rename, Task dialog, or navigation loading blocks the Terminal surface, keep runtime state current and defer presentation until the Terminal becomes visible again. On resume, extract one authoritative current frame and return to normal event-driven polling.

The same principle applies elsewhere: a hidden History transcript, collapsed Sidebar section, stopped Task, background Pane, or off-window result must not keep expensive projection work alive.

### 2.2 Bound work by visible projection, not source size

Do not let UI work scale with total history, total Tasks, total Workspaces, or full retained scrollback when only a bounded visible projection is needed.

Current contracts include:

- History transcript UI materializes at most **60 messages** at once;
- History page navigation uses a **12-message overlap** sliding window rather than ever-growing batches;
- long History message bodies default to at most **1,800 characters / 18 lines** of preview;
- long thinking text defaults to at most **700 characters / 10 lines** of preview;
- full Markdown/thinking content is created only after explicit expansion;
- Task runtime monitoring probes only materialized Tasks with `tab_id + pane_id`;
- stable Task PID/port projections are reused instead of repeatedly invoking `lsof`;
- Terminal input bursts coalesce compatible ordered work;
- terminal presentation updates remain limited to visible Pane ownership.

Visual CSS/GPUI truncation alone is not a performance strategy if the application still clones, parses, lays out, or stores the entire expensive child tree.

### 2.3 Debounce event storms with both quiet-window and max latency

Do not refresh a full projection once per event when an event burst describes one logical state transition.

Use:

```text
first dirty event
  ↓
mark projection dirty
  ↓
wait for quiet window
  ├─ quiet reached → reconcile once
  └─ events keep arriving → reconcile at bounded max delay
```

A quiet-window alone can starve updates during a permanent stream; a fixed interval alone repeatedly refreshes during bursts. Use both where appropriate.

Current examples:

- navigation reconciliation waits for an event quiet window before `workspace.list + tab.list`;
- Agent full-list fallback uses a quiet window while retaining a bounded max discovery delay;
- incremental `pane.updated` Agent patches are preferred over `agent.list`.

### 2.4 No-op updates must remain no-op

Separate:

```text
resolved identity
```

from:

```text
projection changed
```

An event may successfully resolve to an existing Agent/Pane/Task without changing any visible field. Do not `cx.notify()`, rebuild Sidebar/root layout, rewrite JSON, or hit the runtime merely because an event was successfully handled.

### 2.5 Measure queue, lock, and work separately

A wall-clock slow log must not label the whole interval as the underlying library’s work.

For async/background work measure separately:

```text
scheduled_at
work_started
lock_acquired
real_operation_started
operation_finished
result_applied
```

This distinction found a major Terminal latency bug: apparent 36–47 ms “Ghostty extraction” was actually ~43–45 ms of GPUI background-executor queue delay while true Ghostty frame extraction was ~1.8–2.5 ms.

### 2.6 Avoid background hops that do not buy isolation

A 1–3 ms deterministic operation can become tens of milliseconds slower if placed behind a busy generic executor. Use the owner’s current execution context when the operation is short and non-blocking, or use `try_lock + defer` rather than blocking the UI.

For startup/attach work, preserve useful parallelism but remove chained executor hops. History read and controller attach may run concurrently; preload/initial frame extraction should not require a third executor scheduling hop if it can safely continue in the controller task.

### 2.7 Diagnostics must not create the performance problem

Performance logging is asynchronous and bounded.

`crates/herdr-gui/src/diagnostics.rs` owns the shared lag logger:

- one persistent file handle;
- bounded channel;
- non-blocking `try_send` from callers;
- writer thread owns file IO;
- every line includes process PID and sequence number.

Do not synchronously `open → write → flush` `/tmp/shardlane-lag.log` for every runtime call. Do not use unbounded logging in hot loops.

### 2.8 Repeating animations require a lifecycle reason

A decorative repeating animation can keep the GPUI display link and Taffy layout pipeline awake even while the product is otherwise idle.

The previous repeating SwipeHint pulse kept idle debug CPU near ~29%. Removing the unconditional repeat reduced stable debug idle CPU to ~1.5–1.8%. After the Herdr event bridge also became event-driven, an isolated 2026-08-21 release smoke measured **0.6% CPU / ~83 MB RSS** at steady state.

Any repeating animation/timer must answer:

- which visible state owns it;
- when it stops;
- why event-driven rendering is insufficient;
- how idle CPU was verified.

### 2.9 Event bridges should await events when idle

Do not poll an already asynchronous/runtime event source on a fixed short timer merely because the first implementation used `std::mpsc::try_recv`.

The Herdr subscription reader now forwards decoded events through `async-channel`. The GPUI event task awaits the channel indefinitely when there is no pending startup/debounce/reconciliation deadline. An ~80 ms timer is created only while startup completion, navigation quiet-window reconciliation, or Agent reconciliation still needs a deadline check. Runtime events race that temporary timer and win immediately.

The isolated release smoke verified that creating a second Workspace after Shardlane had already become idle still triggered one bounded `workspace.list + tab.list` reconciliation, while stable CPU remained ~0.6%. Preserve this event-driven behavior; do not reintroduce unconditional 80 ms polling.

## 3. History performance contract

History is a read-only projection over the external Agent session files. Shardlane stores no conversation content (2026-10-12): the catalog is an in-memory list-row index rebuilt from the provider files, and every transcript read goes to the original file.

### Current strategy

```text
Launch / explicit refresh (background lane)
  ↓
scanner lists provider roots → (mtime, size) unchanged → keep the in-memory row
  └─ changed or new → adapter parses the file once → keep list-row metadata → drop the transcript
  ↓
unobserved files drop their rows (only after a complete observation)

Conversation opened
  ↓
adapter parses the original file → cut one 60-message window → drop the rest
  ↓
Earlier / Later opens re-read the file for the adjacent window (overlap kept by the caller)
```

`Load earlier` / `Load later` must never grow one range indefinitely: the transcript content alive at once stays bounded regardless of a 2,000-, 10,000-, or larger-message source, and metadata loading never parses a transcript.

### Measured on the reference machine (2026-10-12)

| What | Result |
| --- | --- |
| Cold scan: 460 session files, 316 MB (Pi 223 MB, Codex 55 MB, Claude Code 38 MB), one worker | ≈2.3 s, background lane |
| Same scan on the shipped pool (4 workers; 460 files, 331 MB on a later run) | ≈0.7 s (2 workers ≈1.1 s, 6–8 workers ≈0.6 s; peak process memory 58 MB vs 45 MB) |
| Same scan inside the running app (log `history scan finished`, window shown → rows landed, startup work in parallel; three launches) | ≈1.4–2.4 s |
| Open the largest source (44 MB Pi file) | ≈0.11 s |
| Open a 23 MB Codex rollout | ≈0.06 s |

These costs are why no derived transcript store exists: re-reading is cheaper than keeping, invalidating and migrating a second copy. Parallel parsing is shipped (`maxScanWorkers` = 4; rows are committed in listing order, so the catalog equals a sequential round's). If cold scans ever become the measured bottleneck again, the next lever is a metadata-only persisted index — never a stored transcript or message-body index.

### History performance regressions

Maintain tests for:

- initial transcript window size independent of total message count;
- earlier/later page navigation remaining bounded;
- an anchored open including its target message in the bounded window;
- an open reflecting a changed provider file immediately, without a rescan;
- the scanner re-reading a file whose size changed at an unchanged mtime;
- Unicode-safe long-text preview boundaries;
- obsolete transcript open task cancellation when switching sessions.

## 4. Terminal performance contract

Do not trade correctness for lower CPU.

Keep:

- real font metrics and one authoritative Terminal geometry;
- Ghostty-owned VT state/selection semantics;
- event-driven hosted-PTY wake;
- focused/background adaptive polling fallback;
- ordered input coalescing;
- hidden-surface VT draining without hidden frame extraction;
- release/native measurements.
- recent user interaction (typing / mouse / trackpad) adds **no second software frame gate** after PTY output wakes the host: extract the newest frame immediately and let GPUI/display refresh coalesce paint. This avoids the measured 2026-08-28 aliasing where ~16.7ms wheel events became ~33ms terminal frames;
- non-interactive continuous output remains bounded to the 16ms active presentation cadence, but a frame that arrives before its budget is due waits only the **remaining** budget (for example 7ms elapsed → 9ms retry), never another full 16ms poll interval; only truly idle projection falls back to 100ms;
- macOS precise wheel/trackpad input mirrors Ghostty AppKit behavior: GPUI's raw precise pixel delta is multiplied by 2× before row accumulation; fractional pixel residual survives successive wheel events and is cleared only on gesture end/direction reversal or a non-scroll input boundary.

Current local explicit Ghostty changed-frame smoke at 120×40 is approximately 1.5–1.8 ms/frame on the development Mac. A 2026-08-29 real hosted-PTY 100 Hz repeat-input diagnostic initially measured changed-frame extraction at **p50 5.72 ms / p95 9.24 ms** because one changed cell caused a second full-viewport text/color FFI extraction after the RAW-signature scan. Converting the signature result into an exact changed-row plan reduced the same test to about **p50 1.16–1.31 ms / p95 1.96–2.52 ms**; 119/121 sampled changed frames took the `partial changed_rows=1` path. Keep the full-extract fallback for OSC-8/global-color/unsupported-signature cases. Treat all numbers as local regression baselines, not cross-machine SLAs.

The 2026-08-31 real-device repeat-input investigation decomposed the hosted path instead of attributing the delay to Ghostty. Replaying the legacy shared-viewer `try_recv + sleep(16ms)` loop produced **p50 17.78 ms / p95 18.98 ms / max 19.73 ms** from published output to the viewer queue; the event-driven `broadcast::Receiver::blocking_recv` path measured **p50 ≤0.1 ms / p95 ≤0.2 ms** in a clean run (the final rerun under desktop load was **p50 0.03 ms / p95 2.12 ms / max 5.63 ms**, still below the one-tick gate). In an isolated Shardlane app (temporary `HOME` and `HERDR_SOCKET_PATH`), UI enqueue was typically **2–36 µs**, Host writer queue **8–41 µs**, PTY read-to-forward **5–83 µs**, and VT drain **15–34 µs**. After separating the raw frame from the Herdr chrome projection, unchanged repeats took roughly **0.5 ms** for RAW signatures plus **0.16–0.20 ms** frame application; a one-row change took roughly **0.6–1.1 ms** extraction plus **2.4–3.4 ms** presentation. When OSC-8 forces a conservative full extract, the same pass now immediately reseeds RAW signatures (about **88–100 µs** in the isolated trace), so the next repaint takes the fast path instead of a duplicate full extraction. The trace is opt-in (`SHARDLANE_TERMINAL_TRACE=1`) and records lengths/ids/timings only, never text.

The same run showed occasional **0–20 ms** wake-to-main-thread variance (`poll.wake_signal` → `poll.wake`) while the GPUI foreground executor and display refresh were scheduled; this is scheduling jitter after the transport fix, not a second fixed terminal gate. The direct PTY 100 Hz smoke remains in the low-millisecond range (latest post-fix rerun: **p50 0.71 ms / p95 4.37 ms / max 5.84 ms**; prior runs measured p50 0.64 ms / p95 2.43–3.84 ms / max 5.06–6.19 ms), so treat it as a local regression baseline rather than an SLA. Ghostty's visual A/B could not be automated from this environment (the Computer Use connector rejected the Ghostty app; temporary wrappers did open standalone Herdr TUI windows, but not a synchronized same-state timing harness), so these numbers are a Shardlane/PTY baseline, not a claim about Ghostty's Metal renderer. Manual trackpad momentum, true IME candidate composition, and a visible Ghostty-vs-Shardlane screen comparison remain operator checks.

The repeat-input burst check then isolated a second, independent loss mechanism at the Host seam. A real `herdr` TUI child accepted only **792/2,000** one-byte packets with the old `sync_channel(128)` (1,208 `Backpressure` errors), even though each packet was valid; the bounded replacement queue admitted **2,000/2,000** in **1.76–3.09 ms** across the native reruns (the latest post-fix rerun: **2.04 ms**). Adjacent compatible packets are merged in order, paste/focus packets remain hard boundaries, and a 1 MiB byte budget still fails closed for pathological backlogs. The ignored regression is `shared_tui::tests::shared_tui_burst_input_backpressure_smoke`; the repeatable macOS setup is `scripts/terminal-native-smoke.sh --build`, which isolates `HOME`, `HERDR_SOCKET_PATH`, and `SHARDLANE_LAG_LOG_PATH` and prints the matching Ghostty A/B command. This fixes dropped key-repeat/scroll input; it does not claim a synchronized Ghostty renderer benchmark.

GPUI presentation follows the same row boundary. A direct 120×40 terminal-line element construction smoke measured about **1.39 ms** for rebuilding all 40 rows versus **52.8 µs** for one retained changed row. `TerminalPane` therefore owns cached `TerminalRowPane` entities: ordinary repeat-key echo should notify only the changed content row plus any old/new cursor row; cursor blink/style changes must not invalidate rows without a cursor. Theme/font/selection changes intentionally invalidate the affected/all rows.

Rapid scroll/resize may yield a partial Herdr repaint whose dominant background temporarily falls below the 80% confidence threshold. This is "no new evidence", not a request to fall back to terminal-default black. Preserve the previous confirmed `surface_background` until a new background reaches the threshold. The production-like Ghostty scroll regression covers 100% Herdr background → ~50–70% partial history rows → 100% without `theme → black → theme` flashing.

## 5. Tasks performance contract

Task identity may be numerous; runtime probes must not scale with stopped history.

- Probe set = only Tasks with materialized runtime identity.
- Reuse stable PID/port projection.
- `TaskRuntimeProjection` owns status/PID/ports/start timestamp/transient error and is never serialized.
- Runtime-only projection changes update memory/UI only and do not rewrite `tasks.json`.
- Persist `tasks.json` only when `TaskDefinition` or persisted Herdr correlation (`workspace_id`/`tab_id`/`pane_id`) changes.
- Group Tasks for Sidebar in one linear pass rather than Workspace × Task rescans.

## 6. Profiling workflow

### Step 1 — establish a green baseline

```sh
cargo fmt -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
git diff --check
git diff --cached --check
```

### Step 2 — reproduce with decomposed logging

Use `/tmp/shardlane-lag.log`. Every new diagnostics line must be bounded and useful for deciding ownership or work.

For multi-instance tests, filter by PID and isolate both `HERDR_SOCKET_PATH` and the config/state root. The current TUI-only client no longer owns per-Pane `--takeover` controllers; do not use historical controller-disconnect measurements as the hosted-TUI production baseline.

### Step 3 — profile the real process

On macOS use process CPU/RSS sampling and `sample <pid> <seconds>` when needed. Prefer release measurements for product baselines; use debug builds for development hot-path discovery.

### Step 4 — write structural regression first

Prefer assertions about work volume:

- one changed Terminal row → one changed-row projection;
- 20k compatible inputs → bounded queue entries;
- 2k stopped Tasks → one live Task probe;
- 10k History messages → 60 visible messages;
- long text → bounded preview;
- repeated identical patch → no repaint/persistence.

Avoid brittle unit tests such as “must finish in 4.3 ms on every machine”. Machine-local ignored performance smokes may use a deliberately wide regression ceiling.

### Step 5 — native/release smoke after the change

Compare before/after using the same workload and record:

- CPU;
- RSS;
- operation count;
- queue/lock/real-work decomposition;
- user-visible latency where measurable.

### Step 6 — documentation truth loop

Update:

- this file for new reusable performance lessons;
- feature implementation docs for feature-specific budgets;
- the internal engineering archive with verified measurements only;
- `.agents/skills/herdr-client-development/SKILL.md` when the lesson changes how every future Agent should work.

## 7. Known measured lessons — 2026-08-21

### Terminal frame queue

Before:

```text
total ≈ 45 ms
background queue ≈ 43–45 ms
terminal lock ≈ 0 ms
Ghostty extract ≈ 1.8–2.5 ms
```

After removing the unnecessary generic background hop, the same native smoke no longer produced 30+ ms Terminal frame slow logs while output remained actively consumed.

### Idle window animation

Before removing unconditional SwipeHint repeat:

```text
Debug idle ~29% CPU after startup
```

After:

```text
Debug idle ~1.5–1.8% CPU
Release idle ~0.6% CPU in the latest isolated event-driven smoke
```

### Hosted TUI attach and server-rendered chrome

The current normal surface is one hosted `herdr` child/PTY, not per-Pane controller attach/preload. Historical per-Pane `pane.read`/controller timings are archived evidence only and must not guide current hot-path work.

Herdr 0.8.2 renders Sidebar/tab chrome on the server. A native A/B showed that letting the server render chrome and cropping it in Shardlane cost about **1.2–1.8 ms/frame** in the projection/cloning path; when a fresh Shardlane-started server and child both use the merged runtime config with Sidebar/tab chrome removed at source, the same projection seam was effectively **0–1 µs** and `projected=false`. The 2026-09-18 ruling deleted the crop seam; the second 2026-09-19 revision resurrected it as the sanctioned answer to Herdr 0.9.1's missing always-hide key (`TuiChromeProjection`, `TerminalFrame::project_rect`, chrome probes, and the compensated-grid resize are back). Cost containment is unchanged in kind: single-Tab windows need no crop (the startup migration keeps `ui.hide_tab_bar_when_single_tab = true`), the projection runs once per applied frame with Arc-cloned raw baselines, and `stage=frame.apply` traces again carry `projected=` so regressions are visible. The raw frame remains the plan/signature baseline; the projected frame is the painted grid.

Every Shardlane path that may need to start/restart Herdr (startup, manual Refresh reconnect, event reconnect, New Project reconnect) must pass the same validated derived config path. A Settings presentation change first prepares that merged config off the UI thread; if the current client bootstrap itself started the server with the supplied path, it reloads that server before restarting the child. Pre-existing/user-owned servers remain read-only.

### History

Old behavior allowed transcript materialization to grow 200 → 400 → 600 → … messages as users loaded more history. Current behavior keeps a fixed 60-message sliding window and collapses expensive long content until explicitly expanded.

## 8. Anti-pattern checklist

Do not:

- add a permanent high-frequency poll to “make it feel faster”;
- attribute wall-clock async delay to Ghostty/Herdr without separating queue/lock/work;
- run decorative infinite animations on an idle surface;
- keep rendering a hidden Terminal behind History/Settings/dialogs;
- scale Terminal bootstrap/preload work with total Herdr retained scrollback;
- reintroduce the removed per-Pane controller/local-history scroll architecture into the hosted-TUI path;
- perform interactive Herdr socket commands such as Pane split/zoom/close synchronously on the GPUI thread; run the command off-thread and consume the narrowest authoritative result/event projection available;
- poll an async Herdr/History event source on a fixed short timer while idle;
- poll the Task monitor when there are zero materialized runtime Tasks;
- use CSS/GPUI line clamp as proof that heavy child data was not constructed;
- grow transcript UI windows indefinitely;
- full-refresh navigation/Agents/Tasks once per event in a burst;
- write runtime-only Task projection changes to disk;
- synchronously flush performance logs from hot paths;
- benchmark a debug build only and claim a product baseline;
- use historical `--takeover` controller measurements as a hosted-TUI baseline.
