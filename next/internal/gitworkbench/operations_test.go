package gitworkbench

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func opCtx(t *testing.T) (context.Context, context.CancelFunc) {
	t.Helper()
	return context.WithTimeout(context.Background(), 30*time.Second)
}

func TestStageAndUnstage(t *testing.T) {
	root, r := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()

	writeRepoFile(t, root, "base.txt", "changed\n")
	if err := r.Stage(ctx, root, []string{"base.txt"}); err != nil {
		t.Fatalf("stage: %v", err)
	}
	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	if len(snap.Files) != 1 || snap.Files[0].Status != StatusModified {
		t.Fatalf("staged file must still list as modified, got %+v", snap.Files)
	}

	if err := r.Unstage(ctx, root, snap.NoHead, []string{"base.txt"}); err != nil {
		t.Fatalf("unstage: %v", err)
	}
	out := runGit(t, root, "status", "--porcelain")
	if !strings.Contains(out, " M base.txt") {
		t.Fatalf("change must be back in the worktree, got %q", out)
	}
}

func TestUnstageOnUnbornBranch(t *testing.T) {
	root, r := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()
	// An orphan branch: staged adds with no HEAD to restore from.
	mustRunGit(t, root, "checkout", "--orphan", "fresh")
	writeRepoFile(t, root, "new.txt", "added\n")
	if err := r.Stage(ctx, root, []string{"new.txt"}); err != nil {
		t.Fatal(err)
	}
	if err := r.Unstage(ctx, root, true, []string{"new.txt"}); err != nil {
		t.Fatalf("unstage (unborn): %v", err)
	}
	out := runGit(t, root, "status", "--porcelain")
	if !strings.Contains(out, "?? new.txt") {
		t.Fatalf("file must be untracked again, got %q", out)
	}
}

func TestDiscardAndDeleteUntracked(t *testing.T) {
	root, r := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()

	writeRepoFile(t, root, "base.txt", "wrecked\n")
	if err := r.Discard(ctx, root, []string{"base.txt"}); err != nil {
		t.Fatalf("discard: %v", err)
	}
	if got := readRepoFile(t, root, "base.txt"); got != "base line 1\nbase line 2\n" {
		t.Fatalf("discard must restore HEAD content, got %q", got)
	}

	writeRepoFile(t, root, "untracked.txt", "temp\n")
	if err := r.DeleteUntracked(root, []string{"untracked.txt"}); err != nil {
		t.Fatalf("delete untracked: %v", err)
	}
	if _, err := os.Stat(filepath.Join(root, "untracked.txt")); !os.IsNotExist(err) {
		t.Fatal("untracked file must be gone")
	}
	if err := r.DeleteUntracked(root, []string{"../../escape.txt"}); err == nil {
		t.Fatal("absolute traversal paths must be refused")
	}
}

func TestStashPushListPop(t *testing.T) {
	root, r := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()

	writeRepoFile(t, root, "base.txt", "stashed\n")
	writeRepoFile(t, root, "extra.txt", "untracked\n")
	if err := r.StashPush(ctx, root, "wip review"); err != nil {
		t.Fatalf("stash push: %v", err)
	}
	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	if len(snap.Files) != 0 {
		t.Fatalf("stash must clean the tree, got %d files", len(snap.Files))
	}
	stashes, err := r.StashList(ctx, root)
	if err != nil || len(stashes) != 1 {
		t.Fatalf("stash list = %v, %v", stashes, err)
	}
	if !validStashRef(stashes[0].Ref) {
		t.Fatalf("unexpected stash ref %q", stashes[0].Ref)
	}
	if err := r.StashPop(ctx, root, stashes[0].Ref); err != nil {
		t.Fatalf("stash pop: %v", err)
	}
	if got := readRepoFile(t, root, "base.txt"); got != "stashed\n" {
		t.Fatalf("pop must restore the change, got %q", got)
	}
	if stashes, _ := r.StashList(ctx, root); len(stashes) != 0 {
		t.Fatalf("pop must drop the entry, got %d", len(stashes))
	}
	if err := r.StashPop(ctx, root, "stash@{x}; rm -rf /"); err == nil {
		t.Fatal("non-numeric stash refs must be refused")
	}
}

func TestDeleteBranchAndUndoLastCommit(t *testing.T) {
	root, r := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()

	mustRunGit(t, root, "branch", "spare")
	if err := r.DeleteBranch(ctx, root, "spare"); err != nil {
		t.Fatalf("delete branch: %v", err)
	}
	if err := r.DeleteBranch(ctx, root, "no-such-branch"); err == nil {
		t.Fatal("deleting an unknown branch must fail")
	}
	if err := r.DeleteBranch(ctx, root, "main"); err == nil {
		t.Fatal("deleting the current branch must be refused by git -d")
	}

	writeRepoFile(t, root, "base.txt", "committed too soon\n")
	mustRunGit(t, root, "commit", "-aqm", "too soon")
	if err := r.UndoLastCommit(ctx, root); err != nil {
		t.Fatalf("undo last commit: %v", err)
	}
	out := runGit(t, root, "status", "--porcelain")
	if !strings.Contains(out, "M  base.txt") && !strings.Contains(out, "AM base.txt") {
		t.Fatalf("soft undo must leave the change staged, got %q", out)
	}
	head := strings.TrimSpace(runGit(t, root, "log", "--oneline"))
	if !strings.Contains(head, "initial") {
		t.Fatalf("HEAD must be back at initial, got %q", head)
	}
}

func TestUpstreamStatusAndPushPull(t *testing.T) {
	// origin: the "remote" is a local bare clone, so fetch/pull/push run
	// without any network. The branch name follows whatever git init chose.
	origin := t.TempDir()
	mustRunGitAt(t, origin, "init", "-q", "--bare")
	root, r := tempRepo(t)
	mustRunGit(t, root, "remote", "add", "origin", origin)
	ctx, cancel := opCtx(t)
	defer cancel()
	branch := r.CurrentBranch(ctx, root)
	if branch == "" {
		t.Fatal("the test repository must start on a branch")
	}
	mustRunGit(t, root, "push", "-q", "-u", "origin", branch)

	if got := r.DefaultRemote(ctx, root); got != "origin" {
		t.Fatalf("default remote = %q", got)
	}
	st, err := r.Upstream(ctx, root)
	if err != nil || !st.OK || st.Remote != "origin" || st.Ahead != 0 || st.Behind != 0 {
		t.Fatalf("upstream = %+v, %v", st, err)
	}

	// Publish a commit, then fetch and pull from a clone that lacks it.
	writeRepoFile(t, root, "base.txt", "published\n")
	mustRunGit(t, root, "commit", "-aqm", "publish")
	if err := r.Push(ctx, root, "origin", branch, false); err != nil {
		t.Fatalf("push: %v", err)
	}
	st, _ = r.Upstream(ctx, root)
	if st.Ahead != 0 {
		t.Fatalf("after push the branch must not be ahead, got %+v", st)
	}

	clone := t.TempDir()
	mustRunGitAt(t, clone, "clone", "-q", origin, ".")
	writeRepoFile(t, clone, "theirs.txt", "diverged\n")
	mustRunGitAt(t, clone, "add", ".")
	mustRunGitAt(t, clone, "commit", "-qm", "diverge")
	mustRunGitAt(t, clone, "push", "-q", "origin", "HEAD:"+branch)

	if err := r.Fetch(ctx, root, "origin"); err != nil {
		t.Fatalf("fetch: %v", err)
	}
	st, _ = r.Upstream(ctx, root)
	if st.Behind != 1 {
		t.Fatalf("after the remote moved the branch must be behind 1, got %+v", st)
	}
	if err := r.PullFFOnly(ctx, root, "origin", branch); err != nil {
		t.Fatalf("pull --ff-only: %v", err)
	}
	if got := readRepoFile(t, root, "theirs.txt"); got != "diverged\n" {
		t.Fatalf("pull must bring the remote commit, got %q", got)
	}
}

func TestSnapshotCarriesSideHunks(t *testing.T) {
	root, r := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()

	// Change the file, stage it, then change it again: the snapshot must
	// separate the staged edit from the later worktree edit.
	writeRepoFile(t, root, "base.txt", "staged edit\n")
	mustRunGit(t, root, "add", "base.txt")
	writeRepoFile(t, root, "base.txt", "staged edit\nworktree edit\n")

	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	if len(snap.Files) != 1 || !snap.Files[0].Staged || !snap.Files[0].Unstaged {
		t.Fatalf("file must be partially staged, got %+v", snap.Files)
	}
	cf := snap.Files[0]
	if n := countLines(cf.StagedHunks, KindAdd); n != 1 || !hunksHaveText(cf.StagedHunks, "staged edit") {
		t.Fatalf("staged hunks must carry only the staged edit, got %+v", cf.StagedHunks)
	}
	if n := countLines(cf.UnstagedHunks, KindAdd); n != 1 || !hunksHaveText(cf.UnstagedHunks, "worktree edit") {
		t.Fatalf("unstaged hunks must carry only the worktree edit, got %+v", cf.UnstagedHunks)
	}
	if !hunksHaveText(cf.Hunks, "staged edit") || !hunksHaveText(cf.Hunks, "worktree edit") {
		t.Fatal("combined hunks must carry both edits")
	}
}

func countLines(hunks []Hunk, kind LineKind) int {
	n := 0
	for _, h := range hunks {
		for _, l := range h.Lines {
			if l.Kind == kind {
				n++
			}
		}
	}
	return n
}

func hunksHaveText(hunks []Hunk, want string) bool {
	for _, h := range hunks {
		for _, l := range h.Lines {
			if strings.Contains(l.Text, want) {
				return true
			}
		}
	}
	return false
}

func TestTagsAndWorktrees(t *testing.T) {
	root, r := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()

	if tags, _ := r.ListTags(ctx, root); len(tags) != 0 {
		t.Fatalf("fresh repo must have no tags, got %v", tags)
	}
	if err := r.CreateTag(ctx, root, "v1.0"); err != nil {
		t.Fatalf("create tag: %v", err)
	}
	if err := r.CreateTag(ctx, root, "bad..name"); err == nil {
		t.Fatal("invalid tag names must be refused")
	}
	tags, err := r.ListTags(ctx, root)
	if err != nil || len(tags) != 1 || tags[0].Name != "v1.0" {
		t.Fatalf("tags = %v, %v", tags, err)
	}

	wtPath := filepath.Join(filepath.Dir(root), filepath.Base(root)+"-wt")
	if err := r.AddWorktree(ctx, root, wtPath); err != nil {
		t.Fatalf("add worktree: %v", err)
	}
	worktrees, err := r.ListWorktrees(ctx, root)
	if err != nil || len(worktrees) != 2 {
		t.Fatalf("worktrees = %+v, %v", worktrees, err)
	}
	linked := worktrees[1]
	// macOS reports /private/var for /var paths; compare canonical forms.
	gotPath, _ := filepath.EvalSymlinks(linked.Path)
	wantPath, _ := filepath.EvalSymlinks(wtPath)
	if gotPath != wantPath || linked.Branch != filepath.Base(wtPath) || linked.Detached {
		t.Fatalf("linked worktree = %+v", linked)
	}
	if worktrees[0].Bare {
		t.Fatal("the main worktree must not be bare")
	}

	if err := r.DeleteTag(ctx, root, "v1.0"); err != nil {
		t.Fatalf("delete tag: %v", err)
	}
	if tags, _ := r.ListTags(ctx, root); len(tags) != 0 {
		t.Fatal("tag must be gone")
	}
}

func mustRunGit(t *testing.T, root string, args ...string) {
	t.Helper()
	runGit(t, root, args...)
}

func mustRunGitAt(t *testing.T, dir string, args ...string) {
	t.Helper()
	if out, err := runGitAt(dir, args...); err != nil {
		t.Fatalf("git %s: %v: %s", strings.Join(args, " "), err, out)
	}
}

// runGitAt runs git in any directory (used for bare remotes and clones),
// returning the output for assertions to inspect.
func runGitAt(dir string, args ...string) (string, error) {
	full := append([]string{"-c", "core.quotepath=off"}, args...)
	cmd := exec.Command("git", full...)
	cmd.Dir = dir
	cmd.Env = append(os.Environ(),
		"GIT_AUTHOR_NAME=Test", "GIT_AUTHOR_EMAIL=test@example.com",
		"GIT_COMMITTER_NAME=Test", "GIT_COMMITTER_EMAIL=test@example.com",
		"GIT_PAGER=cat", "LC_ALL=C")
	out, err := cmd.CombinedOutput()
	return string(out), err
}

// readRepoFile reads a file relative to the repo root.
func readRepoFile(t *testing.T, root, rel string) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(root, rel))
	if err != nil {
		t.Fatal(err)
	}
	return string(data)
}

func TestListCommitsAndCommitSnapshot(t *testing.T) {
	root, r := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()

	writeRepoFile(t, root, "base.txt", "second\n")
	writeRepoFile(t, root, "new.txt", "added\n")
	mustRunGit(t, root, "add", "-A")
	mustRunGit(t, root, "commit", "-qm", "second commit")

	commits, err := r.ListCommits(ctx, root, 10)
	if err != nil || len(commits) != 2 {
		t.Fatalf("commits = %v, %v", commits, err)
	}
	if commits[0].Subject != "second commit" || commits[1].Subject != "initial" {
		t.Fatalf("history order wrong: %+v", commits)
	}

	snap, err := r.CommitSnapshot(ctx, root, commits[0].Hash)
	if err != nil {
		t.Fatalf("commit snapshot: %v", err)
	}
	if len(snap.Files) != 2 {
		t.Fatalf("commit must touch 2 files, got %+v", snap.Files)
	}
	if !hunksHaveText(snap.Files[0].Hunks, "second") && !hunksHaveText(snap.Files[1].Hunks, "second") {
		t.Fatalf("commit hunks missing the edit: %+v", snap.Files)
	}
	if snap.Files[0].Staged || snap.Files[0].Unstaged {
		t.Fatal("commit snapshots carry no staged/unstaged sides")
	}

	// The root commit diffs against the empty tree.
	rootSnap, err := r.CommitSnapshot(ctx, root, commits[1].Hash)
	if err != nil {
		t.Fatalf("root commit snapshot: %v", err)
	}
	if len(rootSnap.Files) != 1 || rootSnap.Files[0].Status != StatusAdded {
		t.Fatalf("root commit must add base.txt, got %+v", rootSnap.Files)
	}

	if _, err := r.CommitSnapshot(ctx, root, "deadbeef"); err == nil {
		t.Fatal("unknown hashes must be refused")
	}
}
