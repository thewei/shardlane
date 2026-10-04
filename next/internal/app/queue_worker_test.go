package app

import (
	"context"
	"errors"
	"path/filepath"
	"testing"
	"time"

	"github.com/wh-studio/herdr-client/next/internal/conversation"
)

// TestLaunchServiceFollowUpLedger pins the production queue ownership: the
// service owns the ledger, exposes it as the handoff pending-operation
// seam, and the worker lifecycle is idempotent.
func TestLaunchServiceFollowUpLedger(t *testing.T) {
	service := NewLaunchService(nil)

	if service.Queue() == nil {
		t.Fatal("the service must own the follow-up queue")
	}
	pending := service.PendingOperations()
	if pending == nil {
		t.Fatal("the handoff pending-operation seam must be the owned ledger")
	}
	if pending.HasUnresolvedForConversation("live:claude-code:missing") {
		t.Fatal("unknown conversation reported unresolved")
	}

	// Enqueue + wake: the item lands in the ledger.
	id := conversation.ConversationID("live:claude-code:pane-7")
	item := conversation.QueueItem{
		RequestID:      "req-1",
		ConversationID: id,
		AgentPaneID:    "pane-7",
		Fingerprint:    "fp-1",
		Text:           "continue",
		Instance:       "inst-1",
	}
	if err := service.EnqueueFollowUp(item); err != nil {
		t.Fatal(err)
	}
	if got, ok := service.Queue().Get(id); !ok || got.RequestID != "req-1" {
		t.Fatalf("queued item = %+v ok %v", got, ok)
	}

	// Worker lifecycle: start is idempotent and stop is safe.
	service.StartFollowUpWorker()
	service.StartFollowUpWorker()
	service.StopFollowUpWorker()
	service.StopFollowUpWorker()

	// The reservations coordinator is shared per conversation (worker +
	// handoff serialize on the same lock).
	first := service.reservations.For(string(id))
	if first == nil || service.reservations.For(string(id)) != first {
		t.Fatal("reservation coordinator must be stable per conversation")
	}
}

// TestHerdrDeliveryTransportNilManager pins the fail-closed transport: an
// unavailable Herdr transport retains the queue (never sends, never
// panics) and never reports uncertainty.
func TestHerdrDeliveryTransportNilManager(t *testing.T) {
	transport := &herdrDeliveryTransport{manager: nil}
	item := conversation.QueueItem{Instance: "inst-1", AgentPaneID: "pane-7"}

	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	if err := transport.WaitTurnBoundary(ctx, item); err == nil {
		t.Fatal("unavailable transport must fail the wait (retains the queue)")
	}
	if _, err := transport.OccupantFingerprint(ctx, item); err == nil {
		t.Fatal("unavailable transport must fail the occupant read")
	}
	if err := transport.Deliver(ctx, item); err == nil {
		t.Fatal("unavailable transport must refuse delivery")
	}
	if transport.IsUncertain(errors.New("any error")) {
		t.Fatal("transport unavailability is not delivery uncertainty")
	}
}

// TestLaunchServiceLedgerPersistence pins the §9.3 durable ledger: every
// mutation persists atomically, a fresh service restores the exact items,
// and a crash mid-delivery restarts as the uncertainty tombstone — never a
// silent resend.
func TestLaunchServiceLedgerPersistence(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "follow-up-ledger.json")

	service := NewLaunchService(nil)
	service.AttachLedgerPath(path)
	if err := service.EnqueueFollowUp(conversation.QueueItem{
		RequestID:      "req-1",
		ConversationID: "live:claude-code:pane-7",
		AgentPaneID:    "pane-7",
		Fingerprint:    "fp-1",
		Text:           "continue",
		Instance:       "inst-1",
		State:          conversation.Queued,
	}); err != nil {
		t.Fatal(err)
	}

	// The enqueue persisted synchronously via OnChanged.
	restored := NewLaunchService(nil)
	restored.AttachLedgerPath(path)
	item, ok := restored.Queue().Get("live:claude-code:pane-7")
	if !ok || item.RequestID != "req-1" || item.State != conversation.Queued {
		t.Fatalf("restored item = %+v ok %v", item, ok)
	}

	// A crash mid-delivery: claim on the restored service (persisting the
	// Delivering state exactly as a crash would leave it), then restart.
	if _, err := restored.Queue().ClaimDelivery("live:claude-code:pane-7", "req-1"); err != nil {
		t.Fatal(err)
	}
	item, _ = restored.Queue().Get("live:claude-code:pane-7")
	if item.State != conversation.Delivering {
		t.Fatalf("pre-crash state = %q", item.State)
	}
	restarted := NewLaunchService(nil)
	restarted.AttachLedgerPath(path)
	item, ok = restarted.Queue().Get("live:claude-code:pane-7")
	if !ok || item.State != conversation.DeliveryUncertain {
		t.Fatalf("post-crash state = %+v ok %v", item, ok)
	}
	if item.Detail == "" {
		t.Fatal("the uncertainty tombstone must carry inspect guidance")
	}
	// The tombstone never auto-retries: the worker skips it.
	if pending := restarted.PendingOperations().HasUnresolvedForConversation("live:claude-code:pane-7"); !pending {
		t.Fatal("the restored tombstone must fence handoffs until acknowledged")
	}

	// Later mutations persist too: a new follow-up on the restarted service
	// lands in the file, and the next restore sees it.
	if err := restarted.EnqueueFollowUp(conversation.QueueItem{
		RequestID:      "req-2",
		ConversationID: "live:claude-code:pane-8",
		AgentPaneID:    "pane-8",
		Fingerprint:    "fp-8",
		Text:           "again",
		Instance:       "inst-1",
		State:          conversation.Queued,
	}); err != nil {
		t.Fatal(err)
	}
	again := NewLaunchService(nil)
	again.AttachLedgerPath(path)
	if item, ok := again.Queue().Get("live:claude-code:pane-8"); !ok || item.RequestID != "req-2" {
		t.Fatalf("second restore = %+v ok %v", item, ok)
	}
	if item, _ := again.Queue().Get("live:claude-code:pane-7"); item.State != conversation.DeliveryUncertain {
		t.Fatalf("tombstone restore = %+v", item)
	}

	// Attach is idempotent and empty paths are ignored.
	service.AttachLedgerPath(path)
	service.AttachLedgerPath("")
}
