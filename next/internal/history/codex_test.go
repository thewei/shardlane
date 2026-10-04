package history

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

const codexFixtureRollout = `{"timestamp":"2026-08-02T09:15:00Z","type":"session_meta","payload":{"cwd":"/work/codex-demo","originator":"codex_cli_rs","git":{"branch":"feature/history"}}}
{"timestamp":"2026-08-02T09:15:01Z","type":"turn_context","payload":{"model":"gpt-5"}}
{"timestamp":"2026-08-02T09:15:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Build history"}]}}
{"timestamp":"2026-08-02T09:15:03Z","type":"response_item","payload":{"type":"function_call","call_id":"call-1","name":"shell","arguments":"{\"command\":\"cargo test\"}"}}
{"timestamp":"2026-08-02T09:15:04Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"ok"}}
{"timestamp":"2026-08-02T09:15:05Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Implemented."}]}}
{"timestamp":"2026-08-02T09:15:06Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":42}}}}
`

// TestCodexAdapterParsesRollout pins the audited Rust rollout fixture: meta
// enrichment, tool call/output resolution, token counting, and index units.
func TestCodexAdapterParsesRollout(t *testing.T) {
	dir := t.TempDir()
	sessions := filepath.Join(dir, "sessions", "2026", "08", "02")
	if err := os.MkdirAll(sessions, 0o755); err != nil {
		t.Fatal(err)
	}
	sessionID := "22222222-aaaa-bbbb-cccc-000000000002"
	rollout := filepath.Join(sessions, "rollout-2026-08-02T09-15-00-"+sessionID+".jsonl")
	if err := os.WriteFile(rollout, []byte(codexFixtureRollout), 0o600); err != nil {
		t.Fatal(err)
	}

	references, err := listJSONLRefs(filepath.Join(dir, "sessions"), AgentCodex, rolloutNativeID)
	if err != nil {
		t.Fatal(err)
	}
	if len(references) != 1 {
		t.Fatalf("references = %d, want 1", len(references))
	}
	if references[0].NativeID != sessionID {
		t.Fatalf("native id = %q", references[0].NativeID)
	}

	parsed, err := ParseCodexTranscript(references[0])
	if err != nil {
		t.Fatal(err)
	}
	meta := parsed.Meta
	if meta.Key != "codex:"+sessionID {
		t.Fatalf("key = %q", meta.Key)
	}
	if meta.Title != "Build history" {
		t.Fatalf("title = %q", meta.Title)
	}
	if meta.ProjectPath != "/work/codex-demo" || meta.ProjectName != "codex-demo" {
		t.Fatalf("project = %q / %q", meta.ProjectPath, meta.ProjectName)
	}
	if meta.Model == nil || *meta.Model != "gpt-5" {
		t.Fatalf("model = %v", meta.Model)
	}
	if meta.TokensUsed == nil || *meta.TokensUsed != 42 {
		t.Fatalf("tokens = %v", meta.TokensUsed)
	}
	if meta.Source == nil || *meta.Source != "CLI" {
		t.Fatalf("source = %v", meta.Source)
	}
	if meta.GitBranch == nil || *meta.GitBranch != "feature/history" {
		t.Fatalf("branch = %v", meta.GitBranch)
	}
	// Rust parity: the empty tool-call host message has Text kind and is
	// counted, so the fixture yields 3 (user + host + assistant answer).
	if meta.MessageCount != 3 {
		t.Fatalf("message count = %d, want 3", meta.MessageCount)
	}

	units := unitsFromMessages(parsed.Mainline)
	if len(units) != 3 {
		t.Fatalf("units = %d, want 3", len(units))
	}
	joined := ""
	for _, unit := range units {
		joined += unit.Text + "\n"
	}
	if !strings.Contains(joined, "shell cargo test") {
		t.Fatalf("tool summary missing from units: %q", joined)
	}

	found := false
	for _, message := range parsed.Mainline {
		if message.Kind == MessageText && message.Text == "Implemented." {
			found = true
		}
		for _, call := range message.ToolCalls {
			if call.InputPreview != "cargo test" {
				t.Fatalf("tool preview = %q", call.InputPreview)
			}
			if call.Output == nil || *call.Output != "ok" {
				t.Fatalf("tool output = %v", call.Output)
			}
		}
	}
	if !found {
		t.Fatal("assistant text missing from mainline")
	}
}

// TestCodexAdapterEventFallback pins the fallback rule: event_msg
// user/agent messages are presented only when no real content exists.
func TestCodexAdapterEventFallback(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "rollout-plain.jsonl")
	content := strings.Join([]string{
		`{"timestamp":"2026-08-02T10:00:00Z","type":"event_msg","payload":{"type":"user_message","message":"Hello from events"}}`,
		`{"timestamp":"2026-08-02T10:00:01Z","type":"event_msg","payload":{"type":"agent_message","message":"Working on it"}}`,
	}, "\n")
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}

	parsed, err := ParseCodexTranscript(SessionFileRef{
		Agent: AgentCodex, NativeID: "plain", FilePath: path,
		MtimeMS: time.UnixMilli(1000).UnixMilli(), SizeBytes: int64(len(content)),
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(parsed.Mainline) != 2 {
		t.Fatalf("fallback messages = %d, want 2", len(parsed.Mainline))
	}
	if parsed.Mainline[0].Role != RoleUser || parsed.Mainline[0].Text != "Hello from events" {
		t.Fatalf("fallback user = %+v", parsed.Mainline[0])
	}
	if parsed.Meta.Title != "Hello from events" {
		t.Fatalf("fallback title = %q", parsed.Meta.Title)
	}

	// With a real user message present, the fallback view is dropped.
	mixed := `{"timestamp":"2026-08-02T10:00:00Z","type":"event_msg","payload":{"type":"user_message","message":"Hello from events"}}
{"timestamp":"2026-08-02T10:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Real message"}]}}
`
	path2 := filepath.Join(dir, "rollout-mixed.jsonl")
	if err := os.WriteFile(path2, []byte(mixed), 0o600); err != nil {
		t.Fatal(err)
	}
	parsed2, err := ParseCodexTranscript(SessionFileRef{
		Agent: AgentCodex, NativeID: "mixed", FilePath: path2,
		MtimeMS: 1000, SizeBytes: int64(len(mixed)),
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(parsed2.Mainline) != 1 || parsed2.Mainline[0].Text != "Real message" {
		t.Fatalf("mixed mainline = %+v", parsed2.Mainline)
	}
}

// TestCodexAdapterReasoningMergeAndUnknown pins reasoning merge-into-host and
// unknown-line counting.
func TestCodexAdapterReasoningMergeAndUnknown(t *testing.T) {
	content := strings.Join([]string{
		`{"timestamp":"2026-08-02T10:00:00Z","type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"I should test"}]}}`,
		`{"timestamp":"2026-08-02T10:00:01Z","type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"More thinking"}]}}`,
		`{"timestamp":"2026-08-02T10:00:02Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done"}]}}`,
		`this is not json`,
		`{"type":"mystery_row"}`,
	}, "\n")
	dir := t.TempDir()
	path := filepath.Join(dir, "rollout-r.jsonl")
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
	parsed, err := ParseCodexTranscript(SessionFileRef{
		Agent: AgentCodex, NativeID: "r", FilePath: path,
		MtimeMS: 1000, SizeBytes: int64(len(content)),
	})
	if err != nil {
		t.Fatal(err)
	}
	// Rust parity: a reasoning row only merges into a host with no existing
	// thinking, so consecutive reasoning rows keep separate hosts.
	if len(parsed.Mainline) != 3 {
		t.Fatalf("mainline = %d messages, want 3", len(parsed.Mainline))
	}
	if parsed.Mainline[0].Thinking == nil || !strings.Contains(*parsed.Mainline[0].Thinking, "I should test") {
		t.Fatalf("first host thinking = %v", parsed.Mainline[0].Thinking)
	}
	if parsed.Mainline[1].Thinking == nil || !strings.Contains(*parsed.Mainline[1].Thinking, "More thinking") {
		t.Fatalf("second host thinking = %v", parsed.Mainline[1].Thinking)
	}
	if parsed.Mainline[2].Text != "Done" {
		t.Fatalf("assistant text = %q", parsed.Mainline[2].Text)
	}
	if parsed.UnknownLineCount != 2 {
		t.Fatalf("unknown lines = %d, want 2", parsed.UnknownLineCount)
	}
}

func TestRolloutNativeID(t *testing.T) {
	cases := map[string]string{
		"rollout-2026-08-02T09-15-00-22222222-aaaa-bbbb-cccc-000000000002": "22222222-aaaa-bbbb-cccc-000000000002",
		"plain-stem":    "plain-stem",
		"rollout-short": "rollout-short",
	}
	for input, want := range cases {
		if got := rolloutNativeID(input); got != want {
			t.Fatalf("rolloutNativeID(%q) = %q, want %q", input, got, want)
		}
	}
}

func TestCodexMakePreview(t *testing.T) {
	if got := codexMakePreview(map[string]any{"command": "cargo test"}); got != "cargo test" {
		t.Fatalf("object preview = %q", got)
	}
	if got := codexMakePreview("raw text"); got != "raw text" {
		t.Fatalf("string preview = %q", got)
	}
	long := strings.Repeat("x", 250)
	got := codexMakePreview(long)
	if len([]rune(got)) != 201 || !strings.HasSuffix(got, "…") {
		t.Fatalf("long preview length = %d", len([]rune(got)))
	}
}

func TestPathOwns(t *testing.T) {
	cases := []struct {
		root string
		path string
		want bool
	}{
		{"/tmp/codex", "/tmp/codex/a.jsonl", true},
		{"/tmp/codex", "/tmp/codex", true},
		{"/tmp/codex", "/tmp/codex-other/a.jsonl", false},
		{"/tmp/codex/", "/tmp/codex/a.jsonl", true},
		{"", "/tmp/x", false},
		{"/tmp/codex", "sqlite:/other", false},
	}
	for _, tc := range cases {
		if got := pathOwns(tc.root, tc.path); got != tc.want {
			t.Fatalf("pathOwns(%q, %q) = %v, want %v", tc.root, tc.path, got, tc.want)
		}
	}
}

func TestListJSONLRefsMissingRootIsEmpty(t *testing.T) {
	references, err := listJSONLRefs(filepath.Join(t.TempDir(), "missing"), AgentCodex, rolloutNativeID)
	if err != nil {
		t.Fatalf("missing root must be an empty source, got %v", err)
	}
	if len(references) != 0 {
		t.Fatalf("references = %d", len(references))
	}
}
