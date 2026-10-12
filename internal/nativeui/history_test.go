package nativeui

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

// codexFixtureRollout mirrors the audited rollout fixture used by the
// history package tests.
const codexFixtureRollout = `{"timestamp":"2026-08-02T09:15:00Z","type":"session_meta","payload":{"cwd":"/work/codex-demo","originator":"codex_cli_rs","git":{"branch":"feature/history"}}}
{"timestamp":"2026-08-02T09:15:01Z","type":"turn_context","payload":{"model":"gpt-5"}}
{"timestamp":"2026-08-02T09:15:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Build history"}]}}
{"timestamp":"2026-08-02T09:15:03Z","type":"response_item","payload":{"type":"function_call","call_id":"call-1","name":"shell","arguments":"{\"command\":\"cargo test\"}"}}
{"timestamp":"2026-08-02T09:15:04Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"ok"}}
{"timestamp":"2026-08-02T09:15:05Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Implemented."}]}}
{"timestamp":"2026-08-02T09:15:06Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":42}}}}
`

func writeCodexSessionFile(t *testing.T, dir, name, body string) string {
	t.Helper()
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, name)
	if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
		t.Fatal(err)
	}
	return path
}

// testRolloutNativeID mirrors the Rust rollout stem rule for the fixture
// naming used here.
func testRolloutNativeID(stem string) string {
	if rest, ok := strings.CutPrefix(stem, "rollout-"); ok && len(rest) > 20 && rest[10] == 'T' {
		return rest[20:]
	}
	return stem
}

// newHistoryTestShell builds a Shell over a real HistoryService whose catalog
// is prepopulated from the Codex rollout fixture on disk.
func newHistoryTestShell(t *testing.T) (*Shell, *history.Catalog, string) {
	t.Helper()
	home := t.TempDir()
	sessions := filepath.Join(home, "sessions")
	writeCodexSessionFile(t, sessions, "rollout-2026-08-02T09-15-00-22222222-aaaa-bbbb-cccc-000000000002.jsonl", codexFixtureRollout)

	catalog := history.NewCatalog()
	service := history.NewHistoryService(catalog, []history.SourceRoot{{
		Agent: history.AgentCodex, Directory: sessions, NativeID: testRolloutNativeID,
	}})
	if _, err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	shell := NewShell(WithHistory(service))
	return shell, catalog, home
}

func TestHistoryListRendersRealSessionsWithFilters(t *testing.T) {
	shell, _, _ := newHistoryTestShell(t)
	shell.router.Replace("/history")
	tester := ui.NewTester(shell.View, 1200, 800)

	// The provider filter rail is derived from the sessions actually in the
	// catalog (F37): only Codex is seeded, so Claude Code must not appear.
	for _, want := range []string{"Build history", "Codex", "codex-demo"} {
		if !tester.HasText(want) {
			t.Fatalf("history list missing %q; texts=%q", want, tester.Texts())
		}
	}
	if tester.HasText("Claude Code") {
		t.Fatalf("filter rail shows a provider absent from the catalog; texts=%q", tester.Texts())
	}

	// The Sidebar Recent section is fed from the same metadata.
	if len(shell.recentItems) == 0 || shell.recentItems[0].Title != "Build history" {
		t.Fatalf("recent items = %+v", shell.recentItems)
	}

	// A single-agent catalog offers no agent filter: the rows already name
	// their agent, and a one-option control is noise.
	if tester.HasText("All") && shell.hist.provider != "" {
		t.Fatalf("single-agent catalog shows a filter; texts=%q", tester.Texts())
	}
	shell.hist.provider = string(history.AgentCodex)
	shell.applyHistoryFilters()
	tester.Frame()
	if !tester.HasText("Build history") {
		t.Fatal("codex filter dropped the codex session")
	}
}

func TestHistoryEmptyStateWhenCatalogIsEmpty(t *testing.T) {
	shell := NewShell(WithHistory(history.NewHistoryService(history.NewCatalog(), nil)))
	shell.router.Replace("/history")
	tester := ui.NewTester(shell.View, 1200, 800)

	if !tester.HasText("No sessions yet") {
		t.Fatalf("missing empty state; texts=%q", tester.Texts())
	}
}

func TestHistoryDetailThinkingCollapsedByDefault(t *testing.T) {
	shell, _, home := newHistoryTestShell(t)
	// A second rollout whose assistant turn carries a reasoning summary.
	writeCodexSessionFile(t, filepath.Join(home, "sessions"),
		"rollout-2026-08-03T09-15-00-44444444-aaaa-bbbb-cccc-000000000004.jsonl",
		`{"timestamp":"2026-08-03T09:15:00Z","type":"session_meta","payload":{"cwd":"/work/reasoning","originator":"codex_cli_rs"}}
{"timestamp":"2026-08-03T09:15:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Reasoning session"}]}}
{"timestamp":"2026-08-03T09:15:02Z","type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"line one of hidden reasoning\nline two"}]}}
{"timestamp":"2026-08-03T09:15:03Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Answer with reasoning"}]}}
`)
	if _, err := shell.hist.service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}

	shell.router.Replace("/history/codex:44444444-aaaa-bbbb-cccc-000000000004")
	tester := ui.NewTester(shell.View, 1200, 800)
	if !tester.HasText("Answer with reasoning") {
		t.Fatalf("assistant answer missing; texts=%q", tester.Texts())
	}
	if tester.HasText("line two") {
		t.Fatalf("thinking must be collapsed by default; texts=%q", tester.Texts())
	}
	// DS-05: the official Collapsible owns the thinking disclosure.
	if err := tester.Click("Thinking"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("line two") {
		t.Fatal("expanding thinking did not reveal the body")
	}
}

func TestHistoryListSearchNarrowsSessions(t *testing.T) {
	shell, _, _ := newHistoryTestShell(t)
	shell.router.Replace("/history")
	tester := ui.NewTester(shell.View, 1200, 800)
	if !tester.HasText("Build history") {
		t.Fatal("fixture session missing before search")
	}

	shell.hist.query = "nomatch-anywhere"
	shell.requestHistoryList()
	tester.Frame()
	if tester.HasText("Build history") {
		t.Fatal("search filter did not narrow the list")
	}
	if !tester.HasText("No matching sessions") {
		t.Fatalf("filtered empty state missing; texts=%q", tester.Texts())
	}
}

func strPtr(value string) *string { return &value }
