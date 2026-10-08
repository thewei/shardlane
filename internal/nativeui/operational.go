package nativeui

import (
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/herdr"
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

// OperationalSummary is the pure aggregate over the current projection
// (OPS-02). It is presentation-only and never polls; it is recomputed from
// already-held data when the projection changes.
type OperationalSummary struct {
	HighestPriority operationalState
	NeedsAttention  int
	Working         int
	ReadyForReview  int
	Idle            int
	Total           int
}

func summarizeAgents(agents []herdr.Agent) OperationalSummary {
	summary := OperationalSummary{HighestPriority: opIdle}
	for _, agent := range agents {
		state := normalizeRuntimeStatus(agent.Status)
		summary.Total++
		switch state {
		case opNeedsAttention:
			summary.NeedsAttention++
		case opWorking:
			summary.Working++
		case opReadyForReview:
			summary.ReadyForReview++
		default:
			summary.Idle++
		}
		if operationalPriority(state) < operationalPriority(summary.HighestPriority) {
			summary.HighestPriority = state
		}
	}
	return summary
}

func (s *Shell) operationalSummary() OperationalSummary {
	return summarizeAgents(s.projection.Agents)
}

// scopeOperationalState derives the highest-priority state among the agents
// of one Project or Tab — the aggregate behind Project/Tab rows (OPS-03).
func scopeOperationalState(projection herdr.Projection, projectID, tabID string) operationalState {
	state := opIdle
	for _, agent := range projection.Agents {
		if projectID != "" && agent.ProjectID != projectID {
			continue
		}
		if tabID != "" && agent.TabID != tabID {
			continue
		}
		agentState := normalizeRuntimeStatus(agent.Status)
		if operationalPriority(agentState) < operationalPriority(state) {
			state = agentState
		}
	}
	return state
}
