package nativeui

import (
	"context"
	"log/slog"
	"strings"

	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// submitCommit runs the selected-file commit transaction in the background
// with the live stale-state fence (plan §22.3/§22.4). No mutex is acquired
// on the UI lane and no Git IO runs on the UI update callback (P0-06): the
// gitworkbench Runner owns per-repository transaction serialization, and the
// post-commit snapshot refresh is a single background refresh.
func (s *Shell) submitCommit(subject, body string, selected []string) {
	if s.git == nil || s.git.committing || s.git.root == "" || len(selected) == 0 {
		return
	}
	root := s.git.root
	pre := s.git.preflight
	snap := s.git.preflightSnap
	if pre == nil || snap == nil {
		s.surface.commit.ErrText = "No commit preflight captured; reopen the Commit surface."
		return
	}
	s.git.committing = true
	s.surface.commit.InFlight = true
	s.surface.commit.ErrText = ""

	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), gitworkbench.MutateTimeout)
		defer cancel()
		result, err := s.git.runner.Commit(ctx, root,
			gitworkbench.CommitDraft{Subject: subject, Body: body, Paths: selected}, *pre, snap)
		slog.Info("git commit finished", "op_class", "mutate", "op", "commit", "ok", err == nil)

		s.applyOnUI(func() {
			s.git.committing = false
			s.surface.commit.InFlight = false
			if err != nil {
				s.surface.commit.ErrText = err.Error()
				s.refreshGitChanges()
				return
			}
			s.surface.commit.CommittedHash = result.Hash
			s.git.preflight = nil
			s.git.preflightSnap = nil
			s.git.cache.Invalidate(root)
			s.surface.commitSucceeded(true)
			s.git.expectDrift = true
			s.ensureGitSnapshot(true)
			s.loadBranches()
		})
	}()
}

// submitCommitAmend is submitCommit with an amend switch: amend replaces the
// last commit (message prefilled from HEAD) instead of adding one.
func (s *Shell) submitCommitAmend(subject, body string, selected []string, amend bool) {
	if amend && (s.git == nil || s.git.snapshot == nil || s.git.snapshot.NoHead) {
		s.surface.commit.ErrText = "Nothing to amend: this branch has no commits yet."
		return
	}
	if s.git == nil || s.git.committing || s.git.root == "" || len(selected) == 0 {
		return
	}
	root := s.git.root
	pre := s.git.preflight
	snap := s.git.preflightSnap
	if pre == nil || snap == nil {
		s.surface.commit.ErrText = "No commit preflight captured; reopen the Commit surface."
		return
	}
	s.git.committing = true
	s.surface.commit.InFlight = true
	s.surface.commit.ErrText = ""

	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), gitworkbench.MutateTimeout)
		defer cancel()
		result, err := s.git.runner.Commit(ctx, root,
			gitworkbench.CommitDraft{Subject: subject, Body: body, Paths: selected, Amend: amend}, *pre, snap)
		slog.Info("git commit finished", "op_class", "mutate", "op", "commit", "amend", amend, "ok", err == nil)

		s.applyOnUI(func() {
			s.git.committing = false
			s.surface.commit.InFlight = false
			if err != nil {
				s.surface.commit.ErrText = err.Error()
				s.refreshGitChanges()
				return
			}
			s.surface.commit.CommittedHash = result.Hash
			s.surface.commit.Amend = false
			s.surface.commit.AmendPrefilled = false
			s.git.preflight = nil
			s.git.preflightSnap = nil
			s.git.cache.Invalidate(root)
			s.surface.commitSucceeded(true)
			s.ensureGitSnapshot(true)
			s.loadBranches()
		})
	}()
}

// captureCommitPreflight runs at Commit-surface open (background lane) under
// the dedicated preflight generation.
func (s *Shell) captureCommitPreflight() {
	if s.git == nil || s.git.root == "" || s.git.snapshot == nil {
		return
	}
	root := s.git.root
	snap := s.git.snapshot
	var selected []string
	for path := range s.surface.commit.Selected {
		if s.surface.commit.Selected[path] {
			selected = append(selected, path)
		}
	}
	if len(selected) == 0 {
		for _, cf := range snap.Files {
			selected = append(selected, cf.Path)
		}
	}

	gen := s.git.preflightGen.Add(1)
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), gitworkbench.ReadTimeout)
		defer cancel()
		pre, err := s.git.runner.CapturePreflight(ctx, root, snap, selected)
		s.applyGuarded(s.git.preflightGen.Load, gen, func() {
			if root != s.git.root {
				return
			}
			if err != nil {
				s.surface.commit.ErrText = err.Error()
				return
			}
			s.git.preflight = &pre
			s.git.preflightSnap = snap
			s.surface.commit.Captured = true
		})
	}()
}

// switchBranch runs the branch switch transaction after confirmation
// (plan §23.2). Git's refusal is surfaced, never fought. No UI-lane lock.
func (s *Shell) switchBranch(name string) {
	if s.git == nil || s.git.branchBusy || s.git.root == "" || name == "" {
		return
	}
	root := s.git.root
	s.git.branchBusy = true

	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), gitworkbench.MutateTimeout)
		defer cancel()
		_, err := s.git.runner.SwitchBranch(ctx, root, name)
		slog.Info("git branch switch finished", "op_class", "mutate", "op", "switch", "ok", err == nil)

		s.applyOnUI(func() {
			s.git.branchBusy = false
			if err != nil {
				s.status = "Branch switch refused"
				s.errText = err.Error()
				return
			}
			s.git.cache.Invalidate(root)
			s.git.preflight = nil
			s.surface.branchSwitched()
			s.ensureGitSnapshot(true)
			s.loadBranches()
		})
	}()
}

// createBranch creates and switches to a new local branch. No UI-lane lock.
func (s *Shell) createBranch(name string) {
	if s.git == nil || s.git.branchBusy || s.git.root == "" || strings.TrimSpace(name) == "" {
		return
	}
	root := s.git.root
	s.git.branchBusy = true

	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), gitworkbench.MutateTimeout)
		defer cancel()
		_, err := s.git.runner.CreateBranch(ctx, root, name)
		slog.Info("git branch create finished", "op_class", "mutate", "op", "switch", "ok", err == nil)

		s.applyOnUI(func() {
			s.git.branchBusy = false
			if err != nil {
				s.status = "Branch creation failed"
				s.errText = err.Error()
				return
			}
			s.git.cache.Invalidate(root)
			s.surface.branchSwitched()
			s.ensureGitSnapshot(true)
			s.loadBranches()
		})
	}()
}
