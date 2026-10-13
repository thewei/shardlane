package nativeui

import (
	"github.com/wh-studio/herdr-client/internal/agent"
)

// The operational attention model is owned by internal/agent (0.5+); the
// nativeui names below are thin compatibility aliases so every status
// surface keeps consuming one shared projection (OPS-01/03).
type operationalState = agent.OperationalState

const (
	opUnknown        = agent.OpUnknown
	opIdle           = agent.OpIdle
	opWorking        = agent.OpWorking
	opReadyForReview = agent.OpReadyForReview
	opNeedsAttention = agent.OpNeedsAttention
	opResolved       = agent.OpResolved
)

func normalizeRuntimeStatus(raw string) operationalState {
	return agent.ParseOperationalState(raw)
}

func operationalPriority(state operationalState) int {
	return agent.OperationalPriority(state)
}

func operationalLabel(state operationalState) string {
	return agent.OperationalLabel(state)
}

// operationalTone maps the state onto the semantic status palette.
func operationalTone(st operationalState) StatusTone {
	switch st {
	case opNeedsAttention:
		return ToneAttention
	case opWorking:
		return ToneWorking
	case opReadyForReview:
		return ToneSuccess
	case opResolved:
		return ToneMuted
	default:
		return ToneNeutral
	}
}
