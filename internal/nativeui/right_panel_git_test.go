package nativeui

/**
 * [INPUT]: cached Git worktree/commit surface and legacy panel visibility
 * [OUTPUT]: Changes retains left navigation + center Diff without Review Inspector
 * [POS]: post-right-rail Git workbench regression
 * [PROTOCOL]: update header and check CLAUDE.md
 */

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
	"testing"
)

func TestGitDiffNeverMountsRetiredInspector(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.rightPanel.open = true
	s.showSurface(WorkspaceSurfaceDiff)
	tt := ui.NewTester(s.View, 1280, 800)
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	for _, want := range []string{"Local Changes", "Unstaged Files", "Changes"} {
		if !tt.HasText(want) {
			t.Fatalf("Git diff missing %q: %q", want, tt.Texts())
		}
	}
	if tt.HasText("Review Inspector") || tt.HasText("Show Workspace Tools") {
		t.Fatal("removed right panel appeared in Changes")
	}
}

func TestGitHistoryCommitStillReadOnlyWithoutInspector(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.git.source = gdSourceCommit
	s.git.commitHash = "0123456789abcdef0123456789abcdef01234567"
	s.git.commitSnap = &gitworkbench.ChangesSnapshot{Root: s.git.root}
	tt := ui.NewTester(s.View, 1280, 800)
	tt.Frame()
	if tt.HasText("Review Inspector") || tt.HasText("Stage All") || tt.HasText("Unstage All") {
		t.Fatalf("historical diff must stay read-only and unencumbered: %q", tt.Texts())
	}
}
