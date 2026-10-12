package agenthooks

import (
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"testing"
)

func newTestHome(t *testing.T) string {
	t.Helper()
	return t.TempDir()
}

func findRow(t *testing.T, rows []Diagnosis, id ProviderID) Diagnosis {
	t.Helper()
	for _, row := range rows {
		if row.Provider == id {
			return row
		}
	}
	t.Fatalf("no diagnosis row for %s", id)
	return Diagnosis{}
}

func diagnose(t *testing.T, home string) []Diagnosis {
	t.Helper()
	rows := NewManagerWithHome(home).Diagnose()
	if len(rows) == 0 {
		t.Fatal("Diagnose returned no rows")
	}
	return rows
}

func TestDiagnoseFreshHome(t *testing.T) {
	rows := diagnose(t, newTestHome(t))

	claude := findRow(t, rows, ProviderClaude)
	if claude.State != StateNotInstalled {
		t.Fatalf("claude state = %q, want not-installed", claude.State)
	}
	if len(claude.Missing) != len(claudeHookEvents) {
		t.Fatalf("claude missing = %v, want all %d events", claude.Missing, len(claudeHookEvents))
	}
	if !claude.Actionable() || claude.ActionLabel() != "Install" {
		t.Fatalf("claude action = %q, want Install", claude.ActionLabel())
	}

	antigravity := findRow(t, rows, ProviderAntigravity)
	if antigravity.State != StateNotInstalled || len(antigravity.Missing) != len(antigravityHookEvents) {
		t.Fatalf("antigravity = %+v, want not-installed with all events missing", antigravity)
	}

	aider := findRow(t, rows, ProviderAider)
	if aider.State != StateUnsupported || aider.Actionable() {
		t.Fatalf("aider = %+v, want non-actionable unsupported", aider)
	}
}

func TestInstallClaudeEndToEnd(t *testing.T) {
	home := newTestHome(t)
	settingsDir := filepath.Join(home, ".claude")
	if err := os.MkdirAll(settingsDir, 0o755); err != nil {
		t.Fatal(err)
	}
	seed := `{
  "model": "opus",
  "runDepth": 3,
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {"type": "command", "command": "npx block-no-verify@1.1.2"}
        ]
      }
    ],
    "Stop": [
      {"hooks": [{"type": "command", "command": "\"/Applications/Other.app/MacOS/other\" agent-hook claude stop"}]}
    ]
  },
  "nested": {"path": "/tmp/x", "enabled": true, "runDepth": 0}
}`
	if err := os.WriteFile(filepath.Join(settingsDir, "settings.json"), []byte(seed), 0o644); err != nil {
		t.Fatal(err)
	}

	m := NewManagerWithHome(home)
	if _, err := m.Install(ProviderClaude); err != nil {
		t.Fatalf("Install claude: %v", err)
	}

	// Script landed executable with the current version marker.
	script := filepath.Join(home, ".shardlane", "hooks", "shardlane-claude-hook.sh")
	data, err := os.ReadFile(script)
	if err != nil {
		t.Fatalf("script: %v", err)
	}
	if string(data) != hookScriptTemplate {
		t.Fatal("installed script differs from template")
	}
	info, err := os.Stat(script)
	if err != nil {
		t.Fatal(err)
	}
	if runtime.GOOS != "windows" && info.Mode().Perm()&0o111 == 0 {
		t.Fatal("script is not executable")
	}

	// Wiring covers exactly the proven event set, appended beside the
	// foreign entries.
	var settings map[string]any
	decode(t, settingsDir+"/settings.json", &settings)
	hooks := settings["hooks"].(map[string]any)
	for _, event := range claudeHookEvents {
		entries := arrayOf(hooks[event])
		if !wiringContains(entries, "shardlane-claude-hook.sh") {
			t.Errorf("event %s not wired", event)
		}
	}
	// Foreign content survives: the seeded Bash matcher entry and the Stop
	// foreign command are still present exactly once.
	pre := arrayOf(hooks["PreToolUse"])
	if len(pre) != 2 {
		t.Fatalf("PreToolUse entries = %d, want 2 (foreign + ours)", len(pre))
	}
	if !wiringContains(pre, "block-no-verify") {
		t.Fatal("foreign PreToolUse entry lost")
	}
	stop := arrayOf(hooks["Stop"])
	if len(stop) != 2 || !wiringContains(stop, "Other.app") {
		t.Fatalf("Stop entries = %v, want foreign preserved + ours added", stop)
	}
	// Non-hook keys and numeric fidelity survive.
	if settings["model"] != "opus" {
		t.Fatal("top-level key lost")
	}
	nested := settings["nested"].(map[string]any)
	if nested["path"] != "/tmp/x" || nested["enabled"] != true {
		t.Fatalf("nested = %v, want path+enabled preserved", nested)
	}
	if got := numberInt(t, nested["runDepth"]); got != 0 {
		t.Fatalf("nested.runDepth = %v, want integer 0 preserved", got)
	}
	if got := numberInt(t, settings["runDepth"]); got != 3 {
		t.Fatalf("runDepth = %v, want integer 3 preserved", got)
	}

	// Rolling backup exists beside the original.
	if _, err := os.Stat(filepath.Join(settingsDir, "settings.json.shardlane-hook-backup")); err != nil {
		t.Fatalf("backup: %v", err)
	}

	// Idempotent: a second install changes nothing.
	before := snapshot(t, settingsDir+"/settings.json")
	if _, err := m.Install(ProviderClaude); err != nil {
		t.Fatalf("second Install: %v", err)
	}
	if after := snapshot(t, settingsDir+"/settings.json"); after != before {
		t.Fatal("second Install rewrote wiring")
	}

	row := findRow(t, diagnose(t, home), ProviderClaude)
	if row.State != StateOK || row.Version != hookScriptVersion || len(row.Missing) != 0 {
		t.Fatalf("post-install row = %+v, want ok v%d", row, hookScriptVersion)
	}
}

func TestInstallAntigravityPreservesForeignGroups(t *testing.T) {
	home := newTestHome(t)
	geminiDir := filepath.Join(home, ".gemini", "config")
	if err := os.MkdirAll(geminiDir, 0o755); err != nil {
		t.Fatal(err)
	}
	seed := `{
  "herdr": {
    "PreInvocation": [
      {"command": "bash '/x/herdr-agent-state.sh' session", "timeout": 10, "type": "command"}
    ]
  },
  "orca-status": {
    "PostToolUse": [
      {"matcher": "*", "hooks": [{"command": "/x/orca.sh", "type": "command"}]}
    ]
  }
}`
	path := filepath.Join(geminiDir, "hooks.json")
	if err := os.WriteFile(path, []byte(seed), 0o644); err != nil {
		t.Fatal(err)
	}

	m := NewManagerWithHome(home)
	if _, err := m.Install(ProviderAntigravity); err != nil {
		t.Fatalf("Install antigravity: %v", err)
	}

	var config map[string]any
	decode(t, path, &config)
	group := config["shardlane-hook"].(map[string]any)
	if group["enabled"] != true {
		t.Fatal("shardlane-hook group not enabled")
	}
	for _, event := range antigravityHookEvents {
		if !wiringContains(arrayOf(group[event]), "shardlane-antigravity-hook.sh") {
			t.Errorf("event %s not wired", event)
		}
	}
	// Foreign groups survive untouched.
	herdr := config["herdr"].(map[string]any)
	if !wiringContains(arrayOf(herdr["PreInvocation"]), "herdr-agent-state.sh") {
		t.Fatal("herdr group lost")
	}
	if _, ok := config["orca-status"]; !ok {
		t.Fatal("orca-status group lost")
	}

	// Idempotent.
	before := snapshot(t, path)
	if _, err := m.Install(ProviderAntigravity); err != nil {
		t.Fatalf("second Install: %v", err)
	}
	if after := snapshot(t, path); after != before {
		t.Fatal("second Install rewired antigravity hooks")
	}

	row := findRow(t, diagnose(t, home), ProviderAntigravity)
	if row.State != StateOK || len(row.Missing) != 0 {
		t.Fatalf("post-install row = %+v, want ok", row)
	}
}

func TestInstallCreatesMissingProviderConfigs(t *testing.T) {
	home := newTestHome(t)
	m := NewManagerWithHome(home)
	for _, id := range []ProviderID{ProviderClaude, ProviderAntigravity} {
		if _, err := m.Install(id); err != nil {
			t.Fatalf("Install %s: %v", id, err)
		}
	}
	if _, err := os.Stat(filepath.Join(home, ".claude", "settings.json")); err != nil {
		t.Fatalf("claude settings not created: %v", err)
	}
	if _, err := os.Stat(filepath.Join(home, ".gemini", "config", "hooks.json")); err != nil {
		t.Fatalf("antigravity hooks.json not created: %v", err)
	}
}

func TestInstallRefusesForeignScript(t *testing.T) {
	home := newTestHome(t)
	hooksDir := filepath.Join(home, ".shardlane", "hooks")
	if err := os.MkdirAll(hooksDir, 0o755); err != nil {
		t.Fatal(err)
	}
	foreign := "#!/bin/sh\n# mine, not shardlane\nexit 0\n"
	script := filepath.Join(hooksDir, "shardlane-claude-hook.sh")
	if err := os.WriteFile(script, []byte(foreign), 0o755); err != nil {
		t.Fatal(err)
	}

	if _, err := NewManagerWithHome(home).Install(ProviderClaude); err == nil {
		t.Fatal("Install overwrote a foreign script without error")
	}
	if data, _ := os.ReadFile(script); string(data) != foreign {
		t.Fatal("foreign script content changed")
	}
}

func TestStaleScriptDiagnosedAndUpdated(t *testing.T) {
	home := newTestHome(t)
	m := NewManagerWithHome(home)
	// Start from a fully provisioned state, then downgrade only the script
	// to the previous version: isolates staleness from missing wiring.
	if _, err := m.Install(ProviderClaude); err != nil {
		t.Fatalf("initial Install: %v", err)
	}
	script := filepath.Join(home, ".shardlane", "hooks", "shardlane-claude-hook.sh")
	stale := strings.Replace(hookScriptTemplate,
		"# SHARDLANE_HOOK_VERSION="+itoa(hookScriptVersion),
		"# SHARDLANE_HOOK_VERSION="+itoa(hookScriptVersion-1), 1)
	if err := os.WriteFile(script, []byte(stale), 0o755); err != nil {
		t.Fatal(err)
	}

	row := findRow(t, diagnose(t, home), ProviderClaude)
	if row.State != StateOutdated || row.ActionLabel() != "Update" || row.Version != hookScriptVersion-1 {
		t.Fatalf("row = %+v, want outdated v%d with Update action", row, hookScriptVersion-1)
	}

	if _, err := m.Install(ProviderClaude); err != nil {
		t.Fatalf("Install: %v", err)
	}
	data, _ := os.ReadFile(script)
	if string(data) != hookScriptTemplate {
		t.Fatal("stale script not updated to template")
	}
}

func TestWiringDetectionAcrossShapes(t *testing.T) {
	nested := []any{map[string]any{
		"matcher": "*",
		"hooks":   []any{map[string]any{"type": "command", "command": "bash '/h/shardlane-claude-hook.sh' claude"}},
	}}
	flat := []any{map[string]any{"type": "command", "command": "bash '/h/shardlane-antigravity-hook.sh' agy Stop"}}
	if !wiringContains(nested, "shardlane-claude-hook.sh") {
		t.Fatal("nested shape not detected")
	}
	if !wiringContains(flat, "shardlane-antigravity-hook.sh") {
		t.Fatal("flat shape not detected")
	}
	if wiringContains(flat, "shardlane-claude-hook.sh") {
		t.Fatal("wrong script matched")
	}
}

func decode(t *testing.T, path string, into *map[string]any) {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	dec := json.NewDecoder(strings.NewReader(string(data)))
	dec.UseNumber()
	if err := dec.Decode(into); err != nil {
		t.Fatalf("decode %s: %v", path, err)
	}
}

func snapshot(t *testing.T, path string) string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("snapshot %s: %v", path, err)
	}
	return string(data)
}

func numberInt(t *testing.T, value any) int64 {
	t.Helper()
	n, ok := value.(json.Number)
	if !ok {
		t.Fatalf("value %v (%T) is not a json.Number", value, value)
	}
	out, err := n.Int64()
	if err != nil {
		t.Fatalf("number %s: %v", n, err)
	}
	return out
}

func itoa(v int) string {
	return strconv.Itoa(v)
}
