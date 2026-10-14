package nativeui

import (
	"math"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// TestInnerPageBackPinnedToSidebarFoot pins the unified inner-page chrome:
// every inner page carries its one way back at the foot of the sidebar,
// below the page navigation and in place of the Workspace switch, which only
// the workspace sidebar keeps. A session detail's Back names the Sessions
// list; the other pages name the Terminal.
func TestInnerPageBackPinnedToSidebarFoot(t *testing.T) {
	const height = 800
	s := NewShell()
	s.loading = false

	s.router.Replace(routeWorkspace)
	tester := ui.NewTester(s.View, 1200, height)
	if !tester.HasText("Choose Session") || tester.HasText("Back") {
		t.Fatalf("workspace sidebar must keep its Workspace switch and no back control; texts=%q", tester.Texts())
	}

	s.router.Replace("/settings/general")
	backEntries := countTextExact(ui.NewTester(s.View, 1200, height).Texts(), "Back")
	if backEntries == 0 {
		t.Fatal("the Settings sidebar shows no Back control")
	}
	for _, tc := range []struct{ route, nav string }{
		{"/settings/general", "General"},
		{routeHistory, "All sessions"},
		{"/history-projects", "All sessions"},
		{"/history/codex:abc", "All sessions"},
	} {
		s.router.Replace(tc.route)
		tester := ui.NewTester(s.View, 1200, height)
		back, ok := tester.Find("Back")
		if !ok {
			t.Fatalf("%s: sidebar missing Back; texts=%q", tc.route, tester.Texts())
		}
		if back.X > sidebarWidth || back.Y < height-60 {
			t.Fatalf("%s: back control at %+v, want the sidebar foot", tc.route, back)
		}
		if nav, ok := tester.Find(tc.nav); !ok || nav.Y >= back.Y {
			t.Fatalf("%s: page navigation %q at %+v must sit above Back %+v", tc.route, tc.nav, nav, back)
		}
		if tester.HasText("Choose Session") {
			t.Fatalf("%s: inner page kept the Workspace switch; texts=%q", tc.route, tester.Texts())
		}
		// One control, one name: the destination is the tooltip's job.
		if got := countTextExact(tester.Texts(), "Back"); got != backEntries {
			t.Fatalf("%s: %d Back entries, want the %d of one control; texts=%q", tc.route, got, backEntries, tester.Texts())
		}
	}
}

// The Diff/Commit sidebar is a Git navigator, not the Workspace tree: like
// every inner view it ends in the one way back to the Terminal, in place of
// the Workspace switch.
func TestDiffSidebarFootIsBackToTerminal(t *testing.T) {
	const height = 800
	s := gitSurfaceTestShell(t, t.TempDir())
	s.loading = false
	s.showSurface(WorkspaceSurfaceDiff)
	tester := ui.NewTester(s.View, 1200, height)
	tester.Frame()
	back, ok := tester.Find("Back")
	if !ok {
		t.Fatalf("Diff sidebar missing Back; texts=%q", tester.Texts())
	}
	if back.X > sidebarWidth || back.Y < height-60 {
		t.Fatalf("back control at %+v, want the sidebar foot", back)
	}
	if tester.HasText("Choose Session") {
		t.Fatalf("Diff sidebar kept the Workspace switch; texts=%q", tester.Texts())
	}
	if err := tester.Click("Back"); err != nil {
		t.Fatal(err)
	}
	if s.surface.current() != WorkspaceSurfaceTerminal || s.router.Path() != routeWorkspace {
		t.Fatalf("back landed on surface %v at %s, want the workspace Terminal", s.surface.current(), s.router.Path())
	}
}

// TestInnerPageSidebarPinsPageNavAndBack pins the inner-page style: on
// Settings and History the app sidebar swaps the runtime navigator for
// the page's own navigation over a Back control, and Back returns to the
// workspace sidebar with its Terminal view.
func TestInnerPageSidebarPinsPageNavAndBack(t *testing.T) {
	s := NewShell()
	s.loading = false

	for _, route := range []string{"/settings/general", "/history", "/history-projects"} {
		s.router.Replace(route)
		tester := ui.NewTester(s.View, 1200, 800)
		if _, ok := tester.Find("Back"); !ok {
			t.Fatalf("%s: sidebar missing the back control; texts=%q", route, tester.Texts())
		}
		if err := tester.Click("Back"); err != nil {
			t.Fatalf("%s: back click failed: %v", route, err)
		}
		if got := s.router.Path(); got != "/workspace" {
			t.Fatalf("%s: back route = %q, want /workspace", route, got)
		}
	}
}

// TestSettingsNavLivesInAppSidebar pins the Settings inner-page sidebar: the
// section navigation renders in the app sidebar (no page-internal column)
// and switching sections keeps the nested route.
func TestSettingsNavLivesInAppSidebar(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.router.Replace("/settings/general")

	tester := ui.NewTester(s.View, 1200, 800)
	for _, want := range []string{"Settings", "General", "Terminal", "Agents", "Diagnostics"} {
		if !tester.HasText(want) {
			t.Fatalf("settings sidebar missing %q; texts=%q", want, tester.Texts())
		}
	}
	if err := tester.Click("Terminal"); err != nil {
		t.Fatal(err)
	}
	if got := s.router.Path(); got != "/settings/terminal" {
		t.Fatalf("section click route = %q", got)
	}
	tester.Frame()
	if !tester.HasText("Font family") {
		t.Fatalf("terminal section did not render; texts=%q", tester.Texts())
	}
}

func TestSidebarPinSectionRendersAndRoutes(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.activeInstance = "default"
	s.projection = herdr.Projection{
		Panes: []herdr.Pane{{ID: "p9", TerminalID: "term-9", Label: "Watch tower", TabID: "t1"}},
	}
	s.settings.Workbench.PinnedPanes = map[string][]string{"default": {"p9"}}

	tester := ui.NewTester(s.View, 1200, 800)
	// The Pin section replaces Recent (2026-10-05): it renders only when a
	// live Pane is pinned, with the Pane label as the row.
	for _, want := range []string{"Pin", "Watch tower"} {
		if !tester.HasText(want) {
			t.Fatalf("sidebar missing %q; texts=%q", want, tester.Texts())
		}
	}
	if err := tester.Click("Watch tower"); err != nil {
		t.Fatal(err)
	}
	if s.selectedPaneID != "p9" {
		t.Fatalf("pin click selected pane = %q", s.selectedPaneID)
	}

	// Unpinning through the settings service removes the section.
	s.togglePanePin("p9")
	tester.Frame()
	if tester.HasText("Watch tower") {
		t.Fatalf("pinned row still rendered after unpin; texts=%q", tester.Texts())
	}
	if len(s.settings.Workbench.PinnedPanes["default"]) != 0 {
		t.Fatalf("pins persisted after unpin: %q", s.settings.Workbench.PinnedPanes)
	}
}

func TestProjectAndTabExpansionArePresentationState(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.activeInstance = "default"
	s.projection = herdr.Projection{
		FocusedProjectID: "w1",
		FocusedTabID:     "t1",
		FocusedPaneID:    "p1",
		Projects: []herdr.Project{
			{ID: "w1", Label: "Project A", TabCount: 1},
			{ID: "w2", Label: "Project B", TabCount: 1},
		},
		Tabs: []herdr.Tab{
			{ID: "t1", ProjectID: "w1", Label: "Tab A", PaneCount: 2},
			{ID: "t2", ProjectID: "w2", Label: "Tab B", PaneCount: 1},
		},
		Panes: []herdr.Pane{
			{ID: "p1", ProjectID: "w1", TabID: "t1", Label: "Pane A1"},
			{ID: "p2", ProjectID: "w1", TabID: "t1", Label: "Pane A2"},
			{ID: "p3", ProjectID: "w2", TabID: "t2", Label: "Pane B"},
		},
	}
	s.selectedProjectID = "w1"
	s.selectedTabID = "t1"
	s.selectedPaneID = "p1"

	tester := ui.NewTester(s.View, 1200, 800)
	if !tester.HasText("Tab A") || !tester.HasText("Pane A1") {
		t.Fatalf("focused tree should start expanded; texts=%q", tester.Texts())
	}

	s.setTabExpanded("t1", false)
	tester.Frame()
	if tester.HasText("Pane A2") {
		t.Fatalf("focused tab should collapse without changing runtime focus")
	}
	if s.projection.FocusedTabID != "t1" {
		t.Fatalf("collapse changed runtime projection focus: %q", s.projection.FocusedTabID)
	}

	s.setTabExpanded("t1", true)
	tester.Frame()
	if !tester.HasText("Pane A2") {
		t.Fatalf("tab should expand again")
	}

	s.setProjectExpanded("w2", true)
	tester.Frame()
	if !tester.HasText("Tab A") || !tester.HasText("Tab B") {
		t.Fatalf("multiple projects should remain expanded; texts=%q", tester.Texts())
	}
	if s.projection.FocusedProjectID != "w1" {
		t.Fatalf("presentation expansion changed runtime project focus: %q", s.projection.FocusedProjectID)
	}

	s.setProjectExpanded("w1", false)
	tester.Frame()
	if tester.HasText("Pane A2") {
		t.Fatalf("focused project should collapse from presentation state")
	}
	if !tester.HasText("Tab B") {
		t.Fatalf("collapsing Project A should not collapse Project B")
	}
}

// sidebarViewTabLabels are the four view tabs' stable accessible names.
var sidebarViewTabLabels = []string{"Changes view", "History view", "Files view", "Hosts view"}

// TestSidebarViewTabsSitAtTheTopInEqualShares pins the 2026-10-14 move of the
// four view buttons out of the title bar: on every route they are the first
// row under the traffic-light band, equal in width, on one line, at the same
// place whichever page is open — and they are no longer in the title bar.
func TestSidebarViewTabsSitAtTheTopInEqualShares(t *testing.T) {
	setups := []headerRoute{{"terminal", "", func(t *testing.T) *Shell {
		s := NewShell()
		s.loading = false
		return s
	}}}
	setups = append(setups, headerRoutes()...)
	var baseline map[string]ui.Rect
	for _, route := range setups {
		tester := ui.NewTester(route.setup(t).View, 1200, 800)
		settleFrames(tester, 3)
		got := map[string]ui.Rect{}
		for _, label := range sidebarViewTabLabels {
			if n := countTextExact(tester.Texts(), label); n != 1 {
				t.Fatalf("%s: %d %q controls, want exactly one; texts=%q", route.name, n, label, tester.Texts())
			}
			rect, _ := tester.Find(label)
			got[label] = rect
			if rect.X+rect.W > sidebarWidth {
				t.Errorf("%s: %q at x=%.0f–%.0f is outside the sidebar", route.name, label, rect.X, rect.X+rect.W)
			}
			if rect.Y < 38 || rect.Y+rect.H > 38+gorexIconBtn+16 {
				t.Errorf("%s: %q at y=%.0f–%.0f is not the sidebar's first row under the band", route.name, label, rect.Y, rect.Y+rect.H)
			}
		}
		first := got[sidebarViewTabLabels[0]]
		for _, label := range sidebarViewTabLabels[1:] {
			r := got[label]
			if math.Abs(float64(r.W-first.W)) > 0.5 || math.Abs(float64(r.Y-first.Y)) > 0.5 {
				t.Errorf("%s: %q is %.1f×%.1f at y=%.1f, %q is %.1f×%.1f at y=%.1f — tabs must share width and line",
					route.name, label, r.W, r.H, r.Y, sidebarViewTabLabels[0], first.W, first.H, first.Y)
			}
		}
		if first.W <= gorexIconBtn {
			t.Errorf("%s: a tab is %.1f wide — equal shares of the row, not fixed squares", route.name, first.W)
		}
		if baseline == nil {
			baseline = got
			continue
		}
		for label, r := range got {
			if b := baseline[label]; math.Abs(float64(r.X-b.X)) > 0.5 || math.Abs(float64(r.Y-b.Y)) > 0.5 {
				t.Errorf("%s: %q moved to (%.1f, %.1f) from (%.1f, %.1f) — entering a page must not move the tabs",
					route.name, label, r.X, r.Y, b.X, b.Y)
			}
		}
	}
}

// TestSidebarViewTabsOpenAndLeaveTheirPage pins the tabs' contract, unchanged
// from the title-bar buttons they replaced: a tab opens its page and pressing
// the lit one leaves for the Terminal.
func TestSidebarViewTabsOpenAndLeaveTheirPage(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.hosts.loaded = true // never read the developer's real ~/.ssh/config
	tester := ui.NewTester(s.View, 1200, 800)
	settleFrames(tester, 2)
	for _, tc := range []struct{ label, route string }{
		{"Hosts view", routeHosts},
		{"History view", routeHistory},
	} {
		if err := tester.Click(tc.label); err != nil {
			t.Fatalf("%s: %v", tc.label, err)
		}
		settleFrames(tester, 2)
		if got := s.router.Path(); got != tc.route {
			t.Fatalf("%s opened %q, want %q", tc.label, got, tc.route)
		}
		if err := tester.Click(tc.label); err != nil {
			t.Fatalf("%s (leave): %v", tc.label, err)
		}
		settleFrames(tester, 2)
		if got := s.router.Path(); got != routeWorkspace || s.surface.current() != WorkspaceSurfaceTerminal {
			t.Fatalf("%s again went to %q/%v, want the Terminal", tc.label, got, s.surface.current())
		}
	}
}
