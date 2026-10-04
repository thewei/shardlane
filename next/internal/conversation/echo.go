package conversation

import (
	"strings"

	"github.com/wh-studio/herdr-client/next/internal/history"
)

// PendingEcho is the temporary local User row shown right after a SentNow
// prompt is accepted (0.6 §12): when the provider semantic source later
// emits the matching User row after the baseline, the echo is consumed
// exactly once. Terminal echo text never confirms — reconciliation reads
// only the semantic transcript projection.
type PendingEcho struct {
	Text string
	// Baseline is the committed projection length at submission time; only
	// rows at or after this index can confirm the echo (order evidence).
	Baseline int
	// PaneID pins the target pane: kept when rebinding to the same pane
	// (the History Composer continuation case), dropped when the pane
	// changes (cross-Agent protection).
	PaneID string
}

// ReconcilePendingEcho confirms a pending submission against the committed
// provider projection: a same-trimmed-text User row at or after the baseline
// consumes it exactly once. It is idempotent — a consumed (nil) echo makes
// repeated calls no-ops. Returns the surviving echo and whether it was
// consumed by this call.
func ReconcilePendingEcho(messages []history.TranscriptMessage, echo *PendingEcho) (*PendingEcho, bool) {
	if echo == nil {
		return nil, false
	}
	want := strings.TrimSpace(echo.Text)
	start := echo.Baseline
	if start < 0 {
		start = 0
	}
	for index := start; index < len(messages); index++ {
		message := messages[index]
		if message.Role == history.RoleUser && strings.TrimSpace(message.Text) == want {
			return nil, true
		}
	}
	return echo, false
}
