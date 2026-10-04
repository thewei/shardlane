package nativeui

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"sort"
	"strings"

	"github.com/wh-studio/herdr-client/next/internal/agent"
)

// TrayCounts are the status-center counts the menu bar projects (0.7
// §3/§17.3): attention, ready-for-review and working.
type TrayCounts struct {
	Attention int
	Review    int
	Working   int
}

// TrayCountsFromSnapshot reads the shared StatusCenterSnapshot counters; the
// tray never derives its own status semantics.
func TrayCountsFromSnapshot(snapshot StatusCenterSnapshot) TrayCounts {
	return TrayCounts{
		Attention: snapshot.NeedsAttention,
		Review:    snapshot.ReviewPending,
		Working:   snapshot.Working,
	}
}

// TrayTitle renders the §17.3 status-first menu-bar title, e.g.
// "⚠ 2  ✓ 1  ⚡ 3". Empty when nothing is actionable so the menu bar stays
// quiet; raw token totals never appear in the title (usage belongs inside
// the panel where the Agent context is visible).
func TrayTitle(counts TrayCounts) string {
	parts := make([]string, 0, 3)
	if counts.Attention > 0 {
		parts = append(parts, fmt.Sprintf("⚠ %d", counts.Attention))
	}
	if counts.Review > 0 {
		parts = append(parts, fmt.Sprintf("✓ %d", counts.Review))
	}
	if counts.Working > 0 {
		parts = append(parts, fmt.Sprintf("⚡ %d", counts.Working))
	}
	return strings.Join(parts, "  ")
}

// TrayToolTip renders the §17.4 summary wording shared with the titlebar:
// "Shardlane — 2 need attention, 1 for review, 3 working", "Shardlane —
// Ready", or "Shardlane — Herdr disconnected". It reads the one shared
// segment source with the quick-panel header (§17.4).
func TrayToolTip(counts TrayCounts, connected bool) string {
	if !connected {
		return "Shardlane — Herdr disconnected"
	}
	segments := attentionSegments(counts, "for review")
	if len(segments) == 0 {
		return "Shardlane — Ready"
	}
	return "Shardlane — " + strings.Join(segments, ", ")
}

// TrayFingerprint captures exactly the snapshot state the tray may react to
// (0.7 §18): agent enter/leave, status, unread/review, interaction
// attention, connection state, and title/project identity. Navigation
// selection alone never changes it, so a selection change cannot rebuild
// the tray. Entry order is normalized; the derived snapshot order is a
// presentation concern.
func TrayFingerprint(snapshot StatusCenterSnapshot, connected bool) string {
	rows := make([]string, 0, len(snapshot.Entries))
	for _, entry := range snapshot.Entries {
		rows = append(rows, fmt.Sprintf("%s|%s|%s|%s|%s|%t|%t|%t",
			entry.Card.Key.TerminalID,
			entry.Card.PaneID,
			entry.Card.Title,
			entry.Card.ProjectName,
			string(entry.Card.RuntimePhase)+"/"+agent.OperationalLabel(entry.Card.Attention),
			entry.Card.Unread,
			entry.Card.ReviewPending,
			entry.HasInteraction,
		))
	}
	sort.Strings(rows)
	sum := sha256.Sum256([]byte(fmt.Sprintf("connected=%t|interactions=%d|%s",
		connected, snapshot.Interactions, strings.Join(rows, "\x00"))))
	return hex.EncodeToString(sum[:])
}
