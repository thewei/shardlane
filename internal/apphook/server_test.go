package apphook

import (
	"bufio"
	"encoding/json"
	"net"
	"os"
	"path/filepath"
	"sync"
	"testing"
)

/**
 * [INPUT]: 依赖 testing、net、apphook Server
 * [OUTPUT]: 验证 Hook Server 在各类载荷（JSON-RPC、直发 JSON、Envelope、Tab 行）下的解析与 Socket 生命周期
 * [POS]: internal/apphook 的契约与边界测试
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestNormalizeStatus(t *testing.T) {
	tests := []struct {
		input string
		want  string
	}{
		{"blocked", "blocked"},
		{"PermissionRequest", "blocked"},
		{"ask_user_question", "blocked"},
		{"working", "working"},
		{"PreToolUse", "working"},
		{"user_prompt_submit", "working"},
		{"idle", "idle"},
		{"SessionStart", "idle"},
		{"pre_invocation", "idle"},
		{"done", "done"},
		{"released", "released"},
		{"session_end", "released"},
		{"launch_pending", "launch_pending"},
	}

	for _, tt := range tests {
		got := NormalizeStatus(tt.input)
		if got != tt.want {
			t.Errorf("NormalizeStatus(%q) = %q, want %q", tt.input, got, tt.want)
		}
	}
}

func sendAndReadResponse(t *testing.T, socketPath, msg string) string {
	t.Helper()
	conn, err := net.Dial("unix", socketPath)
	if err != nil {
		t.Fatalf("dial failed: %v", err)
	}
	defer conn.Close()

	if _, err := conn.Write([]byte(msg + "\n")); err != nil {
		t.Fatalf("write failed: %v", err)
	}

	reader := bufio.NewReader(conn)
	line, err := reader.ReadString('\n')
	if err != nil {
		t.Fatalf("read response failed: %v", err)
	}
	return line
}

func TestServerDirectStatusPayload(t *testing.T) {
	sockPath := filepath.Join(t.TempDir(), "test.sock")
	var mu sync.Mutex
	var gotStatus StatusEvent
	var gotSession SessionEvent

	srv := NewServer(sockPath, Handler{
		OnStatus: func(evt StatusEvent) {
			mu.Lock()
			gotStatus = evt
			mu.Unlock()
		},
		OnSession: func(evt SessionEvent) {
			mu.Lock()
			gotSession = evt
			mu.Unlock()
		},
	})
	if err := srv.Start(); err != nil {
		t.Fatalf("server start failed: %v", err)
	}
	defer srv.Close()

	msg := `{"agent":"agy","status":"working","pane_id":"p-1","session_id":"sess-123","event":"PreToolUse","text":"running command"}`
	resp := sendAndReadResponse(t, sockPath, msg)

	var ack map[string]any
	if err := json.Unmarshal([]byte(resp), &ack); err != nil || ack["ok"] != true {
		t.Fatalf("unexpected ack response: %q", resp)
	}

	mu.Lock()
	defer mu.Unlock()
	if gotStatus.PaneID != "p-1" || gotStatus.Agent != "agy" || gotStatus.Status != "working" {
		t.Fatalf("unexpected status event: %+v", gotStatus)
	}
	if gotSession.PaneID != "p-1" || gotSession.AgentSessionID != "sess-123" {
		t.Fatalf("unexpected session event: %+v", gotSession)
	}
}

func TestServerJSONRPCReportSession(t *testing.T) {
	sockPath := filepath.Join(t.TempDir(), "test.sock")
	var mu sync.Mutex
	var gotSession SessionEvent

	srv := NewServer(sockPath, Handler{
		OnSession: func(evt SessionEvent) {
			mu.Lock()
			gotSession = evt
			mu.Unlock()
		},
	})
	if err := srv.Start(); err != nil {
		t.Fatalf("server start failed: %v", err)
	}
	defer srv.Close()

	msg := `{"id":"req-42","method":"pane.report_agent_session","params":{"pane_id":"p-claude","agent":"claude","source":"herdr:claude","seq":999,"agent_session_id":"uuid-abc","agent_session_path":"/tmp/t.jsonl"}}`
	resp := sendAndReadResponse(t, sockPath, msg)

	var rpcResp struct {
		ID     string         `json:"id"`
		Result map[string]any `json:"result"`
	}
	if err := json.Unmarshal([]byte(resp), &rpcResp); err != nil || rpcResp.ID != "req-42" || rpcResp.Result["ok"] != true {
		t.Fatalf("unexpected JSON-RPC response: %q", resp)
	}

	mu.Lock()
	defer mu.Unlock()
	if gotSession.PaneID != "p-claude" || gotSession.Agent != "claude" || gotSession.AgentSessionID != "uuid-abc" || gotSession.AgentSessionPath != "/tmp/t.jsonl" {
		t.Fatalf("unexpected session event: %+v", gotSession)
	}
}

func TestServerJSONRPCReportStatus(t *testing.T) {
	sockPath := filepath.Join(t.TempDir(), "test.sock")
	var mu sync.Mutex
	var gotStatus StatusEvent

	srv := NewServer(sockPath, Handler{
		OnStatus: func(evt StatusEvent) {
			mu.Lock()
			gotStatus = evt
			mu.Unlock()
		},
	})
	if err := srv.Start(); err != nil {
		t.Fatalf("server start failed: %v", err)
	}
	defer srv.Close()

	msg := `{"id":"req-status","method":"pane.report_agent_status","params":{"pane_id":"p-1","status":"blocked"}}`
	resp := sendAndReadResponse(t, sockPath, msg)

	var rpcResp struct {
		ID     string         `json:"id"`
		Result map[string]any `json:"result"`
	}
	if err := json.Unmarshal([]byte(resp), &rpcResp); err != nil || rpcResp.ID != "req-status" || rpcResp.Result["ok"] != true {
		t.Fatalf("unexpected JSON-RPC response: %q", resp)
	}

	mu.Lock()
	defer mu.Unlock()
	if gotStatus.PaneID != "p-1" || gotStatus.Status != "blocked" {
		t.Fatalf("unexpected status event: %+v", gotStatus)
	}
}

func TestServerEnvelopePayload(t *testing.T) {
	sockPath := filepath.Join(t.TempDir(), "test.sock")
	var mu sync.Mutex
	var gotStatus StatusEvent

	srv := NewServer(sockPath, Handler{
		OnStatus: func(evt StatusEvent) {
			mu.Lock()
			gotStatus = evt
			mu.Unlock()
		},
	})
	if err := srv.Start(); err != nil {
		t.Fatalf("server start failed: %v", err)
	}
	defer srv.Close()

	msg := `{"event":"pane.agent_status_changed","data":{"pane_id":"p-env","agent_status":"idle","agent":"agy"}}`
	sendAndReadResponse(t, sockPath, msg)

	mu.Lock()
	defer mu.Unlock()
	if gotStatus.PaneID != "p-env" || gotStatus.Status != "idle" || gotStatus.Agent != "agy" {
		t.Fatalf("unexpected status event: %+v", gotStatus)
	}
}

func TestServerTabDelimitedLine(t *testing.T) {
	sockPath := filepath.Join(t.TempDir(), "test.sock")
	var mu sync.Mutex
	var gotStatus StatusEvent

	srv := NewServer(sockPath, Handler{
		OnStatus: func(evt StatusEvent) {
			mu.Lock()
			gotStatus = evt
			mu.Unlock()
		},
	})
	if err := srv.Start(); err != nil {
		t.Fatalf("server start failed: %v", err)
	}
	defer srv.Close()

	msg := "pane.agent_status_changed\t" + `{"event":"pane.agent_status_changed","data":{"pane_id":"p-tab","agent_status":"done"}}`
	sendAndReadResponse(t, sockPath, msg)

	mu.Lock()
	defer mu.Unlock()
	if gotStatus.PaneID != "p-tab" || gotStatus.Status != "done" {
		t.Fatalf("unexpected status event: %+v", gotStatus)
	}
}

func TestServerStaleSocketRecovery(t *testing.T) {
	tmpDir := t.TempDir()
	sockPath := filepath.Join(tmpDir, "stale.sock")

	// Create a dummy stale socket file (regular file or non-listening socket)
	if err := os.WriteFile(sockPath, []byte("stale dead socket"), 0o600); err != nil {
		t.Fatalf("failed to create stale file: %v", err)
	}

	srv := NewServer(sockPath, Handler{})
	if err := srv.Start(); err != nil {
		t.Fatalf("server should recover from stale socket file, got err: %v", err)
	}
	defer srv.Close()

	if _, err := os.Stat(sockPath); os.IsNotExist(err) {
		t.Fatal("socket file should exist after start")
	}
}

func TestServerCleanShutdown(t *testing.T) {
	sockPath := filepath.Join(t.TempDir(), "shutdown.sock")
	symlinkPath := filepath.Join(t.TempDir(), "symlink.sock")

	srv := NewServer(sockPath, Handler{})
	srv.SetSymlinkPath(symlinkPath)
	if err := srv.Start(); err != nil {
		t.Fatalf("server start failed: %v", err)
	}

	if _, err := os.Stat(sockPath); os.IsNotExist(err) {
		t.Fatal("socket must exist while running")
	}
	if _, err := os.Lstat(symlinkPath); os.IsNotExist(err) {
		t.Fatal("symlink must exist while running")
	}

	if err := srv.Close(); err != nil {
		t.Fatalf("server close failed: %v", err)
	}

	if _, err := os.Stat(sockPath); !os.IsNotExist(err) {
		t.Fatalf("socket file should be removed on close, got: %v", err)
	}
	if _, err := os.Lstat(symlinkPath); !os.IsNotExist(err) {
		t.Fatalf("symlink should be removed on close, got: %v", err)
	}
}
