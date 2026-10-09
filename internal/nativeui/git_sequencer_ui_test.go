package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

/**
 * [INPUT]: cached sequencer state, Git Diff, selected repo and MyGo headless tester
 * [OUTPUT]: visual/interaction regression for conflict detection, disabled Continue and abort confirm
 * [POS]: Git Workbench conflict UI tests; no real repository mutation
 * [PROTOCOL]: update header on change, then check CLAUDE.md
 */

func TestGitConflictBannerAndInspectorFollowActiveRepo(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.showSurface(WorkspaceSurfaceDiff)
	s.git.sequencerChecked = true
	s.git.sequencer = gitworkbench.SequencerStatus{Kind: gitworkbench.SequencerMerge, Unmerged: 2, UnmergedPaths: []string{"src/main.go", "README.md"}}
	s.openRightPanelSurface(SurfaceChanges)
	tester := ui.NewTester(s.View, 1280, 800)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	tester.Frame()
	for _, want := range []string{"Merge needs attention", "Git Operation", "Unresolved", "Continue Merge", "Abort Merge", "Refresh Git State", "Files needing resolution", "Review conflict: src/main.go"} {
		if !tester.HasText(want) {
			t.Fatalf("Git conflict UX missing %q: %q", want, tester.Texts())
		}
	}
	if err := tester.Click("Review conflict: src/main.go"); err != nil {
		t.Fatal(err)
	}
	if s.surface.diff.SelectedPath != "src/main.go" {
		t.Fatalf("review inspector did not navigate to conflicted file: %q", s.surface.diff.SelectedPath)
	}
	// Continue is disabled until resolved files are staged.
	_ = tester.Click("Continue Merge")
	if s.git.opBusy || s.confirmOpen {
		t.Fatal("Continue must not start a mutation while conflicts remain")
	}
	if err := tester.Click("Abort Merge"); err != nil {
		t.Fatal(err)
	}
	if !s.confirmOpen || s.confirmKind != "git-sequencer-abort" || s.confirmTarget != "merge" {
		t.Fatalf("Abort must open explicit confirmation, got %q/%q", s.confirmKind, s.confirmTarget)
	}
}

func TestGitConflictBannerFollowsSequencerKind(t *testing.T) {
	for _, kind := range []gitworkbench.SequencerKind{gitworkbench.SequencerRevert, gitworkbench.SequencerCherryPick} {
		t.Run(string(kind), func(t *testing.T) {
			s := gitSurfaceTestShell(t, t.TempDir())
			s.showSurface(WorkspaceSurfaceDiff)
			s.git.sequencerChecked = true
			s.git.sequencer = gitworkbench.SequencerStatus{Kind: kind}
			tester := ui.NewTester(s.View, 1200, 800)
			tester.Frame()
			want := "Continue " + gitOperationLabel(kind)
			if !tester.HasText(want) {
				t.Fatalf("sequencer %s missing %q: %q", kind, want, tester.Texts())
			}
			if tester.HasText("Abort Merge") {
				t.Fatal("wrong operation affordance leaked into conflict")
			}
		})
	}
}

func TestGitConflictReadOnlyHistoryDoesNotOfferWorktreeActions(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.git.sequencerChecked = true
	s.git.sequencer = gitworkbench.SequencerStatus{Kind: gitworkbench.SequencerMerge, Unmerged: 1}
	s.git.source = gdSourceCommit
	s.git.commitHash = "0123456789abcdef0123456789abcdef01234567"
	s.git.commitSnap = &gitworkbench.ChangesSnapshot{Root: s.git.root}
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	if tester.HasText("Continue Merge") || tester.HasText("Stage All") {
		t.Fatalf("historical diff should remain read only: %q", tester.Texts())
	}
}

func TestGitConflictRootResetClearsInspectorState(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.sequencerChecked = true
	s.git.sequencer = gitworkbench.SequencerStatus{Kind: gitworkbench.SequencerMerge, Unmerged: 2}
	s.resetGitContextState()
	if s.git.sequencerChecked || s.git.sequencer.Active() {
		t.Fatal("sequencer state of previous repository leaked after context reset")
	}
}

func TestCherryPickUsesFullHashAndConfirm(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.confirmCherryPickCommit("abc123", "message")
	if s.confirmOpen {
		t.Fatal("abbreviated SHA cannot open cherry-pick")
	}
	hash := "0123456789abcdef0123456789abcdef01234567"
	s.confirmCherryPickCommit(hash, "message")
	if !s.confirmOpen || s.confirmKind != "git-cherry-pick" || s.confirmTarget != hash {
		t.Fatal("cherry-pick must require exact selected commit + confirmation")
	}
}
