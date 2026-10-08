package conversation

import (
	"errors"
	"sort"
	"sync"
	"time"

	"github.com/wh-studio/herdr-client/internal/agent"
)

// QueueState is the follow-up queue lifecycle (0.6 §11.2). Blocked retains
// the queue but never sends; uncertainty never auto-retries.
type QueueState string

const (
	Queued                 QueueState = "queued"
	WaitingForTurnBoundary QueueState = "waiting-for-turn-boundary"
	Delivering             QueueState = "delivering"
	Delivered              QueueState = "delivered"
	FailedRecoverable      QueueState = "failed-recoverable"
	DeliveryUncertain      QueueState = "delivery-uncertain"
	Cancelled              QueueState = "cancelled"
)

// QueueItem is the one queued semantic follow-up per live Conversation
// (0.6 §11.1).
type QueueItem struct {
	RequestID      string
	ConversationID ConversationID
	AgentPaneID    string
	Fingerprint    string
	BaselineRev    uint64
	Text           string
	CreatedAt      time.Time
	State          QueueState
	Detail         string
	// Instance is the Herdr session the exact-target operations run against
	// (client routing fact for the transport; Herdr stays the authority).
	Instance string
}

// ErrNoQueueItem reports a missing queue entry.
var ErrNoQueueItem = errors.New("no queued item for this conversation")

// ErrAlreadyDelivered reports a cancel/claim racing a completed delivery.
var ErrAlreadyDelivered = errors.New("follow-up already delivered")

// FollowUpQueue is the one-item-per-conversation queue (0.6 §11). Exactly
// one delivery worker can own the atomic claim; cancel-A/enqueue-B races
// cannot let an old worker send B because the claim is keyed by item
// identity (request id), not by conversation alone.
type FollowUpQueue struct {
	mu    sync.Mutex
	items map[ConversationID]*QueueItem
	// onChanged fires after every mutation, outside the mutex — the
	// persistence owner serializes the ledger across restarts (0.6 §9.3).
	onChanged func()
}

func NewFollowUpQueue() *FollowUpQueue {
	return &FollowUpQueue{items: make(map[ConversationID]*QueueItem)}
}

// SetOnChanged wires the mutation observer (persistence). Must be called
// before concurrent use.
func (q *FollowUpQueue) SetOnChanged(fn func()) {
	q.mu.Lock()
	defer q.mu.Unlock()
	q.onChanged = fn
}

// changed reports the observer to call after the caller releases the mutex.
func (q *FollowUpQueue) changed() func() {
	return q.onChanged
}

// Enqueue accepts a follow-up for a conversation with no live queued item.
// A fresh enqueue after a delivered/cancelled item replaces the old record.
func (q *FollowUpQueue) Enqueue(item QueueItem) error {
	if item.RequestID == "" || item.ConversationID == "" || item.Text == "" {
		return errors.New("queued follow-up requires request id, conversation and text")
	}
	q.mu.Lock()
	existing, ok := q.items[item.ConversationID]
	if ok && isActiveQueueState(existing.State) {
		q.mu.Unlock()
		return errors.New("a follow-up is already queued for this conversation")
	}
	if item.State == "" {
		item.State = Queued
	}
	item.CreatedAt = time.Now()
	q.items[item.ConversationID] = &item
	changed := q.onChanged
	q.mu.Unlock()
	if changed != nil {
		changed()
	}
	return nil
}

func isActiveQueueState(state QueueState) bool {
	switch state {
	case Queued, WaitingForTurnBoundary, Delivering, FailedRecoverable:
		return true
	case DeliveryUncertain:
		// The tombstone blocks silent replacement: uncertainty never
		// auto-retries and must be acknowledged explicitly.
		return true
	default:
		return false
	}
}

// Get returns the current item for a conversation, if any.
func (q *FollowUpQueue) Get(id ConversationID) (QueueItem, bool) {
	q.mu.Lock()
	defer q.mu.Unlock()
	item, ok := q.items[id]
	if !ok {
		return QueueItem{}, false
	}
	return *item, true
}

// HasUnresolvedForConversation reports whether an older accepted operation
// still exists for the conversation (0.6 §19.2, the HANDOFF-06 fence):
// every state except Delivered and Cancelled still owns user intent — a
// recoverable failure or an uncertain tombstone included — so a live
// handoff snapshotting now would silently omit it. Mirrors the audited
// Host queue predicate.
func (q *FollowUpQueue) HasUnresolvedForConversation(id string) bool {
	q.mu.Lock()
	defer q.mu.Unlock()
	item, ok := q.items[ConversationID(id)]
	if !ok {
		return false
	}
	switch item.State {
	case Delivered, Cancelled:
		return false
	default:
		return true
	}
}

// Transition moves the item through a legal state and reports it.
func (q *FollowUpQueue) Transition(id ConversationID, to QueueState, detail string) (QueueItem, error) {
	q.mu.Lock()
	item, ok := q.items[id]
	if !ok {
		q.mu.Unlock()
		return QueueItem{}, ErrNoQueueItem
	}
	if !legalQueueTransition(item.State, to) {
		err := errors.New("illegal queue transition " + string(item.State) + " → " + string(to))
		q.mu.Unlock()
		return *item, err
	}
	item.State, item.Detail = to, detail
	changed := q.onChanged
	q.mu.Unlock()
	if changed != nil {
		changed()
	}
	return *item, nil
}

func legalQueueTransition(from, to QueueState) bool {
	switch from {
	case Queued:
		return to == WaitingForTurnBoundary || to == Delivering || to == Cancelled
	case WaitingForTurnBoundary:
		return to == Delivering || to == Cancelled || to == FailedRecoverable
	case Delivering:
		return to == Delivered || to == DeliveryUncertain || to == FailedRecoverable
	case FailedRecoverable:
		return to == Queued || to == Cancelled
	default:
		return false
	}
}

// ClaimDelivery is the atomic delivery claim (0.6 §11.3): exactly one
// worker moves a queued item into Delivering. A worker whose claim lost —
// because another worker took it or the item moved on — must not send.
func (q *FollowUpQueue) ClaimDelivery(id ConversationID, requestID string) (QueueItem, error) {
	q.mu.Lock()
	item, ok := q.items[id]
	if !ok {
		q.mu.Unlock()
		return QueueItem{}, ErrNoQueueItem
	}
	if item.RequestID != requestID {
		err := errors.New("claim lost: item was replaced by a newer request")
		q.mu.Unlock()
		return QueueItem{}, err
	}
	switch item.State {
	case Queued, WaitingForTurnBoundary, FailedRecoverable:
		item.State = Delivering
		changed := q.onChanged
		q.mu.Unlock()
		if changed != nil {
			changed()
		}
		return *item, nil
	default:
		q.mu.Unlock()
		return *item, ErrAlreadyDelivered
	}
}

// CompleteDelivery records the send result. Uncertainty is a tombstone: the
// item never auto-retries and never pretends the text was unsent.
func (q *FollowUpQueue) CompleteDelivery(id ConversationID, delivered bool, detail string) (QueueItem, error) {
	q.mu.Lock()
	item, ok := q.items[id]
	if !ok {
		q.mu.Unlock()
		return QueueItem{}, ErrNoQueueItem
	}
	if item.State != Delivering {
		err := errors.New("complete without an active delivery claim")
		q.mu.Unlock()
		return *item, err
	}
	if delivered {
		item.State = Delivered
	} else {
		item.State = DeliveryUncertain
	}
	item.Detail = detail
	changed := q.onChanged
	q.mu.Unlock()
	if changed != nil {
		changed()
	}
	return *item, nil
}

// Cancel is available only before the delivery commit (0.6 §11.4).
func (q *FollowUpQueue) Cancel(id ConversationID) (QueueItem, error) {
	q.mu.Lock()
	item, ok := q.items[id]
	if !ok {
		q.mu.Unlock()
		return QueueItem{}, ErrNoQueueItem
	}
	switch item.State {
	case Queued, WaitingForTurnBoundary, FailedRecoverable:
		item.State = Cancelled
		changed := q.onChanged
		q.mu.Unlock()
		if changed != nil {
			changed()
		}
		return *item, nil
	case Delivering:
		err := errors.New("cancel raced the delivery commit")
		q.mu.Unlock()
		return *item, err
	default:
		q.mu.Unlock()
		return *item, ErrAlreadyDelivered
	}
}

// Snapshot copies every ledger item in stable conversation order (all
// states — the persistence owner decides what a restart means for each).
func (q *FollowUpQueue) Snapshot() []QueueItem {
	q.mu.Lock()
	defer q.mu.Unlock()
	ids := make([]ConversationID, 0, len(q.items))
	for id := range q.items {
		ids = append(ids, id)
	}
	sort.Slice(ids, func(i, j int) bool { return ids[i] < ids[j] })
	items := make([]QueueItem, 0, len(ids))
	for _, id := range ids {
		items = append(items, *q.items[id])
	}
	return items
}

// Restore replaces the ledger with the given items (restart load).
func (q *FollowUpQueue) Restore(items []QueueItem) {
	q.mu.Lock()
	q.items = make(map[ConversationID]*QueueItem, len(items))
	for i := range items {
		item := items[i]
		q.items[item.ConversationID] = &item
	}
	changed := q.onChanged
	q.mu.Unlock()
	if changed != nil {
		changed()
	}
}

// SendabilityGate decides at commit time whether the queued item may be
// sent now (the service re-derives from live sendability — never from the
// composer label).
func SendabilityGate(sendability agent.AgentSendability) (bool, PromptDisposition) {
	switch DispositionFor(sendability) {
	case DispositionSentNow:
		return true, DispositionSentNow
	case DispositionQueuedAfterTurn:
		return false, DispositionQueuedAfterTurn
	case DispositionNeedsTerminal:
		return false, DispositionNeedsTerminal
	default:
		return false, DispositionUnknown
	}
}

// QueuedConversations lists the conversations whose items are pending
// delivery (queued, waiting for a turn boundary, or recoverably failed) —
// the worker's drain snapshot.
func (q *FollowUpQueue) QueuedConversations() []ConversationID {
	q.mu.Lock()
	defer q.mu.Unlock()
	ids := make([]ConversationID, 0, len(q.items))
	for id, item := range q.items {
		switch item.State {
		case Queued, WaitingForTurnBoundary, FailedRecoverable:
			ids = append(ids, id)
		}
	}
	sort.Slice(ids, func(i, j int) bool { return ids[i] < ids[j] })
	return ids
}
