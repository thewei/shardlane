package herdr

/**
 * [INPUT]: 依赖本地 Unix Socket fixture 和 protocol 22 Herdr request schema
 * [OUTPUT]: 验证 workspace.move / tab.move / pane.swap 方法、参数与 snapshot 回读
 * [POS]: herdr 结构拖拽动作的契约回归，阻止错误 wire key 和隐式焦点变动
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

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

func TestReorderActionsUseCanonicalHerdrMethods(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix socket fixture")
	}
	home, err := os.MkdirTemp("/tmp", "shardlane-drag-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(home) })
	t.Setenv("HOME", home)
	path := filepath.Join(home, ".config", "herdr", "herdr.sock")
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	type request struct {
		Method string         `json:"method"`
		Params map[string]any `json:"params"`
	}
	var mu sync.Mutex
	var seen []request
	go func() {
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			go func(conn net.Conn) {
				defer conn.Close()
				var req request
				if err := json.NewDecoder(bufio.NewReader(conn)).Decode(&req); err != nil {
					return
				}
				mu.Lock()
				seen = append(seen, req)
				mu.Unlock()
				var result any
				switch req.Method {
				case "ping":
					result = map[string]any{"type": "pong", "version": "0.9.3", "protocol": 22}
				case "session.snapshot":
					result = map[string]any{"type": "session_snapshot", "snapshot": map[string]any{
						"version": "0.9.3", "protocol": 22,
						"workspaces": []any{}, "tabs": []any{}, "panes": []any{},
						"layouts": []any{}, "agents": []any{},
					}}
				default:
					result = map[string]any{"type": "ok"}
				}
				_ = json.NewEncoder(conn).Encode(map[string]any{"result": result})
			}(conn)
		}
	}()

	m := NewManager()
	if _, err := m.MoveProject("default", "w1", 2); err != nil {
		t.Fatal(err)
	}
	if _, err := m.MoveTab("default", "t1", 0); err != nil {
		t.Fatal(err)
	}
	if _, err := m.SwapPanes("default", "p1", "p2"); err != nil {
		t.Fatal(err)
	}
	if _, err := m.MovePane("default", "p1", PaneMoveDestination{
		Type:         "tab",
		TabID:        "t2",
		Split:        "down",
		TargetPaneID: "p3",
	}, true); err != nil {
		t.Fatal(err)
	}
	if _, err := m.MovePane("default", "p1", PaneMoveDestination{
		Type:        "new_tab",
		WorkspaceID: "w2",
		Label:       "new",
	}, false); err != nil {
		t.Fatal(err)
	}
	if _, err := m.ResizePane("default", "p1", "right", 0.05); err != nil {
		t.Fatal(err)
	}
	mu.Lock()
	defer mu.Unlock()
	var mutations []request
	for _, req := range seen {
		if req.Method != "ping" && req.Method != "session.snapshot" {
			mutations = append(mutations, req)
		}
	}
	if len(mutations) != 6 {
		t.Fatalf("mutations = %#v", mutations)
	}
	for i, tc := range []struct {
		method string
		keys   map[string]any
	}{
		{"workspace.move", map[string]any{"workspace_id": "w1", "insert_index": float64(2)}},
		{"tab.move", map[string]any{"tab_id": "t1", "insert_index": float64(0)}},
		{"pane.swap", map[string]any{"source_pane_id": "p1", "target_pane_id": "p2"}},
		{"pane.move", map[string]any{
			"pane_id": "p1",
			"focus":   true,
			"destination": map[string]any{
				"type":           "tab",
				"tab_id":         "t2",
				"split":          "down",
				"target_pane_id": "p3",
			},
		}},
		{"pane.move", map[string]any{
			"pane_id": "p1",
			"focus":   false,
			"destination": map[string]any{
				"type":         "new_tab",
				"workspace_id": "w2",
				"label":        "new",
			},
		}},
		{"pane.resize", map[string]any{
			"pane_id":   "p1",
			"direction": "right",
			"amount":    float64(0.05),
		}},
	} {
		got := mutations[i]
		if got.Method != tc.method || len(got.Params) != len(tc.keys) {
			t.Fatalf("mutation[%d] = %+v, want %s keys %v", i, got, tc.method, tc.keys)
		}
		for key, want := range tc.keys {
			if key == "destination" {
				wantDest := want.(map[string]any)
				gotDest, ok := got.Params[key].(map[string]any)
				if !ok {
					t.Fatalf("%s.destination = %v, want map", got.Method, got.Params[key])
				}
				for dk, dv := range wantDest {
					if gotDest[dk] != dv {
						t.Errorf("%s.destination.%s = %v, want %v", got.Method, dk, gotDest[dk], dv)
					}
				}
				continue
			}
			if got.Params[key] != want {
				t.Errorf("%s.%s = %v, want %v", got.Method, key, got.Params[key], want)
			}
		}
	}
}

func TestReorderActionsRejectInvalidIDs(t *testing.T) {
	m := NewManager()
	if _, err := m.MoveProject("default", " ", 1); err == nil {
		t.Error("empty workspace id")
	}
	if _, err := m.MoveTab("default", "", 1); err == nil {
		t.Error("empty tab id")
	}
	if _, err := m.SwapPanes("default", "p1", "p1"); err == nil {
		t.Error("self swap")
	}
	if _, err := m.SwapPanes("default", "", "p2"); err == nil {
		t.Error("empty pane id")
	}
	if _, err := m.MovePane("default", "", PaneMoveDestination{Type: "tab", TabID: "t1"}, false); err == nil {
		t.Error("empty pane id on MovePane")
	}
	if _, err := m.MovePane("default", "p1", PaneMoveDestination{Type: ""}, false); err == nil {
		t.Error("empty destination type on MovePane")
	}
	if _, err := m.MovePane("default", "p1", PaneMoveDestination{Type: "tab", TabID: ""}, false); err == nil {
		t.Error("empty tab id on tab destination")
	}
	if _, err := m.MovePane("default", "p1", PaneMoveDestination{Type: "tab", TabID: "t1", Split: "invalid"}, false); err == nil {
		t.Error("invalid split direction on tab destination")
	}
	if _, err := m.ResizePane("default", "", "right", 0.05); err == nil {
		t.Error("empty pane id on ResizePane")
	}
	if _, err := m.ResizePane("default", "p1", "invalid", 0.05); err == nil {
		t.Error("invalid direction on ResizePane")
	}
}
