package agent

import (
	"sort"
	"time"
)

// WorkingHysteresis is how long an Agent that just left the Working bucket
// keeps sorting as recently active (0.5 §9). Agent CLIs report working →
// idle through hooks after every short task loop, so the honest bucket
// alone would bounce rows between the top and the bottom of the list on
// each loop; the hysteresis keeps the order stable while the status pill
// stays honest.
const WorkingHysteresis = 30 * time.Second

// AgentDirectory is the presentation projection over reconciled Agent cards
// (0.5 §9). It is not runtime authority: it is written only from reconciled
// Herdr projections plus client-owned markers, contains no lifecycle logic,
// and starts no watchers.
type AgentDirectory struct {
	entries map[AgentKey]AgentCardModel
	// now is the hysteresis clock; overridable in tests.
	now func() time.Time
}

func NewAgentDirectory() *AgentDirectory {
	return &AgentDirectory{entries: make(map[AgentKey]AgentCardModel), now: time.Now}
}

// Replace swaps the directory content for one reconciled projection batch.
func (d *AgentDirectory) Replace(cards []AgentCardModel) {
	d.entries = make(map[AgentKey]AgentCardModel, len(cards))
	for _, card := range cards {
		d.entries[card.Key] = card
	}
}

func (d *AgentDirectory) Get(key AgentKey) (AgentCardModel, bool) {
	card, ok := d.entries[key]
	return card, ok
}

// AgentDirectoryBucket is the actionable-first sort bucket (0.5 §9).
type AgentDirectoryBucket uint8

const (
	BucketNeedsAttention AgentDirectoryBucket = iota
	BucketReadyForReview
	BucketWorking
	BucketIdle
)

// BucketOf classifies a card into its directory bucket.
func BucketOf(card AgentCardModel) AgentDirectoryBucket {
	switch {
	case card.Attention == OpNeedsAttention || card.RuntimePhase == PhaseBlocked || card.RuntimePhase == PhaseFailed:
		return BucketNeedsAttention
	case card.ReviewPending || card.Attention == OpReadyForReview:
		return BucketReadyForReview
	case card.Working():
		return BucketWorking
	default:
		return BucketIdle
	}
}

// sortRank is the directory position rank: the honest bucket, except that
// a card inside its working hysteresis window keeps the Working position
// while its own bucket (and status pill) stays Idle.
func (d *AgentDirectory) sortRank(card AgentCardModel) AgentDirectoryBucket {
	bucket := BucketOf(card)
	if bucket == BucketIdle && card.ActiveUntil.After(d.now()) {
		return BucketWorking
	}
	return bucket
}

// All returns the cards in the actionable-first order:
// NeedsAttention, ReadyForReview, Working, Idle; then unread first; then
// title alphabetical; then the stable identity. Every comparison ends in a
// total order: without the identity tail, equal-ranked cards would follow
// Go's randomized map iteration order and reshuffle on every snapshot.
func (d *AgentDirectory) All() []AgentCardModel {
	cards := make([]AgentCardModel, 0, len(d.entries))
	for _, card := range d.entries {
		cards = append(cards, card)
	}
	sort.SliceStable(cards, func(i, j int) bool {
		pi, pj := d.sortRank(cards[i]), d.sortRank(cards[j])
		if pi != pj {
			return pi < pj
		}
		if cards[i].Unread != cards[j].Unread {
			return cards[i].Unread
		}
		if cards[i].Title != cards[j].Title {
			return cards[i].Title < cards[j].Title
		}
		if cards[i].Key.InstanceID != cards[j].Key.InstanceID {
			return cards[i].Key.InstanceID < cards[j].Key.InstanceID
		}
		return cards[i].Key.TerminalID < cards[j].Key.TerminalID
	})
	return cards
}

// Filter returns the cards matching one directory bucket.
func (d *AgentDirectory) Filter(bucket AgentDirectoryBucket) []AgentCardModel {
	var filtered []AgentCardModel
	for _, card := range d.All() {
		if BucketOf(card) == bucket {
			filtered = append(filtered, card)
		}
	}
	return filtered
}

// DirectorySummary is the header's actionable-only count aggregate (0.5
// §10): token/usage facts never appear in the primary status control.
type DirectorySummary struct {
	NeedsAttention int
	ReviewPending  int
	Working        int
	Total          int
}

// Summary derives the counts from the current directory content.
func (d *AgentDirectory) Summary() DirectorySummary {
	summary := DirectorySummary{Total: len(d.entries)}
	for _, card := range d.entries {
		switch BucketOf(card) {
		case BucketNeedsAttention:
			summary.NeedsAttention++
		case BucketReadyForReview:
			summary.ReviewPending++
		case BucketWorking:
			summary.Working++
		}
	}
	return summary
}
