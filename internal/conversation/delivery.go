package conversation

import (
	"context"
	"sync"
)

// Queue-worker delivery (0.6 §11.3/§11.4): the production owner of the
// Delivering→Delivered/DeliveryUncertain state machine. The pipeline is
// exact-target wait → occupant identity revalidation → atomic claim →
// exactly one semantic prompt → reconcile result. Blocked and timeout
// retain the queue; identity change and definite rejection fail closed
// into recoverable text; uncertainty is a tombstone that never auto-retries
// and never pretends the text was unsent; cancel-A/enqueue-B races cannot
// let an old worker send B (claims are request-id keyed).

// DeliveryTransport is the semantic delivery seam of the worker. Production
// binds the verified Herdr prompt path; tests bind deterministic scripts.
type DeliveryTransport interface {
	// WaitTurnBoundary waits for the exact target's working turn to settle.
	// Any error — blocked, timeout, transport — retains the queue.
	WaitTurnBoundary(ctx context.Context, item QueueItem) error
	// OccupantFingerprint reads the exact target's current session
	// fingerprint; empty means the pane holds no provable session.
	OccupantFingerprint(ctx context.Context, item QueueItem) (string, error)
	// Deliver submits exactly one semantic prompt.
	Deliver(ctx context.Context, item QueueItem) error
	// IsUncertain reports a typed delivery uncertainty.
	IsUncertain(err error) bool
}

// Reservations is the per-Conversation reservation coordinator (0.6
// §9.2/§19): one lock per conversation shared by the delivery worker, the
// semantic prompt path, and the live handoff, so a handoff holding the
// conversation defers the worker and two workers cannot deliver the same
// conversation concurrently.
type Reservations struct {
	locks sync.Map
}

func NewReservations() *Reservations {
	return &Reservations{}
}

// For returns the conversation's reservation lock; an empty conversation
// returns an untyped nil (callers treat nil as "no reservation").
func (r *Reservations) For(conversation string) sync.Locker {
	if conversation == "" {
		return nil
	}
	lock, _ := r.locks.LoadOrStore(conversation, &sync.Mutex{})
	return lock.(sync.Locker)
}

// DeliveryOutcome is the deterministic result of one worker cycle.
type DeliveryOutcome struct {
	Item     QueueItem
	Sent     bool
	Retained bool // stays queued for a later cycle (blocked/timeout/transport)
	Reason   string
}

// DeliveryWorker owns the §11.3 pipeline. Run is the background loop;
// DeliverOnce is the deterministic step tests drive directly. All queue
// mutations go through the FollowUpQueue's atomic operations, so exactly
// one worker can own a claim.
type DeliveryWorker struct {
	Queue        *FollowUpQueue
	Transport    DeliveryTransport
	Reservations *Reservations
	// Wake signals (non-blocking) that queued items may be ready.
	Wake <-chan struct{}
	// OnState observes every state transition for UI projection; the worker
	// never blocks on it.
	OnState func(item QueueItem)
}

func NewDeliveryWorker(queue *FollowUpQueue, transport DeliveryTransport, reservations *Reservations, wake <-chan struct{}) *DeliveryWorker {
	return &DeliveryWorker{Queue: queue, Transport: transport, Reservations: reservations, Wake: wake}
}

func (w *DeliveryWorker) observe(item QueueItem) {
	if w.OnState != nil {
		w.OnState(item)
	}
}

// Run drains on every wake until ctx is cancelled.
func (w *DeliveryWorker) Run(ctx context.Context) {
	for {
		select {
		case <-ctx.Done():
			return
		case <-w.Wake:
			w.DrainAll(ctx)
		}
	}
}

// DrainAll processes every currently-pending conversation once.
func (w *DeliveryWorker) DrainAll(ctx context.Context) {
	for _, id := range w.Queue.QueuedConversations() {
		if ctx.Err() != nil {
			return
		}
		w.DeliverOnce(ctx, id)
	}
}

// DeliverOnce runs one pipeline cycle for one conversation. It never sends
// more than one prompt and always reconciles the queue state.
func (w *DeliveryWorker) DeliverOnce(ctx context.Context, id ConversationID) DeliveryOutcome {
	item, ok := w.Queue.Get(id)
	if !ok {
		return DeliveryOutcome{Item: item, Reason: "no queued item"}
	}
	switch item.State {
	case Queued, WaitingForTurnBoundary, FailedRecoverable:
	default:
		// Delivered / DeliveryUncertain / Cancelled / Delivering: nothing
		// this cycle may do (uncertainty never auto-retries; a claim in
		// flight belongs to another worker).
		return DeliveryOutcome{Item: item, Reason: "item is not pending delivery"}
	}

	if w.Reservations != nil {
		if lock := w.Reservations.For(string(id)); lock != nil {
			lock.Lock()
			defer lock.Unlock()
		}
	}

	if item.State == Queued {
		claimed, err := w.Queue.Transition(id, WaitingForTurnBoundary, "delivery worker took the item")
		if err != nil {
			return DeliveryOutcome{Item: item, Reason: "claim race: " + err.Error()}
		}
		item = claimed
		w.observe(item)
	}

	// Exact-target wait: blocked/timeout retain the queue; they never send.
	if err := w.Transport.WaitTurnBoundary(ctx, item); err != nil {
		return DeliveryOutcome{Item: item, Retained: true, Reason: "turn boundary not reached: " + err.Error()}
	}

	// Occupant identity revalidation: a changed occupant fails closed into
	// recoverable text (the user can requeue); an unreadable occupant
	// retains (retry later) instead of guessing.
	fingerprint, err := w.Transport.OccupantFingerprint(ctx, item)
	if err != nil {
		return DeliveryOutcome{Item: item, Retained: true, Reason: "occupant unreadable: " + err.Error()}
	}
	if fingerprint == "" || fingerprint != item.Fingerprint {
		updated, transitionErr := w.Queue.Transition(id, FailedRecoverable, "the conversation occupant changed since the prompt was queued")
		if transitionErr != nil {
			return DeliveryOutcome{Item: item, Reason: "reconcile race: " + transitionErr.Error()}
		}
		w.observe(updated)
		return DeliveryOutcome{Item: updated, Reason: "source identity changed"}
	}

	// Atomic claim: exactly one worker owns the delivery; a lost claim
	// (replaced by a newer request, or another worker took it) never sends.
	claimed, err := w.Queue.ClaimDelivery(id, item.RequestID)
	if err != nil {
		return DeliveryOutcome{Item: item, Reason: "claim lost: " + err.Error()}
	}
	w.observe(claimed)

	// Exactly one semantic prompt, then reconcile the result.
	deliverErr := w.Transport.Deliver(ctx, claimed)
	switch {
	case deliverErr == nil:
		delivered, completeErr := w.Queue.CompleteDelivery(id, true, "")
		if completeErr != nil {
			return DeliveryOutcome{Item: claimed, Sent: true, Reason: "complete race: " + completeErr.Error()}
		}
		w.observe(delivered)
		return DeliveryOutcome{Item: delivered, Sent: true}
	case w.Transport.IsUncertain(deliverErr):
		tombstoned, completeErr := w.Queue.CompleteDelivery(id, false, "delivery uncertain — inspect before retry")
		if completeErr != nil {
			return DeliveryOutcome{Item: claimed, Sent: true, Reason: "complete race: " + completeErr.Error()}
		}
		w.observe(tombstoned)
		return DeliveryOutcome{Item: tombstoned, Sent: true, Reason: "delivery uncertain"}
	default:
		failed, transitionErr := w.Queue.Transition(id, FailedRecoverable, deliverErr.Error())
		if transitionErr != nil {
			return DeliveryOutcome{Item: claimed, Sent: true, Reason: "reconcile race: " + transitionErr.Error()}
		}
		w.observe(failed)
		return DeliveryOutcome{Item: failed, Sent: true, Reason: "prompt rejected: " + deliverErr.Error()}
	}
}
