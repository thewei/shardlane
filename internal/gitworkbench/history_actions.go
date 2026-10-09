package gitworkbench

import (
	"context"
	"errors"
	"fmt"
	"strings"
)

/**
 * [INPUT]: 依赖 Runner 的受限 git Read/Mutate 和本地分支的事实校验
 * [OUTPUT]: 提供 MergeFastForward 与 RevertCommit 安全 Git 操作
 * [POS]: Git history mutation boundary; never touches Herdr or UI, never forces/reset-hard
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// RequireCleanWorktree prevents history mutations from silently combining
// pending local edits with merge/revert. This checks both staged and unstaged
// files and untracked files; a failed status read fails closed.
func (r *Runner) RequireCleanWorktree(ctx context.Context, root string) error {
	out, err := r.Read(ctx, root, "status", "--porcelain=v1", "--untracked-files=all")
	if err != nil {
		return err
	}
	if len(out) != 0 {
		return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "history",
			err: errors.New("working tree is not clean; commit or stash changes first")}
	}
	status, err := r.SequencerState(ctx, root)
	if err != nil {
		return err
	}
	if status.Active() {
		return &Error{Class: ErrMergeInProgess, OpClass: OpMutate, Op: "history",
			err: errors.New("a merge, revert, cherry-pick or rebase is already in progress")}
	}
	return nil
}

// MergeFastForward updates the checked-out branch to a selected EXISTING
// local branch, only if the current HEAD is its ancestor. A divergent
// history is refused: no conflict markers, no surprise merge commits.
// Users can decide separately whether they want a no-ff merge in a later
// explicitly designed conflict-resolution workflow.
func (r *Runner) MergeFastForward(ctx context.Context, root, target string) error {
	if err := r.validateLocalBranch(ctx, root, target); err != nil {
		return err
	}
	current := r.CurrentBranch(ctx, root)
	if current == "" || target == current {
		return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "merge",
			err: errors.New("select a different branch while checked out on a local branch")}
	}
	if err := r.RequireCleanWorktree(ctx, root); err != nil {
		return err
	}
	// Full refs, after local-branch enumeration, cannot be interpreted as
	// Git command-line options, even for unusual user-defined ref names.
	_, err := r.Mutate(ctx, root, "merge", "--ff-only", "refs/heads/"+target)
	return err
}

// RevertCommit creates an inverse commit for an exact full Git commit ID,
// preserving public history. The UI must confirm the selected commit and
// explain that conflicts may require manual resolution in the Terminal.
func (r *Runner) RevertCommit(ctx context.Context, root, hash string) error {
	if !validFullCommitID(hash) {
		return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "revert",
			err: fmt.Errorf("invalid full commit id %q", hash)}
	}
	if err := r.RequireCleanWorktree(ctx, root); err != nil {
		return err
	}
	if _, err := r.Read(ctx, root, "rev-parse", "--verify", "--quiet", hash+"^{commit}"); err != nil {
		return &Error{Class: ErrStaleState, OpClass: OpMutate, Op: "revert",
			err: errors.New("selected commit no longer exists")}
	}
	_, err := r.Mutate(ctx, root, "revert", "--no-edit", hash)
	return err
}

func validFullCommitID(hash string) bool {
	if len(hash) != 40 && len(hash) != 64 {
		return false
	}
	for _, ch := range strings.ToLower(hash) {
		if ch < '0' || ch > '9' && ch < 'a' || ch > 'f' {
			return false
		}
	}
	return true
}
