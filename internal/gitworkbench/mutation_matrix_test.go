package gitworkbench

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// The mutation test matrix (plan "Required mutation test matrix"): every case
// runs against a real temporary Git repository — no mock-only proofs.

func TestCommitSelectedTrackedFile(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	writeRepoFile(t, root, "a.txt", "alpha\nnew line\n")
	writeRepoFile(t, root, "b.txt", "beta changed\n")

	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	pre, err := r.CapturePreflight(ctx, root, snap, []string{"a.txt"})
	if err != nil {
		t.Fatal(err)
	}

	result, err := r.Commit(ctx, root, CommitDraft{Subject: "commit a", Paths: []string{"a.txt"}}, pre, snap)
	if err != nil {
		t.Fatalf("commit: %v", err)
	}
	if result.Hash == "" {
		t.Fatal("expected commit hash")
	}

	// a.txt committed at HEAD; b.txt still dirty.
	if headFiles(t, root)["a.txt"] != true {
		t.Fatal("a.txt should be tracked")
	}
	out := runGit(t, root, "show", "--stat", "--oneline", "HEAD")
	if !strings.Contains(out, "a.txt") || strings.Contains(out, "b.txt") {
		t.Fatalf("HEAD should contain only a.txt: %s", out)
	}
	if staged, dirty := stagedPaths(t, root)["b.txt"], fileIsDirty(t, root, "b.txt"); !dirty {
		t.Fatalf("b.txt should remain a working change (staged=%v)", staged)
	}
}

func TestCommitUnrelatedStagedWorkPreserved(t *testing.T) {
	// Critical invariant (plan §22.4): unrelated already-staged work stays
	// staged and outside the Shardlane-selected commit.
	root, r := tempRepo(t)
	ctx := context.Background()
	writeRepoFile(t, root, "selected.txt", "selected new content\n")
	writeRepoFile(t, root, "unrelated.txt", "unrelated content\n")
	runGit(t, root, "add", "unrelated.txt") // staged BEFORE review

	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	pre, err := r.CapturePreflight(ctx, root, snap, []string{"selected.txt"})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := r.Commit(ctx, root, CommitDraft{Subject: "only selected", Paths: []string{"selected.txt"}}, pre, snap); err != nil {
		t.Fatalf("commit: %v", err)
	}

	// The commit contains only selected.txt.
	out := runGit(t, root, "show", "--stat", "--oneline", "HEAD")
	if strings.Contains(out, "unrelated.txt") {
		t.Fatalf("unrelated staged file must not enter the commit: %s", out)
	}
	if !strings.Contains(out, "selected.txt") {
		t.Fatalf("selected file missing from commit: %s", out)
	}
	// And unrelated.txt remains staged for the next commit.
	if !stagedPaths(t, root)["unrelated.txt"] {
		t.Fatal("unrelated.txt must remain staged after the selected commit")
	}
}

func TestCommitSelectedUntrackedFile(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	writeRepoFile(t, root, "newfile.txt", "brand new\n")

	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	cf, ok := snap.ByPath("newfile.txt")
	if !ok || !cf.Untracked || cf.Status != StatusUntracked {
		t.Fatalf("newfile.txt should be an untracked change: %+v", cf)
	}
	pre, err := r.CapturePreflight(ctx, root, snap, []string{"newfile.txt"})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := r.Commit(ctx, root, CommitDraft{Subject: "add newfile", Paths: []string{"newfile.txt"}}, pre, snap); err != nil {
		t.Fatalf("commit: %v", err)
	}
	out := runGit(t, root, "show", "--stat", "--oneline", "HEAD")
	if !strings.Contains(out, "newfile.txt") {
		t.Fatalf("untracked selected file must be committed: %s", out)
	}
}

func TestCommitUnselectedChangesRemain(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	writeRepoFile(t, root, "keep-worktree.txt", "worktree change\n")
	writeRepoFile(t, root, "commit-me.txt", "commit change\n")
	// keep-index.txt: staged-only change (worktree matches index, differs from HEAD)
	writeRepoFile(t, root, "keep-index.txt", "staged change\n")
	runGit(t, root, "add", "keep-index.txt")

	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	pre, err := r.CapturePreflight(ctx, root, snap, []string{"commit-me.txt"})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := r.Commit(ctx, root, CommitDraft{Subject: "only commit-me", Paths: []string{"commit-me.txt"}}, pre, snap); err != nil {
		t.Fatalf("commit: %v", err)
	}

	if !fileIsDirty(t, root, "keep-worktree.txt") {
		t.Fatal("unselected working change must remain")
	}
	if !stagedPaths(t, root)["keep-index.txt"] {
		t.Fatal("unrelated staged change must remain staged")
	}
}

func TestCommitHookFailureLeavesRecoverableState(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	writeRepoFile(t, root, "hooked.txt", "hook content\n")
	writeRepoFile(t, root, "other.txt", "other content\n")
	runGit(t, root, "add", "other.txt")

	// pre-commit hook that always fails.
	hook := filepath.Join(root, ".git", "hooks", "pre-commit")
	if err := os.WriteFile(hook, []byte("#!/bin/sh\necho 'hook says no' >&2\nexit 1\n"), 0o755); err != nil {
		t.Fatal(err)
	}

	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	pre, err := r.CapturePreflight(ctx, root, snap, []string{"hooked.txt"})
	if err != nil {
		t.Fatal(err)
	}
	_, err = r.Commit(ctx, root, CommitDraft{Subject: "should fail", Paths: []string{"hooked.txt"}}, pre, snap)
	if err == nil {
		t.Fatal("expected hook failure")
	}
	ge, ok := GitError(err)
	if !ok || ge.Class != ErrHookFailed {
		t.Fatalf("expected ErrHookFailed class, got %v (%v)", err, ge)
	}

	// Nothing committed; worktree change remains; unrelated staged preserved.
	out := runGit(t, root, "log", "--oneline")
	if strings.Count(out, "\n") != 1 { // only "initial"
		t.Fatalf("hook failure must not commit: %s", out)
	}
	if !fileIsDirty(t, root, "hooked.txt") {
		t.Fatal("hooked.txt change must remain after failed commit")
	}
	if !stagedPaths(t, root)["other.txt"] {
		t.Fatal("unrelated staged file must remain staged after hook failure")
	}
}

func TestCommitStaleStateFailsClosed(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	writeRepoFile(t, root, "fence.txt", "fence content\n")

	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	pre, err := r.CapturePreflight(ctx, root, snap, []string{"fence.txt"})
	if err != nil {
		t.Fatal(err)
	}

	// External mutation between review and action.
	writeRepoFile(t, root, "external.txt", "external change\n")

	_, err = r.Commit(ctx, root, CommitDraft{Subject: "stale", Paths: []string{"fence.txt"}}, pre, snap)
	if err == nil {
		t.Fatal("expected stale-state failure")
	}
	if !strings.Contains(err.Error(), "stale") {
		t.Fatalf("expected stale error, got: %v", err)
	}
	// Fail-closed: nothing committed.
	out := runGit(t, root, "log", "--oneline")
	if strings.Count(out, "\n") != 1 {
		t.Fatalf("stale fence must refuse to commit: %s", out)
	}
}

func TestCommitStaleHeadFailsClosed(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	writeRepoFile(t, root, "headfence.txt", "content\n")

	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	pre, err := r.CapturePreflight(ctx, root, snap, []string{"headfence.txt"})
	if err != nil {
		t.Fatal(err)
	}
	// External HEAD movement.
	writeRepoFile(t, root, "empty.txt", "x\n")
	runGit(t, root, "add", "empty.txt")
	runGit(t, root, "commit", "-q", "-m", "external")

	_, err = r.Commit(ctx, root, CommitDraft{Subject: "stale head", Paths: []string{"headfence.txt"}}, pre, snap)
	if err == nil || !strings.Contains(err.Error(), "stale") {
		t.Fatalf("expected stale HEAD failure, got: %v", err)
	}
}

func TestCommitChangedFingerprintFailsClosed(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	writeRepoFile(t, root, "fp.txt", "original\n")

	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	pre, err := r.CapturePreflight(ctx, root, snap, []string{"fp.txt"})
	if err != nil {
		t.Fatal(err)
	}
	// Same file count, different content: signature stays if status text is
	// identical — the per-file fingerprint must catch it. Content change
	// alters porcelain only when status letters change, so verify fence via
	// a fresh snapshot comparison at revalidate.
	writeRepoFile(t, root, "fp.txt", "mutated after review\n")
	refreshed, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	cfNew, _ := refreshed.ByPath("fp.txt")
	cfOld, _ := snap.ByPath("fp.txt")
	if cfNew.Fingerprint == cfOld.Fingerprint {
		t.Fatal("fingerprint must change when content changes")
	}
	// revalidate against the fresh snapshot must fail.
	if err := r.revalidate(ctx, root, pre, refreshed); err == nil {
		t.Fatal("revalidate must fail when the reviewed fingerprint drifted")
	}

	// Direct Commit with old preflight and old snapshot must also fail closed
	// because content identity of fp.txt drifted on disk (P0-05).
	_, err = r.Commit(ctx, root, CommitDraft{Subject: "stale content", Paths: []string{"fp.txt"}}, pre, snap)
	if err == nil || !strings.Contains(err.Error(), "stale") {
		t.Fatalf("expected stale content failure on Commit, got: %v", err)
	}
}

func TestCommitWeirdFilenames(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	names := []string{
		"with space.txt",
		"with'quote.txt",
		"with-unicode-日本語.txt",
	}
	for i, name := range names {
		writeRepoFile(t, root, name, strings.Repeat("x\n", i+1))
	}

	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	for _, name := range names {
		if _, ok := snap.ByPath(name); !ok {
			t.Fatalf("snapshot must carry %q exactly (got %d files)", name, len(snap.Files))
		}
	}
	pre, err := r.CapturePreflight(ctx, root, snap, names)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := r.Commit(ctx, root, CommitDraft{Subject: "weird names", Paths: names}, pre, snap); err != nil {
		t.Fatalf("commit with odd filenames: %v", err)
	}
	for _, name := range names {
		if !headFiles(t, root)[name] {
			t.Fatalf("%q missing from HEAD", name)
		}
	}
}

func TestCommitMergeInProgressFailsClosed(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	// Create a conflicting merge state.
	runGit(t, root, "checkout", "-q", "-b", "feature")
	writeRepoFile(t, root, "base.txt", "feature side\n")
	runGit(t, root, "commit", "-aqm", "feature change")
	runGit(t, root, "checkout", "-q", "master")
	writeRepoFile(t, root, "base.txt", "master side\n")
	runGit(t, root, "commit", "-aqm", "master change")
	if out := runGitMerge(t, root); !strings.Contains(out, "CONFLICT") && !strings.Contains(out, "Merge conflict") {
		t.Fatalf("expected merge conflict, got: %s", out)
	}

	facts, err := r.Facts(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	if !r.MergeInProgress(ctx, root) {
		t.Fatal("merge should be in progress")
	}

	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	var selected []string
	for _, cf := range snap.Files {
		selected = append(selected, cf.Path)
	}
	if len(selected) == 0 {
		t.Fatal("expected conflicted file in snapshot")
	}
	pre, err := r.CapturePreflight(ctx, root, snap, selected)
	if err != nil {
		t.Fatal(err)
	}
	_ = facts
	_, err = r.Commit(ctx, root, CommitDraft{Subject: "during merge", Paths: selected}, pre, snap)
	if err == nil {
		t.Fatal("partial commit during merge must fail closed")
	}
}

func runGitMerge(t *testing.T, root string) string {
	t.Helper()
	cmd := exec.Command("git", "merge", "feature")
	cmd.Dir = root
	out, _ := cmd.CombinedOutput()
	return string(out)
}

// fileIsDirty reports a path present in `status --porcelain` output.
func fileIsDirty(t *testing.T, root, path string) bool {
	t.Helper()
	out := runGit(t, root, "status", "--porcelain=v1", "-z", "--untracked-files=all", "--", path)
	return strings.Contains(out, path)
}

// gitInit creates an empty repo with test identity.
func gitInit(t *testing.T, root string) error {
	t.Helper()
	for _, args := range [][]string{
		{"init", "-q"},
		{"config", "user.email", "test@example.com"},
		{"config", "user.name", "Test"},
		{"config", "commit.gpgsign", "false"},
	} {
		cmd := exec.Command("git", args...)
		cmd.Dir = root
		if err := cmd.Run(); err != nil {
			return err
		}
	}
	return nil
}

func TestSnapshotRepositoryWithoutHead(t *testing.T) {
	root := t.TempDir()
	r := &Runner{}
	if err := gitInit(t, root); err != nil {
		t.Fatal(err)
	}
	writeRepoFile(t, root, "only.txt", "first content\n")
	ctx := context.Background()
	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatalf("unborn-branch snapshot: %v", err)
	}
	if !snap.NoHead {
		t.Fatal("expected NoHead for repository without commits")
	}
	cf, ok := snap.ByPath("only.txt")
	if !ok || cf.Status != StatusUntracked {
		t.Fatalf("expected untracked file in unborn repo: %+v", snap.Files)
	}
	if snap.TotalAdditions != 1 {
		t.Fatalf("expected 2 additions, got %d", snap.TotalAdditions)
	}
}

func TestSnapshotMatrix(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()

	// tracked modified
	writeRepoFile(t, root, "base.txt", "base line 1\nbase line 2 changed\n")
	// deleted
	if err := os.Remove(filepath.Join(root, "base.txt")); err == nil {
		// skip: base.txt just rewritten; restore
		writeRepoFile(t, root, "base.txt", "base line 1\nbase line 2 changed\n")
	}
	writeRepoFile(t, root, "gone.txt", "to be deleted\n")
	runGit(t, root, "add", "gone.txt")
	runGit(t, root, "commit", "-qm", "add gone")
	if err := os.Remove(filepath.Join(root, "gone.txt")); err != nil {
		t.Fatal(err)
	}
	// rename (staged working rename, not yet committed)
	writeRepoFile(t, root, "renamed.txt", "rename me content\n")
	runGit(t, root, "add", "renamed.txt")
	runGit(t, root, "commit", "-qm", "add renamed")
	runGit(t, root, "mv", "renamed.txt", "moved.txt")
	// binary
	writeRepoFile(t, root, "blob.bin", "\x00\x01\x02binary\x00")
	// untracked
	writeRepoFile(t, root, "untracked.txt", "untracked content\n")

	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}

	renamed, ok := snap.ByPath("moved.txt")
	if !ok || renamed.Status != StatusRenamed || renamed.OldPath != "renamed.txt" {
		t.Fatalf("expected renamed status for moved.txt: %+v", renamed)
	}

	deleted, ok := snap.ByPath("gone.txt")
	if !ok || deleted.Status != StatusDeleted {
		t.Fatalf("expected deleted status for gone.txt: %+v", deleted)
	}
	modified, ok := snap.ByPath("base.txt")
	if !ok || modified.Status != StatusModified || modified.Additions != 1 || modified.Deletions != 1 {
		t.Fatalf("expected modified base.txt with 1/1: %+v", modified)
	}
	untracked, ok := snap.ByPath("untracked.txt")
	if !ok || !untracked.Untracked || untracked.Additions != 1 {
		t.Fatalf("expected untracked with synthesized additions: %+v", untracked)
	}
	if len(untracked.Hunks) != 1 {
		t.Fatalf("expected synthesized hunk for untracked file")
	}
	binary, ok := snap.ByPath("blob.bin")
	if !ok || !binary.Binary {
		t.Fatalf("expected binary detection for blob.bin: %+v", binary)
	}
	if snap.TotalAdditions == 0 || snap.TotalDeletions == 0 {
		t.Fatalf("expected nonzero totals: +%d/-%d", snap.TotalAdditions, snap.TotalDeletions)
	}
	if snap.Signature == "" || snap.Head == "" {
		t.Fatal("expected signature and HEAD in snapshot")
	}
}

func TestGeneratedClassifier(t *testing.T) {
	cases := []struct {
		path   string
		expect bool
	}{
		{"go.sum", true},
		{"package-lock.json", true},
		{"web/yarn.lock", true},
		{"app/dist/bundle.js", true},
		{"api/pb/service.pb.go", true},
		{"src/main.go", false},
		{"docs/notes/dist.md", false}, // dir "dist" matches — deliberate
		{"Cargo.lock", true},
		{"internal/gitworkbench/tree.go", false},
	}
	for _, tc := range cases {
		if got := IsGenerated(tc.path); got != tc.expect {
			t.Errorf("IsGenerated(%q) = %v, want %v", tc.path, got, tc.expect)
		}
	}
}
