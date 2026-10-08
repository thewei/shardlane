package nativeui

import (
	"os"
	"os/exec"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

// tempGitRepo creates a real Git repository with one commit.
func tempGitRepo(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	run := func(args ...string) {
		cmd := exec.Command("git", args...)
		cmd.Dir = dir
		cmd.Env = append(os.Environ(),
			"GIT_AUTHOR_NAME=test", "GIT_AUTHOR_EMAIL=test@example.com",
			"GIT_COMMITTER_NAME=test", "GIT_COMMITTER_EMAIL=test@example.com")
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v: %s", args, err, out)
		}
	}
	run("init", "-q")
	if err := os.WriteFile(filepath.Join(dir, "README.md"), []byte("demo\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	run("add", "README.md")
	run("commit", "-q", "-m", "init")
	return dir
}

// TestGitContextDiscoversRepositoryWithoutSeeding pins P0-03 on the
// production path: a shell that has never seen the cwd resolves the repo
// root in the background, then loads the snapshot — with no test-side Git
// seeding. The uiApplyOverride observes the real apply lane headlessly.
func TestGitContextDiscoversRepositoryWithoutSeeding(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("git unavailable")
	}
	root := tempGitRepo(t)
	// Git reports the symlink-resolved toplevel (/private/var/... on macOS).
	if resolved, err := filepath.EvalSymlinks(root); err == nil {
		root = resolved
	}

	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: root}},
		Tabs:     []herdr.Tab{{ID: "t1", Label: "main", ProjectID: "w1"}},
		Panes:    []herdr.Pane{{ID: "p1", TerminalID: "term-1", Label: "editor", TabID: "t1", CWD: root}},
	}

	var mu sync.Mutex
	shell.uiApplyOverride = func(fn func()) {
		mu.Lock()
		fn()
		mu.Unlock()
	}

	// Production selection path: syncWorkspaceForSelection is the caller
	// that binds the Git context. No manual git.root / snapshot seeding.
	shell.reconcileLocalSelection(shell.projection)
	shell.syncWorkspaceForSelection()

	deadline := time.Now().Add(15 * time.Second)
	for {
		mu.Lock()
		resolved := shell.git.root == root &&
			shell.git.snapshot != nil && shell.git.snapshot.Root == root &&
			len(shell.git.snapshot.Files) == 0
		mu.Unlock()
		if resolved {
			return
		}
		if time.Now().After(deadline) {
			mu.Lock()
			t.Fatalf("git context did not resolve: root=%q snapshot=%v",
				shell.git.root, shell.git.snapshot)
		}
		time.Sleep(20 * time.Millisecond)
	}
}

// TestGitContextNegativeCacheStaysQuiet pins the negative side of P0-03: a
// non-repository cwd is resolved once, recorded as confirmed-not-a-repo, and
// repeated context syncs never relaunch the resolver.
func TestGitContextNegativeCacheStaysQuiet(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("git unavailable")
	}
	dir := t.TempDir() // not a repository

	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: dir}},
		Tabs:     []herdr.Tab{{ID: "t1", Label: "main", ProjectID: "w1"}},
		Panes:    []herdr.Pane{{ID: "p1", TerminalID: "term-1", Label: "editor", TabID: "t1", CWD: dir}},
	}

	var mu sync.Mutex
	shell.uiApplyOverride = func(fn func()) {
		mu.Lock()
		fn()
		mu.Unlock()
	}

	shell.reconcileLocalSelection(shell.projection)
	shell.syncWorkspaceForSelection()

	deadline := time.Now().Add(15 * time.Second)
	for {
		mu.Lock()
		negativelyCached, ok := shell.git.rootOf[dir]
		mu.Unlock()
		if ok && negativelyCached == "" {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("non-repo cwd was never negative-cached")
		}
		time.Sleep(20 * time.Millisecond)
	}

	// Repeated syncs must stay quiet (no busy flag, no root).
	for i := 0; i < 3; i++ {
		mu.Lock()
		shell.syncGitContext()
		busy, got := shell.git.rootBusy, shell.git.root
		mu.Unlock()
		if busy || got != "" {
			t.Fatalf("sync %d: busy=%v root=%q, want quiet", i, busy, got)
		}
	}
}
