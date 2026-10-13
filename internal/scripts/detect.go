// detect.go sniffs project-defined commands from well-known project files
// (package.json scripts, Makefile targets, justfile recipes). Detection is a
// bounded, read-only local capability in the same class as the Files tree:
// Shardlane only reads metadata and never executes anything itself — every
// command still runs through the Herdr-owned pane paths.
package scripts

import (
	"encoding/json"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
)

// Detection bounds: no file read exceeds maxDetectBytes, no root yields more
// than maxDetectCommands entries, and parse failure of one source never
// blocks the others.
const (
	maxDetectBytes    = 256 << 10 // 256 KiB
	maxDetectCommands = 48
	maxNameLen        = 64
)

// DetectedCommand is one project-defined command. Name is the stable
// identity within the root (menu label, last-used key); Command is the full
// line typed into the pane's shell.
type DetectedCommand struct {
	Name    string
	Command string
	Source  string
}

// CommandEntry is one runnable entry of a project's command menu: a
// user-authored Script definition or a detected project command. User
// definitions win name conflicts, so the menu stays one merged list.
type CommandEntry struct {
	Name    string
	Command string
	Source  string
	User    bool
}

// Detect sniffs the commands defined by root's project files. It returns nil
// unless root is an existing local directory — the guard that keeps remote
// machine panes honest (their paths do not exist locally, so no wrong
// commands are ever offered).
func Detect(root string) []DetectedCommand {
	if root == "" {
		return nil
	}
	if info, err := os.Stat(root); err != nil || !info.IsDir() {
		return nil
	}

	seen := make(map[string]bool)
	var out []DetectedCommand
	add := func(cmds []DetectedCommand) {
		for _, cmd := range cmds {
			if cmd.Name == "" || cmd.Command == "" || seen[cmd.Name] {
				continue
			}
			seen[cmd.Name] = true
			out = append(out, cmd)
			if len(out) >= maxDetectCommands {
				return
			}
		}
	}

	add(detectPackageJSON(root))
	add(detectMakefile(root))
	add(detectJustfile(root))
	return out
}

// MergeCommands joins user script definitions (which win name conflicts) and
// detected commands into one stable menu list.
func MergeCommands(defs []ScriptDefinition, detected []DetectedCommand) []CommandEntry {
	out := make([]CommandEntry, 0, len(defs)+len(detected))
	seen := make(map[string]bool, len(defs)+len(detected))
	for _, def := range defs {
		if def.Name == "" || def.Command == "" || seen[def.Name] {
			continue
		}
		seen[def.Name] = true
		out = append(out, CommandEntry{Name: def.Name, Command: def.Command, Source: "script", User: true})
	}
	for _, cmd := range detected {
		if seen[cmd.Name] {
			continue
		}
		seen[cmd.Name] = true
		out = append(out, CommandEntry{Name: cmd.Name, Command: cmd.Command, Source: cmd.Source})
	}
	return out
}

// detectPackageJSON maps the "scripts" field, typed through the project's
// own package manager (lockfile probe: bun → pnpm → yarn → npm).
func detectPackageJSON(root string) []DetectedCommand {
	data, err := readBounded(filepath.Join(root, "package.json"))
	if err != nil {
		return nil
	}
	var doc struct {
		Scripts map[string]string `json:"scripts"`
	}
	if err := json.Unmarshal(data, &doc); err != nil || len(doc.Scripts) == 0 {
		return nil
	}

	names := make([]string, 0, len(doc.Scripts))
	for name := range doc.Scripts {
		names = append(names, name)
	}
	sort.Strings(names)

	runner := "npm run"
	switch {
	case exists(filepath.Join(root, "bun.lock")), exists(filepath.Join(root, "bun.lockb")):
		runner = "bun run"
	case exists(filepath.Join(root, "pnpm-lock.yaml")):
		runner = "pnpm run"
	case exists(filepath.Join(root, "yarn.lock")):
		runner = "yarn"
	case exists(filepath.Join(root, "package-lock.json")):
		runner = "npm run"
	}

	out := make([]DetectedCommand, 0, len(names))
	for _, name := range names {
		if !validCommandName(name) {
			continue
		}
		out = append(out, DetectedCommand{Name: name, Command: runner + " " + name, Source: "package.json"})
	}
	return out
}

// makefileTarget matches column-0 rule lines ("target:" / "target ::"), the
// only safe approximation of make semantics a client may make.
var makefileTarget = regexp.MustCompile(`^([^.#\s][^:=\t]*?)\s*::?(?:\s|$)`)

func detectMakefile(root string) []DetectedCommand {
	for _, name := range []string{"Makefile", "makefile", "GNUmakefile"} {
		if data, err := readBounded(filepath.Join(root, name)); err == nil {
			return parseColonTargets(string(data), "make ", "Makefile")
		}
	}
	return nil
}

// justRecipe matches column-0 recipe headers ("build:" / "deploy env:").
// Assignments ("x := 1") never match because the colon is taken.
var justRecipe = regexp.MustCompile(`^([a-zA-Z][a-zA-Z0-9_-]*)(?:\s[^:]*)?:(?:\s|$)`)

func detectJustfile(root string) []DetectedCommand {
	for _, name := range []string{"justfile", "Justfile"} {
		if data, err := readBounded(filepath.Join(root, name)); err == nil {
			return parseColonTargets(string(data), "just ", "justfile")
		}
	}
	return nil
}

// parseColonTargets extracts column-0 names from rule/recipe headers and
// prefixes each with runner ("make " / "just "). Recipe bodies (tab- or
// indented lines), comments and variable assignments are skipped.
func parseColonTargets(text, runner, source string) []DetectedCommand {
	var out []DetectedCommand
	for _, line := range strings.Split(text, "\n") {
		if line == "" || line[0] == '\t' || line[0] == '#' || line[0] == ' ' {
			continue
		}
		match := targetRegexp(source).FindStringSubmatch(line)
		if match == nil {
			continue
		}
		name := match[1]
		if !validCommandName(name) || strings.Contains(name, "$") {
			continue
		}
		out = append(out, DetectedCommand{Name: name, Command: runner + name, Source: source})
		if len(out) >= maxDetectCommands {
			break
		}
	}
	return out
}

func targetRegexp(source string) *regexp.Regexp {
	if source == "Makefile" {
		return makefileTarget
	}
	return justRecipe
}

// validCommandName keeps menu labels and typed commands predictable: no
// whitespace, control characters or path separators.
func validCommandName(name string) bool {
	if name == "" || len(name) > maxNameLen {
		return false
	}
	for _, r := range name {
		switch {
		case r <= ' ', r >= 0x7f:
			return false
		case r == '/' || r == '\\' || r == '$' || r == ';' || r == '&' || r == '|' || r == '>' || r == '<' || r == '`':
			return false
		}
	}
	return true
}

// readBounded reads at most maxDetectBytes of one file.
func readBounded(path string) ([]byte, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	buf := make([]byte, maxDetectBytes+1)
	n, err := f.Read(buf)
	if err != nil && n == 0 {
		return nil, err
	}
	if n > maxDetectBytes {
		n = maxDetectBytes
	}
	return buf[:n], nil
}

func exists(path string) bool {
	_, err := os.Stat(path)
	return err == nil
}
