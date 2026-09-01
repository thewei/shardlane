upstream-version: 0.2.2
# Shardlane frame-pacing patch (local, 2026-08-31)

Base: gpui 0.2.2 (crates.io), unmodified except `src/window.rs`:

Removed the anti-throttling branch in the display-link frame callback that
re-presented the full scene on every vsync for 1s after input
(`else if needs_present { window.present() }`). During long key repeats that
branch doubled the Metal commit rate (60/s), starved the drawable pool under
macOS 15 FramePacing, blocked the main thread in `next_drawable()` (measured
at 44% of main-thread time), and collapsed the visible update rate from ~30/s
to ~20/s. With the branch removed, the last presented drawable stays on screen
and rendering correctness is unaffected; the trade-off is that ProMotion
adaptive-refresh probing loses that signal (acceptable on desktop).

Measurement: `scripts/keyrepeat-pacing-ab.sh run <label> --samples 3`

2026-08-31 addendum: the same patch also removed the dead code left behind by
that branch (the `needs_present` / `active` / `last_input_timestamp` captures
and recomputation in the frame callback, which no longer had consumers),
eliminating the `unused variable: needs_present` build warning; behavior is
unchanged.

Update/sync flow: `scripts/update-vendored-gpui.sh status | export-patch | verify | sync [--version V]`
