package nativeui

/**
 * [INPUT]: Realistic SHA ancestry order including a two-parent merge
 * [OUTPUT]: Stable graph lanes that follow parents without synthesizing history
 * [POS]: Pure visual graph regression
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
