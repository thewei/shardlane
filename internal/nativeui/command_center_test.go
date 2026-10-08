package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/commandcenter"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// commandCenterShell builds a shell with two projects/tabs/panes and two
// live agents — enough snapshot for navigation and agent actions.
func commandCenterShell(t *testing.T) *Shell {
	t.Helper()
	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo"}},
		Tabs:     []herdr.Tab{{ID: "t1", Label: "main", ProjectID: "w1"}},
		Panes:    []herdr.Pane{{ID: "p1", TerminalID: "term-1", Label: "editor"}},
		Agents:   []herdr.Agent{{TerminalID: "term-1", PaneID: "p1", Status: "working", Name: "Scout"}},
	}
	shell.reconcileWorkbench()
	return shell
}

// TestBuildCommandCenterActionsFromSnapshot pins the §P2 registry build:
// actions derive from the application snapshot (navigation, agents, app
// surfaces), the palette index is built with zero IO, and unavailable panes
// are gated by snapshot facts.
func TestBuildCommandCenterActionsFromSnapshot(t *testing.T) {
	shell := commandCenterShell(t)
	actions := shell.buildCommandCenterActions()
	index := commandcenter.Build(actions)

	byID := map[string]commandcenter.Action{}
	for _, action := range actions {
		byID[action.ID] = action
	}
	for _, id := range []string{
		"nav.project.w1", "nav.tab.t1", "nav.pane.p1",
		"agent.open.term-1", "app.history", "app.settings",
	} {
		if _, ok := byID[id]; !ok {
			t.Fatalf("registry missing %q", id)
		}
	}
	if !byID["nav.pane.p1"].Available {
		t.Fatal("an attachable pane action must be available")
	}

	// Navigation scope is strict; All includes agents and app surfaces.
	if got := len(index.Query("", commandcenter.ScopeNavigation)); got != 3 {
		t.Fatalf("navigation results = %d, want 3", got)
	}
	if got := len(index.Query("", commandcenter.ScopeAll)); got < 8 {
		t.Fatalf("all results = %d, want >= 8", got)
	}
}

// TestCommandCenterOverlayQueryAndExecute pins the §P1 palette flow: open
// resets state, the query narrows results, and executing a pane result
// revalidates the typed target then navigates client-locally; a stale
// target surfaces recoverable text instead of acting.
func TestCommandCenterOverlayQueryAndExecute(t *testing.T) {
	shell := commandCenterShell(t)

	shell.openCommandCenter(commandcenter.ScopeAll)
	if !shell.commandCenterOpen || shell.commandCenterScope != commandcenter.ScopeAll {
		t.Fatalf("open state = %v/%v", shell.commandCenterOpen, shell.commandCenterScope)
	}
	results := shell.commandCenterIndexCache.Query("pane", commandcenter.ScopeAll)
	if len(results) == 0 || results[0].Action.Kind != commandcenter.TargetPane {
		t.Fatalf("pane query = %+v", results)
	}

	// Execute the pane result: revalidated against the projection, then
	// client-local navigation.
	shell.executeCommandCenterResult(results[0])
	if shell.commandCenterOpen {
		t.Fatal("executing must close the palette")
	}
	if shell.selectedPaneID != "p1" || shell.router.Path() != routeWorkspace {
		t.Fatalf("pane execution landed on %q / %q", shell.selectedPaneID, shell.router.Path())
	}

	// A stale pane target fails closed with recoverable text.
	shell.openCommandCenter(commandcenter.ScopeAll)
	shell.executeCommandCenterResult(commandcenter.Result{Action: commandcenter.Action{
		ID: "nav.pane.pX", Title: "Pane: gone", Kind: commandcenter.TargetPane, TargetID: "pX", Available: true,
	}})
	if shell.selectedPaneID == "pX" {
		t.Fatal("stale pane target must not be selected")
	}
	if shell.status == "" {
		t.Fatal("stale target must surface recoverable text")
	}
}

// TestGroupCommandCenterResults pins the list grouping: consecutive equal
// sections merge into one titled block in encounter order.
func TestGroupCommandCenterResults(t *testing.T) {
	groups := groupCommandCenterResults([]commandcenter.Result{
		{Action: commandcenter.Action{ID: "a1", Section: "Agents"}},
		{Action: commandcenter.Action{ID: "n1", Section: "Navigation"}},
		{Action: commandcenter.Action{ID: "n2", Section: "Navigation"}},
		{Action: commandcenter.Action{ID: "app", Section: "App"}},
	})
	if len(groups) != 3 {
		t.Fatalf("groups = %d, want 3", len(groups))
	}
	want := []struct {
		section string
		n       int
	}{{"Agents", 1}, {"Navigation", 2}, {"App", 1}}
	for i, w := range want {
		if groups[i].Section != w.section || len(groups[i].Results) != w.n {
			t.Fatalf("group %d = %s/%d, want %s/%d", i, groups[i].Section, len(groups[i].Results), w.section, w.n)
		}
	}
}

// TestCommandCenterKeys pins the in-modal keys and the keyboard-first
// open: the query field takes the focus the frame the palette opens, the
// field's arrow claims move the selection with wrap-around (Home/End
// jump), Enter commits through the focused field, Escape closes — and the
// palette-open shortcuts (Cmd+Shift+P / Cmd+P) work from the shell chrome.
func TestCommandCenterKeys(t *testing.T) {
	shell := commandCenterShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)

	// Cmd+Shift+P opens the All palette; Cmd+P opens Navigation.
	tester.Key(ui.Super|ui.Shift, ui.KeyP)
	tester.Frame()
	if !shell.commandCenterOpen || shell.commandCenterScope != commandcenter.ScopeAll {
		t.Fatalf("palette open = %v scope = %v", shell.commandCenterOpen, shell.commandCenterScope)
	}
	// Keyboard-first: the query field holds the focus from open.
	if !shell.commandCenterQueryFocused {
		t.Fatal("the query field must take the focus when the palette opens")
	}
	results := shell.commandCenterIndexCache.Query(shell.commandCenterQuery, shell.commandCenterScope)
	if len(results) < 2 {
		t.Fatalf("need at least two results, got %d", len(results))
	}

	// Arrow down moves the selection; PgDn jumps toward the last result,
	// Down wraps past it to the first, Up wraps back to the last. (Home/
	// End stay with the caret: the focused field takes them, as any text
	// input does.)
	tester.Key(0, ui.KeyDown)
	tester.Frame()
	if shell.commandCenterSelected != 1 {
		t.Fatalf("selection = %d after arrow down", shell.commandCenterSelected)
	}
	tester.Key(0, ui.KeyPageDown)
	tester.Frame()
	if shell.commandCenterSelected != len(results)-1 {
		t.Fatalf("selection = %d after PgDn, want %d", shell.commandCenterSelected, len(results)-1)
	}
	tester.Key(0, ui.KeyDown)
	tester.Frame()
	if shell.commandCenterSelected != 0 {
		t.Fatalf("selection = %d after wrapped arrow down, want 0", shell.commandCenterSelected)
	}
	tester.Key(0, ui.KeyUp)
	tester.Frame()
	if shell.commandCenterSelected != len(results)-1 {
		t.Fatalf("selection = %d after wrapped arrow up, want %d", shell.commandCenterSelected, len(results)-1)
	}

	// Enter commits the selection through the focused field itself and
	// closes the palette (WIX-021: keyboard-first, no pointer needed).
	shell.commandCenterSelected = 0
	tester.Key(0, ui.KeyEnter)
	tester.Frame()
	if shell.commandCenterOpen {
		t.Fatal("enter must commit and close")
	}
	// mygo 0.2.9 keeps the closed modal's scope through the frame right
	// after the close, so window shortcut registration is suppressed for
	// that frame and the first shortcut pressed in it is dropped; one more
	// settle frame restores the registrations. (Upstream regression — the
	// in-app first shortcut after closing a ui.Modal can be swallowed.)
	tester.Frame()

	// Cmd+P opens the strictly-scoped Navigation palette; Escape closes.
	tester.Key(ui.Super, ui.KeyP)
	tester.Frame()
	if !shell.commandCenterOpen || shell.commandCenterScope != commandcenter.ScopeNavigation {
		t.Fatalf("navigation palette = %v/%v", shell.commandCenterOpen, shell.commandCenterScope)
	}
	if results := shell.commandCenterIndexCache.Query("", commandcenter.ScopeNavigation); len(results) != 3 {
		t.Fatalf("navigation results = %d, want 3", len(results))
	}
	tester.Key(0, ui.KeyEscape)
	tester.Frame()
	if shell.commandCenterOpen {
		t.Fatal("escape must close the palette")
	}
}
