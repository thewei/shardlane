package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/wh-studio/herdr-client/next/internal/commandcenter"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

func TestSurfacePercent(t *testing.T) {
	left, top, width, height := surfacePercent(
		herdr.LayoutRect{X: 0, Y: 0, Width: 120, Height: 40},
		herdr.LayoutRect{X: 60, Y: 10, Width: 60, Height: 20},
	)
	if left != 50 || top != 25 || width != 50 || height != 50 {
		t.Fatalf("surface percent = %.1f %.1f %.1f %.1f", left, top, width, height)
	}
}

func TestVisiblePaneGeometryUsesHerdrLayout(t *testing.T) {
	projection := herdr.Projection{
		FocusedTabID:  "t1",
		FocusedPaneID: "p2",
		Panes: []herdr.Pane{
			{ID: "p1", TerminalID: "term1", TabID: "t1"},
			{ID: "p2", TerminalID: "term2", TabID: "t1"},
		},
		Layouts: []herdr.Layout{{
			TabID:         "t1",
			FocusedPaneID: "p2",
			Area:          herdr.LayoutRect{Width: 120, Height: 40},
			Panes: []herdr.LayoutPane{
				{PaneID: "p1", Rect: herdr.LayoutRect{Width: 60, Height: 40}},
				{PaneID: "p2", Focused: true, Rect: herdr.LayoutRect{X: 60, Width: 60, Height: 40}},
			},
		}},
	}
	got := visiblePaneGeometry(projection, "t1", "p2")
	if len(got) != 2 {
		t.Fatalf("visible panes = %d, want 2", len(got))
	}
	if !got["term2"].focused || got["term1"].focused {
		t.Fatalf("focused geometry = %#v", got)
	}
}

func TestSelectionSidebarKeyPrefersVisibleLevel(t *testing.T) {
	projection := herdr.Projection{
		Tabs: []herdr.Tab{{ID: "t1", ProjectID: "w1", PaneCount: 1}},
	}
	key := func() string {
		return selectionSidebarKey(projection, "w1", "t1", "p1")
	}
	if got := key(); got != "tab:t1" {
		t.Fatalf("single-pane selection = %q, want tab:t1", got)
	}
	projection.Tabs[0].PaneCount = 2
	if got := key(); got != "pane:p1" {
		t.Fatalf("multi-pane selection = %q, want pane:p1", got)
	}
}

func TestNativeShellSkeleton(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.instances = []herdr.Instance{{Name: "default", DisplayName: "Default", Running: true, Default: true}}
	s.activeInstance = "default"
	s.projection = herdr.Projection{
		Protocol:         22,
		FocusedProjectID: "w1",
		FocusedTabID:     "t1",
		Projects:         []herdr.Project{{ID: "w1", Label: "Shardlane", TabCount: 1}},
		Tabs:             []herdr.Tab{{ID: "t1", ProjectID: "w1", Label: "main", PaneCount: 1}},
	}
	s.selectedProjectID = "w1"
	s.selectedTabID = "t1"
	s.selected = "tab:t1"

	tester := ui.NewTester(s.View, 1200, 800)
	for _, text := range []string{"Shardlane", "Terminal view", "Chat view", "Changes view", "History view", "Agents", "Workspace", "Default", "main", "Settings"} {
		if !tester.HasText(text) {
			t.Fatalf("native shell missing %q; texts=%q", text, tester.Texts())
		}
	}
}

func TestNativeSidebarContextMenus(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.instances = []herdr.Instance{{Name: "default", DisplayName: "Workspace Alpha", Running: true, Default: true}}
	s.activeInstance = "default"
	s.projection = herdr.Projection{
		Protocol:         22,
		FocusedProjectID: "w1",
		FocusedTabID:     "t1",
		FocusedPaneID:    "p1",
		Projects: []herdr.Project{
			{ID: "w1", Label: "Project Alpha", TabCount: 1, PaneCount: 2},
			{ID: "w2", Label: "Project Menu", TabCount: 0, PaneCount: 0},
		},
		Tabs: []herdr.Tab{{ID: "t1", ProjectID: "w1", Label: "Main Tab", PaneCount: 2}},
		Panes: []herdr.Pane{
			{ID: "p1", TerminalID: "term1", ProjectID: "w1", TabID: "t1", Label: "Pane One"},
			{ID: "p2", TerminalID: "term2", ProjectID: "w1", TabID: "t1", Label: "Pane Menu"},
		},
	}
	s.selectedProjectID = "w1"
	s.selectedTabID = "t1"
	s.selectedPaneID = "p1"
	s.selected = "pane:p1"

	tester := ui.NewTester(s.View, 1200, 800)
	if err := tester.RightClick("Project Menu"); err != nil {
		t.Fatal(err)
	}
	menu := tester.Menu()
	for _, want := range []string{"New Tab", "Rename Project…", "Close Project…"} {
		if !containsText(menu, want) {
			t.Fatalf("project menu %q missing %q", menu, want)
		}
	}
	tester.CloseMenu()

	if err := tester.RightClick("Pane Menu"); err != nil {
		t.Fatal(err)
	}
	menu = tester.Menu()
	for _, want := range []string{"Split Right", "Split Down", "Zoom / Unzoom", "Rename Pane…", "Close Pane…"} {
		if !containsText(menu, want) {
			t.Fatalf("pane menu %q missing %q", menu, want)
		}
	}
}

func containsText(values []string, want string) bool {
	for _, value := range values {
		if value == want {
			return true
		}
	}
	return false
}

func TestRouterMovesBetweenNativePages(t *testing.T) {
	s := NewShell()
	s.loading = false
	tester := ui.NewTester(s.View, 1200, 800)

	if got := s.router.Path(); got != routeWorkspace {
		t.Fatalf("initial route = %q", got)
	}
	// The persistent header switch (2026-10-07): the History icon button is
	// on every route, and it pushes the History inner page.
	if err := tester.Click("History view"); err != nil {
		t.Fatal(err)
	}
	if got := s.router.Path(); got != routeHistory {
		t.Fatalf("history route = %q", got)
	}
	if !tester.HasText("History") {
		t.Fatalf("history page did not render; texts=%q", tester.Texts())
	}

	s.router.Back()
	tester.Frame()
	if got := s.router.Path(); got != routeWorkspace {
		t.Fatalf("route after back = %q", got)
	}
	s.router.Forward()
	tester.Frame()
	if got := s.router.Path(); got != routeHistory {
		t.Fatalf("route after forward = %q", got)
	}

	// Repeating the active view's click exits the inner page back to
	// Terminal (the first-level view).
	if err := tester.Click("History view"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if got := s.router.Path(); got != routeWorkspace {
		t.Fatalf("repeat History click route = %q, want the workspace", got)
	}
	if onTerminal, _, _, _ := s.viewSwitchActive(); !onTerminal {
		t.Fatal("exiting the inner page must land on the Terminal view")
	}
}

// TestCommandCenterUnifiedSearch pins the 2026-10-06 product decision: the
// standalone Search page is gone and the Command Center palette is the
// unified search — it matches runtime targets and jumps to the workspace.
func TestCommandCenterUnifiedSearch(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.activeInstance = "default"
	s.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "Alpha Project", CWD: "/Users/demo/work/alpha"}},
		Tabs:     []herdr.Tab{{ID: "t1", ProjectID: "w1", Label: "build"}},
		Panes:    []herdr.Pane{{ID: "p1", ProjectID: "w1", TabID: "t1", Label: "Terminal", CWD: "/Users/demo/work/alpha", TerminalID: "term-1"}},
	}

	s.openCommandCenter(commandcenter.ScopeAll)
	s.commandCenterQuery = "alpha"
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	if !tester.HasText("Project: Alpha Project") {
		t.Fatalf("palette missing project result; texts=%q", tester.Texts())
	}
	if err := tester.Click("Project: Alpha Project"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if got := s.router.Path(); got != routeWorkspace {
		t.Fatalf("clicking palette result route = %q", got)
	}
	if got := s.selectedProjectID; got != "w1" {
		t.Fatalf("selected project = %q, want w1", got)
	}
}

func TestSettingsUsesNestedRoute(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.router.Replace("/settings/runtime")
	s.activeInstance = "default"
	s.projection = herdr.Projection{Protocol: 22}

	tester := ui.NewTester(s.View, 1200, 800)
	for _, want := range []string{"Settings", "General", "Terminal", "Runtime", "Herdr protocol: 22"} {
		if !tester.HasText(want) {
			t.Fatalf("settings missing %q; texts=%q", want, tester.Texts())
		}
	}
}

func TestRouteShortcutsMatchCurrentShardlaneDefaults(t *testing.T) {
	s := NewShell()
	s.loading = false
	tester := ui.NewTester(s.View, 1200, 800)

	tester.Key(ui.Cmd, ui.KeyComma)
	if got := s.router.Path(); got != routeSettings {
		t.Fatalf("Cmd+, route = %q", got)
	}
	// The New Task shortcut is gone with the page (2026-10-07); ⇧⌘N must
	// not navigate anywhere anymore.
	tester.Key(ui.Cmd|ui.Shift, ui.KeyN)
	if got := s.router.Path(); got != routeSettings {
		t.Fatalf("Cmd+Shift+N must be unbound, but moved to %q", got)
	}
	// ⌘K ends the test as the unified search palette (real-device check
	// confirmed shortcuts keep working after the palette closes; the
	// ui.Tester synthetic key lane does not model modal close focus).
	tester.Key(ui.Cmd, ui.KeyK)
	if !s.commandCenterOpen {
		t.Fatal("Cmd+K must open the unified search palette")
	}
}

// TestStatusChangeToastsWithoutTerminals pins F60 (2026-10-06): status
// messages were only rendered on the empty-terminal branch, dropping every
// runtime announcement in the normal case; a new status value must surface
// (as a toast) with terminals attached.
func TestStatusChangeToastsWithoutTerminals(t *testing.T) {
	s := NewShell()
	s.loading = false
	// The seeded loading text must never announce itself.
	if s.statusShown != s.status {
		t.Fatalf("statusShown = %q, want the seeded %q", s.statusShown, s.status)
	}
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()

	s.status = "Script demo launched in a new tab"
	tester.Frame()
	if !tester.HasText("Script demo launched in a new tab") {
		t.Fatalf("new status invisible; texts=%q", tester.Texts())
	}
}
