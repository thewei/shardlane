package nativeui

import (
	"context"

	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// ensureGitSnapshot applies a cached snapshot or starts a background refresh.
// Uses the dedicated snapshot generation: branch loading, file expansion or
// terminal find can never drop a snapshot result (P0-04).
func (s *Shell) ensureGitSnapshot(force bool) {
	if s.git == nil || s.git.root == "" {
		return
	}
	root := s.git.root
	if !force {
		if snap, fresh := s.git.cache.Get(root); fresh {
			s.applyGitSnapshot(snap)
			return
		}
		if !s.git.cache.NeedsRefresh(root) {
			return
		}
	}
	if s.git.refreshing {
		return
	}
	s.git.refreshing = true
	gen := s.git.snapshotGen.Add(1)

	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 6*gitworkbench.ReadTimeout)
		defer cancel()
		snap, _ := s.git.cache.Refresh(ctx, root)
		s.applyGuarded(s.git.snapshotGen.Load, gen, func() {
			s.git.refreshing = false
			if snap != nil && snap.Root == s.git.root {
				s.applyGitSnapshot(snap)
			}
		})
	}()
}

// applyGitSnapshot swaps the visible change snapshot when the context still
// matches (stale results may populate the cache but never the view, plan §36).
// The right-panel Changes tree is invalidated so it rebuilds from the new
// snapshot (P1-12); the stale banner only clears when no drift was detected
// or the user explicitly refreshed (P2-01).
func (s *Shell) applyGitSnapshot(snap *gitworkbench.ChangesSnapshot) {
	if snap == nil || snap.Root != s.git.root {
		return
	}
	switch {
	case s.git.expectDrift:
		// The app itself mutated the repository; the signature change is
		// ours, not external drift.
		s.git.expectDrift = false
		s.git.staleBanner = false
	case s.git.snapshot != nil && s.git.snapshot.Signature != "" &&
		snap.Signature != s.git.snapshot.Signature:
		// External drift observed between the displayed and fresh snapshot.
		s.git.staleBanner = true
	default:
		s.git.staleBanner = false
	}
	s.git.snapshot = snap
	s.changesTree = nil
	s.gdSetFiles()

	// Selected file removed on refresh falls back to a valid one (GWB-183).
	if s.surface.diff.SelectedPath != "" {
		if _, ok := snap.ByPath(s.surface.diff.SelectedPath); !ok {
			s.surface.diff.SelectedPath = firstChangedPath(snap)
		}
	}
	s.rebuildDiffRows()
}

func firstChangedPath(snap *gitworkbench.ChangesSnapshot) string {
	// The header Changes toggle calls openChanges("") before any snapshot
	// exists (non-repo cwd, refresh still in flight): that must degrade to
	// an empty Diff selection, not crash the process.
	if snap == nil {
		return ""
	}
	for _, cf := range snap.Files {
		return cf.Path
	}
	return ""
}

// refreshGitChanges is the explicit Refresh action.
func (s *Shell) refreshGitChanges() {
	if s.git == nil || s.git.root == "" {
		return
	}
	s.git.staleBanner = false
	s.git.cache.Invalidate(s.git.root)
	s.ensureGitSnapshot(true)
}

// loadBranches refreshes the local branch list, the upstream tracking
// status and the stash list in one background lane. Its own generation keeps
// it independent of snapshot/commit/terminal-find work (P0-04).
func (s *Shell) loadBranches() {
	if s.git == nil || s.git.root == "" {
		return
	}
	root := s.git.root
	s.git.branchesLoading = true
	gen := s.git.branchesGen.Add(1)

	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), gitworkbench.ReadTimeout)
		defer cancel()
		branches, err := s.git.runner.ListBranches(ctx, root)
		upstream, _ := s.git.runner.Upstream(ctx, root)
		stashes, _ := s.git.runner.StashList(ctx, root)
		tags, _ := s.git.runner.ListTags(ctx, root)
		worktrees, _ := s.git.runner.ListWorktrees(ctx, root)
		commits, _ := s.git.runner.ListCommits(ctx, root, 200)
		s.applyGuarded(s.git.branchesGen.Load, gen, func() {
			s.git.branchesLoading = false
			if err != nil || root != s.git.root {
				return
			}
			s.git.branches = branches
			s.git.upstream = upstream
			s.git.stashes = stashes
			s.git.tags = tags
			s.git.worktrees = worktrees
			s.git.commits = commits
		})
	}()
}

// currentBranchName returns the branch reported by the current snapshot.
func (s *Shell) currentBranchName() string {
	if s.git == nil || s.git.snapshot == nil {
		return ""
	}
	return s.git.snapshot.Branch
}

// openChanges opens the Changes-driven Diff surface (changed-file click).
func (s *Shell) openChanges(path string) {
	ctx := s.workspaceContext()
	if path == "" {
		path = firstChangedPath(s.gitSnapshot())
	}
	s.surface.openDiff(ctx, path)
	if s.git != nil {
		gitworkbench.EnsureAncestorsOpen(s.git.collapsedDirs, path)
	}
	s.ensureGitSnapshot(false)
	s.rebuildDiffRows()
}
