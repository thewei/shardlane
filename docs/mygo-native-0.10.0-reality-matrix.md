# Shardlane MyGo 0.10.0 — Live RealityMatrix

Status: **closed for 0.10.0 — every inherited item resolved** (GWB-001)
Source audit: `docs/reference-godiff-shardlane-0.10-audit.md` (plan input)
Rules: every row resolves to `FIXED`, `REMOVED`, or `EXPLICITLY_DEFERRED_WITH_REAL_BLOCKER`
before the 0.10 closure audit may claim the capability complete. Evidence must be
end-to-end (source + test + runtime where applicable), never package-existence.

Baseline evidence recorded 2026-10-05 against `rewrite/mygo` working tree.
Dispositions closed 2026-10-05 after the 0.10 implementation pass; see
`docs/mygo-native-0.10.0-closure-audit.md` for the full evidence map.

| # | Inherited item | Baseline evidence (0.9 tree) | Disposition | 0.10 evidence |
|---|---|---|---|---|
| 1 | Files render-time IO | `right_panel.go` called `filesview.ListDirect` inside `renderDirectoryLevel` on cache miss | **FIXED** | `requestDirectory`/`beginDirectoryLoad` (`nativeui/right_panel.go`): production lane reads in background with generation guard; `TestProductionDirectoryLoadNeverBlocksRender` proves the production path applies nothing synchronously; render reads cached snapshots only |
| 2 | Scripts store unpersisted | `scripts.NewStore("")` memory fallback | **FIXED** | `WithUserDataDir` wires `PathUserData/scripts.json` in `main.go`; tests cover store persistence |
| 3 | Script Run stub | `runScriptCommand` only set `s.status` | **FIXED** | Herdr-backed transaction (`nativeui/script_actions.go`): `CreateTab` in the script cwd → `ShellReady` → `SendAgentKeys`; no client-owned process; busy-guard + honest failure status |
| 4 | Services fake ports | literal `3000/5173/8080` rendered | **FIXED** | `PortProbe.ObserveAll` + `observedPorts()` cached snapshot; `TestRightPanelServicesSurfaces` asserts no sample ports and the honest empty state |
| 5 | Lazygit placeholder | `lazygitToolView` label + status change | **REMOVED** | `SurfaceLazygit`, `lazygitToolView` and all panel actions deleted; `TestRightPanelLazygitRemoved` pins the surface set to Changes/Files/Services |
| 6 | Preview per-click controller | `preview.NewWindowController()` per click | **FIXED** | `previewManager` (`nativeui/right_panel.go`) owns persistent controllers keyed by target URL, reused per click |
| 7 | gitintel not product-visible | `gitintel.NewService()` constructed, never consumed | **REMOVED** | `internal/gitintel` deleted; `internal/gitworkbench` is the single Git cache (`Cache`, per-root, stale-while-refresh); Shell consumes it end-to-end |
| 8 | File Drop helper-only | no `OnFileDrop` wiring, no paste | **FIXED** | `win.OnFileDrop` wired in `AttachWindow`; `HandleFileDropAt` hit-tests the visible pane geometry and calls `terminal.Paste` (never appends newline); drops while Diff/Commit is visible are refused (`TestFileDropGuardOnHiddenTerminal`) |
| 9 | Terminal Find adapter-only | `Manager.CopySearch` live, no UI | **FIXED** | Native find bar (`nativeui/terminal_find.go`) on the Terminal surface only, driven by `pane.copy_search` (Protocol 22 adapter); surface-aware Cmd+F (`handleSurfaceShortcuts`) |
| 10 | Updater defer reason wrong | "plugin unavailable" was false — `mygo@v0.2.7/plugins/updater/native` exists | **FIXED (code) / EXPLICITLY_DEFERRED_WITH_REAL_BLOCKER (activation)** | `mygo.Use(native.Plugin)` + `updater.MenuItem()` wired in `main.go`. Exact remaining blocker: **release signing infrastructure** — no `mygo keygen` update keypair, no `updates.publicKey`/publish target in `mygo.json`, no signed-release CI. The plugin is inert until that exists; it is no longer a code gap |
| 11 | Version metadata stale | `mygo.json` said `0.8.0` | **FIXED** | `mygo.json` version `0.10.0`; `go tool mygo build` produced `Shardlane.app` + `Shardlane 0.10.0.dmg` with `CFBundleShortVersionString = 0.10.0` |

## Disposition ledger (updated as work lands)

- 2026-10-05 — matrix created from live source inspection (GWB-001).
- 2026-10-05 — all eleven items resolved; evidence recorded above and in the closure audit.
