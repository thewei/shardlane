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
		"Status",
		"Runtime",
		string(agent.PhaseBlocked),
		"Can receive input",
		string(agent.NeedsTerminal),
		"Attention",
		agent.OperationalLabel(agent.OpNeedsAttention),
		"Markers",
		"unread",
		"Location",
		"Pane",
		"p1",
		"Conversation",
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
	if !tester.HasText("Status") || !tester.HasText("Location") {
		t.Fatalf("identity dimensions missing; texts=%q", tester.Texts())
	}
	if tester.HasText("Conversation") {
		t.Fatalf("unbound inspector rendered a transcript; texts=%q", tester.Texts())
	}
}

// TestSessionsGroupsByProject pins the N19 #9 grouped Sessions view:
// sessions group under their project with counts, the "No project" fallback
// catches sessions without a path, and rows carry the resume-ready facts.
// Only the latest project is open; the others unfold on a header click.
func TestSessionsGroupsByProject(t *testing.T) {
	shell := NewShell()
	shell.hist.service = newFakeHistoryView()
	shell.hist.loaded = true // pre-seeded list: the render must not re-query
	shell.hist.sessions = []history.SessionSummary{
		{Meta: history.SessionMeta{Key: "claude-code:a", Title: "First", Agent: history.AgentClaudeCode,
			ProjectPath: "/work/alpha", ProjectName: "alpha", UpdatedAt: 1_760_000_000_000}},
		{Meta: history.SessionMeta{Key: "codex:b", Title: "Second", Agent: history.AgentCodex,
			ProjectPath: "/work/alpha", ProjectName: "alpha", UpdatedAt: 1_759_000_000_000}},
		{Meta: history.SessionMeta{Key: "codex:c", Title: "Loose", Agent: history.AgentCodex}},
	}
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	for _, want := range []string{
		"Sessions",
		"alpha",
		"2 sessions",
		"First",
		"Second",
		"No project",
		"1 session",
	} {
		if !tester.HasText(want) {
			t.Fatalf("grouped view missing %q; texts=%q", want, tester.Texts())
		}
	}
	if tester.HasText("Loose") {
		t.Fatalf("an older project must start collapsed; texts=%q", tester.Texts())
	}
	if err := tester.Click("Project No project"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("Loose") {
		t.Fatalf("clicking the header did not open the project; texts=%q", tester.Texts())
	}
	if err := tester.Click("Project alpha"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if tester.HasText("First") || !tester.HasText("Loose") {
		t.Fatalf("the latest project must collapse on a click, others stay; texts=%q", tester.Texts())
	}
}
