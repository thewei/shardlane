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

// TestAnswerInteractionTextRefusesBlankAnswer pins the free-text guard: the
// "Type something." / "Chat about this" options exist to carry a typed
// answer; an empty one is exactly the "no answer provided" defect.
func TestAnswerInteractionTextRefusesBlankAnswer(t *testing.T) {
	shell := NewShell()
	bridge := NewChatBridge(shell)
	if _, err := bridge.AnswerInteractionText(context.Background(), "ia-1", "Chat about this", "   "); err == nil {
		t.Fatal("expected refusal for a blank free-text answer")
	}
}

// TestAnswerInteractionRefusesUnknownInteraction pins the locate guard:
// neither variant touches the runtime for an interaction that is not
// pending on the bound pane.
func TestAnswerInteractionRefusesUnknownInteraction(t *testing.T) {
	shell := NewShell()
	bridge := NewChatBridge(shell)
	if _, err := bridge.AnswerInteraction(context.Background(), "ia-missing", "Yes"); err == nil {
		t.Fatal("expected refusal for an unknown interaction")
	}
	if _, err := bridge.AnswerInteractionText(context.Background(), "ia-missing", "Chat about this", "hello"); err == nil {
		t.Fatal("expected refusal for an unknown interaction")
	}
}
