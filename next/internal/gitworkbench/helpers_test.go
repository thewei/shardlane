package gitworkbench

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// tempRepo builds a real temporary Git repository with one initial commit.
func tempRepo(t *testing.T) (root string, r *Runner) {
	t.Helper()
	root = t.TempDir()
	r = &Runner{}

	mustInit := func(args ...string) {
		t.Helper()
		cmd := exec.Command("git", args...)
		cmd.Dir = root
		cmd.Env = append(os.Environ(),
			"GIT_AUTHOR_NAME=Test", "GIT_AUTHOR_EMAIL=test@example.com",
			"GIT_COMMITTER_NAME=Test", "GIT_COMMITTER_EMAIL=test@example.com",
			"GIT_PAGER=cat", "LC_ALL=C")
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %s: %v: %s", strings.Join(args, " "), err, out)
		}
	}

	mustInit("init", "-q")
	mustInit("config", "user.email", "test@example.com")
	mustInit("config", "user.name", "Test")
	mustInit("config", "commit.gpgsign", "false")

	if err := os.WriteFile(filepath.Join(root, "base.txt"), []byte("base line 1\nbase line 2\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	mustInit("add", ".")
	mustInit("commit", "-q", "-m", "initial")
	return root, r
}

// writeRepoFile writes a file relative to the repo root.
func writeRepoFile(t *testing.T, root, rel, content string) {
	t.Helper()
	path := filepath.Join(root, rel)
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}
}

func runGit(t *testing.T, root string, args ...string) string {
	t.Helper()
	// core.quotepath=off keeps non-ASCII paths literal in machine output.
	full := append([]string{"-c", "core.quotepath=off"}, args...)
	cmd := exec.Command("git", full...)
	cmd.Dir = root
	cmd.Env = append(os.Environ(),
		"GIT_AUTHOR_NAME=Test", "GIT_AUTHOR_EMAIL=test@example.com",
		"GIT_COMMITTER_NAME=Test", "GIT_COMMITTER_EMAIL=test@example.com",
		"GIT_PAGER=cat", "LC_ALL=C")
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("git %s: %v: %s", strings.Join(args, " "), err, out)
	}
	return string(out)
}

// stagedPaths returns currently staged paths via the real index.
func stagedPaths(t *testing.T, root string) map[string]bool {
	t.Helper()
	out := runGit(t, root, "diff", "--cached", "--name-only")
	result := map[string]bool{}
	for _, line := range strings.Split(strings.TrimSpace(out), "\n") {
		if line != "" {
			result[line] = true
		}
	}
	return result
}

// headFiles lists files tracked at HEAD.
func headFiles(t *testing.T, root string) map[string]bool {
	t.Helper()
	out := runGit(t, root, "ls-files")
	result := map[string]bool{}
	for _, line := range strings.Split(strings.TrimSpace(out), "\n") {
		if line != "" {
			result[line] = true
		}
	}
	return result
}
