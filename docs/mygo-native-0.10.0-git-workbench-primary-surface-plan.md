# Shardlane MyGo 0.10.0 — Git Workbench & Unified Primary Surface Plan

Status: **planned post-0.9 corrective + product iteration**
Target: `0.10.0`
Depends on: current 0.9 implementation tree, not merely the 0.9 plan
Architecture authority: `docs/client-product-architecture.md`
Execution rules: `docs/mygo-native-execution-rules.md`
Reference / reality audit: `docs/reference-godiff-shardlane-0.10-audit.md`

## 1. Version goal

0.10 replaces Lazygit with a native Shardlane Git workflow and introduces one explicit owner for the center workspace content.

Goal:

> Make Git review, commit and branch switching feel like a native part of the same Project/Tab workspace as Terminal, with no competing center-content owners and no hidden second Git application.

The release has two equally important responsibilities:

1. **correct inherited 0.9 integration gaps** discovered by the post-implementation audit;
2. deliver the new **Git Workbench + Unified Primary Surface** architecture.

0.10 is not complete if the Git UI looks polished while inherited Files/Services/Script/FileDrop wiring remains false-positive “DONE”.

## 2. Product shape

Target shell:

```text
Shardlane Window
│
├─ Left Sidebar
│  ├─ compact top actions
│  ├─ Agents / Recent / Projects
│  │   └─ Project
│  │      └─ Tab
│  │         └─ Pane
│  └─ Workspace switcher
│
├─ Workspace Main
│  ├─ Workspace Header
│  │   ├─ Project / repo name
│  │   ├─ path
│  │   ├─ branch
│  │   ├─ active surface
│  │   └─ contextual actions
│  │
│  └─ Primary Content Surface
│      ├─ Terminal
│      ├─ Diff Review
│      └─ Commit
│
└─ Right Panel
   ├─ Changes
   ├─ Files
   └─ Services
```

Lazygit is removed from the target architecture.

## 3. Hard decisions

### 3.1 No Lazygit

Delete:

```text
SurfaceLazygit
lazygitToolView
Lazygit-specific panel actions/copy/docs/tests
approved auxiliary Lazygit terminal exception where it only exists for that product feature
```

Do not replace it with another external TUI Git client.

### 3.2 Godiff is a reference, not a current import dependency

Current `github.com/egoist/godiff` cannot be imported as a normal library because its relevant packages are `internal/*`, and the audited repository has no explicit root license file.

Therefore:

```text
0.10 UI/data contracts = Godiff-inspired
0.10 implementation = Shardlane-owned clean-room packages
```

Future upstream public package + compatible license may be integrated behind the same service interfaces.

### 3.3 One center owner

Only `WorkspacePrimarySurface` decides what appears in the `/workspace` center.

Right Panel never renders a second full code/diff/terminal surface.

### 3.4 Git is local Project context, not runtime authority

Git facts/mutations operate on the selected Tab's resolved repository root.

Herdr still owns:

```text
Project/runtime workspace
Tab
Pane
Terminal
Agent
process lifecycle
```

Git does not create a second Project/Tab model.

## 4. Inherited 0.9 correctness gate

Before visual Git work, create a `RealityMatrix` and resolve these:

| Area | Required 0.10 disposition |
|---|---|
| Files render-time `ListDirect` | move off render, background snapshot + generation guard |
| Script store `NewStore("")` | attach persistent PathUserData file |
| Script `Run` stub | implement real Herdr-backed execution or remove action until real |
| Services hard-coded ports | consume actual probe/runtime observations |
| Lazygit placeholder | delete |
| Preview controller per click | make controller/service window lifecycle persistent and keyed |
| GitIntel unused by UI | supersede/evolve into Git Workbench source |
| File Drop helper-only | wire `OnFileDrop` + actual terminal paste, or downgrade feature |
| Terminal Find adapter-only | wire Native find UX, or downgrade feature |
| Updater missing | integrate official MyGo native updater or clearly defer for real build reason |
| 0.9 artifact labeled 0.8.0 | fix app/build version source before 0.10 closure |

The new closure audit must not call a capability `DONE` solely because a package exists.

## 5. WorkspacePrimarySurface architecture

### 5.1 Model

```go
type WorkspaceSurfaceKind string

const (
    SurfaceTerminal WorkspaceSurfaceKind = "terminal"
    SurfaceDiff     WorkspaceSurfaceKind = "diff"
    SurfaceCommit   WorkspaceSurfaceKind = "commit"
)

type WorkspaceSurfaceState struct {
    Kind        WorkspaceSurfaceKind
    Context     WorkspaceContextKey
    Diff        DiffSurfaceState
    Commit      CommitSurfaceState
    Previous    WorkspaceSurfaceKind
}

type WorkspaceContextKey struct {
    InstanceID string
    ProjectID  string
    TabID      string
    RepoRoot   string
}
```

Do not store Herdr runtime objects in this state.

### 5.2 Global Router relationship

The existing Router keeps top-level app pages:

```text
/workspace
/new-task
/history
/search
/chat
/settings/...
...
```

`WorkspacePrimarySurface` is **not** another Router.

It is local state only while `/workspace` owns the center.

Reason:

- clicking files should not flood global back/forward history;
- Git review is contextual to the selected runtime workspace;
- Terminal/Diff/Commit should switch instantly without rebuilding top-level route ownership.

## 6. Surface transition rules

These rules are normative and testable.

| Intent | Center surface |
|---|---|
| app/startup enters `/workspace` | Terminal |
| click Project in left Sidebar | Terminal |
| click Tab in left Sidebar | Terminal |
| click Pane in left Sidebar | Terminal |
| click Agent that resolves to Pane | Terminal |
| click changed file in Right Panel Changes | Diff(file) |
| click `Changes` header/surface action without file | Diff(current/first) |
| click Commit | Commit |
| cancel Commit | previous Diff state |
| successful Commit with remaining changes | Diff refreshed |
| successful Commit with no changes | Diff empty state with explicit Terminal action |
| switch branch | remain Diff, refresh and reconcile selected file |
| Right Panel open/close | center surface unchanged |
| switch Right Panel Changes/Files/Services | center surface unchanged |
| leave `/workspace` | Router page replaces workspace center |
| return to `/workspace` | restore prior workspace surface only if context still valid; else Terminal |
| repository root changes because Tab context changes | Terminal |

No implicit Diff→Terminal switch merely because the Right Panel closes.

## 7. Terminal lifecycle under alternate surfaces

While `/workspace` remains active:

- keep current Herdr terminal attachments alive when `Diff` or `Commit` is visible;
- render no `terminal.View` elements while Terminal surface is hidden;
- route no keyboard/mouse input to hidden terminals;
- retain selected Pane and terminal emulator state;
- when returning to Terminal, render existing attachments and resync geometry;
- if attachment becomes invalid through runtime reconciliation, normal terminal sync replaces it.

This avoids reattach flicker every time the user inspects a diff.

If measurements show hidden attachments are too expensive, optimize after profiling; do not introduce two simultaneous rendered surfaces.

## 8. Workspace Header v2

Godiff-inspired, Shardlane-owned.

### 8.1 Layout

```text
Project/repo name                branch chip     Terminal | Changes     actions
/path/to/parent
```

Optional comparison/source chip when future commit/branch compare mode is active.

### 8.2 Terminal surface actions

```text
Split Right
Split Down
Zoom/Unzoom
New Tab
Refresh Runtime
```

### 8.3 Diff surface actions

```text
Refresh Changes
Find
Split / Unified
Open selected file in editor
Commit
Terminal
```

### 8.4 Commit surface actions

```text
Back to Changes
branch chip
Commit state/progress
Terminal only after explicit switch/Cancel policy
```

### 8.5 Global vs workspace chrome

Do not overload the current global titlebar with all workspace semantics.

0.10 may restructure shell chrome so:

```text
sidebar top chrome owns app/sidebar actions
workspace header owns Project/repo/surface actions
```

Both remain aligned to the MyGo titlebar/traffic-light geometry.

## 9. Left Sidebar visual refresh

Use Godiff's compact tree visual grammar as inspiration while preserving Shardlane hierarchy and structure lines.

### 9.1 Row anatomy

```text
structure joint / chevron
icon
label
secondary compact metadata
status / counts
```

Recommended target heights by density:

```text
compact      26–28 DIP
default      28–30 DIP
comfortable  32–34 DIP
```

Do not hard-code one height outside Design System density tokens.

### 9.2 Keep structure lines

Godiff file tree does not need semantic ancestry lines.
Shardlane does.

Retain Project → Tab → Pane vertical/branch lines and integrate them cleanly into the new compact row geometry.

### 9.3 Top action icons

Adapt Godiff's titlebar-aligned icon control style.

Suggested left-sidebar top actions:

```text
New Task
Command Center / Search
History
Sidebar collapse/toggle where appropriate
```

Use icon-only buttons with clear tooltips/accessible labels.

Do not create a second navigation tab strip.

## 10. Right Panel v2

### 10.1 Surfaces

Replace:

```text
Files / Services / Lazygit
```

with:

```text
Changes / Files / Services
```

### 10.2 Default behavior

When opening tools:

- restore last surface for the current Project/Tab context;
- if no prior choice and a Git repository with changes exists, default to Changes;
- otherwise Files.

### 10.3 Changes surface role

Changes is navigation + summary only.

It owns:

```text
filter
changed-file tree
status
+/- stats
selection
commit footer/action
optional source summary
```

It does **not** render code diff lines.

## 11. Godiff-style changed-file tree

### 11.1 Model

```go
type ChangeFile struct {
    Path        string
    OldPath     string
    Status      ChangeStatus
    Additions   int
    Deletions   int
    Binary      bool
    Generated   bool
    TooLarge    bool
    Fingerprint string
}
```

Tree projection is separate from diff parsing.

### 11.2 Visual row

```text
▾  src/components
   ◇ button.tsx         +12 -4    M
   ◇ dialog.tsx          +5 -1    M
```

Status color is semantic, but selected-row contrast remains correct in light/dark/high-contrast themes.

### 11.3 Tree behavior

- directories first;
- stable Unicode/case-insensitive sorting;
- compact one-child directory chains where clarity improves;
- filter field at top;
- keyboard Up/Down/Home/End;
- Left/Right collapse/expand;
- clicking file opens/reveals corresponding center diff;
- center diff scroll updates Right Panel selected file;
- auto-open ancestors when center selection changes.

### 11.4 Footer

Godiff-inspired footer:

```text
Total: +123 -45                         Commit
```

Commit is disabled when mutation preconditions are not satisfied.

## 12. Git Workbench service boundaries

Recommended package split:

```text
next/internal/gitworkbench/
├─ repository.go       // repo root, branch, status signature
├─ changes.go          // working-tree snapshot
├─ patch.go            // Shardlane-owned patch parser
├─ worddiff.go         // word ranges
├─ highlight.go        // Chroma adapter
├─ tree.go             // changed-file tree
├─ cache.go            // stale-while-refresh / generations
├─ commit.go           // commit transaction
├─ branches.go         // list/switch/create branch
└─ history.go          // optional commit/compare source
```

Native UI:

```text
next/internal/nativeui/
├─ workspace_surface.go
├─ workspace_header.go
├─ changes_panel.go
├─ diff_surface.go
├─ diff_rows.go
├─ commit_surface.go
└─ branch_menu.go
```

The old `gitintel` package should be either migrated into `gitworkbench` or deleted after all consumers move. Do not keep two Git caches.

## 13. Git command runner

One command runner owns Git subprocess policy.

Environment:

```text
GIT_OPTIONAL_LOCKS=0 for read commands
LC_ALL=C where machine parsing requires stable output
GIT_PAGER=cat
GIT_TERMINAL_PROMPT=0
```

Rules:

- no shell interpolation;
- explicit argv;
- context cancellation;
- bounded reads;
- read commands never hold index locks where Git supports avoiding them;
- mutation commands run only after explicit user intent and use separate timeouts/policies from quick reads;
- structured errors keep argv operation class but never secrets.

Do not copy Godiff's helper implementation; implement and test Shardlane's own runner.

## 14. Working-tree diff source

0.10 release-blocking diff source:

```text
working tree + index vs HEAD
+ untracked files
```

Use a stable Git patch command behaviorally equivalent to:

```text
git diff --no-color --no-ext-diff --no-textconv -M -U3 --submodule=short HEAD --
```

Exact argv may evolve after tests.

Repository without HEAD compares against empty tree.

Untracked files are gathered separately and modeled as additions.

## 15. Bounds

Recommended initial limits inspired by measured Godiff behavior:

```text
max rendered patch per file: 4 MiB
max individually listed untracked files: 1000
scanner/token maximum: bounded >= expected patch lines
syntax highlight only visible/needed rows where practical
generated/dependency dirs: collapsed summary where appropriate
```

Also define a whole-snapshot memory budget in implementation preflight; a repository with hundreds of megabytes of generated changes must degrade, not freeze the app.

## 16. Patch model

Clean-room Shardlane model:

```go
type Hunk struct {
    OldStart int
    OldLines int
    NewStart int
    NewLines int
    Section  string
    Lines    []DiffLine
}

type DiffLine struct {
    Kind      LineKind
    OldLine   int
    NewLine   int
    Text      string
    NoNewline bool
}
```

Support:

- modified;
- added;
- deleted;
- renamed;
- copied;
- type/mode change;
- binary;
- conflict marker state where Git reports it;
- untracked;
- too-large note.

## 17. Syntax highlighting

Use an independently importable, properly licensed dependency such as:

```text
github.com/alecthomas/chroma/v2
```

if approved by the dependency/license audit.

Highlighting belongs behind a small `Highlighter` interface so large/generated/plain files can bypass it.

No Godiff internal package import.

## 18. Word-level changes

Implement a bounded clean-room word-diff helper.

Requirements:

- only pair nearby delete/add lines within one change run;
- cap input line length;
- cap algorithmic product to prevent pathological O(n*m) memory/time;
- skip word highlights when lines are mostly rewritten;
- Unicode-safe byte/rune mapping tests.

Do not treat word-diff ranges as Git authority; they are presentation only.

## 19. Diff surface

### 19.1 One scrolling review surface

Default behavior follows Godiff:

```text
all changed files
→ one virtualized list
→ file cards
→ sticky file header
```

Clicking one file in the Right Panel scrolls the center list to that card.

### 19.2 Header per file

```text
chevron
path
rename source if relevant
Copy Path
Open in Editor
+/- totals
Generated / Binary tags
```

`Viewed` is optional/stretch after core.

### 19.3 Unified / split

Both are release-blocking.

Persist user's preferred layout in Shardlane settings.

### 19.4 Unchanged regions

Show bounded context by default.

Allow:

```text
show 100 more above/below
show all bounded region
```

Large expansion must not eagerly rebuild the entire repository list.

### 19.5 Find

Diff Find is separate from Terminal Find.

`Cmd/Ctrl+F` while Diff surface is focused searches rendered diff text/model.

No Herdr RPC is involved.

## 20. Sidebar ↔ Diff bidirectional synchronization

Required behavior:

### Right Panel → center

```text
click changed file
→ set selected path
→ switch center to Diff if needed
→ scroll file card to top/start
```

### center → Right Panel

As visible top file changes:

```text
update selected changed-file key
→ open ancestors
→ scroll tree row into view
```

Use a guard/timestamp/generation so programmatic scroll does not create oscillation.

## 21. Git change refresh

Sources of refresh:

- entering Diff/Changes;
- explicit Refresh;
- commit success/failure reconciliation;
- branch switch/create;
- bounded active-only worktree status-signature check or filesystem watcher;
- app regains focus when Git surface is active.

Never Git-poll every render.

When changes occur externally:

```text
keep current snapshot visible
show “Changes detected” banner/state
background refresh or explicit refresh according to policy
```

Do not blank the diff while a quick refresh runs.

## 22. Commit surface

### 22.1 UI

Godiff-inspired center surface:

```text
Commit                         branch-chip
N files · +A -D

Subject
Summary / body

[changed-file checklist]

                                 Commit
```

Commit is not a modal.

### 22.2 Selection

Initial default: all changed files selected except generated/too-large policy only if product explicitly chooses; safest default is all user-visible changed files selected.

User may uncheck files.

No hunk-level staging in 0.10 core.

### 22.3 Stale-state fence

When Commit surface opens, capture:

```text
repo root
HEAD hash
branch
status signature
selected file fingerprints
```

Before mutation:

- revalidate root/branch/HEAD;
- refresh status signature;
- verify selected paths still represent the same change or ask user to refresh;
- do not commit from a stale rendered snapshot silently.

### 22.4 Index preservation

A critical requirement:

> Committing selected files must not silently consume unrelated staged work.

Design the Git transaction and tests around:

- unrelated staged files remain staged and outside the new commit;
- selected files are committed as the UI describes;
- untracked selected files are supported explicitly;
- definite commit failure has a documented/index-safe recovery path;
- hooks/signing failures surface exact recoverable errors;
- no `git reset --hard`, no force mutation.

The implementation may adapt the **behavioral idea** of Godiff's pathspec commit, but must pin Shardlane's own index-preservation fixtures before shipping.

### 22.5 Message

Fields:

```text
subject required
body optional
```

No automatic AI-generated commit message in 0.10 core.

## 23. Branch menu

Branch chip in Workspace Header is actionable.

### 23.1 List

List local branches only in core 0.10.

Show:

```text
current branch
other local branches
optional ahead/behind if already cached cheaply
```

No automatic network fetch.

### 23.2 Switch

Explicit action:

```text
branch menu → choose branch → preflight → git switch
```

Rules:

- no force;
- no shell;
- validate target as a local branch;
- dirty worktree may show a confirmation explaining Git may refuse if changes conflict;
- Git's refusal is respected and shown;
- after success refresh branch/change snapshot;
- Diff surface remains active and reconciles selected file;
- Terminal processes are not restarted merely because branch changed.

### 23.3 Create branch

Core or near-core:

```text
New Branch…
→ validate name with Git
→ git switch -c <name>
→ refresh
```

Do not implement delete/rename/remote tracking management in the first slice unless time remains after correctness.

## 24. Branch switch safety and running processes

Switching a branch can change files underneath running terminals/dev servers.

Shardlane does not own those processes and must not restart them automatically.

If active Scripts/Services are known for the same repo, the confirmation may say:

```text
This changes files in the working tree. Running processes will keep running.
```

No hidden restart policy.

## 25. Optional Git history/compare

Godiff's Files/History sidebar is useful but not release-blocking for the user request.

If implemented after core:

```text
History mode
→ recent commits
→ click commit
→ center DiffReview(commit vs first parent)
```

Branch comparison source may also be added:

```text
working tree since merge-base with branch
```

Do not confuse compare-source selection with branch switching.

## 26. Right Panel Files inherited fix

Before restyling, make Files async.

Required architecture:

```text
FilesService
selected Tab cwd
→ background ListDirect
→ immutable DirectorySnapshot
→ Native render reads snapshot only
```

Each expand request carries:

```text
context generation
root
path
```

Project/Tab/root switch invalidates stale results.

## 27. Script inherited fix

### Store

Wire production store to:

```text
PathUserData/scripts.json
```

with proper startup error handling.

### Run

Either:

1. implement real Herdr-backed Script launch transaction in 0.10; or
2. remove/disable Run with explicit “not wired” status until implemented.

The preferred target is real execution.

Do not leave a clickable button that only changes status copy.

## 28. Services inherited fix

Delete hard-coded port rows.

Build `ServicesSnapshot` from:

- known Script runtime process/PID facts;
- observed port probe results;
- exact Project/Pane ownership when available.

Native render consumes snapshot only.

If no real ports are observed, show an honest empty state.

## 29. Preview inherited fix

Make Preview ownership persistent at Shell/app service level.

Suggested:

```go
type PreviewManager struct {
    windows map[PreviewKey]*mygo.Window
}
```

Key at least by:

```text
Workspace/Project or repo root + target
```

Reuse/retarget according to product policy.

Do not create an untracked controller on every click.

## 30. File Drop inherited fix

Finish end-to-end behavior:

```text
Window.OnFileDrop
→ determine exact terminal hit target
→ SafeShellQuote
→ exact terminal.Paste
```

No newline/Enter.

If Diff/Commit surface is active, file drops must not fall through to a hidden Terminal.

## 31. Terminal Find inherited fix

The adapter exists.

0.10 should add actual Terminal surface UI only if the contract has been live-verified:

```text
Cmd/Ctrl+F while Terminal focused
→ Native find bar
→ Herdr CopySearch
→ navigate matches
```

When Diff is active, the same shortcut belongs to Diff Find instead.

This is a key example of why the primary-surface focus owner matters.

## 32. Updater inherited fix

MyGo 0.2.7 includes `plugins/updater/native` in the audited framework tree.

Re-audit build/signing prerequisites and integrate it if release infrastructure is available.

Do not keep “plugin unavailable” as the defer reason.

If release signing material/pipeline is the blocker, record that exact blocker.

## 33. Shortcut routing

Shortcuts are surface-aware.

Examples:

### Global/window

```text
Command Center
New Task
History
Settings
```

### Terminal surface

```text
split/zoom/terminal find
```

### Diff surface

```text
Cmd/Ctrl+F        Find in diff
layout toggle     Diff only
refresh changes
open editor
```

### Commit surface

```text
Cmd/Ctrl+Enter    Commit
Escape/back       return to Changes where safe
```

A hidden surface never receives its shortcuts.

All shortcuts continue through the shared Action Registry where practical.

## 34. Focus ownership

At any time exactly one of these owns keyboard focus intent:

```text
modal/palette/dialog
> active workspace primary surface
> right/left navigation panel
```

Terminal input has no fallback priority when hidden.

Switching to Diff/Commit explicitly removes Terminal focus.

Returning to Terminal restores focus to selected Pane after the terminal view exists.

## 35. Performance targets

### Surface switching

```text
Terminal → Diff warm cached: target < 16 ms UI switch
Diff → Terminal with retained attachments: target < 16 ms UI switch
```

Actual diff loading may continue behind a loading state.

### Git snapshot

```text
small repo warm refresh: target < 250 ms
Git command hard deadline for read operation group: target <= 5 s
```

### Diff rendering

- virtualized rows;
- only visible/near-visible syntax highlight work where practical;
- no whole-repository rich-text element tree;
- 4 MiB per-file rendered patch cap initially;
- no UI-thread Git command/patch parse for large data.

### Changed-file tree

```text
1000 files filter/rank/rebuild target < 16 ms UI-side after snapshot ready
```

## 36. Concurrency / generations

Every async Git/Files/Services operation carries a context generation.

At minimum:

```text
workspace generation
selected context/repo root
Git source generation
```

A stale result can populate a cache but cannot replace the current visible state when its context no longer matches.

Commit and branch mutations are single-flight per repository.

## 37. Logging

Useful Git Workbench logs:

```text
operation class
duration
repo identity as normalized path/hash if privacy policy allows
file count
patch bytes
source kind
result/error class
```

Never log:

- diff line contents;
- file contents;
- commit message/body;
- branch credentials/remote URL secrets;
- terminal output.

## 38. Security / correctness

- all Git argv passed without shell;
- no remote fetch automatically;
- no force checkout/switch;
- no reset-hard;
- no destructive clean;
- branch name validation before mutation;
- selected paths use literal pathspec-safe transport;
- paths with newlines/control bytes require NUL/pathspec-safe handling or explicit refusal;
- diff parser handles malicious/odd filenames without command injection;
- external editor open uses argv, not shell string interpolation;
- all mutation results trigger authoritative Git refresh.

## 39. Atomic implementation roadmap

Tasks should generally fit 30 minutes–4 hours.

### R0 — 0.9 reality reconciliation

| ID | Task | Verify |
|---|---|---|
| GWB-001 | create live RealityMatrix from audit | DONE/PARTIAL/REMOVE table |
| GWB-002 | wire `scripts.json` production path | restart persistence test |
| GWB-003 | move Files listing off render | headless no-IO render test |
| GWB-004 | consume real Services/Port snapshot | fake probe Shell test |
| GWB-005 | remove hard-coded sample ports | source audit |
| GWB-006 | choose/implement real Script Run transaction | fake Herdr mutation test |
| GWB-007 | persistent PreviewManager ownership | reuse/lifecycle test |
| GWB-008 | wire `OnFileDrop` | window/drop test |
| GWB-009 | terminal Paste from exact drop target | fake terminal test |
| GWB-010 | wire Native Terminal Find or mark exact blocker | Shell interaction test/GAP |
| GWB-011 | re-audit updater plugin/build signing | exact blocker evidence |
| GWB-012 | align app/package version source | package metadata test |

### S0 — remove Lazygit

| ID | Task | Verify |
|---|---|---|
| GWB-020 | remove `SurfaceLazygit` | compile/source test |
| GWB-021 | remove Lazygit UI/actions/docs references in active target | `rg` audit |
| GWB-022 | remove obsolete auxiliary-Lazygit lifecycle code if any | source audit |
| GWB-023 | update architecture/comments from Files/Services/Lazygit → Changes/Files/Services | doc/source audit |

### S1 — Primary Surface model

| ID | Task | Verify |
|---|---|---|
| GWB-030 | `WorkspaceSurfaceKind` + state model | pure tests |
| GWB-031 | context validity/reconciliation | A/B Project/Tab tests |
| GWB-032 | Terminal default transition | startup test |
| GWB-033 | Pane/Agent selection → Terminal | selection tests |
| GWB-034 | changed file → Diff | transition test |
| GWB-035 | Commit open/cancel transitions | transition test |
| GWB-036 | Right Panel open/close does not change primary surface | regression test |
| GWB-037 | non-workspace Router route hides workspace surface | router test |
| GWB-038 | return-to-workspace valid/invalid restoration | router/context tests |
| GWB-039 | hidden Terminal receives no focus/input | headless focus test |

### S2 — Terminal retained lifecycle

| ID | Task | Verify |
|---|---|---|
| GWB-040 | Terminal canvas conditional on SurfaceTerminal | render test |
| GWB-041 | retain terminal attachments in Diff/Commit | lifecycle test |
| GWB-042 | resync geometry/focus on return | multi-pane headless test |
| GWB-043 | runtime-invalid attachment reconciles while hidden | projection test |

### H0 — Shell/Header v2

| ID | Task | Verify |
|---|---|---|
| GWB-050 | split app/sidebar chrome from workspace content header | layout test |
| GWB-051 | Project/repo + parent path header | fixture render |
| GWB-052 | branch chip | render/action test |
| GWB-053 | surface indicator/switch action | transition test |
| GWB-054 | Terminal contextual actions | existing mutation regressions |
| GWB-055 | Diff contextual actions | headless action test |
| GWB-056 | Commit contextual actions | headless action test |
| GWB-057 | traffic-light/titlebar safe geometry | packaged macOS check |

### L0 — Left Sidebar Godiff-inspired refresh

| ID | Task | Verify |
|---|---|---|
| GWB-060 | compact top action-icon row | headless render |
| GWB-061 | shared compact hierarchical row primitive | component tests |
| GWB-062 | retain Project/Tab/Pane structure lines | hierarchy geometry test |
| GWB-063 | density token integration | 3-density tests |
| GWB-064 | selected/focused/hover states | interaction tests |
| GWB-065 | trailing status/diff metadata slot | render tests |
| GWB-066 | long Unicode labels truncate without hiding hierarchy | layout fixture |

### P0 — Right Panel v2

| ID | Task | Verify |
|---|---|---|
| GWB-070 | surfaces become Changes/Files/Services | UI test |
| GWB-071 | remove Lazygit switch case | compile/source audit |
| GWB-072 | context-scoped last tool restoration | A/B context test |
| GWB-073 | default Changes when Git changes exist | model test |
| GWB-074 | panel close leaves Diff visible | surface regression |

### G0 — Git runner/repository model

| ID | Task | Verify |
|---|---|---|
| GWB-080 | one cancellable Git runner | fake process tests |
| GWB-081 | stable environment/read policy | env fixture |
| GWB-082 | repository root resolver | temp repo test |
| GWB-083 | branch/HEAD/status signature | repo tests |
| GWB-084 | no shell invocation | source/static audit |
| GWB-085 | mutation single-flight per root | concurrency test |

### G1 — changes snapshot

| ID | Task | Verify |
|---|---|---|
| GWB-090 | ChangeStatus/File models | pure tests |
| GWB-091 | working tree vs HEAD patch acquisition | temp repo fixture |
| GWB-092 | repository without HEAD | empty-tree test |
| GWB-093 | untracked file acquisition | fixture |
| GWB-094 | rename/copy/type/binary metadata | fixture patches |
| GWB-095 | generated-file classifier | table tests |
| GWB-096 | per-file/whole-snapshot bounds | large fixture |
| GWB-097 | immutable snapshot fingerprint | change/reload test |
| GWB-098 | stale-while-refresh cache | deterministic clock test |
| GWB-099 | stale generation cannot replace new context | race test |

### G2 — patch parser / word diff

| ID | Task | Verify |
|---|---|---|
| GWB-110 | clean-room patch parser | multi-file fixtures |
| GWB-111 | hunk line numbers/no-newline | fixture |
| GWB-112 | rename/copy/mode/binary notes | fixtures |
| GWB-113 | pathological scanner bounds | fuzz/boundary test |
| GWB-114 | word-pairing helper | table test |
| GWB-115 | bounded Unicode word diff | fuzz/table test |
| GWB-116 | low-similarity rewrite suppresses noisy word marks | fixture |

### G3 — syntax highlighting

| ID | Task | Verify |
|---|---|---|
| GWB-120 | dependency/license audit for Chroma | dependency note |
| GWB-121 | file-name lexer adapter | table test |
| GWB-122 | highlighted segment model | pure test |
| GWB-123 | plain/binary/generated fallback | fixture |
| GWB-124 | cache/bounds | large file test |

### C0 — Changes tree model

| ID | Task | Verify |
|---|---|---|
| GWB-130 | changed-path tree builder | fixture |
| GWB-131 | dirs-first stable order | Unicode table |
| GWB-132 | one-child path compaction | fixture |
| GWB-133 | filter/fuzzy behavior | table tests |
| GWB-134 | expanded/visible rows projection | pure tests |
| GWB-135 | selected-file ancestor auto-open | fixture |

### C1 — Changes panel UI

| ID | Task | Verify |
|---|---|---|
| GWB-140 | filter field | headless interaction |
| GWB-141 | compact changed-file rows | render test |
| GWB-142 | status letters/colors | theme tests |
| GWB-143 | +/- compact stats | render test |
| GWB-144 | keyboard tree navigation | input test |
| GWB-145 | file click → Diff surface/reveal | integration test |
| GWB-146 | total footer | snapshot test |
| GWB-147 | Commit footer action | surface test |

### D0 — Diff row model / virtualization

| ID | Task | Verify |
|---|---|---|
| GWB-150 | flatten file/hunk/gap rows | pure tests |
| GWB-151 | stable row keys | refresh tests |
| GWB-152 | visible-list virtualization | 50k-row headless test |
| GWB-153 | sticky file header behavior | UI test |
| GWB-154 | file card boundary/radius states | render smoke |
| GWB-155 | collapsed generated/too-large rows | fixture |

### D1 — Unified diff UI

| ID | Task | Verify |
|---|---|---|
| GWB-160 | gutters/line numbers | fixture render |
| GWB-161 | add/delete/context colors | light/dark/high-contrast |
| GWB-162 | syntax spans | fixture |
| GWB-163 | word-level highlight | fixture |
| GWB-164 | hunk section/gap row | render test |
| GWB-165 | expand 100 lines | interaction test |

### D2 — Split diff UI

| ID | Task | Verify |
|---|---|---|
| GWB-170 | delete/add line pairing | pure tests |
| GWB-171 | dual gutters | render test |
| GWB-172 | one-sided add/delete files | fixture |
| GWB-173 | horizontal overflow/wrap policy | layout test |
| GWB-174 | split/unified setting persistence | settings roundtrip |

### D3 — Diff navigation sync

| ID | Task | Verify |
|---|---|---|
| GWB-180 | Changes click scrolls correct file | integration test |
| GWB-181 | top-visible file selects Changes row | scroll test |
| GWB-182 | programmatic-scroll oscillation guard | deterministic test |
| GWB-183 | selected file removed on refresh falls back | fixture |
| GWB-184 | project/tab context invalidates diff cleanly | A/B context test |

### D4 — Diff find/editor actions

| ID | Task | Verify |
|---|---|---|
| GWB-190 | surface-aware Cmd/Ctrl+F | shortcut test |
| GWB-191 | diff match index | pure tests |
| GWB-192 | next/previous match scrolling | headless test |
| GWB-193 | Copy Path | clipboard test |
| GWB-194 | Open in Editor argv-safe adapter | fake process test |

### M0 — Commit model/preflight

| ID | Task | Verify |
|---|---|---|
| GWB-200 | CommitDraft model | pure tests |
| GWB-201 | default file selection | fixture |
| GWB-202 | capture HEAD/branch/status/file fingerprints | repo test |
| GWB-203 | stale mutation fence | externally changed fixture |
| GWB-204 | selected-path validation | unusual filename tests |

### M1 — Commit transaction

| ID | Task | Verify |
|---|---|---|
| GWB-210 | pathspec-safe selected commit transaction | real repo test |
| GWB-211 | unrelated staged file preserved | critical fixture |
| GWB-212 | selected tracked file committed | fixture |
| GWB-213 | selected untracked file committed | fixture |
| GWB-214 | unselected working change preserved | fixture |
| GWB-215 | definite commit failure recovery/index state | hook failure fixture |
| GWB-216 | concurrent external HEAD/index change fails closed | race/process fixture |
| GWB-217 | commit hash/result + refresh | integration test |

### M2 — Commit surface UI

| ID | Task | Verify |
|---|---|---|
| GWB-220 | subject/body fields | input test |
| GWB-221 | changed-file checklist | headless test |
| GWB-222 | +/- totals | render test |
| GWB-223 | commit enabled state | validation test |
| GWB-224 | Cmd/Ctrl+Enter | shortcut test |
| GWB-225 | busy/error/success states | fake transaction tests |
| GWB-226 | cancel returns Diff | surface transition test |
| GWB-227 | success refresh/no-change empty state | integration test |

### B0 — branch list/switch

| ID | Task | Verify |
|---|---|---|
| GWB-230 | local branch list parser | repo fixture |
| GWB-231 | branch chip menu | headless UI |
| GWB-232 | switch target validation | invalid ref tests |
| GWB-233 | clean branch switch | real repo test |
| GWB-234 | dirty-but-compatible switch | repo fixture |
| GWB-235 | conflicting switch refused, no force | repo fixture |
| GWB-236 | switch stays Diff + refresh | surface integration |
| GWB-237 | running Herdr terminals remain attached | lifecycle regression |

### B1 — create branch

| ID | Task | Verify |
|---|---|---|
| GWB-240 | New Branch dialog | headless UI |
| GWB-241 | Git branch-name validation | table/repo tests |
| GWB-242 | create + switch | real repo test |
| GWB-243 | existing/invalid branch error | fixture |

### O0 — optional history/source compare

| ID | Task | Verify |
|---|---|---|
| GWB-250 | recent commit metadata | repo fixture |
| GWB-251 | commit vs first-parent diff | fixture |
| GWB-252 | optional Changes/History mode | headless UI |
| GWB-253 | branch compare vs merge-base | fixture |

These tasks are stretch after release-blocking commit/switch work.

### X0 — shortcut/focus integration

| ID | Task | Verify |
|---|---|---|
| GWB-260 | surface-aware Action Registry availability | model test |
| GWB-261 | hidden Terminal shortcut suppression | input test |
| GWB-262 | Diff Find owns Cmd/Ctrl+F in Diff | test |
| GWB-263 | Terminal Find owns Cmd/Ctrl+F in Terminal | test |
| GWB-264 | Commit Cmd/Ctrl+Enter | test |
| GWB-265 | palette/modal priority over surfaces | interaction regression |

### X1 — inherited feature closure

| ID | Task | Verify |
|---|---|---|
| GWB-270 | Files real async end-to-end | Shell test |
| GWB-271 | Scripts persistence + Run end-to-end | restart + fake Herdr |
| GWB-272 | Services real ports end-to-end | Shell fake probe |
| GWB-273 | Preview reuse lifecycle | window/controller test |
| GWB-274 | File Drop end-to-end | window→terminal test |
| GWB-275 | Terminal Find UI end-to-end | fake Herdr search test |
| GWB-276 | updater exact DONE/GAP status | build/plugin test |

### Z0 — final acceptance

| ID | Task | Verify |
|---|---|---|
| GWB-280 | full Go tests | `go test ./...` |
| GWB-281 | race-sensitive suites | `go test -race ...` |
| GWB-282 | gofmt + diff hygiene | `gofmt -l`, `git diff --check` |
| GWB-283 | MyGo packaged build | app + DMG |
| GWB-284 | real macOS shell layout acceptance | screenshots/manual checklist |
| GWB-285 | Terminal↔Diff↔Commit focus/latency acceptance | real app |
| GWB-286 | large-repo diff performance | measured fixture |
| GWB-287 | commit/index safety matrix | real Git fixture suite |
| GWB-288 | branch switch safety matrix | real Git fixture suite |
| GWB-289 | privacy/security audit | checklist/static tests |
| GWB-290 | create 0.10 closure audit with corrected 0.9 inherited statuses | evidence matrix |

## 40. Focused acceptance scenarios

### Primary Surface

1. startup shows Terminal.
2. clicking a changed file switches center to Diff, not Right Panel content.
3. Right Panel can close while Diff remains visible.
4. clicking a Pane returns center to Terminal immediately.
5. Terminal attachments survive Diff/Commit surface switches without duplicate attachments.
6. hidden Terminal receives no typed keys.
7. switching Project/Tab invalidates stale Diff and returns to Terminal.
8. leaving `/workspace` and returning restores only a still-valid surface/context.

### Left Sidebar / Header

9. Project/Tab/Pane hierarchy still has structure lines.
10. density settings change spacing without changing hierarchy.
11. top actions are compact icon controls with tooltips/accessibility labels.
12. Workspace Header shows Project/repo/path/branch without duplicating runtime ownership.
13. Terminal and Diff surface actions never appear enabled for the wrong surface.

### Changes Panel / Diff

14. changed-file tree shows status and +/- totals.
15. clicking a file reveals the correct card.
16. scrolling center updates selected file in Changes.
17. filter does not run Git.
18. unified/split both handle add/delete/rename/untracked/binary.
19. generated and >4 MiB files degrade safely.
20. unchanged context can expand 100 lines without whole-list rebuild.
21. Diff find does not invoke Terminal search.
22. large repo keeps UI responsive.

### Commit

23. Commit opens in center, not modal.
24. user can select subset of files.
25. unrelated staged file is not included in the new commit and remains staged.
26. unselected working changes remain.
27. selected untracked files can commit safely.
28. external file/HEAD change after review triggers stale-state refusal/reconcile.
29. failed hook/commit produces explicit error without silent index corruption.
30. successful commit refreshes Changes and leaves no stale selected file.

### Branch

31. branch chip lists local branches.
32. clean switch succeeds without Terminal restart.
33. dirty compatible switch succeeds after explicit user action.
34. conflicting switch fails without force and leaves repo usable.
35. successful switch refreshes diff and branch header.
36. New Branch validates names and switches exactly once.

### Inherited 0.9 fixes

37. Files render performs no filesystem IO.
38. Services shows only actually observed ports; no hard-coded sample ports.
39. Script Run performs real Herdr work or is disabled honestly.
40. `scripts.json` persists across app restart.
41. Preview reuses/tracks owned windows rather than making unmanaged controllers.
42. OS file drop reaches exact Terminal and performs Paste with no Enter.
43. Terminal Find has actual UI, not adapter-only status.
44. updater closure reason reflects real MyGo plugin/build state.
45. packaged artifact says 0.10.0, not an older release number.

## 41. Definition of Done

0.10 is complete when:

- [ ] Lazygit is removed from active product architecture/code/UI;
- [ ] `/workspace` has one tested `WorkspacePrimarySurface` owner;
- [ ] Terminal/Diff/Commit are mutually exclusive visible center surfaces;
- [ ] Terminal attachments survive alternate surface viewing without input leakage;
- [ ] Right Panel is Changes/Files/Services and does not own full diff rendering;
- [ ] changed-file tree follows the compact Godiff-inspired visual grammar;
- [ ] left Project/Tab/Pane Sidebar is refreshed in the same visual language while preserving structure lines;
- [ ] Workspace Header uses the Project/path/branch/surface structure inspired by Godiff;
- [ ] native Diff Review supports working-tree changes, untracked files, unified/split, syntax highlighting, word highlights and bounded context expansion;
- [ ] Sidebar↔Diff selection synchronization is bidirectional and stable;
- [ ] Commit is a center surface with selected-file commit and stale-state fences;
- [ ] unrelated staged work is proven preserved by tests;
- [ ] local branch switching is safe, non-force and integrated into the header;
- [ ] New Branch works or is explicitly deferred with exact reason;
- [ ] no Godiff `internal/*` code is copied/imported under the current upstream contract;
- [ ] no second Git cache/service remains after migration;
- [ ] Files no longer does render-time IO;
- [ ] Services no longer renders fake sample ports;
- [ ] Script store/runtime behavior matches UI claims;
- [ ] Preview/FileDrop/TerminalFind inherited gaps are corrected or closure status is downgraded honestly;
- [ ] updater status reflects the actual MyGo 0.2.7 plugin/build situation;
- [ ] full tests/race/build/real-app/performance/security gates pass;
- [ ] `docs/mygo-native-0.10.0-closure-audit.md` records real end-to-end evidence, not package-presence claims.

## 42. Post-0.10 boundary

After 0.10, the local macOS client should have a coherent development loop without Lazygit:

```text
Herdr runtime navigation
+ Terminal
+ Native Git review/commit/branch switch
+ Files / Services / Preview
+ Agent/History/Usage/Diagnostics
```

The next major program remains Remote/Mobile/platform convergence and final Rust cutover, not expanding Git into a full GitHub Desktop replacement.
