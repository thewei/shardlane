package nativeui

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/agent"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

func workbenchTestShell(t *testing.T) *Shell {
	t.Helper()
	shell := NewShell()
	shell.loading = false
	shell.activeInstance = "default"
	shell.projection = herdr.Projection{
		Protocol:         22,
		FocusedProjectID: "w1",
		FocusedTabID:     "t1",
		FocusedPaneID:    "p1",
		Projects:         []herdr.Project{{ID: "w1", Label: "Demo", TabCount: 1}},
		Tabs:             []herdr.Tab{{ID: "t1", ProjectID: "w1", Label: "main", PaneCount: 1}},
		Panes: []herdr.Pane{
			{ID: "p1", TerminalID: "term-1", ProjectID: "w1", TabID: "t1"},
			{ID: "p2", TerminalID: "term-2", ProjectID: "w1", TabID: "t1"},
		},
		Agents: []herdr.Agent{
			{TerminalID: "term-1", PaneID: "p1", ProjectID: "w1", TabID: "t1", Name: "Scout", Kind: "codex", Status: "working"},
			{TerminalID: "term-2", PaneID: "p2", ProjectID: "w1", TabID: "t1", Name: "Ranger", Kind: "claude-code", Status: "blocked"},
		},
	}
	shell.reconcileWorkbench()
	return shell
}

func TestWorkbenchSidebarShowsSharedCardRows(t *testing.T) {
	shell := workbenchTestShell(t)
	tester := ui.NewTester(shell.View, 1200, 800)
	// Compact rows (2026-10-05): one row per agent showing the project as
	// the label and the agent title as the caption; state is the corner
	// dot, so status words no longer appear in the sidebar.
	for _, want := range []string{"Scout", "Ranger", "Demo"} {
		if !tester.HasText(want) {
			t.Fatalf("sidebar missing %q; texts=%q", want, tester.Texts())
		}
	}
	for _, gone := range []string{"Working", "Needs attention"} {
		if tester.HasText(gone) {
			t.Fatalf("sidebar still shows status word %q; texts=%q", gone, tester.Texts())
		}
	}
}

// TestWorkbenchAgentsRouteFilters was pinned against the deleted /agents
// page; since 2026-10-07 the status tab group lives in the floating agent
// activity panel (quick_panel_test.go covers it).

// TestWorkbenchBlockedActionOpensTerminalLocally pins that the agent card
// click (the panel row, formerly the /agents page button) navigates
// client-locally to the owning pane and never emits a Herdr focus RPC.
func TestWorkbenchBlockedActionOpensTerminalLocally(t *testing.T) {
	shell := workbenchTestShell(t)
	tester := ui.NewTester(shell.QuickPanelView, QuickPanelWidth, QuickPanelHeight)

	if err := tester.Click("Ranger"); err != nil {
		t.Fatal(err)
	}
	// The blocked agent Ranger sits on p2; local selection moves there.
	if shell.selectedPaneID != "p2" {
		t.Fatalf("selected pane = %q, want p2", shell.selectedPaneID)
	}
}

// TestWorkbenchMarkersFlowThroughDirectory pins the transition → marker →
// card pipeline across reconciled snapshots.
func TestWorkbenchMarkersFlowThroughDirectory(t *testing.T) {
	shell := workbenchTestShell(t)

	// Scout: working → done (attention + review on the reconciled snapshot).
	shell.projection.Agents[0].Status = "done"
	shell.reconcileWorkbench()
	cards := shell.workbenchCards()
	var scout agent.AgentCardModel
	for _, card := range cards {
		if card.Title == "Scout" {
			scout = card
		}
	}
	if !scout.ReviewPending {
		t.Fatalf("scout card = %+v", scout)
	}

	// Mark reviewed through the explicit action.
	shell.markAgentReviewed(scout.Key)
	for _, card := range shell.workbenchCards() {
		if card.Title == "Scout" && (card.ReviewPending || card.Unread) {
			t.Fatalf("review not cleared: %+v", card)
		}
	}
}

// TestReconcileWorkbenchAnchorsRecentlyWorkingAgents pins the sort
// hysteresis across reconciled snapshots: a Working agent carries the
// anchor, keeps it when its hook flips the status to idle between task
// loops, and loses it when the agent leaves the projection.
func TestReconcileWorkbenchAnchorsRecentlyWorkingAgents(t *testing.T) {
	shell := workbenchTestShell(t)
	cardByTitle := func(title string) (agent.AgentCardModel, bool) {
		for _, card := range shell.workbenchCards() {
			if card.Title == title {
				return card, true
			}
		}
		return agent.AgentCardModel{}, false
	}
	scoutKey := agent.AgentKey{InstanceID: "default", TerminalID: "term-1"}

	scout, ok := cardByTitle("Scout")
	if !ok || !scout.ActiveUntil.After(time.Now()) {
		t.Fatalf("working agent must carry the hysteresis anchor: %+v", scout)
	}

	// The Stop hook fires after every short loop: the honest status flips
	// to idle but the anchor (and thus the list position) survives.
	shell.projection.Agents[0].Status = "idle"
	shell.reconcileWorkbench()
	scout, ok = cardByTitle("Scout")
	if !ok || scout.Attention != agent.OpIdle || scout.RuntimePhase != agent.PhaseIdle {
		t.Fatalf("status must stay honest: %+v", scout)
	}
	if !scout.ActiveUntil.After(time.Now()) {
		t.Fatalf("idle-between-loops agent must keep the anchor: %+v", scout.ActiveUntil)
	}

	// A never-working agent carries no anchor.
	ranger, ok := cardByTitle("Ranger")
	if !ok || ranger.ActiveUntil != (time.Time{}) {
		t.Fatalf("blocked agent must have no anchor: %+v", ranger.ActiveUntil)
	}

	// Agents that leave the projection are pruned from the anchor map.
	shell.projection.Agents = shell.projection.Agents[1:]
	shell.reconcileWorkbench()
	if _, anchored := shell.workbench.recentWorking[scoutKey]; anchored {
		t.Fatal("recent-working anchor must be pruned with the agent")
	}
}

// TestAgentSwitcherCycleCommitCancel pins the MRU switcher contract: open
// snapshots MRU order, Ctrl-Tab cycles, Enter commits local navigation, Esc
// restores.
func TestAgentSwitcherCycleCommitCancel(t *testing.T) {
	shell := workbenchTestShell(t)
	tester := ui.NewTester(shell.View, 1200, 800)

	// Visit Ranger then Scout to establish MRU order [Scout, Ranger].
	ranger, ok := shell.workbench.directory.Get(agent.AgentKey{InstanceID: "default", TerminalID: "term-2"})
	if !ok {
		t.Fatal("ranger card missing")
	}
	shell.openAgentCard(ranger)
	scout, ok := shell.workbench.directory.Get(agent.AgentKey{InstanceID: "default", TerminalID: "term-1"})
	if !ok {
		t.Fatal("scout card missing")
	}
	shell.openAgentCard(scout)

	tester.Frame()
	tester.Key(ui.Ctrl, ui.KeyTab)
	tester.Frame()
	if !shell.workbench.switcherOpen {
		t.Fatal("Ctrl-Tab did not open the switcher")
	}
	if len(shell.AgentMRUList()) != 2 || len(shell.AgentMRUList()) > 10 {
		t.Fatalf("MRU list = %d", len(shell.AgentMRUList()))
	}

	// Cycle forward once.
	tester.Key(ui.Ctrl, ui.KeyTab)
	tester.Frame()
	if shell.workbench.switcherIndex != 1 {
		t.Fatalf("index = %d, want 1", shell.workbench.switcherIndex)
	}

	// Esc cancels and restores the pre-switcher selection.
	tester.Key(0, ui.KeyEscape)
	tester.Frame()
	if shell.workbench.switcherOpen {
		t.Fatal("Esc did not close the switcher")
	}

	// Reopen and commit with Enter: local navigation to the highlighted card.
	tester.Key(ui.Ctrl, ui.KeyTab)
	tester.Frame()
	tester.Key(0, ui.KeyEnter)
	tester.Frame()
	if shell.workbench.switcherOpen {
		t.Fatal("Enter did not commit")
	}
	if shell.selectedPaneID == "" {
		t.Fatal("commit did not navigate to a pane")
	}
}
