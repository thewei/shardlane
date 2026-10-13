package conversation

import (
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/wh-studio/herdr-client/internal/agent"
)

func TestSessionFingerprintIsStableAndDistinct(t *testing.T) {
	a := SessionIdentity{Provider: "claude-code", Kind: "id", Source: "provider", Value: "sess-1"}
	b := SessionIdentity{Provider: "claude-code", Kind: "id", Source: "provider", Value: "sess-1"}
	c := SessionIdentity{Provider: "codex", Kind: "id", Source: "provider", Value: "sess-1"}
	if a.Fingerprint() != b.Fingerprint() {
		t.Fatal("same identity must fingerprint identically")
	}
	if a.Fingerprint() == c.Fingerprint() {
		t.Fatal("different provider must fingerprint differently")
	}
}

func TestDispositionFor(t *testing.T) {
	cases := map[agent.AgentSendability]PromptDisposition{
		agent.Sendable:      DispositionSentNow,
		agent.MidTurn:       DispositionQueuedAfterTurn,
		agent.NeedsTerminal: DispositionNeedsTerminal,
		agent.SendUnknown:   DispositionUnknown,
	}
	for sendability, want := range cases {
		if got := DispositionFor(sendability); got != want {
			t.Fatalf("DispositionFor(%v) = %v", sendability, got)
		}
	}
}

func TestFollowUpQueueLifecycle(t *testing.T) {
	queue := NewFollowUpQueue()
	id := ConversationID("history:claude-code:sess-1")

	if err := queue.Enqueue(QueueItem{RequestID: "r1", ConversationID: id, Text: "follow up"}); err != nil {
		t.Fatal(err)
	}
	if err := queue.Enqueue(QueueItem{RequestID: "r2", ConversationID: id, Text: "second"}); err == nil {
		t.Fatal("second live enqueue must be rejected")
	}

	// Atomic claim: exactly one worker wins.
	if _, err := queue.ClaimDelivery(id, "r1"); err != nil {
		t.Fatal(err)
	}
	if _, err := queue.ClaimDelivery(id, "r1"); !errors.Is(err, ErrAlreadyDelivered) {
		t.Fatalf("double claim err = %v", err)
	}
	// Cancel racing delivery is refused.
	if _, err := queue.Cancel(id); err == nil {
		t.Fatal("cancel must race-refuse during delivery")
	}
	if _, err := queue.CompleteDelivery(id, true, ""); err != nil {
		t.Fatal(err)
	}
	item, _ := queue.Get(id)
	if item.State != Delivered {
		t.Fatalf("state = %v", item.State)
	}

	// Fresh enqueue after delivery replaces the old record.
	if err := queue.Enqueue(QueueItem{RequestID: "r3", ConversationID: id, Text: "next"}); err != nil {
		t.Fatal(err)
	}
	if item, _ := queue.Get(id); item.RequestID != "r3" {
		t.Fatalf("replacement item = %+v", item)
	}
	// Old worker claiming by the old request id loses.
	if _, err := queue.ClaimDelivery(id, "r1"); err == nil {
		t.Fatal("stale worker claim must lose")
	}
}

func TestFollowUpQueueUncertainTombstone(t *testing.T) {
	queue := NewFollowUpQueue()
	id := ConversationID("live:codex:s9")
	_ = queue.Enqueue(QueueItem{RequestID: "r9", ConversationID: id, Text: "x"})
	_, _ = queue.ClaimDelivery(id, "r9")
	if _, err := queue.CompleteDelivery(id, false, "response not read"); err != nil {
		t.Fatal(err)
	}
	item, _ := queue.Get(id)
	if item.State != DeliveryUncertain {
		t.Fatalf("state = %v", item.State)
	}
	// Uncertainty is a tombstone: no auto retry, no cancel back to queued.
	if err := queue.Enqueue(QueueItem{RequestID: "r10", ConversationID: id, Text: "y"}); err == nil {
		t.Fatal("uncertain tombstone must not be silently replaced while active")
	}
}

func TestInteractionBrokerCASAndOccupantSupersession(t *testing.T) {
	broker := NewInteractionBroker(func() int64 { return 42 })
	id := ConversationID("live:claude-code:s1")
	interaction := PendingInteraction{
		ID: "i1", ConversationID: id, AgentPaneID: "p1",
		Fingerprint: "fp", Kind: InteractionPermission,
		Prompt:  "Allow write?",
		Options: []string{"Approve", "Deny"},
	}
	if err := broker.Publish(interaction); err != nil {
		t.Fatal(err)
	}

	if _, err := broker.ResolveCAS("i1", "Rewrite everything"); err == nil {
		t.Fatal("non-provider option must fail closed")
	}
	resolution, err := broker.ResolveCAS("i1", "Approve")
	if err != nil {
		t.Fatal(err)
	}
	if resolution.Option != "Approve" || resolution.ResolvedAtMS != 42 {
		t.Fatalf("resolution = %+v", resolution)
	}
	// Second resolver loses the CAS race.
	if _, err := broker.ResolveCAS("i1", "Deny"); !errors.Is(err, ErrInteractionResolved) {
		t.Fatalf("CAS race err = %v", err)
	}

	// A newer interaction supersedes the pending one for the conversation.
	next := interaction
	next.ID = "i2"
	next.Kind = InteractionQuestion
	next.Prompt = "Which database?"
	if err := broker.Publish(next); err != nil {
		t.Fatal(err)
	}
	if pending := broker.Pending(); len(pending) != 1 || pending[0].ID != "i2" {
		t.Fatalf("pending = %+v", pending)
	}
	broker.CancelForConversation(id)
	if pending := broker.Pending(); len(pending) != 0 {
		t.Fatalf("cancel left = %+v", pending)
	}
}

func TestDeriveTimelineTurns(t *testing.T) {
	thinking := "plan carefully"
	items := []ConversationItem{
		{Seq: 0, Kind: KindUser, Text: "Fix it"},
		{Seq: 1, Kind: KindAssistant, Text: "On it", Thinking: &thinking},
		{Seq: 2, Kind: KindTool, ToolCalls: []ConversationToolCall{{ID: "t1", Name: "grep"}}},
		{Seq: 3, Kind: KindTool, ToolCalls: []ConversationToolCall{{ID: "t2", Name: "sed"}}},
		{Seq: 4, Kind: KindAssistant, Text: "Done."},
		{Seq: 5, Kind: KindUser, Text: "Now explain"},
		{Seq: 6, Kind: KindAssistant, Text: "Explanation"},
	}

	turns := DeriveTimeline(items)
	if len(turns) != 4 {
		t.Fatalf("turns = %d: %+v", len(turns), turns)
	}
	if !turns[0].UserRow || turns[0].Text != "Fix it" {
		t.Fatalf("user turn = %+v", turns[0])
	}
	// Assistant turn: narration visible, thinking consolidated, 2 tool runs compacted.
	if turns[1].Narration != "On it\n\nDone." {
		t.Fatalf("narration = %q", turns[1].Narration)
	}
	if turns[1].Thinking == nil || *turns[1].Thinking != thinking {
		t.Fatalf("thinking = %v", turns[1].Thinking)
	}
	if !turns[1].Compacted || len(turns[1].ToolRuns) != 2 {
		t.Fatalf("tool compaction = %+v", turns[1])
	}
	if !turns[2].UserRow || turns[2].Text != "Now explain" {
		t.Fatalf("second user = %+v", turns[2])
	}
	if turns[3].Narration != "Explanation" || turns[3].Compacted {
		t.Fatalf("final turn = %+v", turns[3])
	}
}

func TestDeriveTimelineSingleToolNotCompacted(t *testing.T) {
	items := []ConversationItem{
		{Seq: 0, Kind: KindAssistant, Text: "work"},
		{Seq: 1, Kind: KindTool, ToolCalls: []ConversationToolCall{{ID: "t1", Name: "grep"}}},
	}
	turns := DeriveTimeline(items)
	if len(turns) != 1 || turns[0].Compacted || len(turns[0].ToolRuns) != 1 {
		t.Fatalf("single tool turn = %+v", turns[0])
	}
}

func TestQueueSendabilityGate(t *testing.T) {
	if maySend, disposition := SendabilityGate(agent.Sendable); !maySend || disposition != DispositionSentNow {
		t.Fatalf("sendable gate = (%v, %v)", maySend, disposition)
	}
	if maySend, disposition := SendabilityGate(agent.MidTurn); maySend || disposition != DispositionQueuedAfterTurn {
		t.Fatalf("mid-turn gate = (%v, %v)", maySend, disposition)
	}
	if maySend, disposition := SendabilityGate(agent.NeedsTerminal); maySend || disposition != DispositionNeedsTerminal {
		t.Fatalf("needs-terminal gate = (%v, %v)", maySend, disposition)
	}
	if maySend, disposition := SendabilityGate(agent.SendUnknown); maySend || disposition != DispositionUnknown {
		t.Fatalf("unknown gate must fail closed: (%v, %v)", maySend, disposition)
	}
	_ = time.Now
	_ = strings.TrimSpace
}

// TestDeriveTimelineStepOrder pins the turn's time chain: narration,
// thinking and tool segments keep their original interleaving in Steps;
// consecutive same-kind text merges, tools never do.
func TestDeriveTimelineStepOrder(t *testing.T) {
	firstThinking := "look around"
	items := []ConversationItem{
		{Seq: 1, Kind: KindAssistant, Text: "On it.", Thinking: &firstThinking},
		{Seq: 2, Kind: KindTool, ToolCalls: []ConversationToolCall{{ID: "t1", Name: "grep"}}},
		{Seq: 3, Kind: KindReasoning, Text: "verify result"},
		{Seq: 4, Kind: KindAssistant, Text: "Found it."},
	}
	turns := DeriveTimeline(items)
	if len(turns) != 1 {
		t.Fatalf("turns = %d", len(turns))
	}
	steps := turns[0].Steps
	want := []struct {
		kind string
		text string
		tool string
	}{
		{StepThinking, "look around", ""},
		{StepNarration, "On it.", ""},
		{StepTool, "", "grep"},
		{StepThinking, "verify result", ""},
		{StepNarration, "Found it.", ""},
	}
	if len(steps) != len(want) {
		t.Fatalf("steps = %d, want %d: %+v", len(steps), len(want), steps)
	}
	for i, w := range want {
		if steps[i].Kind != w.kind {
			t.Fatalf("step %d kind = %q, want %q", i, steps[i].Kind, w.kind)
		}
		if w.kind == StepTool {
			if steps[i].Tool == nil || steps[i].Tool.Name != w.tool {
				t.Fatalf("step %d tool = %+v", i, steps[i].Tool)
			}
			continue
		}
		if steps[i].Text != w.text {
			t.Fatalf("step %d text = %q, want %q", i, steps[i].Text, w.text)
		}
	}
	// Consecutive same-kind text merges (chunking must not fragment the chain).
	merged := []ConversationItem{
		{Seq: 1, Kind: KindAssistant, Text: "one"},
		{Seq: 2, Kind: KindAssistant, Text: "two"},
		{Seq: 3, Kind: KindAssistant, Text: "three"},
	}
	mergeTurns := DeriveTimeline(merged)
	if len(mergeTurns[0].Steps) != 1 || mergeTurns[0].Steps[0].Text != "one\n\ntwo\n\nthree" {
		t.Fatalf("consecutive narration must merge into one step: %+v", mergeTurns[0].Steps)
	}
	// Tools never merge.
	tools := []ConversationItem{
		{Seq: 1, Kind: KindTool, ToolCalls: []ConversationToolCall{
			{ID: "t1", Name: "grep"}, {ID: "t2", Name: "sed"},
		}},
	}
	toolTurns := DeriveTimeline(tools)
	if len(toolTurns[0].Steps) != 2 || toolTurns[0].Steps[0].Tool == nil || toolTurns[0].Steps[1].Tool == nil {
		t.Fatalf("tool steps must stay separate: %+v", toolTurns[0].Steps)
	}
}
