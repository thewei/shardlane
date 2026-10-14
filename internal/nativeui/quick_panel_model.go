package nativeui

import (
	"fmt"
)

// Floating Agent activity panel model helpers (0.7 §17.2/§17.4): the
// summary segments of the tray tooltip. The window geometry lives in
// quick_panel.go (2026-10-07).

// attentionSegments is the one summary source (§17.4): the ordered
// actionable-count segments of the tray tooltip. reviewNoun is the
// per-surface wording of the review count; attention and working read
// identically everywhere.
func attentionSegments(counts TrayCounts, reviewNoun string) []string {
	segments := make([]string, 0, 3)
	if counts.Attention > 0 {
		n := counts.Attention
		verb := "need"
		if n == 1 {
			verb = "needs"
		}
		segments = append(segments, fmt.Sprintf("%d %s attention", n, verb))
	}
	if counts.Review > 0 {
		segments = append(segments, fmt.Sprintf("%d %s", counts.Review, reviewNoun))
	}
	if counts.Working > 0 {
		segments = append(segments, fmt.Sprintf("%d working", counts.Working))
	}
	return segments
}
