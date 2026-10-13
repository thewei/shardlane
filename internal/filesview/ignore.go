// ignore.go owns the Files tree's ignore semantics (FILES-01): a fixed set
// of heavy/generated directory names that every root hides by default, plus
// the repository's own ignored paths fetched from git itself — git owns
// ignore semantics (nested .gitignore, negation), so the tree never parses
// ignore files. The lookup is one bounded, directory-folded git call.

package filesview

import (
	"bytes"
	"context"
	"os/exec"
	"path"
	"path/filepath"
	"strings"
	"time"
)

// IgnoreDeadline bounds the git ignore lookup; gitintel uses the same
// discipline. A non-repository root fails fast with an empty set.
const IgnoreDeadline = 5 * time.Second

// defaultIgnoredNames hide at every tree level until "Show ignored" is
// toggled. They are the names that reliably mean generated or vendored
// bulk — dependency caches, build outputs — never one-file entries like
// .env or IDE settings, which stay visible.
var defaultIgnoredNames = map[string]struct{}{
	".DS_Store":     {},
	".cache":        {},
	".git":          {},
	".gradle":       {},
	".mypy_cache":   {},
	".next":         {},
	".nuxt":         {},
	".pytest_cache": {},
	".turbo":        {},
	".venv":         {},
	"__pycache__":   {},
	"build":         {},
	"coverage":      {},
	"dist":          {},
	"node_modules":  {},
	"target":        {},
	"venv":          {},
}

// IgnoreSet answers one question for the tree renderer: is this entry
// hidden? It merges the fixed names with git's ignored paths for the root.
type IgnoreSet struct {
	names map[string]struct{}
	paths map[string]struct{} // slash-relative; ignored directories carry no trailing slash
}

// NewIgnoreSet builds the merged set from git's ignored relative paths.
func NewIgnoreSet(gitPaths []string) *IgnoreSet {
	set := &IgnoreSet{names: defaultIgnoredNames}
	if len(gitPaths) == 0 {
		return set
	}
	set.paths = make(map[string]struct{}, len(gitPaths))
	for _, p := range gitPaths {
		p = strings.TrimSuffix(filepath.ToSlash(filepath.Clean(p)), "/")
		if p == "" || p == "." {
			continue
		}
		set.paths[p] = struct{}{}
	}
	return set
}

// Ignored reports whether the entry hides: by its own name (any depth), by
// an exact git-ignored relative path, or inside a git-ignored directory.
func (s *IgnoreSet) Ignored(name, relPath string) bool {
	if _, ok := s.names[name]; ok {
		return true
	}
	if len(s.paths) == 0 {
		return false
	}
	rel := filepath.ToSlash(filepath.Clean(relPath))
	if rel == "." {
		return false
	}
	if _, ok := s.paths[rel]; ok {
		return true
	}
	for dir := path.Dir(rel); dir != "." && dir != "/"; dir = path.Dir(dir) {
		if _, ok := s.paths[dir]; ok {
			return true
		}
	}
	return false
}

// GitIgnorePaths returns the root's ignored paths as slash-separated
// relative paths, directories without a trailing slash (--directory folds
// node_modules into one line instead of enumerating its contents). -z keeps
// paths verbatim: no C-style quoting for non-ASCII (core.quotePath), no
// whitespace mangling. Any git failure — not a repository, no git binary —
// yields an empty list and an error; the caller keeps the fixed-name set.
func GitIgnorePaths(ctx context.Context, root string) ([]string, error) {
	ctx, cancel := context.WithTimeout(ctx, IgnoreDeadline)
	defer cancel()
	cmd := exec.CommandContext(ctx, "git", "-C", root,
		"ls-files", "-z", "--others", "--ignored", "--exclude-standard", "--directory")
	out, err := cmd.Output()
	if err != nil {
		return nil, err
	}
	paths := make([]string, 0, 64)
	for _, entry := range bytes.Split(out, []byte{0}) {
		line := strings.TrimSuffix(filepath.ToSlash(filepath.Clean(string(entry))), "/")
		if line == "" || line == "." {
			continue
		}
		paths = append(paths, line)
	}
	return paths, nil
}
