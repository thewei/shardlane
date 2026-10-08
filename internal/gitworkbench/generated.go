package gitworkbench

import (
	"path/filepath"
	"strings"
)

// generatedRules classifies dependency-lock, build-output and generated
// paths so review can collapse them by default (plan GWB-095). Table-driven
// and conservative: false negatives are fine, false positives hide real work.
type generatedRule struct {
	note  string
	match func(path string) bool
}

var generatedRules = []generatedRule{
	{note: "lockfile", match: baseIn(
		"package-lock.json", "yarn.lock", "pnpm-lock.yaml", "bun.lockb", "bun.lock",
		"Cargo.lock", "go.sum", "Podfile.lock", "Cartfile.resolved",
		"packages.lock.json", "poetry.lock", "uv.lock", "composer.lock",
		"Gemfile.lock", "mix.lock", "flake.lock", "pubspec.lock",
	)},
	{note: "minified", match: suffixIn(".min.js", ".min.css", ".min.ts", ".map")},
	{note: "codegen dir", match: dirIn("node_modules", "vendor", "dist", "build", "out",
		"target", ".next", ".nuxt", ".output", "generated", "__generated__",
		"Pods", ".terraform", "obj", "bin")},
	{note: "codegen suffix", match: suffixIn(
		".pb.go", "_pb2.py", ".generated.go", ".g.dart", ".generated.cs",
		".g.dart", "_pb.js", ".d.ts")}, // .d.ts heuristic stays permissive
	{note: "lockfile-ish", match: func(path string) bool {
		base := filepath.Base(path)
		return strings.HasSuffix(base, ".sum") || strings.HasSuffix(base, ".lock")
	}},
}

func baseIn(names ...string) func(string) bool {
	set := make(map[string]struct{}, len(names))
	for _, n := range names {
		set[n] = struct{}{}
	}
	return func(path string) bool {
		_, ok := set[filepath.Base(path)]
		return ok
	}
}

func suffixIn(suffixes ...string) func(string) bool {
	return func(path string) bool {
		lower := strings.ToLower(path)
		for _, s := range suffixes {
			if strings.HasSuffix(lower, s) {
				return true
			}
		}
		return false
	}
}

func dirIn(dirs ...string) func(string) bool {
	set := make(map[string]struct{}, len(dirs))
	for _, d := range dirs {
		set[d] = struct{}{}
	}
	return func(path string) bool {
		parts := strings.Split(filepath.ToSlash(path), "/")
		// Every segment except the final filename participates.
		for _, part := range parts[:max(0, len(parts)-1)] {
			if _, ok := set[part]; ok {
				return true
			}
		}
		return false
	}
}

// IsGenerated reports whether the path looks like generated/dependency
// output by the table above.
func IsGenerated(path string) bool {
	normalized := filepath.ToSlash(path)
	for _, rule := range generatedRules {
		if rule.match(normalized) {
			return true
		}
	}
	return false
}
