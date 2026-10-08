package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/conversation"
	"github.com/wh-studio/herdr-client/internal/history"
)

// TestStatusCenterSnapshotPriorityOrder pins the §6 priority order:
// pending interactions first, then NeedsAttention > ReadyForReview >
// Working > Idle.
func TestStatusCenterSnapshotPriorityOrder(t *testing.T) {
	shell := workbenchTestShell(t)

	// Ranger blocked; Scout working. After Scout finishes, it becomes review.
	shell.projection.Agents[0].Status = "done"
	shell.reconcileWorkbench()

	snapshot := shell.BuildStatusCenterSnapshot()
	if len(snapshot.Entries) != 2 {
		t.Fatalf("entries = %d", len(snapshot.Entries))
	}
	if snapshot.Entries[0].Card.Title != "Ranger" {
		t.Fatalf("first entry = %q, want the blocked agent", snapshot.Entries[0].Card.Title)
	}
	if snapshot.Entries[1].Card.Title != "Scout" {
		t.Fatalf("second entry = %q, want the review agent", snapshot.Entries[1].Card.Title)
	}
	if snapshot.NeedsAttention != 1 || snapshot.ReviewPending != 1 || snapshot.Working != 0 {
		t.Fatalf("counts = %+v", snapshot)
	}
}

// TestStatusCenterInteractionFirst pins that a pending structured
// interaction outranks every other state and carries the interaction marker.
func TestStatusCenterInteractionFirst(t *testing.T) {
	shell := workbenchTestShell(t)
	shell.interactionBroker.Publish(conversation.PendingInteraction{
		ID:             "i1",
		ConversationID: "live:codex:s1",
		AgentPaneID:    "p1",
		Kind:           conversation.InteractionPermission,
		Prompt:         "Allow workspace write?",
		Options:        []string{"Approve", "Deny"},
	})

	snapshot := shell.BuildStatusCenterSnapshot()
	// Ranger stays blocked; the interaction forces working Scout into the
	// attention bucket as well.
	if snapshot.Interactions != 1 || snapshot.NeedsAttention != 2 {
		t.Fatalf("snapshot = %+v", snapshot)
	}
	if len(snapshot.Entries) == 0 || snapshot.Entries[0].Card.Title != "Scout" {
		t.Fatalf("interaction agent must sort first: %+v", snapshot.Entries)
	}
	if snapshot.Entries[0].Pending == nil || !snapshot.Entries[0].HasInteraction {
		t.Fatalf("interaction marker missing: %+v", snapshot.Entries[0])
	}
	if got := statusCenterRecommendedDestination(snapshot.Entries[0]); got != "open-chat" {
		t.Fatalf("interaction destination = %v", got)
	}
}

// TestStatusCenterRecommendedDestinations pins the destination contract:
// blocked/failed → Terminal, review/idle → Agent.
func TestStatusCenterRecommendedDestinations(t *testing.T) {
	blocked := StatusCenterEntry{Card: agent.ProjectAgentCard(agent.AgentCardInputs{
		InstanceID: "default", TerminalID: "t1", Provider: history.AgentCodex, AgentStatus: "blocked",
	})}
	if got := statusCenterRecommendedDestination(blocked); got != agent.ActionOpenTerminal {
		t.Fatalf("blocked destination = %v", got)
	}
	idle := StatusCenterEntry{Card: agent.ProjectAgentCard(agent.AgentCardInputs{
		InstanceID: "default", TerminalID: "t2", Provider: history.AgentCodex, AgentStatus: "idle",
	})}
	if got := statusCenterRecommendedDestination(idle); got != agent.ActionOpenAgent {
		t.Fatalf("idle destination = %v", got)
	}
}

// TestStatusCenterSnapshotEmpty pins that a quiet workspace produces a
// zero snapshot with no entries.
func TestStatusCenterSnapshotEmpty(t *testing.T) {
	shell := NewShell()
	shell.reconcileWorkbench()
	snapshot := shell.BuildStatusCenterSnapshot()
	if snapshot.NeedsAttention != 0 || snapshot.ReviewPending != 0 || snapshot.Working != 0 || len(snapshot.Entries) != 0 {
		t.Fatalf("empty snapshot = %+v", snapshot)
	}
}
