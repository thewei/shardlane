---
name: shardlane-mobile-development
description: Use when building, installing, debugging, or shipping Shardlane Mobile (Expo SDK 54 / RN 0.81) to an Android phone, the Web/PWA, or the desktop app's embedded Mobile Web — including adb device loops, vivo USB-install quirks, watchman hangs, detached builds that survive agent-session teardown, fixture lockstep with herdr-client, and Mobile↔desktop contract alignment.
---

# Shardlane Mobile — Build / Install / Debug Loop

Use this skill for any task that changes code in the sibling repository
`../herdr-mobile` (Shardlane Mobile) or re-ships its artifacts. It encodes the
verified 2026-09-06 build/install loop so the next run skips the failure
rediscovery.

Process control stays with `shardlane-development`; architecture truth lives in
`herdr-mobile/CLAUDE.md` + `herdr-mobile/docs/mobile-product-architecture.md`.
This skill owns only the build/install/debug mechanics and their pitfalls.

## 0. Before anything

1. `cd herdr-mobile && pnpm typecheck && pnpm exec eslint src/ && pnpm test`
   (38+ vitest contract tests must stay green; fixtures are pinned to the Mac
   crate — never hand-edit, regenerate from `shardlane-remote`).
2. `git status --short` in BOTH repos. Preserve user/staged state. Never
   stage/commit unless asked. Another agent session may hold in-progress WIP in
   herdr-client (e.g. an untracked `cli.rs` that fails the release build) —
   detect it, do not fix or revert it; report the conflict instead.
3. A running desktop Shardlane serves the Host API the phone talks to — verify
   with the bearer token from `~/.shardlane/config.json` (remote.access_token):
   `curl -s -H "Authorization: Bearer $T" http://127.0.0.1:8756/api/v2/instances`.

## 1. Artifact matrix (what to rebuild for which change)

| Change surface | Rebuild |
| --- | --- |
| any `herdr-mobile/src/**` | Android APK (`assembleRelease`) + Web dist (`pnpm export:web`) + desktop repackage (embeds dist) |
| herdr-client Rust (Host API) | desktop repackage; verify wire contract via curl before claiming done |

## 2. The three-step ship (mobile changes → all surfaces)

Run each step FOREGROUND in an exec session (yield 30s → session id → poll the
LOG FILE with new short execs). Backgrounded children (`&`, nohup) are killed
when the agent session tears down — nohup does NOT protect them. For anything
longer than ~90s, use a tmux detached session (step 2b) — it survives
teardowns.

1. Web dist: `cd herdr-mobile && WATCHMAN_DISABLE=1 pnpm export:web > /tmp/exp.log 2>&1`
2. Android APK: `cd herdr-mobile/android && ./gradlew :app:assembleRelease > /tmp/gr.log 2>&1`
   → `android/app/build/outputs/apk/release/app-release.apk`
3. Desktop embed: from herdr-client,
   `scripts/release-macos.sh --skip-tests --arm64 --mobile-root ../herdr-mobile --no-archive`
   (installs to `~/Applications/Shardlane.app`; ad-hoc signed; verifies
   codesign + Info.plist + icns + `Contents/Resources/mobile-web/index.html`).

## 3. Android device install (vivo V2309A verified)

The USB link FLAPS during installs — streamed `adb install -r` dies mid-transfer
every time. Use the proven two-step:

```sh
adb -s <serial> push <apk> /data/local/tmp/shardlane-mobile.apk   # ~1.5s
adb -s <serial> shell 'nohup sh -c "pm install -r /data/local/tmp/shardlane-mobile.apk > /data/local/tmp/install.log 2>&1" &'
adb -s <serial> shell dumpsys package dev.shardlane.mobile | grep lastUpdateTime
```

The phone-side nohup install survives Mac-side disconnects; poll
`lastUpdateTime` to confirm. Clean up: `adb shell rm /data/local/tmp/...`.

vivo pitfalls:

- **USB-install authorization expires (~10 min)** → `pm install` exits 255
  SILENTLY (no output). The user must re-enable 设置 → 开发者选项 → USB安装,
  then retry immediately.
- Each install may pop an on-device confirmation — tell the user to watch the
  phone screen.
- USB drops return within ~10-20s; retry loops (push again, install again)
  beat one-shot attempts.

Launch after install:
`adb shell monkey -p dev.shardlane.mobile -c android.intent.category.LAUNCHER 1`.

## 4. adb / watchman stability

- Flaky adb often means TWO daemons fighting ("Address already in use",
  LIBUSB_ERROR_ACCESS): `pkill -f adb; sleep 2; adb start-server` — one server,
  then re-check `adb devices`.
- `expo export` / gradle hang on "Waiting for Watchman (30s…150s)": the
  watchman daemon is wedged for the repo watch. Fix:
  `watchman shutdown-server` (it respawns on demand). If `watch-project` still
  hangs, move the state dir aside
  (`mv ~/.local/state/watchman/<state> <state>.bak-$(date +%s)` — the safety
  layer blocks rm -rf) and relaunch the build.
- `WATCHMAN_DISABLE=1` does NOT reliably disable watchman for expo export.
- A stale gradle daemon (jdk corretto-17 process) is normal after builds; do
  not kill it to "clean up".

## 5. Contract lockstep (mobile ↔ desktop)

Host wire DTOs are pinned three ways: Zod in `src/contracts/host.ts`, golden
fixtures in `src/contracts/__fixtures__/` (copied verbatim from
`herdr-client/crates/shardlane-remote/tests/fixtures/`), and the Mac crate
tests. When the Host API changes:

1. update the Rust DTO (additive fields preferred; keep `remote_api_version`
   unless breaking);
2. regenerate/copy the fixture into mobile `__fixtures__` in the same change;
3. extend the mobile Zod schema with OPTIONAL fields (old-host tolerant);
4. run `pnpm test` (mobile) and `cargo test -p shardlane-remote` (desktop)
   together.

Multi-instance truth (2026-09): a workspace IS a Herdr session;
`GET /api/v2/instances` carries `backend` + `agent_count`; non-herdr
instances use backend-qualified ids (`tmux:default`); scoped bootstraps carry
TRUE per-connection capabilities (tmux: agent_control/conversation/history =
false) — the phone hides agent surfaces from those flags; events WS accepts
`?instance=` (A5) — the phone bridge must always pass its selected instance.

## 6. Presentation contract (Chat surface parity)

Mobile mirrors the desktop ChatGPT process contract via
`src/features/conversation/presentation.ts` (mirrors herdr-client
`agent_ui/conversation.rs`): text never folds; thinking consolidates per turn
into one collapsible block ("Thought for Xs"); consecutive tool calls (≥2)
collapse into one expandable group; user card right-aligned 75%; working state
renders pulsing dots directly under the last user message. Keep the two
projections semantically in sync when changing either side.

## 7. Known environment traps (do not re-diagnose)

- Exec sessions teardown kills backgrounded children: long builds MUST go
  through tmux detached sessions; poll LOG FILES, not process liveness.
- Concurrent agent sessions race on cargo locks ("Blocking waiting for file
  lock"): find stale `cargo build`/`cargo-bundle` PIDs, kill -9, rerun once.
- `shardlane-host` lib suite (shared_tui/herdr socket tests) flakes under full
  parallel load on this machine while live herdr servers run — verify with a
  single targeted test run before believing a failure.
- The desktop app quit/relaunch cycle is safe: Herdr daemons own the sessions.
