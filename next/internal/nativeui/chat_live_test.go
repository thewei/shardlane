package nativeui

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/wh-studio/herdr-client/next/internal/agent"
	"github.com/wh-studio/herdr-client/next/internal/conversation"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
	"github.com/wh-studio/herdr-client/next/internal/history"
)

// liveBindShell builds a shell whose projection carries one live agent with
// a measured path-kind session identity pointing at a temp transcript.
func liveBindShell(t *testing.T, provider string, transcript string) (*Shell, agent.AgentCardModel) {
	t.Helper()
	shell := NewShell()
	shell.activeInstance = "inst-1"
	var session *herdr.AgentSessionIdentity
	if provider != "" {
		session = &herdr.AgentSessionIdentity{Agent: provider, Kind: "path", Source: "transcript", Value: transcript}
	}
	shell.projection = herdr.Projection{
		Agents: []herdr.Agent{{
			TerminalID:   "term-1",
			PaneID:       "p1",
			Status:       "working",
			AgentSession: session,
		}},
	}
	card := agent.AgentCardModel{
		Key:      agent.AgentKey{InstanceID: "inst-1", TerminalID: "term-1"},
		Provider: history.AgentClaudeCode,
		Title:    "Scout",
		PaneID:   "p1",
	}
	return shell, card
}

// TestBindLiveAgentStreamsProviderTranscript pins the live chat binding
// (CONV-08/CONV-11..19): identity → exact source; hydration lands on the
// bind lane; appends surface through deterministic pump steps; the
// provisional Claude tail renders without committing.
func TestBindLiveAgentStreamsProviderTranscript(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "live.jsonl")
	initial := "{\"type\":\"user\",\"cwd\":\"/work/demo\",\"message\":{\"content\":\"hello\"}}\n" +
		"{\"type\":\"user\",\"cwd\":\"/work/demo\",\"message\":{\"content\":\"again\"}}\n"
	if err := os.WriteFile(path, []byte(initial), 0o600); err != nil {
		t.Fatal(err)
	}
	shell, card := liveBindShell(t, "claude-code", path)
	shell.chatLiveBackstopOverride = 10 * time.Second

	if !shell.bindLiveAgent(card.Key) {
		t.Fatal("live binding with a path identity must take over")
	}
	if shell.chatConversationID != "live:claude-code:term-1" {
		t.Fatalf("conversation id = %q", shell.chatConversationID)
	}
	if len(shell.chatTurns) != 2 || shell.chatLoading {
		t.Fatalf("hydration turns = %d loading = %v", len(shell.chatTurns), shell.chatLoading)
	}

	// An appended assistant turn renders as the provisional tail only.
	file, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		t.Fatal(err)
	}
	_, err = file.WriteString("{\"type\":\"assistant\",\"message\":{\"id\":\"m1\",\"content\":[{\"type\":\"text\",\"text\":\"working on it\"}]}}\n")
	if err != nil {
		t.Fatal(err)
	}
	file.Close()
	shell.chatLivePump.PumpOnce()
	if len(shell.chatTurns) != 3 {
		t.Fatalf("pending tail must render provisionally: %d turns", len(shell.chatTurns))
	}
	if shell.chatTurns[2].UserRow || shell.chatTurns[2].Narration != "working on it" {
		t.Fatalf("provisional tail turn = %+v", shell.chatTurns[2])
	}

	// The next user turn commits the tail: three committed rows, no pending.
	file, err = os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		t.Fatal(err)
	}
	_, err = file.WriteString("{\"type\":\"user\",\"message\":{\"content\":\"and?\"}}\n")
	if err != nil {
		t.Fatal(err)
	}
	file.Close()
	shell.chatLivePump.PumpOnce()
	if len(shell.chatTurns) != 4 {
		t.Fatalf("committed tail turns = %d, want 4", len(shell.chatTurns))
	}
	// No duplicated assistant row and no lingering provisional turn.
	found := 0
	for _, turn := range shell.chatTurns {
		if turn.Narration == "working on it" {
			found++
		}
	}
	if found != 1 {
		t.Fatalf("committed assistant narration found %d times", found)
	}
}

// TestBindLiveAgentSwitchStopsTailAndIgnoresStaleSyncs pins the lifecycle:
// switching to a history conversation stops the pump binding and a stale
// late sync can never apply over the newer bind.
func TestBindLiveAgentSwitchStopsTailAndIgnoresStaleSyncs(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "live.jsonl")
	if err := os.WriteFile(path, []byte("{\"type\":\"user\",\"message\":{\"content\":\"hello\"}}\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	shell, card := liveBindShell(t, "claude-code", path)
	shell.chatLiveBackstopOverride = 10 * time.Second
	if !shell.bindLiveAgent(card.Key) {
		t.Fatal("live binding must take over")
	}
	pump := shell.chatLivePump
	if pump == nil {
		t.Fatal("binding must expose its pump")
	}

	// Switch to a history conversation: the live binding clears.
	fake := newFakeHistoryView()
	shell.hist.service = fake
	done := make(chan struct{})
	go func() {
		defer close(done)
		shell.bindChatConversation("codex:second")
	}()
	call := <-fake.openCalls
	call.release <- openResult{}
	<-done
	if shell.chatLiveCancel != nil || shell.chatLivePump != nil || shell.chatLivePath != "" {
		t.Fatal("switching conversations must stop the live tail binding")
	}

	// A stale late sync from the old pump cannot apply (generation guard).
	// Join the pump goroutine first: it observes the cancellation and stops.
	select {
	case <-pump.Done:
	case <-time.After(2 * time.Second):
		t.Fatal("unbound pump did not stop")
	}
	file, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		t.Fatal(err)
	}
	_, err = file.WriteString("{\"type\":\"user\",\"message\":{\"content\":\"stale\"}}\n")
	if err != nil {
		t.Fatal(err)
	}
	file.Close()
	turnsBefore := len(shell.chatTurns)
	pump.PumpOnce()
	if len(shell.chatTurns) != turnsBefore {
		t.Fatalf("stale live sync applied over history bind: %d → %d", turnsBefore, len(shell.chatTurns))
	}
}

// TestBindLiveAgentFailsClosedWithoutIdentity pins the fallback: agents
// without a measured identity or with an unsupported provider never guess a
// live source and leave the chat surface to the history bind path.
func TestBindLiveAgentFailsClosedWithoutIdentity(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "live.jsonl")

	// No agent_session at all.
	shell, card := liveBindShell(t, "", path)
	if shell.bindLiveAgent(card.Key) {
		t.Fatal("agent without a session identity must not bind live")
	}

	// Unsupported provider identity fails closed at the decoder factory.
	shell2, card2 := liveBindShell(t, "qoder", path)
	if shell2.bindLiveAgent(card2.Key) {
		t.Fatal("unsupported provider must not bind live")
	}
	if shell2.chatLiveCancel != nil {
		t.Fatal("failed bind must not leave a live binding behind")
	}
}

// appendLiveLines appends raw transcript lines to a live fixture file.
func appendLiveLines(t *testing.T, path string, lines ...string) {
	t.Helper()
	file, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		t.Fatal(err)
	}
	for _, line := range lines {
		if _, err := file.WriteString(line + "\n"); err != nil {
			t.Fatal(err)
		}
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
}

func countUserRows(turns []conversation.TimelineTurn, text string) int {
	count := 0
	for _, turn := range turns {
		if turn.UserRow && turn.Text == text {
			count++
		}
	}
	return count
}

// TestChatPendingEchoRendersAndConsumesExactlyOnce pins §12 end to end over
// the live tail: a SentNow commit shows one temporary User row; the provider
// semantic source's matching row after the baseline consumes it exactly once
// and only the committed row remains visible.
func TestChatPendingEchoRendersAndConsumesExactlyOnce(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "live.jsonl")
	if err := os.WriteFile(path, []byte(
		"{\"type\":\"user\",\"message\":{\"content\":\"hello\"}}\n"+
			"{\"type\":\"user\",\"message\":{\"content\":\"again\"}}\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	shell, card := liveBindShell(t, "claude-code", path)
	shell.chatLiveBackstopOverride = 10 * time.Second
	if !shell.bindLiveAgent(card.Key) {
		t.Fatal("live binding must take over")
	}
	shell.chatPaneID = "p1"
	if len(shell.chatTurns) != 2 {
		t.Fatalf("hydrated turns = %d, want 2", len(shell.chatTurns))
	}

	// SentNow accepted: one temporary pending row appears.
	shell.chatAfterPrompt("req-1", "run tests", conversation.DispositionSentNow, nil)
	if shell.chatPendingEcho == nil || shell.chatPendingEcho.Baseline != 2 || shell.chatPendingEcho.PaneID != "p1" {
		t.Fatalf("pending echo = %#v", shell.chatPendingEcho)
	}
	if len(shell.chatTurns) != 3 || countUserRows(shell.chatTurns, "run tests") != 1 {
		t.Fatalf("pending row not rendered: %+v", shell.chatTurns)
	}

	// Duplicate wake without new bytes: the echo stays pending, one row.
	shell.chatLivePump.PumpOnce()
	if shell.chatPendingEcho == nil || len(shell.chatTurns) != 3 {
		t.Fatalf("idle wake disturbed the pending echo: %#v / %d turns", shell.chatPendingEcho, len(shell.chatTurns))
	}

	// The provider semantic source echoes after the baseline (an assistant
	// row then the matching User row): consumed exactly once, one visible
	// User row remains.
	appendLiveLines(t, path,
		"{\"type\":\"assistant\",\"message\":{\"id\":\"m1\",\"content\":[{\"type\":\"text\",\"text\":\"Running.\"}]}}",
		"{\"type\":\"user\",\"message\":{\"content\":\"run tests\"}}")
	shell.chatLivePump.PumpOnce()
	if shell.chatPendingEcho != nil {
		t.Fatalf("echo not consumed: %#v", shell.chatPendingEcho)
	}
	if len(shell.chatTurns) != 4 || countUserRows(shell.chatTurns, "run tests") != 1 {
		t.Fatalf("visible rows after consumption: %+v", shell.chatTurns)
	}

	// Repeated syncs have zero effect (exactly once).
	shell.chatLivePump.PumpOnce()
	if shell.chatPendingEcho != nil || len(shell.chatTurns) != 4 {
		t.Fatalf("repeat sync changed state: %#v / %d turns", shell.chatPendingEcho, len(shell.chatTurns))
	}
}

// TestChatPendingEchoBaselineOrderRule pins the order evidence: an identical
// User row already committed before the baseline never consumes the echo;
// only the row written after the submission does.
func TestChatPendingEchoBaselineOrderRule(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "live.jsonl")
	if err := os.WriteFile(path, []byte(
		"{\"type\":\"user\",\"message\":{\"content\":\"run tests\"}}\n"+
			"{\"type\":\"user\",\"message\":{\"content\":\"hello\"}}\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	shell, card := liveBindShell(t, "claude-code", path)
	shell.chatLiveBackstopOverride = 10 * time.Second
	if !shell.bindLiveAgent(card.Key) {
		t.Fatal("live binding must take over")
	}
	shell.chatPaneID = "p1"

	// Re-sending text that already exists in the transcript: the earlier
	// row must not confirm it.
	shell.chatAfterPrompt("req-1", "run tests", conversation.DispositionSentNow, nil)
	if shell.chatPendingEcho == nil {
		t.Fatal("echo missing")
	}
	shell.chatLivePump.PumpOnce()
	if shell.chatPendingEcho == nil || shell.chatPendingEcho.Baseline != 2 {
		t.Fatalf("pre-baseline row consumed the echo: %#v", shell.chatPendingEcho)
	}

	// The provider's real echo for the new submission consumes it.
	appendLiveLines(t, path, "{\"type\":\"user\",\"message\":{\"content\":\"run tests\"}}")
	shell.chatLivePump.PumpOnce()
	if shell.chatPendingEcho != nil {
		t.Fatalf("post-baseline echo not consumed: %#v", shell.chatPendingEcho)
	}
	if countUserRows(shell.chatTurns, "run tests") != 2 {
		t.Fatalf("user rows after consumption: %+v", shell.chatTurns)
	}
}

// TestChatPendingEchoRebindRules pin §12 rebind semantics: a rebind onto
// another pane's Agent drops the unrelated echo; a same-pane rebind keeps
// it; binding to a history conversation (which cannot reconcile against the
// provider source) drops it.
func TestChatPendingEchoRebindRules(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "live.jsonl")
	if err := os.WriteFile(path, []byte("{\"type\":\"user\",\"message\":{\"content\":\"hello\"}}\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	shell, card := liveBindShell(t, "claude-code", path)
	shell.chatLiveBackstopOverride = 10 * time.Second

	// A second agent on another pane in the same projection.
	shell.projection.Agents = append(shell.projection.Agents, herdr.Agent{
		TerminalID:   "term-2",
		PaneID:       "p2",
		Status:       "working",
		AgentSession: &herdr.AgentSessionIdentity{Agent: "claude-code", Kind: "path", Source: "transcript", Value: path},
	})
	other := agent.AgentCardModel{
		Key:      agent.AgentKey{InstanceID: "inst-1", TerminalID: "term-2"},
		Provider: history.AgentClaudeCode,
		Title:    "Ranger",
		PaneID:   "p2",
	}

	if !shell.bindLiveAgent(card.Key) {
		t.Fatal("live binding must take over")
	}
	shell.chatPaneID = "p1"
	shell.chatAfterPrompt("req-1", "run tests", conversation.DispositionSentNow, nil)
	if shell.chatPendingEcho == nil {
		t.Fatal("echo missing after send")
	}

	// Rebind onto another pane's Agent: the unrelated echo drops.
	shell.openChat(other)
	if shell.chatPendingEcho != nil {
		t.Fatalf("cross-pane rebind kept the echo: %#v", shell.chatPendingEcho)
	}

	// Same-pane rebind (History Composer continuation): the composer now
	// targets the current pane; rebinding onto it again keeps the echo.
	shell.chatAfterPrompt("req-2", "ship it", conversation.DispositionSentNow, nil)
	if shell.chatPendingEcho == nil {
		t.Fatal("second echo missing")
	}
	shell.openChat(other)
	if shell.chatPendingEcho == nil || shell.chatPendingEcho.Text != "ship it" {
		t.Fatalf("same-pane rebind dropped the echo: %#v", shell.chatPendingEcho)
	}

	// History rebind cannot reconcile: the echo drops.
	fake := newFakeHistoryView()
	shell.hist.service = fake
	done := make(chan struct{})
	go func() {
		defer close(done)
		shell.bindChatConversation("codex:second")
	}()
	call := <-fake.openCalls
	call.release <- openResult{}
	<-done
	if shell.chatPendingEcho != nil {
		t.Fatalf("history rebind kept the echo: %#v", shell.chatPendingEcho)
	}
}

// TestChatQueuedDispositionEnqueuesInLedger pins the §11 production wiring:
// a QueuedAfterTurn prompt lands in the service-owned delivery ledger with
// the enqueue-time identity coordinate; the composer cancel cancels the
// ledger item; an unresolved identity refuses to queue.
func TestChatQueuedDispositionEnqueuesInLedger(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "live.jsonl")
	if err := os.WriteFile(path, []byte("{\"type\":\"user\",\"message\":{\"content\":\"hello\"}}\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	shell, _ := liveBindShell(t, "claude-code", path)
	shell.chatPaneID = "p1"
	shell.chatConversationID = "live:claude-code:term-1"
	shell.chatPhase = agent.AgentRuntimePhase("working")

	fingerprint := shell.liveFingerprintForPane("p1")
	if fingerprint == "" {
		t.Fatal("the projection identity must resolve a fingerprint")
	}

	// QueuedAfterTurn goes through the real ledger.
	shell.chatDraft = "follow up please"
	shell.sendChatPrompt()
	if shell.chatQueuedText != "follow up please" {
		t.Fatalf("queued text = %q outcome = %q", shell.chatQueuedText, shell.chatOutcome)
	}
	item, ok := shell.launch.Queue().Get(conversation.ConversationID(shell.chatConversationID))
	if !ok || item.Text != "follow up please" || item.Fingerprint != fingerprint || item.State != conversation.Queued {
		t.Fatalf("ledger item = %+v ok %v", item, ok)
	}
	if shell.chatDraft != "" {
		t.Fatalf("draft = %q after enqueue", shell.chatDraft)
	}

	// A second prompt for the same conversation stays in the single slot.
	shell.chatDraft = "second"
	shell.sendChatPrompt()
	if shell.chatOutcome == "" {
		t.Fatal("second enqueue must surface the single-slot refusal")
	}

	// Composer cancel cancels the ledger item (before delivery commit).
	shell.cancelChatFollowUp()
	if shell.chatQueuedText != "" {
		t.Fatalf("queued chip = %q after cancel", shell.chatQueuedText)
	}
	item, _ = shell.launch.Queue().Get(conversation.ConversationID(shell.chatConversationID))
	if item.State != conversation.Cancelled {
		t.Fatalf("ledger state after cancel = %q", item.State)
	}

	// Unresolved identity refuses to queue instead of guessing: the pane
	// holds no provable session identity.
	shell.chatPhase = agent.AgentRuntimePhase("working")
	shell.chatDraft = "again"
	shell.projection = herdr.Projection{}
	shell.sendChatPrompt()
	if shell.chatQueuedText != "" || shell.chatOutcome == "" {
		t.Fatalf("unresolved identity must refuse: chip %q outcome %q", shell.chatQueuedText, shell.chatOutcome)
	}
}
