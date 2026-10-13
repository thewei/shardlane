---
name: shardlane-install
description: "Build the newest Shardlane.app from the herdr-client working tree and replace the copy the user runs day to day in ~/Applications, restarting the running instance. Use whenever the user says 编译最新app / 覆盖安装 / 更新我正在用的 Shardlane / build and install the latest app / deploy the newest build / 给我装上最新版 — i.e. they want the code that is on disk right now running in their real app, not left in build/ output."
---

# Shardlane Install

One script performs the whole verified flow — vet, build, bundle checks,
graceful restart, replace, relaunch, liveness — with each stage gating the
next. Run it from anywhere inside the repository and read its staged output;
do not re-implement the steps by hand.

## Run

```sh
bash .agents/skills/shardlane-install/scripts/install.sh
```

Stages, in order (each prints `==> N/7`):

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

`SHARDLANE_INSTALL_DST=/Applications/Shardlane.app` overrides the target.

## Reading the result

- `done: Shardlane <id> <version> installed at <path> and running (pid N)` —
  report success with the pid and the dirty-file count from stage 2.
- Any `ERROR: ...` line names the failed gate. Report it verbatim and stop:
  - *identity mismatch* — the target holds a different app; ask before
    replacing.
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
