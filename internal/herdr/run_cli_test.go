package herdr

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

// fakeHerdr writes an executable stand-in for the Herdr CLI and points
// HERDR_BIN_PATH at it, the same discovery every other CLI call uses.
func fakeHerdr(t *testing.T, script string) {
	t.Helper()
	if runtime.GOOS == "windows" {
		t.Skip("shell script fixture")
	}
	path := filepath.Join(t.TempDir(), "herdr")
	if err := os.WriteFile(path, []byte("#!/bin/sh\n"+script+"\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("HERDR_BIN_PATH", path)
}

// A GUI-launched app has a minimal PATH, so callers must reach the CLI the
// way the rest of the adapter does instead of by bare command name.
func TestRunCLIRunsTheResolvedBinary(t *testing.T) {
	fakeHerdr(t, `echo "args: $@"`)
	out, err := RunCLI(context.Background(), []string{"integration", "status"})
	if err != nil {
		t.Fatal(err)
	}
	if strings.TrimSpace(out) != "args: integration status" {
		t.Fatalf("output = %q", out)
	}
}

func TestRunCLIReportsStderrOnFailure(t *testing.T) {
	fakeHerdr(t, `echo "unknown integration target" >&2; exit 3`)
	_, err := RunCLI(context.Background(), []string{"integration", "install", "nope"})
	if err == nil || !strings.Contains(err.Error(), "unknown integration target") {
		t.Fatalf("err = %v, want the CLI's own stderr", err)
	}
	if errors.Is(err, ErrCLINotFound) {
		t.Fatalf("a failing command must not read as a missing CLI: %v", err)
	}
}
