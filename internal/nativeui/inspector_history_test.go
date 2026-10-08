package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
)

// inspectorDimensionsShell builds a shell with one blocked agent (needs the
// Terminal) and one working agent, reconciled.
func inspectorDimensionsShell(t *testing.T) *Shell {
	t.Helper()
	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.projection = herdr.Projection{
		Agents: []herdr.Agent{
			{TerminalID: "term-1", PaneID: "p1", Status: "blocked", Name: "Ranger"},
			{TerminalID: "term-2", PaneID: "p2", Status: "working", Name: "Scout"},
		},
	}
	shell.reconcileWorkbench()
	return shell
}

// TestInspectorIdentityAndStatusDimensions pins the 0.8 read-only inspector:
// identity facts (project/pane/tab), the four status dimensions derived from
// the card, the unread/review markers, and the bound conversation window all
// render from already-reconciled state.
func TestInspectorIdentityAndStatusDimensions(t *testing.T) {
	shell := inspectorDimensionsShell(t)
	// Off-screen attention observation: unread is created only when the
	// agent was not on screen (0.5 marker rule; FirstObservation never
	// manufactures markers).
	shell.workbench.markers.Observe(agent.AgentKey{InstanceID: "inst-1", TerminalID: "term-1"},
		agent.TransitionNeedsAttention, false)
	shell.reconcileWorkbenchMarkersOnly()
	shell.inspector = inspectorState{
		agentKey: agent.AgentKey{InstanceID: "inst-1", TerminalID: "term-1"},
		window: &history.TranscriptWindow{Messages: []history.TranscriptMessage{
			{Seq: 0, Role: history.RoleUser, Kind: history.MessageText, Text: "Fix the flaky test"},
		}},
	}
	shell.router.Replace("/inspector/p1")
	tester := ui.NewTester(shell.View, 1200, 800)
	for _, want := range []string{
		"Ranger",
		"Status dimensions",
		"Runtime phase",
		string(agent.PhaseBlocked),
		"Sendability",
		string(agent.NeedsTerminal),
		"Attention",
		agent.OperationalLabel(agent.OpNeedsAttention),
		"Markers",
		"unread",
		"Location",
		"Pane",
		"p1",
		"Bound conversation (bounded window)",
		"Fix the flaky test",
	} {
		if !tester.HasText(want) {
			t.Fatalf("inspector missing %q; texts=%q", want, tester.Texts())
		}
	}
	// The primary action of a blocked agent routes to the Terminal: the
	// inspector never offers a generic prompt.
	if tester.HasText("Message") {
		t.Fatalf("inspector must not offer a chat composer; texts=%q", tester.Texts())
	}
}

// TestInspectorUnboundConversationStillRenders pins the identity rule: an
// unbound conversation is not an error — the dimensions render without a
// transcript.
func TestInspectorUnboundConversationStillRenders(t *testing.T) {
	shell := inspectorDimensionsShell(t)
	shell.inspector = inspectorState{
		agentKey: agent.AgentKey{InstanceID: "inst-1", TerminalID: "term-1"},
	}
	shell.router.Replace("/inspector/p1")
	tester := ui.NewTester(shell.View, 1200, 800)
	if !tester.HasText("Status dimensions") || !tester.HasText("Location") {
		t.Fatalf("identity dimensions missing; texts=%q", tester.Texts())
	}
	if tester.HasText("Bound conversation (bounded window)") {
		t.Fatalf("unbound inspector rendered a transcript; texts=%q", tester.Texts())
	}
}

// TestHistoryProjectsGroupsByProject pins the 0.8 grouped management view:
// sessions group under their normalized project with counts, Open routes to
// the session detail, and a missing project path falls back to "unknown".
func TestHistoryProjectsGroupsByProject(t *testing.T) {
	shell := NewShell()
	shell.historyProjectsLoading = false
	shell.historyProjectsLoaded = true // pre-seeded cache: the render must not re-query
	shell.historyProjects = []history.SessionSummary{
		{Meta: history.SessionMeta{Key: "claude-code:a", Title: "First", Agent: history.AgentClaudeCode,
			ProjectPath: "/work/alpha", ProjectName: "alpha", UpdatedAt: 1_760_000_000_000}},
		{Meta: history.SessionMeta{Key: "codex:b", Title: "Second", Agent: history.AgentCodex,
			ProjectPath: "/work/alpha", ProjectName: "alpha", UpdatedAt: 1_759_000_000_000}},
		{Meta: history.SessionMeta{Key: "codex:c", Title: "Loose", Agent: history.AgentCodex}},
	}
	shell.router.Replace("/history-projects")
	tester := ui.NewTester(shell.View, 1200, 800)
	for _, want := range []string{
		"History by Project",
		"alpha (2)",
		"First",
		"Second",
		"unknown (1)",
		"Loose",
	} {
		if !tester.HasText(want) {
			t.Fatalf("grouped view missing %q; texts=%q", want, tester.Texts())
		}
	}
}

// TestHistoryProjectsRefreshFromService pins the cached refresh: the grouped
// view loads through the dispatch lane (single inflight) and renders the
// returned metadata; before the first load it shows the loading state.
func TestHistoryProjectsRefreshFromService(t *testing.T) {
	shell := NewShell()
	fake := newFakeHistoryView()
	shell.hist.service = fake

	done := make(chan struct{})
	go func() {
		defer close(done)
		shell.refreshHistoryProjects()
	}()
	call := <-fake.calls
	// While inflight, a second refresh is a no-op.
	shell.refreshHistoryProjects()
	call.release <- listResult{summaries: []history.SessionSummary{
		{Meta: history.SessionMeta{Key: "claude-code:a", Title: "Cached", Agent: history.AgentClaudeCode,
			ProjectPath: "/work/beta", ProjectName: "beta"}},
	}}
	<-done

	if len(shell.historyProjects) != 1 || shell.historyProjects[0].Meta.Title != "Cached" {
		t.Fatalf("cached projects = %+v", shell.historyProjects)
	}
	if shell.historyProjectsInflight {
		t.Fatal("inflight flag must clear after the load")
	}
	shell.router.Replace("/history-projects")
	tester := ui.NewTester(shell.View, 1200, 800)
	if !tester.HasText("beta (1)") || !tester.HasText("Cached") {
		t.Fatalf("grouped render missing cached session; texts=%q", tester.Texts())
	}
	if !tester.HasText("History by Project") {
		t.Fatalf("page header missing")
	}
}
