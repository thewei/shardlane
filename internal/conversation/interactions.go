package conversation

import (
	"errors"
	"sync"
	"time"
)

// InteractionKind is the structured provider interaction vocabulary (0.6
// §17): permission requests and questions, never free-form prompts.
type InteractionKind string

const (
	InteractionPermission InteractionKind = "permission"
	InteractionQuestion   InteractionKind = "question"
)

// PendingInteraction is one blocked interaction awaiting resolution.
type PendingInteraction struct {
	ID             string
	ConversationID ConversationID
	AgentPaneID    string
	Fingerprint    string
	Kind           InteractionKind
	Prompt         string
	// Options are the provider's own labeled actions (e.g. Approve / Deny);
	// resolving with anything outside them fails closed.
	Options []string
}

// InteractionResolution records the first-winner resolution.
type InteractionResolution struct {
	InteractionID string
	Option        string
	ResolvedAtMS  int64
}

// ErrInteractionResolved reports a lost CAS race.
var ErrInteractionResolved = errors.New("interaction already resolved")

// ErrUnknownOption reports a resolution outside the provider's options:
// blocked interactions may only be answered through their exact bridge.
var ErrUnknownOption = errors.New("resolution is not one of the provider options")

// InteractionBroker resolves blocked interactions with first-winner CAS and
// occupant-change cancellation (0.6 §17).
type InteractionBroker struct {
	mu           sync.Mutex
	pending      map[string]PendingInteraction
	resolutions  map[string]InteractionResolution
	resolveClock func() int64
}

func NewInteractionBroker(resolveClock func() int64) *InteractionBroker {
	if resolveClock == nil {
		resolveClock = unixMilli
	}
	return &InteractionBroker{
		pending:      make(map[string]PendingInteraction),
		resolutions:  make(map[string]InteractionResolution),
		resolveClock: resolveClock,
	}
}

func unixMilli() int64 { return time.Now().UnixMilli() }

// Publish registers one pending interaction; a newer interaction for the
// same conversation cancels the older pending one (occupant/lifecycle
// supersession).
func (b *InteractionBroker) Publish(interaction PendingInteraction) error {
	if interaction.ID == "" || interaction.ConversationID == "" || interaction.Prompt == "" {
		return errors.New("interaction requires id, conversation and prompt")
	}
	if len(interaction.Options) == 0 {
		return errors.New("interaction requires provider options")
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	for id, existing := range b.pending {
		if existing.ConversationID == interaction.ConversationID {
			delete(b.pending, id)
		}
	}
	b.pending[interaction.ID] = interaction
	return nil
}

// Pending lists unresolved interactions.
func (b *InteractionBroker) Pending() []PendingInteraction {
	b.mu.Lock()
	defer b.mu.Unlock()
	pending := make([]PendingInteraction, 0, len(b.pending))
	for _, interaction := range b.pending {
		pending = append(pending, interaction)
	}
	return pending
}

// ResolveCAS resolves with compare-and-swap semantics: exactly the first
// caller wins; later resolvers of the same interaction get
// ErrInteractionResolved. The option must be one of the provider's own.
func (b *InteractionBroker) ResolveCAS(interactionID, option string) (InteractionResolution, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if resolution, ok := b.resolutions[interactionID]; ok {
		return resolution, ErrInteractionResolved
	}
	interaction, ok := b.pending[interactionID]
	if !ok {
		return InteractionResolution{}, errors.New("unknown interaction")
	}
	matched := false
	for _, candidate := range interaction.Options {
		if candidate == option {
			matched = true
			break
		}
	}
	if !matched {
		return InteractionResolution{}, ErrUnknownOption
	}
	resolution := InteractionResolution{
		InteractionID: interactionID,
		Option:        option,
		ResolvedAtMS:  b.resolveClock(),
	}
	b.resolutions[interactionID] = resolution
	delete(b.pending, interactionID)
	return resolution, nil
}

// CancelForConversation drops a pending interaction when the occupant
// changed or the lifecycle moved on.
func (b *InteractionBroker) CancelForConversation(id ConversationID) {
	b.mu.Lock()
	defer b.mu.Unlock()
	for interactionID, interaction := range b.pending {
		if interaction.ConversationID == id {
			delete(b.pending, interactionID)
		}
	}
}
