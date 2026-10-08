package gitworkbench

import (
	"strconv"
	"strings"
)

// LineKind is the role of one diff line in a hunk.
type LineKind uint8

const (
	KindContext LineKind = iota
	KindAdd
	KindDelete
)

// DiffLine is one parsed line of a hunk (plan §16).
type DiffLine struct {
	Kind      LineKind
	OldLine   int // 1-based; 0 for pure additions
	NewLine   int // 1-based; 0 for pure deletions
	Text      string
	NoNewline bool // "\ No newline at end of file" follows this line
}

// Hunk is one @@ region of a file patch.
type Hunk struct {
	OldStart int
	OldLines int
	NewStart int
	NewLines int
	Section  string // trailing @@ context
	Lines    []DiffLine
}

// filePatch is the parsed model of one file's unified diff.
type filePatch struct {
	raw      []byte
	oldPath  string
	newPath  string
	oldMode  string
	newMode  string
	status   ChangeStatus
	binary   bool
	tooLarge bool
	hunks    []Hunk
}

// maxPatchFileLines bounds one file's parsed line count against pathological
// input; real patches under the 32 MiB read bound stay far below it.
const maxPatchFileLines = 1 << 20

// parsePatchSet splits a full `git diff` output into per-file patches keyed
// by destination path.
func parsePatchSet(data []byte) map[string]*filePatch {
	result := make(map[string]*filePatch)
	if len(data) == 0 {
		return result
	}
	lines := splitLines(string(data))
	var current *filePatch
	var body []string
	bodyBytes := 0
	flush := func() {
		if current != nil {
			mergeBody(current, body)
			key := current.newPath
			if key == "" {
				key = current.oldPath
			}
			if key != "" {
				result[key] = current
			}
			current = nil
			body = nil
			bodyBytes = 0
		}
	}
	for _, line := range lines {
		if strings.HasPrefix(line, "diff --git ") {
			flush()
			current = &filePatch{raw: []byte(line + "\n")}
			parseDiffGitLine(current, line)
			continue
		}
		if current == nil {
			continue // preamble outside any file section
		}
		current.raw = append(current.raw, []byte(line+"\n")...)
		if len(current.raw) > MaxPatchBytesPerFile*2 {
			current.tooLarge = true // keep the section, stop retaining body
			continue
		}
		body = append(body, line)
		bodyBytes += len(line)
		if bodyBytes > MaxPatchBytesPerFile*2 {
			current.tooLarge = true
		}
	}
	flush()
	return result
}

// parseDiffGitLine extracts best-effort paths from the section header. The
// `---`/`+++` extended headers remain the path authority when present.
func parseDiffGitLine(fp *filePatch, line string) {
	rest := strings.TrimSpace(strings.TrimPrefix(line, "diff --git "))
	old, new, ok := splitGitPaths(rest)
	if ok {
		fp.oldPath = old
		fp.newPath = new
	}
}

// splitGitPaths handles both quoted and unquoted `a/... b/...` pairs.
func splitGitPaths(rest string) (old, new string, ok bool) {
	if a, after, ok := decodeGitName(rest); ok {
		if b, _, ok2 := decodeGitName(after); ok2 {
			return stripPrefix(a), stripPrefix(b), true
		}
	}
	return "", "", false
}

// decodeGitName reads one possibly C-quoted name plus its trailing space
// separator; returns the remainder after it.
func decodeGitName(s string) (name, rest string, ok bool) {
	if s == "" {
		return "", "", false
	}
	if s[0] == '"' {
		end := -1
		for i := 1; i < len(s); i++ {
			if s[i] == '\\' {
				i++
				continue
			}
			if s[i] == '"' {
				end = i
				break
			}
		}
		if end < 0 {
			return "", "", false
		}
		return unquotePath(s[:end+1]), strings.TrimPrefix(s[end+1:], " "), true
	}
	// Unquoted: the separator between the two names is " b/" for the
	// diff --git line; fall back to a single-space split handled by callers.
	if idx := strings.Index(s, " b/"); idx >= 0 {
		return s[:idx], s[idx+1:], true
	}
	if space := strings.Index(s, " "); space >= 0 {
		return s[:space], s[space+1:], true
	}
	return s, "", true
}

func stripPrefix(p string) string {
	switch {
	case p == "/dev/null":
		return p
	case len(p) > 2 && p[0:2] == "a/":
		return p[2:]
	case len(p) > 2 && p[0:2] == "b/":
		return p[2:]
	}
	return p
}

// mergeBody interprets one file section's extended headers and hunk body.
func mergeBody(fp *filePatch, body []string) {
	var oldPath, newPath string
	inHunks := false
	oldCur, newCur := 0, 0
	for _, line := range body {
		switch {
		case strings.HasPrefix(line, "--- "):
			if p := decodeHeaderPath(line[4:]); p != "" {
				oldPath = p
			}
		case strings.HasPrefix(line, "+++ "):
			if p := decodeHeaderPath(line[4:]); p != "" {
				newPath = p
			}
		case strings.HasPrefix(line, "old mode "):
			fp.oldMode = strings.TrimSpace(line[len("old mode "):])
		case strings.HasPrefix(line, "new mode "):
			fp.newMode = strings.TrimSpace(line[len("new mode "):])
		case strings.HasPrefix(line, "new file mode "):
			fp.newMode = strings.TrimSpace(line[len("new file mode "):])
			fp.status = StatusAdded
		case strings.HasPrefix(line, "deleted file mode "):
			fp.oldMode = strings.TrimSpace(line[len("deleted file mode "):])
			fp.status = StatusDeleted
		case strings.HasPrefix(line, "rename from "):
			fp.status = StatusRenamed
			if p := decodeHeaderPath(line[len("rename from "):]); p != "" {
				fp.oldPath = p
			}
		case strings.HasPrefix(line, "rename to "):
			fp.status = StatusRenamed
			if p := decodeHeaderPath(line[len("rename to "):]); p != "" {
				fp.newPath = p
			}
		case strings.HasPrefix(line, "copy from "):
			fp.status = StatusCopied
			if p := decodeHeaderPath(line[len("copy from "):]); p != "" {
				fp.oldPath = p
			}
		case strings.HasPrefix(line, "copy to "):
			fp.status = StatusCopied
			if p := decodeHeaderPath(line[len("copy to "):]); p != "" {
				fp.newPath = p
			}
		case strings.HasPrefix(line, "GIT binary patch"),
			strings.HasPrefix(line, "Binary files "):
			fp.binary = true
		case strings.HasPrefix(line, "@@ "):
			if hunk, ok := parseHunkHeader(line); ok {
				fp.hunks = append(fp.hunks, hunk)
				oldCur = hunk.OldStart
				newCur = hunk.NewStart
				inHunks = true
			}
		case inHunks && len(fp.hunks) > 0:
			h := &fp.hunks[len(fp.hunks)-1]
			if len(h.Lines) >= maxPatchFileLines {
				continue
			}
			switch {
			case strings.HasPrefix(line, "\\"):
				// "\ No newline at end of file" marks the previous line.
				if n := len(h.Lines); n > 0 {
					h.Lines[n-1].NoNewline = true
				}
			case strings.HasPrefix(line, "+"):
				h.Lines = append(h.Lines, DiffLine{Kind: KindAdd, NewLine: newCur, Text: line[1:]})
				newCur++
			case strings.HasPrefix(line, "-"):
				h.Lines = append(h.Lines, DiffLine{Kind: KindDelete, OldLine: oldCur, Text: line[1:]})
				oldCur++
			case strings.HasPrefix(line, " "):
				h.Lines = append(h.Lines, DiffLine{Kind: KindContext, OldLine: oldCur, NewLine: newCur, Text: line[1:]})
				oldCur++
				newCur++
			}
		}
	}
	if oldPath != "" && oldPath != "/dev/null" {
		fp.oldPath = oldPath
	}
	if newPath != "" && newPath != "/dev/null" {
		fp.newPath = newPath
	}
	if fp.status == "" {
		switch {
		case oldPath == "/dev/null":
			fp.status = StatusAdded
		case newPath == "/dev/null":
			fp.status = StatusDeleted
		case fp.oldMode != fp.newMode && fp.oldMode != "" && fp.newMode != "":
			fp.status = StatusTypeChange
		default:
			fp.status = StatusModified
		}
	}
}

func decodeHeaderPath(s string) string {
	s = strings.TrimSpace(s)
	if idx := strings.Index(s, "\t"); idx >= 0 {
		s = s[:idx]
	}
	return stripPrefix(unquotePath(s))
}

// parseHunkHeader parses `@@ -l,s +l,s @@ section`.
func parseHunkHeader(line string) (Hunk, bool) {
	end := strings.Index(line[3:], "@@")
	if end < 0 {
		return Hunk{}, false
	}
	ranges := strings.TrimSpace(line[3 : 3+end])
	section := ""
	if 3+end+2 <= len(line) {
		section = strings.TrimSpace(line[3+end+2:])
	}
	var h Hunk
	h.Section = section
	oldPart, newPart, ok := splitHunkRanges(ranges)
	if !ok {
		return Hunk{}, false
	}
	h.OldStart, h.OldLines = parseRange(oldPart)
	h.NewStart, h.NewLines = parseRange(newPart)
	return h, true
}

func splitHunkRanges(s string) (old, new string, ok bool) {
	idx := strings.Index(s, "+")
	if idx < 0 {
		return "", "", false
	}
	return strings.TrimSpace(s[:idx]), strings.TrimSpace(s[idx:]), true
}

func parseRange(s string) (start, count int) {
	s = strings.TrimPrefix(s, "-")
	s = strings.TrimPrefix(s, "+")
	if comma := strings.Index(s, ","); comma >= 0 {
		start, _ = strconv.Atoi(s[:comma])
		count, _ = strconv.Atoi(s[comma+1:])
		return start, count
	}
	start, _ = strconv.Atoi(s)
	if start == 0 {
		return 0, 0
	}
	return start, 1
}

// parsePatch parses a single-file patch (untracked synthesis path).
func parsePatch(data []byte) *filePatch {
	set := parsePatchSet(data)
	for _, fp := range set {
		return fp
	}
	return nil
}
