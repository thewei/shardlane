package nativeui

/**
 * [INPUT]: native MyGo sidebar animation, Terminal/Changes/Files/History routes and narrow screens
 * [OUTPUT]: only the left navigation slides; no second rail competes with center
 * [POS]: WIX layout/route regression after retiring Right Panel
 * [PROTOCOL]: update header and check CLAUDE.md
 */

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"testing"
)

func slideTestShell(t *testing.T) *Shell {
	t.Helper()
	s := NewShell()
	s.activeInstance = "inst-1"
	s.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: t.TempDir()}},
		Tabs:     []herdr.Tab{{ID: "t1", Label: "main", ProjectID: "w1"}},
		Panes:    []herdr.Pane{{ID: "p1", TerminalID: "term-1", TabID: "t1"}},
	}
	s.reconcileLocalSelection(s.projection)
	return s
}
func slideWithoutMotion(s *Shell) *ui.Tester {
	tt := ui.NewTester(s.View, 1200, 800)
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	return tt
}
func TestSidebarSlideKeepsContentWhileAnimating(t *testing.T) {
	s := slideTestShell(t)
	tt := ui.NewTester(s.View, 1200, 800)
	if !tt.HasText("No active agents") {
		t.Fatal("no original sidebar")
	}
	s.toggleSidebar()
	tt.Frame()
	if !tt.HasText("No active agents") {
		t.Fatal("sidebar disappeared before width animation finished")
	}
}
func TestSidebarSlideUnmountsWhenSettled(t *testing.T) {
	s := slideTestShell(t)
	tt := slideWithoutMotion(s)
	s.toggleSidebar()
	tt.Frame()
	if tt.HasText("No active agents") {
		t.Fatal("collapsed rail should not build")
	}
	s.toggleSidebar()
	tt.Frame()
	if !tt.HasText("No active agents") {
		t.Fatal("expanded rail missing")
	}
}

func TestWorkspaceRouteHasNoRightRailAcrossWidths(t *testing.T) {
	s := slideTestShell(t)
	s.rightPanel.open = true // migrated stale session preferences must be inert
	s.rightPanel.historyOpen = true
	tt := ui.NewTester(s.View, 960, 640)
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	for _, size := range [][2]int{{700, 640}, {960, 640}, {1280, 800}, {1512, 982}} {
		tt.SetSize(size[0], size[1])
		tt.Frame()
		if tt.HasText("Show Workspace Tools") || tt.HasText("History details") || tt.HasText("Listening ports") {
			t.Fatalf("right rail returned at %dx%d: %q", size[0], size[1], tt.Texts())
		}
		if !tt.HasText("Files view") {
			t.Fatal("Files header entry disappeared on resize")
		}
	}
}

func TestFilesNavigationKeepsLeftTreeAtNarrowWidth(t *testing.T) {
	s := slideTestShell(t)
	s.sidebarCollapsed = true
	s.showViewFiles()
	tt := ui.NewTester(s.View, 960, 640)
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	// Headless runs have no window, so the WebView preview cannot exist;
	// the center must still render its truthful state beside the tree.
	if s.router.Path() != routeFiles || s.sidebarCollapsed || !tt.HasText("Preview unavailable") {
		t.Fatalf("Files should show directory tree and central preview: %q", tt.Texts())
	}
}
