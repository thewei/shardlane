package herdr

import (
	"bufio"
	"encoding/json"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"testing"
)

func TestSplitPaneUsesVerifiedMutationThenSnapshot(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix socket fixture")
	}
	home, err := os.MkdirTemp("/tmp", "shardlane-actions-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(home) })
	t.Setenv("HOME", home)
	path := filepath.Join(home, ".config", "herdr", "herdr.sock")
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()

	type seenRequest struct {
		Method string         `json:"method"`
		Params map[string]any `json:"params"`
	}
	var mu sync.Mutex
	seen := make([]seenRequest, 0, 3)
	go func() {
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			go func(conn net.Conn) {
				defer conn.Close()
				line, err := bufio.NewReader(conn).ReadBytes('\n')
				if err != nil {
					return
				}
				var request seenRequest
				if json.Unmarshal(line, &request) != nil {
					return
				}
				mu.Lock()
				seen = append(seen, request)
				mu.Unlock()
				var result any
				switch request.Method {
				case "ping":
					result = map[string]any{"type": "pong", "version": "0.9.3", "protocol": 22}
				case "pane.split":
					result = map[string]any{"type": "pane_created"}
				case "session.snapshot":
					result = map[string]any{
						"type": "session_snapshot",
						"snapshot": map[string]any{
							"version": "0.9.3", "protocol": 22,
							"workspaces": []any{}, "tabs": []any{}, "panes": []any{}, "layouts": []any{}, "agents": []any{},
						},
					}
				default:
					result = map[string]any{"type": "ok"}
				}
				_ = json.NewEncoder(conn).Encode(map[string]any{"result": result})
			}(conn)
		}
	}()

	if _, err := NewManager().SplitPane("default", "p1", "right"); err != nil {
		t.Fatal(err)
	}
	mu.Lock()
	defer mu.Unlock()
	if len(seen) != 3 {
		t.Fatalf("requests = %#v", seen)
	}
	wantMethods := []string{"ping", "pane.split", "session.snapshot"}
	for index, method := range wantMethods {
		if seen[index].Method != method {
			t.Fatalf("request[%d] method = %q, want %q", index, seen[index].Method, method)
		}
	}
	params := seen[1].Params
	if params["target_pane_id"] != "p1" || params["direction"] != "right" || params["focus"] != true {
		t.Fatalf("pane.split params = %#v", params)
	}
}
