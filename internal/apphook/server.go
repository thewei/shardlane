package apphook

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
)

/**
 * [INPUT]: 依赖 net、os、bufio、encoding/json 处理 Unix Domain Socket 流
 * [OUTPUT]: 对外提供 Server、NewServer、DefaultSocketPath、DefaultRuntimeSocketPath、NormalizeStatus
 * [POS]: internal/apphook 的进程内 IPC 监听服务，统一接收并规范化各 Agent Hook 上报
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// DefaultSocketPath returns the primary Unix domain socket path for Shardlane agent status hooks.
func DefaultSocketPath() string {
	if path := strings.TrimSpace(os.Getenv("SHARDLANE_SOCKET_PATH")); path != "" {
		return path
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return filepath.Join(os.TempDir(), "shardlane-agent-status.sock")
	}
	return filepath.Join(home, ".shardlane", "run", "agent-status.sock")
}

// DefaultRuntimeSocketPath returns the convenience runtime socket path (~/.shardlane/runtime.sock).
func DefaultRuntimeSocketPath() string {
	home, err := os.UserHomeDir()
	if err != nil {
		return ""
	}
	return filepath.Join(home, ".shardlane", "runtime.sock")
}

// NormalizeStatus standardizes raw agent hook events and status keywords
// into canonical status strings: "working", "blocked", "idle", "done", "released".
func NormalizeStatus(raw string) string {
	s := strings.ToLower(strings.TrimSpace(raw))
	switch s {
	case "blocked", "waiting", "permission_request", "ask_user_question", "permissionrequest", "askuserquestion":
		return "blocked"
	case "working", "busy", "running", "pre_tool_use", "post_tool_use", "user_prompt_submit", "pretooluse", "posttooluse", "userpromptsubmit":
		return "working"
	case "idle", "session_start", "pre_invocation", "stop", "session_idle", "sessionstart", "preinvocation":
		return "idle"
	case "done", "complete", "completed":
		return "done"
	case "released", "session_end", "session_shutdown", "sessionend", "sessionshutdown":
		return "released"
	case "launch_pending":
		return "launch_pending"
	default:
		if s != "" {
			return s
		}
		return "idle"
	}
}

// Server is the in-app Unix domain socket server ingesting hook events.
type Server struct {
	socketPath  string
	symlinkPath string
	handler     Handler

	listener net.Listener
	quit     chan struct{}
	wg       sync.WaitGroup
	mu       sync.Mutex
	closed   bool
}

// NewServer creates a new in-app hook server instance.
func NewServer(socketPath string, handler Handler) *Server {
	if socketPath == "" {
		socketPath = DefaultSocketPath()
	}
	return &Server{
		socketPath: socketPath,
		handler:    handler,
		quit:       make(chan struct{}),
	}
}

// SetSymlinkPath sets an optional symlink alias path (such as ~/.shardlane/runtime.sock).
func (s *Server) SetSymlinkPath(path string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.symlinkPath = path
}

// SocketPath returns the configured socket path.
func (s *Server) SocketPath() string {
	return s.socketPath
}

// Start begins listening on the Unix domain socket.
func (s *Server) Start() error {
	s.mu.Lock()
	defer s.mu.Unlock()

	if s.listener != nil {
		return nil
	}

	dir := filepath.Dir(s.socketPath)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return fmt.Errorf("create socket directory %s: %w", dir, err)
	}

	// Stale socket detection: attempt connecting first.
	if _, err := os.Stat(s.socketPath); err == nil {
		conn, dialErr := net.DialTimeout("unix", s.socketPath, 100*time.Millisecond)
		if dialErr == nil {
			conn.Close()
			return fmt.Errorf("socket %s is already in use by an active process", s.socketPath)
		}
		// Connection failed, stale socket file left behind: remove it.
		if removeErr := os.Remove(s.socketPath); removeErr != nil {
			return fmt.Errorf("remove stale socket %s: %w", s.socketPath, removeErr)
		}
	}

	ln, err := net.Listen("unix", s.socketPath)
	if err != nil {
		return fmt.Errorf("listen unix socket %s: %w", s.socketPath, err)
	}
	_ = os.Chmod(s.socketPath, 0o600)
	s.listener = ln

	// Create symlink if requested and different from socketPath
	if s.symlinkPath != "" && s.symlinkPath != s.socketPath {
		_ = os.MkdirAll(filepath.Dir(s.symlinkPath), 0o700)
		_ = os.Remove(s.symlinkPath)
		_ = os.Symlink(s.socketPath, s.symlinkPath)
	}

	s.wg.Add(1)
	go s.acceptLoop(ln)

	slog.Info("apphook server started", "socket", s.socketPath)
	return nil
}

func (s *Server) acceptLoop(ln net.Listener) {
	defer s.wg.Done()

	for {
		conn, err := ln.Accept()
		if err != nil {
			select {
			case <-s.quit:
				return
			default:
				if ne, ok := err.(net.Error); ok && ne.Temporary() {
					time.Sleep(10 * time.Millisecond)
					continue
				}
				return
			}
		}

		s.wg.Add(1)
		go func(c net.Conn) {
			defer s.wg.Done()
			s.handleConnection(c)
		}(conn)
	}
}

// Close gracefully stops the server and removes the socket file.
func (s *Server) Close() error {
	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		return nil
	}
	s.closed = true
	close(s.quit)

	var closeErr error
	if s.listener != nil {
		closeErr = s.listener.Close()
	}
	s.mu.Unlock()

	s.wg.Wait()

	_ = os.Remove(s.socketPath)
	if s.symlinkPath != "" {
		_ = os.Remove(s.symlinkPath)
	}
	slog.Info("apphook server stopped", "socket", s.socketPath)
	return closeErr
}

type jsonRPCRequest struct {
	ID     any             `json:"id"`
	Method string          `json:"method"`
	Params json.RawMessage `json:"params"`
}

type directHookPayload struct {
	Agent       string `json:"agent"`
	Status      string `json:"status"`
	PaneID      string `json:"pane_id"`
	HerdrPane   string `json:"herdr_pane"`
	TmuxPane    string `json:"tmux_pane"`
	WorkspaceID string `json:"workspace_id"`
	SessionID   string `json:"session_id"`
	CWD         string `json:"cwd"`
	Event       string `json:"event"`
	Text        string `json:"text"`
}

type envelopePayload struct {
	Event string          `json:"event"`
	Data  json.RawMessage `json:"data"`
}

type envelopeData struct {
	PaneID      string `json:"pane_id"`
	WorkspaceID string `json:"workspace_id"`
	Agent       string `json:"agent"`
	AgentStatus string `json:"agent_status"`
	Status      string `json:"status"`
	SessionID   string `json:"session_id"`
}

func (s *Server) handleConnection(conn net.Conn) {
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(5 * time.Second))

	reader := bufio.NewReaderSize(conn, 64*1024)
	for {
		line, err := reader.ReadString('\n')
		if len(line) > 0 {
			line = strings.TrimRight(line, "\r\n")
			if len(line) > 0 {
				s.dispatchLine(line, conn)
			}
		}
		if err != nil {
			if err != io.EOF {
				slog.Debug("apphook connection read ended", "error", err)
			}
			break
		}
	}
}

func (s *Server) dispatchLine(line string, conn net.Conn) {
	now := time.Now()

	// 1. Format: Tab-delimited (name\tjson)
	if name, payload, ok := strings.Cut(line, "\t"); ok && name != "" {
		s.dispatchEnvelope(name, []byte(payload), now)
		writeACK(conn, nil)
		return
	}

	trimmed := strings.TrimSpace(line)
	if !strings.HasPrefix(trimmed, "{") {
		return
	}

	// 2. Check JSON-RPC format
	var rpcReq jsonRPCRequest
	if err := json.Unmarshal([]byte(trimmed), &rpcReq); err == nil && rpcReq.Method != "" {
		s.handleJSONRPC(rpcReq, now, conn)
		return
	}

	// 3. Check Envelope format {"event":"...", "data":{...}}
	var env envelopePayload
	if err := json.Unmarshal([]byte(trimmed), &env); err == nil && env.Event != "" && len(env.Data) > 0 {
		s.dispatchEnvelope(env.Event, env.Data, now)
		writeACK(conn, nil)
		return
	}

	// 4. Direct Shardlane status object
	var direct directHookPayload
	if err := json.Unmarshal([]byte(trimmed), &direct); err == nil && (direct.PaneID != "" || direct.HerdrPane != "" || direct.TmuxPane != "") {
		paneID := direct.PaneID
		if paneID == "" {
			paneID = direct.HerdrPane
		}
		if paneID == "" {
			paneID = direct.TmuxPane
		}

		status := NormalizeStatus(direct.Status)
		if status == "idle" && direct.Event != "" {
			status = NormalizeStatus(direct.Event)
		}

		statusEvt := StatusEvent{
			PaneID:      paneID,
			WorkspaceID: direct.WorkspaceID,
			Agent:       direct.Agent,
			Status:      status,
			SessionID:   direct.SessionID,
			CWD:         direct.CWD,
			RawEvent:    direct.Event,
			Text:        direct.Text,
			Timestamp:   now,
		}

		if s.handler.OnStatus != nil {
			s.handler.OnStatus(statusEvt)
		}

		if direct.SessionID != "" && s.handler.OnSession != nil {
			s.handler.OnSession(SessionEvent{
				PaneID:         paneID,
				Agent:          direct.Agent,
				AgentSessionID: direct.SessionID,
				Source:         "shardlane",
				Timestamp:      now,
			})
		}

		writeACK(conn, nil)
		return
	}
}

func (s *Server) handleJSONRPC(req jsonRPCRequest, now time.Time, conn net.Conn) {
	switch req.Method {
	case "pane.report_agent_session":
		var params struct {
			PaneID             string `json:"pane_id"`
			Agent              string `json:"agent"`
			Source             string `json:"source"`
			Seq                int64  `json:"seq"`
			AgentSessionID     string `json:"agent_session_id"`
			AgentSessionPath   string `json:"agent_session_path"`
			SessionStartSource string `json:"session_start_source"`
		}
		if err := json.Unmarshal(req.Params, &params); err == nil && params.PaneID != "" {
			if s.handler.OnSession != nil {
				s.handler.OnSession(SessionEvent{
					PaneID:             params.PaneID,
					Agent:              params.Agent,
					AgentSessionID:     params.AgentSessionID,
					AgentSessionPath:   params.AgentSessionPath,
					Source:             params.Source,
					SessionStartSource: params.SessionStartSource,
					Seq:                params.Seq,
					Timestamp:          now,
				})
			}
		}
		writeJSONRPCResponse(conn, req.ID, map[string]any{"ok": true})

	case "pane.report_agent_status", "pane.agent_status_changed":
		var params struct {
			PaneID      string `json:"pane_id"`
			WorkspaceID string `json:"workspace_id"`
			Agent       string `json:"agent"`
			Status      string `json:"status"`
			AgentStatus string `json:"agent_status"`
			SessionID   string `json:"session_id"`
		}
		if err := json.Unmarshal(req.Params, &params); err == nil && params.PaneID != "" {
			st := params.Status
			if st == "" {
				st = params.AgentStatus
			}
			status := NormalizeStatus(st)
			if s.handler.OnStatus != nil {
				s.handler.OnStatus(StatusEvent{
					PaneID:      params.PaneID,
					WorkspaceID: params.WorkspaceID,
					Agent:       params.Agent,
					Status:      status,
					SessionID:   params.SessionID,
					Timestamp:   now,
				})
			}
		}
		writeJSONRPCResponse(conn, req.ID, map[string]any{"ok": true})

	default:
		writeJSONRPCResponse(conn, req.ID, map[string]any{})
	}
}

func (s *Server) dispatchEnvelope(eventName string, raw []byte, now time.Time) {
	if s.handler.OnStatus == nil {
		return
	}
	var env envelopePayload
	if err := json.Unmarshal(raw, &env); err == nil && len(env.Data) > 0 {
		raw = env.Data
		if env.Event != "" {
			eventName = env.Event
		}
	}
	var data envelopeData
	if err := json.Unmarshal(raw, &data); err != nil || data.PaneID == "" {
		return
	}
	st := data.AgentStatus
	if st == "" {
		st = data.Status
	}
	s.handler.OnStatus(StatusEvent{
		PaneID:      data.PaneID,
		WorkspaceID: data.WorkspaceID,
		Agent:       data.Agent,
		Status:      NormalizeStatus(st),
		SessionID:   data.SessionID,
		RawEvent:    eventName,
		Timestamp:   now,
	})
}

func writeJSONRPCResponse(conn net.Conn, id any, result any) {
	resp := map[string]any{"id": id, "result": result}
	b, err := json.Marshal(resp)
	if err != nil {
		return
	}
	b = append(b, '\n')
	_, _ = conn.Write(b)
}

func writeACK(conn net.Conn, extra map[string]any) {
	payload := map[string]any{"ok": true}
	for k, v := range extra {
		payload[k] = v
	}
	b, err := json.Marshal(payload)
	if err != nil {
		return
	}
	b = append(b, '\n')
	_, _ = conn.Write(b)
}
