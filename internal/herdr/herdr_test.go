package herdr

import (
	"encoding/json"
	"net"
	"path/filepath"
	"runtime"
	"strings"
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
	if runtime.GOOS == "windows" {
		t.Skip("Unix socket fixture")
	}
	path := filepath.Join(t.TempDir(), "herdr.sock")
	listener, err := net.Listen("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	go func() {
		conn, err := listener.Accept()
		if err != nil {
			return
		}
		defer conn.Close()
		var request map[string]any
		_ = json.NewDecoder(conn).Decode(&request)
		_ = json.NewEncoder(conn).Encode(map[string]any{
			"result": map[string]any{"version": "0.9.3", "protocol": 22},
		})
	}()

	got, err := ping(path)
	if err != nil {
		t.Fatal(err)
	}
	if got.Protocol != 22 || got.Version != "0.9.3" {
		t.Fatalf("ping = %#v", got)
	}
}

func TestPingRejectsOldProtocol(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix socket fixture")
	}
	path := filepath.Join(t.TempDir(), "herdr.sock")
	listener, err := net.Listen("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	go func() {
		conn, err := listener.Accept()
		if err != nil {
			return
		}
		defer conn.Close()
		var request map[string]any
		_ = json.NewDecoder(conn).Decode(&request)
		_ = json.NewEncoder(conn).Encode(map[string]any{
			"result": map[string]any{"version": "0.8.x", "protocol": 19},
		})
	}()

	if _, err := ping(path); err == nil || !strings.Contains(err.Error(), "need 22+") {
		t.Fatalf("ping error = %v", err)
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
