package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/agent"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
	"github.com/wh-studio/herdr-client/next/internal/history"
)

// inspectorShell builds a shell with one reconciled agent on pane p1 and
// the given inspector window (nil = unbound).
func inspectorShell(t *testing.T, meta history.SessionMeta, withWindow bool) *Shell {
	t.Helper()
	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.projection = herdr.Projection{
		Agents: []herdr.Agent{{TerminalID: "term-1", PaneID: "p1", Status: "working"}},
	}
	shell.reconcileWorkbench()
	shell.inspector = inspectorState{
		agentKey: agent.AgentKey{InstanceID: "inst-1", TerminalID: "term-1"},
	}
	if withWindow {
		shell.inspector.window = &history.TranscriptWindow{Meta: meta}
	}
	return shell
}

// TestInspectorUsageFactsCard pins the 0.8 usage-facts display consuming the
// §17.6 projection: model/token facts render from the bound conversation's
// metadata, with the compact summary line and the provenance source.
func TestInspectorUsageFactsCard(t *testing.T) {
	model := "claude-sonnet-4-5"
	tokens := int64(38000)
	shell := inspectorShell(t, history.SessionMeta{
		Agent:      history.AgentClaudeCode,
		Model:      &model,
		TokensUsed: &tokens,
	}, true)

	shell.router.Replace("/inspector/p1")
	tester := ui.NewTester(shell.View, 1200, 800)
	for _, want := range []string{
		"Usage facts",
		"claude-sonnet-4-5 · 38k tok",
		"38k (38000)",
		"history-session",
	} {
		if !tester.HasText(want) {
			t.Fatalf("usage card missing %q; texts=%q", want, tester.Texts())
		}
	}
}

// TestInspectorUsageFactsAbsent pins the quiet rule: a fact-less session
// says so explicitly, and an unbound inspector renders no usage card.
func TestInspectorUsageFactsAbsent(t *testing.T) {
	shell := inspectorShell(t, history.SessionMeta{Agent: history.AgentClaudeCode}, true)
	shell.router.Replace("/inspector/p1")
	tester := ui.NewTester(shell.View, 1200, 800)
	if !tester.HasText("No provider-reported facts") {
		t.Fatalf("fact-less session must say so; texts=%q", tester.Texts())
	}

	unbound := inspectorShell(t, history.SessionMeta{}, false)
	unbound.router.Replace("/inspector/p1")
	unboundTester := ui.NewTester(unbound.View, 1200, 800)
	if unboundTester.HasText("Usage facts") {
		t.Fatalf("unbound inspector must not render a usage card; texts=%q", unboundTester.Texts())
	}
}
