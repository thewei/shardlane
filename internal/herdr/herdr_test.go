package herdr

import (
	"encoding/json"
	"errors"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync/atomic"
	"testing"
)

func TestNormalizeSession(t *testing.T) {
	if got := normalizeSession(""); got != "default" {
		t.Fatalf("normalize empty = %q", got)
	}
	if got := normalizeSession(" work "); got != "work" {
		t.Fatalf("normalize named = %q", got)
	}
}

func TestSocketPathShape(t *testing.T) {
	defaultPath := SocketPath("default")
	if filepath.Base(defaultPath) != "herdr.sock" {
		t.Fatalf("default socket = %q", defaultPath)
	}
	named := SocketPath("work")
	wantTail := filepath.Join("sessions", "work", "herdr.sock")
	if len(named) < len(wantTail) || named[len(named)-len(wantTail):] != wantTail {
		t.Fatalf("named socket = %q, want tail %q", named, wantTail)
	}
}

func TestPingValidatesProtocol(t *testing.T) {
	path := filepath.Join(t.TempDir(), "herdr.sock")
	servePings(t, path, "0.9.3", 22)
	got, err := ping(path)
	if err != nil {
		t.Fatal(err)
	}
	if got.Protocol != 22 || got.Version != "0.9.3" {
		t.Fatalf("ping = %#v", got)
	}
}

func TestPingRejectsOldProtocol(t *testing.T) {
	path := filepath.Join(t.TempDir(), "herdr.sock")
	servePings(t, path, "0.8.x", 19)
	if _, err := ping(path); !errors.Is(err, ErrProtocolTooOld) || !strings.Contains(err.Error(), "need 22+") {
		t.Fatalf("ping error = %v, want typed ErrProtocolTooOld with the range", err)
	}
}

// servePings answers ping RPCs on a unix socket with one fixed identity for
// the whole test and returns how many pings arrived; it is the shared
// fixture for the protocol-gate regressions.
func servePings(t *testing.T, path, version string, protocol int) *atomic.Int32 {
	t.Helper()
	if runtime.GOOS == "windows" {
		t.Skip("Unix socket fixture")
	}
	listener, err := net.Listen("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { listener.Close() })
	var pings atomic.Int32
	go func() {
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			pings.Add(1)
			var request map[string]any
			_ = json.NewDecoder(conn).Decode(&request)
			_ = json.NewEncoder(conn).Encode(map[string]any{
				"result": map[string]any{"version": version, "protocol": protocol},
			})
			conn.Close()
		}
	}()
	return &pings
}

// shortHome returns a HOME whose path is short enough for unix socket binds
// (t.TempDir nests under the test name and exceeds the socket length limit).
func shortHome(t *testing.T) string {
	t.Helper()
	home, err := os.MkdirTemp("", "herdr-home")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.RemoveAll(home) })
	return home
}

// An old but running server must read as a compatibility error, and
// EnsureRunning must never spawn a second server on top of it: exactly one
// ping reaches the socket, then the adapter gives up with ErrProtocolTooOld.
func TestEnsureRunningRejectsOldServerWithoutSpawning(t *testing.T) {
	home := shortHome(t)
	t.Setenv("HOME", home)
	t.Setenv("HERDR_BIN_PATH", filepath.Join(t.TempDir(), "herdr-must-not-run"))
	sockDir := filepath.Join(home, ".config", "herdr")
	if err := os.MkdirAll(sockDir, 0o755); err != nil {
		t.Fatal(err)
	}
	socket := filepath.Join(sockDir, "herdr.sock")
	pings := servePings(t, socket, "0.8.2", 20)

	err := (&Manager{}).EnsureRunning("default")
	if !errors.Is(err, ErrProtocolTooOld) {
		t.Fatalf("EnsureRunning error = %v, want typed ErrProtocolTooOld", err)
	}
	if got := pings.Load(); got != 1 {
		t.Fatalf("old server received %d pings, want exactly 1 and no spawn attempt", got)
	}
}

func TestProbeRuntimeReportsLiveIdentity(t *testing.T) {
	home := shortHome(t)
	t.Setenv("HOME", home)
	if err := os.MkdirAll(filepath.Join(home, ".config", "herdr"), 0o755); err != nil {
		t.Fatal(err)
	}
	servePings(t, SocketPath("default"), "0.9.3", 22)
	probe, err := ProbeRuntime("")
	if err != nil {
		t.Fatal(err)
	}
	if probe.Version != "0.9.3" || probe.Protocol != 22 {
		t.Fatalf("probe = %#v", probe)
	}
}

// An old-but-running runtime still reports its identity: the caller needs
// it to offer the update, not to refuse the connection.
func TestProbeRuntimeKeepsOldIdentityReachable(t *testing.T) {
	home := shortHome(t)
	t.Setenv("HOME", home)
	if err := os.MkdirAll(filepath.Join(home, ".config", "herdr"), 0o755); err != nil {
		t.Fatal(err)
	}
	servePings(t, SocketPath("default"), "0.8.2", 20)
	probe, err := ProbeRuntime("")
	if err != nil {
		t.Fatal(err)
	}
	if probe.Protocol != 20 || probe.Version != "0.8.2" {
		t.Fatalf("probe = %#v, want the old identity without a gate", probe)
	}
}

func TestProbeRuntimeReportsUnreachable(t *testing.T) {
	t.Setenv("HOME", shortHome(t))
	if _, err := ProbeRuntime(""); err == nil {
		t.Fatal("an unreachable runtime must surface as an error")
	}
}

func TestTerminalAttachArgsTargetsPaneWithoutTakeover(t *testing.T) {
	args := terminalAttachArgs("/usr/local/bin/herdr", "/tmp/herdr.sock", "/tmp/config.toml", "term-42")
	joined := strings.Join(args, " ")
	for _, want := range []string{
		"HERDR_SOCKET_PATH=/tmp/herdr.sock",
		"HERDR_CONFIG_PATH=/tmp/config.toml",
		"/usr/local/bin/herdr terminal attach term-42",
	} {
		if !strings.Contains(joined, want) {
			t.Fatalf("attach args %q missing %q", joined, want)
		}
	}
	if strings.Contains(joined, "--takeover") {
		t.Fatalf("attach args must never take over an unrelated controller: %q", joined)
	}
}
