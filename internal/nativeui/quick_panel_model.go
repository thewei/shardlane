package nativeui

import (
	"fmt"
	"strings"
)

// Floating Agent activity panel model helpers (0.7 §17.2/§17.4): the
// summary segments shared by the tray tooltip and the panel header. The
// window geometry lives in quick_panel.go (2026-10-07).

// attentionSegments is the one summary source (§17.4): the ordered
// actionable-count segments shared by the tray tooltip and the quick-panel
// header. reviewNoun is per-surface wording ("for review" tray / "review"
// panel); attention and working read identically everywhere.
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

// QuickPanelHeader renders the §17.2 panel header summary beneath the
// product name: "2 need attention · 1 review · 3 working · 5 agents",
// "Ready", or "Herdr disconnected" — the same summary source as the tray,
// with the full Agent count the panel lists beneath it.
func QuickPanelHeader(snapshot StatusCenterSnapshot, connected bool) string {
	if !connected {
		return "Herdr disconnected"
	}
	counts := TrayCountsFromSnapshot(snapshot)
	segments := attentionSegments(counts, "review")
	if total := len(snapshot.Entries); total > 0 {
		agents := fmt.Sprintf("%d agents", total)
		if total == 1 {
			agents = "1 agent"
		}
		segments = append(segments, agents)
	}
	if len(segments) == 0 {
		return "Ready"
	}
	return strings.Join(segments, " · ")
}
