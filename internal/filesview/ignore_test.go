package filesview

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

// TestDefaultIgnoredPinsTheFixedSet pins the default name set: heavy and
// generated directories hide, one-file entries and IDE settings stay.
func TestDefaultIgnoredPinsTheFixedSet(t *testing.T) {
	hidden := []string{".git", ".DS_Store", "node_modules", "__pycache__", ".venv", "venv",
		"dist", "build", "target", "coverage", ".cache", ".next", ".turbo", ".mypy_cache", ".pytest_cache", ".gradle", ".nuxt"}
	for _, name := range hidden {
		if !DefaultIgnored(name) {
			t.Errorf("expected %q to be ignored by default", name)
		}
	}
	visible := []string{".env", ".github", ".vscode", ".idea", "src", "README.md", "go.mod", "node_modules.bak"}
	for _, name := range visible {
		if DefaultIgnored(name) {
			t.Errorf("expected %q to stay visible", name)
		}
	}
}

// TestIgnoreSetMergesNamesAndGitPaths covers the three hide rules: own
// name at any depth, exact git-ignored path, and any git-ignored ancestor.
func TestIgnoreSetMergesNamesAndGitPaths(t *testing.T) {
	set := NewIgnoreSet([]string{"assets/generated/", "vendor/extra", ".env.local", "./dist"})

	cases := []struct {
		name, rel string
		want      bool
	}{
		{"node_modules", "node_modules", true},          // fixed name, root level
		{"node_modules", "app/node_modules", true},      // fixed name, any depth
		{"generated", "assets/generated", true},         // git-ignored dir exact
		{"sprite.png", "assets/generated/sprite", true}, // inside git-ignored dir
		{"extra", "vendor/extra", true},                 // git-ignored file exact
		{"other.go", "vendor/other.go", false},          // sibling of an ignored file stays
		{".env.local", ".env.local", true},              // ignored file at root
		{"dist", "dist", true},                          // "./dist" cleaned
		{"main.go", "main.go", false},
		{"generated", "generated", false}, // same name elsewhere is not
	}
	for _, tc := range cases {
		if got := set.Ignored(tc.name, tc.rel); got != tc.want {
			t.Errorf("Ignored(%q, %q) = %v, want %v", tc.name, tc.rel, got, tc.want)
		}
	}
}

// TestIgnoreSetWithoutGitPathsKeepsTheFixedNames: a non-repository root
// still hides the default names.
func TestIgnoreSetWithoutGitPathsKeepsTheFixedNames(t *testing.T) {
	set := NewIgnoreSet(nil)
	if !set.Ignored("node_modules", "node_modules") {
		t.Fatal("expected node_modules ignored without git paths")
	}
	if set.Ignored("vendor", "vendor") {
		t.Fatal("expected vendor visible without git paths")
	}
}

// TestGitIgnorePathsReadsTheRepository pins the git-owned lookup over a
// real repository: ignored directories fold to one entry, ignored files
// carry their relative path, tracked files never appear, and non-ASCII
// names survive verbatim (-z: no core.quotePath C-style quoting).
func TestGitIgnorePathsReadsTheRepository(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("git not available")
	}
	root := t.TempDir()
	run := func(args ...string) {
		t.Helper()
		cmd := exec.Command("git", append([]string{"-C", root}, args...)...)
		cmd.Env = append(os.Environ(),
			"GIT_AUTHOR_NAME=t", "GIT_AUTHOR_EMAIL=t@t", "GIT_COMMITTER_NAME=t", "GIT_COMMITTER_EMAIL=t@t")
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v\n%s", args, err, out)
		}
	}
	run("init", "-q")
	if err := os.WriteFile(filepath.Join(root, ".gitignore"), []byte("node_modules/\n*.log\n.secret\n文档/\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "main.go"), []byte("package main\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(root, "node_modules", "left-pad"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "node_modules", "left-pad", "index.js"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "debug.log"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, ".secret"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(root, "文档", "v1"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "文档", "v1", "a.md"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	run("add", ".")
	run("commit", "-qm", "init")

	paths, err := GitIgnorePaths(context.Background(), root)
	if err != nil {
		t.Fatal(err)
	}
	got := map[string]bool{}
	for _, p := range paths {
		got[p] = true
	}
	for _, want := range []string{"node_modules", "debug.log", ".secret", "文档"} {
		if !got[want] {
			t.Errorf("expected ignored path %q, got %v", want, paths)
		}
	}
	if got["main.go"] || got[".gitignore"] {
		t.Errorf("tracked files must not be listed as ignored: %v", paths)
	}

	set := NewIgnoreSet(paths)
	if !set.Ignored("index.js", "node_modules/left-pad/index.js") {
		t.Error("entries inside a folded ignored directory must hide")
	}
	if !set.Ignored("a.md", "文档/v1/a.md") {
		t.Error("non-ASCII ignored directory must hide its entries (quotePath)")
	}
}

// TestGitIgnorePathsFailsClosedOutsideARepository: no repository, empty
// set, an error — the caller falls back to the fixed names.
func TestGitIgnorePathsFailsClosedOutsideARepository(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("git not available")
	}
	paths, err := GitIgnorePaths(context.Background(), t.TempDir())
	if err == nil {
		t.Skip("git treats a plain directory as a repository on this platform")
	}
	if len(paths) != 0 {
		t.Errorf("expected no paths outside a repository, got %v", paths)
	}
}
