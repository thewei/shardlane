package nativeui

import (
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/conversation"
)

// StatusCenterEntry is one row of the Status Center (0.7 §5): an Agent or
// pending interaction, projected from already-reconciled state with a
// recommended destination.
type StatusCenterEntry struct {
	Card           agent.AgentCardModel
	Pending        *conversation.PendingInteraction
	Bucket         agent.AgentDirectoryBucket
	HasInteraction bool
}

// StatusCenterSnapshot is the one shared projection behind the titlebar
// trigger, the in-window panel and the macOS Tray (0.7 §3/§5). Pure: built
// from already-reconciled state, never from render-time IO.
type StatusCenterSnapshot struct {
	NeedsAttention int
	ReviewPending  int
	Working        int
	Interactions   int
	Entries        []StatusCenterEntry
}

// StatusCenterPriority order (0.7 §6): interactions first (they block the
// agent), then NeedsAttention > ReadyForReview > Working > Idle.
func statusCenterPriority(entry StatusCenterEntry) int {
	if entry.HasInteraction {
		return -1 // pending interactions outrank every other state (0.7 §6)
	}
	return agent.OperationalPriority(cardAttentionOf(entry.Card))
}

func cardAttentionOf(card agent.AgentCardModel) agent.OperationalState {
	return card.Attention
}

// BuildStatusCenterSnapshot derives the shared snapshot from the reconciled
// workbench directory and the interaction broker's pending set.
func (s *Shell) BuildStatusCenterSnapshot() StatusCenterSnapshot {
	snapshot := StatusCenterSnapshot{}
	pendingByPane := make(map[string]conversation.PendingInteraction)
	for _, interaction := range s.interactionBroker.Pending() {
		pendingByPane[interaction.AgentPaneID] = interaction
		snapshot.Interactions++
	}

	for _, card := range s.workbenchCards() {
		entry := StatusCenterEntry{Card: card, Bucket: agent.BucketOf(card)}
		if interaction, ok := pendingByPane[card.PaneID]; ok {
			interaction := interaction
			entry.Pending = &interaction
			entry.HasInteraction = true
			entry.Bucket = agent.BucketNeedsAttention
		}
		switch entry.Bucket {
		case agent.BucketNeedsAttention:
			snapshot.NeedsAttention++
		case agent.BucketReadyForReview:
			snapshot.ReviewPending++
		case agent.BucketWorking:
			snapshot.Working++
		}
		snapshot.Entries = append(snapshot.Entries, entry)
	}

	// Stable priority order.
	for i := 1; i < len(snapshot.Entries); i++ {
		for j := i; j > 0 && statusCenterPriority(snapshot.Entries[j]) < statusCenterPriority(snapshot.Entries[j-1]); j-- {
			snapshot.Entries[j], snapshot.Entries[j-1] = snapshot.Entries[j-1], snapshot.Entries[j]
		}
	}
	return snapshot
}

// statusCenterRecommendedDestination names the surface a click should open
// (0.7 §10): interaction → Chat, blocked/failed → Terminal, else the agent.
func statusCenterRecommendedDestination(entry StatusCenterEntry) agent.PrimaryAction {
	if entry.HasInteraction {
		return "open-chat"
	}
	return entry.Card.PrimaryAction()
}
