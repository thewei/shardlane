package nativeui

/**
 * [INPUT]: Headless MyGo Git page, repository ref fixtures and native pointer events
 * [OUTPUT]: Git tree grouping, center split geometry and commit review regression tests
 * [POS]: Presentation contract tests for screenshot-inspired Changes layout
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

func TestGitRefTreeGroupsSlashPathsAndFilters(t *testing.T) {
	names := []string{"main", "feat/editor/alpha", "feat/editor/beta", "fix/critical"}
	tree := gitRefTree(names, "")
	if len(tree) != 3 || tree[0].name != "feat" || len(tree[0].children) != 1 || tree[0].children[0].name != "editor" ||
		len(tree[0].children[0].children) != 2 {
		t.Fatalf("unexpected ref hierarchy: %+v", tree)
	}
	filtered := gitRefTree(names, "BETA")
	if len(filtered) != 1 || filtered[0].name != "feat" ||
		len(filtered[0].children[0].children) != 1 || filtered[0].children[0].children[0].name != "beta" {
		t.Fatalf("filter must preserve ancestors of the matching ref: %+v", filtered)
	}
}

func TestGitSidebarTreeAndCenterSplitGeometry(t *testing.T) {
	root := t.TempDir()
	s := gitSurfaceTestShell(t, root)
	s.git.rootOf[root] = root
	s.git.refreshing = true
	s.git.branches = []gitworkbench.Branch{
		{Name: "main", Current: true}, {Name: "feat/editor/alpha"}, {Name: "feat/editor/beta"},
	}
	s.git.remoteBranches = []gitworkbench.RemoteBranch{
		{Remote: "origin", Name: "main"}, {Remote: "upstream", Name: "feat/editor"},
	}
	s.git.tags = []gitworkbench.Tag{{Name: "v0.10"}}
	s.git.stashes = []gitworkbench.Stash{{Ref: "stash@{0}", Subject: "WIP"}}
	s.git.worktrees = []gitworkbench.Worktree{{Path: root, Branch: "main"}}
	s.showSurface(WorkspaceSurfaceDiff)
	s.rebuildDiffRows()
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()

	for _, want := range []string{"Worktrees", "Branches", "Remotes", "Tags", "Stashes", "origin", "upstream", "feat", "Unstaged Files", "Resize Git file navigator", "Commit subject draft"} {
		if !tester.HasText(want) {
			t.Fatalf("missing Git tree/split item %q: %q", want, tester.Texts())
		}
	}
	sidebar, ok := tester.Find("Toggle Remotes")
	if !ok || sidebar.X >= sidebarWidth {
		t.Fatalf("remote tree not in left rail: %+v found=%v", sidebar, ok)
	}
	splitter, ok := tester.Find("Resize Git file navigator")
	if !ok || splitter.X < sidebarWidth {
		t.Fatalf("file navigator divider not to the right of sidebar: %+v found=%v", splitter, ok)
	}
	file, ok := tester.Find("src/main.go")
	if !ok || file.X <= splitter.X {
		t.Fatalf("diff must be right of file navigator: file=%+v divider=%+v found=%v", file, splitter, ok)
	}

	if err := tester.Click("Toggle Remotes"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if tester.HasText("origin") {
		t.Fatalf("collapsing Remotes must hide remote refs: %q", tester.Texts())
	}
	if err := tester.Click("Toggle Remotes"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("origin") {
		t.Fatal("reopening Remotes did not reveal refs")
	}

	x, y := splitter.X+splitter.W/2, splitter.Y+splitter.H/2
	previous := s.gitFilePaneWidth
	tester.Press(x, y)
	tester.Move(x+45, y)
	tester.Release(x+45, y)
	tester.Frame()
	if s.gitFilePaneWidth <= previous {
		t.Fatalf("dragging right should enlarge file navigator: before=%v after=%v", previous, s.gitFilePaneWidth)
	}
}

// TestGitSplitSmallWindow checks the local horizontal layout and the
// historical vertical layout at the minimum supported size.
func TestGitSplitSmallWindow(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.git.refreshing = true
	s.showSurface(WorkspaceSurfaceDiff)
	s.rebuildDiffRows()
	tester := ui.NewTester(s.View, 960, 640)
	tester.Frame()
	handle, ok := tester.Find("Resize Git file navigator")
	if !ok || handle.X < sidebarWidth || handle.X >= 960 {
		t.Fatalf("local navigator divider outside compact width: %+v", handle)
	}
	review, ok := tester.Find("src/main.go")
	if !ok || review.X <= handle.X || review.X >= 960 {
		t.Fatalf("Diff must remain to right of divider: %+v divider=%+v", review, handle)
	}
	s.gitFilePaneWidth = 2000
	tester.Frame()
	handle, ok = tester.Find("Resize Git file navigator")
	if !ok || handle.X >= 960-240 {
		t.Fatalf("oversized file tree hid Diff: %+v", handle)
	}
	s.commitsMode = true
	tester.Frame()
	handle, ok = tester.Find("Resize Git history and diff panes")
	if !ok || handle.Y <= 0 || handle.Y >= 640 {
		t.Fatalf("history divider is outside viewport: %+v", handle)
	}
	s.gitUpperHeight = 2000
	tester.Frame()
	handle, ok = tester.Find("Resize Git history and diff panes")
	if !ok || handle.Y+gitSplitMinBottom > 640 {
		t.Fatalf("history swallowed diff region: %+v", handle)
	}
}

// A history tab opened before its first commit arrives must never expose
// stage/commit controls for the unrelated worktree beneath it.
func TestGitHistoryEmptyCannotMutateWorktree(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.git.refreshing = true
	s.showSurface(WorkspaceSurfaceDiff)
	s.commitsMode = true
	tt := ui.NewTester(s.View, 1200, 800)
	tt.Frame()
	if !tt.HasText("No commit selected") || tt.HasText("Stage All") || tt.HasText("Commit…") {
		t.Fatalf("history waiting state leaked worktree controls: %q", tt.Texts())
	}
	s.reviewLocalChanges()
	tt.Frame()
	if s.commitsMode || !tt.HasText("Git actions") || !tt.HasText("Commit subject draft") {
		t.Fatalf("return to worktree must restore navigator and controls: %q", tt.Texts())
	}
}

func TestGitQuickComposerHandoffUsesExistingCommitModal(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.refreshing = true
	s.showSurface(WorkspaceSurfaceDiff)
	s.gitQuickSubject = "feat: new native Git layout"
	s.gitQuickBody = "Prepared in bottom composer"
	s.gitQuickAmend = true
	tt := ui.NewTester(s.View, 1200, 800)
	tt.Frame()
	if err := tt.Click("Commit"); err != nil {
		t.Fatal(err)
	}
	if s.surface.current() != WorkspaceSurfaceCommit || !s.commitDialogOpen {
		t.Fatal("bottom composer must open the sole fenced Commit surface")
	}
	if s.surface.commit.Subject != s.gitQuickSubject ||
		s.surface.commit.Body != s.gitQuickBody ||
		!s.surface.commit.Amend || !s.surface.commit.AmendPrefilled {
		t.Fatalf("draft not preserved across composer handoff: %+v", s.surface.commit)
	}
}

func TestGitHistoryLivesAboveReadOnlyCommitDiff(t *testing.T) {
	root := t.TempDir()
	s := gitSurfaceTestShell(t, root)
	s.git.rootOf[root] = root
	s.git.refreshing = true
	cm := gitworkbench.CommitInfo{Hash: "0123456789abcdef0123456789abcdef01234567", Short: "0123456", Subject: "Add split review", Author: "Example", Time: time.Now()}
	s.git.commits = []gitworkbench.CommitInfo{cm}
	s.showSurface(WorkspaceSurfaceDiff)
	s.commitsMode = true
	s.git.source = gdSourceCommit
	s.git.commitHash = cm.Hash
	commitSnap := *s.git.snapshot
	s.git.commitSnap = &commitSnap
	s.git.commitMeta = &cm
	s.git.gdFilesFor = nil
	s.rebuildDiffRows()

	tt := ui.NewTester(s.View, 1200, 800)
	tt.Frame()
	if !tt.HasText("Add split review") || !tt.HasText("Example") || !tt.HasText("0123456") {
		t.Fatalf("commit metadata missing from upper table: %q", tt.Texts())
	}
	divider, ok := tt.Find("Resize Git history and diff panes")
	row, okRow := tt.Find("Add split review")
	if !ok || !okRow || row.Y >= divider.Y {
		t.Fatalf("commit history should be above lower Diff: row=%+v divider=%+v", row, divider)
	}
	if tt.HasText("Stage All") || tt.HasText("Commit…") {
		t.Fatalf("historical commit Diff must be read-only: %q", tt.Texts())
	}
}
