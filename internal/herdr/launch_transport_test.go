package herdr

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"net"
	"os"
	"path/filepath"
	"sync"
	"testing"
)

func errorsNew(message string) error { return errors.New(message) }

// fakeHerdrServer speaks the verified newline-JSON envelope over a unix
// socket placed where SocketPath resolves, so the launch transport is
// contract-tested against the shapes pinned from `herdr api schema`.
type fakeHerdrServer struct {
	mu       sync.Mutex
	requests []fakeRequest
	handler  func(method string, params map[string]any) (json.RawMessage, error)
	close    func()
}

type fakeRequest struct {
	Method string         `json:"method"`
	Params map[string]any `json:"params"`
}

func startFakeHerdr(t *testing.T, handler func(method string, params map[string]any) (json.RawMessage, error)) *fakeHerdrServer {
	t.Helper()
	// macOS unix socket paths are limited to 104 bytes: keep the socket under
	// a short root while HOME still points at the fake config tree.
	shortRoot, err := os.MkdirTemp("/tmp", "shfake")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { os.RemoveAll(shortRoot) })
	home := filepath.Join(shortRoot, "home")
	if err := os.MkdirAll(home, 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("HOME", home)
	socketDir := filepath.Join(home, ".config", "herdr")
	if err := os.MkdirAll(socketDir, 0o755); err != nil {
		t.Fatal(err)
	}
	socketPath := filepath.Join(socketDir, "herdr.sock")
	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		t.Fatal(err)
	}
	server := &fakeHerdrServer{handler: handler, close: func() { listener.Close() }}
	go func() {
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			go server.serve(conn)
		}
	}()
	t.Cleanup(server.close)
	return server
}

func (s *fakeHerdrServer) serve(conn net.Conn) {
	defer conn.Close()
	reader := bufio.NewReader(conn)
	for {
		line, err := reader.ReadBytes('\n')
		if err != nil {
			return
		}
		var generic map[string]any
		if err := json.Unmarshal(line, &generic); err != nil {
			continue
		}
		method, _ := generic["method"].(string)
		params, _ := generic["params"].(map[string]any)
		s.mu.Lock()
		s.requests = append(s.requests, fakeRequest{Method: method, Params: params})
		s.mu.Unlock()

		if method == "ping" {
			respond(conn, map[string]any{"result": map[string]any{"type": "pong", "protocol": 22, "version": "fake"}})
			continue
		}
		result, handlerErr := s.handler(method, params)
		if handlerErr != nil {
			respond(conn, map[string]any{"error": map[string]string{"code": "runtime", "message": handlerErr.Error()}})
			continue
		}
		respond(conn, map[string]any{"result": result})
	}
}

func respond(conn net.Conn, envelope map[string]any) {
	data, err := json.Marshal(envelope)
	if err != nil {
		return
	}
	conn.Write(append(data, '\n'))
}

func (s *fakeHerdrServer) recorded() []fakeRequest {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]fakeRequest(nil), s.requests...)
}

func TestLaunchTransportCreateTabWithoutFocus(t *testing.T) {
	server := startFakeHerdr(t, func(method string, params map[string]any) (json.RawMessage, error) {
		if method != "tab.create" {
			return nil, errorsUnexpected(method)
		}
		if focus, ok := params["focus"].(bool); !ok || focus {
			return nil, errorsNew("tab.create must never carry focus=true")
		}
		return json.RawMessage(`{"type":"tab_created","tab":{"tab_id":"tab-9","workspace_id":"w1"},"root_pane":{"pane_id":"pane-9"}}`), nil
	})
	manager := NewManager()
	tabID, paneID, err := manager.CreateTabWithoutFocus(context.Background(), "default", "w1", "/work/demo")
	if err != nil {
		t.Fatal(err)
	}
	if tabID != "tab-9" || paneID != "pane-9" {
		t.Fatalf("created = %q/%q", tabID, paneID)
	}
	for _, request := range server.recorded() {
		if request.Method == "tab.create" && request.Params["workspace_id"] != "w1" {
			t.Fatalf("params = %+v", request.Params)
		}
	}
}

func errorsUnexpected(method string) error { return errorsNew("unexpected method " + method) }

func TestLaunchTransportStartPromptWaitGet(t *testing.T) {
	server := startFakeHerdr(t, func(method string, params map[string]any) (json.RawMessage, error) {
		switch method {
		case "agent.start":
			return json.RawMessage(`{"type":"agent_started","agent":{"pane_id":"pane-9","tab_id":"tab-9","workspace_id":"w1","agent_status":"launch_pending","interactive_ready":false,"revision":3},"argv":["claude","--permission-mode","default"]}`), nil
		case "agent.prompt":
			return json.RawMessage(`{"type":"agent_prompted","agent":{"pane_id":"pane-9","agent_status":"working","interactive_ready":true,"revision":4}}`), nil
		case "agent.wait":
			return json.RawMessage(`{"type":"wait_matched"}`), nil
		case "agent.get":
			return json.RawMessage(`{"type":"agent_info","agent":{"pane_id":"pane-9","agent_status":"idle","interactive_ready":true,"revision":5,"agent":"claude-code","agent_session":{"agent":"claude-code","kind":"native","source":"provider","value":"sess-1"}}}`), nil
		default:
			return nil, errorsUnexpected(method)
		}
	})
	manager := NewManager()
	ctx := context.Background()

	started, err := manager.StartAgent(ctx, "default", AgentStartParams{
		Name: "claude-code-op1", Kind: "claude-code", PaneID: "pane-9",
		Args: []string{"--permission-mode", "default"}, TimeoutMS: 30000,
	})
	if err != nil {
		t.Fatal(err)
	}
	if started.AgentInfo.PaneID != "pane-9" || started.AgentInfo.AgentStatus != "launch_pending" {
		t.Fatalf("started = %+v", started.AgentInfo)
	}
	if len(started.Argv) != 3 {
		t.Fatalf("argv = %v", started.Argv)
	}

	if err := manager.PromptAgent(ctx, "default", "pane-9", "do the thing"); err != nil {
		t.Fatal(err)
	}
	if err := manager.WaitAgentState(ctx, "default", "pane-9", []string{"idle"}, 60000); err != nil {
		t.Fatal(err)
	}
	agent, err := manager.AgentByPane(ctx, "default", "pane-9")
	if err != nil {
		t.Fatal(err)
	}
	if agent == nil || !agent.InteractiveReady || agent.AgentStatus != "idle" || agent.Revision != 5 {
		t.Fatalf("agent = %+v", agent)
	}
	if agent.AgentSession == nil || agent.AgentSession.Value != "sess-1" {
		t.Fatalf("session = %+v", agent.AgentSession)
	}

	// Every method was exercised through the verified envelopes.
	methods := map[string]bool{}
	for _, request := range server.recorded() {
		methods[request.Method] = true
	}
	for _, want := range []string{"agent.start", "agent.prompt", "agent.wait", "agent.get"} {
		if !methods[want] {
			t.Fatalf("method %s never called", want)
		}
	}
}

func TestLaunchTransportShellReady(t *testing.T) {
	observations := 0
	startFakeHerdr(t, func(method string, params map[string]any) (json.RawMessage, error) {
		if method != "pane.process_info" {
			return nil, errorsUnexpected(method)
		}
		observations++
		if observations == 1 {
			// A foreground process beyond the shell means the pane is busy.
			return json.RawMessage(`{"type":"pane_process_info","process_info":{"pane_id":"pane-9","shell_pid":401,"foreground_processes":[{"pid":512,"name":"claude"}]}}`), nil
		}
		return json.RawMessage(`{"type":"pane_process_info","process_info":{"pane_id":"pane-9","shell_pid":401,"foreground_processes":[]}}`), nil
	})
	manager := NewManager()

	busy, err := manager.ShellReady(context.Background(), "default", "pane-9")
	if err != nil || busy {
		t.Fatalf("foreground agent must read not-ready: (%v, %v)", busy, err)
	}
	ready, err := manager.ShellReady(context.Background(), "default", "pane-9")
	if err != nil || !ready {
		t.Fatalf("interactive shell with no extra foreground process must be ready: (%v, %v)", ready, err)
	}
}

func TestLaunchTransportRejectsWrongResultType(t *testing.T) {
	startFakeHerdr(t, func(method string, params map[string]any) (json.RawMessage, error) {
		return json.RawMessage(`{"type":"tab_list","tabs":[]}`), nil
	})
	manager := NewManager()
	if _, _, err := manager.CreateTabWithoutFocus(context.Background(), "default", "w1", "/work/demo"); err == nil {
		t.Fatal("a mismatching result discriminator must fail loudly")
	}
}
