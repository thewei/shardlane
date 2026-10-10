package gitworkbench

/**
 * [INPUT]: Scratch Git repository and real history reader
 * [OUTPUT]: CommitInfo.Parents matches the authority's first-parent SHA
 * [POS]: Git history ancestry contract for the Changes graph
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"testing"
)

func TestListCommitsIncludesActualParentHashes(t *testing.T) {
	root, r := tempRepo(t)
	mustRunGit(t, root, "commit", "--allow-empty", "-m", "next")
	commits, err := r.ListCommits(context.Background(), root, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(commits) < 2 || len(commits[0].Parents) != 1 || commits[0].Parents[0] != commits[1].Hash {
		t.Fatalf("actual parent SHA not preserved in commit history: %+v", commits)
	}
}
