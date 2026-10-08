package gitworkbench

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"path/filepath"
	"strings"
)

// RepoFacts is the identity set of one repository at one moment: root,
// current branch, HEAD hash, and a status signature covering the full
// working-tree/index state. It is the stale-state fence input (plan §22.3).
type RepoFacts struct {
	Root      string
	Branch    string
	Head      string // empty when the repository has no commits yet
	Signature string
	// NoHead marks an unborn branch (repository without any commit).
	NoHead bool
}

// ResolveRoot walks up from dir to the enclosing worktree top-level. The
// empty string means "not inside a repository".
func (r *Runner) ResolveRoot(ctx context.Context, dir string) (string, error) {
	out, err := r.Read(ctx, dir, "rev-parse", "--show-toplevel")
	if err != nil {
		return "", err
	}
	root := strings.TrimSpace(string(out))
	if root == "" {
		return "", &Error{Class: ErrNotARepo, OpClass: OpRead, Op: "rev-parse"}
	}
	return filepath.Clean(root), nil
}

// Facts captures root/branch/HEAD plus the status signature in as few Git
// calls as possible. The signature hashes `status --porcelain=v1 -z` output
// (index + worktree + untracked), so any index/worktree mutation changes it.
func (r *Runner) Facts(ctx context.Context, root string) (RepoFacts, error) {
	facts := RepoFacts{Root: root}

	if head, err := r.Read(ctx, root, "rev-parse", "--verify", "--quiet", "HEAD"); err == nil {
		facts.Head = strings.TrimSpace(string(head))
	} else {
		// Unborn branch: distinguish from hard failures.
		if ge, ok := GitError(err); ok && ge.Class != ErrNotARepo {
			facts.NoHead = true
		} else if !ok {
			facts.NoHead = true
		} else {
			return facts, err
		}
	}

	if branch, err := r.Read(ctx, root, "rev-parse", "--abbrev-ref", "HEAD"); err == nil {
		facts.Branch = strings.TrimSpace(string(branch))
	}

	status, err := r.Read(ctx, root, "status", "--porcelain=v1", "-z", "--untracked-files=all")
	if err != nil {
		return facts, err
	}
	sum := sha256.Sum256(append(facts.HeadByte(), status...))
	facts.Signature = hex.EncodeToString(sum[:8])
	return facts, nil
}

// HeadByte returns the HEAD hash bytes for signature hashing, with a stable
// marker for the unborn-branch case.
func (f RepoFacts) HeadByte() []byte {
	if f.Head == "" {
		return []byte("@unborn@")
	}
	return []byte(f.Head)
}

// MergeInProgress reports an in-progress merge/revert/cherry-pick state,
// where Git refuses partial commits (fail-closed input for Commit).
func (r *Runner) MergeInProgress(ctx context.Context, root string) bool {
	for _, marker := range []string{"MERGE_HEAD", "REVERT_HEAD", "CHERRY_PICK_HEAD"} {
		// rev-parse --git-path resolves per-worktree marker paths portably.
		out, err := r.Read(ctx, root, "rev-parse", "--git-path", marker)
		if err != nil {
			continue
		}
		path := strings.TrimSpace(string(out))
		if path == "" {
			continue
		}
		if !filepath.IsAbs(path) {
			path = filepath.Join(root, path)
		}
		if fileExists(path) {
			return true
		}
	}
	return false
}
