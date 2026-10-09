package nativeui

/**
 * [INPUT]: local branch catalog, filter preference and bounded MyGo branch popover
 * [OUTPUT]: stable current-first search, scroll-friendly rows and non-leaking repository filters
 * [POS]: Native Git branch picker interaction contract (no actual Git mutation)
 * [PROTOCOL]: update header and check CLAUDE.md
 */

import (
	"testing"

	"github.com/egoist/mygo/ui"
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

func TestBranchPickerSearchAndCreateRemainAccessible(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.git.branches = []gitworkbench.Branch{{Name: "main", Current: true}, {Name: "topic/merge"}, {Name: "topic/refactor"}}
	s.branchMenuOpen = true
	s.branchFilter = "merge"
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	for _, want := range []string{"Branches", "Filter branches", "topic/merge", "Create branch…"} {
		if !tester.HasText(want) {
			t.Fatalf("branch picker missing %q: %q", want, tester.Texts())
		}
	}
	if tester.HasText("topic/refactor") {
		t.Fatalf("filter must hide non-matches: %q", tester.Texts())
	}
	s.branchFilter = "not-found"
	tester.Frame()
	if !tester.HasText("No matching branches") || !tester.HasText("Create branch…") {
		t.Fatalf("empty branch filter should explain results, not hide create: %q", tester.Texts())
	}
	s.resetGitContextState()
	if s.branchFilter != "" || s.branchMenuOpen {
		t.Fatal("branch popover leaked across repository switch")
	}
}
