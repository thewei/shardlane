package gitworkbench

/**
 * [INPUT]: Git's per-worktree sequencer marker paths and unmerged index entries
 * [OUTPUT]: bounded read-only operation status, validated Continue/Abort actions
 * [POS]: local Git operation lifecycle owner, no UI/Herdr dependencies
 * [PROTOCOL]: update this header when the file changes, then check CLAUDE.md
 */

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// SequencerKind identifies Git's active conflict-resolution transaction.
// Only operations explicitly supported by this package can be continued or
// aborted; an unknown/rebase state must not be interpreted as clean.
type SequencerKind string

const (
	SequencerNone        SequencerKind = ""
	SequencerMerge       SequencerKind = "merge"
	SequencerRevert      SequencerKind = "revert"
	SequencerCherryPick  SequencerKind = "cherry-pick"
	SequencerUnsupported SequencerKind = "unsupported"
)

// SequencerStatus contains no file bodies, commit messages, or credentials.
type SequencerStatus struct {
	Kind     SequencerKind
	Unmerged int
	// UnmergedPaths is a bounded, sorted subset of actual unmerged index
	// entries; Unmerged remains the exact total even when the list is long.
	UnmergedPaths []string
}

const maxSequencerDisplayPaths = 24

func (s SequencerStatus) Active() bool { return s.Kind != SequencerNone }

// SequencerState asks Git for *this worktree's* state. It fails closed if
// any marker lookup fails and never trusts a stale UI snapshot.
func (r *Runner) SequencerState(ctx context.Context, root string) (SequencerStatus, error) {
	var state SequencerStatus
	for _, pair := range []struct {
		marker string
		kind   SequencerKind
	}{
		{"rebase-merge", SequencerUnsupported},
		{"rebase-apply", SequencerUnsupported},
		{"MERGE_HEAD", SequencerMerge},
		{"REVERT_HEAD", SequencerRevert},
		{"CHERRY_PICK_HEAD", SequencerCherryPick},
	} {
		raw, err := r.Read(ctx, root, "rev-parse", "--git-path", pair.marker)
		if err != nil {
			return state, err
		}
		path := strings.TrimSpace(string(raw))
		if path == "" {
			return state, errors.New("Git did not resolve a sequencer marker")
		}
		if !filepath.IsAbs(path) {
			path = filepath.Join(root, path)
		}
		_, err = os.Stat(path)
		if err == nil {
			// A rebase takes precedence if multiple markers are present.
			if state.Kind == SequencerNone || pair.kind == SequencerUnsupported {
				state.Kind = pair.kind
			}
		} else if !os.IsNotExist(err) {
			return state, err
		}
	}
	out, err := r.Read(ctx, root, "ls-files", "-u", "-z")
	if err != nil {
		return state, err
	}
	seen := make(map[string]struct{})
	for _, entry := range strings.Split(string(out), "\x00") {
		if entry == "" {
			continue
		}
		if tab := strings.IndexByte(entry, '\t'); tab >= 0 {
			seen[entry[tab+1:]] = struct{}{}
		} else {
			return state, errors.New("invalid unmerged index entry")
		}
	}
	state.Unmerged = len(seen)
	if state.Unmerged > 0 {
		paths := make([]string, 0, len(seen))
		for path := range seen {
			paths = append(paths, path)
		}
		sort.Strings(paths)
		if len(paths) > maxSequencerDisplayPaths {
			paths = paths[:maxSequencerDisplayPaths]
		}
		state.UnmergedPaths = paths
	}
	// Unmerged entries without a supported sequencer (e.g. a manual
	// merge state) must never be offered a fabricated Continue action.
	if state.Kind == SequencerNone && state.Unmerged > 0 {
		state.Kind = SequencerUnsupported
	}
	return state, nil
}

// FinishSequencer revalidates live state before any Git mutation. Abort is
// explicit and may throw away conflict-resolution edits, so the UI confirms
// first. Continue refuses to operate while any index entry is unresolved.
func (r *Runner) FinishSequencer(ctx context.Context, root string, expected SequencerKind, abort bool) error {
	state, err := r.SequencerState(ctx, root)
	if err != nil {
		return err
	}
	if expected == SequencerNone || state.Kind != expected ||
		state.Kind == SequencerUnsupported {
		return &Error{Class: ErrStaleState, OpClass: OpMutate, Op: "sequencer",
			err: fmt.Errorf("Git operation changed since selection (now %s)", state.Kind)}
	}
	if !abort && state.Unmerged > 0 {
		return &Error{Class: ErrConflict, OpClass: OpMutate, Op: "sequencer",
			err: fmt.Errorf("%d files still have unresolved conflicts; resolve and stage them first", state.Unmerged)}
	}
	args := []string{string(state.Kind), "--continue"}
	if abort {
		args[1] = "--abort"
	}
	// Avoid an editor hanging a background worker. Git keeps its prepared
	// MERGE_MSG / REVERT_MSG; no commit subject is synthesized here.
	if !abort {
		args = append([]string{"-c", "core.editor=true"}, args...)
	}
	_, err = r.Mutate(ctx, root, args...)
	return err
}

// MergeNoFastForward performs an explicitly confirmed merge. It cannot
// silently merge a dirty tree; conflicts may leave MERGE_HEAD for the user to
// resolve in the inspector or Terminal.
func (r *Runner) MergeNoFastForward(ctx context.Context, root, target string) error {
	if err := r.validateLocalBranch(ctx, root, target); err != nil {
		return err
	}
	current := r.CurrentBranch(ctx, root)
	if current == "" || current == target {
		return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "merge",
			err: errors.New("select a different local branch while on a checked-out branch")}
	}
	if err := r.RequireCleanWorktree(ctx, root); err != nil {
		return err
	}
	_, err := r.Mutate(ctx, root, "merge", "--no-ff", "--no-edit", "refs/heads/"+target)
	return err
}

// CherryPickCommit applies one verified full commit ID to current HEAD.
// Selecting a merge commit without mainline is deliberately refused.
func (r *Runner) CherryPickCommit(ctx context.Context, root, hash string) error {
	if !validFullCommitID(hash) {
		return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "cherry-pick", err: errors.New("a full Git commit hash is required")}
	}
	if err := r.RequireCleanWorktree(ctx, root); err != nil {
		return err
	}
	parents, err := r.Read(ctx, root, "rev-list", "--parents", "-n", "1", hash)
	if err != nil {
		return err
	}
	if len(strings.Fields(string(parents))) == 0 {
		return &Error{Class: ErrStaleState, OpClass: OpMutate, Op: "cherry-pick", err: errors.New("commit no longer exists")}
	}
	if len(strings.Fields(string(parents))) > 2 {
		return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "cherry-pick", err: errors.New("merge commits need an explicit mainline; not supported")}
	}
	_, err = r.Mutate(ctx, root, "cherry-pick", hash)
	return err
}
