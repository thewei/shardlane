package nativeui

/**
 * [INPUT]: Headless Git reference fixtures, repeated native clicks and search state
 * [OUTPUT]: Verified fold/reopen behavior in sections, nested folders and filtered trees
 * [POS]: Real interaction regression for previously unresponsive disclosure controls
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

func TestGitSectionsAlwaysFoldAndReopen(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.git.refreshing = true
	s.git.branches = []gitworkbench.Branch{{Name: "main", Current: true}, {Name: "feature/editor"}}
	s.git.remoteBranches = []gitworkbench.RemoteBranch{{Remote: "origin", Name: "develop"}}
	s.git.tags = []gitworkbench.Tag{{Name: "v3.2.1"}}
	s.git.stashes = []gitworkbench.Stash{{Ref: "stash@{0}", Subject: "WIP secret"}}
	s.git.worktrees = []gitworkbench.Worktree{{Branch: "other", Path: "/tmp/git-other"}}
	s.showSurface(WorkspaceSurfaceDiff)
	tester := ui.NewTester(s.View, 1200, 800)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	cases := []struct{ section, leaf string }{
		{"Worktrees", "other"},
		{"Branches", "feature"},
		{"Remotes", "origin"},
		{"Tags", "v3.2.1"},
		{"Stashes", "WIP secret"},
	}
	for _, tc := range cases {
		if !tester.HasText(tc.leaf) && tc.section != "Stashes" {
			t.Fatalf("%s initial child %q absent", tc.section, tc.leaf)
		}
		for i := 0; i < 3; i++ {
			if err := tester.Click("Toggle " + tc.section); err != nil {
				t.Fatalf("%s close %d: %v", tc.section, i, err)
			}
			tester.Frame()
			if !s.gitTreeClosed["section/"+tc.section] {
				t.Fatalf("%s was not closed, pass %d", tc.section, i)
			}
			if err := tester.Click("Toggle " + tc.section); err != nil {
				t.Fatalf("%s reopen %d: %v", tc.section, i, err)
			}
			tester.Frame()
			if s.gitTreeClosed["section/"+tc.section] {
				t.Fatalf("%s was not reopened, pass %d", tc.section, i)
			}
		}
	}
}

func TestGitNestedFoldersReopenAfterFiltering(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.git.refreshing = true
	s.git.branches = []gitworkbench.Branch{{Name: "main", Current: true}, {Name: "feature/editor/alpha"}, {Name: "feature/editor/beta"}}
	s.showSurface(WorkspaceSurfaceDiff)
	tt := ui.NewTester(s.View, 1200, 800)
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	for i := 0; i < 3; i++ {
		if err := tt.Click("feature"); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		tt.Frame()
		if !s.gitTreeClosed["local/feature"] {
			t.Fatalf("feature folder did not close on pass %d", i)
		}
		if slices.Contains(tt.Texts(), "editor") {
			t.Fatalf("closed feature still shows editor child: %+v", tt.Texts())
		}
		if err := tt.Click("feature"); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		tt.Frame()
		if s.gitTreeClosed["local/feature"] || !slices.Contains(tt.Texts(), "editor") {
			t.Fatal("feature could not be reopened")
		}
	}
	// Normal browse retains its fold when filtering, but search starts with
	// expanded matches and allows user-controlled fold/reopen.
	if err := tt.Click("feature"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	s.gitRefsFilter = "beta"
	tt.Frame()
	if !tt.HasText("beta") {
		t.Fatal("filtered matching branch did not expand")
	}
	if err := tt.Click("feature"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	tt.Frame()
	if !s.gitTreeClosed["search/beta/local/feature"] || tt.HasText("beta") {
		t.Fatal("filtered folder click did not actually close matching children")
	}
	if err := tt.Click("feature"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	tt.Frame()
	if s.gitTreeClosed["search/beta/local/feature"] || !tt.HasText("beta") {
		t.Fatal("filtered folder could not reopen")
	}
	s.gitRefsFilter = ""
	tt.Frame()
	if !s.gitTreeClosed["local/feature"] {
		t.Fatal("search clobbered previous normal fold state")
	}
	if err := tt.Click("feature"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if !tt.HasText("alpha") || !tt.HasText("beta") {
		t.Fatal("clearing filter did not restore original branches")
	}
}
