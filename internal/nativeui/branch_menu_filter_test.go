package nativeui

/**
 * [INPUT]: local branch catalog and filter preference
 * [OUTPUT]: stable current-first, case-insensitive branch search
 * [POS]: Native Git branch picker ordering contract; the pull-down itself is covered in sidebar_tree_test.go
 * [PROTOCOL]: update header and check CLAUDE.md
 */

import (
	"testing"

	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

func TestMatchingLocalBranchesCurrentFirst(t *testing.T) {
	branches := []gitworkbench.Branch{
		{Name: "feature/old"},
		{Name: "main", Current: true},
		{Name: "feature/Git-workbench"},
		{Name: "feature/ui"},
	}
	all := matchingLocalBranches(branches, "")
	if len(all) != 4 || all[0].Name != "main" {
		t.Fatalf("current branch must sort to top: %+v", all)
	}
	filtered := matchingLocalBranches(branches, "  FEATURE/  ")
	if len(filtered) != 3 || filtered[0].Name != "feature/old" || filtered[2].Name != "feature/ui" {
		t.Fatalf("stable case-insensitive search: %+v", filtered)
	}
	if got := matchingLocalBranches(branches, "missing"); len(got) != 0 {
		t.Fatalf("search should empty: %+v", got)
	}
}
