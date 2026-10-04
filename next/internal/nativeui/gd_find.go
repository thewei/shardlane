package nativeui

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/gitworkbench"
)

// Find in diffs and hunk navigation, ported from egoist/godiff find.go
// (commit 88b89e0), against the shell's guarded state.

// gdMatch is a line holding the query of the find bar, or a file whose
// path holds it (hunk -1).
type gdMatch struct {
	file int
	hunk int32
	line int32
}

// gdSearching reports whether the find bar is open with a query.
func (s *Shell) gdSearching() bool {
	return s.git.gdFinding && strings.TrimSpace(s.git.gdQuery) != ""
}

// gdFindRanges returns [start,end] byte pairs where query is in text,
// ignoring case.
func gdFindRanges(text, query string) [][2]int {
	query = strings.TrimSpace(query)
	if query == "" || len(query) > len(text) {
		return nil
	}
	lower, q := strings.ToLower(text), strings.ToLower(query)
	if len(lower) != len(text) {
		// Lowercasing changed the length: match the text as it is.
		lower, q = text, query
	}
	var out [][2]int
	for start := 0; ; {
		i := strings.Index(lower[start:], q)
		if i < 0 {
			return out
		}
		out = append(out, [2]int{start + i, start + i + len(q)})
		start += i + len(q)
	}
}

// gdUpdateMatches finds the query in the files' paths and lines.
func (s *Shell) gdUpdateMatches() {
	key := s.git.gdQuery
	if !s.git.gdFinding {
		key = ""
	}
	if key == s.git.gdMatchesFor {
		return
	}
	s.git.gdMatchesFor = key
	s.git.gdMatches = s.git.gdMatches[:0]
	s.git.gdFileMatches = map[int]bool{}
	q := strings.ToLower(strings.TrimSpace(key))
	if q == "" {
		s.git.gdMatch = 0
		return
	}
	filter := strings.TrimSpace(s.changes.filter)
	for fi, f := range s.git.gdFiles {
		if !s.gdHasContent(f) || !gdFuzzyMatch(f.cf.Path, filter) {
			continue
		}
		found := false
		if strings.Contains(strings.ToLower(f.cf.Path), q) {
			s.git.gdMatches = append(s.git.gdMatches, gdMatch{file: fi, hunk: -1, line: -1})
			found = true
		}
		for hi, h := range f.hunks() {
			for li, l := range h.Lines {
				if strings.Contains(strings.ToLower(l.Text), q) {
					s.git.gdMatches = append(s.git.gdMatches, gdMatch{file: fi, hunk: int32(hi), line: int32(li)})
					found = true
				}
			}
		}
		if !found && strings.Contains(strings.ToLower(s.gdFileNote(f)), q) {
			s.git.gdMatches = append(s.git.gdMatches, gdMatch{file: fi, hunk: -1, line: -1})
			found = true
		}
		if found {
			s.git.gdFileMatches[fi] = true
		}
	}
	if s.git.gdMatch >= len(s.git.gdMatches) {
		s.git.gdMatch = 0
	}
	s.git.gdRowsDirty = true
}

// gdActiveMatch reports whether a line is the current match.
func (s *Shell) gdActiveMatch(f *gdFile, ls gdLineSide) bool {
	if s.git.gdMatch < 0 || s.git.gdMatch >= len(s.git.gdMatches) || ls.index < 0 {
		return false
	}
	m := s.git.gdMatches[s.git.gdMatch]
	return s.git.gdFiles[m.file] == f && m.hunk == ls.hunk && m.line == ls.index
}

// gdShowMatch scrolls the current match into the middle of the surface.
func (s *Shell) gdShowMatch() {
	if s.git.gdMatch < 0 || s.git.gdMatch >= len(s.git.gdMatches) {
		return
	}
	m := s.git.gdMatches[s.git.gdMatch]
	if s.git.gdRowsDirty {
		s.buildGdRows()
	}
	for i := range s.git.gdRows {
		r := &s.git.gdRows[i]
		if int(r.file) != m.file {
			continue
		}
		if m.hunk < 0 && r.kind == gdRowHeader {
			s.git.gdList.ScrollTo(i, ui.Start)
			break
		}
		if r.kind == gdRowLine && r.hunk == m.hunk && (r.a == m.line || r.b == m.line) {
			s.git.gdList.ScrollTo(i, ui.Center)
			break
		}
	}
	s.git.gdCurrent = m.file
	s.selectGdTreeFile(m.file)
}

// gdAnchor is where j and k stop: a hunk of a file shown open.
type gdAnchor struct {
	file int
	hunk int
	row  int
}

// gdAnchors lists the hunks in the order they show.
func (s *Shell) gdAnchors() []gdAnchor {
	if s.git.gdRowsDirty {
		s.buildGdRows()
	}
	var out []gdAnchor
	seen := map[[2]int]bool{}
	for i := range s.git.gdRows {
		r := &s.git.gdRows[i]
		switch r.kind {
		case gdRowHeader:
			f := s.git.gdFiles[r.file]
			if f.collapsed && !s.gdForceOpen(int(r.file)) {
				out = append(out, gdAnchor{file: int(r.file), hunk: -1, row: i})
			}
		case gdRowLine:
			if r.hunk < 0 {
				continue
			}
			k := [2]int{int(r.file), int(r.hunk)}
			if seen[k] {
				continue
			}
			f := s.git.gdFiles[r.file]
			l1, l2 := f.lineAt(r.hunk, r.a), f.lineAt(r.hunk, r.b)
			if (l1 != nil && l1.Kind != gitworkbench.KindContext) || (l2 != nil && l2.Kind != gitworkbench.KindContext) {
				seen[k] = true
				out = append(out, gdAnchor{file: int(r.file), hunk: int(r.hunk), row: i})
			}
		}
	}
	return out
}

// gdNextHunk moves the choice to the next (dir 1) or the previous (dir -1)
// hunk, and selects its changed lines.
func (s *Shell) gdNextHunk(dir int) {
	anchors := s.gdAnchors()
	if len(anchors) == 0 {
		return
	}
	at := -1
	for i, a := range anchors {
		if a.file == s.git.gdSelFile && a.hunk == s.git.gdSelHunk {
			at = i
		}
	}
	if at >= 0 {
		at = min(max(at+dir, 0), len(anchors)-1)
	} else {
		first, _ := s.git.gdList.Visible()
		at = 0
		if dir > 0 {
			for i, a := range anchors {
				if a.row > first {
					at = i
					break
				}
			}
		} else {
			for i, a := range anchors {
				if a.row < first {
					at = i
				}
			}
		}
	}
	a := anchors[at]
	s.git.gdSelFile, s.git.gdSelHunk = a.file, a.hunk
	if a.hunk < 0 {
		s.git.gdList.ScrollTo(a.row, ui.Start)
	} else {
		s.git.gdList.ScrollTo(a.row, ui.Center)
	}
	s.git.gdCurrent = a.file
	s.git.revealAt = timeNow()
	s.selectGdTreeFile(a.file)
}

// gdIsSelectedLine reports whether a line is a changed line of the hunk
// chosen with j and k.
func (s *Shell) gdIsSelectedLine(f *gdFile, ls gdLineSide) bool {
	if ls.hunk < 0 || int(ls.hunk) != s.git.gdSelHunk || ls.kind == gitworkbench.KindContext ||
		s.git.gdSelFile < 0 || s.git.gdSelFile >= len(s.git.gdFiles) {
		return false
	}
	return s.git.gdFiles[s.git.gdSelFile] == f
}

// selectGdTreeFile chooses a file's row in the Changes tree, as the surface
// scrolls to it.
func (s *Shell) selectGdTreeFile(i int) {
	if i < 0 || i >= len(s.git.gdFiles) {
		return
	}
	path := s.git.gdFiles[i].cf.Path
	if path == "" {
		return
	}
	s.syncChangesSelectionFromDiff(path)
}
