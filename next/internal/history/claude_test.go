package history

import (
	"os"
	"path/filepath"
	"testing"
)

func TestClaudeAdapterParsesMainlineThinkingAndToolOutput(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "claude-session.jsonl")
	jsonl := "" +
		"{\"type\":\"user\",\"cwd\":\"/work/demo\",\"gitBranch\":\"main\",\"timestamp\":\"2026-08-01T01:00:00Z\",\"message\":{\"content\":\"Fix the parser\"}}\n" +
		"{\"type\":\"assistant\",\"cwd\":\"/work/demo\",\"timestamp\":\"2026-08-01T01:00:01Z\",\"message\":{\"id\":\"m1\",\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5},\"content\":[{\"type\":\"thinking\",\"thinking\":\"Inspect first\"},{\"type\":\"tool_use\",\"id\":\"tool-1\",\"name\":\"Read\",\"input\":{\"file_path\":\"src/lib.rs\"}},{\"type\":\"text\",\"text\":\"I found it.\"}]}}\n" +
		"{\"type\":\"user\",\"cwd\":\"/work/demo\",\"timestamp\":\"2026-08-01T01:00:02Z\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"tool-1\",\"content\":\"source text\"}]}}\n"
	if err := os.WriteFile(path, []byte(jsonl), 0o600); err != nil {
		t.Fatal(err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	reference := SessionFileRef{
		Agent:     AgentClaudeCode,
		NativeID:  "claude-session",
		FilePath:  path,
		MtimeMS:   info.ModTime().UnixMilli(),
		SizeBytes: info.Size(),
	}

	parsed, err := ParseClaudeTranscript(reference)
	if err != nil {
		t.Fatal(err)
	}
	if parsed.Meta.Agent != AgentClaudeCode {
		t.Fatalf("agent = %q", parsed.Meta.Agent)
	}
	if parsed.Meta.Title != "Fix the parser" {
		t.Fatalf("title = %q", parsed.Meta.Title)
	}
	if parsed.Meta.ProjectPath != "/work/demo" || parsed.Meta.ProjectName != "demo" {
		t.Fatalf("project = %q / %q", parsed.Meta.ProjectPath, parsed.Meta.ProjectName)
	}
	if parsed.Meta.GitBranch == nil || *parsed.Meta.GitBranch != "main" {
		t.Fatalf("git branch = %#v", parsed.Meta.GitBranch)
	}
	if parsed.Meta.Model == nil || *parsed.Meta.Model != "claude-sonnet-4-5" {
		t.Fatalf("model = %#v", parsed.Meta.Model)
	}
	if parsed.Meta.TokensUsed == nil || *parsed.Meta.TokensUsed != 15 {
		t.Fatalf("tokens = %#v", parsed.Meta.TokensUsed)
	}
	if parsed.Meta.MessageCount != 2 || len(parsed.Mainline) != 2 {
		t.Fatalf("message counts meta=%d mainline=%d", parsed.Meta.MessageCount, len(parsed.Mainline))
	}
	if parsed.Mainline[0].Role != RoleUser || parsed.Mainline[1].Role != RoleAssistant {
		t.Fatalf("roles = %q, %q", parsed.Mainline[0].Role, parsed.Mainline[1].Role)
	}
	if parsed.Mainline[1].Thinking == nil || *parsed.Mainline[1].Thinking != "Inspect first" {
		t.Fatalf("thinking = %#v", parsed.Mainline[1].Thinking)
	}
	if len(parsed.Mainline[1].ToolCalls) != 1 {
		t.Fatalf("tool calls = %#v", parsed.Mainline[1].ToolCalls)
	}
	tool := parsed.Mainline[1].ToolCalls[0]
	if tool.Name != "Read" || tool.Output == nil || *tool.Output != "source text" {
		t.Fatalf("tool = %#v", tool)
	}
	if parsed.UnknownLineCount != 0 {
		t.Fatalf("unknown lines = %d", parsed.UnknownLineCount)
	}
}

func TestClaudeAdapterSkipsKnownMetadataAndCountsUnknown(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "session.jsonl")
	jsonl := "" +
		"{\"type\":\"custom-title\",\"customTitle\":\"Named Session\"}\n" +
		"{\"type\":\"queue-operation\"}\n" +
		"{\"type\":\"future-provider-row\"}\n" +
		"{\"type\":\"user\",\"cwd\":\"/work/demo\",\"message\":{\"content\":\"hello\"}}\n"
	if err := os.WriteFile(path, []byte(jsonl), 0o600); err != nil {
		t.Fatal(err)
	}
	info, _ := os.Stat(path)
	parsed, err := ParseClaudeTranscript(SessionFileRef{
		Agent: AgentClaudeCode, NativeID: "session", FilePath: path, SizeBytes: info.Size(),
	})
	if err != nil {
		t.Fatal(err)
	}
	if parsed.Meta.Title != "Named Session" {
		t.Fatalf("title = %q", parsed.Meta.Title)
	}
	if parsed.UnknownLineCount != 1 {
		t.Fatalf("unknown = %d", parsed.UnknownLineCount)
	}
}
