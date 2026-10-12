package nativeui

/**
 * [INPUT]: Current fenced Git snapshot, Git & AI settings, existing Pi suggestion adapter
 * [OUTPUT]: AI candidate applied ONLY to Local Changes quick commit subject, with async stale guards
 * [POS]: Lightweight commit composer coordinator, no Git mutation or automatic commit
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"strings"

	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// User chose AI Commit in the explicit split-button menu. The review is
// metadata-only by default, exactly like the existing Pi modal. A changed
// root, change snapshot, or manually edited subject makes the response stale.
func (s *Shell) startQuickAICommit() {
	if s.git == nil || s.git.root == "" || s.piQuickRunning || s.git.opBusy || s.git.committing || s.gdReviewingCommit() {
		return
	}
	snap := s.gitSnapshot()
	if snap == nil || len(snap.Files) == 0 {
		s.piQuickError = "No changes to generate commit message from."
		return
	}
	paths := make([]string, 0, len(snap.Files))
	for _, cf := range snap.Files {
		paths = append(paths, cf.Path)
	}
	preview := gitworkbench.PrepareCommitPrompt(snap, paths, false)
	if preview.Text == "" {
		s.piQuickError = "No eligible files; sensitive paths were excluded."
		return
	}
	rules := commitPromptPreference(s.settings.Workbench.CommitPrompt)
	prompt := agent.BuildPiCommitPrompt(rules, preview.Text)
	if len(prompt) > agent.MaxPiCommitInputBytes {
		s.piQuickError = "Changes exceed maximum prompt length."
		return
	}
	root, signature, before := s.git.root, snap.Signature, s.gitQuickSubject
	executable := s.settings.Workbench.PiExecutable
	ctx, cancel := context.WithCancel(context.Background())
	s.piQuickCancel = cancel
	s.piQuickGen++
	generation := s.piQuickGen
	s.piQuickRunning = true
	s.piQuickError = ""
	executor := s.piCommitExecutor
	if executor == nil {
		executor = agent.GeneratePiCommitSuggestion
	}
	go func() {
		result, err := executor(ctx, agent.PiCommitRequest{Executable: executable, Root: root, Prompt: prompt})
		s.applyOnUI(func() {
			if generation != s.piQuickGen {
				return
			}
			s.piQuickCancel = nil
			s.piQuickRunning = false
			if s.git == nil || s.git.root != root || s.gitSnapshot() == nil || s.gitSnapshot().Signature != signature ||
				s.gitQuickSubject != before || s.surface.current() != WorkspaceSurfaceDiff || s.commitsMode ||
				commitPromptPreference(s.settings.Workbench.CommitPrompt) != rules || s.settings.Workbench.PiExecutable != executable {
				s.piQuickError = "Changes or draft changed. Generate again to avoid overwriting your edits."
				return
			}
			if err != nil {
				if ctx.Err() != nil {
					s.piQuickError = "Generation canceled."
				} else {
					s.piQuickError = firstErrLine(err)
				}
				return
			}
			subject, _, parseErr := parseCommitCandidate(result.Message)
			if parseErr != nil {
				s.piQuickError = parseErr.Error()
				return
			}
			s.gitQuickSubject = strings.TrimSpace(subject)
			s.piQuickError = ""
			s.pendingToast = "Commit subject generated."
		})
	}()
}

func (s *Shell) cancelQuickAICommit() {
	s.piQuickGen++
	if s.piQuickCancel != nil {
		s.piQuickCancel()
		s.piQuickCancel = nil
	}
	s.piQuickRunning = false
	s.piQuickError = ""
}
