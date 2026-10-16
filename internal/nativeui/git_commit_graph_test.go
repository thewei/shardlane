package nativeui

/**
 * [INPUT]: Realistic SHA ancestry order including a two-parent merge
 * [OUTPUT]: Stable graph lanes that follow parents without synthesizing history; wire-continuous lane colors (first-parent inheritance, compact-slot survival)
 * [POS]: Pure visual graph regression
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
	"testing"
)

func TestGitHistoryGraphTracksMergeParentLanes(t *testing.T) {
	commits := []gitworkbench.CommitInfo{
		{Hash: "merge", Parents: []string{"left", "right"}},
		{Hash: "left", Parents: []string{"base"}},
		{Hash: "right", Parents: []string{"base"}},
		{Hash: "base"},
	}
	rows := gitHistoryGraph(commits)
	if len(rows) != 4 || rows[0].Lane != 0 || len(rows[0].After) != 2 ||
		rows[0].After[0] != "left" || rows[0].After[1] != "right" {
		t.Fatalf("merge must create two actual parent lanes: %+v", rows)
	}
	if rows[2].Lane != 1 || len(rows[2].After) != 1 || rows[2].After[0] != "base" {
		t.Fatalf("secondary branch must rejoin base exactly once: %+v", rows)
	}
	if len(rows[3].After) != 0 {
		t.Fatalf("root commit must terminate graph: %+v", rows[3])
	}
}

// TestGitHistoryGraphColorsFollowTheWire pins lane colors to lane identity:
// a wire keeps its color from fork through rejoin and through the slot
// compaction that follows a sibling lane's termination, so the drawn graph
// reads as continuous strokes instead of color-changing fragments.
func TestGitHistoryGraphColorsFollowTheWire(t *testing.T) {
	commits := []gitworkbench.CommitInfo{
		{Hash: "merge", Parents: []string{"left", "right"}},
		{Hash: "left", Parents: []string{"base"}},
		{Hash: "right", Parents: []string{"base"}},
		{Hash: "base"},
	}
	rows := gitHistoryGraph(commits)
	left := rows[0].AfterColors[0]
	right := rows[0].AfterColors[1]
	if left == right {
		t.Fatalf("fork lanes must wear different colors: %d", left)
	}
	if rows[1].Color != left || rows[1].BeforeColors[0] != left {
		t.Fatalf("left wire must keep its color into its commit: %+v", rows[1])
	}
	if rows[2].Color != right || rows[2].BeforeColors[1] != right {
		t.Fatalf("right wire must keep its color into its commit: %+v", rows[2])
	}
	base := rows[1].AfterColors[0]
	if rows[2].AfterColors[0] != base || rows[2].BeforeColors[0] != base ||
		rows[3].Color != base || rows[3].BeforeColors[0] != base {
		t.Fatalf("base wire must hold one color through the rejoin: %+v", rows)
	}
	if base != left {
		t.Fatalf("first-parent continuation must inherit the lane color: base %d, left %d", base, left)
	}

	// The left lane terminates first; the right lane compacts from slot 1
	// into slot 0 and must still wear the color it forked with.
	commits = []gitworkbench.CommitInfo{
		{Hash: "merge", Parents: []string{"short", "long"}},
		{Hash: "short"},
		{Hash: "long", Parents: []string{"tip"}},
		{Hash: "tip"},
	}
	rows = gitHistoryGraph(commits)
	long := rows[0].AfterColors[1]
	if rows[1].BeforeColors[1] != long || rows[1].AfterColors[0] != long ||
		rows[2].BeforeColors[0] != long || rows[2].Color != long {
		t.Fatalf("compacted lane must keep its fork color across the slot shift: %+v", rows)
	}

	// A first-parent chain is one wire: every commit on it wears the lane
	// color the chain inherited at its fork, instead of turning into a
	// per-commit rainbow.
	chain := []gitworkbench.CommitInfo{
		{Hash: "m1", Parents: []string{"a1", "b1"}},
		{Hash: "b1", Parents: []string{"c1"}},
		{Hash: "c1", Parents: []string{"d1"}},
		{Hash: "d1", Parents: []string{"e1"}},
		{Hash: "e1"},
		{Hash: "a1", Parents: []string{"f1"}},
		{Hash: "f1"},
	}
	rows = gitHistoryGraph(chain)
	laneColor := rows[1].Color
	for i := 2; i <= 4; i++ {
		if rows[i].Color != laneColor {
			t.Fatalf("first-parent chain must hold one lane color: row %d is %d, want %d: %+v", i, rows[i].Color, laneColor, rows[i])
		}
	}
	if rows[5].Color == laneColor {
		t.Fatalf("independent mainline must not wear the forked lane's color: %+v", rows)
	}
}
