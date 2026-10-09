package agent

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestPiExecutableRequiresExplicitAbsoluteExecutable(t *testing.T) {
	if _, err := ResolvePiExecutable("relative/pi"); err == nil {
		t.Fatal("relative executable accepted")
	}
	if _, err := ResolvePiExecutable(filepath.Join(t.TempDir(), "missing")); err == nil {
		t.Fatal("missing Pi accepted")
	}
}

func TestPiDiscoveryFromWorkspaceAndNodeRuntime(t *testing.T) {
	home := t.TempDir()
	t.Setenv("HOME", home)
	t.Setenv("PATH", "/usr/bin:/bin")
	binary := filepath.Join(home, "Workspaces", "team", "devspace", "node_modules", ".bin", "pi")
	if err := os.MkdirAll(filepath.Dir(binary), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(binary, []byte("#!/bin/sh\nexit 0\n"), 0o700); err != nil {
		t.Fatal(err)
	}
	resolved, err := ResolvePiExecutable("")
	if err != nil || resolved != binary {
		t.Fatalf("Pi discovery = %q, %v", resolved, err)
	}
	node := filepath.Join(home, ".vite-plus", "js_runtime", "node", "24.21.0", "bin", "node")
	if err := os.MkdirAll(filepath.Dir(node), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(node, []byte("#!/bin/sh\nexit 0\n"), 0o700); err != nil {
		t.Fatal(err)
	}
	env := piRuntimeEnv(binary)
	lastPath := ""
	for _, entry := range env {
		if strings.HasPrefix(entry, "PATH=") {
			lastPath = strings.TrimPrefix(entry, "PATH=")
		}
	}
	if !strings.Contains(lastPath, filepath.Dir(node)) || !strings.Contains(lastPath, filepath.Dir(binary)) {
		t.Fatalf("Finder-style Pi environment lacks Node or Pi path: %q", lastPath)
	}
}

func TestPiCompletionIsToolFreeAndEphemeral(t *testing.T) {
	root := t.TempDir()
	binary := filepath.Join(t.TempDir(), "fake-pi")
	script := `#!/bin/sh
for arg in "$@"; do
 case "$arg" in
  "--no-tools") tools=1 ;;
  "--no-session") session=1 ;;
  "--no-extensions") extensions=1 ;;
  "--no-context-files") no_context=1 ;;
 esac
done
[ "$tools" = "1" ] && [ "$session" = "1" ] && [ "$extensions" = "1" ] && [ "$no_context" = "1" ] || exit 4
[ "$PWD" = "$EXPECTED_PI_ROOT" ] || exit 5
input=$(cat)
case "$input" in *"SAFE REVIEW PAYLOAD"*) printf 'fix: improve git workflow\n\n- preserve changes\n';; *) exit 6;; esac
`
	if err := os.WriteFile(binary, []byte(script), 0o700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("EXPECTED_PI_ROOT", root)
	result, err := GeneratePiCommitSuggestion(context.Background(), PiCommitRequest{Executable: binary, Root: root, Prompt: "SAFE REVIEW PAYLOAD"})
	if err != nil {
		t.Fatal(err)
	}
	if result.Message != "fix: improve git workflow\n\n- preserve changes" {
		t.Fatalf("unexpected Pi output %q", result.Message)
	}
}

func TestPiBoundedOutputAndInput(t *testing.T) {
	root := t.TempDir()
	binary := filepath.Join(t.TempDir(), "fake-pi")
	if err := os.WriteFile(binary, []byte("#!/bin/sh\nhead -c 10000 /dev/zero | tr '\\0' 'A'\n"), 0o700); err != nil {
		t.Fatal(err)
	}
	_, err := GeneratePiCommitSuggestion(context.Background(), PiCommitRequest{Executable: binary, Root: root, Prompt: "short"})
	if err == nil || !strings.Contains(err.Error(), "limit") {
		t.Fatalf("oversize response should fail: %v", err)
	}
	_, err = GeneratePiCommitSuggestion(context.Background(), PiCommitRequest{Executable: binary, Root: root, Prompt: strings.Repeat("x", MaxPiCommitInputBytes+1)})
	if err == nil {
		t.Fatal("oversize input accepted")
	}
}
