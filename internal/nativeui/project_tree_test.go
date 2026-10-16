package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// TestSidebarCollapsedPaneCount pins the pane tally (2026-10-07, widened
// 2026-10-15 user request): a nesting row shows tiny "N panes" digits at its
// trailing edge when more than one pane runs beneath it — the project row
// whether folded or expanded, a tab row only while folded — and the digits
// step aside under the pointer.
func TestSidebarCollapsedPaneCount(t *testing.T) {
	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo"}},
		Tabs:     []herdr.Tab{{ID: "t1", Label: "main", ProjectID: "w1"}},
		Panes: []herdr.Pane{
			{ID: "p1", TabID: "t1", TerminalID: "term-1", Label: "one"},
			{ID: "p2", TabID: "t1", TerminalID: "term-2", Label: "two"},
			{ID: "p3", TabID: "t1", TerminalID: "term-3", Label: "three"},
		},
	}
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	// Nothing is selected, so the project starts collapsed: the tally
	// shows on the project row.
	if !tester.HasText("3 panes") {
		t.Fatalf("collapsed project must show the pane tally; texts=%q", tester.Texts())
	}

	// Expanding the project exposes its collapsed tab, which carries its
	// own tally; both rows read "3 panes" through their labels.
	shell.setProjectExpanded("w1", true)
	tester.Frame()
	if !tester.HasText("3 panes") {
		t.Fatalf("collapsed tab must show the pane tally; texts=%q", tester.Texts())
	}

	// Expanding the tab drops its own tally (the panes are visible); the
	// project row keeps its tally even fully expanded (2026-10-15 user
	// request): the load stays readable at the tree's top.
	shell.setTabExpanded("t1", true)
	tester.Frame()
	if !tester.HasText("3 panes") {
		t.Fatalf("expanded project must keep the pane tally; texts=%q", tester.Texts())
	}

	// Under the pointer the tally steps aside for the hover controls:
	// hovering the project row hides its digits, and a single-pane tab
	// never shows one, so none remains anywhere.
	shell2 := NewShell()
	shell2.activeInstance = "inst-1"
	shell2.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo"}},
		Tabs: []herdr.Tab{
			{ID: "t1", Label: "main", ProjectID: "w1"},
			{ID: "t2", Label: "side", ProjectID: "w1"},
		},
		Panes: []herdr.Pane{
			{ID: "p1", TabID: "t1", TerminalID: "term-1", Label: "one"},
			{ID: "p2", TabID: "t2", TerminalID: "term-2", Label: "two"},
		},
	}
	tester2 := ui.NewTester(shell2.View, 1200, 800)
	shell2.setProjectExpanded("w1", true)
	tester2.Frame()
	if !tester2.HasText("2 panes") {
		t.Fatalf("expanded project with two panes must show its tally; texts=%q", tester2.Texts())
	}
	rect, ok := tester2.Find("demo")
	if !ok {
		t.Fatal("project row not found")
	}
	tester2.Move(rect.X+rect.W/2, rect.Y+rect.H/2)
	tester2.Frame()
	for _, text := range tester2.Texts() {
		if text == "2 panes" {
			t.Fatalf("the tally must hide under the pointer; texts=%q", tester2.Texts())
		}
	}
}
