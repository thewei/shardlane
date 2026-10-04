package diagnostics

import (
	"encoding/json"
	"strings"
	"testing"
)

// TestSanitizeTextStripsSecretsAndHome pins P17 redaction rules:
// home path normalized, API tokens/passwords masked.
func TestSanitizeTextStripsSecretsAndHome(t *testing.T) {
	home := "/Users/developer"
	input := "Failed connecting with api_key=sk-secret1234567890 at /Users/developer/project/file.go"
	output := SanitizeText(input, home)

	if strings.Contains(output, "/Users/developer") {
		t.Fatalf("home directory was not redacted: %q", output)
	}
	if !strings.Contains(output, "~/project/file.go") {
		t.Fatalf("home directory was not replaced with ~: %q", output)
	}
	if strings.Contains(output, "sk-secret1234567890") {
		t.Fatalf("API key was not redacted: %q", output)
	}
	if !strings.Contains(output, "api_key=REDACTED") {
		t.Fatalf("expected api_key=REDACTED, got: %q", output)
	}
}

// TestBuildSnapshotContainsPlatformFacts pins P16 snapshot facts.
func TestBuildSnapshotContainsPlatformFacts(t *testing.T) {
	snap := BuildSnapshot("0.9.1", 22, "inst-dev", "/Users/demo")

	// The app version comes from the packaged bundle (mygo.json → App.Version);
	// unpackaged test binaries report the "dev" fallback, never a stale copy.
	if snap.ShardlaneVersion == "" || snap.ShardlaneVersion == "0.9.0" {
		t.Fatalf("version = %q, want the bundle version or the dev fallback", snap.ShardlaneVersion)
	}
	if snap.MyGoVersion == "" {
		t.Fatalf("MyGo version missing: %+v", snap)
	}
	if snap.HerdrProtocol != 22 {
		t.Fatalf("protocol = %d, want 22", snap.HerdrProtocol)
	}
	if snap.OS == "" || snap.Arch == "" || snap.GoVersion == "" {
		t.Fatalf("missing runtime facts: %+v", snap)
	}
	summary := FormatSnapshotSummary(snap)
	if !strings.Contains(summary, "Shardlane v"+snap.ShardlaneVersion) || !strings.Contains(summary, "proto 22") {
		t.Fatalf("summary formatting mismatch: %q", summary)
	}
}

// TestFilterLogsByLevelAndSearch pins P16 in-memory filtering.
func TestFilterLogsByLevelAndSearch(t *testing.T) {
	entries := []LogEntry{
		{Time: "10:00", Level: "INFO", Message: "agent started", Raw: `{"msg":"agent started"}`},
		{Time: "10:01", Level: "WARN", Message: "connection retry", Raw: `{"msg":"connection retry"}`},
		{Time: "10:02", Level: "ERROR", Message: "failed to attach terminal", Raw: `{"msg":"failed to attach terminal"}`},
		{Time: "10:03", Level: "INFO", Message: "pane refreshed", Raw: `{"msg":"pane refreshed"}`},
	}

	// Filter by level
	errs := FilterLogs(entries, "ERROR", "")
	if len(errs) != 1 || errs[0].Message != "failed to attach terminal" {
		t.Fatalf("expected 1 ERROR entry, got %+v", errs)
	}

	// Filter by keyword
	terminals := FilterLogs(entries, "ALL", "terminal")
	if len(terminals) != 1 {
		t.Fatalf("expected 1 entry matching 'terminal', got %d", len(terminals))
	}

	// Filter by level and keyword
	retry := FilterLogs(entries, "WARN", "retry")
	if len(retry) != 1 {
		t.Fatalf("expected 1 entry for WARN retry, got %d", len(retry))
	}
}

// TestBuildExportArchiveSanitization pins P17 export artifact compliance.
func TestBuildExportArchiveSanitization(t *testing.T) {
	home := "/Users/tester"
	snap := BuildSnapshot("0.9.1", 22, "inst-1", home)
	rawLogs := []LogEntry{
		{Time: "12:00", Level: "INFO", Message: "Loaded /Users/tester/.zcode/config.toml with token=secret_token_12345"},
	}

	jsonStr, err := BuildExportArchive(snap, rawLogs, home)
	if err != nil {
		t.Fatal(err)
	}

	if strings.Contains(jsonStr, "/Users/tester") {
		t.Fatal("exported archive contains unredacted home directory")
	}
	if strings.Contains(jsonStr, "secret_token_12345") {
		t.Fatal("exported archive contains unredacted credential")
	}

	var parsed ExportArchive
	if err := json.Unmarshal([]byte(jsonStr), &parsed); err != nil {
		t.Fatalf("export archive must be valid JSON: %v", err)
	}
	if len(parsed.Logs) != 1 {
		t.Fatalf("expected 1 log in archive, got %d", len(parsed.Logs))
	}
}
