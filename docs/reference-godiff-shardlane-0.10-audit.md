# Shardlane 0.10 Reference & Reality Audit — Godiff × Current MyGo Client

Status: **planning / pre-0.10 audit**
Audit date: 2026-10-05
Target release: `0.10.0`
Architecture authority: `docs/client-product-architecture.md`

> **2026-10-05 addendum (product decision, supersedes §2 and the §13
> "source-code reuse: REJECT" row for the Diff/Changes presentation):**
> Wilson directed that Godiff's review presentation be adopted directly:
> `egoist/godiff` is cloned at `../godiff` (commit `88b89e0`) and its view
> layer ported into `next/internal/nativeui/gd_*.go` (palette, rows, surface,
> find, tree) plus `next/internal/codehl` (its `internal/highlight`, Chroma
> MIT). Godiff's Git/file loading is **not** ported: `gitworkbench` remains
> the single Git authority and feeds the ported view from
> `ChangesSnapshot`/`ChangeFile`. Note Godiff ships no LICENSE file; this
> adoption is the product owner's call for this private codebase. Comments,
> image previews and Godiff's History/Commit views remain out of scope (see
> §13).
>
> **Same-day operations addendum:** on explicit direction to integrate the
> common Git operations of mature clients (Fork), `gitworkbench/operations.go`
> adds stage/unstage/discard/delete-untracked/stash/fetch/pull --ff-only/
> push/branch delete/soft undo/upstream reading behind the single-flight
> Runner, and the Diff toolbar carries Fetch/Pull/Push, ahead/behind and the
> branch menu (switch/create/delete, stash pop/drop). Destructive operations
> run only through the confirm dialog; the package bans (no force, no
> `reset --hard`, no `clean`) are unchanged — untracked deletion is a direct
> bounded `os.Remove`, the sanctioned `git clean` replacement.

Reference snapshot inspected:

- `egoist/godiff` — `88b89e0` (`main`, 2026-10-05 14:11 +08:00)
- Shardlane — current `rewrite/mygo` working tree after the 0.9 implementation pass
- MyGo — repository pin `v0.2.7`

This document has two purposes:

1. decompose Godiff's useful product/UI architecture without copying its app wholesale;
2. correct the gap between Shardlane's 0.9 closure claims and the actual current code before 0.10 starts.

## 1. Executive conclusion

The right next step is not “embed Lazygit better.”

It is:

> **Remove Lazygit and make Git review a first-class native Shardlane workspace surface.**

The key structural change is a single `WorkspacePrimarySurface` owner inside `/workspace`:

```text
/workspace
└─ WorkspacePrimarySurface
   ├─ Terminal
   ├─ DiffReview
   └─ Commit
```

The Right Panel becomes a **navigator/inspector**, not a place where every tool must render its full UI:

```text
Right Panel
├─ Changes
├─ Files
└─ Services
```

Clicking a changed file in `Changes` opens `DiffReview` in the center content area that normally shows Terminal. Clicking Commit opens the Commit surface in the same center area. Selecting a Project/Tab/Pane/Agent from the canonical left Sidebar returns to Terminal.

This resolves the current ambiguity between Router pages, Terminal, and Right Panel tool content.

## 2. Important Godiff integration constraint

The current Godiff repository is **not an importable Go library** for Shardlane:

- its module is `github.com/egoist/godiff`;
- the application is `package main`;
- core packages are under `github.com/egoist/godiff/internal/diff`, `internal/git`, `internal/highlight`;
- Go's `internal` visibility rule prevents Shardlane from importing those packages from another module;
- no root `LICENSE`, `COPYING`, or `NOTICE` file was present in the audited snapshot.

Therefore 0.10 must **not** vendor/copy Godiff implementation code or pretend a normal `go get` integration exists.

Approved planning stance:

```text
Godiff = behavioral / UI / architecture reference
Shardlane = clean-room implementation behind Shardlane-owned service interfaces
```

If a future Godiff revision exposes a public importable package and explicit compatible license, Shardlane may add an adapter later without changing the 0.10 UI/domain contracts.

The implementation prompt must instruct the Agent not to copy Godiff internal source into this repository.

## 3. Godiff verification caveat

The audited Godiff tree was also test-run with Go 1.27.1.

Observed:

```text
PASS github.com/egoist/godiff
PASS github.com/egoist/godiff/internal/diff
PASS github.com/egoist/godiff/internal/highlight
FAIL github.com/egoist/godiff/internal/git TestWorkingTreeAndCommit
     node_modules generated-directory expectation differed in this audit environment
```

This does not invalidate Godiff as a UX reference, but reinforces that Shardlane should pin its own behavior and tests rather than copy current implementation details blindly.

## 4. What Godiff does especially well

### 4.1 Window composition

Godiff's main view is structurally simple:

```text
Row
├─ Sidebar
├─ resizer
└─ Main Column
   ├─ toolbar / header
   └─ main area
```

This is a better mental model for Shardlane than the current “global route center + independent right tool UI” when Git content needs the full center width.

### 4.2 Sidebar geometry

Audited constants:

```text
default width: 292 DIP
min: 220
max: 640
collapse threshold: 80
title/header height: 52
file tree row height: 28
depth indent: 14
```

The important part is not the exact numbers. It is the compact information architecture:

```text
chevron
icon
name
+additions/-deletions
status letter
```

The row remains readable without turning into a card.

### 4.3 File tree model

Godiff builds a compact changed-file tree from changed paths only.

Strong patterns:

- directory chains with one child can compact (`src/app/components`);
- directories-first ordering;
- expand/collapse state;
- file filter above the tree;
- file status letter at the trailing edge;
- line-change totals before the status;
- selected row follows Diff scrolling;
- selecting a file scrolls the Diff surface to that file.

This two-way selection sync is especially valuable for Shardlane.

### 4.4 Sidebar top chrome

Godiff aligns its Sidebar top controls with the macOS traffic-light/titlebar area and uses compact icon-based controls rather than full-width navigation rows.

Shardlane should adapt that visual language for its top Sidebar actions while keeping the canonical Project → Tab → Pane hierarchy.

### 4.5 Main content header

Godiff's main header carries:

```text
repository name
parent path
branch chip
optional compare-source chip
loading state
find/comments/layout actions
```

This is a better reference for Shardlane's **workspace content header** than putting every workspace detail into the global titlebar.

Shardlane adaptation:

```text
Project/repository name
selected Tab cwd / repository parent
branch chip
surface chip: Terminal / Changes / Commit
surface-specific actions
```

### 4.6 Main area is a surface, not a permanent Diff widget

Godiff's `mainArea` renders either:

```text
Diff Review
Commit view
empty/error/loading state
```

Commit is not a modal. It replaces the main content.

That pattern maps directly to Shardlane's desired center-area behavior.

### 4.7 Diff review surface

Strong patterns to reproduce behaviorally:

- split and unified layouts;
- one scrollable review surface across all files;
- file cards with sticky headers;
- syntax highlighting;
- word-level change highlighting;
- binary / too-large / rename / mode-change notes;
- generated file awareness;
- collapsed generated/viewed files;
- unchanged-region expansion;
- virtualized visible rows rather than rendering all lines;
- Sidebar follows top visible file.

0.10 does not need every Godiff feature. Review comments and image diff previews can remain later work.

### 4.8 Git source model

Godiff distinguishes:

```text
working tree
commit
branch comparison
```

The source chip in the header makes the active comparison explicit.

For Shardlane 0.10, the release-blocking source is working-tree vs `HEAD`. Commit/history/branch comparison may be added where the task plan marks them, but **branch switching** is a separate Shardlane feature: current Godiff compares with branches; it does not provide the branch-switch workflow requested for Shardlane.

### 4.9 Commit experience

Godiff's Commit surface provides:

- selected files;
- subject;
- summary/body;
- additions/deletions total;
- branch identity;
- explicit commit action;
- commit result.

It also uses pathspec-based Git operations to avoid committing unrelated staged paths.

Shardlane should preserve the user-facing idea, while independently designing stronger stale-state/index-preservation tests around the mutation.

## 5. Godiff patterns Shardlane should not copy blindly

Do not inherit these just because Godiff has them:

- its entire single-repository window model — Shardlane still has Herdr Workspace / Projects / Tabs / Panes;
- Cmd+1/Cmd+2 semantics if they conflict with Shardlane's existing action registry;
- its app-level Files/History tabs as a second Shardlane navigation system;
- any source code under Godiff `internal/*` without a legal/importable upstream contract;
- current generated-directory implementation details that failed one upstream test in this audit environment;
- review comments in 0.10 core;
- general-purpose commit/history browser that duplicates Shardlane History semantics.

## 6. Current Shardlane implementation reality audit

The 0.9 closure audit is useful as a ledger, but several `DONE` rows describe **package-level implementation** rather than **end-to-end product completion**.

0.10 must begin by reconciling these.

| 0.9 area | Current code reality | 0.10 action |
|---|---|---|
| Command Center | real, snapshot-based implementation exists | retain; add Git actions/surface destinations |
| Right Panel | real skeleton exists; Files/Services/Lazygit segmented | redesign as Changes/Files/Services navigator |
| Files | package exists, but `ListDirect` is called synchronously from render | **FIX** async snapshot/cancellation before UI redesign |
| Scripts store | atomic store exists, but production Shell initializes `scripts.NewStore("")` | **FIX** wire to `PathUserData/scripts.json` |
| Script execution | `runScriptCommand` currently only changes status text | **FIX** implement actual Herdr-backed transaction or downgrade/remove action |
| Services | `PortProbe` exists, but UI renders hard-coded `3000/5173/8080` sample rows | **FIX** consume real observed service/port snapshot |
| Lazygit | current surface is a placeholder button/status string, not a complete auxiliary terminal | **DELETE** from 0.10 product/code |
| Preview | loopback validation/window exists, but controller is recreated per click and does not actually retain project-scoped reuse state | **FIX** persistent PreviewService/controller ownership |
| Git intelligence | `gitintel.Service` exists but is only initialized; Native UI does not consume it | **REPLACE/EXTEND** with Git Workbench snapshot feeding Changes/Header |
| Dock attention | implementation exists | retain |
| File Drop | safe quoting helper exists, but no `Window.OnFileDrop` wiring and `HandleFileDrop` does not actually paste into terminal | **FIX** end-to-end or correct closure claim |
| Terminal Find | typed Herdr adapter exists, but no Native find bar/interaction wiring | **FIX** complete UI or mark adapter-only |
| Diagnostics | domain/UI exists | retain; verify no regressions |
| Updater | no current app integration; closure reason saying native plugin unavailable is incorrect | **FIX** MyGo 0.2.7 contains `plugins/updater/native`; schedule real integration |
| Sidebar density/high contrast | implemented | retain and restyle through shared tokens |
| Task Presets | package/UI exists | verify production persistence path during preflight |
| Activity | deferred | not a 0.10 blocker unless product priority changes |

## 7. Additional correctness findings

### 7.1 Files violates the documented no-render-IO rule

Current `renderDirectoryLevel` calls:

```text
filesview.ListDirect(...)
```

when cache is empty. That is filesystem IO inside Native render.

0.10 must move Files loading behind a service/background snapshot before visual work continues.

### 7.2 Services copy and behavior overstate reality

Current UI says:

> Observed TCP sockets available for browser preview.

But renders a fixed slice:

```text
3000
5173
8080
```

No actual observation is consumed there.

This must be replaced with real `PortProbe`/process facts or the section removed until data exists.

### 7.3 Script execution is a stub

Current `runScriptCommand` performs no Herdr mutation.

It must not be described as runtime-complete until it creates/targets an authoritative Herdr Pane/process through the canonical transaction.

### 7.4 Lazygit never reached the intended 0.9 lifecycle

The current code does not instantiate an auxiliary MyGo Terminal in `lazygitToolView`.

0.10's decision to remove Lazygit therefore removes an unfinished branch rather than discarding a deeply integrated runtime.

### 7.5 Git intelligence is not yet product-visible

`gitIntel = gitintel.NewService()` is initialized, but no current Native UI source consumes `SnapshotFor` or `Cached`.

0.10 should not layer a second Git service beside it blindly. Either evolve/replace it under one Git Workbench package or delete the obsolete abstraction after migration.

### 7.6 File Drop is helper-only today

`SafeShellQuote` is useful and tested.

But:

- no `OnFileDrop` registration was found;
- `HandleFileDrop` resolves a target but does not call terminal Paste/input.

0.10 must finish the actual path or change the closure status.

### 7.7 Terminal Find is adapter-only today

`pane.copy_search` typed protocol code exists.

No Native find UI calls it.

This is not the same as user-visible Terminal Find parity.

### 7.8 Preview ownership is not project-scoped in practice

`openLocalPreviewURL` constructs a new controller for every click.

The controller does not keep a keyed window registry.

The product claim “project-scoped preview reuse” therefore needs real ownership/lifecycle work.

### 7.9 0.9 package/version evidence is inconsistent

The 0.9 closure audit records a packaged artifact named:

```text
Shardlane 0.8.0.dmg
```

for the 0.9 closure.

0.10 preflight must align app/build versioning so closure artifacts prove the release they claim.

## 8. Current tests do not disprove these integration gaps

Focused Shardlane tests currently pass for:

```text
internal/gitintel
internal/filesview
internal/scripts
internal/services
internal/nativeui
```

This demonstrates package/test health, not end-to-end product completion. Most of the gaps above are missing wiring or render-path ownership errors that isolated unit tests do not currently fail on.

0.10 tests must therefore add Shell-level and packaged behavior evidence, not only package-local tests.

## 9. Recommended 0.10 product model

### Left Sidebar — canonical runtime navigator

Keep ownership:

```text
Agents
Recent
Projects
  Tab
    Pane
```

Restyle rows using Godiff's compact visual grammar:

```text
chevron / structure-line joint
icon
primary label
optional compact metadata
status glyph / diff stats
```

Retain Shardlane structure lines; Godiff does not have the Herdr hierarchy and cannot replace that semantic affordance.

### Right Panel — contextual workspace navigator

Replace:

```text
Files / Services / Lazygit
```

with:

```text
Changes / Files / Services
```

`Changes` uses a Godiff-inspired changed-file tree.

### Center — one primary surface owner

```text
Terminal
DiffReview
Commit
```

No two primary surfaces render at once.

### Workspace Header — Godiff-inspired content header

```text
Project/repository name
path
branch
current surface/source
surface actions
```

Global application navigation should remain distinct from workspace content actions.

## 10. Recommended explicit surface transitions

| User intent | Result |
|---|---|
| select Project / Tab / Pane in left Sidebar | `Terminal` |
| select Agent whose destination is a Pane | `Terminal` |
| click changed file in Right Panel | `DiffReview(file)` |
| scroll DiffReview to another file | Right Panel selection follows; surface stays Diff |
| click Commit in Changes footer/header | `Commit` |
| cancel Commit | return to `DiffReview` |
| successful Commit | refresh Git snapshot; return to DiffReview/empty Changes state |
| switch branch | stay in DiffReview, refresh source; selected file falls back if gone |
| close Right Panel while viewing Diff | Diff remains visible |
| reopen Right Panel | panel restores last tool without changing center surface |
| navigate to non-workspace Router page | existing Router owns center; workspace surface state retained but hidden |
| return to `/workspace` | restore workspace surface only if its repository/context is still valid; otherwise Terminal |

This table should become tests, not remain design prose only.

## 11. Terminal lifecycle while Diff/Commit is visible

Recommended 0.10 behavior:

- keep the current workspace's Herdr terminal attachments alive while an alternate workspace surface is shown;
- do **not** render `terminal.View` while the primary surface is Diff/Commit;
- terminal processes remain Herdr-owned and continue regardless;
- no keyboard/mouse input may route into hidden terminal views;
- returning to Terminal should be instant and preserve the local emulator state when possible;
- top-level navigation away from `/workspace` may keep the existing close/reattach policy unless measured UX justifies changing it separately.

This avoids teardown/flicker every time a developer reviews one diff while still ensuring only one visible/focused center surface exists.

## 12. Why the Right Panel should not own Diff rendering

A code diff needs width.

Putting it inside the 240–500 DIP Right Panel would create:

- poor split-diff readability;
- duplicate scroll/focus owners;
- an incentive to grow the panel until it competes with Terminal;
- awkward commit UI;
- difficult keyboard routing.

The Right Panel should select/contextualize; the primary center should render rich work content.

## 13. Godiff feature adoption matrix

| Godiff capability | 0.10 decision |
|---|---|
| compact file tree | ADOPT behavior/style |
| tree filter | ADOPT |
| changed-file status letters | ADOPT |
| per-file +/− totals | ADOPT |
| Sidebar↔Diff scroll sync | ADOPT |
| repo/path/branch header | ADAPT to Project/Tab context |
| split/unified diff | ADOPT |
| syntax highlight | ADOPT via independently importable dependency such as Chroma |
| word-level diff | ADOPT behavior, clean-room implementation |
| sticky file headers | ADOPT |
| generated files collapsed | ADOPT |
| viewed files | DEFER unless time remains after core |
| unchanged-line expansion | ADOPT |
| comments/review Markdown | DEFER |
| image previews | DEFER |
| History commit browser | OPTIONAL / later 0.10 workstream |
| branch comparison | OPTIONAL |
| branch switching | ADD — Shardlane feature, not copied from Godiff |
| create branch | ADD |
| selective commit | ADOPT UX, independently implement/test mutation semantics |
| source-code reuse | REJECT under current import/license state |

## 14. Final audit recommendation

The Agent implementing 0.10 must treat the 0.9 closure audit as historical evidence, not current truth.

Before new Git UI work, it must create a live `RealityMatrix` and resolve every inherited item in section 6 as one of:

```text
FIXED
REMOVED
EXPLICITLY_DEFERRED
```

No inherited `DONE` claim should survive solely because a package exists or a unit test passes.
