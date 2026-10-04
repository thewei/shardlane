package nativeui

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/wh-studio/herdr-client/next/internal/agent"
	"github.com/wh-studio/herdr-client/next/internal/conversation"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

// TestChatUnboundShowsEmptyState pins the pre-bind surface.
func TestChatUnboundShowsEmptyState(t *testing.T) {
	shell := NewShell()
	shell.router.Replace("/chat")
	tester := ui.NewTester(shell.View, 1200, 800)
	if !tester.HasText("No conversation selected") {
		t.Fatalf("missing empty state; texts=%q", tester.Texts())
	}
}

// TestChatBindFillsTurnsFromHistory pins the full bind path over the Codex
// rollout fixture: bounded window → items → derived turns render.
func TestChatBindFillsTurnsFromHistory(t *testing.T) {
	shell, _, _ := newHistoryTestShell(t)

	shell.bindChatConversation("codex:22222222-aaaa-bbbb-cccc-000000000002")
	if shell.chatLoading {
		t.Fatal("synchronous dispatch must not leave the chat loading")
	}
	if len(shell.chatTurns) == 0 {
		t.Fatalf("turns not filled: %+v", shell.chatTurns)
	}
	// Fixture: user "Build history", assistant host with the shell tool, "Implemented.".
	foundUser, foundNarration := false, false
	for _, turn := range shell.chatTurns {
		if turn.UserRow && strings.Contains(turn.Text, "Build history") {
			foundUser = true
		}
		if strings.Contains(turn.Narration, "Implemented.") {
			foundNarration = true
		}
	}
	if !foundUser || !foundNarration {
		t.Fatalf("turns = %+v", shell.chatTurns)
	}

	tester := ui.NewTester(shell.View, 1200, 800)
	shell.router.Replace("/chat")
	tester.Frame()
	for _, want := range []string{"USER", "ASSISTANT", "Implemented.", "Tool activity"} {
		if !tester.HasText(want) {
			t.Fatalf("chat page missing %q; texts=%q", want, tester.Texts())
		}
	}
	// Tool details live inside the collapsed tool-activity disclosure.
	if err := tester.Click("Tool activity"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("cargo test") {
		t.Fatal("expanding tool activity did not reveal the input preview")
	}
}

// TestChatBindStaleGenerationCannotApply pins the cancellation semantics:
// switching conversations cancels the obsolete read and a stale late result
// cannot overwrite the newer answer.
func TestChatBindStaleGenerationCannotApply(t *testing.T) {
	shell := NewShell()
	fake := newFakeHistoryView()
	shell.hist.service = fake
	shell.router.Replace("/chat")

	// First bind held in flight.
	firstDone := make(chan struct{})
	go func() {
		defer close(firstDone)
		shell.bindChatConversation("codex:first")
	}()
	first := <-fake.openCalls

	// Switch to the second conversation while the first is in flight.
	secondDone := make(chan struct{})
	go func() {
		defer close(secondDone)
		shell.bindChatConversation("codex:second")
	}()
	second := <-fake.openCalls
	if second.conversationID != "codex:second" {
		t.Fatalf("second bind = %q", second.conversationID)
	}

	// The latest bind applies immediately (fake Open returns no items).
	second.release <- openResult{}
	<-secondDone

	// The stale first result resolves late; it must not apply.
	first.release <- openResult{}
	<-firstDone
	if first.ctx.Err() == nil {
		t.Fatalf("superseded bind context = %v, want canceled", first.ctx.Err())
	}
	if shell.chatConversationID != "codex:second" {
		t.Fatalf("conversation id = %q", shell.chatConversationID)
	}
	if shell.chatLoading {
		t.Fatal("stale result resurrected the loading state")
	}
}

// TestOpenChatWithoutBindableSourceDegradesToTerminal pins F58 (2026-10-06):
// an agent card with neither a live session identity nor a history
// conversation must not land on /chat's "No conversation selected" empty
// state — the shell degrades to the agent's Terminal with a hint.
func TestOpenChatWithoutBindableSourceDegradesToTerminal(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.activeInstance = "default"
	s.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: "/tmp"}},
		Tabs:     []herdr.Tab{{ID: "t1", ProjectID: "w1", Label: "main"}},
		Panes:    []herdr.Pane{{ID: "p1", ProjectID: "w1", TabID: "t1", TerminalID: "term-1"}},
	}
	s.router.Replace(routeHistory)

	s.openChat(agent.AgentCardModel{PaneID: "p1"})

	if got := s.router.Path(); got == "/chat" {
		t.Fatal("un-bindable card landed on /chat's empty state")
	}
	if got := s.router.Path(); got != routeWorkspace {
		t.Fatalf("route = %q, want the workspace", got)
	}
	if !strings.Contains(s.status, "No chat source") {
		t.Fatalf("status = %q, want the degrade hint", s.status)
	}
}

// TestChatOutcomeRenders pins F65 (2026-10-06): chat outcome messages —
// delivery failures, queue refusals, the "do not resend" warning — were
// written in eight places but never rendered; the composer must show them.
func TestChatOutcomeRenders(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.router.Replace("/chat")
	s.chatConversationID = "live:codex:term-1"
	s.chatOutcome = "Prompt delivery uncertain — do not resend; check the agent."
	tester := ui.NewTester(s.View, 1200, 800)
	if !tester.HasText("Prompt delivery uncertain — do not resend; check the agent.") {
		t.Fatalf("outcome invisible; texts=%q", tester.Texts())
	}
	// F71: the composer input must exist on a bound chat (its Label reads).
	if !tester.HasText("Chat prompt") {
		t.Fatalf("composer missing; texts=%q", tester.Texts())
	}
}

// TestChatDisclosuresArePerTurn pins F94 (2026-10-06): expanding one turn's
// Thinking must not expand every other turn's — disclosure state is keyed
// per turn, not shared.
func TestChatDisclosuresArePerTurn(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.router.Replace("/chat")
	s.chatConversationID = "live:codex:term-1"
	think := "thinking text"
	s.chatTurns = []conversation.TimelineTurn{
		{Narration: "first", Thinking: &think, ToolRuns: []conversation.ConversationToolCall{{Name: "exec"}}},
		{Narration: "second", Thinking: &think, ToolRuns: []conversation.ConversationToolCall{{Name: "exec"}}},
	}
	tester := ui.NewTester(s.View, 1200, 800)

	// Expand turn 0's thinking only.
	s.setChatTurnPartOpen(0, "thinking", true)
	tester.Frame()
	opens := 0
	for i := 0; i < 2; i++ {
		if s.chatTurnPartOpen(i, "thinking") {
			opens++
		}
	}
	if opens != 1 {
		t.Fatalf("thinking open count = %d, want 1", opens)
	}
	if s.chatTurnPartOpen(0, "tools") || s.chatTurnPartOpen(1, "tools") {
		t.Fatal("tools disclosure leaked from thinking state")
	}
}
