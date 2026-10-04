package nativeui

import (
	"testing"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/wh-studio/herdr-client/next/internal/commandcenter"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

func TestLocalSelectionIgnoresExternalHerdrFocusChurn(t *testing.T) {
	s := NewShell()
	s.router.Replace(routeHistory) // keep this test presentation-only; no terminal attach.
	s.projection = selectionFixture("w1", "t1", "p1")
	s.adoptRuntimeSelection(s.projection)

	s.selectTab("t2")
	s.selectPane("p2")
	if s.selectedProjectID != "w2" || s.selectedTabID != "t2" || s.selectedPaneID != "p2" {
		t.Fatalf("local selection = %q/%q/%q", s.selectedProjectID, s.selectedTabID, s.selectedPaneID)
	}

	// A different Herdr client changes global focus back to Project/Tab/Pane A.
	next := selectionFixture("w1", "t1", "p1")
	s.applyProjection(next, false)

	if s.selectedProjectID != "w2" || s.selectedTabID != "t2" || s.selectedPaneID != "p2" {
		t.Fatalf("external runtime focus stole local selection: %q/%q/%q",
			s.selectedProjectID, s.selectedTabID, s.selectedPaneID)
	}
}

func TestLocalSelectionFallsBackWhenSelectedRuntimeObjectDisappears(t *testing.T) {
	s := NewShell()
	s.router.Replace(routeHistory)
	s.projection = selectionFixture("w1", "t1", "p1")
	s.selectedProjectID = "w2"
	s.selectedTabID = "t2"
	s.selectedPaneID = "p2"

	next := selectionFixture("w1", "t1", "p1")
	next.Projects = next.Projects[:1]
	next.Tabs = next.Tabs[:1]
	next.Panes = next.Panes[:1]
	next.Layouts = next.Layouts[:1]
	s.applyProjection(next, false)

	if s.selectedProjectID != "w1" || s.selectedTabID != "t1" || s.selectedPaneID != "p1" {
		t.Fatalf("fallback selection = %q/%q/%q",
			s.selectedProjectID, s.selectedTabID, s.selectedPaneID)
	}
}

func TestVisiblePaneGeometryUsesLocalSelectedTabNotRuntimeFocus(t *testing.T) {
	projection := selectionFixture("w1", "t1", "p1")
	got := visiblePaneGeometry(projection, "t2", "p2")
	if len(got) != 1 {
		t.Fatalf("visible panes = %d, want 1 from selected Tab B", len(got))
	}
	if _, ok := got["term2"]; !ok {
		t.Fatalf("selected Tab B terminal missing: %#v", got)
	}
	if _, ok := got["term1"]; ok {
		t.Fatalf("runtime-focused Tab A leaked into local selection: %#v", got)
	}
	if !got["term2"].focused {
		t.Fatalf("selected pane should own presentation focus")
	}
}

func TestCommandCenterResultChangesLocalSelectionWithoutRuntimeMutation(t *testing.T) {
	s := NewShell()
	s.activeInstance = "default"
	s.router.Replace(routeHistory)
	s.projection = selectionFixture("w1", "t1", "p1")
	s.adoptRuntimeSelection(s.projection)

	s.executeCommandCenterResult(commandcenter.Result{Action: commandcenter.Action{
		Kind: commandcenter.TargetTab, TargetID: "t2",
	}})
	if s.router.Path() != routeWorkspace {
		t.Fatalf("route = %q", s.router.Path())
	}
	if s.selectedProjectID != "w2" || s.selectedTabID != "t2" {
		t.Fatalf("search selection = %q/%q", s.selectedProjectID, s.selectedTabID)
	}
	// No real Window is attached, therefore a local selection cannot trigger
	// any Herdr RPC/terminal side effect. This test specifically proves the
	// navigation path is presentation-local.
}

func selectionFixture(focusedProject, focusedTab, focusedPane string) herdr.Projection {
	return herdr.Projection{
		Protocol:         22,
		FocusedProjectID: focusedProject,
		FocusedTabID:     focusedTab,
		FocusedPaneID:    focusedPane,
		Projects: []herdr.Project{
			{ID: "w1", Label: "Project A", ActiveTabID: "t1", TabCount: 1},
			{ID: "w2", Label: "Project B", ActiveTabID: "t2", TabCount: 1},
		},
		Tabs: []herdr.Tab{
			{ID: "t1", ProjectID: "w1", Label: "Tab A", PaneCount: 1},
			{ID: "t2", ProjectID: "w2", Label: "Tab B", PaneCount: 1},
		},
		Panes: []herdr.Pane{
			{ID: "p1", TerminalID: "term1", ProjectID: "w1", TabID: "t1", Label: "Pane A"},
			{ID: "p2", TerminalID: "term2", ProjectID: "w2", TabID: "t2", Label: "Pane B"},
		},
		Layouts: []herdr.Layout{
			{
				ProjectID: "w1", TabID: "t1", FocusedPaneID: "p1",
				Area:  herdr.LayoutRect{Width: 100, Height: 40},
				Panes: []herdr.LayoutPane{{PaneID: "p1", Focused: true, Rect: herdr.LayoutRect{Width: 100, Height: 40}}},
			},
			{
				ProjectID: "w2", TabID: "t2", FocusedPaneID: "p2",
				Area:  herdr.LayoutRect{Width: 100, Height: 40},
				Panes: []herdr.LayoutPane{{PaneID: "p2", Focused: true, Rect: herdr.LayoutRect{Width: 100, Height: 40}}},
			},
		},
	}
}

// Compile-time guards: selection tests should never need a real MyGo Window.
// Keep imports of the UI/window types exercised by neighboring headless tests.
var (
	_ *mygo.Window
	_ *ui.Context
)
