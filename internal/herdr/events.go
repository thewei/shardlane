package herdr

import (
	"bufio"
	"encoding/json"
	"fmt"
	"net"
	"runtime"
	"sync"
	"time"
)

// RuntimeEvent is the stable outer envelope emitted by Herdr's event socket.
// The Go migration currently uses event kinds as reconciliation triggers; the
// authoritative shell projection still comes from session.snapshot.
type RuntimeEvent struct {
	Event string          `json:"event"`
	Data  json.RawMessage `json:"data"`
}

// EventStream owns one long-lived events.subscribe socket.
type EventStream struct {
	conn   net.Conn
	events chan RuntimeEvent
	done   chan struct{}
	once   sync.Once
}

func (s *EventStream) Events() <-chan RuntimeEvent { return s.events }
func (s *EventStream) Done() <-chan struct{}       { return s.done }

func (s *EventStream) Close() {
	s.once.Do(func() {
		_ = s.conn.Close()
		<-s.done
	})
}

// SubscribeEvents follows the verified Herdr events.subscribe contract. Only
// structural/runtime events are subscribed: Herdr global focus events are
// intentionally excluded because Shardlane navigation is client-local. Pane-
// scoped Agent status events are added for the materialized Pane set. Callers
// recreate the stream after pane membership changes.
func (m *Manager) SubscribeEvents(session string, paneIDs []string) (*EventStream, error) {
	socket, err := m.reachableSocket(session)
	if err != nil {
		return nil, err
	}
	if runtime.GOOS == "windows" {
		return nil, fmt.Errorf("Herdr named-pipe event streams are not ported to Windows yet")
	}
	conn, err := net.DialTimeout("unix", socket, 400*time.Millisecond)
	if err != nil {
		return nil, fmt.Errorf("connect Herdr event socket: %w", err)
	}
	_ = conn.SetDeadline(time.Now().Add(2 * time.Second))

	subscriptions := eventSubscriptions(paneIDs)

	request, err := json.Marshal(map[string]any{
		"id":     "shardlane-next-events",
		"method": "events.subscribe",
		"params": map[string]any{"subscriptions": subscriptions},
	})
	if err != nil {
		_ = conn.Close()
		return nil, err
	}
	if _, err := conn.Write(append(request, '\n')); err != nil {
		_ = conn.Close()
		return nil, fmt.Errorf("write events.subscribe: %w", err)
	}

	reader := bufio.NewReader(conn)
	ack, err := reader.ReadBytes('\n')
	if err != nil {
		_ = conn.Close()
		return nil, fmt.Errorf("read events.subscribe acknowledgement: %w", err)
	}
	var envelope rpcEnvelope
	if err := json.Unmarshal(ack, &envelope); err != nil {
		_ = conn.Close()
		return nil, fmt.Errorf("decode events.subscribe acknowledgement: %w", err)
	}
	if envelope.Error != nil {
		_ = conn.Close()
		return nil, fmt.Errorf("Herdr events.subscribe: %s", envelope.Error.Message)
	}
	var started struct {
		Type string `json:"type"`
	}
	if err := json.Unmarshal(envelope.Result, &started); err != nil || started.Type != "subscription_started" {
		_ = conn.Close()
		return nil, fmt.Errorf("unexpected events.subscribe acknowledgement: %s", string(envelope.Result))
	}
	_ = conn.SetDeadline(time.Time{})

	stream := &EventStream{
		conn:   conn,
		events: make(chan RuntimeEvent, 64),
		done:   make(chan struct{}),
	}
	go stream.read(reader)
	return stream, nil
}

func eventSubscriptions(paneIDs []string) []map[string]any {
	subscriptions := []map[string]any{
		{"type": "workspace.created"},
		{"type": "workspace.updated"},
		{"type": "workspace.metadata_updated"},
		{"type": "workspace.closed"},
		{"type": "workspace.renamed"},
		{"type": "workspace.moved"},
		{"type": "workspace.reordered"},
		{"type": "worktree.created"},
		{"type": "worktree.opened"},
		{"type": "worktree.removed"},
		{"type": "tab.created"},
		{"type": "tab.closed"},
		{"type": "tab.renamed"},
		{"type": "tab.moved"},
		{"type": "pane.created"},
		{"type": "pane.closed"},
		{"type": "pane.updated"},
		{"type": "pane.exited"},
		{"type": "pane.moved"},
		{"type": "pane.agent_detected"},
		{"type": "layout.updated"},
	}
	for _, paneID := range paneIDs {
		if paneID == "" {
			continue
		}
		subscriptions = append(subscriptions,
			map[string]any{"type": "pane.agent_status_changed", "pane_id": paneID},
		)
	}
	return subscriptions
}

func (s *EventStream) read(reader *bufio.Reader) {
	defer close(s.done)
	defer close(s.events)
	for {
		line, err := reader.ReadBytes('\n')
		if err != nil {
			return
		}
		var event RuntimeEvent
		if err := json.Unmarshal(line, &event); err != nil || event.Event == "" {
			continue
		}
		select {
		case s.events <- event:
		default:
			// Do not let a presentation consumer back up the Herdr socket. One
			// later snapshot reconciles the complete shell state.
		}
	}
}
