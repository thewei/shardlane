# Shardlane MyGo 0.10.0 — Code Audit

Status: **FAILED closure / remediation required**
Audit date: 2026-10-05
Branch: `rewrite/mygo`
Scope: current implemented 0.10 tree, with emphasis on application entrypoint, WorkspacePrimarySurface, Git Workbench, Diff/Commit/Branch, Right Panel, Files/Services/Preview, File Drop, Terminal Find, persistence, updater, tests and code structure.

This audit is source-level and runtime-contract oriented. Passing package tests/build artifacts are treated as evidence, not proof of end-to-end completion.

## 1. Executive verdict

0.10 has substantial implementation, but **does not currently meet release closure**.

The most important finding is that the packaged application entrypoint is wrong: `next/main.go` is a terminal right-click debug harness, not the Shardlane Native application. `go tool mygo build` therefore succeeds while packaging the wrong program.

Several Git Workbench paths also contain correctness bugs that unit tests do not cover, including initial repository binding, cross-operation generation cancellation, stale commit fences, UI-thread Git IO and snapshot truncation.

Recommended disposition:

```text
P0 blockers: fix before any visual polish/package acceptance
P1 correctness/security: fix before release candidate
P2 quality/performance: close before declaring 0.10 complete
Refactors: perform alongside affected fixes, not as a separate rewrite
```

## 2. Verification performed

Observed during this audit:

```text
GOTOOLCHAIN=go1.27.1 go test ./internal/gitworkbench ...       PASS
GOTOOLCHAIN=go1.27.1 go test -race ./...                      PASS
GOTOOLCHAIN=go1.27.1 go vet ./...                             PASS
GOTOOLCHAIN=go1.27.1 go tool mygo build                       PASS, but packages debug harness
```

A direct `go test ./internal/nativeui` run failed once in `TestTerminalRightClickReachesMouseTrackingProgram`; five isolated reruns passed. The failure was traced to a timing bug in the test helper itself (§P2-04).

`gofmt -l` over the audited packages returned no files.

## 3. P0 — release blockers

### P0-01 — `next/main.go` is a debug harness, not the application entrypoint

**Evidence**

`next/main.go` currently:

```text
exec.Command("herdr")
terminal.New(...)
ui.NewTester(...)
RightClickAt(...)
fmt.Println("herdr TUI started ...")
```

There is exactly one `package main` in `next/`.

No production main constructs:

```text
mygo App
main Window
nativeui.NewShell(...)
AttachWindow
Tray / Quick Panel
WithUserDataDir
```

`go tool mygo build` still succeeds and creates `Shardlane 0.10.0.dmg`, but binary strings contain the debug-harness markers (`herdr TUI started`, `view sent to TUI on right click`).

**Impact**

- package success is a false-positive;
- the DMG does not prove the actual Shardlane UI can launch;
- every production-only integration currently lacks a real composition root.

**Fix**

Restore a real application composition root. Move the terminal mouse experiment to a test/tool file outside the production entrypoint.

**Verify**

- packaged app launches the Shardlane Native window;
- layer-0 window exists;
- Sidebar/WorkspaceHeader/Terminal render;
- binary no longer contains debug harness output strings as reachable app behavior;
- isolated startup smoke exercises real Herdr bootstrap.

### P0-02 — production user-data persistence is not wired

**Evidence**

`NewShell()` initializes:

```go
scripts.NewStore("")
presets.NewPresetStore("")
```

`WithUserDataDir()` correctly rewires these stores, but there is no caller anywhere in the production tree because the real app entrypoint is absent.

**Impact**

Scripts and Task Presets are memory-only in the current packaged program path; restart persistence is not proven.

**Fix**

The restored main composition root must resolve MyGo `PathUserData` and pass `nativeui.WithUserDataDir(...)`.

**Verify**

Create script/preset → quit app → relaunch → item still exists.

### P0-03 — initial Git repository discovery returns before `ResolveRoot`

**Evidence** — `nativeui/git_service.go::syncGitContext`

Missing `rootOf[cwd]` returns the zero string. Initial `s.git.root` is also `""`:

```go
want := s.git.rootOf[cwd]
...
if want == s.git.root {
    return
}
```

The only writers to `rootOf` are inside this function's own asynchronous resolution path. Tests seed `shell.git.root` directly and therefore bypass production discovery.

**Impact**

First entry into a real Git repository may never call `ResolveRoot`; Changes/Diff/Branch remain unavailable despite the directory being a repo.

**Fix**

Represent cache state explicitly:

```text
missing
negative/not-repo
resolved(root)
```

Do not use `""` for both missing and negative-cache values.

**Verify**

Shell-level test starts with empty `rootOf`, selects a real temp repo cwd, and proves root resolution + snapshot + branch list without pre-seeding any Git state.

### P0-04 — one global Git generation cancels unrelated asynchronous operations

**Evidence**

`gitService.gen` is shared by:

- repository resolution;
- snapshot refresh;
- branch loading;
- commit preflight;
- gap expansion;
- Files directory loading;
- Terminal Find.

Every operation calls `s.git.gen.Add(1)` and `updateUI` only accepts the newest generation.

Example after root resolution:

```text
ensureGitSnapshot → gen 2
loadBranches      → gen 3
snapshot result(gen 2) is always dropped
```

Dropped callbacks are also responsible for clearing flags such as `refreshing`, `branchesLoading`, `committing`, `branchBusy`, `terminalFind.busy`, `loadingDirs`.

**Impact**

- snapshot can remain permanently stuck loading;
- branch list can remain permanently busy;
- expanding Files can cancel a Commit result UI update;
- Terminal Find can cancel Git refresh and vice versa;
- successful mutations can happen in Git while UI remains stuck in `InFlight` state.

**Fix**

Use operation-specific owners/generations/cancel functions:

```text
rootGen
snapshotGen
branchesGen
preflightGen
filesGen per root/path
terminalFindGen
contextExpansionGen
```

A higher-level workspace context token may additionally invalidate all child operations on Tab/repo switch, but unrelated operations must not cancel each other.

**Verify**

Deterministic overlap tests:

- root resolve + snapshot + branches all complete;
- Files expansion during commit does not drop commit completion;
- Terminal Find during refresh returns normally;
- obsolete Project context still rejects stale results.

### P0-05 — Commit stale-state fence accepts unreviewed content changes

**Evidence**

`RepoFacts.Signature` hashes:

```text
HEAD + git status --porcelain=v1 -z
```

Changing contents of an already-modified file from version A to version B normally leaves porcelain status unchanged (` M path`).

`Runner.Commit()` calls:

```go
r.revalidate(ctx, root, pre, snap)
```

where `snap` is the old `preflightSnap`. `revalidate` compares fingerprints against that same old snapshot, not a fresh live snapshot.

The existing fingerprint test explicitly creates a fresh snapshot manually and calls `revalidate` with it; the real `Commit()` path does not.

**Impact**

User reviews content A, file changes to content B with the same status, and Commit may commit B without forcing re-review.

**Fix**

Immediately before mutation, build fresh authoritative change identity for every selected path (or a fresh bounded snapshot) and compare against captured preflight.

For strong mutation fences, prefer Git object/content identity over presentation fingerprint shortcuts.

**Verify**

Real repo test:

1. modify `a.go`;
2. capture preflight;
3. rewrite `a.go` again while status remains `M`;
4. `Commit()` must return stale-state error and create no commit.

### P0-06 — UI thread can block on Git mutation mutex and post-commit Git refresh

**Evidence**

`submitCommit`, `switchBranch`, `createBranch` call:

```go
singleGitMutex.Lock()
go func() { ... }()
```

The lock acquisition occurs synchronously on the UI lane.

If a mutation is in flight, the second user action blocks the UI until the first mutation releases the lock.

Additionally commit success enters `win.Update` then calls `dirtyWorktreeEstimate`, which synchronously runs `cache.Refresh()` / multiple Git commands on the UI lane. It then schedules another refresh afterward.

**Impact**

- UI freeze up to mutation timeout;
- Git subprocess IO from UI callback violates execution rules;
- duplicate post-commit refresh work.

**Fix**

Move transaction serialization into `gitworkbench` as per-repository transaction ownership. Never acquire a blocking mutation lock from the UI lane. Commit completion should background-refresh once, then apply the result snapshot.

**Verify**

- second mutation click produces disabled/busy UI without blocking frame loop;
- frame test stays responsive while an injected 1s Git mutation runs;
- no `Runner.Read/Snapshot/Refresh` occurs inside UI update callback.

## 4. P1 — correctness / security bugs

### P1-01 — Commit transaction is not atomic single-flight inside the domain

`Runner.Commit()` performs selected-untracked `git add`, releases Runner mutation ownership, then reacquires it for `git commit` through `Stdin()`.

The UI's global mutex currently masks part of this for UI-initiated actions, but the domain contract itself does not own the full transaction and external/internal callers can interleave mutations.

Fix with one per-root transaction lock around revalidate → stage selected untracked → commit → result reconciliation.

### P1-02 — fingerprint is too weak for a commit safety fence

`fingerprint(...)` hashes only the first 64 KiB of patch plus patch length.

Same-length edits after 64 KiB can retain the same fingerprint. `TooLarge` files clear `Patch`, leaving a fingerprint derived mostly from metadata.

Use full selected-file Git blob/worktree identity or an independently streamed cryptographic digest for commit fencing. Presentation fingerprints may stay short/capped for cache invalidation, but must not be the mutation safety authority.

### P1-03 — Git stdout truncation is silent

`limitedWriter` swallows bytes past `MaxReadOutput` and still reports full success. It exposes no overflow bit.

A `git diff` larger than 32 MiB can be parsed as if complete. `Snapshot.Truncated` may remain false and last files/hunks may silently disappear.

Fix `limitedWriter` to record overflow and propagate `ErrTooLarge` / explicit partial snapshot state. Do not parse a silently cut patch as authoritative review content.

Also cap stderr; current stderr buffers are unbounded.

### P1-04 — untracked symlink can read outside repository

`gitworkbench.untrackedEntry` uses `os.Stat` + `os.ReadFile`, following symlinks.

An untracked symlink inside repo can point to a file outside repo and its content can enter Diff review.

Use `Lstat`, model symlink entries explicitly, and never follow an untracked symlink for content review by default.

### P1-05 — Files tree follows directory symlinks despite the documented rule

`filesview.ListDirect` detects `TargetDir`; Native UI then treats:

```go
isDir := e.IsDir || e.TargetDir
```

as expandable and calls `ListDirect(e.Path)`. `ListDirect` uses `os.Stat`, so it follows the symlink and may traverse outside the selected root.

This directly contradicts “directory symlink no-recursive-follow”.

Fix with canonical root containment and `Lstat`; symlink rows are non-expandable by default.

### P1-06 — Files preview follows symlinks outside root

`ReadPreview` uses `os.Stat` / `os.Open` and accepts an arbitrary absolute path from UI state. It does not prove the path remains inside the selected root.

Make preview API root-aware:

```go
ReadPreview(root, candidate)
```

and validate containment without following unsupported symlinks.

### P1-07 — Git path quoting/parsing is not complete for arbitrary filenames

- numstat command is not NUL-delimited;
- `parseNumstat` is newline/tab based;
- custom `unquotePath` handles a small subset of Git C quoting and not the full octal/byte form;
- synthesized untracked patch writes the raw path into textual patch headers.

Filenames containing newlines/tabs/non-UTF8 bytes can fail mapping between porcelain, numstat and patch data.

Use `-z` machine formats wherever available and carry raw/literal path fields separately from display strings. Unsupported textual patch-header cases should degrade explicitly instead of being misparsed.

### P1-08 — PreviewManager reuses controller, not window

`previewManager.controllers[targetURL]` caches `WindowController`, but every `OpenPreviewWindow()` calls `mygo.NewWindow()`.

Repeated clicks create unlimited preview windows; the manager map never removes closed targets.

The implementation does not meet “persistent project-scoped window reuse”.

Make controller own/track its window or let `PreviewManager` own `map[PreviewKey]*mygo.Window`, focus an existing live window, and remove it on close.

### P1-09 — Services are machine-wide, not Project-scoped

`observedPorts()` calls `ObserveAll()` then merges every PID's listening ports.

The selected Project can show ports belonging to unrelated applications.

Preserve PID/process association and intersect observed ports with known Project/Pane/Script process ownership where available. Unknown machine-wide ports should not be presented as this Project's services.

### P1-10 — File drop may paste into the wrong terminal

When hit testing fails, `HandleFileDropAt` falls back to the selected Pane.

This violates the exact-target rule and can paste a path into an unintended shell.

Fail closed when no visible terminal is under the pointer. Do not guess.

`terminalAtPoint` also uses inclusive boundaries and iterates a map; shared Pane edges can match more than one Pane nondeterministically. Use half-open rectangles and deterministic ordering/tie-breaking.

### P1-11 — File drop has no Windows shell capability gate

`SafeShellQuote` implements POSIX single-quote semantics but is called without checking active platform/shell.

On Windows/PowerShell/cmd this produces incorrect text. Disable until shell semantics are proven or add a shell-specific quoting adapter.

For POSIX interactive shells, quote all paths rather than maintaining a partial metacharacter list (`!` is currently missed and can trigger interactive history expansion).

### P1-12 — Changes tree cache is not invalidated when Git snapshot changes

`currentChangesTree` returns cached `s.changesTree` whenever non-nil. `applyGitSnapshot` swaps `s.git.snapshot` but does not clear `s.changesTree`.

After external change/commit/branch switch, center Diff can use the new snapshot while Right Panel still displays the old file tree.

Invalidate/rebuild tree on snapshot identity/signature change while preserving compatible collapse/filter state.

### P1-13 — Terminal Find incorrectly depends on Git generation

`runTerminalFind` calls:

```go
s.git.gen.Add(1)
s.updateUI(...)
```

Terminal search should be independent of Git. Git work can discard its result, and future shells without Git service would panic.

Give Terminal Find its own cancel/generation owner and normal UI dispatch helper.

## 5. P2 — behavior/performance/quality issues

### P2-01 — stale banner is immediately cleared

`applyGitSnapshot` may set `staleBanner = true`, then unconditionally sets it to false after swapping snapshot. The state effectively cannot persist.

Define banner semantics explicitly (e.g. external drift observed before refresh) and clear only on user/authoritative refresh completion.

### P2-02 — final unchanged-gap expansion uses additions as file length

`gapTrailingCount` derives final file tail from `cf.Additions`, which is changed-line count, not file total line count.

Final hunk expansion is therefore incorrect, often capped to one line.

Use actual new-side file line count/patch metadata and background `FileLineCount` with the same root/path safety rules.

### P2-03 — split Diff right context gutter uses old line number

`renderDiffSplitLine.drawSide` uses `OldLine` first regardless of left/right side. Right-side context rows should display `NewLine`.

Add a side parameter or separate left/right rendering functions.

### P2-04 — terminal mouse test is flaky because its test helper rereads old bytes

`dropPipe.take(9)` returns immediately whenever accumulated buffer length is already ≥9. After the right press, the release assertion calls the same method without consuming/resetting prior bytes; if release is delayed slightly, the old press buffer is returned and the test fails.

Wait for buffer growth beyond the previous length or consume bytes between assertions.

### P2-05 — word diff is incomplete

Unified mode only pairs the most recent delete line with the first following add in a multi-line replace run. Split rows currently do not carry word-diff ranges at all.

If 0.10 claims word-level diff parity, pair change runs deterministically with bounded heuristics in both layouts.

### P2-06 — context gap cannot actually collapse cleanly

Expanded context rows are inserted but the same gap row remains. `collapseGap()` exists but is not wired into the button behavior; repeated clicks re-request expansion.

Model gap state explicitly: collapsed / expanded-N / full; render appropriate Show more / Collapse actions.

### P2-07 — Diff virtualization still performs heavy precomputation on UI lane

`ui.List` virtualizes element creation, but `rebuildDiffRows()` flattens all patch lines and computes `WordDiff` synchronously on the UI lane. Syntax highlighting tokenizes visible lines again during render on every frame.

For large diffs:

- move flattening/word-pair calculation to immutable background presentation snapshots;
- cache syntax spans by `(snapshot,file,line,theme)` or highlight a bounded region once;
- apply new row model via generation guard.

### P2-08 — Files preview performs file IO synchronously in click handler

`openFilePreview()` directly calls `filesview.ReadPreview()` on the UI lane. It is capped to 1 MiB but can still stall on network/slow storage.

Use the same asynchronous snapshot pattern as directory listing.

### P2-09 — platform helpers are duplicated and partially incorrect

Current platform behavior is split across:

```text
nativeui/git_platform.go
nativeui/script_actions.go
preview/preview.go
right_panel.go wrapper
```

Examples:

- clipboard via `pbcopy` / `clip` / `wl-copy`;
- reveal path via `open -R` / `xdg-open`;
- editor open via `open -t` / `xdg-open -t`;
- external browser only implemented on macOS in Preview.

`xdg-open -t` is not the macOS `open -t` equivalent, and Windows reveal/browser paths are incomplete.

Create one `platform` / desktop-actions adapter:

```text
ClipboardWrite
OpenFile
RevealFile
OpenURL
```

Prefer MyGo/native APIs where available. Avoid `Start()` without a corresponding wait/reap policy for short-lived helper processes.

### P2-10 — several production files violate the project's own size guidance

Notable production files:

```text
nativeui/git_service.go       601 LOC
history/live.go               561
app/launch_service.go         539
gitworkbench/changes.go       535
nativeui/right_panel.go       500
nativeui/diff_surface.go      488
agent/live_handoff.go         435
gitworkbench/runner.go        400
nativeui/shell.go             391
nativeui/diff_rows.go         363
```

For 0.10-specific code, split now while repairing bugs:

```text
git_service_context.go
git_service_snapshot.go
git_service_mutation.go
git_service_context_expansion.go
right_panel_files.go
right_panel_services.go
right_panel_preview.go
diff_render_unified.go
diff_render_split.go
diff_find.go
changes_snapshot.go
changes_untracked.go
changes_parsing.go
```

Do not perform a broad unrelated rewrite of older domains solely to hit LOC numbers.

### P2-11 — temporary import pins / dead bridge code remain

Examples:

```go
var _ = strings.TrimSpace
var _ = gitworkbench.StatusModified
var _ = gitworkbench.StatusAdded
_ = tokens
_ = context.Background()
```

These are strong signals of unfinished cleanup. Remove unused imports/variables and let the compiler enforce cleanliness.

### P2-12 — duplicated/reinvented helpers should use standard/shared utilities

Candidates:

- custom `contains/indexOf` → `strings.Contains`;
- custom `utf8Valid` → `unicode/utf8.Valid` (the custom validator is less correct);
- custom `itoa` → `strconv.Itoa` unless profiling proves otherwise;
- `fmtSscanf` indirection provides little value;
- `copyToClipboard` merely forwards to another Shell helper;
- path/open/browser helpers should move to one platform adapter.

Avoid extracting tiny one-use visual code just for abstraction count; focus shared behavior and safety policy.

### P2-13 — behavioral constants are duplicated

Examples:

- `services.DefaultPortScanInterval = 10s`, while UI independently hardcodes `10*time.Second`;
- UI hardcodes port observation timeout `3s`;
- Terminal Find timeout independently hardcodes `3s`;
- Diff reveal oscillation window `350ms` is unnamed;
- tree indent `14` appears in shared components and Right Panel;
- context expansion has both `100` and `1000` without one cohesive policy;
- diff gutter widths 38/34 are local magic numbers.

Extract semantic policy constants where behavior must stay synchronized:

```text
PortRefreshInterval
PortProbeTimeout
TerminalSearchTimeout
DiffRevealGuardWindow
TreeIndentWidth
ContextExpansionStep / ContextExpansionMax
DiffGutterWidth
```

Do not extract every one-off padding/font size; those belong in existing Design System tokens when reused.

## 6. Additional findings / incomplete planned functionality

### UPDATER — NOT IMPLEMENTED

No `github.com/egoist/mygo/plugins/updater/native` import or app integration exists in `next/`, and `mygo.json` contains no updater configuration.

MyGo v0.2.7 on the audited machine does include the native updater package, so “plugin unavailable” is not a valid closure reason.

### PREVIEW REUSE — PARTIAL

Loopback validation is implemented, but project/target window reuse and close cleanup are not.

### SCRIPT PERSISTENCE — CODE EXISTS, PRODUCTION WIRING MISSING

The store implementation is present and testable, but the real composition root is absent; production persistence is therefore not achieved.

### FILE DROP — PARTIAL

Window wiring and terminal Paste exist in `Shell.AttachWindow`, but the production application does not currently construct/attach that Shell because of P0-01. Exact hit targeting also fails open to selected Pane (§P1-10).

### TERMINAL FIND — PARTIAL

Native bar exists, but generation ownership is incorrect and the production application entrypoint is absent.

## 7. Refactor recommendations

Do these together with correctness fixes, not before them.

### 7.1 Introduce operation-specific async controllers

Avoid one “god generation number”. Use a small reusable pattern:

```go
type AsyncSlot[T any] struct {
    gen    atomic.Uint64
    cancel context.CancelFunc
    busy   bool // UI lane
}
```

or explicit domain-specific controllers when cancellation semantics differ.

Important: the reusable part is lifecycle ownership, not a generic abstraction that hides domain behavior.

### 7.2 Create one desktop platform adapter

Recommended ownership:

```text
internal/platform/desktop_actions.go
```

with OS-specific files where necessary.

### 7.3 Separate Git domain transaction locking from UI busy state

UI should ask for mutation and show busy/unavailable. `gitworkbench` should own per-canonical-root transaction serialization.

Canonicalize repository root before using it as a lock/cache key to avoid the same repo being addressed through path aliases/symlinks.

### 7.4 Separate review identity from presentation fingerprint

Use two concepts:

```text
DisplayFingerprint   // cheap/cache/UI
MutationIdentity     // strong, authoritative
```

Never reuse an intentionally capped presentation hash as a commit safety fence.

## 8. Required remediation order

### Gate A — make the product runnable

```text
A1 restore real main/app composition root
A2 wire PathUserData
A3 real startup/package smoke
```

### Gate B — make Git context reliable

```text
B1 fix initial ResolveRoot cache-state logic
B2 split async generations by operation
B3 remove UI-thread Git mutex/read IO
B4 invalidate Changes tree with snapshot
```

### Gate C — make mutations safe

```text
C1 fresh live mutation identity at commit
C2 strong file identity beyond first 64 KiB
C3 full transaction single-flight per canonical repo
C4 add stdout overflow signaling
C5 exhaustive index/stale tests
```

### Gate D — close filesystem/input safety

```text
D1 stop Files/untracked symlink traversal
D2 root-aware preview containment
D3 exact file-drop targeting + Windows gate
D4 Git filename NUL-safe parsing
```

### Gate E — behavior/performance cleanup

```text
E1 Diff line number/gap/word-diff fixes
E2 background/cached diff presentation computation
E3 Preview window reuse
E4 project-scoped Services
E5 updater integration or exact real blocker
E6 flaky test repair
```

### Gate F — code structure cleanup

```text
F1 split 0.10 files >450 LOC
F2 consolidate platform helpers
F3 extract shared behavioral constants
F4 remove import pins/custom stdlib replacements
```

## 9. Closure criteria after fixes

Do not declare 0.10 complete until all of the following hold:

```text
real Shardlane app is the packaged main binary
go test ./... passes repeatedly
go test -race ./... passes
go vet ./... passes
git diff --check passes
real clean repo discovers Git without test seeding
concurrent branches/snapshot/files/find operations do not cancel each other
content-only post-review mutation is rejected before commit
unrelated staged work remains untouched
32+ MiB diff degrades explicitly, never silently truncates
symlink cannot escape Files/Diff root policy
right-panel Changes refreshes with snapshot
Terminal/Diff/Commit focus and file-drop routing are exact
Preview window reuse is real
Scripts/Presets survive restart
Updater status is truthful
packaged 0.10 app opens the real Native window
```

## 10. Current verdict matrix

| Area | Verdict |
|---|---|
| App composition / package | **P0 BROKEN** |
| WorkspacePrimarySurface model | PARTIAL / structurally good |
| Initial Git context | **P0 BROKEN** |
| Async Git/UI orchestration | **P0 BROKEN** |
| Git snapshot/parser | PARTIAL, multiple P1 bounds/path issues |
| Commit correctness | **P0/P1 UNSAFE stale fence** |
| Branch mutation | PARTIAL; UI orchestration needs repair |
| Changes panel | PARTIAL; stale tree cache |
| Unified Diff | PARTIAL |
| Split Diff | PARTIAL; right context gutter bug |
| Word diff | PARTIAL |
| Files | PARTIAL; symlink/root escape risk |
| Services | PARTIAL; machine-wide instead of Project-scoped |
| Preview | PARTIAL; no window reuse/cleanup |
| File Drop | PARTIAL; wrong-target fallback / Windows quoting gap |
| Terminal Find | PARTIAL; wrong async generation owner |
| Scripts | implementation exists; production persistence blocked by main |
| Presets | implementation exists; production persistence blocked by main |
| Updater | **NOT IMPLEMENTED** |
| Tests | mostly strong but package signal includes a known flaky test |
| Race detector | PASS in audit run |
| Vet | PASS in audit run |
| Formatting | PASS |
