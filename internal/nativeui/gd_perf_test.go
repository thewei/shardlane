package nativeui

/**
 * [INPUT]: 合成的大 diff 快照（多文件 / 单个超大文件）、官方 MyGo headless Tester
 * [OUTPUT]: 首次构建行模型与逐帧渲染的耗时基准，以及"惰性计算"结构性回归（词级 diff / 度量 / 缓存不随总行数增长）
 * [POS]: Diff 大文件性能回归；预算数字（首次构建 ≤30 ms、帧 ≤16 ms）以基准为准，测试只断言结构
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// gdPerfFile builds one modified file of `pairs` replaced lines (each a
// delete/add pair with a changed word) separated by a few context lines.
func gdPerfFile(path string, pairs int) gitworkbench.ChangeFile {
	var lines []gitworkbench.DiffLine
	old, new := 1, 1
	for i := 0; i < pairs; i++ {
		if i%3 == 0 { // context
			lines = append(lines, gitworkbench.DiffLine{Kind: gitworkbench.KindContext, OldLine: old, NewLine: new, Text: fmt.Sprintf("\tcontext line %d of %s", i, path)})
			old++
			new++
		}
		lines = append(lines,
			gitworkbench.DiffLine{Kind: gitworkbench.KindDelete, OldLine: old, Text: fmt.Sprintf("\tvalue := compute(%d, alpha, beta) // old %d", i, i)},
			gitworkbench.DiffLine{Kind: gitworkbench.KindAdd, NewLine: new, Text: fmt.Sprintf("\tvalue := compute(%d, alpha, gamma) // new %d", i, i)},
		)
		old++
		new++
	}
	h := gitworkbench.Hunk{OldStart: 1, OldLines: old - 1, NewStart: 1, NewLines: new - 1, Lines: lines}
	return gitworkbench.ChangeFile{
		Path: path, Status: gitworkbench.StatusModified, Additions: pairs, Deletions: pairs,
		Fingerprint: "fp-" + path, Hunks: []gitworkbench.Hunk{h}, Unstaged: true,
	}
}

func gdPerfShell(t testing.TB, files, pairs int) *Shell {
	t.Helper()
	root := t.TempDir()
	shell := gitSurfaceTestShell(t, root)
	shell.git.rootOf[root] = root
	shell.git.refreshing = true
	snap := shell.git.snapshot
	snap.Files = nil
	for i := 0; i < files; i++ {
		snap.Files = append(snap.Files, gdPerfFile(fmt.Sprintf("pkg/f%03d.go", i), pairs))
	}
	if shell.router.Path() != routeWorkspace {
		shell.router.Push(routeWorkspace)
	}
	return shell
}

// TestGdPerfBaseline prints the timings the lazy work is measured by. It
// never fails on time: CI machines vary, and the structural tests below
// carry the regression guarantee.
func TestGdPerfBaseline(t *testing.T) {
	for _, tc := range []struct {
		name         string
		files, pairs int
	}{
		{"one-huge-file", 1, 25000},
		{"many-files", 300, 200},
	} {
		shell := gdPerfShell(t, tc.files, tc.pairs)
		shell.showSurface(WorkspaceSurfaceDiff)
		start := time.Now()
		shell.rebuildDiffRows()
		shell.buildGdRows()
		build := time.Since(start)
		start = time.Now()
		tester := ui.NewTester(shell.View, 1340, 800) // renders the first frame
		first := time.Since(start)
		var worst, total time.Duration
		const frames = 20
		for i := 0; i < frames; i++ {
			start = time.Now()
			tester.Frame()
			d := time.Since(start)
			total += d
			worst = max(worst, d)
		}
		t.Logf("%-14s rows=%d build=%v firstFrame=%v steady avg=%v worst=%v", tc.name, len(shell.git.gdRows), build, first, total/frames, worst)
	}
}

// A diff pays for word marks only on the lines it draws: building the rows
// computes none, a frame computes the visible pairs, and the marks equal what
// the eager computation produced.
func TestGdWordMarksAreComputedLazily(t *testing.T) {
	shell := gdPerfShell(t, 1, 5000)
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	shell.buildGdRows()
	f := shell.git.gdFiles[0]
	if n := len(f.words); n != 0 {
		t.Fatalf("building rows computed %d word entries up front", n)
	}
	tester := ui.NewTester(shell.View, 1340, 800)
	tester.Frame()
	if n := len(f.words); n == 0 || n > 400 {
		t.Fatalf("a frame computed %d word entries; want the drawn lines only (1..400)", n)
	}
	lines := f.hunks()[0].Lines
	pairs := gdPairs(lines)
	p := pairs[len(pairs)/2]
	wantOld, wantNew, ok := gitworkbench.WordDiff(lines[p.del].Text, lines[p.add].Text)
	if !ok {
		t.Fatal("fixture pair must produce word marks")
	}
	if got := f.wordsAt(0, p.del); !reflect.DeepEqual(got, wantOld) {
		t.Fatalf("old side marks = %v, want %v", got, wantOld)
	}
	if got := f.wordsAt(0, p.add); !reflect.DeepEqual(got, wantNew) {
		t.Fatalf("new side marks = %v, want %v", got, wantNew)
	}
	if got := f.wordsAt(0, 0); got != nil && lines[0].Kind == gitworkbench.KindContext {
		t.Fatalf("a context line has no marks, got %v", got)
	}
}

// What is derived from the shown hunks goes when the shown hunks change.
func TestGdViewSwitchDropsDerivedCaches(t *testing.T) {
	cf := gdPerfFile("a.go", 20)
	cf.StagedHunks = cf.Hunks
	f := &gdFile{cf: &cf}
	pairs := gdPairs(f.hunks()[0].Lines)
	_ = f.wordsAt(0, pairs[0].del)
	_ = f.metrics()
	f.spans = map[gdSpanKey][]ui.Span{{}: nil}
	if len(f.words) == 0 || !f.metricsDone {
		t.Fatal("fixture did not derive anything")
	}
	f.setView(gdViewCombined) // same side: nothing to drop
	if len(f.words) == 0 || !f.metricsDone || len(f.spans) == 0 {
		t.Fatal("re-selecting the same side dropped its caches")
	}
	f.setView(gdViewStaged)
	if f.words != nil || f.pairs != nil || f.spans != nil || f.metricsDone {
		t.Fatal("switching sides kept caches derived from the other side's hunks")
	}
}

// Metrics are cached even before the contents arrive, so a frame never walks
// the file's lines once per drawn row; loading the contents refreshes them.
func TestGdMetricsAreComputedOncePerContentLoad(t *testing.T) {
	cf := gdPerfFile("a.go", 20)
	f := &gdFile{cf: &cf}
	m := f.metrics()
	if !f.metricsDone {
		t.Fatal("an unloaded file's metrics must be cached too")
	}
	cf.Hunks[0].Lines[0].Text = strings.Repeat("x", 500) // would widen the file if it were recomputed
	if f.metrics() != m {
		t.Fatal("metrics were recomputed on a second call")
	}
	(&gdLoaded{file: f, newLines: []string{strings.Repeat("y", 300)}}).apply()
	if got := f.metrics().maxCols; got < 300 {
		t.Fatalf("metrics after the contents loaded = %d columns, want at least 300", got)
	}
}

// A snapshot refresh keeps what was derived for the files that did not change
// and drops it for those that did, or that show a staged/unstaged side whose
// hunks the fingerprint does not cover.
func TestGdSetFilesCarriesDerivedCaches(t *testing.T) {
	shell := gdPerfShell(t, 3, 30)
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	shell.buildGdRows()
	for i, f := range shell.git.gdFiles {
		_ = f.wordsAt(0, gdPairs(f.hunks()[0].Lines)[0].del)
		_ = f.metrics()
		f.spans = map[gdSpanKey][]ui.Span{{}: nil}
		if i == 2 {
			f.setView(gdViewUnstaged)
			_ = f.wordsAt(0, gdPairs(f.hunks()[0].Lines)[0].del)
		}
	}
	next := *shell.git.snapshot
	next.Files = append([]gitworkbench.ChangeFile(nil), shell.git.snapshot.Files...)
	next.Files[1].Fingerprint = "changed"
	shell.git.snapshot = &next
	shell.rebuildDiffRows()
	kept, changed, sided := shell.git.gdFiles[0], shell.git.gdFiles[1], shell.git.gdFiles[2]
	if len(kept.words) == 0 || !kept.metricsDone || len(kept.spans) == 0 {
		t.Fatalf("an unchanged file lost its derived caches: words=%d metrics=%v spans=%d", len(kept.words), kept.metricsDone, len(kept.spans))
	}
	if changed.words != nil || changed.metricsDone || changed.spans != nil {
		t.Fatal("a changed file kept caches derived from its old hunks")
	}
	if sided.view != gdViewUnstaged {
		t.Fatalf("the shown side did not carry: %v", sided.view)
	}
	if sided.words != nil || sided.metricsDone || sided.spans != nil {
		t.Fatal("a staged/unstaged view kept caches the fingerprint cannot vouch for")
	}
}

// Contents load nearest-first around the file in view; loaded files and
// collapsed ones (which may never open) are not read.
func TestGdLoadOrderStartsNearTheFileInView(t *testing.T) {
	shell := gdPerfShell(t, 6, 5)
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	files := shell.git.gdFiles
	files[1].loaded = true
	files[4].collapsed = true
	shell.git.gdCurrent = 3
	var got []string
	for _, f := range shell.gdLoadOrder() {
		got = append(got, f.cf.Path)
	}
	want := []string{"pkg/f003.go", "pkg/f002.go", "pkg/f005.go", "pkg/f000.go"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("load order = %v, want %v", got, want)
	}
}

// BenchmarkGdBuildRows is the cost of laying out the row model of one huge
// file (58k rows): the work of opening or refreshing a large review.
func BenchmarkGdBuildRows(b *testing.B) {
	shell := gdPerfShell(b, 1, 25000)
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	for b.Loop() {
		shell.git.gdRowsDirty = true
		shell.buildGdRows()
	}
}

// BenchmarkGdFrame is the steady frame of that same file scrolled to the top.
func BenchmarkGdFrame(b *testing.B) {
	shell := gdPerfShell(b, 1, 25000)
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	tester := ui.NewTester(shell.View, 1340, 800)
	tester.Frame()
	for b.Loop() {
		tester.Frame()
	}
}
