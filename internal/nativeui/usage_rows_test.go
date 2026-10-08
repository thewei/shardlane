package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
)

// usageProjectionShell builds a shell with one live agent whose typed
// session identity (id kind, session-1) can match a catalog row.
func usageProjectionShell(t *testing.T) *Shell {
	t.Helper()
	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.projection = herdr.Projection{
		Agents: []herdr.Agent{{
			TerminalID: "term-1", PaneID: "p1", Status: "working", Name: "Scout",
			AgentSession: &herdr.AgentSessionIdentity{Agent: "claude-code", Kind: "id", Source: "transcript", Value: "session-1"},
		}},
	}
	shell.reconcileWorkbench()
	return shell
}

// TestUsageProjectionMatchesCatalogMeta pins the §17.2 cached projection:
// the exact identity matches the catalog row and the §17.6 snapshot lands
// in the render cache; non-matching identities stay quiet.
func TestUsageProjectionMatchesCatalogMeta(t *testing.T) {
	shell := usageProjectionShell(t)
	fake := newFakeHistoryView()
	shell.hist.service = fake

	model := "Sonnet"
	tokens := int64(38000)
	matching := history.SessionSummary{Meta: history.SessionMeta{
		Agent: history.AgentClaudeCode, ID: "session-1", FilePath: "/claude/session-1.jsonl",
		Model: &model, TokensUsed: &tokens,
	}}
	other := history.SessionSummary{Meta: history.SessionMeta{
		Agent: history.AgentClaudeCode, ID: "other-session",
	}}

	done := make(chan struct{})
	go func() {
		defer close(done)
		shell.refreshUsageProjection(true)
	}()
	call := <-fake.calls
	call.release <- listResult{summaries: []history.SessionSummary{other, matching}}
	<-done

	usage := shell.usageByAgent[agent.AgentKey{InstanceID: "inst-1", TerminalID: "term-1"}]
	if !usage.Complete || usage.Model != "Sonnet" || usage.Tokens == nil || *usage.Tokens != 38000 {
		t.Fatalf("cached snapshot = %+v", usage)
	}
	if got := shell.usageLineFor(usage.AgentKey); got != "Sonnet · 38k tok" {
		t.Fatalf("usage line = %q", got)
	}
}

// TestUsageProjectionRendersRowSecondaryLine pins the §17.2 row render: the
// quick panel/status row shows the usage line as its secondary caption.
func TestUsageProjectionRendersRowSecondaryLine(t *testing.T) {
	shell := usageProjectionShell(t)
	shell.usageByAgent = map[agent.AgentKey]agent.AgentUsageSnapshot{
		agent.AgentKey{InstanceID: "inst-1", TerminalID: "term-1"}: {
			AgentKey: agent.AgentKey{InstanceID: "inst-1", TerminalID: "term-1"},
			Provider: history.AgentClaudeCode, Model: "Sonnet",
			Tokens: func() *int64 { v := int64(38000); return &v }(),
		},
	}

	tester := ui.NewTester(shell.QuickPanelView, QuickPanelWidth, QuickPanelHeight)
	for _, want := range []string{"Sonnet · 38k tok", "Scout"} {
		if !tester.HasText(want) {
			t.Fatalf("usage row missing %q; texts=%q", want, tester.Texts())
		}
	}
}

// TestUsageIdentityMatches pins the exact-identity matcher: same provider
// plus the declared kind's value against the native id or the exact source
// path — never guessed from other fields.
func TestUsageIdentityMatches(t *testing.T) {
	meta := history.SessionMeta{Agent: history.AgentClaudeCode, ID: "session-1", FilePath: "/claude/session-1.jsonl"}
	if !usageIdentityMatches(history.AgentClaudeCode, "id", "session-1", meta) {
		t.Fatal("id kind must match the native id")
	}
	if !usageIdentityMatches(history.AgentClaudeCode, "path", "/claude/session-1.jsonl", meta) {
		t.Fatal("path kind must match the exact source path")
	}
	for _, tc := range []struct{ provider, kind, value string }{
		{string(history.AgentCodex), "id", "session-1"},
		{"claude-code", "id", "other"},
		{"claude-code", "path", "/other.jsonl"},
		{"claude-code", "fingerprint", "session-1"},
		{"claude-code", "id", ""},
	} {
		if usageIdentityMatches(history.AgentID(tc.provider), tc.kind, tc.value, meta) {
			t.Fatalf("%v must not match", tc)
		}
	}
}

// TestUsageProjectionThrottled pins the one-minute refresh throttle: an
// unforced refresh inside the window does not hit the catalog; forced does.
func TestUsageProjectionThrottled(t *testing.T) {
	shell := usageProjectionShell(t)
	fake := newFakeHistoryView()
	shell.hist.service = fake

	release := func(summaries []history.SessionSummary) {
		call := <-fake.calls
		call.release <- listResult{summaries: summaries}
	}
	done := make(chan struct{})
	go func() {
		defer close(done)
		shell.refreshUsageProjection(true)
	}()
	release(nil)
	<-done

	// Unforced inside the throttle window: no catalog call.
	shell.refreshUsageProjection(false)
	select {
	case call := <-fake.calls:
		t.Fatalf("throttled refresh hit the catalog: %+v", call)
	default:
	}

	// Forced bypasses the throttle.
	go func() {
		shell.refreshUsageProjection(true)
	}()
	release(nil)
}
