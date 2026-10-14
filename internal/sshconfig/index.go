package sshconfig

/**
 * [INPUT]: 依赖用户自己的 OpenSSH 客户端配置（~/.ssh/config 及其 Include 闭包），仅标准库
 * [OUTPUT]: DefaultPath/IndexPath 行级索引（Host/Match 块、多模式、`=` 与引号、Include 递归+glob+环防护）、块原文 Facts 与只读的主机分组（SSH Config Editor 式注释标记）
 * [POS]: 本地客户端能力域包：只做"列目录"级解析，绝不求值语义（求值委托系统 ssh，见 effective.go）；不读写私钥、不记录文件内容
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// The indexer is deliberately line-level and bounded: it must list what the
// file declares, never decide what it means. Resolution and connection stay
// with the system ssh client, so a listing mistake can never break a real
// connection — it only hides a row.
const (
	maxIncludeDepth = 8
	maxFiles        = 64
	maxEntries      = 4000
	maxBlockLines   = 400
	maxWarnings     = 32
	maxGroups       = 200
	maxGroupName    = 64
)

// Kind distinguishes the two block headers ssh_config knows.
type Kind string

const (
	KindHost  Kind = "host"
	KindMatch Kind = "match"
)

// KV is one directive fact.
type KV struct {
	Key, Value string
}

// Entry is one top-level Host/Match block. Lines keeps the raw block text
// (header line first) so the UI can show directive facts without re-reading
// the file; content is never logged.
type Entry struct {
	Kind     Kind
	Patterns []string // KindHost: the Host line's patterns, as written
	Match    string   // KindMatch: the condition text after the keyword
	File     string   // absolute path of the defining file
	Line     int      // 1-based line of the block header
	Lines    []string
	Group    string // id of the group a `#SCEGroup <id>` marker files the block under; "" when none
}

// Alias returns the one literal connectable name of a host entry, or ""
// when the entry is a Match block, carries several patterns, uses
// wildcard/negation syntax that only the ssh client itself can resolve, or
// begins with "-" (an option-like token must never reach an ssh argv).
func (e Entry) Alias() string {
	if e.Kind != KindHost || len(e.Patterns) != 1 {
		return ""
	}
	pattern := e.Patterns[0]
	if strings.ContainsAny(pattern, "*?!") || strings.HasPrefix(pattern, "-") {
		return ""
	}
	return pattern
}

// Title renders the entry the way the sidebar lists it.
func (e Entry) Title() string {
	if e.Kind == KindMatch {
		return "Match " + e.Match
	}
	return strings.Join(e.Patterns, " ")
}

// Facts lists the block's directives (every line after the header) in file
// order. Values stay raw; this is display data, never re-serialized.
func (e Entry) Facts() []KV {
	var facts []KV
	for _, line := range e.Lines[min(1, len(e.Lines)):] {
		keyword, rest, comment := splitKeyword(line)
		if comment || keyword == "" {
			continue
		}
		facts = append(facts, KV{Key: keyword, Value: rest})
	}
	return facts
}

// Group is one named collection of hosts as the config's own comments
// declare it. The markers are the ones SSH Config Editor writes: a
// `#SCE_GROUP:<id>:::<name>` line names a group, and a `#SCEGroup <id>` line
// inside a Host block files that block under it. ssh ignores them as comments
// and they are only ever read here — never written, so a config that carries
// groups keeps working in the tool that made them.
type Group struct {
	ID, Name string
}

// Index is the listing of one config file plus its Include closure.
type Index struct {
	MainPath string
	Files    []string // absolute, main first, in Include encounter order
	Entries  []Entry  // document order, includes expanded in place
	Groups   []Group  // declared groups, in declaration order
	Warnings []string // bounded, path-level only
}

// DefaultPath is the user's own OpenSSH client configuration.
func DefaultPath() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".ssh", "config"), nil
}

// IndexPath lists mainPath and its Include closure. The only error is the
// main file itself being unreadable (fs.ErrNotExist for a fresh machine);
// include failures degrade to warnings so one bad file cannot hide the
// rest of the listing.
func IndexPath(mainPath string) (*Index, error) {
	ix := &Index{MainPath: mainPath}
	if err := ix.addFile(mainPath, map[string]bool{}, 0); err != nil {
		return nil, err
	}
	return ix, nil
}

func (ix *Index) addFile(path string, seen map[string]bool, depth int) error {
	if depth > maxIncludeDepth {
		ix.warn("include depth limit reached")
		return nil
	}
	if len(ix.Files) >= maxFiles {
		ix.warn("include file limit reached")
		return nil
	}
	abs, err := filepath.Abs(path)
	if err != nil {
		abs = path
	}
	// Key the cycle guard on the resolved target so linked files cannot
	// loop the walk.
	if real, err := filepath.EvalSymlinks(abs); err == nil {
		abs = real
	}
	if seen[abs] {
		return nil
	}
	data, err := os.ReadFile(abs)
	if err != nil {
		if depth == 0 {
			return err
		}
		ix.warn("cannot read include: " + filepath.Base(abs))
		return nil
	}
	seen[abs] = true
	ix.Files = append(ix.Files, abs)

	// A Host keyword always starts a host block — it also terminates an
	// open Match block (ssh_config(5)) — and Match bodies cannot contain
	// one, so line-level block tracking is exact for listing purposes.
	block := -1
	for i, raw := range splitLines(data) {
		keyword, rest, comment := splitKeyword(raw)
		if comment {
			ix.noteGroupMarker(strings.TrimSpace(raw), block)
			continue
		}
		switch strings.ToLower(keyword) {
		case "host":
			if len(ix.Entries) >= maxEntries {
				ix.warn("entry limit reached")
				return nil
			}
			ix.Entries = append(ix.Entries, Entry{
				Kind:     KindHost,
				Patterns: splitFields(rest),
				File:     abs,
				Line:     i + 1,
				Lines:    []string{raw},
			})
			block = len(ix.Entries) - 1
		case "match":
			if len(ix.Entries) >= maxEntries {
				ix.warn("entry limit reached")
				return nil
			}
			ix.Entries = append(ix.Entries, Entry{
				Kind:  KindMatch,
				Match: rest,
				File:  abs,
				Line:  i + 1,
				Lines: []string{raw},
			})
			block = len(ix.Entries) - 1
		case "include":
			for _, inc := range ix.expandIncludes(rest) {
				if err := ix.addFile(inc, seen, depth+1); err != nil {
					return err
				}
			}
		default:
			if block >= 0 && len(ix.Entries[block].Lines) < maxBlockLines {
				ix.Entries[block].Lines = append(ix.Entries[block].Lines, raw)
			}
		}
	}
	return nil
}

// noteGroupMarker reads one comment line as a group declaration or as the
// marker of the block above it (block is that block's index, -1 before the
// first one). Any other comment is just a comment.
func (ix *Index) noteGroupMarker(line string, block int) {
	if rest, ok := strings.CutPrefix(line, "#SCE_GROUP:"); ok {
		id, name, _ := strings.Cut(rest, ":::")
		id = strings.Trim(strings.TrimSpace(id), ":")
		if id == "" {
			return
		}
		for _, have := range ix.Groups {
			if have.ID == id {
				return
			}
		}
		if len(ix.Groups) >= maxGroups {
			ix.warn("group limit reached")
			return
		}
		name = strings.TrimSpace(name)
		if runes := []rune(name); len(runes) > maxGroupName {
			name = string(runes[:maxGroupName])
		}
		ix.Groups = append(ix.Groups, Group{ID: id, Name: name})
		return
	}
	if id, ok := strings.CutPrefix(line, "#SCEGroup "); ok && block >= 0 && ix.Entries[block].Group == "" {
		ix.Entries[block].Group = strings.TrimSpace(id)
	}
}

// expandIncludes turns one Include line's arguments into file paths.
// Relative arguments resolve against ~/.ssh, as ssh_config(5) requires.
func (ix *Index) expandIncludes(rest string) []string {
	home, _ := os.UserHomeDir()
	var out []string
	for _, token := range splitFields(rest) {
		path := expandTilde(token, home)
		if !filepath.IsAbs(path) {
			path = filepath.Join(home, ".ssh", path)
		}
		matches, err := filepath.Glob(path)
		if err != nil {
			ix.warn("bad include pattern: " + token)
			continue
		}
		if len(matches) == 0 {
			ix.warn("include matched no files: " + token)
			continue
		}
		sort.Strings(matches)
		out = append(out, matches...)
		if len(out) >= maxFiles {
			break
		}
	}
	return out
}

func (ix *Index) warn(msg string) {
	if len(ix.Warnings) < maxWarnings {
		ix.Warnings = append(ix.Warnings, msg)
	}
}

// splitKeyword splits one config line into its keyword and raw remainder.
// A leading '#' marks a comment; ssh_config has no trailing comments, so an
// inner '#' stays part of the value. Both "Keyword value" and "Keyword=value"
// (with or without surrounding spaces) resolve to the same shape.
func splitKeyword(line string) (keyword, rest string, comment bool) {
	trimmed := strings.TrimSpace(line)
	if trimmed == "" || strings.HasPrefix(trimmed, "#") {
		return "", "", true
	}
	end := 0
	for end < len(trimmed) {
		c := trimmed[end]
		if c == ' ' || c == '\t' || c == '=' || c == '"' {
			break
		}
		end++
	}
	keyword = trimmed[:end]
	rest = strings.TrimLeft(trimmed[end:], " \t")
	rest = strings.TrimPrefix(rest, "=")
	rest = strings.TrimLeft(rest, " \t")
	return keyword, rest, false
}

// splitFields splits a value into whitespace-separated tokens, honoring
// double quotes ("Host \"my host\"" is one pattern).
func splitFields(line string) []string {
	var fields []string
	i := 0
	for i < len(line) {
		for i < len(line) && (line[i] == ' ' || line[i] == '\t') {
			i++
		}
		if i >= len(line) {
			break
		}
		if line[i] == '"' {
			i++
			start := i
			for i < len(line) && line[i] != '"' {
				i++
			}
			fields = append(fields, line[start:i])
			if i < len(line) {
				i++
			}
			continue
		}
		start := i
		for i < len(line) && line[i] != ' ' && line[i] != '\t' {
			i++
		}
		fields = append(fields, line[start:i])
	}
	return fields
}

func expandTilde(path, home string) string {
	if home == "" {
		return path
	}
	if path == "~" {
		return home
	}
	if strings.HasPrefix(path, "~/") {
		return filepath.Join(home, path[2:])
	}
	return path
}

func splitLines(data []byte) []string {
	return strings.Split(string(data), "\n")
}
