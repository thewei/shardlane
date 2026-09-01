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

## Upgrade flow (the binary itself is the ABI authority)

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
