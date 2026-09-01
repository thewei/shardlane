# Terminal latency evidence playbook

<!--
[INPUT]: A reported delay in hosted Herdr TUI key repeat, scroll, selection, IME,
or terminal repaint; a reported visual artifact (segments, dashes, seams) in
rendered TUI chrome such as the Herdr scrollbar; the current Shardlane source
tree and an available macOS host.
[OUTPUT]: A reproducible diagnosis that names the slow segment or the layer that
owns the artifact, a targeted regression, native evidence, and an honest list of
any manual or blocked acceptance.
[POS]: Reusable evidence recipe reached from the Shardlane development skill; it does
not own product runtime behavior or replace docs/performance-engineering.md.
-->

Use this playbook when Terminal feels one beat behind, especially during held-key
repeat or rapid precision-wheel input. The goal is to prove ownership of the delay,
not to make a timer smaller until the symptom moves.

## Evidence contract

Measure the path as separate segments, in this order:

~~~text
native input
  → ordered enqueue
  → Host/shared writer
  → PTY read
  → event fan-out
  → VT drain
  → Ghostty frame plan/extraction
  → visible projection/paint
~~~

Record a monotonic packet/read id, byte length, stage, and elapsed microseconds.
Never record the user's text. Keep transport timings (shared/pty write/read)
separate from Ghostty extraction and GPUI row construction. Classify every result as
one of:

- **deterministic** — a unit/work-bound invariant that must stay green on every host;
- **native** — an ignored macOS process smoke with local p50/p95/max evidence;
- **manual/open** — IME candidate behavior, momentum scrolling, or visual A/B that the
  available automation cannot drive faithfully.

The completion criterion is a named bottleneck plus a regression that would fail if
the old behavior returned. A green test with no observable invariant is not evidence.

## Step 1 — guard and isolate the run

Before changing or measuring code, capture the worktree and use the existing native
harness:

~~~sh
git status --short --branch
git diff --stat
git diff --cached --stat
cargo build --locked -p shardlane --bin shardlane
scripts/terminal-native-smoke.sh --keep
~~~

For a fresh binary, use scripts/terminal-native-smoke.sh --build --keep. The
harness starts exactly one Herdr server and one Shardlane process with a temporary
HOME, a unique HERDR_SOCKET_PATH, and a per-run SHARDLANE_LAG_LOG_PATH. It
prints the PIDs, logs, and a Ghostty command that targets the same isolated runtime.
Do not use the user's default Home/config/socket for a performance baseline, and do
not kill unrelated herdr server or Shardlane processes.

After the app exits, the harness removes only its own temporary directory unless
--keep was supplied. When a test fails, preserve the exact runtime directory and
logs before retrying.

## Step 2 — reproduce the same matrix

Exercise each row at least once; repeat the row that first shows the symptom.

| Row | Input/operation | Evidence to capture | Correctness boundary |
| --- | --- | --- | --- |
| Repeat | Hold an ASCII key, then shifted symbol, Ctrl/Alt/Cmd chord, and named key | packet count, enqueue/write/read/frame percentiles | order is lossless; no fixed 16 ms edge |
| IME | Mark/update/commit an actual candidate | ui.ime_*, ui.text_submit, visible committed text | one commit, no duplicate control clear |
| Scroll | Precise vertical deltas, direction reversal, release, and Shift-wheel | ui.scroll route/steps/residual | Shift remains with parent; residual cannot leak later rows |
| Selection/mouse | Click, drag, middle/right release, copy/paste | mouse encode bytes and frame update | right-click has one native owner; no PTY right-button report |
| Repaint | Same frame, one changed row, full viewport redraw | changed-row count, signature/extract/present timings | work follows RAW rows, not Herdr chrome or total viewport |
| OSC-8 | Open/relink/close hyperlink, split 7-bit and C1 framing across writes | ghostty.osc8, dirty/reuse path, link spans where backend supports it | close/relink invalidates stale reuse; C1 parser state returns idle |
| A/B | Run Ghostty with the printed isolated HOME/socket | visual/interaction notes only unless synchronized timing exists | never claim Metal parity when focus or state is unsynchronized |
| Visible pacing | Held-key repeat (30Hz `isARepeat` bursts) under `scripts/keyrepeat-pacing-ab.sh run <label> --samples 3` | visible-state count, gap p50/p90/max vs *measured* input period, slips, key→present percentiles | screen frame-diff is ground truth; app-internal present timings can be all-green while the display merges updates |
| Visual continuity | Hosted TUI with scrollback overflow (`seq 400`) under `scripts/scrollbar-continuity-ab.sh run <label> --samples 3` | scrollbar-track gap count/max gap/period, measured row pitch vs configured cell height | row pitch must equal cell pitch; per-row full-height overlays tile with zero background gaps |
| Scroll tracking | Multi-screen scrollback (default 100 screens) under `scripts/scroll-tracking-ab.sh run <label> [--screens N] [--pixel]` | stall count (gap > 50ms), sustained ups, scroll→present percentiles, drain/present cadence | steady movement without freezes; content-update cadence is child-owned and must not be scored as app slips |
| Repeat modes | Same 30Hz burst with `--mode insert` (nvim insert typing) / `--mode backspace` (fill + delete) / `--interval-us` | key→present percentiles, echo batching factor, visible ups vs measured input period | input→paint must stay well under one vsync; echo batching above that is child-side |

## Step 3 — inspect bounded trace evidence

Enable tracing only in the isolated run:

~~~sh
export SHARDLANE_TERMINAL_TRACE=1
export SHARDLANE_LAG_LOG_PATH="$RUNTIME_DIR/shardlane-lag.log"
python3 scripts/analyze-terminal-trace.py "$RUNTIME_DIR/shardlane-lag.log" --pid "$APP_PID"
~~~

The harness prints the concrete values for RUNTIME_DIR and APP_PID; do not copy
the placeholders literally. If using the default path, filter by PID:

~~~sh
tail -n 200 /tmp/shardlane-lag.log
rg "pid=$APP_PID|terminal\.(trace|shared)" /tmp/shardlane-lag.log | tail -n 200
~~~

Useful stage names are ui.key_route, ui.key_encode, ui.text_submit, ui.scroll,
shared.enqueue, shared.write, shared.read, shared.forward, pty.enqueue,
pty.write, pty.read, vt.drain, ghostty.frame, frame.extract, frame.apply,
pane.render_build, root.render, poll.wake_signal, and poll.wake. The analyzer
ignores unset integer sentinels and reports stage-local plus approximate packet
end-to-end p50/p95/max values.

Read the trace as a timeline:

1. A repeat delay clustered near 16 ms before fan-out indicates a polling gate; use
   an event edge (blocking_recv or equivalent) with a coalesced wake.
2. Backpressure or accepted-count loss before the writer indicates a queue seam;
   coalesce only adjacent byte-compatible packets, preserve paste/focus boundaries
   and order, and keep a byte budget.
3. A fan-out/frame delay independent of PTY write/read indicates receiver or frame
   queue pressure; keep the viewer queue bounded and measure its capacity.
4. Extraction or row-build work proportional to the whole viewport indicates a
   projection baseline bug; use exact RAW row signatures and changed-row plans.
5. A full extract caused by OSC-8/global state must reseed signatures immediately so
   the next unchanged frame can reuse safely.
6. A shortcut name changing while terminal encoding improves indicates conflated
   representations; preserve raw GPUI key identity for GUI/script registries and
   apply physical-key/implicit-shift mapping only inside the Ghostty encoder.

## Step 3.5 — visible pacing: video × trace joint scoring

App-side `frame.present_request`/`root.render` cadence can be perfect (every key applied ≤17 ms, draws at ~33/s) while the screen shows only ~20 updates/s — the loss then lives in the GPU/compositor layer, after notify. Never conclude "pipeline is healthy" from trace-only evidence when the symptom is perceived stutter.

- Ground truth: record the screen at 60 fps during the burst and run a *state-machine* frame diff (a new state = frame differing from the last accepted state, not from the previous frame — compression flicker then cannot recount). Count distinct visible states and gap distribution.
- Known stall source (macOS 15 FramePacing): `CAMetalLayer::next_drawable` blocks the main thread (CPU `sample` shows the share, e.g. 44%) whenever drawable demand outpaces recycling. In gpui 0.2.2 the "re-present the full scene every vsync for 1 s after input" anti-underclock branch doubles Metal submissions during bursts and starves the pool; the vendor patch (`vendor/gpui/PACING-PATCH.md`, wired via `[patch.crates-io]`) removes it. If `wake→loop` or `read_to_drain` p50 jumps to ~10 ms+ during bursts while idle is ~1–4 ms, suspect this class of main-thread blocking first.
- Harness: `scripts/keyrepeat-pacing-ab.sh setup` (once) → `run <label> --samples 3 [--profile]` → `report <label>...`. Score slips against the *measured* input period, not a nominal one (System Events keystroke loops actually run 20–40 Hz depending on load).

Synthetic input pitfalls (each one produced a false "the keys are dropped" dead end):

- **IME capture**: a Chinese IME (e.g. Doubao) intercepts posted printable keys and enters composition — nothing reaches the PTY/TUI. Select the plain ABC keyboard layout (`evpost src set com.apple.keylayout.ABC`) before synthetic typing and restore the user's source afterwards; also clear any candidate window before measuring.
- **Frontmost vs key window**: HID-tap key events (`CGEvent` at `.cghidEventTap`) are dropped when the target app is frontmost-set but its window is not key. System Events `keystroke` routes through the full AppKit pipeline and still lands — use it as the fallback and decide via a probe: one 'j' must produce `ui.text_submit` before trusting any burst.
- **No OS autorepeat from synthetic holds**: `hold_key`/single posts never repeat. Post repeated keydowns with `.keyboardEventAutorepeat = 1` for a faithful repeat stream, and verify the *measured* inter-key period from the trace instead of assuming the nominal delay.
- **Frontmost verification before every injection**: `osascript ... unix id of first process whose frontmost is true` must equal the probe pid — a stray keystroke otherwise lands in whatever app the user is using.

## Step 3.6 — visible rendering continuity: pixel forensics × recolor diagnostic

Use this when a *spatial* artifact is reported — a hosted-TUI element renders as
segments, dashes, or seams where Ghostty shows one continuous line. Canonical
case (2026-08-31): the Herdr pane scrollbar rendered as one dash per row in
Shardlane but as a continuous line in Ghostty. The failure lived in the layout
layer, not the paint layer; only pixel evidence could prove that.

### Ground truth 1 — what the child actually draws

Do not assume glyph identity from font tables or memory. Capture the child's
raw PTY output in an isolated runtime (temporary `HOME` + unique
`HERDR_SOCKET_PATH` + `herdr server`, then run the TUI under a Python
`pty.openpty` harness with `TIOCSWINSZ`, feed scrollback via `herdr pane run`,
and dump the raw byte stream). Strip ANSI/OSC sequences and read the exact
codepoints. This is how the scrollbar was proven to be track `▕` (U+2595) per
row plus thumb `▐` (U+2590) — both already covered by Shardlane's cell-geometry
overlay table, which redirected the investigation from "missing glyph handling"
to "the overlay's pixels are wrong".

### Ground truth 2 — what the pixels actually show

Capture the real window (`screencapture -x -o -l <winid>` for a window image, or
`-R x,y,w,h` from `evpost bounds`) and analyze with Pillow:

1. Take the background color from a known-empty interior region.
2. Scan the right edge strip for the scrollbar track column; within its ink
   span, record background gaps: count, each gap's width in device pixels, and
   the spacing between gaps.
3. Measure the *realized* row pitch independently (e.g. per-row text band
   starts) and compare it with the configured `cell_height × scale`.

Decision rule: gaps whose spacing follows the measured row pitch are per-row
layout artifacts; gaps that follow nothing are paint-layer or compositing
artifacts. In the canonical case the measured pitch was ~36.7 device px while
`cell_height` was 18pt = 36 px — the mismatch *was* the bug.

### The recolor diagnostic — is the layer drawn, and where

Element-tree reasoning ("the overlay div is a child of the row div, therefore
it renders") is not evidence. Recolor the suspect paint layer to a color that
cannot occur naturally (magenta), rebuild, rerun the isolated app, screenshot:

- **No magenta** → the layer is not drawn at all; debug the data/decision path
  (a real-FFI regression that asserts overlay presence catches this class).
- **Magenta with the same gaps** → the layer is drawn but its geometry is
  wrong; debug layout.
- **Magenta, no gaps** → the layer is fine; the artifact you were chasing came
  from something underneath.

Revert the diagnostic color before the final diff.

### The root-cause family to suspect first

GPUI layout wrappers override element sizing. `cached_view` wraps every child in
`flex_1()`, so a `size_full` flex column stretches rows to
`container_height / rows` — never exactly `cell_height` — and every per-row
full-height overlay grows a ~sub-pixel bottom sliver that the rasterizer turns
into periodic 1px gaps on continuous elements. Row backgrounds hide it (same
color as neighbors); scrollbars, progress bars, and box-drawing runs reveal it.
Fix at the layout layer (pin the row grid to `rows × cell_height` and back-fill
the remainder with the same background), then re-check input mapping that
assumes the same grid (`y / cell_height` hit-testing was skewed by the same
stretch).

### Harness recipe — writing a `*-continuity-ab.sh`

Model on `scripts/scrollbar-continuity-ab.sh` (structure copied from
`keyrepeat-pacing-ab.sh`):

1. **Bootstrap** a fresh isolated runtime per label (`rm -rf` then mkdir):
   temporary `HOME`, unique `HERDR_SOCKET_PATH`, `nohup herdr server`, wait for
   the socket, `nohup` the shardlane binary, wait for `evpost bounds <pid>` to
   return the window frame. Compile `keyrepeat-evpost.swift` on first use.
2. **Drive the UI deterministically**: `evpost click` at window-relative
   coordinates (record them as constants with a comment naming the surface).
   Focus first via System Events `frontmost of (first process whose unix id is
   PID)`. A fresh home has no Project, and the Terminal segmented control does
   not exist until one exists — create it via the CLI against the isolated
   socket, not by typing.
3. **Seed scrollback via the composer** (submitting a command is what mounts
   the hosted TUI surface): click the composer, switch the input source to ABC
   (`evpost src set com.apple.keylayout.ABC`), `osascript keystroke` the seed
   command (e.g. `seq 400`), Enter, restore the previous input source. Seeding
   through `herdr pane run` alone does NOT mount the surface.
4. **Sample**: `screencapture -x -o -R wx,wy,ww,wh` N times; keep the PNGs.
5. **Score in Python/Pillow**, one JSON verdict per sample: find the track
   column by *total ink rows* (not longest run — segmentation fragments runs),
   excluding the outer ~12 device px of the capture (window border/shadow
   columns are full-height false positives); require a minimum span; count
   background gaps within `[first_ink, last_ink]`;
   `score = clamp(100 − gap_count×2.5 − max_gap_px×10, 0, 100)`;
   PASS ≥ 85, WARN ≥ 60, else FAIL.
6. **Report**: append a summary JSON per label; a `report <label>...` subcommand
   prints the cross-label table. Kill only the PIDs the script created, and
   verify any pre-existing `herdr server` by its `HERDR_SOCKET_PATH` before
   assuming it is yours — the user's default server has none.

Evidence from 2026-08-31: baseline 3/3 samples = 26 gaps (all 1px, 73/37px
spacing), score 25.0 FAIL; after pinning the row grid, 3/3 samples = 0 gaps,
coverage 1.0, score 100.0 PASS. Store per-sample PNGs so a human can re-judge
the machine verdict.

### Regression pairing

A visual fix needs one deterministic unit regression (the layout math or
overlay decision that changed) plus, where the decision depends on real
emulator state, a vendored-lib FFI regression (feed SGR-colored `▕` bytes
through `GhosttyTerminal`, assert overlays for raw *and* chrome-projected
frames). The harness itself is the visible acceptance, not a CI gate; record
its labels and scores in the evidence ledger.

## Step 3.7 — scroll tracking and repeat-mode evidence (2026-08-31 round)

Two harness extensions turn the pacing recipe into a full input-surface lab:

- **Scroll tracking**: `scripts/scroll-tracking-ab.sh` seeds a *multi-screen*
  scrollback (default 100 screens ≈ 4000 lines; `--screens N`) — small
  histories saturate the scroll range and fake a FAIL — then fires up/down
  wheel bursts (`--pixel` posts continuous precise deltas, faithful trackpad
  emulation; line mode posts `mouse_scroll_lines`-style steps). Score by
  *stalls* (gap > 50ms ≈ 3 vsyncs), not by per-vsync alignment: the child TUI
  repaints at ~30Hz, and macOS splits synthetic wheel events into sub-vsync
  NSEvents, so absolute thresholds misclassify the child's cadence as slips.
  Calibrated result: line + pixel both median 100, 0 stalls, scroll→present
  p50 6ms.
- **Repeat modes**: `keyrepeat-pacing-ab.sh run <label> --mode insert|backspace
  [--interval-us US]`. Insert mode enters nvim insert and bursts printable
  chars; backspace mode types 60 filler chars first then bursts keycode 51.
  Trace cluster source differs per path: printable text arrives as
  `ui.text_submit`, named keys (backspace) as `shared.enqueue kind=key` —
  group clusters by the right stage or the burst looks missing.

Findings that transfer to any future "feels worse than Ghostty" report:

1. **Name the path before blaming it.** key→present was p50 3ms / max 7ms and
   per-frame main-thread cost ~4ms while the user still felt stutter — the
   pipeline was never the bottleneck. Attribute every millisecond: publish→
   forward ~0ms, forward→drain p50 3ms, wake edge Output-triggered.
2. **Synthetic input has a fidelity ceiling.** macOS pair-delivers 30ms
   CGEvent repeats (50ms+10ms) and throttles 15ms bursts to ~35/s. The child
   (nvim) redraws once per delivered batch, so visible ups 17-27 under a
   30Hz burst is input-shaping + child batching, not app lag. Real hardware
   repeats arrive as a steady stream. Never "fix" the app to compensate a
   synthetic-input artifact.
3. **Child-owned cadence is a boundary, not a bug to score.** Scroll content
   updates cap at ~30Hz (herdr TUI redraw loop) and wheel motion is
   row-quantized by the SGR mouse protocol (sub-row pixel smoothness is not
   expressible). Record these as Herdr-boundary findings; do not score them
   as app regressions.
4. **Cursor semantics**: blink phase must reset on cursor movement (Ghostty
   behavior; `cursor_phase_reset_on_move` + re-armed tick). The vendored
   model defaults to blinking=false and nvim's default insert cursor is a
   *steady* bar — probe the actual surface state before assuming blinking
   explains a visual artifact.
5. **Known open P2**: under ≥100 events/s output, read_to_drain is bimodal
   (fast <2ms vs 12-30ms for ~45% of batches; slow ones are Output-triggered,
   so the wake edge works — it is main-thread scheduling of the poll future
   behind draw work). Fix direction: move VT parse/extract off the UI thread.
   Impact today is bounded (scroll→present p95 27ms); measured before
   attempting the refactor.
6. **`open -na Ghostty.app` is single-instance**: `-e env ...` args merge into
   the user's running Ghostty, and injected keys can land in their session.
   Do not drive Ghostty A/B by injection on a shared machine; use the smoke
   script's printed manual command for a human operator instead.


## Step 4 — run the regression matrix

Run deterministic tests first. Keep the commands exact so a future agent can paste
them without rediscovering module paths:

~~~sh
cargo test --locked -p shardlane-host shared_tui::tests::input_queue_coalesces_ordered_burst_without_dropping_bytes -- --exact --nocapture
cargo test --locked -p shardlane-host shared_tui::tests::input_queue_keeps_paste_and_focus_boundaries -- --exact --nocapture
cargo test --locked -p shardlane-host shared_tui::tests::input_queue_retains_a_hard_byte_budget -- --exact --nocapture
cargo test --locked -p shardlane terminal_stream::tests::viewer_frame_queue_has_a_hard_capacity -- --exact --nocapture
cargo test --locked -p shardlane terminal_stream::tests::shared_event_forwarder_ignores_lifecycle_wakes_and_stops_on_its_own_wake -- --exact --nocapture
cargo test --locked -p shardlane shell_tui::tests::shift_wheel_stays_with_parent_and_cannot_accumulate_terminal_rows -- --exact --nocapture
cargo test --locked -p shardlane input::tests::shifted_symbol_shortcut_chords_preserve_gpui_key_names -- --exact --nocapture
cargo test --locked -p shardlane ghostty::tests::terminal_delta_work_is_bounded_to_changed_rows_in_large_viewport -- --exact --nocapture
cargo test --locked -p shardlane ghostty::tests::terminal_delta_identical_large_frame_produces_no_projection_work -- --exact --nocapture
cargo test --locked -p shardlane ghostty::tests::c1_osc8_markers_are_observed_for_safe_frame_reuse -- --exact --nocapture
cargo test --locked -p shardlane ghostty::tests::frame_reusing_signature_fast_path_reuses_unchanged_frame -- --exact --nocapture
cargo test --locked -p shardlane ghostty::tests::osc8_links_survive_frame_reuse_without_new_writes -- --exact --nocapture
cargo test --locked -p shardlane ghostty::tests::osc8_relink_updates_uri_after_new_osc8_write -- --exact --nocapture
cargo test --locked -p shardlane ghostty::tests::osc8_close_invalidates_same_text_link_reuse -- --exact --nocapture
cargo test --locked -p shardlane-host shared_tui::tests::child_reap_signal_does_not_complete_before_actual_wait_notification -- --exact --nocapture
~~~

Expected invariants are ordered bytes with no drop, hard paste/focus boundaries,
bounded memory, a full viewer queue returning Full, Shift-wheel staying
unhandled, raw GUI shortcut names remaining stable, one changed row for one-cell
input, no projection work for an identical frame, and OSC-8 C1 state returning idle.

Then run the intentionally ignored native diagnostics on macOS:

~~~sh
cargo test --locked -p shardlane-host shared_tui::tests::shared_tui_burst_input_backpressure_smoke -- --ignored --exact --nocapture
cargo test --locked -p shardlane terminal_stream::tests::shared_event_forwarder_repeat_input_latency_smoke -- --ignored --exact --nocapture
cargo test --locked -p shardlane terminal_stream::tests::hosted_pty_repeated_text_input_latency_smoke -- --ignored --exact --nocapture
cargo test --locked -p shardlane ghostty::tests::ghostty_frame_reusing_changed_vs_unchanged_smoke -- --ignored --exact --nocapture
cargo test --locked -p shardlane ghostty::tests::ghostty_full_viewport_redraw_reuse_smoke -- --ignored --exact --nocapture
~~~

The burst smoke should admit 2,000 one-byte packets with zero rejection. The
fan-out and PTY smokes print p50/p95/max; use them as local regression baselines,
not cross-machine SLAs. A full-screen redraw is expected to cost more than an
unchanged frame, but its work must remain bounded and explainable.

Finally run the repository gates:

~~~sh
cargo fmt -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
git diff --check
git diff --cached --check
~~~

If the workspace suite reports a socket connection refusal or real-child reap
timeout, rerun the named test alone with --exact --nocapture. A passing isolated
rerun proves timing sensitivity, not a green workspace gate; record both facts and
do not edit product logic solely to hide the flake.

## Step 5 — write the evidence ledger

Before declaring the work complete, record in the internal engineering archive or the active
performance document:

- date, commit/working-tree state, macOS/build profile, Herdr version/path;
- exact deterministic and ignored commands that ran, including counts and
  p50/p95/max output;
- the slow segment and the invariant that changed it;
- native isolation (HOME, HERDR_SOCKET_PATH, log path) and PID filtering;
- manual checks completed, plus every visual/IME/trackpad/A-B item that remains open;
- any flaky test with its full-run result and isolated rerun result.

Do not turn an unperformed Ghostty visual comparison, true IME candidate test, or
momentum-trackpad test into a completed fact. The skill is reusable only when its
map distinguishes measured behavior from operator work still required.
