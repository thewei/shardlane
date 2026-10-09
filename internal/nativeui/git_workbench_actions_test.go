package nativeui

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/commandcenter"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

/**
 * [INPUT]: scratch-less headless Git snapshot and MyGo native interaction
 * [OUTPUT]: Git toolbar action ownership, Commit modal cancellation, Merge/Revert confirmation
 * [POS]: Phase 5 Native Git workbench UX contract, no user Git mutations
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestGitToolbarCompactsWithCenterWidth(t *testing.T) {
	for _, tc := range []struct {
		window             float32
		sidebar, inspector bool
		compact            bool
	}{
		{960, true, false, true},
		{1200, true, false, false},
		{1280, true, true, true},
		{1512, true, true, false},
		{960, false, true, true},
	} {
		if got := gitActionBarCompact(tc.window, tc.sidebar, tc.inspector, DefaultRightPanelWidth); got != tc.compact {
			t.Errorf("window %v sidebar %v inspector %v: compact=%v, want %v", tc.window, tc.sidebar, tc.inspector, got, tc.compact)
		}
	}
}

func TestGitToolbarCompactKeepsPrimaryActions(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	tester := ui.NewTester(s.View, 960, 640)
	tester.Frame()
	for _, want := range []string{"Commit…", "More…"} {
		if !tester.HasText(want) {
			t.Fatalf("compact toolbar lacks %q: %q", want, tester.Texts())
		}
	}
	if tester.HasText("Stage All") || tester.HasText("Unstage All") {
		t.Fatalf("compact toolbar still consumes Diff space: %q", tester.Texts())
	}
	tester.SetSize(1512, 982)
	tester.Frame()
	if !tester.HasText("Stage All") || !tester.HasText("Unstage All") {
		t.Fatalf("expanded toolbar did not restore batch actions: %q", tester.Texts())
	}
}

func TestCommitAIPromptRequiresPreviewAndExplicitOptIn(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.snapshot.Files = append(s.git.snapshot.Files, gitworkbench.ChangeFile{Path: ".env", Status: gitworkbench.StatusModified, Additions: 1, Hunks: []gitworkbench.Hunk{{Lines: []gitworkbench.DiffLine{{Kind: gitworkbench.KindAdd, Text: "SECRET_KEY=leak"}}}}})
	s.showSurface(WorkspaceSurfaceDiff)
	s.openCommitSurface()
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	if tester.HasText("Copy Reviewed AI Prompt") || s.commitPromptIncludeExcerpt {
		t.Fatal("AI handoff must not start with a ready-to-copy source prompt")
	}
	if err := tester.Click("AI Commit Message…"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	for _, want := range []string{"AI commit message", "Include code excerpts (explicit opt-in)", "Generate with Pi", "Review prompt"} {
		if !tester.HasText(want) {
			t.Fatalf("preview lacks %q: %q", want, tester.Texts())
		}
	}
	if s.commitPromptIncludeExcerpt || tester.HasText("Copy Reviewed AI Prompt") {
		t.Fatal("AI preview must default to metadata-only and hide raw prompt")
	}
	if err := tester.Click("Review prompt"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("Copy Reviewed AI Prompt") {
		t.Fatal("review disclosure must reveal a copyable, user-inspected prompt")
	}
	meta := gitworkbench.PrepareCommitPrompt(s.git.snapshot, s.selectedCommitPaths(), s.commitPromptIncludeExcerpt)
	if strings.Contains(meta.Text, "SECRET_KEY=leak") || strings.Contains(meta.Text, "fmt.Println") || meta.ExcludedFiles != 1 {
		t.Fatalf("default preview leaked snippets or failed to exclude sensitive path: %+v", meta)
	}
	s.commitPromptIncludeExcerpt = true
	tester.Frame()
	if err := tester.Click("Paste suggestion…"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("Suggested commit message") || !tester.HasText("Use Suggested Message") {
		t.Fatalf("manual suggestion disclosure missing: %q", tester.Texts())
	}
	optedIn := gitworkbench.PrepareCommitPrompt(s.git.snapshot, s.selectedCommitPaths(), s.commitPromptIncludeExcerpt)
	if !strings.Contains(optedIn.Text, "fmt.Println(\"new\")") || strings.Contains(optedIn.Text, "SECRET_KEY=leak") {
		t.Fatalf("opt-in must never include sensitive-file contents: %+v", optedIn)
	}
	// The suggestion form is nested in a scrollable modal; headless text
	// exposure is tested above, while the state handler is exercised by
	// TestManualAISuggestionRequiresReplacementApproval.
	s.commitAIResponse = "fix: imported suggestion"
	s.applyManualAISuggestion()
	if s.surface.commit.Subject != "fix: imported suggestion" || s.surface.commit.InFlight {
		t.Fatalf("manual suggestion handler failed: %q", s.surface.commit.Subject)
	}
	s.cancelCommitSurface()
	if s.commitPromptPreviewOpen || s.commitPromptIncludeExcerpt || s.commitAIResponse != "" {
		t.Fatal("AI preview flags leaked into a subsequent commit task")
	}
}

func TestGitActionPathsSeparateStagedAndUnstaged(t *testing.T) {
	snap := &gitworkbench.ChangesSnapshot{Files: []gitworkbench.ChangeFile{
		{Path: "unstaged", Unstaged: true},
		{Path: "staged", Staged: true},
		{Path: "both", Staged: true, Unstaged: true},
		{Path: "new", Untracked: true},
	}}
	stage, unstage := gitActionPaths(snap)
	if len(stage) != 3 || len(unstage) != 2 {
		t.Fatalf("stage=%v unstage=%v", stage, unstage)
	}
}

func TestCommitIsNativeModalWithDraftGuard(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.openCommitSurface()
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	if !s.commitDialogOpen || s.surface.current() != WorkspaceSurfaceCommit {
		t.Fatal("Commit editor was not opened as a modal task")
	}
	for _, label := range []string{"Commit changes", "Commit subject", "AI Commit Message…", "Cancel Commit"} {
		if !tester.HasText(label) {
			t.Fatalf("Commit modal missing %q: %q", label, tester.Texts())
		}
	}
	s.surface.commit.Subject = "fix: keep this draft"
	tester.Frame()
	if err := tester.Click("Cancel Commit"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !s.commitDiscardPrompt || s.surface.current() != WorkspaceSurfaceCommit {
		t.Fatal("Cancel dismissed an edited commit draft without asking")
	}
	if err := tester.Click("Keep Editing"); err != nil {
		t.Fatal(err)
	}
	if s.commitDiscardPrompt || s.surface.commit.Subject != "fix: keep this draft" {
		t.Fatal("Keep Editing should preserve the draft")
	}
	if err := tester.Click("Cancel Commit"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if err := tester.Click("Discard Draft"); err != nil {
		t.Fatal(err)
	}
	if s.surface.current() != WorkspaceSurfaceDiff || s.commitDialogOpen || s.surface.commit.Subject != "" {
		t.Fatal("Discard must return to Diff without touching repository state")
	}
}

func TestCommitModalGuardsHeaderAndShortcutNavigation(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.openCommitSurface()
	s.surface.commit.Subject = "draft: keep"
	for _, action := range []func(){s.showViewTerminal, s.showViewHistory, s.showViewChanges,
		func() { s.runShortcut(shortcutSettings) }, func() { s.runShortcut(shortcutBack) }} {
		s.commitDiscardPrompt = false
		action()
		if s.surface.current() != WorkspaceSurfaceCommit || s.router.Path() != routeWorkspace || !s.commitDiscardPrompt {
			t.Fatal("navigation bypassed edited Commit modal")
		}
	}
	s.commitDiscardPrompt = false
	s.surface.commit.Subject = ""
	s.showViewTerminal()
	if s.surface.current() != WorkspaceSurfaceTerminal || s.commitDialogOpen {
		t.Fatal("clean commit dialog should return to Terminal without draft confirmation")
	}
}

func TestCommitModalEscapeProtectsDraft(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.openCommitSurface()
	s.surface.commit.Subject = "feat: draft"
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Key(0, ui.KeyEscape)
	tester.Frame()
	if s.surface.current() != WorkspaceSurfaceCommit || s.surface.commit.Subject != "feat: draft" || !s.commitDiscardPrompt {
		t.Fatalf("Escape dropped draft or failed to explain dismissal: kind=%s prompt=%v", s.surface.current(), s.commitDiscardPrompt)
	}
}

func TestChangesClickFromPristineCommitReturnsToDiff(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.openCommitSurface()
	s.showViewChanges()
	if got := s.surface.current(); got != WorkspaceSurfaceDiff {
		t.Fatalf("Changes from untouched Commit should restore Diff, got %s", got)
	}
}

func TestCommitDraftGuardsCommandCenterAndAgentDestinations(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.openCommitSurface()
	s.surface.commit.Subject = "fix: keep"
	s.openCommandCenter(commandcenter.ScopeAll)
	if s.commandCenterOpen || !s.commitDiscardPrompt || s.surface.current() != WorkspaceSurfaceCommit {
		t.Fatal("Command Center bypassed edited Commit modal")
	}
}

func TestCommitDialogDoesNotDismissWhileInFlight(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.openCommitSurface()
	s.surface.commit.InFlight = true
	s.requestCancelCommit()
	if !s.commitDialogOpen || s.surface.current() != WorkspaceSurfaceCommit || s.pendingToast == "" {
		t.Fatal("in-flight Commit must refuse to close")
	}
}

func TestGitBranchMergeAsksBeforeMutation(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.git.branches = []gitworkbench.Branch{{Name: "main", Current: true}, {Name: "topic"}}
	s.branchMenuOpen = true
	tester := ui.NewTester(s.View, 1200, 800)
	if !tester.HasText("More…") {
		t.Fatalf("branch row missing overflow action: %q", tester.Texts())
	}
	// Native NSMenu cannot be driven by headless Tester. Its handler closes
	// the branch Popover before showing the strategy modal.
	s.branchMenuOpen = false
	s.openMergeDialog("topic")
	if !s.mergeDialogOpen || s.confirmOpen {
		t.Fatal("branch merge must open the strategy dialog before confirmation")
	}
	tester.Frame()
	if !tester.HasText("Fast-forward only") || !tester.HasText("Create merge commit") {
		t.Fatalf("merge dialog missing strategy choices: %q", tester.Texts())
	}
	if err := tester.Click("Review merge"); err != nil {
		t.Fatal(err)
	}
	if !s.confirmOpen || s.confirmKind != "git-merge-ff" || s.confirmTarget != "topic" {
		t.Fatalf("FF merge must reach its own confirmation: %q/%q", s.confirmKind, s.confirmTarget)
	}
	s.confirmOpen = false
	s.openMergeDialog("topic")
	s.mergeStrategy = 1
	tester.Frame()
	if err := tester.Click("Review merge"); err != nil {
		t.Fatal(err)
	}
	if !s.confirmOpen || s.confirmKind != "git-merge-no-ff" || s.confirmTarget != "topic" {
		t.Fatalf("merge-commit strategy must request distinct confirmation: %q/%q", s.confirmKind, s.confirmTarget)
	}
}

func TestDirtyBranchSwitchHasLiveConfirmationHandler(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.confirmSwitchBranch("topic")
	if !s.confirmOpen || s.confirmKind != "git-switch-branch" || s.confirmRepoRoot != s.git.root {
		t.Fatalf("dirty switch must bind a routed Git confirmation, got %q root=%q", s.confirmKind, s.confirmRepoRoot)
	}
}

func TestGitConfirmRefusesRepositoryChange(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.confirmRevertCommit("0123456789abcdef0123456789abcdef01234567", "selected")
	if !s.confirmOpen || s.confirmRepoRoot != s.git.root {
		t.Fatal("confirmation should remember original Git root")
	}
	s.git.root = t.TempDir()
	s.submitConfirm(s.confirmKind, s.confirmTarget)
	if s.git.opBusy || s.pendingToast == "" {
		t.Fatal("confirming in a different repository must fail without mutation")
	}
}

func TestMergeDialogRefusesRepositoryChange(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.openMergeDialog("topic")
	if !s.mergeDialogOpen || s.mergeRepoRoot != s.git.root {
		t.Fatal("merge selector did not capture its repository root")
	}
	s.git.root = t.TempDir()
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	if s.mergeDialogOpen {
		t.Fatal("merge selector must close if repository context changed")
	}
}

func TestGitRevertRequiresFullHashAndConfirmation(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.confirmRevertCommit("deadbeef", "old change")
	if s.confirmOpen {
		t.Fatal("abbreviated ref must not open Revert confirmation")
	}
	s.confirmRevertCommit("0123456789abcdef0123456789abcdef01234567", "old change")
	if !s.confirmOpen || s.confirmKind != "git-revert-commit" {
		t.Fatal("full commit hash should request Revert confirmation")
	}
}
