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

// TestScrollPaneUsesVerifiedMutationWithoutSnapshot pins the pane.scroll
// contract against the live protocol schema (PaneScrollParams: pane_id +
// offset_from_bottom) and the bounded-frequency rule: scrolling never
// refetches the session snapshot — the event watcher reconciles instead.
func TestScrollPaneUsesVerifiedMutationWithoutSnapshot(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix socket fixture")
	}
	home, err := os.MkdirTemp("/tmp", "shardlane-scroll-")
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
	seen := make([]seenRequest, 0, 2)
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
				var result any = map[string]any{"type": "ok"}
				if request.Method == "ping" {
					result = map[string]any{"type": "pong", "version": "0.9.3", "protocol": 22}
				}
				_ = json.NewEncoder(conn).Encode(map[string]any{"result": result})
			}(conn)
		}
	}()

	if err := NewManager().ScrollPane("default", "w1:p1", 42); err != nil {
		t.Fatal(err)
	}
	if err := NewManager().ScrollPane("default", "  ", 1); err == nil {
		t.Fatal("empty pane id must be rejected before any RPC")
	}
	mu.Lock()
	defer mu.Unlock()
	if len(seen) != 2 {
		t.Fatalf("requests = %#v", seen)
	}
	if seen[0].Method != "ping" {
		t.Fatalf("request[0] method = %q, want ping", seen[0].Method)
	}
	if seen[1].Method != "pane.scroll" {
		t.Fatalf("request[1] method = %q, want pane.scroll", seen[1].Method)
	}
	params := seen[1].Params
	if params["pane_id"] != "w1:p1" || params["offset_from_bottom"] != float64(42) {
		t.Fatalf("pane.scroll params = %#v", params)
	}
}

// TestPaneScrollProjectionParsesRuntimeMetrics pins the snapshot projection
// of PaneInfo.scroll, the runtime's authoritative viewport metrics.
func TestPaneScrollProjectionParsesRuntimeMetrics(t *testing.T) {
	snapshot := sessionSnapshot{
		Protocol: 22,
		Panes: []paneInfo{
			{ID: "p1", Scroll: &PaneScroll{OffsetFromBottom: 7, MaxOffsetFromBottom: 9000, ViewportRows: 39}},
			{ID: "p2"},
		},
	}
	projection := projectSnapshot(snapshot)
	if len(projection.Panes) != 2 {
		t.Fatalf("panes = %d, want 2", len(projection.Panes))
	}
	scroll := projection.Panes[0].Scroll
	if scroll == nil || scroll.OffsetFromBottom != 7 || scroll.MaxOffsetFromBottom != 9000 || scroll.ViewportRows != 39 {
		t.Fatalf("p1 scroll = %#v", scroll)
	}
	if projection.Panes[1].Scroll != nil {
		t.Fatalf("p2 scroll = %#v, want nil", projection.Panes[1].Scroll)
	}
}
