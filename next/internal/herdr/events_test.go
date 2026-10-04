package herdr

import (
	"bufio"
	"encoding/json"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

func TestEventSubscriptionsExcludeGlobalFocusChurn(t *testing.T) {
	subscriptions := eventSubscriptions([]string{"p1"})
	for _, subscription := range subscriptions {
		eventType, _ := subscription["type"].(string)
		switch eventType {
		case "workspace.focused", "tab.focused", "pane.focused":
			t.Fatalf("global focus event must not drive Shardlane navigation: %q", eventType)
		}
	}
	foundAgentStatus := false
	for _, subscription := range subscriptions {
		if subscription["type"] == "pane.agent_status_changed" && subscription["pane_id"] == "p1" {
			foundAgentStatus = true
			break
		}
	}
	if !foundAgentStatus {
		t.Fatal("pane-scoped agent status subscription missing")
	}
}

func TestSubscribeEventsReadsEventAfterAck(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix socket fixture")
	}
	home, err := os.MkdirTemp("/tmp", "shardlane-next-events-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(home) })
	t.Setenv("HOME", home)
	socket := filepath.Join(home, ".config", "herdr", "herdr.sock")
	if err := os.MkdirAll(filepath.Dir(socket), 0o755); err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("unix", socket)
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
			go func(conn net.Conn) {
				defer conn.Close()
				reader := bufio.NewReader(conn)
				line, err := reader.ReadBytes('\n')
				if err != nil {
					return
				}
				var request struct {
					Method string `json:"method"`
				}
				if json.Unmarshal(line, &request) != nil {
					return
				}
				switch request.Method {
				case "ping":
					_ = json.NewEncoder(conn).Encode(map[string]any{
						"result": map[string]any{"type": "pong", "version": "0.9.3", "protocol": 22},
					})
				case "events.subscribe":
					encoder := json.NewEncoder(conn)
					_ = encoder.Encode(map[string]any{"result": map[string]any{"type": "subscription_started"}})
					_ = encoder.Encode(map[string]any{
						"event": "pane.updated",
						"data":  map[string]any{"type": "pane_updated"},
					})
					time.Sleep(time.Second)
				}
			}(conn)
		}
	}()

	stream, err := NewManager().SubscribeEvents("default", []string{"p1"})
	if err != nil {
		t.Fatal(err)
	}
	defer stream.Close()
	select {
	case event := <-stream.Events():
		if event.Event != "pane.updated" {
			t.Fatalf("event = %#v", event)
		}
	case <-time.After(time.Second):
		t.Fatal("timed out waiting for Herdr event")
	}
}
