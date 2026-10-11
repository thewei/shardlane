// Package herdrhooks tails the hook spool the Shardlane herdr plugin
// appends to: our own hook layer on top of Herdr's plugin event hooks
// (N19). Herdr runs the plugin's forwarder per declared event; the
// forwarder appends "NAME\tJSON" lines to the spool file, and the app
// turns them into typed hook events.
package herdrhooks

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// DefaultSpoolPath is the spool the plugin forwarder writes when
// SHARDLANE_HOOK_SPOOL is unset.
func DefaultSpoolPath() string {
	home, err := os.UserHomeDir()
	if err != nil {
		return ""
	}
	return filepath.Join(home, ".shardlane", "hooks", "events.jsonl")
}

// Event is one forwarded hook event.
type Event struct {
	Name    string
	Payload json.RawMessage
}

// Envelope is the forwarder line's JSON: Herdr injects the plugin event as
// {"event": "<name>", "data": {…}} — the fields live under data.
type Envelope struct {
	Event string          `json:"event"`
	Data  json.RawMessage `json:"data"`
}

// StatusPayload is the pane.agent_status_changed data's fields the hook
// layer consumes.
type StatusPayload struct {
	PaneID      string `json:"pane_id"`
	AgentStatus string `json:"agent_status"`
}

// Start tails the spool file, delivering each appended line to onEvent on
// its own goroutine. It creates the file if missing and survives truncation
// (rotation). The returned stop function releases the poller.
func Start(path string, onEvent func(Event)) (stop func(), err error) {
	if path == "" {
		return nil, nil
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return nil, err
	}
	file, err := os.OpenFile(path, os.O_CREATE|os.O_RDONLY, 0o644)
	if err != nil {
		return nil, err
	}
	offset := int64(0)
	done := make(chan struct{})
	go func() {
		defer file.Close()
		ticker := time.NewTicker(250 * time.Millisecond)
		defer ticker.Stop()
		for {
			select {
			case <-done:
				return
			case <-ticker.C:
				info, err := file.Stat()
				if err != nil {
					continue
				}
				if info.Size() < offset {
					// Truncated or rotated: start over.
					if _, err := file.Seek(0, 0); err != nil {
						continue
					}
					offset = 0
				}
				if info.Size() == offset {
					continue
				}
				raw := make([]byte, info.Size()-offset)
				read, err := file.ReadAt(raw, offset)
				if read > 0 {
					offset += int64(read)
					deliver(raw, onEvent)
				}
			}
		}
	}()
	return func() { close(done) }, nil
}

// deliver splits complete lines and dispatches them; a trailing partial
// line stays in the buffer for the next read (the offset already covers it,
// so re-reads would duplicate — accept the rare torn tail line as lost; the
// next event of the same family supersedes it).
func deliver(raw []byte, onEvent func(Event)) {
	for _, line := range strings.Split(string(raw), "\n") {
		line = strings.TrimSuffix(line, "\r")
		if line == "" {
			continue
		}
		name, payload, ok := strings.Cut(line, "\t")
		if !ok || name == "" {
			continue
		}
		onEvent(Event{Name: name, Payload: json.RawMessage(payload)})
	}
}
