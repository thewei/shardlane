package nativeui

import (
	"github.com/wh-studio/herdr-client/next/internal/settings"
)

// The content header bar was removed in the 2026-10-05 round-two review:
// identity moved into the title bar (breadcrumb + muted path line), the
// Terminal/Changes switch became icon tabs in the title bar, and the
// surface actions moved into the title-bar ⋯ menu (header_overflow.go).

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

// toggleDiffLayout flips unified/split and persists the choice (GWB-174).
func (s *Shell) toggleDiffLayout() {
	if s.git == nil {
		return
	}
	s.git.splitLayout = !s.git.splitLayout
	split := s.git.splitLayout
	s.applySettings(func(settings *settings.Settings) error {
		settings.Workbench.SplitDiff = split
		return nil
	})
	s.rebuildDiffRows()
}

// openCommitSurface enters the Commit surface and captures the fence.
func (s *Shell) openCommitSurface() {
	s.surface.openCommit()
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

// cancelCommitSurface leaves Commit back to the previous Diff state.
func (s *Shell) cancelCommitSurface() {
	if s.surface.cancelCommit() == WorkspaceSurfaceTerminal {
		s.showSurface(WorkspaceSurfaceTerminal)
		return
	}
	s.ensureGitSnapshot(false)
}
