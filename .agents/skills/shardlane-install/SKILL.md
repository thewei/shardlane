---
name: shardlane-install
description: "Build the newest Shardlane.app from the herdr-client working tree and replace the copy the user runs day to day in ~/Applications, restarting the running instance. Use whenever the user says 编译最新app / 覆盖安装 / 更新我正在用的 Shardlane / build and install the latest app / deploy the newest build / 给我装上最新版 — i.e. they want the code that is on disk right now running in their real app, not left in build/ output."
---

# Shardlane Install

One script performs the whole verified flow — vet, build, artifact checks,
graceful restart, replace, relaunch, liveness — with each stage gating the
next. Run it from anywhere inside the repository and read its staged output;
do not re-implement the steps by hand.

The script is **platform-aware** (2026-10-16, per user): it branches on
`uname -s` and installs what the current machine runs.

## macOS (`Darwin`)

Builds and replaces the `Shardlane.app` bundle (default target
`~/Applications/Shardlane.app`, override `SHARDLANE_INSTALL_DST`). Stages, in
order (each prints `==> N/7`):

1. **vet** — `go tool mygo vet .`; a broken tree stops before any build.
2. **build** — `go tool mygo build` over the **working tree**, so uncommitted
   changes (the user's WIP included) are in. The script announces how many
   dirty files it is building; always repeat that number when reporting.
3. **bundle checks** — executable present, `Info.plist` lint, bundle id +
   version readout.
4. **target preflight** — refuses to overwrite an installed bundle whose
   `CFBundleIdentifier` differs from the build (never clobber a different
   app).
5. **quit** — only the instance whose executable lives under the target:
   AppleScript quit, up to 10 s grace, then SIGTERM. If it still will not
   die the script **aborts** — by design there is no SIGKILL, which would
   skip the app's session save.
6. **replace** — `rm -rf` + `ditto` from `build/darwin-arm64/Shardlane.app`,
   lint again on the installed copy.
7. **launch and verify** — `open`, then a two-stage liveness check so a
   crash-on-launch is caught, not reported as success.

## Linux (`Linux`)

Builds the user-local Linux app and installs it through the **installer that
`go tool mygo build` generates** (`build/linux-amd64/install.sh`) — the
framework's installer owns staging, whole-directory replacement and the
applications-menu entry; this script only gates and drives it. Target:
`~/.local/shardlane.app` + `~/.local/share/applications/shardlane.desktop`
(override the app dir with `SHARDLANE_INSTALL_APP_DIR`; no root needed).

1. **vet** — same gate as macOS.
2. **build** — same `go tool mygo build`; announces the dirty-file count.
3. **artifact checks** — executable, generated installer, and the
   `<arch>` archive must all exist under `build/linux-amd64/`.
4. **target preflight** — reports fresh install vs replace. Linux has no
   bundle identity file, so the macOS identity guard does not apply; the
   MyGo-owned target name is the identity.
5. **quit** — graceful TERM with 10 s grace, then abort (no SIGKILL, same
   rule). Scope is the target **plus this repository's `build/` outputs**:
   instances launched day to day from a build output directory are this
   repo's own products, never user-authored state, so they are replaced too.
6. **install** — runs the generated installer; then verifies the executable
   and the desktop entry landed.
7. **launch and verify** — `gtk-launch shardlane` when a Wayland/X display
   is reachable, then a two-stage liveness check; without a display session
   it reports the install and points at the applications menu.

`libwebkit2gtk-4.1` is the Chat surface's runtime dependency; the generated
installer warns when it is missing.

## Run

```sh
bash .agents/skills/shardlane-install/scripts/install.sh
```

## Reading the result

- `done: Shardlane <id|version> installed at <path> and running (pid N)` —
  report success with the pid and the dirty-file count from stage 2. On a
  Linux box without a display session the done line says the app was
  installed and names the applications menu instead.
- Any `ERROR: ...` line names the failed gate. Report it verbatim and stop:
  - *identity mismatch* (macOS) — the target holds a different app; ask
    before replacing.
  - *still running* — the user must quit the app manually; rerun after.
  - *vet/build failures* — fix the tree first; this script never patches
    around a failed gate.

## Rules

- The script never touches git state; keep it that way. Building WIP is the
  point — but say explicitly that the installed build contains uncommitted
  work.
- Do not run ad-hoc `rm -rf` on the target yourself to "fix" a failed stage;
  rerun the script after resolving the named error.
- If the user only wants a rebuilt bundle without installing, `go tool
  mygo build` alone is enough — this skill is for the replace-and-restart
  flow.
