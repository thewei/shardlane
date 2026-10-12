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
	if err := tt.Click("More Git actions"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	menu := strings.Join(tt.Menu(), " | ")
	for _, want := range []string{"Fetch", "Pull", "Push", "Git Preferences"} {
		if !strings.Contains(menu, want) {
			t.Fatalf("overflow lost %s: %s", want, menu)
		}
	}
	if strings.Contains(menu, "Refresh") {
		t.Fatalf("duplicate Refresh in Git actions: %s", menu)
	}
	tt.CloseMenu()
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
	if !strings.Contains(menu, "Select all files in folder") || !strings.Contains(menu, "Stage folder (2 files)") {
		t.Fatalf("directory menu incomplete: %q", menu)
	}
	if err := tt.ChooseMenuItem("Select all files in folder"); err != nil {
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
	if err := tt.ChooseMenuItem("Draft subject with Pi"); err != nil {
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
