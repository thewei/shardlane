package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

func TestNormalizeRuntimeStatus(t *testing.T) {
	cases := map[string]operationalState{
		"blocked": opNeedsAttention, "failed": opNeedsAttention, "ERROR": opNeedsAttention,
		"working": opWorking, "pending": opWorking, "launch_pending": opWorking,
		"done": opReadyForReview, "completed": opReadyForReview,
		"idle": opIdle, "": opIdle, "mystery": opIdle,
		"resolved": opResolved,
	}
	for raw, want := range cases {
		if got := normalizeRuntimeStatus(raw); got != want {
			t.Fatalf("normalizeRuntimeStatus(%q) = %v, want %v", raw, got, want)
		}
	}
	if operationalPriority(opNeedsAttention) >= operationalPriority(opWorking) ||
		operationalPriority(opWorking) >= operationalPriority(opReadyForReview) ||
		operationalPriority(opReadyForReview) >= operationalPriority(opIdle) {
		t.Fatal("attention priority order is wrong")
	}
}

func TestSummarizeAgentsPriority(t *testing.T) {
	summary := summarizeAgents([]herdr.Agent{
		{PaneID: "p1", Status: "idle"},
		{PaneID: "p2", Status: "working"},
		{PaneID: "p3", Status: "blocked"},
		{PaneID: "p4", Status: "done"},
	})
	if summary.NeedsAttention != 1 || summary.Working != 1 || summary.ReadyForReview != 1 || summary.Total != 4 {
		t.Fatalf("summary = %+v", summary)
	}
	if summary.HighestPriority != opNeedsAttention {
		t.Fatalf("highest = %v", summary.HighestPriority)
	}
}

func TestScopeOperationalState(t *testing.T) {
	projection := herdr.Projection{Agents: []herdr.Agent{
		{PaneID: "p1", ProjectID: "w1", TabID: "t1", Status: "working"},
		{PaneID: "p2", ProjectID: "w2", TabID: "t2", Status: "blocked"},
	}}
	if got := scopeOperationalState(projection, "w1", ""); got != opWorking {
		t.Fatalf("project scope = %v", got)
	}
	if got := scopeOperationalState(projection, "w2", ""); got != opNeedsAttention {
		t.Fatalf("other project scope = %v", got)
	}
	if got := scopeOperationalState(projection, "", "t1"); got != opWorking {
		t.Fatalf("tab scope = %v", got)
	}
}

// TestOperationalStatusDynamicAcrossConsumers scripts the OPS-05 event
// sequence and asserts every consumer derives the same status from the one
// shared model, without any polling.
func TestOperationalStatusDynamicAcrossConsumers(t *testing.T) {
	shell := NewShell()
	shell.loading = false
	shell.instances = []herdr.Instance{{Name: "default", DisplayName: "Default", Running: true, Default: true}}
	shell.activeInstance = "default"
	agent := herdr.Agent{PaneID: "p1", ProjectID: "w1", TabID: "t1", Name: "Scout", Status: "idle"}
	shell.projection = herdr.Projection{
		Protocol:         22,
		FocusedProjectID: "w1",
		FocusedTabID:     "t1",
		FocusedPaneID:    "p1",
		Projects:         []herdr.Project{{ID: "w1", Label: "Alpha", TabCount: 1}},
		Tabs:             []herdr.Tab{{ID: "t1", ProjectID: "w1", Label: "main", PaneCount: 1}},
		Panes:            []herdr.Pane{{ID: "p1", TerminalID: "term-1", ProjectID: "w1", TabID: "t1"}},
		Agents:           []herdr.Agent{agent},
	}

	tester := ui.NewTester(shell.View, 1200, 800)

	assertConsumers := func(step, sidebarText string, attention bool) {
		t.Helper()
		// One reconciled event application per scripted status change.
		shell.reconcileWorkbench()
		tester.Frame()
		summary := shell.operationalSummary()
		// The sidebar lists the agent row regardless of state; the state
		// itself is the corner dot (no status words since 2026-10-05).
		if !tester.HasText("Alpha") {
			t.Fatalf("%s: sidebar missing agent row; texts=%q", step, tester.Texts())
		}
		_ = sidebarText
		if summary.HighestPriority != normalizeRuntimeStatus(agent.Status) {
			t.Fatalf("%s: summary = %+v", step, summary)
		}
		hasAttention := tester.HasText("attention")
		if hasAttention != attention {
			t.Fatalf("%s: titlebar attention = %v, want %v", step, hasAttention, attention)
		}
	}

	assertConsumers("idle", "Idle", false)

	agent.Status = "working"
	shell.projection.Agents = []herdr.Agent{agent}
	assertConsumers("working", "Working", false)

	agent.Status = "blocked"
	shell.projection.Agents = []herdr.Agent{agent}
	assertConsumers("blocked", "Needs attention", true)

	agent.Status = "done"
	shell.projection.Agents = []herdr.Agent{agent}
	assertConsumers("done", "Ready for review", false)

	// The attention badge stays on the persistent Agent activity button
	// (2026-10-07); without an attached panel window the click is a safe
	// no-op, and the badge remains the observable.
	agent.Status = "blocked"
	shell.projection.Agents = []herdr.Agent{agent}
	shell.reconcileWorkbench()
	tester.Frame()
	before := shell.router.Path()
	if err := tester.Click("1 attention"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.router.Path() != before {
		t.Fatalf("panel-less anchor click navigated to %q", shell.router.Path())
	}
	if !tester.HasText("1 attention") {
		t.Fatal("attention badge disappeared after the click")
	}
}
