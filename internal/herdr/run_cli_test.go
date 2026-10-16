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

// TestUpgradeRuntimeLeavesTheOldProtocolBehind walks the decision table that
// keeps the bootstrap page's one-click update out of dead ends: the updater
// runs first; afterwards a server still answering with the old protocol is
// stopped through Herdr's own command, so the next bootstrap starts the
// upgraded binary instead of looping on the compatibility error.
func TestUpgradeRuntimeLeavesTheOldProtocolBehind(t *testing.T) {
	recordScript := `echo "$@ [$HERDR_SESSION]" >> "$UPGRADE_LOG"`
	readLog := func(t *testing.T) []string {
		t.Helper()
		data, err := os.ReadFile(os.Getenv("UPGRADE_LOG"))
		if errors.Is(err, os.ErrNotExist) {
			return nil
		}
		if err != nil {
			t.Fatal(err)
		}
		var lines []string
		for _, line := range strings.Split(strings.TrimSpace(string(data)), "\n") {
			if line != "" {
				lines = append(lines, line)
			}
		}
		return lines
	}

	t.Run("hands off to a compatible server", func(t *testing.T) {
		fakeHerdr(t, recordScript)
		t.Setenv("UPGRADE_LOG", filepath.Join(t.TempDir(), "calls.log"))
		home := shortHome(t)
		t.Setenv("HOME", home)
		if err := os.MkdirAll(filepath.Join(home, ".config", "herdr"), 0o755); err != nil {
			t.Fatal(err)
		}
		servePings(t, SocketPath("default"), "0.9.3", 22)
		if err := UpgradeRuntime(context.Background(), "default"); err != nil {
			t.Fatalf("UpgradeRuntime = %v", err)
		}
		if got := readLog(t); len(got) != 1 || !strings.Contains(got[0], "update --handoff") {
			t.Fatalf("calls = %q, want exactly one update and no stop", got)
		}
	})

	t.Run("stops a server that survived the handoff", func(t *testing.T) {
		fakeHerdr(t, recordScript)
		t.Setenv("UPGRADE_LOG", filepath.Join(t.TempDir(), "calls.log"))
		home := shortHome(t)
		t.Setenv("HOME", home)
		if err := os.MkdirAll(filepath.Join(home, ".config", "herdr"), 0o755); err != nil {
			t.Fatal(err)
		}
		servePings(t, SocketPath("default"), "0.8.2", 20)
		if err := UpgradeRuntime(context.Background(), "default"); err != nil {
			t.Fatalf("UpgradeRuntime = %v", err)
		}
		got := readLog(t)
		if len(got) != 2 || !strings.Contains(got[0], "update --handoff") || !strings.Contains(got[1], "server stop") {
			t.Fatalf("calls = %q, want update then server stop", got)
		}
	})

	t.Run("scopes the stop to the failing session", func(t *testing.T) {
		fakeHerdr(t, recordScript)
		t.Setenv("UPGRADE_LOG", filepath.Join(t.TempDir(), "calls.log"))
		home := shortHome(t)
		t.Setenv("HOME", home)
		socket := SocketPath("work")
		if err := os.MkdirAll(filepath.Dir(socket), 0o755); err != nil {
			t.Fatal(err)
		}
		servePings(t, socket, "0.8.2", 20)
		if err := UpgradeRuntime(context.Background(), "work"); err != nil {
			t.Fatalf("UpgradeRuntime = %v", err)
		}
		got := readLog(t)
		if len(got) != 2 || !strings.Contains(got[1], "server stop [work]") {
			t.Fatalf("calls = %q, want a HERDR_SESSION=work-scoped server stop", got)
		}
	})

	t.Run("no server running", func(t *testing.T) {
		fakeHerdr(t, recordScript)
		t.Setenv("UPGRADE_LOG", filepath.Join(t.TempDir(), "calls.log"))
		t.Setenv("HOME", shortHome(t))
		if err := UpgradeRuntime(context.Background(), "default"); err != nil {
			t.Fatalf("UpgradeRuntime = %v", err)
		}
		if got := readLog(t); len(got) != 1 || !strings.Contains(got[0], "update --handoff") {
			t.Fatalf("calls = %q, want only the update", got)
		}
	})

	t.Run("updater failure surfaces", func(t *testing.T) {
		fakeHerdr(t, recordScript+"\n"+`echo "download interrupted" >&2; exit 1`)
		t.Setenv("UPGRADE_LOG", filepath.Join(t.TempDir(), "calls.log"))
		t.Setenv("HOME", shortHome(t))
		err := UpgradeRuntime(context.Background(), "default")
		if err == nil || !strings.Contains(err.Error(), "download interrupted") {
			t.Fatalf("err = %v, want the updater's own stderr", err)
		}
		if got := readLog(t); len(got) != 1 {
			t.Fatalf("calls = %q, want only the failed update", got)
		}
	})

	// A system-location install (/usr/bin) is the one failure the app
	// cannot fix by itself: UpgradeRuntime asks the OS for the user's
	// authorization (pkexec on Linux) and finishes the very same updater
	// in place when it is granted.
	t.Run("escalates through the authorizer when not writable", func(t *testing.T) {
		fakeHerdr(t, recordScript+"\n"+`echo "update failed: install directory not writable: /usr/bin (Permission denied (os error 13)). Try running with appropriate permissions." >&2; exit 1`)
		bin := t.TempDir()
		if err := os.WriteFile(filepath.Join(bin, "pkexec"), []byte("#!/bin/sh\necho \"$@\" >> \"$UPGRADE_LOG\"\n"), 0o755); err != nil {
			t.Fatal(err)
		}
		t.Setenv("PATH", bin+string(os.PathListSeparator)+os.Getenv("PATH"))
		t.Setenv("UPGRADE_LOG", filepath.Join(t.TempDir(), "calls.log"))
		t.Setenv("HOME", shortHome(t))
		if err := UpgradeRuntime(context.Background(), "default"); err != nil {
			t.Fatalf("UpgradeRuntime = %v", err)
		}
		got := readLog(t)
		if len(got) != 2 || !strings.Contains(got[1], "--disable-internal-agent") || !strings.Contains(got[1], "update --handoff") {
			t.Fatalf("calls = %q, want the refused update then the pkexec authorizer", got)
		}
	})

	t.Run("dismissed authorization falls back to the way out", func(t *testing.T) {
		fakeHerdr(t, recordScript+"\n"+`echo "update failed: install directory not writable: /usr/bin (Permission denied (os error 13)). Try running with appropriate permissions." >&2; exit 1`)
		bin := t.TempDir()
		if err := os.WriteFile(filepath.Join(bin, "pkexec"), []byte("#!/bin/sh\necho \"$@\" >> \"$UPGRADE_LOG\"\necho \"dismissed\" >&2\nexit 1\n"), 0o755); err != nil {
			t.Fatal(err)
		}
		t.Setenv("PATH", bin+string(os.PathListSeparator)+os.Getenv("PATH"))
		t.Setenv("UPGRADE_LOG", filepath.Join(t.TempDir(), "calls.log"))
		t.Setenv("HOME", shortHome(t))
		err := UpgradeRuntime(context.Background(), "default")
		if !errors.Is(err, ErrUpdateNotWritable) {
			t.Fatalf("err = %v, want typed ErrUpdateNotWritable after a dismissed dialog", err)
		}
	})
}
