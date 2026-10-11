package nativeui

import (
	"context"
	"testing"
)

// TestChatInterruptAndSendRefusesUnboundChat pins the bridge guard: without
// a bound chat pane the cut-in path refuses before touching the runtime
// (headless shell; the RPC legs need a live Herdr).
func TestChatInterruptAndSendRefusesUnboundChat(t *testing.T) {
	shell := NewShell()
	bridge := NewChatBridge(shell)
	if _, err := bridge.InterruptAndSend(context.Background(), "hello"); err == nil {
		t.Fatal("expected refusal without a bound chat pane")
	}
	if _, err := bridge.InterruptAndSend(context.Background(), "  "); err == nil {
		t.Fatal("expected refusal for a blank prompt")
	}
}
