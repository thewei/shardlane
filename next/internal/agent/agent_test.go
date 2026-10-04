package agent

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

func TestCapabilityRowsMatchRustAuthorities(t *testing.T) {
	ready := func(agent history.AgentID) Environment { return Environment{Enabled: true, Installed: true} }

	claude := ProjectCapabilities(history.AgentClaudeCode, ready(history.AgentClaudeCode))
	if !claude.Startable || !claude.SemanticLive || !claude.NativeResume {
		t.Fatalf("claude row = %+v", claude)
	}
	if claude.Exposure != "stable" || claude.PlanMode != "native-cli" || !claude.PermissionModes {
		t.Fatalf("claude facts = %+v", claude)
	}
	if claude.UnavailableReason != "" {
		t.Fatalf("ready claude must have no unavailable reason: %q", claude.UnavailableReason)
	}

	codex := ProjectCapabilities(history.AgentCodex, ready(history.AgentCodex))
	if codex.PlanMode != "post-launch-keys" || codex.BridgeKind != "command-hook" {
		t.Fatalf("codex facts = %+v", codex)
	}

	gemini := ProjectCapabilities(history.AgentGemini, ready(history.AgentGemini))
	// Rust parity: startable reflects the integration strategy only; a hidden
	// provider is gated for presentation through unavailable_reason.
	if !gemini.Startable || gemini.SemanticLive {
		t.Fatalf("hidden gemini row = %+v", gemini)
	}
	if gemini.UnavailableReason != "Gemini CLI is not available in this build" {
		t.Fatalf("gemini reason = %q", gemini.UnavailableReason)
	}

	qoder := ProjectCapabilities(history.AgentQoder, ready(history.AgentQoder))
	// Rust parity: Qoder is Preview-exposed with a Deferred integration, so
	// the deferred reason gates it.
	if qoder.Startable {
		t.Fatal("deferred integration must not be startable")
	}
	if qoder.UnavailableReason != "Qoder runtime integration is pending" {
		t.Fatalf("qoder reason = %q", qoder.UnavailableReason)
	}

	disabled := ProjectCapabilities(history.AgentClaudeCode, Environment{Enabled: false, Installed: true})
	if disabled.Startable || disabled.UnavailableReason != "Claude Code is disabled in Settings" {
		t.Fatalf("disabled row = %+v", disabled)
	}
	uninstalled := ProjectCapabilities(history.AgentClaudeCode, Environment{Enabled: true, Installed: false})
	if uninstalled.Startable || uninstalled.UnavailableReason != "Setup required" {
		t.Fatalf("uninstalled row = %+v", uninstalled)
	}
}

func TestProjectAllCapabilitiesCoversEveryProvider(t *testing.T) {
	rows := ProjectAllCapabilities(func(history.AgentID) Environment { return Environment{Enabled: true, Installed: true} })
	if len(rows) != len(history.AllAgents) {
		t.Fatalf("rows = %d, want %d", len(rows), len(history.AllAgents))
	}
	seen := make(map[history.AgentID]bool)
	for _, row := range rows {
		if row.Provider == "" || row.DisplayName == "" {
			t.Fatalf("incomplete row = %+v", row)
		}
		if seen[row.Provider] {
			t.Fatalf("duplicate provider %q", row.Provider)
		}
		seen[row.Provider] = true
	}
}

func TestStartAgentRequestValidationAndWire(t *testing.T) {
	valid := StartAgentRequest{
		RequestID: "req-1",
		Instance:  "default",
		ProjectID: "w1",
		Provider:  history.AgentClaudeCode,
		CWD:       "/work/demo",
		Prompt:    "fix the build",
	}
	if err := valid.Validate(); err != nil {
		t.Fatalf("valid request rejected: %v", err)
	}

	data, err := json.Marshal(valid)
	if err != nil {
		t.Fatal(err)
	}
	var roundTrip StartAgentRequest
	if err := json.Unmarshal(data, &roundTrip); err != nil {
		t.Fatal(err)
	}
	if roundTrip != valid {
		t.Fatalf("wire round trip = %+v", roundTrip)
	}

	cases := []struct {
		name    string
		mutate  func(*StartAgentRequest)
		wantErr string
	}{
		{"missing request id", func(r *StartAgentRequest) { r.RequestID = "" }, "request id"},
		{"missing instance", func(r *StartAgentRequest) { r.Instance = "" }, "instance"},
		{"missing project", func(r *StartAgentRequest) { r.ProjectID = "" }, "project id"},
		{"missing cwd", func(r *StartAgentRequest) { r.CWD = "" }, "cwd"},
		{"unknown provider", func(r *StartAgentRequest) { r.Provider = "mystery" }, "unknown provider"},
	}
	for _, tc := range cases {
		request := valid
		tc.mutate(&request)
		err := request.Validate()
		if err == nil || !contains(err.Error(), tc.wantErr) {
			t.Fatalf("%s: err = %v, want containing %q", tc.name, err, tc.wantErr)
		}
	}
}

func TestLaunchRegistryIsIdempotent(t *testing.T) {
	registry := NewLaunchRegistry()
	launches := 0
	request := StartAgentRequest{
		RequestID: "req-1", Instance: "default", ProjectID: "w1",
		Provider: history.AgentCodex, CWD: "/work/demo",
	}
	launch := func(ctx context.Context, r StartAgentRequest) (LaunchOutcomeRecord, error) {
		launches++
		return agentLaunchOutcomeForTest(r), nil
	}

	first, err := registry.Submit(context.Background(), request, launch)
	if err != nil || !first.Launched || launches != 1 {
		t.Fatalf("first submit = (%+v, %v, %d launches)", first, err, launches)
	}
	second, err := registry.Submit(context.Background(), request, launch)
	if err != nil {
		t.Fatal(err)
	}
	if launches != 1 {
		t.Fatalf("duplicate request launched again (%d launches)", launches)
	}
	if second != first {
		t.Fatalf("duplicate outcome = %+v, want %+v", second, first)
	}
	if _, ok := registry.Outcome("req-1"); !ok {
		t.Fatal("outcome not recorded")
	}

	invalid := request
	invalid.RequestID = ""
	if _, err := registry.Submit(context.Background(), invalid, launch); err == nil {
		t.Fatal("invalid request must be rejected before launch")
	}
	if launches != 1 {
		t.Fatalf("invalid request invoked launch (%d launches)", launches)
	}
}

func agentLaunchOutcomeForTest(r StartAgentRequest) LaunchOutcomeRecord {
	return LaunchOutcomeRecord{RequestID: r.RequestID, Launched: true, AgentKey: "pane-1"}
}

func contains(haystack, needle string) bool {
	return len(haystack) >= len(needle) && (haystack == needle || len(needle) == 0 || indexOf(haystack, needle) >= 0)
}

func indexOf(haystack, needle string) int {
	for i := 0; i+len(needle) <= len(haystack); i++ {
		if haystack[i:i+len(needle)] == needle {
			return i
		}
	}
	return -1
}
