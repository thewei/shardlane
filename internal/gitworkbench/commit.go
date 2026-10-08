package gitworkbench

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// CommitDraft is the user-authored commit intent (plan §22): subject
// required, body optional, selected paths only. No AI generation in 0.10.
// Amend replaces the last commit instead of adding one; the caller prefills
// the message from LastCommitMessage.
type CommitDraft struct {
	Subject string
	Body    string
	Paths   []string
	Amend   bool
}

// CommitPreflight is the stale-state fence captured at Commit-surface open
// and revalidated immediately before mutation (plan §22.3).
type CommitPreflight struct {
	Root      string
	Head      string
	Branch    string
	Signature string
	// Fingerprints maps each selected path to the ChangeFile fingerprint the
	// user reviewed (presentation cache identity).
	Fingerprints map[string]string
	// ContentIDs maps each selected path to its authoritative live content/object
	// identity (blob hash via git hash-object, "deleted", or symlink target).
	ContentIDs map[string]string
	// Untracked marks paths that must be `git add`ed before the pathspec
	// commit (untracked paths are invisible to `git commit -- <path>`).
	Untracked map[string]bool
}

// CommitResult is the verified outcome of one transaction.
type CommitResult struct {
	Hash      string
	Branch    string
	Committed []string
}

// ErrStale is returned when the repository moved between review and commit;
// the UI must re-review, never auto-commit.
var ErrStale = errors.New("review snapshot is stale: repository changed since review")

// CapturePreflight snapshots HEAD/branch/status signature plus per-file
// fingerprints and live object identities for the selected paths.
func (r *Runner) CapturePreflight(ctx context.Context, root string, snap *ChangesSnapshot, selected []string) (CommitPreflight, error) {
	facts, err := r.Facts(ctx, root)
	if err != nil {
		return CommitPreflight{}, err
	}
	pre := CommitPreflight{
		Root:         root,
		Head:         facts.Head,
		Branch:       facts.Branch,
		Signature:    facts.Signature,
		Fingerprints: make(map[string]string, len(selected)),
		ContentIDs:   make(map[string]string, len(selected)),
		Untracked:    map[string]bool{},
	}
	for _, path := range selected {
		cf, ok := snap.ByPath(path)
		if !ok {
			return CommitPreflight{}, fmt.Errorf("selected file %q is not in the reviewed snapshot", path)
		}
		pre.Fingerprints[path] = cf.Fingerprint
		if cf.Untracked {
			pre.Untracked[path] = true
		}
		id, err := r.filePathIdentity(ctx, root, path)
		if err == nil {
			pre.ContentIDs[path] = id
		}
	}
	return pre, nil
}

// filePathIdentity computes the live authoritative content identity for a path
// in root: git hash-object for existing files, symlink target for symlinks, or
// "deleted" if missing from the worktree.
func (r *Runner) filePathIdentity(ctx context.Context, root, relPath string) (string, error) {
	full := filepath.Join(root, relPath)
	info, err := os.Lstat(full)
	if err != nil {
		if os.IsNotExist(err) {
			return "@deleted@", nil
		}
		return "", err
	}
	if info.Mode()&os.ModeSymlink != 0 {
		target, err := os.Readlink(full)
		if err != nil {
			return "", err
		}
		return "@symlink:" + target, nil
	}
	out, err := r.Read(ctx, root, "hash-object", "--no-filters", "--", relPath)
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(string(out)), nil
}

// revalidate re-reads repository facts and live content identity for each selected path,
// failing closed on any drift between the review snapshot and the live repository.
func (r *Runner) revalidate(ctx context.Context, root string, pre CommitPreflight, snap *ChangesSnapshot) error {
	facts, err := r.Facts(ctx, root)
	if err != nil {
		return err
	}
	if facts.Head != pre.Head || facts.Branch != pre.Branch || facts.Signature != pre.Signature {
		return fmt.Errorf("%w: HEAD/branch/status drifted", ErrStale)
	}
	for path, preID := range pre.ContentIDs {
		liveID, err := r.filePathIdentity(ctx, root, path)
		if err != nil || liveID != preID {
			return fmt.Errorf("%w: %s changed since review", ErrStale, path)
		}
	}
	// Fall back to presentation fingerprint check if content identity was not captured
	for path, fp := range pre.Fingerprints {
		if _, hasContentID := pre.ContentIDs[path]; !hasContentID {
			cf, ok := snap.ByPath(path)
			if !ok || cf.Fingerprint != fp {
				return fmt.Errorf("%w: %s changed since review", ErrStale, path)
			}
		}
	}
	if r.MergeInProgress(ctx, root) {
		return &Error{Class: ErrMergeInProgess, OpClass: OpMutate, Op: "commit",
			err: errors.New("a merge/revert/cherry-pick is in progress; resolve it outside Shardlane first")}
	}
	return nil
}

// Commit executes the index-preserving selected-file transaction (plan
// §22.4). Holds the single-flight repository lock for the entire transaction.
func (r *Runner) Commit(ctx context.Context, root string, draft CommitDraft, pre CommitPreflight, snap *ChangesSnapshot) (CommitResult, error) {
	unlock, err := r.LockRepo(root, "commit")
	if err != nil {
		return CommitResult{Branch: pre.Branch}, err
	}
	defer unlock()

	result := CommitResult{Branch: pre.Branch}
	if strings.TrimSpace(draft.Subject) == "" {
		return result, errors.New("commit subject is required")
	}
	if len(draft.Paths) == 0 {
		return result, errors.New("no files selected for commit")
	}
	if err := r.revalidate(ctx, root, pre, snap); err != nil {
		return result, err
	}

	paths := append([]string(nil), draft.Paths...)
	sort.Strings(paths)

	// 1. Stage selected untracked paths only (never `git add -A`).
	var untracked []string
	for _, p := range paths {
		if pre.Untracked[p] {
			untracked = append(untracked, p)
		}
	}
	if len(untracked) > 0 {
		args := append([]string{"add", "--"}, untracked...)
		if _, err := r.run(ctx, OpMutate, MutateTimeout, MaxReadOutput, mutateEnv(), root, args); err != nil {
			return result, fmt.Errorf("staging selected untracked files failed: %w", err)
		}
	}

	// 2. Pathspec commit; the message travels by stdin (`commit -F -`), so
	// it is never in argv and never shell-interpolated. Hooks run normally.
	args := []string{"commit", "-F", "-"}
	if draft.Amend {
		args = append(args, "--amend", "--no-edit")
	}
	args = append(args, "--")
	args = append(args, paths...)

	stdin := draft.Subject + "\n"
	if body := strings.TrimSpace(draft.Body); body != "" {
		stdin += "\n" + body + "\n"
	}
	out, err := r.runStdin(ctx, OpMutate, MutateTimeout, root, stdin, args)
	if err != nil {
		return result, r.wrapCommitFailure(err, untracked)
	}

	result.Hash = parseCommitHash(string(out))
	result.Committed = paths
	return result, nil
}

// LastCommitMessage returns the subject and body of HEAD, for the Amend
// prefill. It fails on an unborn branch.
func (r *Runner) LastCommitMessage(ctx context.Context, root string) (subject, body string, err error) {
	out, err := r.Read(ctx, root, "log", "-1", "--format=%s%x00%b")
	if err != nil {
		return "", "", err
	}
	parts := strings.SplitN(string(out), "\x00", 2)
	subject = strings.TrimSpace(parts[0])
	if len(parts) > 1 {
		body = strings.TrimRight(parts[1], "\n")
	}
	return subject, body, nil
}

// wrapCommitFailure documents the index-safe recovery state after a definite
// commit failure (plan GWB-215): selected untracked files may remain staged
// by the explicit `git add`; nothing else moved.
func (r *Runner) wrapCommitFailure(err error, untracked []string) error {
	ge, ok := GitError(err)
	if !ok {
		return err
	}
	detail := "commit failed"
	switch ge.Class {
	case ErrHookFailed:
		detail = "commit rejected by a Git hook; nothing was committed, the index is unchanged"
	case ErrMergeInProgess:
		detail = "a merge/revert/cherry-pick is in progress; resolve it first, nothing was committed"
	case ErrConflict:
		detail = "Git refused the commit; nothing was committed"
	}
	if len(untracked) > 0 {
		detail += fmt.Sprintf("; %d selected untracked file(s) may remain staged and can be unstaged with `git restore --staged`", len(untracked))
	}
	return fmt.Errorf("%s: %w", detail, err)
}

// parseCommitHash extracts the short hash from commit's stdout
// ("[main abc1234] subject").
func parseCommitHash(out string) string {
	open := strings.Index(out, "] ")
	if open < 0 || !strings.HasPrefix(out, "[") {
		return ""
	}
	fields := strings.Fields(out[1:open])
	if len(fields) >= 2 {
		return fields[len(fields)-1]
	}
	return ""
}
