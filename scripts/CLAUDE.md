# scripts/
> L2 | Parent: ../CLAUDE.md

Member inventory

- `bundle-web.sh` — Builds/copies herdr-mobile's static `dist/`; lands in `target/mobile-web/` during development and in `Shardlane.app/Contents/Resources/mobile-web/` during packaging.
- `package-macos.sh` — Orchestrates single-target or Universal 2 (arm64+x86_64) Cargo build, cargo-bundle, optional Mobile Web embedding, codesigning after the merge, recursive Mach-O structural verification, and explicit installation; app identity configuration is not duplicated in the script.
- `archive-macos.sh` — Verifies an already-built `.app` (including the `universal2` architecture gate), creates a distributable ZIP and a SHA-256 checksum file in the same directory; does not build, sign, or notarize.
- `release-macos.sh` — One-click release entry point (Universal 2 by default): static gates (fmt/clippy/test) → Mobile Web static bundle ready (auto `pnpm export:web` when missing) → release `.app` packaging + install → dist/ ZIP+SHA256; it only composes the three scripts above and adds no new artifact rules.
- `analyze-terminal-trace.py` — Parses `SHARDLANE_TERMINAL_TRACE=1` and Host `terminal.shared.*` records, outputting count and p50/p95/max per stage/input kind; filters Rust's unset duration sentinel, consumes only lengths/durations/packet ids, and never reads or prints user input text.
- `update-vendored-gpui.sh` — Upstream/downstream sync mechanism for vendored gpui: `status` (baseline version vs Cargo.lock resolved vs wrapper exact pin) / `export-patch` (after whitelist validation, regenerates `vendor/patches/gpui/*.patch` from the current delta) / `verify` (pristine+patch round-trip byte-identical) / `sync [--version V]` (fetch pristine → apply patches → swap in → build). See the script usage for the upgrade workflow and acceptance steps.
- `keyrepeat-pacing-ab.sh` + `keyrepeat-evpost.swift` — Scored harness for long-press key-repeat rendering cadence: a seeded isolated environment (separate HOME+socket+probe .app) is prepared once and run repeatedly; N consecutive sampling rounds within a single session (deterministic navigation to Terminal+composer → System Events reset → screen recording → 30Hz autorepeat burst), jointly analyzed with trace to compute visible states/update rate/interval dropped-frame rate and output score(0-100)+PASS/WARN/FAIL; `report` compares across labels. Used for pacing regressions and fix acceptance (evidence source of the 2026-08-31 gpui presentation patch).
- `terminal-native-smoke.sh` — macOS Terminal real-device forensics harness; starts one Herdr server and one Shardlane app with a temporary `HOME` + unique `HERDR_SOCKET_PATH` + dedicated `SHARDLANE_LAG_LOG_PATH`, prints Ghostty A/B commands and trace analysis commands for the same runtime, and cleans up only its own PIDs/directories.

Boundary: scripts only orchestrate build artifacts and verification; they own no Herdr runtime, Remote API, or Mobile Web business logic.
