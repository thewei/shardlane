package nativeui

import (
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// timeSince is how long ago t was.
func timeSince(t time.Time) time.Duration { return timeNow().Sub(t) }

// The diff surface's row model, ported from egoist/godiff rows.go and
// sidebar.go (commit 88b89e0).

// gdRowKind is what a row of the diff surface shows.
type gdRowKind uint8

const (
	gdRowHeader gdRowKind = iota // a file's header, pinned while its lines scroll
	gdRowNote                    // why the lines do not show
	gdRowGap                     // unchanged lines not shown
	gdRowLine                    // a line, or a pair of lines side by side
	gdRowEnd                     // the bottom of a file's card
	gdRowCommit                  // the message of the commit under review
)

// gdRow is a row of the diff surface.
type gdRow struct {
	kind gdRowKind
	file int32
	// For lines of hunks: the hunk, and the indices of its lines on the
	// left (old) and the right (new) side, -1 for none. Unified rows use a
	// alone. Lines of context the user expanded have hunk -1 and their
	// numbers in old and new.
	hunk     int32
	a, b     int32
	old, new int32
	// For gaps: which gap, its first lines on each side, and how many
	// lines it hides.
	gap   int32
	count int32
}

// gdRowKey identifies a row across rebuilds, so that the list keeps its
// place as rows come and go above it.
type gdRowKey struct {
	kind     gdRowKind
	file     string
	old, new int32
	gap      int32
}

// fileVisible reports whether a file shows: it passes the Changes filter
// and has something to show.
func (s *Shell) gdFileVisible(i int) bool {
	f := s.git.gdFiles[i]
	if !s.gdHasContent(f) {
		return false
	}
	return gdFuzzyMatch(f.cf.Path, strings.TrimSpace(s.changes.filter))
}

// gdHasContent reports whether a file has something to show: files whose
// only changes were whitespace-only hunks, which git omits, have nothing.
func (s *Shell) gdHasContent(f *gdFile) bool {
	cf := f.cf
	return len(cf.Hunks) > 0 || cf.Binary || cf.TooLarge ||
		cf.Status != gitworkbench.StatusModified || (cf.OldPath != "" && cf.OldPath != cf.Path)
}

// gdFileNote returns why a file's lines do not show, "" when they do.
func (s *Shell) gdFileNote(f *gdFile) string {
	cf := f.cf
	switch {
	case cf.Binary:
		return "Binary file changed."
	case cf.TooLarge:
		return "File is too large, so the review skipped rendering it."
	case len(cf.Hunks) == 0 && cf.OldPath != "" && cf.OldPath != cf.Path:
		return "File renamed without changes."
	case len(cf.Hunks) == 0 && cf.Status == gitworkbench.StatusAdded, len(cf.Hunks) == 0 && cf.Status == gitworkbench.StatusUntracked:
		return "Empty file added."
	case len(cf.Hunks) == 0 && cf.Status == gitworkbench.StatusDeleted:
		return "Empty file deleted."
	case len(cf.Hunks) == 0:
		return "No changes to show."
	}
	return ""
}

// splitFile reports whether a file shows side by side: files that are
// all new or all gone show in one column.
func (s *Shell) gdSplitFile(f *gdFile) bool {
	if !s.git.splitLayout {
		return false
	}
	return !f.oneSided()
}

// gdStatusLetter is the tree's status letter (U for untracked, ! for
// conflicts, else git's letter).
func gdStatusLetter(cf *gitworkbench.ChangeFile) string {
	switch cf.Status {
	case gitworkbench.StatusUntracked:
		return "U"
	case gitworkbench.StatusConflicted:
		return "!"
	}
	return cf.Status.Letter()
}

// buildGdRows lays out the rows of the files.
func (s *Shell) buildGdRows() {
	rows := s.git.gdRows[:0]
	if cm := s.git.commitMeta; s.gdReviewingCommit() && cm != nil {
		rows = append(rows, gdRow{kind: gdRowCommit})
	}
	for fi, f := range s.git.gdFiles {
		if !s.gdFileVisible(fi) {
			continue
		}
		idx := int32(fi)
		rows = append(rows, gdRow{kind: gdRowHeader, file: idx})
		if f.collapsed {
			rows = append(rows, gdRow{kind: gdRowEnd, file: idx})
			continue
		}
		if note := s.gdFileNote(f); note != "" {
			rows = append(rows, gdRow{kind: gdRowNote, file: idx})
			rows = append(rows, gdRow{kind: gdRowEnd, file: idx})
			continue
		}
		f.computeWords()
		split := s.gdSplitFile(f)
		addGap := func(g gdGap) {
			if g.count <= 0 {
				return
			}
			shown := f.expanded[g.index]
			if !f.canExpand() {
				shown = gdGapShown{}
			} else if g.count <= gdInlineGap {
				shown = gdGapShown{top: g.count}
			}
			hidden := g.count - shown.top - shown.bottom
			if hidden <= 0 {
				shown = gdGapShown{top: g.count}
				hidden = 0
			}
			for k := range shown.top {
				rows = s.appendGdContext(rows, idx, f, int32(g.oldStart+k), int32(g.newStart+k))
			}
			if hidden > 0 {
				rows = append(rows, gdRow{kind: gdRowGap, file: idx, gap: int32(g.index), old: int32(g.oldStart + shown.top), new: int32(g.newStart + shown.top), count: int32(hidden)})
			}
			for k := g.count - shown.bottom; k < g.count; k++ {
				rows = s.appendGdContext(rows, idx, f, int32(g.oldStart+k), int32(g.newStart+k))
			}
		}
		for hi := range f.hunks() {
			addGap(f.gapBefore(hi))
			lines := f.hunks()[hi].Lines
			h := int32(hi)
			if !split {
				for li := range lines {
					rows = append(rows, gdRow{kind: gdRowLine, file: idx, hunk: h, a: int32(li), b: -1})
				}
				continue
			}
			for li := 0; li < len(lines); {
				if lines[li].Kind == gitworkbench.KindContext {
					rows = append(rows, gdRow{kind: gdRowLine, file: idx, hunk: h, a: int32(li), b: int32(li)})
					li++
					continue
				}
				start := li
				for li < len(lines) && lines[li].Kind == gitworkbench.KindDelete {
					li++
				}
				dels := li - start
				addStart := li
				for li < len(lines) && lines[li].Kind == gitworkbench.KindAdd {
					li++
				}
				adds := li - addStart
				for k := range max(dels, adds) {
					a, b := int32(-1), int32(-1)
					if k < dels {
						a = int32(start + k)
					}
					if k < adds {
						b = int32(addStart + k)
					}
					rows = append(rows, gdRow{kind: gdRowLine, file: idx, hunk: h, a: a, b: b})
				}
			}
		}
		if len(f.hunks()) > 0 {
			addGap(f.gapBefore(len(f.hunks())))
		}
		rows = append(rows, gdRow{kind: gdRowEnd, file: idx})
	}
	s.git.gdRows = rows
	s.git.gdRowsDirty = false
}

// appendGdContext adds a line of context the user expanded.
func (s *Shell) appendGdContext(rows []gdRow, idx int32, f *gdFile, old, new int32) []gdRow {
	return append(rows, gdRow{kind: gdRowLine, file: idx, hunk: -1, a: -1, b: -1, old: old, new: new})
}

// key returns the identity of a row.
func (s *Shell) gdRowIdentity(r *gdRow) gdRowKey {
	if r.kind == gdRowCommit {
		return gdRowKey{kind: gdRowCommit, file: s.git.commitHash}
	}
	f := s.git.gdFiles[r.file]
	k := gdRowKey{kind: r.kind, file: f.cf.Path, gap: r.gap}
	if r.kind == gdRowLine {
		if r.hunk < 0 {
			k.old, k.new = r.old, r.new
		} else {
			if l := f.lineAt(r.hunk, r.a); l != nil {
				k.old, k.new = int32(l.OldLine), int32(l.NewLine)
			}
			if l := f.lineAt(r.hunk, r.b); l != nil {
				if k.old == 0 {
					k.old = int32(l.OldLine)
				}
				k.new = int32(l.NewLine)
			}
			if k.old == 0 && k.new == 0 {
				k.gap = r.hunk<<16 | r.a
			}
		}
	}
	return k
}

// ensureGdListKeys gives the diff list its row identity and its sticky
// file headers, once.
func (s *Shell) ensureGdListKeys() {
	if s.git.gdList.Key != nil {
		return
	}
	s.git.gdList.Key = func(i int) any {
		if i < 0 || i >= len(s.git.gdRows) {
			return nil
		}
		return s.gdRowIdentity(&s.git.gdRows[i])
	}
	s.git.gdList.Header = func(i int) bool {
		return i >= 0 && i < len(s.git.gdRows) && s.git.gdRows[i].kind == gdRowHeader
	}
}

// rebuildDiffRows invalidates the godiff row model after state changes;
// the list rebuilds lazily at the next render (GWB-150: only the rows in
// view are ever built).
func (s *Shell) rebuildDiffRows() {
	if s.git == nil {
		return
	}
	s.gdSetFiles()
	s.git.gdRowsDirty = true
}

// revealDiffFile scrolls the surface so path's card is at top (GWB-180),
// opening the center→right oscillation guard window.
func (s *Shell) revealDiffFile(path string) {
	if s.git == nil || path == "" {
		return
	}
	if _, i := s.gdFileForPath(path); i >= 0 {
		s.gdRevealFile(i)
	}
}

// selectedChangeFileInfo returns the ChangeFile for any path.
func (s *Shell) selectedChangeFileInfo(path string) (gitworkbench.ChangeFile, bool) {
	if s.git == nil || s.git.snapshot == nil {
		return gitworkbench.ChangeFile{}, false
	}
	return s.git.snapshot.ByPath(path)
}

// gdRevealFile scrolls the surface to a file's card.
func (s *Shell) gdRevealFile(i int) {
	if s.git.gdRowsDirty {
		s.buildGdRows()
	}
	for r := range s.git.gdRows {
		if s.git.gdRows[r].kind == gdRowHeader && int(s.git.gdRows[r].file) == i {
			s.git.gdList.ScrollTo(r, ui.Start)
			break
		}
	}
	s.git.gdCurrent = i
	s.git.revealAt = timeNow()
	s.git.revealPath = s.git.gdFiles[i].cf.Path
	s.surface.diff.SelectedPath = s.git.gdFiles[i].cf.Path
}

// gdFileForPath returns the review file of a path.
func (s *Shell) gdFileForPath(path string) (*gdFile, int) {
	for i, f := range s.git.gdFiles {
		if f.cf.Path == path {
			return f, i
		}
	}
	return nil, -1
}
