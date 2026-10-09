package gitworkbench

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

// Working-tree and repository operations behind the Changes surface, beyond
// snapshot reading and commit: stage/unstage, discard, stash, fetch/pull/
// push, branch deletion and soft undo. Every mutation runs only on explicit
// user intent through the single-flight Runner; nothing here runs
// automatically, and destructive operations are confirmed by the UI first.
// The package-level bans stay: no force, no reset --hard, no clean, and no
// network operation except the explicitly invoked fetch/pull/push here.

// NetworkTimeout bounds one explicit network operation (fetch/pull/push).
const NetworkTimeout = 120 * time.Second

// Stage moves worktree changes of paths into the index (`git add --`).
func (r *Runner) Stage(ctx context.Context, root string, paths []string) error {
	if len(paths) == 0 {
		return errors.New("no paths to stage")
	}
	args := append([]string{"add", "--"}, paths...)
	_, err := r.Mutate(ctx, root, args...)
	return err
}

// Unstage moves index changes of paths back out of the index. On an unborn
// branch the staged files are simply unlinked from the index (`git rm
// --cached`), since there is no HEAD to restore from.
func (r *Runner) Unstage(ctx context.Context, root string, noHead bool, paths []string) error {
	if len(paths) == 0 {
		return errors.New("no paths to unstage")
	}
	var args []string
	if noHead {
		args = append([]string{"rm", "--cached", "--"}, paths...)
	} else {
		args = append([]string{"restore", "--staged", "--"}, paths...)
	}
	_, err := r.Mutate(ctx, root, args...)
	return err
}

// Discard throws away the worktree changes of tracked paths (`git restore
// --`). The UI must confirm before calling: the content is unrecoverable.
func (r *Runner) Discard(ctx context.Context, root string, paths []string) error {
	if len(paths) == 0 {
		return errors.New("no paths to discard")
	}
	_, err := r.Mutate(ctx, root, append([]string{"restore", "--"}, paths...)...)
	return err
}

// DeleteUntracked removes untracked paths from the worktree directly — the
// sanctioned replacement for `git clean -f`, which stays banned. The UI must
// confirm before calling: the files are deleted, not stashed.
func (r *Runner) DeleteUntracked(root string, paths []string) error {
	if len(paths) == 0 {
		return errors.New("no paths to delete")
	}
	if len(paths) > MaxUntrackedFiles {
		return &Error{Class: ErrTooLarge, OpClass: OpMutate, Op: "delete",
			err: fmt.Errorf("%d paths exceed the delete bound", len(paths))}
	}
	for _, rel := range paths {
		if strings.Contains(rel, "\x00") || filepath.IsAbs(rel) {
			return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "delete",
				err: fmt.Errorf("refusing to delete %q", rel)}
		}
		// The joined path must stay inside the repository.
		target := filepath.Clean(filepath.Join(root, filepath.FromSlash(rel)))
		if target != root && !strings.HasPrefix(target, root+string(filepath.Separator)) {
			return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "delete",
				err: fmt.Errorf("refusing to delete %q", rel)}
		}
		if err := os.Remove(target); err != nil && !os.IsNotExist(err) {
			return &Error{Class: ErrOther, OpClass: OpMutate, Op: "delete", err: err}
		}
	}
	return nil
}

// Stash is one entry of the stash list.
type Stash struct {
	Ref     string // e.g. stash@{0}
	Subject string
}

// StashPush stashes the worktree and index; untracked files join with -u so
// "stash" puts the whole change set away, as the Changes list shows it.
func (r *Runner) StashPush(ctx context.Context, root, message string) error {
	args := []string{"stash", "push", "-u"}
	if message != "" {
		args = append(args, "-m", message)
	}
	_, err := r.Mutate(ctx, root, args...)
	return err
}

// StashList enumerates the stash entries, newest first.
func (r *Runner) StashList(ctx context.Context, root string) ([]Stash, error) {
	out, err := r.Read(ctx, root, "stash", "list", "--format=%gd\t%s")
	if err != nil {
		return nil, err
	}
	var stashes []Stash
	for _, line := range strings.Split(strings.TrimRight(string(out), "\n"), "\n") {
		if line == "" {
			continue
		}
		parts := strings.SplitN(line, "\t", 2)
		s := Stash{Ref: parts[0]}
		if len(parts) > 1 {
			s.Subject = parts[1]
		}
		stashes = append(stashes, s)
	}
	return stashes, nil
}

// StashPop applies a stash entry and drops it on success.
func (r *Runner) StashPop(ctx context.Context, root, ref string) error {
	if !validStashRef(ref) {
		return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "stash",
			err: fmt.Errorf("invalid stash ref %q", ref)}
	}
	_, err := r.Mutate(ctx, root, "stash", "pop", ref)
	return err
}

// StashDrop deletes one stash entry.
func (r *Runner) StashDrop(ctx context.Context, root, ref string) error {
	if !validStashRef(ref) {
		return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "stash",
			err: fmt.Errorf("invalid stash ref %q", ref)}
	}
	_, err := r.Mutate(ctx, root, "stash", "drop", ref)
	return err
}

// validStashRef accepts only git's own stash references, so a name can never
// travel as an option value.
func validStashRef(ref string) bool {
	if !strings.HasPrefix(ref, "stash@{") || !strings.HasSuffix(ref, "}") {
		return false
	}
	for _, c := range ref[len("stash@{") : len(ref)-1] {
		if c < '0' || c > '9' {
			return false
		}
	}
	return true
}

// DefaultRemote returns the repository's remote to act against: the first
// configured remote, or "origin" when none is configured yet.
func (r *Runner) DefaultRemote(ctx context.Context, root string) string {
	out, err := r.Read(ctx, root, "remote")
	if err != nil {
		return "origin"
	}
	for _, line := range strings.Split(strings.TrimRight(string(out), "\n"), "\n") {
		if line = strings.TrimSpace(line); line != "" {
			return line
		}
	}
	return "origin"
}

// CurrentBranch returns the checked-out branch, "" on a detached HEAD.
func (r *Runner) CurrentBranch(ctx context.Context, root string) string {
	out, err := r.Read(ctx, root, "rev-parse", "--abbrev-ref", "HEAD")
	if err != nil {
		return ""
	}
	name := strings.TrimSpace(string(out))
	if name == "HEAD" {
		return ""
	}
	return name
}

// UpstreamStatus is the tracking state of the current branch.
type UpstreamStatus struct {
	Remote string // the remote the branch tracks, "" when none
	Branch string // the upstream branch name
	Ahead  int
	Behind int
	OK     bool
}

// Upstream reads the tracking status of the current branch: no network.
func (r *Runner) Upstream(ctx context.Context, root string) (UpstreamStatus, error) {
	var st UpstreamStatus
	name, err := r.Read(ctx, root, "rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}")
	if err != nil {
		// No upstream configured: a normal state, not a failure.
		return st, nil
	}
	full := strings.TrimSpace(string(name))
	st.OK = true
	if i := strings.Index(full, "/"); i > 0 {
		st.Remote, st.Branch = full[:i], full[i+1:]
	} else {
		st.Branch = full
	}
	count, err := r.Read(ctx, root, "rev-list", "--left-right", "--count", full+"...HEAD")
	if err != nil {
		return st, nil
	}
	fields := strings.Fields(string(count))
	if len(fields) == 2 {
		st.Behind, _ = atoi(fields[0])
		st.Ahead, _ = atoi(fields[1])
	}
	return st, nil
}

// Fetch refreshes one remote (explicit user intent; never automatic).
func (r *Runner) Fetch(ctx context.Context, root, remote string) error {
	_, err := r.network(ctx, root, "fetch", remote)
	return err
}

// PullFFOnly pulls with --ff-only: Git refuses a diverged history instead of
// creating surprise merges, and the UI surfaces the refusal.
func (r *Runner) PullFFOnly(ctx context.Context, root, remote, branch string) error {
	_, err := r.network(ctx, root, "pull", "--ff-only", remote, branch)
	return err
}

// Push publishes the branch to the remote; setUpstream sets tracking the
// first time a branch is published.
func (r *Runner) Push(ctx context.Context, root, remote, branch string, setUpstream bool) error {
	args := []string{"push"}
	if setUpstream {
		args = append(args, "-u")
	}
	args = append(args, remote, branch)
	_, err := r.network(ctx, root, args...)
	return err
}

// network runs one explicit network mutation under the single-flight lock
// with the longer network deadline.
func (r *Runner) network(ctx context.Context, root string, args ...string) ([]byte, error) {
	unlock, err := r.LockRepo(root, first(args))
	if err != nil {
		return nil, err
	}
	defer unlock()

	out, err := r.run(ctx, OpMutate, NetworkTimeout, MaxReadOutput, mutateEnv(), root, args)
	if err != nil {
		if ge, ok := GitError(err); ok && ge.Class == ErrOther {
			ge.Class = classifyErr(ge.Stderr)
		}
	}
	return out, err
}

// DeleteBranch deletes a local branch with Git's safe -d: unmerged branches
// are refused, never forced away.
func (r *Runner) DeleteBranch(ctx context.Context, root, name string) error {
	if err := r.validateLocalBranch(ctx, root, name); err != nil {
		return err
	}
	_, err := r.Mutate(ctx, root, "branch", "-d", name)
	return err
}

// UndoLastCommit soft-resets a LOCAL unpushed non-root HEAD commit:
// changes remain staged. Never silently rewrite a commit referenced by a
// known remote-tracking ref; published history should use Revert instead.
func (r *Runner) UndoLastCommit(ctx context.Context, root string) error {
	if err := r.RequireCleanWorktree(ctx, root); err != nil {
		return err
	}
	parent, err := r.Read(ctx, root, "rev-parse", "--verify", "--quiet", "HEAD^")
	if err != nil || strings.TrimSpace(string(parent)) == "" {
		return &Error{Class: ErrStaleState, OpClass: OpMutate, Op: "reset",
			err: errors.New("no parent commit to restore; the initial commit cannot be undone here")}
	}
	refs, err := r.Read(ctx, root, "for-each-ref", "--format=%(refname)", "--contains", "HEAD", "refs/remotes")
	if err != nil {
		return err
	}
	if len(strings.TrimSpace(string(refs))) != 0 {
		return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "reset",
			err: errors.New("HEAD is included in a remote-tracking branch; use Revert instead of rewriting published history")}
	}
	_, err = r.Mutate(ctx, root, "reset", "--soft", "HEAD~1")
	return err
}

// Tag is one lightweight tag of the repository.
type Tag struct{ Name string }

// ListTags enumerates tags, newest creation first, bounded.
func (r *Runner) ListTags(ctx context.Context, root string) ([]Tag, error) {
	out, err := r.Read(ctx, root, "tag", "--sort=-creatordate")
	if err != nil {
		return nil, err
	}
	var tags []Tag
	for _, line := range strings.Split(strings.TrimRight(string(out), "\n"), "\n") {
		if line = strings.TrimSpace(line); line != "" {
			tags = append(tags, Tag{Name: line})
		}
		if len(tags) >= 500 {
			break
		}
	}
	return tags, nil
}

// CreateTag creates a lightweight tag at HEAD.
func (r *Runner) CreateTag(ctx context.Context, root, name string) error {
	if strings.TrimSpace(name) == "" {
		return errors.New("tag name is required")
	}
	if _, err := r.Read(ctx, root, "check-ref-format", "--allow-onelevel", name); err != nil {
		return fmt.Errorf("invalid tag name %q", name)
	}
	_, err := r.Mutate(ctx, root, "tag", name)
	return err
}

// DeleteTag removes one tag.
func (r *Runner) DeleteTag(ctx context.Context, root, name string) error {
	_, err := r.Mutate(ctx, root, "tag", "-d", name)
	return err
}

// Worktree is one checked-out work tree of the repository.
type Worktree struct {
	Path     string
	Head     string
	Branch   string // "main" for refs/heads/main; "" when bare or detached
	Bare     bool
	Detached bool
}

// ListWorktrees enumerates the repository's work trees (no network).
func (r *Runner) ListWorktrees(ctx context.Context, root string) ([]Worktree, error) {
	out, err := r.Read(ctx, root, "worktree", "list", "--porcelain")
	if err != nil {
		return nil, err
	}
	var worktrees []Worktree
	var cur *Worktree
	flush := func() {
		if cur != nil {
			worktrees = append(worktrees, *cur)
			cur = nil
		}
	}
	for _, line := range strings.Split(string(out), "\n") {
		switch {
		case strings.HasPrefix(line, "worktree "):
			flush()
			cur = &Worktree{Path: strings.TrimPrefix(line, "worktree ")}
		case cur == nil:
			continue
		case strings.HasPrefix(line, "HEAD "):
			cur.Head = strings.TrimPrefix(line, "HEAD ")
		case strings.HasPrefix(line, "branch "):
			ref := strings.TrimPrefix(line, "branch ")
			cur.Branch = strings.TrimPrefix(ref, "refs/heads/")
		case line == "bare":
			cur.Bare = true
		case line == "detached":
			cur.Detached = true
		case line == "":
			flush()
		}
	}
	flush()
	return worktrees, nil
}

// AddWorktree checks out a new work tree at path on a branch named after
// the path's folder, created from HEAD.
func (r *Runner) AddWorktree(ctx context.Context, root, path string) error {
	path = filepath.Clean(path)
	if !filepath.IsAbs(path) {
		return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "worktree",
			err: fmt.Errorf("worktree path must be absolute, got %q", path)}
	}
	folder := filepath.Base(path)
	if _, err := r.Read(ctx, root, "check-ref-format", "--allow-onelevel", folder); err != nil {
		return &Error{Class: ErrRefusal, OpClass: OpMutate, Op: "worktree",
			err: fmt.Errorf("cannot derive a branch name from %q", folder)}
	}
	_, err := r.Mutate(ctx, root, "worktree", "add", "-b", folder, path, "HEAD")
	return err
}

// CommitInfo is one entry of the commit history.
type CommitInfo struct {
	Hash    string // full hash
	Short   string
	Subject string
	Author  string
	Time    time.Time
}

// ListCommits reads the history of HEAD, newest first, bounded.
func (r *Runner) ListCommits(ctx context.Context, root string, limit int) ([]CommitInfo, error) {
	if limit <= 0 || limit > 2000 {
		limit = 200
	}
	out, err := r.Read(ctx, root,
		"log", "-n", strconv.Itoa(limit),
		"--format=%H%x09%h%x09%s%x09%an%x09%at")
	if err != nil {
		return nil, err
	}
	var commits []CommitInfo
	for _, line := range strings.Split(strings.TrimRight(string(out), "\n"), "\n") {
		if line == "" {
			continue
		}
		f := strings.SplitN(line, "\t", 5)
		if len(f) < 5 {
			continue
		}
		sec, _ := strconv.ParseInt(f[4], 10, 64)
		commits = append(commits, CommitInfo{Hash: f[0], Short: f[1], Subject: f[2], Author: f[3], Time: time.Unix(sec, 0)})
	}
	return commits, nil
}

// CommitSnapshot builds the change set of one commit: its diff against the
// first parent (or the empty tree for a root commit). Review-only: the
// staged/unstaged sides and the worktree do not apply.
func (r *Runner) CommitSnapshot(ctx context.Context, root, hash string) (*ChangesSnapshot, error) {
	if _, err := r.Read(ctx, root, "rev-parse", "--verify", "--quiet", hash+"^{commit}"); err != nil {
		return nil, &Error{Class: ErrStaleState, OpClass: OpRead, Op: "show",
			err: fmt.Errorf("%q is not a commit", hash)}
	}
	snap := &ChangesSnapshot{Root: root, Head: hash, Signature: hash, GeneratedAt: r.now()}

	// The first parent, or the empty tree for a root commit.
	parent, perr := r.Read(ctx, root, "rev-parse", "--verify", "--quiet", hash+"^")
	from := strings.TrimSpace(string(parent))
	if perr != nil || from == "" {
		from = emptyTreeID(ctx, r, root)
	}
	rangeArg := from + ".." + hash

	ns, err := r.Read(ctx, root, "diff", "--name-status", "-z", "-M", rangeArg)
	if err != nil {
		snap.Err = err.Error()
		return snap, err
	}
	entries := parseNameStatusZ(ns)

	var untracked []string // none in a commit; keeps the census below honest
	_ = untracked
	budget := &snapshotBudget{limit: MaxSnapshotBytes}
	patch, err := r.Read(ctx, root,
		"diff", "--no-color", "--no-ext-diff", "--no-textconv", "-M", "-U3",
		"--submodule=short", rangeArg)
	if err != nil {
		snap.Err = err.Error()
		return snap, err
	}
	if !budget.take(int64(len(patch))) {
		snap.Truncated = true
	}
	patches := parsePatchSet(patch)
	nsStats, _ := r.Read(ctx, root, "diff", "--numstat", "-z", "-M", rangeArg)
	numstat := parseNumstatZ(nsStats)

	files, adds, dels := emitChangeFiles(entries, patches, numstat, nil, nil)
	for i := range files {
		// Name-status entries always set the first column; a commit review
		// has no index sides.
		files[i].Staged, files[i].Unstaged = false, false
	}
	snap.Files, snap.TotalAdditions, snap.TotalDeletions = files, adds, dels
	sortFilesByPath(snap.Files)
	return snap, nil
}

// parseNameStatusZ parses `git diff --name-status -z`: a status token
// ("M", "R100", …) followed by one path, or a source and a destination for
// renames and copies.
func parseNameStatusZ(data []byte) []porcelainEntry {
	var entries []porcelainEntry
	tokens := strings.Split(string(data), "\x00")
	for i := 0; i < len(tokens); i++ {
		status := tokens[i]
		if status == "" {
			continue
		}
		letter := status[0]
		if i+1 >= len(tokens) {
			break
		}
		e := porcelainEntry{XY: string(letter) + " ", Path: tokens[i+1]}
		i++
		if letter == 'R' || letter == 'C' {
			if i+1 >= len(tokens) {
				break
			}
			e.OldPath = e.Path
			e.Path = tokens[i+1]
			i++
		}
		entries = append(entries, e)
	}
	return entries
}

// atoi parses a small non-negative count, 0 on anything else.
func atoi(s string) (int, bool) {
	n := 0
	for _, c := range s {
		if c < '0' || c > '9' {
			return 0, false
		}
		n = n*10 + int(c-'0')
		if n > 1<<30 {
			return 0, false
		}
	}
	return n, true
}
