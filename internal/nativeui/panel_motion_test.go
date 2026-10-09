package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
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
// TestContextPanelWidthPolicy keeps the Terminal useful instead of letting
// two rails consume nearly all available content width.
func TestContextPanelWidthPolicy(t *testing.T) {
	for _, tc := range []struct {
		width   float32
		sidebar bool
		panel   int
		fits    bool
	}{
		{960, true, DefaultRightPanelWidth, false},
		{960, false, DefaultRightPanelWidth, true},
		{1280, true, DefaultRightPanelWidth, true},
		{700, false, DefaultRightPanelWidth, false},
		{960, true, MinRightPanelWidth, false},
		{1512, true, MaxRightPanelWidth, true},
	} {
		if got := contextPanelFits(tc.width, tc.sidebar, tc.panel); got != tc.fits {
			t.Errorf("width %v sidebar %v panel %d: fits=%v, want %v", tc.width, tc.sidebar, tc.panel, got, tc.fits)
		}
	}
}

func TestContextPanelAutoHidesAndRestoresWithoutLosingPreference(t *testing.T) {
	shell := slideTestShell(t)
	shell.openRightPanelSurface(SurfaceServices)
	tester := ui.NewTester(shell.View, 960, 640)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	tester.Frame()
	if tester.HasText("Services") || !tester.HasText("Show Workspace Tools") {
		t.Fatalf("narrow window must auto-hide inspector: %q", tester.Texts())
	}
	if !shell.rightPanel.open || shell.sidebarCollapsed {
		t.Fatal("responsive auto-hide must not overwrite saved rail visibility")
	}
	tester.SetSize(1280, 800)
	tester.Frame()
	if !tester.HasText("Services") || !tester.HasText("Hide Workspace Tools") {
		t.Fatalf("inspector should reappear after resize: %q", tester.Texts())
	}
	if shell.rightPanel.surface != SurfaceServices {
		t.Fatal("responsive auto-hide lost selected Workspace tool")
	}
}

func TestExplicitInspectorOpenMakesRoomOrShowsReason(t *testing.T) {
	shell := slideTestShell(t)
	shell.toggleRightPanelForViewport(960)
	if !shell.sidebarCollapsed || !shell.rightPanel.open {
		t.Fatal("explicit open at 960 DIP should collapse sidebar to make room")
	}
	shell.toggleRightPanelForViewport(960)
	if shell.rightPanel.open {
		t.Fatal("repeat toggle should close the now-visible right panel")
	}
	shell.sidebarCollapsed = false
	shell.toggleRightPanelForViewport(700)
	if shell.rightPanel.open || shell.sidebarCollapsed || shell.pendingToast == "" {
		t.Fatal("too-narrow explicit open must explain limitation without mutating rail state")
	}
}

func TestCommandCenterInspectorDestinationRespectsWidthAndRoute(t *testing.T) {
	shell := slideTestShell(t)
	shell.router.Replace(routeHistory)
	shell.openRightPanelSurfaceForViewport(SurfaceFiles, 700)
	if shell.router.Path() != routeHistory || shell.rightPanel.open || shell.pendingToast == "" {
		t.Fatal("too narrow Files action must not silently open a hidden Workspace panel")
	}
	shell.pendingToast = ""
	shell.openRightPanelSurfaceForViewport(SurfaceFiles, 960)
	if shell.router.Path() != routeWorkspace || !shell.rightPanel.open || !shell.sidebarCollapsed || shell.rightPanel.surface != SurfaceFiles {
		t.Fatalf("Files action failed to expose Workspace tool: route=%q open=%v sidebar=%v tool=%q", shell.router.Path(), shell.rightPanel.open, shell.sidebarCollapsed, shell.rightPanel.surface)
	}
}

func TestHistoryInspectorResizesIndependently(t *testing.T) {
	shell := slideTestShell(t)
	shell.router.Replace(routeHistory)
	shell.rightPanel.historyOpen = true
	tester := ui.NewTester(shell.View, 960, 640)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	tester.Frame()
	if tester.HasText("History details") || !shell.rightPanel.historyOpen {
		t.Fatal("narrow History inspector should auto-hide without clearing its state")
	}
	tester.SetSize(1280, 800)
	tester.Frame()
	if !tester.HasText("History details") || !shell.rightPanel.historyOpen {
		t.Fatal("History inspector should return on resize independently of Workspace tool")
	}
}

// TestRightPanelSlideMatchesOpenState pins the standard open/closed states.
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
