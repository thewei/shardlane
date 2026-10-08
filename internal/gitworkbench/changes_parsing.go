package gitworkbench

import (
	"bytes"
	"strconv"
	"strings"
)

// porcelainEntry is one NUL-delimited `status --porcelain=v1 -z` record.
type porcelainEntry struct {
	XY      string
	Path    string
	OldPath string
}

func parsePorcelainZ(data []byte) []porcelainEntry {
	var entries []porcelainEntry
	fields := strings.Split(string(data), "\x00")
	for i := 0; i < len(fields); i++ {
		f := fields[i]
		if len(f) < 3 {
			continue
		}
		xy := f[:2]
		path := f[3:]
		entry := porcelainEntry{XY: xy, Path: path}
		// Rename/copy records carry the NUL-separated source after the target.
		if (xy[0] == 'R' || xy[0] == 'C') && i+1 < len(fields) {
			entry.OldPath = fields[i+1]
			i++
		}
		entries = append(entries, entry)
	}
	return entries
}

// statusToChange maps a porcelain XY pair to a ChangeStatus.
func statusToChange(xy string) ChangeStatus {
	x, y := xy[0], xy[1]
	switch {
	case x == '?' || y == '?':
		return StatusUntracked
	case x == 'U' || y == 'U' || x == 'A' && y == 'A' || x == 'D' && y == 'D':
		return StatusConflicted
	case x == 'R' || y == 'R':
		return StatusRenamed
	case x == 'C' || y == 'C':
		return StatusCopied
	case x == 'T' || y == 'T':
		return StatusTypeChange
	case x == 'A' || y == 'A':
		return StatusAdded
	case x == 'D' || y == 'D':
		return StatusDeleted
	default:
		return StatusModified
	}
}

// numstatLine is one `diff --numstat` record.
type numstatLine struct {
	Add, Del int64
	Path     string
	Binary   bool
}

func parseNumstat(data []byte) map[string]numstatLine {
	result := make(map[string]numstatLine)
	for _, line := range strings.Split(string(data), "\n") {
		if line == "" {
			continue
		}
		parts := strings.SplitN(line, "\t", 3)
		if len(parts) != 3 {
			continue
		}
		entry := numstatLine{Path: unquotePath(parts[2])}
		if parts[0] == "-" {
			entry.Binary = true
		} else {
			entry.Add, _ = strconv.ParseInt(parts[0], 10, 32)
			entry.Del, _ = strconv.ParseInt(parts[1], 10, 32)
		}
		// Rename numstat prints "old => new" or "{prefix => prefix}suffix".
		result[cleanNumstatPath(entry.Path)] = entry
	}
	return result
}

// parseNumstatZ parses NUL-delimited `git diff --numstat -z` output, which
// safely handles filenames with spaces, newlines, tabs, quotes, and non-UTF8
// bytes (P1-07). Rename records carry the old path in the two NUL fields
// after the empty destination slot:
//
//	add \t del \t \x00 oldPath \x00 newPath \x00
func parseNumstatZ(data []byte) map[string]numstatLine {
	result := make(map[string]numstatLine)
	tokens := bytes.Split(data, []byte{0})
	for i := 0; i < len(tokens); i++ {
		tok := tokens[i]
		if len(tok) == 0 {
			continue
		}
		parts := bytes.SplitN(tok, []byte{'\t'}, 3)
		if len(parts) != 3 {
			continue
		}
		entry := numstatLine{}
		if bytes.Equal(parts[0], []byte{'-'}) {
			entry.Binary = true
		} else {
			entry.Add, _ = strconv.ParseInt(string(parts[0]), 10, 32)
			entry.Del, _ = strconv.ParseInt(string(parts[1]), 10, 32)
		}
		path := string(parts[2])
		if path == "" && i+2 < len(tokens) {
			path = string(tokens[i+2])
			i += 2
		}
		entry.Path = path
		result[entry.Path] = entry
	}
	return result
}

// cleanNumstatPath resolves git's rename numstat brace syntax to the plain
// destination path; the destination is the review key.
func cleanNumstatPath(p string) string {
	if open := strings.Index(p, "{"); open >= 0 {
		if arrow := strings.Index(p[open:], " => "); arrow >= 0 {
			close_ := strings.LastIndex(p, "}")
			if close_ > open {
				return p[:open] + p[open+arrow+4:close_] + p[close_+1:]
			}
		}
	}
	if arrow := strings.Index(p, " => "); arrow >= 0 {
		return p[arrow+4:]
	}
	return p
}

// unquotePath decodes Git's C-quoted path form, including full octal byte
// escapes ("\346\227\245" → 日本). Non-quoted paths pass through unchanged.
func unquotePath(p string) string {
	if len(p) >= 2 && p[0] == '"' && p[len(p)-1] == '"' {
		inner := p[1 : len(p)-1]
		var out []byte
		for i := 0; i < len(inner); i++ {
			c := inner[i]
			if c != '\\' || i+1 >= len(inner) {
				out = append(out, c)
				continue
			}
			i++
			esc := inner[i]
			if esc >= '0' && esc <= '7' {
				oct := int(esc - '0')
				j := 1
				for j < 3 && i+1 < len(inner) && inner[i+1] >= '0' && inner[i+1] <= '7' {
					i++
					oct = oct*8 + int(inner[i]-'0')
					j++
				}
				out = append(out, byte(oct))
			} else {
				switch esc {
				case 'n':
					out = append(out, '\n')
				case 't':
					out = append(out, '\t')
				case 'r':
					out = append(out, '\r')
				case '\\':
					out = append(out, '\\')
				case '"':
					out = append(out, '"')
				default:
					out = append(out, '\\', esc)
				}
			}
		}
		return string(out)
	}
	return p
}
