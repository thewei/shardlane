package nativeui

/**
 * [INPUT]: Headless MyGo Git page, repository ref fixtures and native pointer events
 * [OUTPUT]: Git tree grouping, center split geometry, file navigator auto-close and commit review regression tests
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
// historical vertical layout at the minimum supported size, with the file
// navigator open (a narrow window closes it until the user opens it).
func TestGitSplitSmallWindow(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.git.refreshing = true
	s.gitFilePaneOpenNarrow = true
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

// Beside a 252 px sidebar, a 292 px navigator leaves a 900 px window's diff
// 350 px: the navigator closes by itself there, and a toolbar button brings
// it back. The user's choice is kept per width class, so opening it in a
// narrow window does not undo hiding it in a wide one.
func TestGitFileNavigatorClosesByItselfInANarrowWindow(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.git.refreshing = true
	s.showSurface(WorkspaceSurfaceDiff)
	s.rebuildDiffRows()

	const navigator = "Resize Git file navigator"
	open := func(w int) bool {
		tt := ui.NewTester(s.View, w, 700)
		tt.Frame()
		_, ok := tt.Find(navigator)
		return ok
	}
	press := func(w int, label string) {
		t.Helper()
		tt := ui.NewTester(s.View, w, 700)
		tt.Frame()
		if err := tt.Click(label); err != nil {
			t.Fatalf("%dpx: %v (texts %q)", w, err, tt.Texts())
		}
	}

	if open(900) {
		t.Fatal("the file navigator stays open in a 900 px window")
	}
	if !open(1340) {
		t.Fatal("the file navigator is closed in a 1340 px window")
	}

	press(900, "Show file list")
	if !open(900) {
		t.Fatal("Show file list did not open the navigator in a narrow window")
	}
	press(900, "Hide file list")
	if open(900) {
		t.Fatal("Hide file list did not close the navigator again")
	}

	press(1340, "Hide file list")
	if open(1340) {
		t.Fatal("Hide file list did not close the navigator in a wide window")
	}
	press(900, "Show file list")
	if open(1340) {
		t.Fatal("opening the navigator in a narrow window reopened it in a wide one")
	}
	press(1340, "Show file list")
	if !open(1340) || !open(900) {
		t.Fatalf("each width class keeps its own choice: wide=%v narrow=%v", open(1340), open(900))
	}
}

// The sidebar's width is not the diff's to use when it is hidden: the same
// 900 px window keeps the navigator once the sidebar is out of the way.
func TestGitFileNavigatorJudgesTheCenterNotTheWindow(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.git.refreshing = true
	s.showSurface(WorkspaceSurfaceDiff)
	s.rebuildDiffRows()

	s.sidebarCollapsed = true
	tt := ui.NewTester(s.View, 900, 700)
	tt.Frame()
	if _, ok := tt.Find("Resize Git file navigator"); !ok {
		t.Fatalf("a 900 px center closed the navigator: %q", tt.Texts())
	}
}

// A commit review has the same navigator beside its diff, and the same rule.
func TestGitCommitReviewNavigatorClosesInANarrowWindow(t *testing.T) {
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

	narrow := ui.NewTester(s.View, 900, 800)
	narrow.Frame()
	if _, ok := narrow.Find("Resize Git file navigator"); ok {
		t.Fatal("the commit review's navigator stays open in a 900 px window")
	}
	if err := narrow.Click("Show file list"); err != nil {
		t.Fatalf("a closed commit navigator offers no way back: %v (texts %q)", err, narrow.Texts())
	}
	narrow.Frame()
	if _, ok := narrow.Find("Resize Git file navigator"); !ok {
		t.Fatal("Show file list did not open the commit review's navigator")
	}

	wide := ui.NewTester(s.View, 1340, 800)
	wide.Frame()
	if _, ok := wide.Find("Resize Git file navigator"); !ok {
		t.Fatal("the commit review's navigator is closed in a 1340 px window")
	}
}

// The commit review's Commit / Changes / File Tree views are a native
// segmented control: choosing a segment switches the lower view, and the
// selected one wears the face of a plain ui.Segmented — one face for every
// segmented control in the app, not a hand-painted tab with a tint of its own.
func TestGitCommitReviewViewsAreASegmentedControl(t *testing.T) {
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
	s.gitLowerTab = 1
	s.rebuildDiffRows()

	// The review rendered without the title bar (which also says "Changes"),
	// above a plain Segmented to compare faces with.
	reference := 1
	tt := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).FillWidth().Grow(1).Children(func() {
			ui.Segmented(c, &reference, "Ref A", "Ref B").Shrink(0)
			ui.Column(c).Grow(1).MinHeight(0).Children(func() { s.gitCommitReview(c) })
		})
	}, 1100, 700)
	tt.Frame()
	if _, ok := tt.Find("Commit view"); !ok {
		t.Fatalf("the lower views are not one labelled segmented control; texts=%q", tt.Texts())
	}
	// A spot just inside the left edge of a segment's text, clear of glyphs.
	chip := func(label string) [4]uint8 {
		t.Helper()
		rect, ok := tt.Find(label)
		if !ok {
			t.Fatalf("no %q segment; texts=%q", label, tt.Texts())
		}
		return pixelAt(tt, int(rect.X)-3, int(rect.Y+rect.H/2))
	}
	tt.Move(1090, 690)
	tt.Frame()
	selected, native := chip("Changes"), chip("Ref B")
	if rgbDiff(selected, native) > 2 {
		t.Fatalf("the selected view wears %v where a plain Segmented's selected segment wears %v", selected, native)
	}
	if idle := chip("File Tree"); rgbDiff(selected, idle) < 6 {
		t.Fatalf("the selected view (%v) is not told apart from an unselected one (%v)", selected, idle)
	}

	if err := tt.Click("File Tree"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if s.gitLowerTab != 2 {
		t.Fatalf("File Tree did not switch the lower view: tab=%d", s.gitLowerTab)
	}
	if err := tt.Click("Changes"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if s.gitLowerTab != 1 {
		t.Fatalf("Changes did not switch the lower view back: tab=%d", s.gitLowerTab)
	}
}

// The two Commit buttons take their size from a shared tier instead of an
// inline font size: the dense Git strip's composer wears the compact tier,
// and the Commit dialog's footer pairs the primary with Cancel Commit on the
// regular one — the two were the only Buttons restating Body / BodySmall.
func TestCommitPrimaryButtonsTakeTheirSizeFromASharedTier(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)

	composer := ui.NewTester(func(c *ui.Context) { s.gitQuickCommitForm(c, s.git.snapshot) }, 600, 200)
	composer.Frame()
	reference := ui.NewTester(func(c *ui.Context) { compactPrimaryButton(c, "Commit") }, 200, 60)
	reference.Frame()
	got, ok := composer.Find("Commit")
	want, refOK := reference.Find("Commit")
	if !ok || !refOK {
		t.Fatalf("missing Commit button; composer=%q", composer.Texts())
	}
	if got.H != want.H {
		t.Fatalf("composer Commit text is %.2f high, the compact primary tier's is %.2f", got.H, want.H)
	}

	s.openCommitSurface()
	footer := ui.NewTester(s.commitActionFooter, 800, 100)
	footer.Frame()
	selected := len(s.selectedCommitPaths())
	cancel, cancelOK := footer.Find("Cancel Commit")
	primary, primaryOK := footer.Find("Commit " + itoa(selected) + " file" + pluralS(selected))
	if !cancelOK || !primaryOK {
		t.Fatalf("footer lacks its buttons; texts=%q", footer.Texts())
	}
	if cancel.H != primary.H {
		t.Fatalf("Cancel Commit text is %.2f high but the primary beside it is %.2f", cancel.H, primary.H)
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
