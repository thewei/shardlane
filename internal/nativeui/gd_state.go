package nativeui

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/codehl"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// Godiff-style review state, ported from egoist/godiff rows.go/window.go
// (commit 88b89e0): per-file view state, background content loading with
// batched applies, and the row model built lazily at render.

// gdViewSide is which side of a change the review shows: the combined
// HEAD-vs-worktree parse, or one of the per-side parses.
type gdViewSide uint8

const (
	gdViewCombined gdViewSide = iota
	gdViewStaged
	gdViewUnstaged
)

// gdFile is a changed file as the review shows it: its change, the
// contents of both sides once loaded, and what the user did to it.
type gdFile struct {
	cf *gitworkbench.ChangeFile
	// view picks the hunks the surface shows: combined, staged-only or
	// unstaged-only, set from the sidebar section the file was opened from.
	view gdViewSide
	// The lines of the old and the new file, and their tokens, once loaded;
	// loaded is set then, even for files without contents.
	oldLines, newLines []string
	oldHL, newHL       [][]codehl.Seg
	loaded             bool
	// collapsed hides the file's lines.
	collapsed bool
	// loadGen is the content-load generation that asked for this file's
	// contents, so a file is requested once per generation.
	loadGen uint64
	// large is the on-demand state of a deferred file (idle when it is not).
	large gdLarge
	// expanded is how many lines of each gap of unchanged lines show, from
	// its top and its bottom.
	expanded map[int]gdGapShown
	// words memoizes the changed ranges of lines replaced by others, by the
	// index of the hunk and of the line in it. A pair is computed when one of
	// its lines is first drawn, never for the whole diff; pairs holds, per
	// hunk, the partner of each line (-1 for none).
	words map[[2]int][]gitworkbench.WordRange
	pairs [][]int32
	// metric is computed once per change of the shown hunks or contents.
	metric      gdFileMetrics
	metricsDone bool
	// spans keeps the styled code of the lines shown, which does not change
	// from frame to frame.
	spans map[gdSpanKey][]ui.Span
}

// gdLargeState is where a deferred file stands on its way to being shown.
type gdLargeState uint8

const (
	gdLargeIdle     gdLargeState = iota // behind Show
	gdLargeLoading                      // the on-demand read runs
	gdLargeFailed                       // the read failed; Retry
	gdLargeTooLarge                     // past the on-demand limit: nothing to offer
	gdLargeShown                        // its hunks arrived
)

// gdLarge is the on-demand state of a deferred file.
type gdLarge struct {
	state gdLargeState
	err   string                // why the read failed, on one line
	diff  gitworkbench.FileDiff // what it read, kept to carry over a refresh
}

// gdSpanKey identifies the code of a line on one side, in the light or the
// dark.
type gdSpanKey struct {
	hunk, index int32
	num         int32
	side        gdSide
	dark        bool
}

type gdGapShown struct{ top, bottom int }

// gdSide is which side of a split row a line shows on.
type gdSide uint8

const (
	gdSideOld gdSide = iota
	gdSideNew
)

// gdFileMetrics are the widths a file's rows share: of its line numbers,
// and how far its lines go.
type gdFileMetrics struct {
	digits  int
	maxCols int
}

// gdMaxContent is the size of the files whose contents are loaded.
const gdMaxContent = 2 << 20

// gdExpandStep is how many lines a click on a gap's arrows shows, and
// gdInlineGap the most unchanged lines between hunks shown without asking.
const (
	gdExpandStep = 100
	gdInlineGap  = 12
)

// hunks returns the hunks the review shows: the selected side's parse when
// it carries one, else the combined parse.
func (f *gdFile) hunks() []gitworkbench.Hunk {
	switch f.view {
	case gdViewStaged:
		if f.cf.StagedHunks != nil {
			return f.cf.StagedHunks
		}
	case gdViewUnstaged:
		if f.cf.UnstagedHunks != nil {
			return f.cf.UnstagedHunks
		}
	}
	return f.cf.Hunks
}

// sideCounts are the +/− totals of the hunks the review shows.
func (f *gdFile) sideCounts() (adds, dels int) {
	if f.view == gdViewCombined || f.cf.Deferred {
		return f.cf.Additions, f.cf.Deletions
	}
	for _, h := range f.hunks() {
		for _, l := range h.Lines {
			switch l.Kind {
			case gitworkbench.KindAdd:
				adds++
			case gitworkbench.KindDelete:
				dels++
			}
		}
	}
	return adds, dels
}

// oneSided reports whether the file is all new or all gone, with lines on
// one side only.
func (f *gdFile) oneSided() bool {
	switch f.cf.Status {
	case gitworkbench.StatusAdded, gitworkbench.StatusUntracked, gitworkbench.StatusDeleted:
		return true
	}
	return false
}

// lineAt returns the line of a hunk, nil for -1.
func (f *gdFile) lineAt(hunk, i int32) *gitworkbench.DiffLine {
	if hunk < 0 || i < 0 || int(hunk) >= len(f.hunks()) {
		return nil
	}
	lines := f.hunks()[hunk].Lines
	if int(i) >= len(lines) {
		return nil
	}
	return &lines[i]
}

// gdGap is a run of unchanged lines between hunks, or before the first or
// after the last.
type gdGap struct {
	index              int
	oldStart, newStart int // the numbers of its first lines
	count              int
}

// gdLastLines returns the numbers of the last lines of a hunk on each side.
func gdLastLines(h *gitworkbench.Hunk) (int, int) {
	o := h.OldStart + h.OldLines - 1
	if h.OldLines == 0 {
		o = h.OldStart
	}
	n := h.NewStart + h.NewLines - 1
	if h.NewLines == 0 {
		n = h.NewStart
	}
	return o, n
}

// gapBefore returns the gap before hunk i; i == len(Hunks) is the gap
// after the last, known once the contents are loaded.
func (f *gdFile) gapBefore(i int) gdGap {
	prevOld, prevNew := 0, 0
	if i > 0 {
		prevOld, prevNew = gdLastLines(&f.hunks()[i-1])
	}
	g := gdGap{index: i, oldStart: prevOld + 1, newStart: prevNew + 1}
	if i < len(f.hunks()) {
		h := &f.hunks()[i]
		first := h.OldStart
		if h.OldLines == 0 {
			first = h.OldStart + 1
		}
		g.count = first - g.oldStart
		return g
	}
	switch {
	case f.newLines != nil:
		g.count = len(f.newLines) - prevNew
	case f.oldLines != nil:
		g.count = len(f.oldLines) - prevOld
	}
	return g
}

// contextText returns the text of an unchanged line, by its numbers.
func (f *gdFile) contextText(old, new int32) string {
	if new > 0 && int(new) <= len(f.newLines) {
		return f.newLines[new-1]
	}
	if old > 0 && int(old) <= len(f.oldLines) {
		return f.oldLines[old-1]
	}
	return ""
}

// canExpand reports whether gaps can show their lines.
func (f *gdFile) canExpand() bool {
	return f.newLines != nil || f.oldLines != nil
}

// wordsAt returns the changed ranges of one line of a shown hunk. Its
// delete/add pair is computed on the first request and kept, so a diff of
// any size pays only for the lines that are drawn.
func (f *gdFile) wordsAt(hunk, line int) []gitworkbench.WordRange {
	if r, ok := f.words[[2]int{hunk, line}]; ok {
		return r
	}
	partner := f.partnerOf(hunk, line)
	if partner < 0 {
		return nil
	}
	lines := f.hunks()[hunk].Lines
	del, add := line, partner
	if lines[line].Kind == gitworkbench.KindAdd {
		del, add = partner, line
	}
	ra, rb, ok := gitworkbench.WordDiff(lines[del].Text, lines[add].Text)
	if !ok {
		ra, rb = nil, nil
	}
	if f.words == nil {
		f.words = map[[2]int][]gitworkbench.WordRange{}
	}
	f.words[[2]int{hunk, del}] = ra
	f.words[[2]int{hunk, add}] = rb
	return f.words[[2]int{hunk, line}]
}

// partnerOf returns the index of the line a hunk's line is paired with, as a
// split view shows them side by side, or -1.
func (f *gdFile) partnerOf(hunk, line int) int {
	hunks := f.hunks()
	if hunk < 0 || hunk >= len(hunks) || line < 0 || line >= len(hunks[hunk].Lines) {
		return -1
	}
	if f.pairs == nil {
		f.pairs = make([][]int32, len(hunks))
	}
	if f.pairs[hunk] == nil {
		lines := hunks[hunk].Lines
		partners := make([]int32, len(lines))
		for i := range partners {
			partners[i] = -1
		}
		for _, p := range gdPairs(lines) {
			partners[p.del], partners[p.add] = int32(p.add), int32(p.del)
		}
		f.pairs[hunk] = partners
	}
	return int(f.pairs[hunk][line])
}

// setView picks which side's hunks the review shows. What was derived from
// the previous side's hunks (marks, styled lines, widths) goes with them.
func (f *gdFile) setView(view gdViewSide) {
	if f.view == view {
		return
	}
	f.view = view
	f.resetDerived()
}

// resetDerived drops everything computed from the shown hunks.
func (f *gdFile) resetDerived() {
	f.words, f.pairs, f.spans = nil, nil, nil
	f.metricsDone = false
}

// finishLarge records the outcome of a deferred file's on-demand read.
func (f *gdFile) finishLarge(d gitworkbench.FileDiff, err error) {
	if err == nil {
		f.adoptLarge(d)
		return
	}
	ge, _ := gitworkbench.GitError(err)
	switch {
	case ge != nil && ge.Class == gitworkbench.ErrTooLarge:
		f.large = gdLarge{state: gdLargeTooLarge}
	case ge != nil && ge.Class == gitworkbench.ErrTimeout:
		f.large = gdLarge{state: gdLargeFailed, err: "timed out"}
	default:
		f.large = gdLarge{state: gdLargeFailed, err: firstErrLine(err)}
	}
}

// adoptLarge shows a deferred file with the hunks an on-demand read produced.
// They go on a private copy of the change: the snapshot stays immutable.
func (f *gdFile) adoptLarge(d gitworkbench.FileDiff) {
	shown := *f.cf
	shown.Deferred = false
	shown.Hunks, shown.StagedHunks, shown.UnstagedHunks = d.Hunks, d.StagedHunks, d.UnstagedHunks
	f.cf = &shown
	f.large = gdLarge{state: gdLargeShown, diff: d}
	f.resetDerived()
}

// metrics computes the line-number digits and the widest line of the file,
// once: it runs for every drawn row, so it must stay a lookup. Loading the
// contents, expanding a gap or switching sides clears metricsDone.
func (f *gdFile) metrics() gdFileMetrics {
	if f.metricsDone {
		return f.metric
	}
	maxNum, maxCols := 0, 0
	for _, h := range f.hunks() {
		maxNum = max(maxNum, h.OldStart+h.OldLines, h.NewStart+h.NewLines)
		for _, l := range h.Lines {
			maxCols = max(maxCols, gdColumns(l.Text))
		}
	}
	maxNum = max(maxNum, len(f.oldLines), len(f.newLines))
	if f.canExpand() {
		for _, l := range f.newLines {
			maxCols = max(maxCols, gdColumns(l))
		}
		for _, l := range f.oldLines {
			maxCols = max(maxCols, gdColumns(l))
		}
	}
	f.metric = gdFileMetrics{digits: max(len(itoa(maxNum)), 2), maxCols: maxCols}
	f.metricsDone = true
	return f.metric
}

// gdIsViewed reports whether the user marked the file viewed, as it is now.
func (s *Shell) gdIsViewed(f *gdFile) bool {
	return f.cf.Fingerprint != "" && s.git.gdViewed[f.cf.Path] == f.cf.Fingerprint
}

// gdSetViewed marks a file viewed, which collapses it.
func (s *Shell) gdSetViewed(f *gdFile, viewed bool) {
	if viewed {
		s.git.gdViewed[f.cf.Path] = f.cf.Fingerprint
	} else {
		delete(s.git.gdViewed, f.cf.Path)
	}
	f.collapsed = viewed
	s.git.gdRowsDirty = true
}

// gdSourceKind is what the review shows.
type gdSourceKind uint8

const (
	gdSourceWorktree gdSourceKind = iota
	gdSourceCommit
)

// gdActiveSnapshot returns the snapshot the review draws from: the worktree
// snapshot, or the selected commit's.
func (s *Shell) gdActiveSnapshot() *gitworkbench.ChangesSnapshot {
	if s.git == nil {
		return nil
	}
	if s.git.source == gdSourceCommit {
		return s.git.commitSnap
	}
	return s.git.snapshot
}

// gdSetFiles shows the snapshot's files, keeping what the user did to those
// that did not change. Same pointer twice is a no-op, so the cached-snapshot
// apply path stays cheap.
func (s *Shell) gdSetFiles() {
	if s.git == nil {
		return
	}
	snap := s.gdActiveSnapshot()
	if snap == nil {
		return
	}
	if s.git.gdFilesFor == snap && s.git.gdFiles != nil {
		return
	}
	s.git.gdFilesFor = snap
	s.git.gdLoadGen.Add(1) // in-flight loads of the previous set are dropped
	prev := map[string]*gdFile{}
	for _, f := range s.git.gdFiles {
		prev[f.cf.Path] = f
	}
	next := make([]*gdFile, 0, len(snap.Files))
	for i := range snap.Files {
		cf := &snap.Files[i]
		nf := &gdFile{cf: cf}
		switch p := prev[cf.Path]; {
		case p != nil && p.cf.Fingerprint == cf.Fingerprint:
			// A large diff the user opened stays open while the file is as it
			// was and staged the same way (the per-side hunks come with it).
			if cf.Deferred && p.large.state == gdLargeShown && p.cf.Staged == cf.Staged && p.cf.Unstaged == cf.Unstaged {
				nf.adoptLarge(p.large.diff)
			}
			nf.view = p.view
			nf.collapsed = p.collapsed
			nf.expanded = p.expanded
			nf.oldLines, nf.newLines = p.oldLines, p.newLines
			nf.oldHL, nf.newHL = p.oldHL, p.newHL
			nf.loaded = p.loaded
			// The fingerprint vouches for the combined hunks only; the staged
			// and unstaged parses behind the other sides are not part of it.
			if p.view == gdViewCombined {
				nf.words, nf.pairs, nf.spans = p.words, p.pairs, p.spans
				nf.metric, nf.metricsDone = p.metric, p.metricsDone
			}
		default:
			nf.collapsed = s.gdIsViewed(nf) || cf.Generated
		}
		next = append(next, nf)
	}
	s.git.gdFiles = next
	s.git.gdRowsDirty = true
	s.git.gdCurrent = 0
	s.loadGdContents()
}

// gdLoaded is a file's contents, read and tokenized.
type gdLoaded struct {
	file               *gdFile
	oldLines, newLines []string
	oldHL, newHL       [][]codehl.Seg
	path, oldPath      string
}

// gdIsBinary reports whether content looks binary, as git decides: a NUL
// byte in its first 8000 bytes.
func gdIsBinary(content []byte) bool {
	n := min(len(content), 8000)
	return bytes.IndexByte(content[:n], 0) >= 0
}

// gdReadSide reads one side's lines, nil when it has none.
func gdReadSide(data []byte) []string {
	if data == nil || gdIsBinary(data) {
		return nil
	}
	lines := gdSplitLines(string(data))
	if lines == nil {
		lines = []string{}
	}
	return lines
}

// gdSplitLines splits text into lines without their line endings; a final
// newline does not start another line.
func gdSplitLines(s string) []string {
	if s == "" {
		return nil
	}
	s = strings.TrimSuffix(s, "\n")
	lines := strings.Split(s, "\n")
	for i, l := range lines {
		lines[i] = strings.TrimSuffix(l, "\r")
	}
	return lines
}

// gdLoadOrder lists the files whose contents still need reading: the ones
// nearest the file in view first (the later of two equally near ones first,
// as the review is read downward). Loaded files are done, collapsed ones wait
// until they open, and a deferred diff until it is shown, so a long review
// reads what is on screen before the rest.
func (s *Shell) gdLoadOrder() []*gdFile {
	var order []*gdFile
	index := map[*gdFile]int{}
	for i, f := range s.git.gdFiles {
		if f.loaded || f.cf.Deferred || f.collapsed && !s.gdForceOpen(i) {
			continue
		}
		index[f] = i
		order = append(order, f)
	}
	current := s.git.gdCurrent
	dist := func(f *gdFile) int {
		d := index[f] - current
		if d < 0 {
			d = -d
		}
		return d
	}
	slices.SortStableFunc(order, func(a, b *gdFile) int {
		if da, db := dist(a), dist(b); da != db {
			return da - db
		}
		return index[b] - index[a]
	})
	return order
}

// loadGdContents reads both sides of the files, for their colors and their
// unchanged lines, and shows them as they come (godiff loadContents, on the
// shell's guarded lanes).
func (s *Shell) loadGdContents() {
	if s.git == nil || s.git.root == "" || s.win == nil {
		return
	}
	pending := s.gdLoadOrder()
	if len(pending) == 0 {
		return
	}
	root := s.git.root
	head := s.git.snapshot.Head
	gen := s.git.gdLoadGen.Add(1)
	for _, f := range pending {
		f.loadGen = gen
	}

	go func() {
		jobs := make(chan *gdLoaded)
		results := make(chan gdLoaded)
		var wg sync.WaitGroup
		// A few workers: more would take the CPU from the main thread,
		// which draws the window.
		workers := min(max(runtime.NumCPU()/4, 1), 2)
		for range workers {
			wg.Add(1)
			go func() {
				defer wg.Done()
				for job := range jobs {
					job.oldHL = codehl.Lines(job.oldPath, job.oldLines)
					job.newHL = codehl.Lines(job.path, job.newLines)
					results <- *job
				}
			}()
		}
		go func() {
			wg.Wait()
			close(results)
		}()
		go func() {
			defer close(jobs)
			for _, f := range pending {
				if s.git == nil || s.git.gdLoadGen.Load() != gen {
					return
				}
				job := s.gdReadFile(root, head, f.cf)
				job.file = f
				jobs <- job
			}
		}()

		var batch []gdLoaded
		flush := func() {
			if len(batch) == 0 {
				return
			}
			done := batch
			batch = nil
			s.applyOnUI(func() {
				if s.git == nil || s.git.gdLoadGen.Load() != gen {
					return
				}
				for i := range done {
					done[i].apply()
				}
				s.git.gdRowsDirty = true
			})
		}
		tick := time.NewTicker(60 * time.Millisecond)
		defer tick.Stop()
		for {
			select {
			case l, ok := <-results:
				if !ok {
					flush()
					return
				}
				batch = append(batch, l)
			case <-tick.C:
				flush()
			}
		}
	}()
}

// gdShowLarge reads a deferred file's diff on demand, off the UI lane; Show
// and Retry both land here.
func (s *Shell) gdShowLarge(f *gdFile) {
	snap := s.gdActiveSnapshot()
	if s.git == nil || s.git.root == "" || snap == nil || !f.cf.Deferred || f.large.state == gdLargeLoading {
		return
	}
	f.large = gdLarge{state: gdLargeLoading}
	s.git.gdRowsDirty = true
	runner, root, base, files, cf := s.git.runner, s.git.root, snap.DiffBase, s.git.gdFilesFor, *f.cf
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), gitworkbench.OnDemandTimeout)
		defer cancel()
		diff, err := runner.ReadFileDiff(ctx, root, base, cf)
		s.applyOnUI(func() {
			// A refresh that replaced the files has dropped this one with them.
			if s.git == nil || s.git.gdFilesFor != files {
				return
			}
			f.finishLarge(diff, err)
			s.git.gdRowsDirty = true
		})
	}()
}

// gdReadFile reads both sides of one change: HEAD's lines through the
// bounded Git runner, the work tree's from disk.
func (s *Shell) gdReadFile(root, head string, cf *gitworkbench.ChangeFile) *gdLoaded {
	l := &gdLoaded{path: cf.Path, oldPath: cf.Path}
	if cf.OldPath != "" {
		l.oldPath = cf.OldPath
	}
	if cf.Binary || cf.TooLarge {
		return l
	}
	ctx, cancel := context.WithTimeout(context.Background(), gitworkbench.ReadTimeout)
	defer cancel()
	hasOld := head != "" && cf.Status != gitworkbench.StatusAdded && cf.Status != gitworkbench.StatusUntracked
	hasNew := cf.Status != gitworkbench.StatusDeleted
	if hasOld {
		if data, err := s.git.runner.Read(ctx, root, "show", head+":"+l.oldPath); err == nil && len(data) <= gdMaxContent {
			l.oldLines = gdReadSide(data)
		}
	}
	if hasNew {
		if data, err := os.ReadFile(filepath.Join(root, filepath.FromSlash(cf.Path))); err == nil && len(data) <= gdMaxContent {
			l.newLines = gdReadSide(data)
		}
	}
	return l
}

// apply gives the contents to their file, on the UI lane.
func (l *gdLoaded) apply() {
	f := l.file
	if f == nil {
		return
	}
	f.oldLines, f.newLines = l.oldLines, l.newLines
	f.oldHL, f.newHL = l.oldHL, l.newHL
	f.loaded = true
	f.metricsDone = false
	f.spans = nil
}
