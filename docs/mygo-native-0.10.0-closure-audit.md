# Shardlane MyGo 0.10.0 — Closure Audit

Date: 2026-10-05
Branch: `rewrite/mygo`
Scope: Git Workbench & Unified Primary Surface (plan:
`docs/mygo-native-0.10.0-git-workbench-primary-surface-plan.md`)
Reality gate: `docs/mygo-native-0.10.0-reality-matrix.md` (all eleven inherited
items resolved)

Verdict legend: `DONE` (end-to-end evidence), `PARTIAL` (implemented with
evidence, named gaps remain), `EXPLICITLY_DEFERRED` (real blocker named),
`REMOVED`.

## 1. Release gates actually run

| Gate | Command | Result |
|---|---|---|
| Full test suite | `GOTOOLCHAIN=go1.27.1 go test ./...` | PASS (all packages) |
| Race detector | `go test -race ./internal/{gitworkbench,filesview,scripts,services,preview,nativeui}/...` | PASS |
| Formatting | `gofmt -l .` | clean |
| Vet | `go vet ./...` | clean |
| Native build | `go tool mygo build` | built `build/darwin-arm64/Shardlane.app` + `Shardlane 0.10.0.dmg` |
| Whitespace | `git diff --check`, `git diff --cached --check` | clean |
| Package metadata | PlistBuddy on built app | `CFBundleShortVersionString = 0.10.0`, identifier `com.whstudio.shardlane.next` |

## 2. 0.9 RealityMatrix inherited fixes

All eleven rows: see the RealityMatrix for per-item evidence.

| Item | Verdict |
|---|---|
| Files render-time IO | DONE — production lane background + generation guard; `TestProductionDirectoryLoadNeverBlocksRender` |
| Scripts persistence | DONE — `WithUserDataDir` → `PathUserData/scripts.json` |
| Script Run stub | DONE — Herdr transaction (CreateTab → ShellReady → SendAgentKeys); no client-owned process |
| Services fake ports | DONE — `PortProbe.ObserveAll` snapshot; no sample ports (`TestRightPanelServicesSurfaces`) |
| Lazygit placeholder | REMOVED |
| Preview per-click controller | DONE — persistent `previewManager` keyed by target |
| gitintel unused | REMOVED — package deleted; `gitworkbench` is the single Git cache |
| File Drop helper-only | DONE — `OnFileDrop` wired, exact-target paste, surface guard |
| Terminal Find adapter-only | DONE — native find bar on Terminal surface over `pane.copy_search` (Protocol 22 adapter) |
| Updater wrong defer reason | DONE (code) + EXPLICITLY_DEFERRED (activation: signing/release infrastructure — no update keypair, no `updates.publicKey`/publish target; `mygo.Use(native.Plugin)` + menu item are wired) |
| Version metadata | DONE — 0.10.0 in `mygo.json` and the built bundle |

## 3. Product capabilities

| Capability | Verdict | Evidence |
|---|---|---|
| Lazygit removal | DONE | `TestRightPanelLazygitRemoved`; source sweep: no `SurfaceLazygit`/`lazygitToolView` |
| WorkspacePrimarySurface | DONE | `workspace_surface.go` state machine; full §6 transition table in `workspace_surface_test.go` |
| Terminal attachment retention / input isolation | DONE | attachments survive surface switches (`TestSurfaceSwitchRetainsTerminalState`); canvas renders only on Terminal surface (`TestHiddenTerminalSurfaceRendersNoTerminal`); drops refused when hidden (`TestFileDropGuardOnHiddenTerminal`) |
| Workspace Header v2 | DONE (headless) | `workspace_header.go`: project/repo + path, branch chip, Terminal⇄Changes segmented switch, per-surface actions |
| Sidebar compact refresh + structure lines | DONE | `sidebarTopAction` icon row with labels/tooltips; density-token row heights (26/28/32); treeRow connector guides preserved |
| Right Panel Changes/Files/Services | DONE | v2 surfaces wired; per-Tab last-tool restore; Changes default when Git changes exist |
| Git runner/repository | DONE | argv-only `exec.CommandContext`, read/mutate policy, single-flight per root, classification (`runner.go`, `TestRunnerMutationSingleFlight`, `TestRunnerReadTimeout`) |
| Change snapshots | DONE | working tree + index vs HEAD + untracked; unborn-branch empty-tree; bounds (4 MiB/file, 1000 untracked, 64 MiB snapshot); fingerprints (`changes.go`, `TestSnapshotMatrix`, `TestSnapshotRepositoryWithoutHead`) |
| Clean-room patch parser | DONE | multi-file fixtures incl. quoted/exotic paths, rename/copy/mode/binary, no-newline marker, pathological caps (`models_test.go`) |
| Word diff | DONE | bounded LCS word diff, Unicode-safe, rewrite suppression (`worddiff.go`, `TestWordDiff*`) |
| Syntax highlighting | DONE | Chroma v2.27.0 (MIT; dependency audit in `highlight.go`) behind `Highlighter`; plain/binary/unsupported fallback |
| Changed-file tree | DONE | dirs-first stable sort, one-child compaction, filter, collapse, ancestor auto-open (`tree.go`, `TestTree*`); 1000-file filter 0.35 ms measured (target <16 ms) |
| Unified Diff | DONE | virtualized `ui.List` rows, gutters, add/delete washes, syntax spans, word highlight |
| Split Diff | DONE | change-run delete/add pairing, dual gutters, one-sided rows |
| Unchanged-context expansion | DONE | background `FileLines` lane, numbered context, 1000-line cap |
| Bidirectional Changes↔Diff sync | DONE | click→reveal + top-visible→select with 350 ms oscillation guard (`TestDiffFileClickRevealsAndSyncs`) |
| Diff Find | DONE | surface-aware Cmd+F, match index, next/prev + scroll into view |
| Copy Path / Open in Editor | DONE | clipboard helper; argv-only open (no shell interpolation) |
| Commit surface | DONE (UI headless) | subject/body/checklist/totals/branch identity; `TestCommitSurfaceFlow` |
| Stale-state Commit fence | DONE | capture at open; revalidate HEAD/branch/signature/fingerprints; fail-closed (`TestCommitStaleStateFailsClosed`, `TestCommitStaleHeadFailsClosed`, `TestCommitChangedFingerprintFailsClosed`) |
| Unrelated-index preservation | DONE | pathspec-commit transaction with explicit untracked staging; `TestCommitUnrelatedStagedWorkPreserved` and `TestCommitUnselectedChangesRemain` on real repos |
| Commit failure recovery | DONE | hook-failure fixture proves index untouched + honest recovery copy (`TestCommitHookFailureLeavesRecoverableState`); merge-in-progress fail-closed (`TestCommitMergeInProgressFailsClosed`) |
| Branch list/switch/create | DONE | local list, validated switch (clean/dirty-compatible/conflict-refusal fixtures), `switch -c` create with `check-ref-format` validation (`branches_test.go`) |
| Optional history/compare | REMOVED from 0.10 scope | not implemented, not claimed (plan §25 allows) |
| Files no-render-IO | DONE | evidence above |
| Services real-port wiring | DONE | honest empty state; render reads cached snapshot only |
| Preview manager lifecycle | DONE (headless) | persistent controller map + reuse; in-app window reuse behavior needs the real-app pass (below) |
| File Drop actual paste | DONE (guard + adapter) | paste call verified at the adapter boundary; end-to-end drag in the real app needs the real-app pass (below) |
| Terminal Find actual UI | DONE (headless) | bar renders only on Terminal surface; adapter executed via Herdr; live-drag/verify in real app pending |
| macOS packaged artifact version = 0.10.0 | DONE | built bundle verified |
| Performance targets | PARTIAL | tree filter 0.35 ms (measured); Git read deadline 5 s enforced by runner (tested); warm surface switch is an O(1) state flip with no reattach by construction but was not instrumented in the running app |
| Privacy/security audit | DONE | runner logs operation class/duration only; no diff/file/commit content logging; argv-only Git and editor/clipboard/preview openers; no shell interpolation (`grep` sweep clean) |

## 4. Invariant proofs

- **No Godiff internal source copied/imported**: `grep -rn godiff internal/ main.go`
  — zero hits; Chroma is the only new dependency (MIT).
- **No active Lazygit target**: source sweep clean; surface set pinned by test.
- **No second Git cache**: `internal/gitintel` deleted; only `gitworkbench.Cache` exists.
- **No render-time filesystem/Git/network/Herdr work**: production render paths read
  snapshots only (`TestProductionDirectoryLoadNeverBlocksRender`, changes/diff render
  over cached snapshots); background lanes carry generations.
- **Hidden Terminal receives no input**: `terminal.View` elements exist only under the
  Terminal surface; file drops are refused when Diff/Commit is visible.
- **No force/reset-hard/destructive mutation**: `grep` sweep over `gitworkbench`
  finds no `reset`/`checkout`/`clean`/`--force`; switches use `git switch`.
- **Unrelated staged work cannot be silently consumed**: pathspec-commit fixtures on
  real repositories (critical test `TestCommitUnrelatedStagedWorkPreserved`).
- **No user code/diff/commit bodies in logs**: runner emits operation class, duration,
  counts; stderr stays inside returned errors for the UI, never logged.
- **Mutation test matrix**: every required case ran against real temporary Git
  repositories (see `mutation_matrix_test.go`, `branches_test.go`) — tracked modified,
  untracked, deleted, rename, binary, unrelated staged, selected+unselected, hook
  failure, clean/dirty-compatible/conflicting switch, external HEAD/index mutation,
  paths with spaces/unicode/single quotes.

## 5. Honest gaps (not silently absorbed)

1. **Real-app macOS acceptance scenarios were not driven in this pass.** The
   UI acceptance contract (`.agents/skills/ui-acceptance-testing/SKILL.md`,
   Computer Use MCP with isolated Herdr/tmux ground truth) was not executed;
   surface-level behavior is proven by headless render tests
   (`ui.NewTester`) and the packaged build, not by a driven interactive
   session. A 0.10.1 follow-up should run the scripted scenarios: surface
   switching against a live Herdr instance, real drag-and-drop paste, and
   in-app preview window reuse/focus.
2. **Updater activation** waits on signing/release infrastructure (keypair +
   publish target + CI), documented in the RealityMatrix row 10.
3. **Optional history/compare** (plan §25) is out of 0.10 scope by design.
4. **`Viewed` markers, hunk-level staging, remote branch management** remain
   out of core 0.10 per plan.

## 6. Task ledger coverage

GWB-001..012 (inherited fixes), GWB-020..023 (Lazygit removal), GWB-030..043
(primary surface + terminal lifecycle), GWB-050..057 (header v2), GWB-060..066
(sidebar refresh), GWB-070..074 (right panel v2), GWB-080..099 (runner + change
snapshots), GWB-110..124 (patch/word-diff/highlight), GWB-130..147 (changes
tree/panel), GWB-150..194 (diff UI/navigation/find), GWB-200..227 (commit
model/transaction/UI), GWB-230..236 (branch switch/create), GWB-260..263
(shortcuts/focus) — implemented with the verifications listed above.
GWB-250..253 (optional history) intentionally not started. GWB-270..276
(end-to-end closure) partially satisfied pending the real-app pass (gap 1).
GWB-280..290: this audit.
