package gitworkbench

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

func TestListBranchesMarksCurrent(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	runGit(t, root, "branch", "feature-a")
	runGit(t, root, "branch", "feature-b")

	branches, err := r.ListBranches(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	if len(branches) != 3 {
		t.Fatalf("branches = %v", branches)
	}
	current := ""
	for _, b := range branches {
		if b.Current {
			current = b.Name
		}
	}
	if current == "" {
		t.Fatal("no current branch marked")
	}
	names := map[string]bool{}
	for _, b := range branches {
		names[b.Name] = true
	}
	if !names["feature-a"] || !names["feature-b"] {
		t.Fatalf("missing branches: %v", branches)
	}
}

func TestSwitchBranchClean(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	writeRepoFile(t, root, "branchfile.txt", "content on main\n")
	runGit(t, root, "add", ".")
	runGit(t, root, "commit", "-qm", "seed")
	runGit(t, root, "branch", "topic")

	if _, err := r.SwitchBranch(ctx, root, "topic"); err != nil {
		t.Fatalf("switch: %v", err)
	}
	out := runGit(t, root, "branch", "--show-current")
	if strings.TrimSpace(out) != "topic" {
		t.Fatalf("current branch = %q", out)
	}
}

func TestSwitchBranchDirtyCompatibleCarriesChanges(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	runGit(t, root, "branch", "topic")
	writeRepoFile(t, root, "carried.txt", "carried work\n")

	if _, err := r.SwitchBranch(ctx, root, "topic"); err != nil {
		t.Fatalf("dirty compatible switch should succeed: %v", err)
	}
	if !fileIsDirty(t, root, "carried.txt") {
		t.Fatal("carried change must survive the switch")
	}
}

func TestSwitchBranchConflictRefused(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	// main: base.txt v1; other: base.txt v2
	writeRepoFile(t, root, "base.txt", "version one\n")
	runGit(t, root, "commit", "-aqm", "v1")
	runGit(t, root, "branch", "other")
	runGit(t, root, "checkout", "-q", "other")
	writeRepoFile(t, root, "base.txt", "version two\n")
	runGit(t, root, "commit", "-aqm", "v2")
	runGit(t, root, "checkout", "-q", "master")

	// Dirty change conflicts with other's base.txt.
	writeRepoFile(t, root, "base.txt", "local uncommitted edit\n")

	_, err := r.SwitchBranch(ctx, root, "other")
	if err == nil {
		t.Fatal("conflicting switch must be refused")
	}
	// Git's refusal respected: still on master.
	out := runGit(t, root, "branch", "--show-current")
	if strings.TrimSpace(out) == "other" {
		t.Fatal("switch must not have happened")
	}
}

func TestSwitchBranchNotLocal(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	if _, err := r.SwitchBranch(ctx, root, "no-such-branch"); err == nil {
		t.Fatal("switch to nonexistent branch must fail")
	}
	if _, err := r.SwitchBranch(ctx, root, "../../../etc"); err == nil {
		t.Fatal("path-like name must be refused")
	}
}

func TestCreateBranch(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	if _, err := r.CreateBranch(ctx, root, "fresh-branch"); err != nil {
		t.Fatalf("create: %v", err)
	}
	out := runGit(t, root, "branch", "--show-current")
	if strings.TrimSpace(out) != "fresh-branch" {
		t.Fatalf("current = %q", out)
	}
	if _, err := r.CreateBranch(ctx, root, "fresh-branch"); err == nil {
		t.Fatal("duplicate creation must fail")
	}
	if _, err := r.CreateBranch(ctx, root, "bad name with spaces"); err == nil {
		t.Fatal("invalid name must be refused by check-ref-format")
	}
}

func TestRunnerReadPolicy(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	if _, err := r.Read(ctx, root, "status"); err != nil {
		t.Fatal(err)
	}

	// Not a repo classification.
	if _, err := r.Read(ctx, t.TempDir(), "status"); err == nil {
		t.Fatal("expected not-a-repo error")
	} else if ge, ok := GitError(err); !ok || ge.Class != ErrNotARepo {
		t.Fatalf("expected ErrNotARepo, got %v", err)
	}
}

func TestRunnerMutationSingleFlight(t *testing.T) {
	root := t.TempDir()
	if err := gitInit(t, root); err != nil {
		t.Fatal(err)
	}
	// Fake git binary that sleeps 300ms and exits 0.
	dir := t.TempDir()
	script := filepath.Join(dir, "slowgit.sh")
	if err := os.WriteFile(script, []byte("#!/bin/sh\nsleep 0.3\nexit 0\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	r := &Runner{GitBinary: script}

	var wg sync.WaitGroup
	errs := make([]error, 2)
	for i := 0; i < 2; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			_, errs[idx] = r.Mutate(context.Background(), root, "status")
		}(i)
	}
	wg.Wait()
	failed := 0
	for _, err := range errs {
		if err != nil {
			if !strings.Contains(err.Error(), "in flight") {
				t.Fatalf("unexpected error: %v", err)
			}
			failed++
		}
	}
	if failed != 1 {
		t.Fatalf("expected exactly one in-flight refusal, got %d (%v)", failed, errs)
	}
}

func TestRunnerReadTimeout(t *testing.T) {
	dir := t.TempDir()
	script := filepath.Join(dir, "slowgit.sh")
	if err := os.WriteFile(script, []byte("#!/bin/sh\nsleep 60\nexit 0\n"), 0o755); err != nil {
		t.Fatal(err)
	}
	r := &Runner{GitBinary: script}
	start := time.Now()
	if _, err := r.Read(context.Background(), t.TempDir(), "status"); err == nil {
		t.Fatal("expected timeout")
	}
	if elapsed := time.Since(start); elapsed > 7*time.Second {
		t.Fatalf("read timeout not enforced near 5s bound: %v", elapsed)
	}
}

func TestFactsSignatureChangesWithWorktree(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	before, err := r.Facts(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	writeRepoFile(t, root, "new.txt", "change\n")
	after, err := r.Facts(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	if before.Signature == after.Signature {
		t.Fatal("signature must change when worktree changes")
	}
	if before.Head != after.Head {
		t.Fatal("HEAD must be stable without commits")
	}
}

func TestCacheStaleWhileRefresh(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	cache := NewCache(r)

	var fresh bool
	if _, fresh = cache.Get(root); fresh {
		t.Fatal("empty cache must miss")
	}
	snap1, err := cache.Refresh(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	_, fresh = cache.Get(root)
	if !fresh {
		t.Fatal("fresh refresh must be fresh")
	}

	// Expire the entry via clock override.
	r.Now = func() time.Time { return time.Now().Add(2 * StaleAfter) }
	_, fresh = cache.Get(root)
	if fresh {
		t.Fatal("expired entry must be stale")
	}
	if !cache.NeedsRefresh(root) {
		t.Fatal("stale entry needs refresh")
	}

	// Concurrent refreshes share one call.
	var wg sync.WaitGroup
	snaps := make([]*ChangesSnapshot, 2)
	for i := 0; i < 2; i++ {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			s, _ := cache.Refresh(ctx, root)
			snaps[idx] = s
		}(i)
	}
	wg.Wait()
	if snaps[0] == nil || snaps[1] == nil || snaps[0] != snaps[1] {
		t.Fatalf("concurrent refresh must share one snapshot result")
	}
	r.Now = nil
	_ = snap1
}
