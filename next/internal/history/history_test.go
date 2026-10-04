package history

import (
	"encoding/json"
	"path/filepath"
	"testing"
)

func TestAgentIDsMatchRustContract(t *testing.T) {
	if len(AllAgents) != 15 {
		t.Fatalf("agent count = %d", len(AllAgents))
	}
	for _, test := range []struct {
		value   string
		want    AgentID
		display string
	}{
		{"claude-code", AgentClaudeCode, "Claude Code"},
		{"codex", AgentCodex, "Codex"},
		{"commandcode", AgentCommandCode, "Command Code"},
		{"gemini", AgentGemini, "Gemini CLI"},
		{"qoder", AgentQoder, "Qoder"},
	} {
		got, ok := ParseAgentID(test.value)
		if !ok || got != test.want || got.DisplayName() != test.display {
			t.Fatalf("ParseAgentID(%q) = %q %v display=%q", test.value, got, ok, got.DisplayName())
		}
	}
}

func TestHistoryModelJSONUsesContractSlugs(t *testing.T) {
	value := TranscriptMessage{
		Seq:       1,
		Role:      RoleAssistant,
		Kind:      MessageCompactSummary,
		Text:      "summary",
		ToolCalls: []ToolCall{},
	}
	data, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	var raw map[string]any
	if err := json.Unmarshal(data, &raw); err != nil {
		t.Fatal(err)
	}
	if raw["role"] != "assistant" || raw["kind"] != "compact-summary" {
		t.Fatalf("unexpected wire values: %s", data)
	}
}

func TestNormalizePathKeyMatchesComponentCleanupWithoutCanonicalize(t *testing.T) {
	for _, test := range []struct {
		in   string
		want string
	}{
		{"", ""},
		{"/Users/me/work/./project", "/Users/me/work/project"},
		{"/Users/me//work///project", "/Users/me/work/project"},
		{"/var/tmp/../tmp/project", "/var/tmp/project"},
	} {
		if got := NormalizePathKey(test.in); got != test.want {
			t.Fatalf("NormalizePathKey(%q) = %q, want %q", test.in, got, test.want)
		}
	}
	// No os.Stat/EvalSymlinks/canonicalization is involved; spelling stays local.
	if got := NormalizePathKey(filepath.Join("/var", "project")); got != "/var/project" {
		t.Fatalf("alias spelling changed: %q", got)
	}
}

func TestResolveSessionSourceLocatorMatchesMeasuredHerdrShapes(t *testing.T) {
	tests := []struct {
		agent AgentID
		kind  string
		value string
		want  SourceLocatorKind
	}{
		{AgentPi, "path", "/Users/x/.pi/sessions/a.jsonl", LocatorFilePath},
		{AgentClaudeCode, "id", "claude-1", LocatorNativeID},
		{AgentCodex, "id", "codex-1", LocatorNativeID},
		{AgentKimi, "id", "kimi-1", LocatorNativeID},
		{AgentCursor, "id", "cursor-1", LocatorNativeID},
		{AgentAntigravity, "id", "agy-1", LocatorNativeID},
		{AgentGemini, "id", "gemini-1", LocatorMetadataOnly},
		{AgentClaudeCode, "sqlite", "x", LocatorMetadataOnly},
	}
	for _, test := range tests {
		got := ResolveSessionSourceLocator(test.agent, test.kind, "herdr:"+string(test.agent), test.value)
		if got.Kind != test.want || got.Agent != test.agent || got.NativeIdentity() != test.value {
			t.Fatalf("locator(%s,%s) = %#v, want %s", test.agent, test.kind, got, test.want)
		}
	}
}
