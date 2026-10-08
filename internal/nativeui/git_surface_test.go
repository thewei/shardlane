package nativeui

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// gitSurfaceTestShell builds a shell bound to a repo context with a seeded
// snapshot (headless; no live Herdr needed for surface behavior).
func gitSurfaceTestShell(t *testing.T, root string) *Shell {
	t.Helper()
	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: root}},
		Tabs:     []herdr.Tab{{ID: "t1", Label: "main", ProjectID: "w1"}},
		Panes:    []herdr.Pane{{ID: "p1", TerminalID: "term-1", Label: "editor", TabID: "t1", CWD: root}},
	}
	shell.reconcileLocalSelection(shell.projection)
	shell.git.root = root
	shell.git.snapshot = &gitworkbench.ChangesSnapshot{
		Root:      root,
		Branch:    "main",
		Signature: "sig-1",
		Files: []gitworkbench.ChangeFile{
			{Path: "src/main.go", Status: gitworkbench.StatusModified, Additions: 12, Deletions: 4,
				Hunks: []gitworkbench.Hunk{{
					OldStart: 1, OldLines: 4, NewStart: 1, NewLines: 5,
					Lines: []gitworkbench.DiffLine{
						{Kind: gitworkbench.KindContext, OldLine: 1, NewLine: 1, Text: "package main"},
						{Kind: gitworkbench.KindDelete, OldLine: 2, Text: "fmt.Println(\"old\")"},
						{Kind: gitworkbench.KindAdd, NewLine: 2, Text: "fmt.Println(\"new\")"},
					},
				}}},
			{Path: "README.md", Status: gitworkbench.StatusAdded, Additions: 3},
		},
	}
	return shell
}

// TestSurfaceSwitchRetainsTerminalState pins GWB-040/041: switching to Diff
// keeps the terminal map intact (retained attachments) and the surface
// state machine lands on Terminal for sidebar navigation.
func TestSurfaceSwitchRetainsTerminalState(t *testing.T) {
	shell := gitSurfaceTestShell(t, t.TempDir())
	shell.terminals["term-1"] = &terminalSurface{key: "term-1", paneID: "p1"}

	shell.showSurface(WorkspaceSurfaceDiff)
	if shell.surface.current() != WorkspaceSurfaceDiff {
		t.Fatalf("surface = %v", shell.surface.current())
	}
	if len(shell.terminals) != 1 {
		t.Fatalf("terminals must be retained across surface switch, got %d", len(shell.terminals))
	}

	// Sidebar navigation lands back on Terminal.
	shell.selectPane("p1")
	if shell.surface.current() != WorkspaceSurfaceTerminal {
		t.Fatalf("pane selection surface = %v", shell.surface.current())
	}
	if len(shell.terminals) != 1 {
		t.Fatalf("terminals must survive round trip, got %d", len(shell.terminals))
	}
}

// TestHiddenTerminalSurfaceRendersNoTerminal pins GWB-039/040: the diff
// surface renders no terminal frames — the canvas exists only on Terminal.
func TestHiddenTerminalSurfaceRendersNoTerminal(t *testing.T) {
	shell := gitSurfaceTestShell(t, t.TempDir())
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()

	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	// The diff card for the seeded file renders.
	if !tester.HasText("src/main.go") {
		t.Fatalf("diff surface missing file card; texts=%q", tester.Texts())
	}
	// The right panel is closed by default; no terminal chrome label leaks.
	if tester.HasText("Split Right") {
		t.Fatalf("terminal actions visible on Diff surface; texts=%q", tester.Texts())
	}
}

// TestSplitTrackedUntracked pins the discard routing: untracked paths go to
// the delete lane, tracked ones to the restore lane.
func TestSplitTrackedUntracked(t *testing.T) {
	shell := gitSurfaceTestShell(t, t.TempDir())
	shell.git.snapshot.Files[1].Untracked = true

	tracked, untracked := shell.splitTrackedUntracked([]string{"src/main.go", "README.md"})
	if len(tracked) != 1 || tracked[0] != "src/main.go" {
		t.Fatalf("tracked = %v", tracked)
	}
	if len(untracked) != 1 || untracked[0] != "README.md" {
		t.Fatalf("untracked = %v", untracked)
	}
}

// TestPendingToastFlushesOnFrame pins the background-lane notice channel: a
// queued toast is consumed by the next rendered frame.
func TestPendingToastFlushesOnFrame(t *testing.T) {
	shell := gitSurfaceTestShell(t, t.TempDir())
	shell.pendingToast = "Changes stashed"
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	if shell.pendingToast != "" {
		t.Fatalf("toast must flush on the next frame, got %q", shell.pendingToast)
	}
}

// TestAmendWithoutHeadRefused pins the amend guard: amending on an unborn
// branch fails honestly instead of reaching git.
func TestAmendWithoutHeadRefused(t *testing.T) {
	shell := gitSurfaceTestShell(t, t.TempDir())
	shell.git.snapshot.NoHead = true

	shell.submitCommitAmend("subject", "", []string{"README.md"}, true)
	if shell.surface.commit.ErrText == "" {
		t.Fatal("amend without HEAD must be refused with an error message")
	}
}

// TestSidebarSplitPanesAndRepoBar pins the Fork-style diff-mode sidebar:
// Unstaged/Staged split panes with their section headers, and the repo bar
// (Branches/Tags/Stashes/Worktrees) leading the panel — while the app action
// row stays off the git surface (2026-10-06 annotation A5) and everything
// diff-specific is gone again back on the Terminal surface.
func TestSidebarSplitPanesAndRepoBar(t *testing.T) {
	root := t.TempDir()
	shell := gitSurfaceTestShell(t, root)
	shell.git.rootOf[root] = root
	// src/main.go is modified in the worktree only; give it a staged side
	// too so both panes have an entry.
	cf := &shell.git.snapshot.Files[0]
	cf.Staged, cf.Unstaged = true, true

	if shell.router.Path() != routeWorkspace {
		shell.router.Push(routeWorkspace)
	}
	tester := ui.NewTester(shell.View, 1200, 800)

	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	tester.Frame()
	if !tester.HasText("Local Changes") || !tester.HasText("All Commits") {
		t.Fatalf("diff sidebar missing the mode toggle; texts=%q", tester.Texts())
	}
	if !tester.HasText("Unstaged Files") || !tester.HasText("Staged Files") {
		t.Fatalf("diff sidebar missing the split panes; texts=%q", tester.Texts())
	}
	for _, chip := range []string{"Tags", "Stashes", "Worktrees"} {
		if !tester.HasText(chip) {
			t.Fatalf("repo bar missing %s; texts=%q", chip, tester.Texts())
		}
	}
	// The sidebar's top action row is gone entirely (2026-10-06 A7-A9):
	// New Task and History moved into the header's leading cluster, which
	// every route shares. The diff sidebar itself shows no action row —
	// asserted here by the repo bar being the first sidebar content.
	if !tester.HasText("Tags") || !tester.HasText("Stashes") {
		t.Fatalf("diff sidebar missing the repo bar; texts=%q", tester.Texts())
	}
	// The pane actions read Stage/Unstage (A1/A2): they act on the pane's
	// Cmd/Shift selection, or on every file when nothing is chosen.
	if !tester.HasText("Stage") || !tester.HasText("Unstage") {
		t.Fatalf("pane headers missing the Stage/Unstage actions; texts=%q", tester.Texts())
	}
	if tester.HasText("Stage All") || tester.HasText("Unstage All") {
		t.Fatalf("pane actions must not read Stage All/Unstage All; texts=%q", tester.Texts())
	}

	shell.showSurface(WorkspaceSurfaceTerminal)
	tester.Frame()
	if tester.HasText("Unstaged Files") || tester.HasText("Stashes") {
		t.Fatalf("terminal sidebar must drop the repo bar and panes; texts=%q", tester.Texts())
	}
	// The runtime sections return outside the git surface.
	if !tester.HasText("Agents") || !tester.HasText("Workspace") {
		t.Fatalf("terminal sidebar missing the runtime navigator; texts=%q", tester.Texts())
	}
}

// TestSidebarSwitchesWithPrimarySurface pins the Diff-mode sidebar: on the
// Diff surface the left sidebar becomes Godiff's Files navigator (filter +
// tree + Total/Commit footer); back on Terminal it is the canonical runtime
// navigator again.
func TestSidebarSwitchesWithPrimarySurface(t *testing.T) {
	root := t.TempDir()
	shell := gitSurfaceTestShell(t, root)
	// The seeded root is already resolved: syncGitContext (fired by
	// showSurface(Terminal)) must keep it instead of restarting an async
	// resolution whose result no window would deliver headlessly.
	shell.git.rootOf[root] = root
	if shell.router.Path() != routeWorkspace {
		shell.router.Push(routeWorkspace)
	}

	tester := ui.NewTester(shell.View, 1200, 800)

	// Terminal: the runtime navigator, no diff footer.
	shell.showSurface(WorkspaceSurfaceTerminal)
	tester.Frame()
	if tester.HasText("Total:") {
		t.Fatalf("terminal sidebar must not show the diff footer; texts=%q", tester.Texts())
	}

	// Diff: the Godiff Files sidebar.
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	tester.Frame()
	if !tester.HasText("Total:") {
		t.Fatalf("diff sidebar missing the Total footer; texts=%q", tester.Texts())
	}
	if !tester.HasText("Filter files") {
		t.Fatalf("diff sidebar missing the file filter; texts=%q", tester.Texts())
	}
	if !tester.HasText("README.md") {
		t.Fatalf("diff sidebar missing the changed file row; texts=%q", tester.Texts())
	}

	// Back to Terminal: the navigator returns.
	shell.showSurface(WorkspaceSurfaceTerminal)
	tester.Frame()
	if tester.HasText("Total:") {
		t.Fatalf("terminal sidebar must drop the diff footer; texts=%q", tester.Texts())
	}
	if !tester.HasText("Projects") && !tester.HasText("Agents") {
		t.Fatalf("terminal sidebar missing the runtime navigator; texts=%q", tester.Texts())
	}
}

// TestDiffSurfaceRendersGodiffCardChrome pins the Godiff presentation port:
// the card header carries the Viewed toggle and the +/- pill, and line rows
// render without the old hunk-header rows.
func TestDiffSurfaceRendersGodiffCardChrome(t *testing.T) {
	shell := gitSurfaceTestShell(t, t.TempDir())
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	shell.buildGdRows()

	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	if !tester.HasText("Viewed") {
		t.Fatalf("diff card header missing the Viewed toggle; texts=%q", tester.Texts())
	}
	if !tester.HasText("+12") || !tester.HasText("-4") {
		t.Fatalf("diff card header missing the +/- pill; texts=%q", tester.Texts())
	}
	if tester.HasText("@1,4 +1,5") {
		t.Fatal("old hunk-header rows must not render; Godiff folds hunks without headers")
	}
}

// TestDiffFileClickRevealsAndSyncs pins GWB-180/181/182: a Changes click
// switches to Diff, selects the path, and a programmatic reveal guards the
// center→right oscillation window.
func TestDiffFileClickRevealsAndSyncs(t *testing.T) {
	shell := gitSurfaceTestShell(t, t.TempDir())

	shell.openChanges("README.md")
	if shell.surface.current() != WorkspaceSurfaceDiff {
		t.Fatalf("surface = %v", shell.surface.current())
	}
	if shell.surface.diff.SelectedPath != "README.md" {
		t.Fatalf("selected = %q", shell.surface.diff.SelectedPath)
	}

	// Reveal sets the guard window.
	shell.revealDiffFile("README.md")
	if !shell.diffScrollGuardActive() {
		t.Fatal("reveal must open the oscillation guard window")
	}
	time.Sleep(5 * time.Millisecond)

	// Center→right sync respects the guard while it is active.
	shell.git.revealAt = timeNow()
	shell.syncChangesSelectionFromDiff("src/main.go")
	if shell.surface.diff.SelectedPath != "README.md" {
		t.Fatalf("guard window violated: selected = %q", shell.surface.diff.SelectedPath)
	}
}

// TestCommitSurfaceFlow pins GWB-218..227 headless behavior: the surface
// lists files with default selection, refuses to submit without a subject
// or fence, and lands back on Diff after success.
func TestCommitSurfaceFlow(t *testing.T) {
	shell := gitSurfaceTestShell(t, t.TempDir())
	shell.showSurface(WorkspaceSurfaceCommit)
	if shell.surface.current() != WorkspaceSurfaceCommit {
		t.Fatalf("surface = %v", shell.surface.current())
	}

	// Default selection: every changed file (plan §22.2).
	if len(shell.surface.commit.Selected) != 2 {
		t.Fatalf("default selection = %v", shell.surface.commit.Selected)
	}
	if paths := shell.selectedCommitPaths(); len(paths) != 2 {
		t.Fatalf("selectedCommitPaths = %v", paths)
	}

	// No preflight yet: submit must be refused.
	shell.submitCommit("subject", "", shell.selectedCommitPaths())
	if shell.surface.commit.ErrText == "" {
		t.Fatal("commit without captured preflight must fail honestly")
	}

	// Cancel returns to the previous surface.
	shell.cancelCommitSurface()
	if got := shell.surface.current(); got != WorkspaceSurfaceDiff {
		t.Fatalf("cancel landed on %v", got)
	}
}

// TestFileDropGuardOnHiddenTerminal pins GWB-020 (file drop surface rule):
// a drop while Diff is visible never falls through to the hidden terminal.
func TestFileDropGuardOnHiddenTerminal(t *testing.T) {
	shell := gitSurfaceTestShell(t, t.TempDir())
	shell.showSurface(WorkspaceSurfaceDiff)

	err := shell.HandleFileDrop([]string{"/tmp/a.txt"}, "p1")
	if err != nil {
		t.Fatalf("guarded drop must not error, got %v", err)
	}
	if shell.surface.current() != WorkspaceSurfaceDiff {
		t.Fatal("drop must not change the visible surface")
	}
}

// TestChangesPanelClicksOpenDiff pins GWB-145: the changes tree row
// activation opens the center diff for the file.
func TestChangesPanelClicksOpenDiff(t *testing.T) {
	shell := gitSurfaceTestShell(t, t.TempDir())
	shell.changesClick(gitworkbench.FlatRow{
		Node:  &gitworkbench.TreeNode{Name: "README.md", Path: "README.md", File: &gitworkbench.ChangeFile{Path: "README.md"}},
		Depth: 1,
	})
	if shell.surface.current() != WorkspaceSurfaceDiff {
		t.Fatalf("surface = %v", shell.surface.current())
	}
	if shell.surface.diff.SelectedPath != "README.md" {
		t.Fatalf("selected = %q", shell.surface.diff.SelectedPath)
	}
}

// TestGeneratedFileCardsCollapseByDefault pins GWB-155: a generated file's
// card starts collapsed — a header row only, never line rows — until the
// user expands it.
func TestGeneratedFileCardsCollapseByDefault(t *testing.T) {
	shell := gitSurfaceTestShell(t, t.TempDir())
	shell.git.snapshot.Files = append(shell.git.snapshot.Files, gitworkbench.ChangeFile{
		Path:      "go.sum",
		Status:    gitworkbench.StatusModified,
		Generated: true,
		Additions: 500,
		Hunks:     []gitworkbench.Hunk{{OldStart: 1, OldLines: 1, NewStart: 1, NewLines: 1}},
	})
	shell.rebuildDiffRows()
	shell.buildGdRows()

	idx := -1
	for i, f := range shell.git.gdFiles {
		if f.cf.Path == "go.sum" {
			idx = i
		}
	}
	if idx < 0 {
		t.Fatal("generated file missing from the review files")
	}
	if !shell.git.gdFiles[idx].collapsed {
		t.Fatal("generated file must collapse by default")
	}
	for _, row := range shell.git.gdRows {
		if int(row.file) == idx && row.kind == gdRowLine {
			t.Fatal("generated file must not emit line rows while collapsed")
		}
	}
}

// TestProductionDirectoryLoadNeverBlocksRender pins GWB-003 on the
// production path: beginDirectoryLoad marks the request in-flight and
// returns without populating entries — the render path cannot have waited
// on filesystem IO.
func TestProductionDirectoryLoadNeverBlocksRender(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "README.md"), []byte("x\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	shell := gitSurfaceTestShell(t, dir)
	shell.rightPanel.currentRoot = dir

	gen := shell.rightPanel.filesGen.Add(1)
	done := make(chan struct{})
	go func() { shell.beginDirectoryLoad(dir, dir, gen); close(done) }()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("beginDirectoryLoad blocked the caller")
	}
	if _, ok := shell.rightPanel.cachedDirEntries[dir]; ok {
		t.Fatal("production lane must not apply entries synchronously")
	}
	if !shell.rightPanel.loadingDirs[dir] {
		t.Fatal("production lane must mark the request in-flight")
	}
}

// TestHeaderChangesToggleWithoutSnapshotDoesNotPanic pins the 2026-10-05
// production crash: the header Terminal|Changes toggle calls
// openChanges(""), which resolved firstChangedPath over a nil snapshot
// (non-repo cwd, or the refresh still in flight) and dereferenced
// snap.Files, killing the whole process. The toggle must degrade to an
// empty Diff selection instead.
func TestHeaderChangesToggleWithoutSnapshotDoesNotPanic(t *testing.T) {
	shell := NewShell()
	shell.git.root = "" // no repository bound, no snapshot ever loaded

	if firstChangedPath(nil) != "" {
		t.Fatal("firstChangedPath(nil) must return an empty path")
	}

	shell.openChanges("") // must not panic
	if shell.surface.current() != WorkspaceSurfaceDiff {
		t.Fatalf("surface = %v, want Diff", shell.surface.current())
	}
	if shell.surface.diff.SelectedPath != "" {
		t.Fatalf("selected path = %q, want empty", shell.surface.diff.SelectedPath)
	}
}

// TestApplyGitSnapshotSuppressesDriftForOwnMutations pins F25 (2026-10-06):
// a snapshot refreshed after the app's own stage/commit carries a new
// signature by design and must not raise the "local changes detected"
// external-drift banner; an unexpected signature change still does.
func TestApplyGitSnapshotSuppressesDriftForOwnMutations(t *testing.T) {
	root := t.TempDir()
	shell := gitSurfaceTestShell(t, root)

	// The app's own mutation: expectDrift suppresses the banner.
	shell.git.expectDrift = true
	shell.applyGitSnapshot(&gitworkbench.ChangesSnapshot{
		Root: root, Branch: "main", Signature: "sig-2",
		Files: []gitworkbench.ChangeFile{{Path: "src/main.go", Status: gitworkbench.StatusModified}},
	})
	if shell.git.staleBanner {
		t.Fatal("own mutation flagged as external drift")
	}
	if shell.git.expectDrift {
		t.Fatal("expectDrift was not consumed")
	}

	// A later unexpected signature change is still external drift.
	shell.applyGitSnapshot(&gitworkbench.ChangesSnapshot{
		Root: root, Branch: "main", Signature: "sig-3",
		Files: []gitworkbench.ChangeFile{{Path: "src/main.go", Status: gitworkbench.StatusModified}},
	})
	if !shell.git.staleBanner {
		t.Fatal("external signature change did not raise the drift banner")
	}
}
