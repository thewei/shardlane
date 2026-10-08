/**
 * [INPUT]: 依赖 history/pi.go, models.go, live.go
 * [OUTPUT]: TestPiParseTranscript, TestPiLiveDecoderIncremental, TestParseAgentIDAliases
 * [POS]: history 包中 Pi / Oh My Pi 解析与 live 增量解码测试
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

package history

import (
	"os"
	"path/filepath"
	"testing"
)

const testPiJSONL = `{"type":"session","version":3,"id":"pi-sess-123","timestamp":"2026-10-06T10:00:00.000Z","cwd":"/work/herdr-client"}
{"type":"message","timestamp":"2026-10-06T10:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"Check the git status"}]}}
{"type":"message","timestamp":"2026-10-06T10:00:02.000Z","message":{"role":"assistant","model":"glm-5.3-flash","content":[{"type":"thinking","thinking":"Let me inspect git status."},{"type":"toolCall","id":"call_1","name":"sh","arguments":{"command":"git status"}}],"usage":{"totalTokens":150}}}
{"type":"message","timestamp":"2026-10-06T10:00:03.000Z","message":{"role":"toolResult","toolCallId":"call_1","toolName":"sh","content":[{"type":"text","text":"On branch main\nnothing to commit"}],"isError":false}}
{"type":"message","timestamp":"2026-10-06T10:00:04.000Z","message":{"role":"assistant","model":"glm-5.3-flash","content":[{"type":"text","text":"Working tree is clean."}]}}
`

func TestPiParseTranscript(t *testing.T) {
	tmpDir := t.TempDir()
	filePath := filepath.Join(tmpDir, "2026-10-06T10-00-00-000Z_pi-sess-123.jsonl")
	if err := os.WriteFile(filePath, []byte(testPiJSONL), 0644); err != nil {
		t.Fatal(err)
	}

	ref := SessionFileRef{
		Agent:     AgentPi,
		NativeID:  "pi-sess-123",
		FilePath:  filePath,
		MtimeMS:   1000,
		SizeBytes: int64(len(testPiJSONL)),
	}

	transcript, err := ParsePiTranscript(ref)
	if err != nil {
		t.Fatalf("ParsePiTranscript failed: %v", err)
	}

	if transcript.Meta.ID != "pi-sess-123" {
		t.Errorf("got ID %q, want %q", transcript.Meta.ID, "pi-sess-123")
	}
	if transcript.Meta.ProjectPath != "/work/herdr-client" {
		t.Errorf("got ProjectPath %q, want %q", transcript.Meta.ProjectPath, "/work/herdr-client")
	}
	if transcript.Meta.Title != "Check the git status" {
		t.Errorf("got Title %q, want %q", transcript.Meta.Title, "Check the git status")
	}
	if transcript.Meta.TokensUsed == nil || *transcript.Meta.TokensUsed != 150 {
		t.Errorf("got TokensUsed %v, want 150", transcript.Meta.TokensUsed)
	}

	// Mainline messages: 1 User, 1 merged Assistant (containing both toolCall + final text)
	if len(transcript.Mainline) != 2 {
		t.Fatalf("got %d messages, want 2 (user + merged assistant)", len(transcript.Mainline))
	}

	user := transcript.Mainline[0]
	if user.Role != RoleUser || user.Text != "Check the git status" {
		t.Errorf("user message mismatch: %+v", user)
	}

	assistant := transcript.Mainline[1]
	if assistant.Role != RoleAssistant {
		t.Errorf("assistant role mismatch: %v", assistant.Role)
	}
	if assistant.Thinking == nil || *assistant.Thinking != "Let me inspect git status." {
		t.Errorf("assistant thinking mismatch: %v", assistant.Thinking)
	}
	if assistant.Text != "Working tree is clean." {
		t.Errorf("assistant text mismatch: %q", assistant.Text)
	}
	if len(assistant.ToolCalls) != 1 {
		t.Fatalf("assistant tool calls length %d, want 1", len(assistant.ToolCalls))
	}
	tc := assistant.ToolCalls[0]
	if tc.ID != "call_1" || tc.Name != "sh" {
		t.Errorf("tool call mismatch: %+v", tc)
	}
	if tc.Output == nil || *tc.Output != "On branch main\nnothing to commit" {
		t.Errorf("tool call output mismatch: %v", tc.Output)
	}
	if tc.IsError {
		t.Errorf("tool call isError should be false")
	}
}

func TestPiLiveDecoderIncremental(t *testing.T) {
	decoder, err := NewLiveDecoder(AgentPi)
	if err != nil {
		t.Fatalf("NewLiveDecoder(AgentPi) failed: %v", err)
	}

	// 1. Line 1: session header
	sync := decoder.Append([]byte("{\"type\":\"session\",\"id\":\"s1\",\"cwd\":\"/app\"}\n"))
	if len(decoder.Projection()) != 0 {
		t.Fatalf("expected 0 messages from session header, got %d", len(decoder.Projection()))
	}

	// 2. Line 2: user message
	sync = decoder.Append([]byte("{\"type\":\"message\",\"timestamp\":\"2026-10-06T10:00:00Z\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}}\n"))
	if len(sync.Appended) != 1 {
		t.Fatalf("expected 1 appended message, got %d", len(sync.Appended))
	}
	if decoder.Projection()[0].Text != "hello" {
		t.Errorf("expected text hello, got %q", decoder.Projection()[0].Text)
	}

	// 3. Line 3: assistant tool call
	sync = decoder.Append([]byte("{\"type\":\"message\",\"timestamp\":\"2026-10-06T10:00:01Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"toolCall\",\"id\":\"c1\",\"name\":\"ls\",\"arguments\":{}}]}}\n"))
	if len(sync.Appended) != 1 {
		t.Fatalf("expected 1 appended assistant message, got %d", len(sync.Appended))
	}
	if len(decoder.Projection()[1].ToolCalls) != 1 {
		t.Fatalf("expected 1 tool call, got %d", len(decoder.Projection()[1].ToolCalls))
	}

	// 4. Line 4: tool result (mutates earlier assistant message)
	sync = decoder.Append([]byte("{\"type\":\"message\",\"timestamp\":\"2026-10-06T10:00:02Z\",\"message\":{\"role\":\"toolResult\",\"toolCallId\":\"c1\",\"content\":[{\"type\":\"text\",\"text\":\"file.txt\"}]}}\n"))
	if len(sync.Changed) != 1 {
		t.Fatalf("expected 1 changed message for tool result, got %d", len(sync.Changed))
	}
	tc := decoder.Projection()[1].ToolCalls[0]
	if tc.Output == nil || *tc.Output != "file.txt" {
		t.Errorf("expected output file.txt, got %v", tc.Output)
	}

	// 5. Line 5: consecutive assistant message merges into existing assistant turn
	sync = decoder.Append([]byte("{\"type\":\"message\",\"timestamp\":\"2026-10-06T10:00:03Z\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Done listing.\"}]}}\n"))
	if len(decoder.Projection()) != 2 {
		t.Fatalf("expected projection length 2 (merged), got %d", len(decoder.Projection()))
	}
	if len(sync.Changed) != 1 {
		t.Fatalf("expected 1 changed message on merged assistant turn, got %d", len(sync.Changed))
	}
	if decoder.Projection()[1].Text != "Done listing." {
		t.Errorf("expected merged text Done listing., got %q", decoder.Projection()[1].Text)
	}

	// 6. Idempotent poll without new data
	status := decoder.Status()
	if status.HasUpdates() {
		t.Errorf("duplicate wake must report HasUpdates = false")
	}
}

func TestParseAgentIDAliases(t *testing.T) {
	cases := []struct {
		input string
		want  AgentID
		ok    bool
	}{
		{"pi", AgentPi, true},
		{"pi-coding-agent", AgentPi, true},
		{"omp", AgentOMP, true},
		{"oh-my-pi", AgentOMP, true},
		{"claude", AgentClaudeCode, true},
		{"claude-code", AgentClaudeCode, true},
		{"claude_code", AgentClaudeCode, true},
		{"codex", AgentCodex, true},
		{"codex-cli", AgentCodex, true},
		{"antigravity", AgentAntigravity, true},
		{"agy", AgentAntigravity, true},
		{"command-code", AgentCommandCode, true},
		{"commandcode", AgentCommandCode, true},
		{"unknown-agent", "", false},
	}

	for _, c := range cases {
		got, ok := ParseAgentID(c.input)
		if ok != c.ok || got != c.want {
			t.Errorf("ParseAgentID(%q) = (%q, %v), want (%q, %v)", c.input, got, ok, c.want, c.ok)
		}
	}
}
