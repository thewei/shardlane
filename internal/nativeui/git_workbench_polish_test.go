package nativeui

/**
 * [INPUT]: MyGo Git controls, folder changed paths, fake asynchronous Pi suggestion
 * [OUTPUT]: Layout switching, nonduplicated refresh, folder staging selection, AI composer handoff
 * [POS]: Git polish regression and no-automatic-commit contract
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

func TestGitLayoutButtonsActuallySwitchAndStayVisible(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	tt := ui.NewTester(s.View, 960, 640)
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	tt.Frame()
	if !tt.HasText("Split") || !tt.HasText("Unified") {
		t.Fatal("compact Git UI must still expose layout control")
	}
	for i := 0; i < 3; i++ {
		if err := tt.Click("Unified"); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		if s.git.splitLayout {
			t.Fatalf("Unified did not switch layout on pass %d", i)
		}
		if err := tt.Click("Split"); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		if !s.git.splitLayout {
			t.Fatalf("Split did not restore layout on pass %d", i)
		}
	}
	if err := tt.Click("Git actions"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	menu := strings.Join(tt.Menu(), " | ")
	for _, want := range []string{"Stash All Changes", "View All Commits", "Git Settings"} {
		if !strings.Contains(menu, want) {
			t.Fatalf("Git actions lost %s: %s", want, menu)
		}
	}
	// Fetch / Pull / Push are toolbar icons now; Refresh lives in the
	// sidebar header. Neither repeats in the pull-down.
	for _, dup := range []string{"Fetch", "Pull", "Push", "Refresh"} {
		if strings.Contains(menu, dup) {
			t.Fatalf("duplicate %s in Git actions: %s", dup, menu)
		}
	}
	tt.CloseMenu()
}

// A button's face, probed by hand: whether hovering fills a point along its
// edge, and whether it leaves a point near a corner bare (a rounded square
// of the shared radius does; a flatter radius paints into the corner).
func hoverFaceOf(tt *ui.Tester, label string) (edgeFilled, cornerBare bool) {
	rect, ok := tt.Find(label)
	if !ok {
		return false, false
	}
	at := func() (edge, corner [4]uint8) {
		settleFrames(tt, 2) // hover takes a pass to be seen, a second to be painted
		return pixelAt(tt, int(rect.X)+2, int(rect.Y+rect.H/2)), pixelAt(tt, int(rect.X)+2, int(rect.Y)+2)
	}
	tt.Move(1, 1)
	idleEdge, idleCorner := at()
	tt.Move(rect.X+rect.W/2, rect.Y+rect.H/2)
	hotEdge, hotCorner := at()
	return rgbDiff(hotEdge, idleEdge) > 6, rgbDiff(hotCorner, idleCorner) <= 2
}

// The Git review's icon buttons wear the shared icon-button face
// (gorexIconButton) rather than a look of their own: hovered, each paints the
// same rounded square as a shared button of its size — the toolbar icons and
// the Git actions pull-down at the toolbar size, the composer's Commit options
// chevron at the denser pane-header size.
func TestGitIconButtonsWearTheSharedFace(t *testing.T) {
	for _, dark := range []bool{false, true} {
		shared := func(size, glyph float32) (edge, corner bool) {
			reference := ui.NewTester(func(c *ui.Context) {
				ui.Row(c).FillWidth().Padding(20).Background(designTokens(c.Theme().Dark).Content).Children(func() {
					gorexIconButton(c, gorexColorsOf(c.Theme().Dark), iconFetch, "Reference", "", false, size, glyph)
				})
			}, 200, 80)
			reference.SetDark(dark)
			reference.SetPreferences(ui.Preferences{ReduceMotion: true})
			reference.Frame()
			return hoverFaceOf(reference, "Reference")
		}
		toolbarEdge, toolbarCorner := shared(gorexIconBtn, gorexIconGlyph)
		paneEdge, paneCorner := shared(gorexPaneBtn, gorexPaneGlyph)
		if !toolbarEdge || !toolbarCorner || !paneEdge {
			t.Fatalf("dark=%v: the probe cannot read the shared face (toolbar edge=%v corner=%v, pane edge=%v)",
				dark, toolbarEdge, toolbarCorner, paneEdge)
		}

		s := gitSurfaceTestShell(t, t.TempDir())
		s.showSurface(WorkspaceSurfaceDiff)
		tt := ui.NewTester(s.View, 1280, 760)
		tt.SetDark(dark)
		tt.SetPreferences(ui.Preferences{ReduceMotion: true})
		tt.Frame()
		for _, tc := range []struct {
			label        string
			edge, corner bool
		}{
			{"Fetch", toolbarEdge, toolbarCorner},
			{"Pull (fast-forward only)", toolbarEdge, toolbarCorner},
			{"Push", toolbarEdge, toolbarCorner},
			{"Git actions", toolbarEdge, toolbarCorner},
			{"Commit options", paneEdge, paneCorner},
		} {
			if edge, corner := hoverFaceOf(tt, tc.label); edge != tc.edge || corner != tc.corner {
				t.Errorf("dark=%v %s: hover fills its edge=%v and leaves the corner bare=%v; the shared face does %v and %v",
					dark, tc.label, edge, corner, tc.edge, tc.corner)
			}
		}
	}
}

// The shared face must not cost the Git review its keyboard: unlike the
// shared Box buttons its toolbar icons stay Tab stops (the file-header Copy
// button even appears with FocusVisible), and the Git actions pull-down too.
func TestGitToolbarIconButtonsStayTabStops(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	tt := ui.NewTester(s.View, 1280, 760)
	tt.Frame()
	for _, label := range []string{"Fetch", "Pull (fast-forward only)", "Push", "Git actions", "Copy Path"} {
		reached := false
		for i := 0; i < 80 && !reached; i++ {
			tt.Key(0, ui.KeyTab)
			tt.Frame()
			reached = tt.Focused(label)
		}
		if !reached {
			t.Errorf("Tab never reaches %q; the Git review's toolbar is no longer keyboard-reachable", label)
		}
	}
}

func TestGitDirectoryPathsRespectPaneAndPrefixBoundary(t *testing.T) {
	snap := &gitworkbench.ChangesSnapshot{Files: []gitworkbench.ChangeFile{
		{Path: "src/a.go", Unstaged: true},
		{Path: "src/deep/b.go", Unstaged: true, Staged: true},
		{Path: "src/staged.go", Staged: true},
		{Path: "src-old/no.go", Unstaged: true},
		{Path: "third_party/x.go", Untracked: true},
	}}
	for _, tc := range []struct {
		pane gdViewSide
		want string
	}{
		{gdViewUnstaged, "src/a.go,src/deep/b.go"},
		{gdViewStaged, "src/deep/b.go,src/staged.go"},
		{gdViewCombined, ""},
	} {
		got := strings.Join(gitDirectoryPaths(snap, "src", tc.pane), ",")
		if got != tc.want {
			t.Fatalf("directory paths for pane %v = %s want %s", tc.pane, got, tc.want)
		}
	}
}

func TestGitFolderContextMenuSelectsAllChangedDescendants(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.git.refreshing = true
	s.git.snapshot.Files = []gitworkbench.ChangeFile{
		{Path: "src/a.go", Unstaged: true, Status: gitworkbench.StatusModified},
		{Path: "src/deep/b.go", Unstaged: true, Status: gitworkbench.StatusModified},
		{Path: "other/c.go", Unstaged: true, Status: gitworkbench.StatusModified},
	}
	s.showSurface(WorkspaceSurfaceDiff)
	s.rebuildDiffRows()
	tt := ui.NewTester(s.View, 1280, 800)
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	tt.Frame()
	if err := tt.RightClick("src"); err != nil {
		t.Fatal(err)
	}
	menu := strings.Join(tt.Menu(), " | ")
	if !strings.Contains(menu, "Select All Files in Folder") || !strings.Contains(menu, "Stage Folder (2 Files)") {
		t.Fatalf("directory menu incomplete: %q", menu)
	}
	if err := tt.ChooseMenuItem("Select All Files in Folder"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if !s.unstagedChoice.Has("src/a.go") || !s.unstagedChoice.Has("src/deep/b.go") || s.unstagedChoice.Has("other/c.go") {
		t.Fatalf("directory selection leaked to unrelated paths")
	}
}

func TestGitQuickAICommitFillsSubjectButNeverCommits(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.git.refreshing = true
	s.showSurface(WorkspaceSurfaceDiff)
	s.gitQuickBody = "Keep manually written body"
	queue := make(chan func(), 2)
	s.uiApplyOverride = func(fn func()) { queue <- fn }
	s.piCommitExecutor = func(ctx context.Context, req agent.PiCommitRequest) (agent.PiCommitResponse, error) {
		if req.Root != s.git.root || !strings.Contains(req.Prompt, "File: src/main.go") || !strings.Contains(req.Prompt, "Do NOT stage, commit, push") ||
			strings.Contains(req.Prompt, "fmt.Println") {
			return agent.PiCommitResponse{}, context.Canceled
		}
		return agent.PiCommitResponse{Message: "feat: refine Git operations\n\nGenerated details"}, nil
	}
	tt := ui.NewTester(s.View, 1200, 800)
	tt.Frame()
	if err := tt.Click("Commit options"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if err := tt.ChooseMenuItem("Draft Subject with Pi"); err != nil {
		t.Fatal(err)
	}
	if !s.piQuickRunning {
		t.Fatal("Pi was not started by split button")
	}
	deadline := time.After(3 * time.Second)
	for s.piQuickRunning {
		select {
		case apply := <-queue:
			apply()
		case <-deadline:
			t.Fatal("Pi completion timeout")
		}
	}
	if s.gitQuickSubject != "feat: refine Git operations" || s.gitQuickBody != "Keep manually written body" || s.git.committing || s.surface.current() != WorkspaceSurfaceDiff {
		t.Fatalf("AI must fill Subject only and never commit: subject=%q body=%q error=%q mode=%v", s.gitQuickSubject, s.gitQuickBody, s.piQuickError, s.surface.current())
	}
}

func TestGitQuickAIRejectsStaleDraft(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.git.refreshing = true
	s.showSurface(WorkspaceSurfaceDiff)
	queue := make(chan func(), 1)
	s.uiApplyOverride = func(fn func()) { queue <- fn }
	s.piCommitExecutor = func(context.Context, agent.PiCommitRequest) (agent.PiCommitResponse, error) {
		return agent.PiCommitResponse{Message: "feat: obsolete"}, nil
	}
	s.startQuickAICommit()
	s.gitQuickSubject = "feat: my newer draft"
	deadline := time.After(3 * time.Second)
	for s.piQuickRunning {
		select {
		case f := <-queue:
			f()
		case <-deadline:
			t.Fatal("Pi timeout")
		}
	}
	if s.gitQuickSubject != "feat: my newer draft" || s.piQuickError == "" || s.git.committing {
		t.Fatal("stale AI result overwrote user draft")
	}
}
