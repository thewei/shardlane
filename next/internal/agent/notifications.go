package agent

import (
	"fmt"
)

// AgentNotification is one meaningful transition notification (0.5 §15).
// The exact initial transition set is preserved from the original client:
//
//	working → done    = Finished — ready for review
//	blocked → done    = Finished — ready for review
//	working → blocked = Needs attention
//	working → idle    = Ready
type AgentNotification struct {
	Title string
	Body  string
}

// NotificationFor maps one classified transition onto the narrow
// notification vocabulary. Every other transition — initial snapshot,
// same-status echo, first observation — produces nil: no arbitrary
// notifications for ordinary status changes.
func NotificationFor(transition AgentTransition, agentTitle string) *AgentNotification {
	var body string
	switch transition {
	case TransitionReadyForReview:
		body = "Finished — ready for review"
	case TransitionNeedsAttention:
		body = "Needs attention"
	case TransitionReady:
		body = "Ready"
	default:
		return nil
	}
	return &AgentNotification{
		Title: fmt.Sprintf("%s — %s", agentTitle, body),
		Body:  body,
	}
}

// Test-adjacent contract: the exact transition set is preserved.
func init() {
	for _, transition := range []AgentTransition{
		TransitionReadyForReview, TransitionNeedsAttention, TransitionReady,
	} {
		if NotificationFor(transition, "X") == nil {
			panic("agent: missing notification for " + string(transition))
		}
	}
	for _, transition := range []AgentTransition{
		TransitionNone, TransitionFirstObservation, TransitionStartedWorking,
		TransitionReviewCleared, TransitionFailed,
	} {
		if NotificationFor(transition, "X") != nil {
			panic("agent: unexpected notification for " + string(transition))
		}
	}
}
