package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// TestRepoBarChipsOpenMenus guards the Git reference group's action popover.
// Opening its menu must not collapse the section or self-dismiss.
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

	if err := tester.Click("Tags actions"); err != nil {
		t.Fatalf("click Tags section action: %v", err)
	}
	tester.Frame() // the click is evaluated on the next render pass
	if !shell.tagsMenuOpen {
		t.Fatal("clicking the Tags section action must open its menu")
	}
	tester.Frame()
	if !tester.HasText("No tags") && !tester.HasText("Create") {
		t.Fatal("the tags panel must stay open across frames")
	}
}
