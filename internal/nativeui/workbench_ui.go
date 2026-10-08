package nativeui

import (
	"github.com/wh-studio/herdr-client/internal/agent"
)

// workbenchFilterLabels are the agent activity panel's status tab group
// (2026-10-07): the choices switch which agent list the panel shows.
var workbenchFilterLabels = []string{"All", "Attention", "Review", "Working", "Idle"}

func (w *workbenchState) filterChoice() int {
	if w.filterAll {
		return 0
	}
	switch w.filter {
	case agent.BucketNeedsAttention:
		return 1
	case agent.BucketReadyForReview:
		return 2
	case agent.BucketWorking:
		return 3
	default:
		return 4
	}
}

func (w *workbenchState) setFilterChoice(chosen int) {
	switch chosen {
	case 0:
		w.filterAll = true
	case 1:
		w.filterAll, w.filter = false, agent.BucketNeedsAttention
	case 2:
		w.filterAll, w.filter = false, agent.BucketReadyForReview
	case 3:
		w.filterAll, w.filter = false, agent.BucketWorking
	default:
		w.filterAll, w.filter = false, agent.BucketIdle
	}
}

// pluralS returns "s" for counts other than exactly one.
func pluralS(n int) string {
	if n == 1 {
		return ""
	}
	return "s"
}

// The /agents workbench page and its standard card were removed on
// 2026-10-07 together with the in-window Status Center: agent activity is
// presented only by the floating panel (quick_panel.go) over the shared
// agentCardRow builder (agent_card.go).
