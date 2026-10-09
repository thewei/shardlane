package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// TestInnerPageSidebarPinsPageNavAndBack pins the inner-page style: on
// Settings and History the app sidebar swaps the runtime navigator for
// the page's own navigation under a Back control, and Back returns to the
// workspace sidebar with its Terminal view.
func TestInnerPageSidebarPinsPageNavAndBack(t *testing.T) {
	s := NewShell()
	s.loading = false

	for _, route := range []string{"/settings/general", "/history", "/history-projects"} {
		s.router.Replace(route)
		tester := ui.NewTester(s.View, 1200, 800)
		if !tester.HasText("Back to Terminal") {
			t.Fatalf("%s: sidebar missing the back control; texts=%q", route, tester.Texts())
		}
		if err := tester.Click("Back to Terminal"); err != nil {
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
	for _, want := range []string{"Settings", "General", "Terminal", "Providers", "Runtime", "Diagnostics"} {
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
