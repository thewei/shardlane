package nativeui

import (
	"os"
	"os/exec"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/gitworkbench"
)

// sidebarSplitTestShell builds a diff-mode shell whose snapshot splits two
// files per pane, so the Unstaged and Staged panes each have distinct rows.
func sidebarSplitTestShell(t *testing.T, root string) (*Shell, *ui.Tester) {
	t.Helper()
	shell := gitSurfaceTestShell(t, root)
	shell.git.rootOf[root] = root
	shell.git.snapshot.Files = []gitworkbench.ChangeFile{
		{Path: "a.go", Status: gitworkbench.StatusModified, Unstaged: true, Additions: 1},
		{Path: "b.go", Status: gitworkbench.StatusModified, Unstaged: true, Additions: 2},
		{Path: "c.go", Status: gitworkbench.StatusModified, Staged: true, Additions: 3},
		{Path: "d.go", Status: gitworkbench.StatusModified, Staged: true, Additions: 4},
	}
	// Pin the refresh flag: a background snapshot refresh lands the cache
	// asynchronously, and a later ensureGitSnapshot would swap the seeded
	// snapshot for the temp dir's real (empty) one mid-test.
	shell.git.refreshing = true
	if shell.router.Path() != routeWorkspace {
		shell.router.Push(routeWorkspace)
	}
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	return shell, tester
}

// clickChoice clicks the row labeled text until want holds. The headless
// frame loop is real-time driven (row animations), so a click can land on a
// frame boundary and need one more push; the choice itself is the arbiter.
func clickChoice(t *testing.T, shell *Shell, tester *ui.Tester, mods ui.Modifiers, text string, want func() bool, context string) {
	t.Helper()
	var lastTexts []string
	var lastErr string
	for attempt := 0; attempt < 5; attempt++ {
		if want() {
			return
		}
		var err error
		if mods == 0 {
			err = tester.Click(text)
		} else {
			err = tester.ClickWith(mods, text)
		}
		lastErr = ""
		if err != nil {
			lastErr = err.Error()
		}
		tester.Frame()
		lastTexts = tester.Texts()
		time.Sleep(10 * time.Millisecond)
	}
	if !want() {
		t.Fatalf("%s: click did not take effect (lastErr=%v texts=%q)", context, lastErr, lastTexts)
	}
}

// TestSidebarPaneToggleReexpands pins the 2026-10-06 annotation A3: a
// collapsed pane's header re-expands it on the next click — the first
// click after the collapse used to be swallowed, so the pane never came
// back.
func TestSidebarPaneToggleReexpands(t *testing.T) {
	shell, tester := sidebarSplitTestShell(t, t.TempDir())

	for _, title := range []string{"Toggle Staged Files", "Toggle Unstaged Files"} {
		if err := tester.Click(title); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
		if err := tester.Click(title); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
		switch title {
		case "Toggle Staged Files":
			if !shell.stagedOpen {
				t.Fatal("staged pane did not re-expand on the second click")
			}
		case "Toggle Unstaged Files":
			if !shell.unstagedOpen {
				t.Fatal("unstaged pane did not re-expand on the second click")
			}
		}
	}
	// The rows are back after the round trip.
	if !tester.HasText("c.go") || !tester.HasText("d.go") {
		t.Fatalf("staged rows missing after re-expand; texts=%q", tester.Texts())
	}
}

// TestSidebarPaneDividerDragTracksPointer pins the 2026-10-06 annotation
// A4: dragging the pane divider resizes the panes, and dragging down hands
// the space to the unstaged pane above (the staged pane below shrinks).
func TestSidebarPaneDividerDragTracksPointer(t *testing.T) {
	shell, tester := sidebarSplitTestShell(t, t.TempDir())
	r, ok := tester.Find("Resize panes")
	if !ok {
		t.Fatalf("pane divider missing; texts=%q", tester.Texts())
	}
	x, y := r.X+r.W/2, r.Y+r.H/2

	tester.Press(x, y)
	tester.Move(x, y+120)
	tester.Release(x, y+120)
	tester.Frame()
	if grown := shell.stagedPaneHeight; grown >= 220 {
		t.Fatalf("dragging the divider down must shrink the staged pane, got %v", grown)
	}

	// Dragging up gives the space back to the staged pane.
	tester.Frame()
	r, _ = tester.Find("Resize panes")
	x, y = r.X+r.W/2, r.Y+r.H/2
	tester.Press(x, y)
	tester.Move(x, y-60)
	tester.Release(x, y-60)
	tester.Frame()
	if shell.stagedPaneHeight <= 100 {
		t.Fatalf("dragging the divider up must grow the staged pane, got %v", shell.stagedPaneHeight)
	}
}

// TestSidebarPaneMultiSelectTargetsAction pins the 2026-10-06 annotations
// A1/A2: Cmd-click chooses several rows of a pane, the pane action targets
// exactly that selection, and a plain click falls back to choosing one row.
func TestSidebarPaneMultiSelectTargetsAction(t *testing.T) {
	shell, tester := sidebarSplitTestShell(t, t.TempDir())

	clickChoice(t, shell, tester, ui.Cmd, "a.go", func() bool { return shell.unstagedChoice.Has("a.go") }, "choose a.go")
	clickChoice(t, shell, tester, ui.Cmd, "b.go", func() bool {
		return shell.unstagedChoice.Has("a.go") && shell.unstagedChoice.Has("b.go")
	}, "choose b.go")

	// The Stage action then targets exactly the selection.
	unstagedFiles, _ := shell.splitSections(shell.gitSnapshot())
	if got := gdActionPaths(&shell.unstagedChoice, unstagedFiles); len(got) != 2 || got[0] != "a.go" || got[1] != "b.go" {
		t.Fatalf("action paths = %v, want the two chosen files in pane order", got)
	}

	// A plain click replaces the choice with the one row and opens it.
	clickChoice(t, shell, tester, 0, "a.go", func() bool {
		return shell.unstagedChoice.Len() == 1 && shell.unstagedChoice.Has("a.go")
	}, "plain-click a.go")

	// With nothing chosen the action targets the whole pane.
	shell.unstagedChoice.Clear()
	if got := gdActionPaths(&shell.unstagedChoice, unstagedFiles); len(got) != 2 {
		t.Fatalf("empty choice must fall back to the whole pane, got %v", got)
	}

	// The staged pane's selection is its own.
	clickChoice(t, shell, tester, ui.Cmd, "c.go", func() bool {
		return shell.stagedChoice.Has("c.go") && shell.unstagedChoice.Len() == 0
	}, "choose c.go")
}

// TestSidebarStageSelectionRunsGitAdd pins the selection end to end: with
// two rows chosen, the Stage action stages exactly those files.
func TestSidebarStageSelectionRunsGitAdd(t *testing.T) {
	root := t.TempDir()
	gitScript(t, root,
		[]string{"init", "-b", "main"},
		[]string{"config", "user.email", "test@example.com"},
		[]string{"config", "user.name", "Test"},
	)
	writeRepoFile(t, root, "a.go", "one\n")
	writeRepoFile(t, root, "b.go", "two\n")
	writeRepoFile(t, root, "c.go", "three\n")
	gitScript(t, root, []string{"add", "."}, []string{"commit", "-m", "base"})
	writeRepoFile(t, root, "a.go", "one changed\n")
	writeRepoFile(t, root, "b.go", "two changed\n")
	writeRepoFile(t, root, "c.go", "three changed\n")

	shell, tester := sidebarSplitTestShell(t, root)
	shell.git.root = root

	clickChoice(t, shell, tester, ui.Cmd, "a.go", func() bool { return shell.unstagedChoice.Has("a.go") }, "choose a.go")
	clickChoice(t, shell, tester, ui.Cmd, "b.go", func() bool {
		return shell.unstagedChoice.Has("a.go") && shell.unstagedChoice.Has("b.go")
	}, "choose b.go")
	if err := tester.Click("Stage"); err != nil {
		t.Fatal(err)
	}

	// The op runs on a background lane; its toast flashes for one frame, so
	// the real effect — the index — is the assertion.
	deadline := time.Now().Add(10 * time.Second)
	for {
		out, err := exec.Command("git", "-C", root, "diff", "--cached", "--name-only").Output()
		if err == nil && string(out) == "a.go\nb.go\n" {
			break
		}
		if !time.Now().Before(deadline) {
			t.Fatalf("stage op did not stage the two chosen files in time, index=%q (%v)", out, err)
		}
		time.Sleep(5 * time.Millisecond)
	}
}

// gitScript runs git commands in dir, failing the test on the first error.
func gitScript(t *testing.T, dir string, argv ...[]string) {
	t.Helper()
	for _, args := range argv {
		cmd := exec.Command("git", args...)
		cmd.Dir = dir
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v\n%s", args, err, out)
		}
	}
}

func writeRepoFile(t *testing.T, dir, name, content string) {
	t.Helper()
	if err := os.WriteFile(dir+"/"+name, []byte(content), 0o644); err != nil {
		t.Fatalf("write %s: %v", name, err)
	}
}

// TestSidebarRepoChipPopoverReopens pins the 2026-10-06 annotation A2: a
// repo chip's popover opens again after it was closed — the click-counter
// toggle used to swallow every click after the first, so the second open
// never came.
func TestSidebarRepoChipPopoverReopens(t *testing.T) {
	shell, tester := sidebarSplitTestShell(t, t.TempDir())

	if err := tester.Click("Stashes"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !shell.stashesMenuOpen {
		t.Fatal("first click did not open the stashes menu")
	}
	if !tester.HasText("Stash All Changes") {
		t.Fatalf("stashes panel missing after open; texts=%q", tester.Texts())
	}

	// A click outside the popover closes it through the backdrop.
	tester.ClickAt(600, 600)
	tester.Frame()
	if shell.stashesMenuOpen {
		t.Fatal("outside click did not close the stashes menu")
	}

	// The pin: after the close, the chip opens the popover again.
	if err := tester.Click("Stashes"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !shell.stashesMenuOpen {
		t.Fatal("popover did not reopen after being closed")
	}
	if !tester.HasText("Stash All Changes") {
		t.Fatalf("stashes panel missing after reopen; texts=%q", tester.Texts())
	}
}
