package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

// actionableTreeShell builds one project, two tabs and five panes whose
// projection order is deliberately the worst case: a plain shell first,
// the blocked agent last.
func actionableTreeShell() *Shell {
	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo"}},
		Tabs: []herdr.Tab{
			{ID: "t-plain", Label: "plain first", ProjectID: "w1"},
			{ID: "t-hot", Label: "hot last", ProjectID: "w1"},
		},
		Panes: []herdr.Pane{
			{ID: "p-plain-a", TabID: "t-plain", TerminalID: "term-plain-a", Label: "shell a"},
			{ID: "p-plain-b", TabID: "t-plain", TerminalID: "term-plain-b", Label: "shell b"},
			{ID: "p-idle", TabID: "t-hot", TerminalID: "term-idle", Label: "idle agent"},
			{ID: "p-working", TabID: "t-hot", TerminalID: "term-working", Label: "working agent"},
			{ID: "p-blocked", TabID: "t-hot", TerminalID: "term-blocked", Label: "blocked agent"},
		},
		Agents: []herdr.Agent{
			{TerminalID: "term-idle", PaneID: "p-idle", Status: "idle"},
			{TerminalID: "term-working", PaneID: "p-working", Status: "working"},
			{TerminalID: "term-blocked", PaneID: "p-blocked", Status: "blocked"},
		},
	}
	return shell
}

// TestPaneNavRank pins the rank mapping: human-facing states (attention
// and ready-for-review) first, then working, then idle agents, and the
// agent-less service/shell panes last.
func TestPaneNavRank(t *testing.T) {
	cases := []struct {
		hasAgent bool
		status   string
		want     int
	}{
		{true, "blocked", 0},
		{true, "failed", 0},
		{true, "done", 0},
		{true, "working", 1},
		{true, "running", 1},
		{true, "idle", 2},
		{false, "", 3},
	}
	for _, tc := range cases {
		hasAgent, state := tc.hasAgent, opUnknown
		if tc.hasAgent {
			state = normalizeRuntimeStatus(tc.status)
		}
		if got := paneNavRank(hasAgent, state); got != tc.want {
			t.Fatalf("paneNavRank(%v, %q) = %d, want %d", tc.hasAgent, tc.status, got, tc.want)
		}
	}
}

// TestSortPanesActionable pins the tree order the user asked for: the
// blocked agent's pane first, then the working one, then idle agents,
// plain panes last — and the projection order preserved inside a rank.
func TestSortPanesActionable(t *testing.T) {
	shell := actionableTreeShell()
	panes := panesForTab(shell.projection, "t-hot")
	shell.sortPanesActionable(panes)
	got := make([]string, 0, len(panes))
	for _, pane := range panes {
		got = append(got, pane.ID)
	}
	want := []string{"p-blocked", "p-working", "p-idle"}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("pane order = %v, want %v", got, want)
		}
	}

	plain := panesForTab(shell.projection, "t-plain")
	shell.sortPanesActionable(plain)
	if plain[0].ID != "p-plain-a" || plain[1].ID != "p-plain-b" {
		t.Fatalf("rank ties must keep projection order, got %v/%v", plain[0].ID, plain[1].ID)
	}
}

// TestSortTabsActionable pins the tab rise: the tab holding the blocked
// agent sorts before the all-plain tab, whatever the projection order.
func TestSortTabsActionable(t *testing.T) {
	shell := actionableTreeShell()
	tabs := tabsForProject(shell.projection, "w1")
	shell.sortTabsActionable(tabs)
	if tabs[0].ID != "t-hot" || tabs[1].ID != "t-plain" {
		t.Fatalf("tab order = %v/%v, want t-hot first", tabs[0].ID, tabs[1].ID)
	}
}

// TestSidebarCollapsedPaneCount pins the collapsed tally (2026-10-07
// user request): a collapsed project/tab row shows tiny "N panes" digits
// at its trailing edge when it hides more than one pane; the digits
// vanish when the row expands and step aside under the pointer.
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

	// Expanding the tab drops its tally (the children are visible); the
	// project row keeps showing nothing once expanded.
	shell.setTabExpanded("t1", true)
	tester.Frame()
	if tester.HasText("3 panes") {
		t.Fatalf("expanded rows must drop the tally; texts=%q", tester.Texts())
	}

	// Under the pointer the tally steps aside for the hover controls.
	shell2 := NewShell()
	shell2.activeInstance = "inst-1"
	shell2.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo"}},
		Tabs:     []herdr.Tab{{ID: "t1", Label: "main", ProjectID: "w1"}},
		Panes: []herdr.Pane{
			{ID: "p1", TabID: "t1", TerminalID: "term-1", Label: "one"},
			{ID: "p2", TabID: "t1", TerminalID: "term-2", Label: "two"},
		},
	}
	tester2 := ui.NewTester(shell2.View, 1200, 800)
	shell2.setProjectExpanded("w1", true)
	tester2.Frame()
	rect, ok := tester2.Find("main")
	if !ok {
		t.Fatal("tab row not found")
	}
	tester2.Move(rect.X+rect.W/2, rect.Y+rect.H/2)
	tester2.Frame()
	for _, text := range tester2.Texts() {
		if text == "2 panes" {
			t.Fatalf("the tally must hide under the pointer; texts=%q", tester2.Texts())
		}
	}
}
