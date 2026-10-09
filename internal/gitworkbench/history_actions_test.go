package gitworkbench

import (
	"strings"
	"testing"
)

/**
 * [INPUT]: 依赖 scratch Git repositories 的真实 branch/merge/revert 状态
 * [OUTPUT]: 覆盖 FF-only 成功/分歧拒绝/脏工作树拒绝/精确 SHA revert
 * [POS]: Git history-actions integration tests, never touch the user's checkout
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestMergeFastForwardAndRejectDivergedHistory(t *testing.T) {
	root, runner := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()
	original := runner.CurrentBranch(ctx, root)
	mustRunGit(t, root, "switch", "-q", "-c", "feature")
	writeRepoFile(t, root, "feature.txt", "new feature\n")
	mustRunGit(t, root, "add", "feature.txt")
	mustRunGit(t, root, "commit", "-q", "-m", "feature")
	head := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
	mustRunGit(t, root, "switch", "-q", original)
	if err := runner.MergeFastForward(ctx, root, "feature"); err != nil {
		t.Fatalf("fast forward merge: %v", err)
	}
	if got := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD")); got != head {
		t.Fatalf("fast forward HEAD = %s, want %s", got, head)
	}
	if err := runner.MergeFastForward(ctx, root, original); err == nil {
		t.Fatal("merging current branch must be refused")
	}
	if err := runner.MergeFastForward(ctx, root, "does-not-exist"); err == nil {
		t.Fatal("unknown branch must be refused")
	}
	mustRunGit(t, root, "switch", "-q", "feature")
	writeRepoFile(t, root, "feature.txt", "another feature\n")
	mustRunGit(t, root, "add", "feature.txt")
	mustRunGit(t, root, "commit", "-q", "-m", "feature two")
	mustRunGit(t, root, "switch", "-q", original)
	writeRepoFile(t, root, "main.txt", "another commit\n")
	mustRunGit(t, root, "add", "main.txt")
	mustRunGit(t, root, "commit", "-q", "-m", "main two")
	before := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
	if err := runner.MergeFastForward(ctx, root, "feature"); err == nil {
		t.Fatal("divergent history must not create an implicit merge commit")
	}
	if got := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD")); got != before {
		t.Fatal("divergent merge unexpectedly moved HEAD")
	}
}

func TestHistoryMutationRefusesDirtyWorktree(t *testing.T) {
	root, runner := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()
	mustRunGit(t, root, "branch", "topic")
	writeRepoFile(t, root, "unsaved.txt", "never erase\n")
	if err := runner.MergeFastForward(ctx, root, "topic"); err == nil {
		t.Fatal("untracked content must block merge")
	}
	hash := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
	if err := runner.RevertCommit(ctx, root, hash); err == nil {
		t.Fatal("untracked content must block revert")
	}
	if got := readRepoFile(t, root, "unsaved.txt"); got != "never erase\n" {
		t.Fatal("dirty rejection must not touch the user's file")
	}
}

func TestUndoLastCommitRefusesPublishedAndRootCommits(t *testing.T) {
	root, runner := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()
	if err := runner.UndoLastCommit(ctx, root); err == nil {
		t.Fatal("root commit cannot be soft-reset to a non-existing parent")
	}
	writeRepoFile(t, root, "new.txt", "to publish\n")
	mustRunGit(t, root, "add", "new.txt")
	mustRunGit(t, root, "commit", "-q", "-m", "publish")
	origin := t.TempDir()
	mustRunGitAt(t, origin, "init", "-q", "--bare")
	mustRunGit(t, root, "remote", "add", "origin", origin)
	branch := runner.CurrentBranch(ctx, root)
	mustRunGit(t, root, "push", "-q", "-u", "origin", branch)
	before := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
	if err := runner.UndoLastCommit(ctx, root); err == nil || !strings.Contains(err.Error(), "Revert") {
		t.Fatalf("published commit undo should recommend Revert: %v", err)
	}
	if got := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD")); got != before {
		t.Fatal("refusing a published undo must not move HEAD")
	}
}

func TestRevertSelectedCommitAndValidateFullHash(t *testing.T) {
	root, runner := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()
	writeRepoFile(t, root, "feature.txt", "first\n")
	mustRunGit(t, root, "add", "feature.txt")
	mustRunGit(t, root, "commit", "-q", "-m", "introduce feature")
	hash := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
	if err := runner.RevertCommit(ctx, root, hash); err != nil {
		t.Fatalf("revert: %v", err)
	}
	if got := strings.TrimSpace(runGit(t, root, "show", "-s", "--format=%s", "HEAD")); got != "Revert \"introduce feature\"" {
		t.Fatalf("reverted subject = %q", got)
	}
	if err := runner.RevertCommit(ctx, root, "--hard"); err == nil {
		t.Fatal("invalid hash must be refused")
	}
	if err := runner.RevertCommit(ctx, root, "abc123"); err == nil {
		t.Fatal("abbreviated hash must be refused")
	}
}
