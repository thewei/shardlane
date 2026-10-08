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

	catalog, err := history.OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { catalog.Close() })
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

	// Provider filter narrows the list.
	if err := tester.Click("Codex"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("Build history") {
		t.Fatal("codex filter dropped the codex session")
	}
}

func TestHistoryEmptyStateWhenCatalogIsEmpty(t *testing.T) {
	catalog, err := history.OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()
	shell := NewShell(WithHistory(history.NewHistoryService(catalog, nil)))
	shell.router.Replace("/history")
	tester := ui.NewTester(shell.View, 1200, 800)

	if !tester.HasText("No conversations found") {
		t.Fatalf("missing empty state; texts=%q", tester.Texts())
	}
}

func TestHistoryDetailRendersBoundedNativeBlocks(t *testing.T) {
	shell, _, _ := newHistoryTestShell(t)
	shell.router.Replace("/history/codex:22222222-aaaa-bbbb-cccc-000000000002")
	tester := ui.NewTester(shell.View, 1200, 800)

	for _, want := range []string{"Build history", "USER", "ASSISTANT", "Implemented.", "cargo test", "Showing 1–3 of 3 messages"} {
		if !tester.HasText(want) {
			t.Fatalf("detail missing %q; texts=%q", want, tester.Texts())
		}
	}
	// The empty tool host message contributes no visible text block.
	if shell.hist.detail == nil || len(shell.hist.detail.window.Messages) != 3 {
		t.Fatalf("materialized messages = %+v", shell.hist.detail)
	}
}

func TestHistoryDetailThinkingCollapsedByDefault(t *testing.T) {
	shell, catalog, _ := newHistoryTestShell(t)
	// Seed one thinking-bearing session directly into the catalog + cache.
	source := history.SessionFileRef{Agent: history.AgentCodex, NativeID: "think", FilePath: filepath.Join(t.TempDir(), "rollout-think.jsonl"), MtimeMS: 1000, SizeBytes: 12}
	meta := history.SessionMeta{
		Key: "codex:think", ID: "think", Agent: history.AgentCodex, Title: "Reasoning session",
		FilePath: source.FilePath, UpdatedAt: 1000, SizeBytes: source.SizeBytes,
	}
	transcript := history.ParsedTranscript{Meta: meta, Mainline: []history.TranscriptMessage{{
		Seq: 0, Role: history.RoleAssistant, Kind: history.MessageText,
		Text: "Answer with reasoning", Thinking: strPtr("line one of hidden reasoning\nline two"),
	}}}
	if err := catalog.WriteSession(meta, source.MtimeMS, nil); err != nil {
		t.Fatal(err)
	}
	if err := catalog.CacheTranscript(source, transcript); err != nil {
		t.Fatal(err)
	}

	shell.router.Replace("/history/codex:think")
	tester := ui.NewTester(shell.View, 1200, 800)
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

func TestHistoryDetailSwitchCancelsObsoleteWindow(t *testing.T) {
	shell, _, home := newHistoryTestShell(t)
	// A second conversation to switch to.
	writeCodexSessionFile(t, filepath.Join(home, "sessions"),
		"rollout-2026-08-02T10-15-00-33333333-aaaa-bbbb-cccc-000000000003.jsonl", codexFixtureRollout)
	if _, err := shell.hist.service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}

	shell.router.Replace("/history/codex:22222222-aaaa-bbbb-cccc-000000000002")
	tester := ui.NewTester(shell.View, 1200, 800)
	if shell.hist.detail == nil || shell.hist.detail.key != "codex:22222222-aaaa-bbbb-cccc-000000000002" {
		t.Fatalf("first detail = %+v", shell.hist.detail)
	}
	// Switching conversations supersedes the first window.
	shell.router.Replace("/history/codex:33333333-aaaa-bbbb-cccc-000000000003")
	tester.Frame()
	if shell.hist.detail == nil || shell.hist.detail.key != "codex:33333333-aaaa-bbbb-cccc-000000000003" {
		t.Fatalf("switched detail = %+v", shell.hist.detail)
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
	if !tester.HasText("No conversations found") {
		t.Fatalf("empty search state missing; texts=%q", tester.Texts())
	}
}

func TestHistoryBlocksBoundsAndPureModel(t *testing.T) {
	longLine := strings.Repeat("x", historyMessagePreviewChars+500)
	manyLines := strings.Repeat("line\n", historyMessagePreviewLines+30)
	blocks := historyBlocksFromMessages([]history.TranscriptMessage{
		{Seq: 0, Role: history.RoleUser, Kind: history.MessageText, Text: longLine},
		{Seq: 1, Role: history.RoleAssistant, Kind: history.MessageText, Text: manyLines},
		{Seq: 2, Role: history.RoleSystem, Kind: history.MessageMeta, Text: "meta stays whole"},
	})
	if len(blocks) != 3 {
		t.Fatalf("blocks = %d", len(blocks))
	}
	if len([]rune(blocks[0].Text)) > historyMessagePreviewChars+len("\n… (truncated)") || !blocks[0].Truncated {
		t.Fatalf("char bound: %d runes truncated=%v", len([]rune(blocks[0].Text)), blocks[0].Truncated)
	}
	if lineCount(blocks[1].Text) > historyMessagePreviewLines || !blocks[1].Truncated {
		t.Fatalf("line bound: %d lines truncated=%v", lineCount(blocks[1].Text), blocks[1].Truncated)
	}
	if blocks[2].Kind != historyBlockMeta || blocks[2].Truncated {
		t.Fatalf("meta block = %+v", blocks[2])
	}

	thinking := strings.Repeat("thought ", historyThinkingChars)
	withThinking := historyBlocksFromMessages([]history.TranscriptMessage{{
		Seq: 9, Role: history.RoleAssistant, Kind: history.MessageText,
		Text: "answer", Thinking: &thinking,
	}})
	if len([]rune(withThinking[0].Thinking)) > historyThinkingChars+len("\n… (truncated)") {
		t.Fatalf("thinking bound = %d runes", len([]rune(withThinking[0].Thinking)))
	}

	// Empty tool hosts collapse away; compact summaries survive.
	empty := historyBlocksFromMessages([]history.TranscriptMessage{
		{Seq: 0, Role: history.RoleAssistant, Kind: history.MessageText, Text: ""},
	})
	if len(empty) != 0 {
		t.Fatalf("empty host blocks = %+v", empty)
	}
}

func lineCount(text string) int {
	return strings.Count(text, "\n") + 1
}

func strPtr(value string) *string { return &value }
