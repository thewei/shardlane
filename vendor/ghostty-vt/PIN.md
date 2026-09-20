# Vendored libghostty-vt — provenance and upgrade record (PIN)

## Current version (inventoried 2026-08-25)

- File: `vendor/ghostty-vt/lib/libghostty-vt.a` (17,564,536 bytes), statically
  linked into the binary; no dylib shadow since `34355f1`.
- Import history: added 2026-07-05 (`7688bff chore: include libghostty-vt.a for
  static linking`); static-link conversion `4098518` follows the cmux linking
  approach and originates from the `ghostling/libghostty-rs` extraction project.
- Self-reported version string: `0.1.0-dev` (the extraction project's own
  version, **not** a Ghostty version).
- Export surface: 162 `T _ghostty_*` symbols (cell / grid_ref / key_encoder /
  mouse_encoder / osc / sgr / formatter / selection_gesture / kitty_graphics /
  mode_report / paste_encode / simd_* …).
- Embedded-symbol evidence: contains `terminal.osc.parsers.kitty_clipboard_protocol`
  and `terminal.kitty.graphics_storage` modules, i.e. a Ghostty main snapshot
  between 1.3.1 and 1.4; the key enum was verified item-by-item against
  upstream `ghostty.h` v1.3.1.
- The binary embeds self-describing struct-layout JSON (offset/size for
  GhosttyStyle / GridRef / Selection / RenderStateColors …) used as the direct
  reference for ABI checks.
- **Known debt: the exact upstream Ghostty source commit was not recorded when
  the archive was first vendored.** Reconstructing an exact diff is not
  possible; only symbol/layout comparison is available. Before relying on a
  rebuild, pin a Ghostty commit and record it here per the upgrade flow below.

## Multi-target layout (2026-09-12)

Vendored archives now live per Rust target triple so the release matrix can
build macOS (arm64 + x86_64), Linux x86_64 and Windows x86_64 from the same
repository:

- `lib/<rust-triple>/libghostty-vt.a` — unix targets (darwin/linux)
- `lib/<rust-triple>/ghostty-vt.lib` — the windows-msvc target (COFF)
- `lib/libghostty-vt.a` — legacy single archive, kept only as the
  `aarch64-apple-darwin` fallback inside `build.rs`

Produce a target archive with `scripts/vendor-ghostty-vt.sh --triple
<rust-triple> --ghostty-ref <tag-or-commit>` (or trigger
`.github/workflows/vendor-ghostty-vt.yml`); the script runs the ABI gate
(exported ghostty_* symbol set vs the baseline archive + embedded
struct-layout JSON fingerprint) before installing, and appends the
provenance entry below. Windows note: the default named-pipe convention
(`\\.\pipe\herdr\...`) that the client mirrors for Herdr sockets is
verified separately against live herdr by the release workflow.

- `lib/x86_64-apple-darwin/libghostty-vt.a` (installed 2026-09-20): the
  x86_64 slice of the pinned universal baseline `lib/libghostty-vt.a`
  (inventoried 2026-08-25), extracted locally with
  `lipo -thin x86_64` — no upstream rebuild. ABI gate: exported
  `_ghostty_*` symbol set (162 symbols) is identical to
  `lib/aarch64-apple-darwin/libghostty-vt.a`. Required by the per-target
  `build.rs` lookup for the CI Intel cross-build and the universal
  macOS bundle.

## x86_64-unknown-linux-gnu / x86_64-pc-windows-msvc — installed 2026-09-20

- Source: https://github.com/ghostty-org/ghostty.git @
  699387c2c16dd5723e8825ad608538142b07b86b (2026-06-14, "Update VOUCHED
  list") — identified as the baseline-vintage snapshot by a local bisect:
  at this commit the rebuilt aarch64 archive reproduces the baseline's
  exported symbol set (162/162, zero missing and zero extra) and the
  embedded struct-layout fingerprint byte-for-byte.
- Zig 0.15.2 (the ref's build.zig.zon minimum; the vendoring workflow
  takes it as an explicit `zig_version` dispatch input).
- Artifacts: `lib/x86_64-unknown-linux-gnu/libghostty-vt.a`
  (14,647,614 bytes) and `lib/x86_64-pc-windows-msvc/ghostty-vt.lib`
  (7,028,626 bytes), built by
  `.github/workflows/vendor-ghostty-vt.yml` (run 35491443333).
- ABI verification: layout fingerprint of both artifacts matches the
  baseline exactly; linux exports 162 symbols == the baseline set;
  windows exports 161 == baseline minus `ghostty_hwy_detect_targets`
  (target-internal highway dispatch helper, recorded in
  `BASELINE_SYMBOLS.exceptions`, unreferenced by the client FFI). All
  function symbols bound by `crates/herdr-gui/src/ghostty/ffi.rs` are
  present in both.
- Note: the runner-side gate entries appended by the workflow reported
  `baseline=0` due to an in-place filter truncation bug in the script
  (fixed 2026-09-20); the numbers above were re-verified locally against
  the installed artifacts with Apple llvm-nm and the fixed script.

NaN

1. **Source**: clone Ghostty upstream at the target tag/commit (nightly
   releases ship no library artifact; build it yourself) and use the
   extraction project's (ghostling) ghostty-vt static-library target,
   macOS aarch64, ReleaseFast.
2. **Land**: replace `vendor/ghostty-vt/lib/libghostty-vt.a` and append to this
   file: `source URL + commit + build command + date`.
3. **ABI comparison**:
   - diff the `nm` exported symbol set (baseline: 162 symbols) — record new
     symbols; disappearing symbols must be explained;
   - compare every struct our FFI uses (GhosttyStyle, GhosttyGridRef,
     GhosttySelection, GhosttyTerminalOptions, GhosttyRenderStateColors …)
     against the embedded layout JSON offsets/sizes;
   - re-verify each of the 21 `extern "C"` bindings in `ghostty.rs` against
     the binary (derive new bindings' signatures from disassembly).
4. **Behavior regression**: `cargo test --locked --workspace` (includes key
   encoder behavior regressions: ctrl+a→`\x01`, enter→`\r`, home→`CSI H`,
   DECCKM→`SS3 A`, kitty `>1u` shift+up→`CSI 1;2A`, alt+b→`CSI 98;3u`);
   re-run micro-benchmarks with
   `cargo test --release --bin shardlane -- --ignored perf_`.
5. **Full gates + smoke**: fmt/clippy/test/build/diff-check plus
   `crepus dev --bin shardlane`; inspect `/tmp/shardlane-lag.log`.

## Architecture fact: upgrading the client library ≠ unlocking the kitty clipboard

Since Ghostty 1.4, libghostty fully implements the kitty clipboard protocol
(applications can read/write non-text clipboards; Claude Code is the main
beneficiary). **This is unreachable from Shardlane's pane terminals:**

- Data path: the in-pane app ↔ PTY ↔ the Ghostty embedded in the **Herdr
  server** (the consumer). Isolated testing on 2026-08-25 confirmed Herdr
  consumes both OSC 52 (BEL/ST terminators) and kitty APC without forwarding
  them to the controller, without touching the system clipboard, and without a
  configuration switch.
- Our vendored library is only a client-side mirror parser for the bytes Herdr
  forwards; the clipboard handshake responder and the host-clipboard writer
  must both be the PTY owner (Herdr).
- **Conclusion**: that capability unlocks when **Herdr upgrades its embedded
  Ghostty and wires up host clipboard access**. Downstream consumers should
  only re-probe after each Herdr upgrade (OSC 52 forwarding / `pbpaste` change /
  kitty clipboard query response) and start client-side work only once it is
  unlocked.

The benefit boundary of upgrading our own library: upstream VT-parsing
correctness fixes, rendering/model fixes, and newly extractable ABI surface —
unrelated to in-pane clipboard/image capabilities.
