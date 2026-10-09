package nativeui

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

/**
 * [INPUT]: Diff/Terminal surface route, repo context and cached Git/Commit snapshots
 * [OUTPUT]: Locks Changes-tool contextual ownership: Terminal tree vs Git Review Inspector
 * [POS]: Right Panel role regression test; no filesystem or Git IO during UI render
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestGitInspectorChangesToolFollowsPrimarySurface(t *testing.T) {
	root := t.TempDir()
	s := gitSurfaceTestShell(t, root)
	s.git.rootOf[root] = root
	s.openRightPanelSurface(SurfaceChanges)
	tester := ui.NewTester(s.View, 1280, 800)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	tester.Frame()
	if tester.HasText("Review Inspector") || tester.HasText("READ ONLY") {
		t.Fatalf("Terminal must keep compact Changes tool: %q", tester.Texts())
	}
	s.showSurface(WorkspaceSurfaceDiff)
	s.surface.diff.SelectedPath = "src/main.go"
	tester.Frame()
	for _, want := range []string{"Review Inspector", "Repository", "Selected file", "src/main.go", "Review"} {
		if !tester.HasText(want) {
			t.Fatalf("Diff inspector missing %q: %q", want, tester.Texts())
		}
	}
	if tester.HasText("READ ONLY") {
		t.Fatal("worktree review must not be marked read-only")
	}
	s.showSurface(WorkspaceSurfaceTerminal)
	tester.Frame()
	if tester.HasText("Review Inspector") || !tester.HasText("Changes") {
		t.Fatalf("Terminal Changes tool not restored: %q", tester.Texts())
	}
}

func TestGitInspectorHistoricalCommitIsReadOnly(t *testing.T) {
	root := t.TempDir()
	s := gitSurfaceTestShell(t, root)
	s.git.rootOf[root] = root
	s.showSurface(WorkspaceSurfaceDiff)
	s.openRightPanelSurface(SurfaceChanges)
	hash := "0123456789abcdef0123456789abcdef01234567"
	s.git.source = gdSourceCommit
	s.git.commitHash = hash
	s.git.commitMeta = &gitworkbench.CommitInfo{
		Hash: hash, Short: "0123456", Subject: "test commit", Author: "tester", Time: time.Unix(0, 0),
	}
	s.git.commitSnap = &gitworkbench.ChangesSnapshot{Root: root, Head: hash}
	tester := ui.NewTester(s.View, 1280, 800)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	tester.Frame()
	for _, want := range []string{"Review Inspector", "READ ONLY", "Commit", "test commit", "Revert commit…"} {
		if !tester.HasText(want) {
			t.Fatalf("read-only Commit inspector missing %q: %q", want, tester.Texts())
		}
	}
	if tester.HasText("Stage All") || tester.HasText("Unstage All") {
		t.Fatalf("historical diff must not offer staged worktree actions: %q", tester.Texts())
	}
}
