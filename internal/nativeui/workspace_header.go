package nativeui

import (
	"github.com/wh-studio/herdr-client/internal/settings"
)

// The content header bar was removed in the 2026-10-05 round-two review:
// identity moved into the title bar (breadcrumb + muted path line), the
// Terminal/Changes switch became icon tabs in the title bar, and the
// surface actions moved into the title-bar Actions menu (header_overflow.go).

// workspaceTitle remains for callers that need the display name.
func (s *Shell) workspaceTitle() string {
	if project := s.selectedProject(); project != nil && project.Label != "" {
		return project.Label
	}
	if repo := s.gitRepoName(); repo != "" {
		return repo
	}
	return "Workspace"
}

// setDiffLayout updates only when the requested layout differs, making both
// icon buttons and compact overflow routes obey one source of truth.
func (s *Shell) setDiffLayout(split bool) {
	if s.git == nil || s.git.splitLayout == split {
		return
	}
	s.git.splitLayout = split
	s.applySettings(func(cfg *settings.Settings) error {
		cfg.Workbench.SplitDiff = split
		return nil
	})
	s.rebuildDiffRows()
}

func (s *Shell) toggleDiffLayout() {
	if s.git != nil {
		s.setDiffLayout(!s.git.splitLayout)
	}
}

// openCommitSurface retains the existing stale-state preflight, but moves
// the commit editor into a native modal over the underlying Diff Review.
func (s *Shell) openCommitSurface() {
	if s.surface.current() == WorkspaceSurfaceCommit {
		s.commitDialogOpen = true
		return
	}
	s.surface.openCommit()
	s.commitDialogOpen = true
	s.commitDiscardPrompt = false
	s.resetCommitAIFlow()
	snap := s.gitSnapshot()
	if snap == nil {
		s.surface.commit.ErrText = "Changes are still loading."
		return
	}
	// Default selection: all visible changed files (plan §22.2).
	for _, cf := range snap.Files {
		s.surface.commit.Selected[cf.Path] = true
	}
	s.captureCommitPreflight()
}

// requestCancelCommit protects message edits and in-flight Git mutations.
func (s *Shell) requestCancelCommit() {
	if s.surface.commit.InFlight || s.git != nil && s.git.committing {
		s.pendingToast = "Commit in progress; wait for it to finish."
		return
	}
	if s.surface.commit.Subject != "" || s.surface.commit.Body != "" {
		s.commitDiscardPrompt = true
		return
	}
	s.cancelCommitSurface()
}

// holdCommitNavigation intercepts intent to leave the sole Commit editor.
// The caller may navigate only after the modal is cleanly cancelled. An
// edited draft asks for deliberate Discard; no route/surface change occurs.
func (s *Shell) holdCommitNavigation() bool {
	if s.surface.current() != WorkspaceSurfaceCommit {
		return false
	}
	s.commitDialogOpen = true
	s.requestCancelCommit()
	return s.surface.current() == WorkspaceSurfaceCommit
}

// cancelCommitSurface leaves Commit back to the previous Diff state.
func (s *Shell) cancelCommitSurface() {
	s.commitDialogOpen = false
	s.commitDiscardPrompt = false
	s.resetCommitAIFlow()
	if s.surface.cancelCommit() == WorkspaceSurfaceTerminal {
		s.showSurface(WorkspaceSurfaceTerminal)
		return
	}
	s.ensureGitSnapshot(false)
}
