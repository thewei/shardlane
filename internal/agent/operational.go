package agent

import "strings"

// OperationalState is the shared operational attention projection (0.4/0.5):
// a presentation/priority dimension over Herdr runtime status, never runtime
// authority itself. Both the nativeui status surfaces and the 0.7 Status
// Center consume this one model.
type OperationalState uint8

const (
	OpUnknown OperationalState = iota
	OpIdle
	OpWorking
	OpReadyForReview
	OpNeedsAttention
	OpResolved
)

// ParseOperationalState maps one raw Herdr status string onto the shared
// operational state; unknown values fall back to the presentation-safe idle.
func ParseOperationalState(raw string) OperationalState {
	switch strings.ToLower(strings.TrimSpace(raw)) {
	case "blocked", "failed", "error", "needs_attention", "needs-attention":
		return OpNeedsAttention
	case "working", "pending", "launch_pending", "launch-pending", "running":
		return OpWorking
	case "done", "completed":
		return OpReadyForReview
	case "resolved", "reviewed":
		return OpResolved
	default:
		return OpIdle
	}
}

// OperationalPriority is the architecture's attention order:
// NeedsAttention > Working > ReadyForReview > Idle > Resolved.
func OperationalPriority(state OperationalState) int {
	switch state {
	case OpNeedsAttention:
		return 0
	case OpWorking:
		return 1
	case OpReadyForReview:
		return 2
	case OpIdle:
		return 3
	case OpResolved:
		return 4
	default:
		return 5
	}
}

// OperationalLabel is the shared user-facing status text.
func OperationalLabel(state OperationalState) string {
	switch state {
	case OpNeedsAttention:
		return "Needs attention"
	case OpWorking:
		return "Working"
	case OpReadyForReview:
		return "Ready for review"
	case OpResolved:
		return "Resolved"
	case OpIdle:
		return "Idle"
	default:
		return "Unknown"
	}
}
