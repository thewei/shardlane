package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// TestRepoBarChipsOpenMenus pins the 2026-10-05 fix: the diff-mode repo bar
// chips (Tags/Stashes/Worktrees and the branch chip) must open their menus
// on click. Popover only renders while open, the anchor click must toggle
// it exactly once, and the popover content must not touch the open flag
// while building (which closed the menu the frame it opened).
func TestRepoBarChipsOpenMenus(t *testing.T) {
	root := t.TempDir()
	shell := gitSurfaceTestShell(t, root)
	shell.git.rootOf[root] = root
	shell.git.tags = []gitworkbench.Tag{{Name: "v1.0"}}
	if shell.router.Path() != routeWorkspace {
		shell.router.Push(routeWorkspace)
	}
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	if err := tester.Click("Tags 1"); err != nil {
		t.Fatalf("click Tags chip: %v", err)
	}
	tester.Frame() // the click is evaluated on the next render pass
	if !shell.tagsMenuOpen {
		t.Fatal("clicking the Tags chip must open its menu")
	}
	tester.Frame()
	if !tester.HasText("No tags") && !tester.HasText("Create") {
		t.Fatal("the tags panel must stay open across frames")
	}
}
