package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

// slideTestShell builds a minimal shell with one visible project so the
// sidebar has content to assert on.
func slideTestShell(t *testing.T) *Shell {
	t.Helper()
	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: t.TempDir()}},
		Tabs:     []herdr.Tab{{ID: "t1", Label: "main", ProjectID: "w1"}},
		Panes:    []herdr.Pane{{ID: "p1", TerminalID: "term-1", TabID: "t1"}},
	}
	shell.reconcileLocalSelection(shell.projection)
	return shell
}

// slideWithoutMotion builds the tester with the desktop Reduce Motion
// preference: Animate then lands widths at once, so collapsed/expanded
// assertions are deterministic (headless frames advance no real time).
func slideWithoutMotion(shell *Shell) *ui.Tester {
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	return tester
}

// TestSidebarSlideKeepsContentWhileAnimating pins the animation seam:
// one frame after collapsing, the sidebar content still builds (clipped
// by the sliding host) instead of popping out.
func TestSidebarSlideKeepsContentWhileAnimating(t *testing.T) {
	shell := slideTestShell(t)
	tester := ui.NewTester(shell.View, 1200, 800)
	if !tester.HasText("No active agents") {
		t.Fatal("sidebar project missing before collapse")
	}

	shell.toggleSidebar()
	tester.Frame()
	if !tester.HasText("No active agents") {
		t.Fatal("sidebar content must stay mounted while the slide animates")
	}
}

// TestSidebarSlideUnmountsWhenSettled pins the resting contract: under
// Reduce Motion the collapsed sidebar builds no content, and expanding
// brings it back.
func TestSidebarSlideUnmountsWhenSettled(t *testing.T) {
	shell := slideTestShell(t)
	tester := slideWithoutMotion(shell)

	shell.toggleSidebar()
	tester.Frame()
	if tester.HasText("No active agents") {
		t.Fatal("collapsed sidebar must not build content")
	}

	shell.toggleSidebar()
	tester.Frame()
	if !tester.HasText("No active agents") {
		t.Fatal("expanded sidebar must build content again")
	}
}

// TestRightPanelSlideMatchesOpenState pins the Right Panel reveal: with
// motion off, open builds the tool surface and closed unmounts it; the
// center surface never changes (GWB-074 contract still holds).
func TestRightPanelSlideMatchesOpenState(t *testing.T) {
	shell := slideTestShell(t)
	tester := slideWithoutMotion(shell)

	if tester.HasText("Files") && tester.HasText("Services") {
		t.Fatal("right panel must start closed")
	}

	shell.openRightPanelSurface(SurfaceServices)
	tester.Frame()
	if !tester.HasText("Services") {
		t.Fatal("opened right panel must build its surface")
	}

	shell.toggleRightPanel()
	tester.Frame()
	if tester.HasText("Services") {
		t.Fatal("closed right panel must not build content")
	}
}
