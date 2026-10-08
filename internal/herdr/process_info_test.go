package herdr

import (
	"encoding/json"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

// TestPaneProcessInfoDecodesLiveShape pins the decoder to the shape captured
// from a live protocol-22 session (2026-10-06): process_info nested in the
// result envelope, nullable shell/group ids, one foreground process.
func TestPaneProcessInfoDecodesLiveShape(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix socket fixture")
	}
	// The socket path must stay under the macOS 104-char sun_path limit, so
	// this fixture sits in a shallow temp dir rather than t.TempDir().
	dir, err := os.MkdirTemp("", "herdrp")
	if err != nil {
		t.Fatal(err)
	}
	defer os.RemoveAll(dir)
	path := filepath.Join(dir, "s.sock")
	listener, err := net.Listen("unix", path)
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	go func() {
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			servePaneProcessInfo(conn)
		}
	}()

	info, err := paneProcessInfoAt(path, "w3:p3")
	if err != nil {
		t.Fatal(err)
	}
	if info.PaneID != "w3:p3" || info.ShellPID != 3055 || info.ForegroundPGID != 3055 || info.TTY != "/dev/ttys021" {
		t.Fatalf("info = %#v", info)
	}
	if len(info.ForegroundProcesses) != 1 {
		t.Fatalf("foreground processes = %#v", info.ForegroundProcesses)
	}
	proc := info.ForegroundProcesses[0]
	if proc.PID != 3055 || proc.Name != "zsh" || proc.CWD != "/Users/wilson/Workspaces" {
		t.Fatalf("process = %#v", proc)
	}

	if _, err := paneProcessInfoAt(path, "gone"); err == nil {
		t.Fatal("expected error for missing pane")
	}
}

// servePaneProcessInfo answers one fixture connection with the captured
// live response for w3:p3 and a not_found for any other pane.
func servePaneProcessInfo(conn net.Conn) {
	defer conn.Close()
	var request map[string]any
	_ = json.NewDecoder(conn).Decode(&request)
	if request["method"] != "pane.process_info" {
		_ = json.NewEncoder(conn).Encode(map[string]any{
			"error": map[string]any{"code": "bad_request", "message": "wrong method"},
		})
		return
	}
	params, _ := request["params"].(map[string]any)
	if params["pane_id"] != "w3:p3" {
		_ = json.NewEncoder(conn).Encode(map[string]any{
			"error": map[string]any{"code": "not_found", "message": "no pane"},
		})
		return
	}
	_ = json.NewEncoder(conn).Encode(map[string]any{
		"result": map[string]any{
			"type": "pane_process_info",
			"process_info": map[string]any{
				"id":                          "cli:pane:process_info",
				"pane_id":                     "w3:p3",
				"shell_pid":                   3055,
				"foreground_process_group_id": 3055,
				"tty":                         "/dev/ttys021",
				"foreground_processes": []any{
					map[string]any{
						"argv":    []string{"-zsh"},
						"argv0":   "zsh",
						"cmdline": "-zsh",
						"cwd":     "/Users/wilson/Workspaces",
						"name":    "zsh",
						"pid":     3055,
					},
				},
			},
		},
	})
}
