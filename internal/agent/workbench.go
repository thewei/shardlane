package agent

import (
	"strings"

	"github.com/wh-studio/herdr-client/internal/history"
)

// AgentKey is the stable process-wide live Agent identity (0.5 §4):
// terminal_id is the durable identity; pane_id moves and is only a
// navigation locator; the instance participates because Herdr sessions can
// reuse terminal identifiers.
type AgentKey struct {
	InstanceID string
	TerminalID string
}

// AgentRuntimePhase is the Herdr-authoritative runtime dimension (0.5 §3.1).
// It is deliberately separate from attention, markers, sendability and
// integration health.
type AgentRuntimePhase string

const (
	PhaseLaunching AgentRuntimePhase = "launching"
	PhaseWorking   AgentRuntimePhase = "working"
	PhaseBlocked   AgentRuntimePhase = "blocked"
	PhaseIdle      AgentRuntimePhase = "idle"
	PhaseDone      AgentRuntimePhase = "done"
	PhaseFailed    AgentRuntimePhase = "failed"
	PhaseUnknown   AgentRuntimePhase = "unknown"
)

// ParseRuntimePhase maps the authoritative Herdr facts (agent_status plus
// the typed launch_pending flag) onto the runtime phase.
func ParseRuntimePhase(agentStatus string, launchPending bool) AgentRuntimePhase {
	if launchPending {
		return PhaseLaunching
	}
	switch strings.ToLower(strings.TrimSpace(agentStatus)) {
	case "working", "pending", "running":
		return PhaseWorking
	case "blocked":
		return PhaseBlocked
	case "idle":
		return PhaseIdle
	case "done", "completed":
		return PhaseDone
	case "failed", "error":
		return PhaseFailed
	default:
		return PhaseUnknown
	}
}

// AgentSendability is the mutation-safety dimension (0.5 §3.4): the single
// classifier that decides whether a semantic prompt may be sent. It is the
// future Chat/Follow-up safety source of truth and must never be derived
// from attention levels.
type AgentSendability string

const (
	Sendable      AgentSendability = "sendable"
	MidTurn       AgentSendability = "mid-turn"
	NeedsTerminal AgentSendability = "needs-terminal"
	SendUnknown   AgentSendability = "unknown"
)

// ClassifySendability ports the original single classifier exactly:
//
//	working / pending / launch_pending → MidTurn
//	blocked / failed                   → NeedsTerminal
//	idle / done                        → Sendable
//	unknown / absent                   → Unknown (fail closed)
func ClassifySendability(phase AgentRuntimePhase) AgentSendability {
	switch phase {
	case PhaseWorking, PhaseLaunching:
		return MidTurn
	case PhaseBlocked, PhaseFailed:
		return NeedsTerminal
	case PhaseIdle, PhaseDone:
		return Sendable
	default:
		return SendUnknown
	}
}

// AgentTransition is the classified change between two runtime phases
// (0.5 §8). One classifier result drives every consumer: unread, review
// marker, header summary, agent card, and notification eligibility.
type AgentTransition string

const (
	TransitionNone             AgentTransition = "none"
	TransitionStartedWorking   AgentTransition = "started-working"
	TransitionNeedsAttention   AgentTransition = "needs-attention"
	TransitionReadyForReview   AgentTransition = "ready-for-review"
	TransitionReady            AgentTransition = "ready"
	TransitionReviewCleared    AgentTransition = "review-cleared"
	TransitionFailed           AgentTransition = "failed"
	TransitionFirstObservation AgentTransition = "first-observation"
)

// ClassifyTransition ports the original pure transition behavior: a previous
// unknown phase produces no synthetic transition, identical phases no
// transition at all.
func ClassifyTransition(previous, next AgentRuntimePhase) AgentTransition {
	if previous == PhaseUnknown {
		return TransitionFirstObservation
	}
	if previous == next {
		return TransitionNone
	}
	switch next {
	case PhaseFailed:
		return TransitionFailed
	case PhaseBlocked:
		if previous == PhaseWorking {
			return TransitionNeedsAttention
		}
		return TransitionNone
	case PhaseDone:
		if previous == PhaseWorking || previous == PhaseBlocked {
			return TransitionReadyForReview
		}
		return TransitionNone
	case PhaseWorking:
		if previous == PhaseDone {
			return TransitionReviewCleared
		}
		return TransitionStartedWorking
	case PhaseIdle:
		if previous == PhaseWorking {
			return TransitionReady
		}
		return TransitionNone
	default:
		return TransitionNone
	}
}

// AgentTitle resolves the user-facing title with the original fallback
// order: agent title → agent name → provider display name → "Agent".
func AgentTitle(title, name string, provider history.AgentID) string {
	if value := strings.TrimSpace(title); value != "" {
		return value
	}
	if value := strings.TrimSpace(name); value != "" {
		return value
	}
	if provider != "" {
		return provider.DisplayName()
	}
	return "Agent"
}
