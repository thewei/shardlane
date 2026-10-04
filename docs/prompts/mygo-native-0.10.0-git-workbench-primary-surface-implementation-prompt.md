# Shardlane MyGo 0.10.0 Git Workbench & Unified Primary Surface — Implementation Prompt

Continue Shardlane after the current 0.9 implementation pass.

Repository:

`/Users/wilson/Workspaces/wh-studio/herdr-client`

Branch:

`rewrite/mygo`

Use the existing Devspace workspace.
Preserve all user/parallel-agent changes. Do not reset, stage, commit or push unless explicitly requested.

## Read first

1. `AGENTS.md`
2. `CLAUDE.md`
3. `.agents/skills/herdr-client-development/SKILL.md`
4. `docs/client-product-architecture.md`
5. `docs/performance-engineering.md`
6. `docs/terminal-interaction-spec.md`
7. `docs/mygo-native-execution-rules.md`
8. `docs/mygo-native-migration-roadmap.md`
9. `docs/reference-godiff-shardlane-0.10-audit.md`
10. `docs/mygo-native-0.10.0-git-workbench-primary-surface-plan.md`
11. `docs/mygo-native-0.9.0-closure-audit.md` — historical evidence only; see its post-audit correction note
12. `next/CLAUDE.md`

Then inspect the actual current code before changing anything:

```text
next/internal/nativeui/shell.go
next/internal/nativeui/workspace.go
next/internal/nativeui/router.go
next/internal/nativeui/selection.go
next/internal/nativeui/terminal.go
next/internal/nativeui/sidebar.go
next/internal/nativeui/project_tree.go
next/internal/nativeui/titlebar.go
next/internal/nativeui/right_panel.go
next/internal/nativeui/file_drop.go
next/internal/gitintel
next/internal/filesview
next/internal/scripts
next/internal/services
next/internal/preview
next/internal/herdr/search_gate.go
next/internal/settings
next/main.go
next/mygo.json
```

## Reference snapshot

The 0.10 planning audit inspected:

```text
egoist/godiff @ 88b89e0
MyGo v0.2.7
```

Godiff is a **behavioral/UI/architecture reference**, not a source-code dependency.

Current Godiff facts that matter:

- app module is `github.com/egoist/godiff`;
- app is `package main`;
- core packages are under `internal/diff`, `internal/git`, `internal/highlight`;
- Shardlane cannot import those `internal/*` packages from another module;
- the audited root did not contain an explicit LICENSE/COPYING/NOTICE file.

Therefore:

> **Do not copy or vendor Godiff internal source into Shardlane.**

Implement Shardlane-owned clean-room models/services using Godiff only as a UX/behavior reference. If upstream later exposes a public licensed package, it may be adapted behind the same interfaces.

## Mission

Implement **Shardlane 0.10.0 — Git Workbench & Unified Primary Surface**.

The target experience is:

```text
canonical Project / Tab / Pane Sidebar
+ Godiff-inspired compact visual language
+ Right Panel: Changes / Files / Services
+ one center WorkspacePrimarySurface
    Terminal
    Diff Review
    Commit
+ native Git branch switching / branch creation
+ safe selected-file commit
+ inherited 0.9 correctness fixes
```

Lazygit is removed completely from the target product.

## Non-negotiable architecture invariants

Do not violate these:

- Herdr remains authoritative for Workspace/Project/Tab/Pane/Agent/Terminal/process lifecycle.
- Git context follows selected Tab cwd/repository root; do not create another Project registry.
- one canonical left Sidebar owns Project → Tab → Pane navigation.
- one `WorkspacePrimarySurface` owns the `/workspace` center.
- Terminal, Diff and Commit are mutually exclusive visible center surfaces.
- the Right Panel is navigation/inspection; it never renders a second full Diff or Terminal.
- no render-time filesystem/Git/network/Herdr subprocess work.
- no shell interpolation for Git commands.
- no automatic remote fetch.
- no force checkout/switch.
- no `git reset --hard` or destructive clean.
- no hidden second Git cache after migration.
- no copy/import of Godiff `internal/*` source.
- no general browser expansion.
- no provider credential/model-gateway ownership.
- no user file/diff/commit contents in logs.

## Phase 0 — reconcile reality before adding features

The current 0.9 closure audit overstates several package-level implementations.
Treat these as inherited defects, not completed prerequisites.

### 0.9 gaps already confirmed in current code

1. **Files render-time IO**
   - `nativeui/right_panel.go` calls `filesview.ListDirect(...)` during render on cache miss.
   - Move directory IO to background/service snapshots with generation/cancellation.

2. **Scripts production persistence missing**
   - Shell currently initializes `scripts.NewStore("")`.
   - Wire production persistence under `PathUserData/scripts.json`.

3. **Script Run is a stub**
   - current `runScriptCommand` only changes `s.status`.
   - Implement a real Herdr-backed launch transaction or disable the action honestly until real.

4. **Services uses fake sample ports**
   - UI currently renders `3000`, `5173`, `8080` regardless of observations.
   - Delete this and render actual observed service/port snapshot only.

5. **Lazygit is only a placeholder**
   - current view is a label/button/status change, not a real auxiliary terminal lifecycle.
   - Delete the feature instead of finishing it.

6. **Preview ownership is not project-scoped in practice**
   - `openLocalPreviewURL` creates a new controller per click.
   - Introduce persistent PreviewManager/controller ownership and window reuse/lifecycle.

7. **gitintel is not product-visible**
   - Shell initializes the service, but current Native UI does not consume it.
   - Evolve/migrate it into one Git Workbench service; do not keep two caches.

8. **File Drop is helper-only**
   - safe quoting exists;
   - no `Window.OnFileDrop` wiring found;
   - `HandleFileDrop` currently does not call terminal Paste/input.
   - Finish end-to-end or mark unsupported honestly.

9. **Terminal Find is adapter-only**
   - typed `pane.copy_search` adapter exists;
   - no Native find UX consumes it.
   - Wire the actual Terminal find bar if live protocol verification still holds.

10. **Updater closure reason is wrong**
    - MyGo v0.2.7 module contains `plugins/updater/native`.
    - Re-audit signing/build prerequisites; do not defer because “plugin unavailable.”

11. **release versioning is wrong**
    - current `next/mygo.json` still says `0.8.0`.
    - align version metadata before final 0.10 packaging.

Create a live RealityMatrix and resolve every inherited item to:

```text
FIXED
REMOVED
EXPLICITLY_DEFERRED_WITH_REAL_BLOCKER
```

Do not proceed as if package-local tests prove end-to-end completion.

## Phase 1 — remove Lazygit

Delete the active target capability:

```text
SurfaceLazygit
lazygitToolView
Lazygit panel labels/actions
Lazygit Command Center actions
Lazygit-specific tests
MyGo-target Lazygit auxiliary lifecycle/comment/docs assumptions
```

Right Panel target becomes:

```text
Changes
Files
Services
```

Do not replace Lazygit with another external TUI client.

## Phase 2 — introduce WorkspacePrimarySurface

Create a small UI-independent surface model, e.g.:

```go
type WorkspaceSurfaceKind string

const (
    WorkspaceSurfaceTerminal WorkspaceSurfaceKind = "terminal"
    WorkspaceSurfaceDiff WorkspaceSurfaceKind = "diff"
    WorkspaceSurfaceCommit WorkspaceSurfaceKind = "commit"
)

type WorkspaceContextKey struct {
    InstanceID string
    ProjectID string
    TabID string
    RepoRoot string
}
```

The global MyGo Router remains responsible for top-level pages.

Do **not** make Terminal/Diff/Commit separate global routes.
They are contextual surface state inside `/workspace`.

### Required transition table

```text
startup / workspace selection       → Terminal
Project click                       → Terminal
Tab click                           → Terminal
Pane click                          → Terminal
Agent → Pane navigation             → Terminal
changed file click                  → Diff(file)
Changes open                        → Diff(current/first)
Commit action                       → Commit
Commit cancel                       → previous Diff
Commit success with changes         → Diff refreshed
Commit success no changes           → Diff empty state
branch switch                       → remain Diff + refresh
Right Panel open/close              → center unchanged
Right Panel tool switch             → center unchanged
leave /workspace                    → Router page owns center
return /workspace                   → restore only valid prior context; else Terminal
Tab/repo context changes            → Terminal
```

Write tests for the transition table before styling.

## Phase 3 — terminal lifecycle while Diff/Commit is visible

While `/workspace` stays active:

- keep current Herdr terminal attachments alive;
- do not render `terminal.View` when Terminal surface is hidden;
- hidden terminals receive no keyboard/mouse/file-drop input;
- retain selected Pane/emulator state;
- return to Terminal without unnecessary process reattach;
- resync layout/focus on return;
- runtime-invalid attachments still reconcile normally.

Do not keep two visible/focused primary surfaces at once.

## Phase 4 — restructure workspace header

Adapt Godiff's main header pattern.

Target:

```text
Project / repo name                      branch chip    active surface    actions
parent/path or selected Tab cwd
```

Keep global app navigation distinct from workspace content actions.

### Terminal header actions

```text
Split Right
Split Down
Zoom/Unzoom
New Tab
Refresh Runtime
```

### Diff header actions

```text
Refresh Changes
Find
Split / Unified
Open selected file in editor
Commit
Terminal
```

### Commit header actions

```text
Back to Changes
branch identity
commit progress/result
```

The visual structure may use Godiff's compact chips/icon buttons, but stays inside Shardlane Design System tokens.

## Phase 5 — refresh canonical left Sidebar

Keep semantic hierarchy:

```text
Agents
Recent
Projects
  Project
    Tab
      Pane
```

Adopt Godiff-inspired compact row grammar:

```text
structure-line joint / chevron
icon
label
optional compact secondary metadata
status/count trailing slot
```

Must preserve the existing Project → Tab → Pane structure lines.

Use density tokens, not hard-coded one-size rows.

Recommended row ranges:

```text
compact:     26–28 DIP
default:     28–30 DIP
comfortable: 32–34 DIP
```

Top actions should become compact icon controls with accessible labels/tooltips.
Do not create another tab/navigation strip.

## Phase 6 — Git Workbench package

Create one service boundary. Recommended split:

```text
next/internal/gitworkbench/
  repository.go
  runner.go
  changes.go
  patch.go
  worddiff.go
  highlight.go
  tree.go
  cache.go
  commit.go
  branches.go
  history.go         // optional/stretch
```

Native UI should be split rather than growing `right_panel.go`:

```text
next/internal/nativeui/
  workspace_surface.go
  workspace_header.go
  changes_panel.go
  diff_surface.go
  diff_rows.go
  commit_surface.go
  branch_menu.go
```

Do not make `right_panel.go` or `shell.go` a giant catch-all.

After migration, remove/supersede `internal/gitintel`; do not retain a second Git cache.

## Phase 7 — Git runner

One command runner owns subprocess policy.

Read environment should include where appropriate:

```text
GIT_OPTIONAL_LOCKS=0
LC_ALL=C
GIT_PAGER=cat
GIT_TERMINAL_PROMPT=0
```

Requirements:

- `exec.CommandContext` or equivalent argv-based execution;
- never `sh -c` / interpolated shell command;
- cancellation;
- bounded output/time;
- read vs mutation policy separated;
- structured error classification;
- no automatic network fetch.

Log operation class + duration, not Git output/file/commit contents.

## Phase 8 — working-tree changes model

Release-blocking source:

```text
working tree + index vs HEAD
+ untracked files
```

Repository without HEAD compares to Git's empty tree.

Model at least:

```text
path
old path
status
additions/deletions
binary
generated
too large
fingerprint
hunks
```

Initial safety limits:

```text
per-file rendered patch <= 4 MiB
untracked entries <= 1000 before summary/degrade
whole-snapshot memory budget must be explicitly defined
```

Handle odd filenames safely.

## Phase 9 — clean-room patch parser and word diff

Implement Shardlane-owned parser.

Support:

```text
modified
added
deleted
renamed
copied
mode/type changes
binary
conflict status
untracked
too-large note
no-newline marker
```

Word diff:

- bounded line length;
- bounded algorithmic product;
- pair nearby delete/add lines only;
- Unicode-safe;
- suppress noisy word marks on mostly rewritten lines.

Do not copy Godiff implementation source.

## Phase 10 — syntax highlighting

Use an independently importable dependency only after license/dependency review.
Chroma v2 is an acceptable candidate.

Place it behind a small `Highlighter` interface.

Avoid highlighting huge/generated/binary files unnecessarily.

## Phase 11 — Right Panel Changes

Target surfaces:

```text
Changes | Files | Services
```

`Changes` owns navigation/summary, not code rendering.

### File-tree row

Godiff-inspired:

```text
▾ src/components
  ◇ button.tsx                 +12 -4    M
  ◇ dialog.tsx                  +5 -1    M
```

Features:

- filter input;
- dirs-first stable sort;
- optional one-child path compaction;
- expand/collapse;
- Up/Down/Home/End;
- Left/Right tree navigation;
- status letters/colors;
- +/- stats;
- selection;
- total footer;
- Commit action.

## Phase 12 — Native Diff surface

Use one virtualized scrolling surface of file cards.

Release-blocking:

- unified layout;
- split layout;
- syntax highlighting;
- word-level change highlight;
- sticky file headers;
- binary/rename/mode/too-large notes;
- generated-file collapsed state;
- bounded unchanged context expansion;
- Copy Path;
- Open in Editor;
- Diff Find.

Do not put the diff in the 240–500 DIP Right Panel.

## Phase 13 — bidirectional Changes ↔ Diff synchronization

### Right → center

```text
click changed file
→ select path
→ switch to Diff
→ reveal its card
```

### center → Right

```text
visible top file changes
→ update selected path
→ expand ancestors
→ scroll selected tree row into view
```

Add oscillation/programmatic-scroll guards.

## Phase 14 — Commit surface

Commit is a center surface, not a modal.

Fields:

```text
subject required
body optional
selected files checklist
+/- totals
branch identity
```

No AI-generated message in 0.10 core.

### Mandatory stale-state fence

At Commit open capture:

```text
repo root
HEAD
branch
status signature
selected file fingerprints
```

Before mutation revalidate all relevant facts.
Never silently commit against a stale review snapshot.

### Critical index-preservation invariant

> Unrelated already-staged work must remain staged and must not enter the Shardlane-selected commit.

Build real Git fixtures proving:

- unrelated staged file remains staged;
- selected tracked files commit;
- selected untracked files commit;
- unselected working changes remain;
- commit hook/signing failures have explicit recovery state;
- external HEAD/index change fails closed;
- no hard reset/force cleanup is used.

Use literal/pathspec-safe input rather than shell concatenation.

## Phase 15 — Branch menu and switching

Header branch chip is interactive.

Core:

```text
list local branches
switch local branch
create new branch
```

No auto-fetch.
No force.
No branch deletion/rename/remote-management in core 0.10.

### Switch semantics

```text
select branch
→ validate local ref
→ explicit preflight/confirmation when dirty
→ git switch target
→ respect Git refusal
→ refresh branch/change snapshot
→ keep Diff surface and reconcile file
```

Do not restart Herdr Terminals/Agents/dev servers merely because the branch changed.

If known Services are active, warn that processes continue running against changed files.

## Phase 16 — optional history/compare

Only after core is closed:

```text
recent commits
commit vs first-parent Diff
branch compare from merge-base
```

Do not let optional history block Terminal/Diff/Commit/Branch/Commit core.
Do not create a second Shardlane History product.

## Phase 17 — Files inherited correction

Create a Files service/state seam:

```text
expand request
→ background direct-directory read
→ immutable snapshot
→ dispatch apply if generation/root still current
→ render snapshot only
```

Tests must prove render itself performs no filesystem read.

## Phase 18 — Scripts/Services inherited correction

### Scripts

- production store under `PathUserData/scripts.json`;
- actual Herdr-backed launch transaction or disabled action;
- no client-owned process runtime.

### Services

Delete hard-coded ports.
Render only real observed facts.

If ownership/PID association cannot be proven, show a neutral observed-port row rather than inventing Script ownership.

## Phase 19 — Preview inherited correction

Add persistent PreviewManager ownership.

Key by stable workspace/repo + target.

Reuse/focus existing owned window according to tested policy.
Clean map entry on close.

Keep loopback-only security boundary.

## Phase 20 — File Drop inherited correction

Wire real Window file-drop events.

Target exact visible Terminal under pointer when Terminal surface is active.

Then:

```text
paths
→ SafeShellQuote
→ terminal.Paste
```

Never append Enter/newline.

When Diff/Commit is visible, a drop must not fall through to hidden Terminal input.

## Phase 21 — Terminal Find inherited correction

If Herdr `pane.copy_search` is still live-compatible:

```text
Terminal surface + Cmd/Ctrl+F
→ Native find bar
→ typed CopySearch adapter
→ previous/next
→ match count
```

Diff surface uses the same shortcut for Diff Find, not Terminal Find.

Surface focus decides ownership.

## Phase 22 — updater inherited correction

MyGo v0.2.7 contains:

```text
github.com/egoist/mygo/plugins/updater/native
```

Re-audit plugin configuration/signing/release prerequisites.

If you can integrate it safely, do so.
If not, closure status must name the **actual blocker** (for example signing/release infrastructure), not “plugin unavailable.”

## Phase 23 — shortcut and focus ownership

Priority:

```text
modal / palette / dialog
> active WorkspacePrimarySurface
> navigation panel
```

Hidden surfaces receive no shortcuts.

Examples:

```text
Terminal + Cmd/Ctrl+F → Terminal Find
Diff + Cmd/Ctrl+F     → Diff Find
Commit + Cmd/Ctrl+Enter → Commit
```

Reuse the shared Action Registry for labels/shortcuts/availability where practical.

## Phase 24 — performance

Targets:

```text
warm Terminal ↔ Diff surface switch: <16 ms UI state/render transition
1000 changed-file tree filter/rebuild after snapshot ready: <16 ms target
small repo warm Git refresh: <250 ms target
Git read hard deadline per command/group: <=5 s
```

Use virtualized diff rows.
Do not build a giant rich-text tree for the whole repository.

Large/generated changes degrade instead of freezing.

## Phase 25 — logging/privacy

May log:

```text
operation class
duration
file count
patch byte count
source kind
error class
```

Do not log:

```text
diff/file content
commit subject/body
terminal output
History conversation bodies
credentials/tokens
remote URLs containing secrets
```

Keep bounded existing app logging policy.

## Atomic task ledger

Use the exact task IDs from:

`docs/mygo-native-0.10.0-git-workbench-primary-surface-plan.md`

Ranges:

```text
GWB-001..012   0.9 reality reconciliation
GWB-020..023   remove Lazygit
GWB-030..039   Primary Surface
GWB-040..043   Terminal retained lifecycle
GWB-050..057   Workspace Header v2
GWB-060..066   Sidebar visual refresh
GWB-070..074   Right Panel v2
GWB-080..085   Git runner/repository
GWB-090..099   change snapshots
GWB-110..116   patch / word diff
GWB-120..124   syntax highlight
GWB-130..147   Changes tree/panel
GWB-150..194   Diff UI/navigation/find
GWB-200..227   Commit model/transaction/UI
GWB-230..243   Branch switch/create
GWB-250..253   optional history/compare
GWB-260..265   shortcuts/focus
GWB-270..276   inherited end-to-end closure
GWB-280..290   final acceptance/audit
```

One task = one objective + one focused verification.
Do not combine unrelated feature slices into giant commits/patches.

## Fast verification policy

Do not run the full suite after every small task.

Per task:

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./internal/<affected-package> -run '<focused test>'
cd ..
git diff --check
```

Per workstream, run only related packages.

Run full/race/build/real-app acceptance only after the implementation is materially closed.

## Required mutation test matrix

Before calling Commit/Branch work done, run real temporary Git repository fixtures that cover:

```text
tracked modified file
untracked file
deleted file
rename
binary file
unrelated staged change
selected + unselected files
commit hook failure
branch clean switch
branch dirty compatible switch
branch conflicting switch refusal
external HEAD/index mutation between render and action
paths with spaces/unicode/single quotes
```

No mock-only proof is sufficient for Git mutation correctness.

## Final gate

At the end:

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go test -race \
  ./internal/gitworkbench/... \
  ./internal/filesview/... \
  ./internal/scripts/... \
  ./internal/services/... \
  ./internal/preview/... \
  ./internal/nativeui/...

gofmt -l .
GOTOOLCHAIN=go1.27.1 go tool mygo build
cd ..
git diff --check
```

Also run the real macOS acceptance scenarios from the 0.10 plan.

## Required final closure audit

Create:

`docs/mygo-native-0.10.0-closure-audit.md`

Report `DONE / PARTIAL / EXPLICITLY_DEFERRED / REMOVED` with real end-to-end evidence for:

- 0.9 RealityMatrix inherited fixes;
- Lazygit removal;
- WorkspacePrimarySurface;
- Terminal attachment retention/input isolation;
- Workspace Header v2;
- Sidebar visual refresh + structure lines;
- Right Panel Changes/Files/Services;
- Git repository runner/snapshot cache;
- changed-file tree;
- unified Diff;
- split Diff;
- syntax highlighting;
- word-level highlight;
- unchanged-context expansion;
- bidirectional Changes↔Diff sync;
- Diff Find;
- editor open;
- Commit surface;
- stale-state Commit fence;
- unrelated-index preservation;
- branch list/switch;
- New Branch;
- optional history/compare separately;
- Files no-render-IO;
- Scripts persistence/runtime;
- Services real-port wiring;
- Preview manager lifecycle;
- File Drop actual paste;
- Terminal Find actual UI;
- updater exact status;
- macOS packaged artifact version = 0.10.0;
- performance targets;
- privacy/security audit.

Also prove:

- no Godiff internal source copied/imported;
- no active Lazygit target remains;
- no second Git cache remains;
- no render-time filesystem/Git/network/Herdr work exists;
- hidden Terminal gets no input;
- no force/reset-hard/destructive Git mutation exists;
- unrelated staged work cannot be silently consumed;
- no user code/diff/commit body is written to logs;
- full tests/race/build/real-app acceptance pass.

Proceed without asking for confirmation.
