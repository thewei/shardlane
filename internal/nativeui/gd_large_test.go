package nativeui

/**
 * [INPUT]: 真实 Git 仓库里的超大 diff（Snapshot 标记 Deferred）、headless Tester、uiApplyOverride 队列
 * [OUTPUT]: 占位 → Show → 按需读取 → 渲染 的端到端回归；失败重试 / 过大 / 快照刷新保留语义
 * [POS]: Diff 大文件占位交互；后端读取语义由 gitworkbench/large_diff_test.go 固定
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"errors"
	"fmt"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

func gdLargeLines(prefix string, n int) string {
	var b strings.Builder
	for i := 0; i < n; i++ {
		fmt.Fprintf(&b, "%s %d\n", prefix, i)
	}
	return b.String()
}

// gdLargeShell opens the Diff surface on a real repository whose big.txt is
// rewritten in every line (deferred) next to a small, ordinary change.
func gdLargeShell(t *testing.T) (*Shell, chan func()) {
	t.Helper()
	for _, kv := range [][2]string{{"GIT_AUTHOR_NAME", "test"}, {"GIT_AUTHOR_EMAIL", "test@example.com"}, {"GIT_COMMITTER_NAME", "test"}, {"GIT_COMMITTER_EMAIL", "test@example.com"}} {
		t.Setenv(kv[0], kv[1])
	}
	root := tempGitRepo(t)
	n := gitworkbench.LargeDiffLines
	writeRepoFile(t, root, "big.txt", gdLargeLines("old", n))
	writeRepoFile(t, root, "small.txt", "a\nb\n")
	gitScript(t, root, []string{"add", "."}, []string{"commit", "-q", "-m", "seed"})
	writeRepoFile(t, root, "big.txt", gdLargeLines("new", n))
	writeRepoFile(t, root, "small.txt", "a\nc\n")

	shell := gitSurfaceTestShell(t, root)
	snap, err := shell.git.runner.Snapshot(t.Context(), root)
	if err != nil {
		t.Fatal(err)
	}
	shell.git.snapshot = snap
	shell.git.rootOf[root] = root
	shell.git.refreshing = true
	queue := make(chan func(), 16)
	shell.uiApplyOverride = func(fn func()) { queue <- fn }
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	shell.buildGdRows()
	return shell, queue
}

func gdLargeFile(t *testing.T, s *Shell, path string) *gdFile {
	t.Helper()
	f, _ := s.gdFileForPath(path)
	if f == nil {
		t.Fatalf("%s is not in the review", path)
	}
	return f
}

// settle applies queued completions until the file leaves the loading state.
func gdLargeSettle(t *testing.T, f *gdFile, queue chan func()) {
	t.Helper()
	deadline := time.After(20 * time.Second)
	for f.large.state == gdLargeLoading {
		select {
		case apply := <-queue:
			apply()
		case <-deadline:
			t.Fatal("the on-demand read never completed")
		}
	}
}

// The review shows a large diff as one line with Show; Show reads it, then the
// lines take the place of the note.
func TestLargeDiffWaitsForShowThenLoads(t *testing.T) {
	s, queue := gdLargeShell(t)
	n := gitworkbench.LargeDiffLines
	big := gdLargeFile(t, s, "big.txt")
	if !big.cf.Deferred || len(big.hunks()) != 0 {
		t.Fatalf("big.txt deferred=%v hunks=%d, want a placeholder", big.cf.Deferred, len(big.hunks()))
	}

	tester := ui.NewTester(s.View, 1340, 800)
	tester.Frame()
	texts := tester.Texts()
	if !containsText(texts, "Large diff") || !containsText(texts, "Show") {
		t.Fatalf("placeholder missing its note or Show button: %q", texts)
	}
	if !tester.HasText("+" + gdThousands(n)) {
		t.Fatalf("the file header keeps the counts of a deferred diff: %q", texts)
	}
	var lineRows int
	for _, r := range s.git.gdRows {
		if r.kind == gdRowLine {
			lineRows++
		}
	}
	if lineRows > 4 {
		t.Fatalf("a deferred file contributed %d line rows", lineRows)
	}

	if err := tester.Click("Show"); err != nil {
		t.Fatal(err)
	}
	if big.large.state != gdLargeLoading {
		t.Fatalf("state after Show = %v, want loading", big.large.state)
	}
	gdLargeSettle(t, big, queue)
	if big.large.state != gdLargeShown || big.cf.Deferred {
		t.Fatalf("state=%v deferred=%v after the read", big.large.state, big.cf.Deferred)
	}
	if got := len(big.hunks()[0].Lines); got != 2*n {
		t.Fatalf("shown lines = %d, want %d", got, 2*n)
	}
	s.buildGdRows()
	lineRows = 0
	_, bigIndex := s.gdFileForPath("big.txt")
	for _, r := range s.git.gdRows {
		if r.kind == gdRowLine && int(r.file) == bigIndex {
			lineRows++
		}
	}
	if lineRows < n {
		t.Fatalf("line rows after Show = %d, want at least %d", lineRows, n)
	}
	tester.Frame()
	if containsText(tester.Texts(), "Show") {
		t.Fatal("the Show button outlived the placeholder")
	}
}

// A failed read says so and offers Retry, which reads again.
func TestLargeDiffFailureOffersRetry(t *testing.T) {
	s, queue := gdLargeShell(t)
	big := gdLargeFile(t, s, "big.txt")
	realRoot := s.git.root
	s.git.root = t.TempDir() // not a repository: the read fails

	tester := ui.NewTester(s.View, 1340, 800)
	tester.Frame()
	if err := tester.Click("Show"); err != nil {
		t.Fatal(err)
	}
	gdLargeSettle(t, big, queue)
	if big.large.state != gdLargeFailed || big.large.err == "" {
		t.Fatalf("state=%v err=%q, want a failure", big.large.state, big.large.err)
	}
	tester.Frame()
	texts := tester.Texts()
	if !containsText(texts, "Retry") || containsText(texts, "Show") {
		t.Fatalf("a failed file offers Retry in place of Show: %q", texts)
	}
	if !tester.HasText("Couldn't load the diff") {
		t.Fatalf("the failure copy is missing: %q", texts)
	}

	s.git.root = realRoot
	if err := tester.Click("Retry"); err != nil {
		t.Fatal(err)
	}
	gdLargeSettle(t, big, queue)
	if big.large.state != gdLargeShown {
		t.Fatalf("state after Retry = %v, want shown", big.large.state)
	}
}

// Past the on-demand limit there is nothing to offer: no Show, no Retry.
func TestLargeDiffTooLargeOffersNothing(t *testing.T) {
	s, _ := gdLargeShell(t)
	big := gdLargeFile(t, s, "big.txt")
	big.finishLarge(gitworkbench.FileDiff{}, &gitworkbench.Error{Class: gitworkbench.ErrTooLarge, OpClass: gitworkbench.OpRead, Op: "diff"})
	if big.large.state != gdLargeTooLarge {
		t.Fatalf("state = %v, want too large", big.large.state)
	}
	s.git.gdRowsDirty = true
	tester := ui.NewTester(s.View, 1340, 800)
	tester.Frame()
	texts := tester.Texts()
	if containsText(texts, "Show") || containsText(texts, "Retry") {
		t.Fatalf("a too-large diff offers an action: %q", texts)
	}
	if !tester.HasText("Too large") {
		t.Fatalf("the too-large note is missing: %q", texts)
	}
	if got := s.gdFileNote(big); !strings.HasPrefix(got, "Too large") {
		t.Fatalf("note = %q", got)
	}
}

// What a snapshot refresh keeps: a shown file that did not change stays
// shown; one that changed, or whose staging changed, asks again.
func TestLargeDiffSurvivesSnapshotRefresh(t *testing.T) {
	s, queue := gdLargeShell(t)
	big := gdLargeFile(t, s, "big.txt")
	tester := ui.NewTester(s.View, 1340, 800)
	tester.Frame()
	if err := tester.Click("Show"); err != nil {
		t.Fatal(err)
	}
	gdLargeSettle(t, big, queue)

	refresh := func(edit func(f *gitworkbench.ChangeFile)) *gdFile {
		next := *s.git.snapshot
		next.Files = slices.Clone(s.git.snapshot.Files)
		for i := range next.Files {
			if next.Files[i].Path == "big.txt" {
				edit(&next.Files[i])
			}
		}
		s.git.snapshot = &next
		s.rebuildDiffRows()
		return gdLargeFile(t, s, "big.txt")
	}

	kept := refresh(func(*gitworkbench.ChangeFile) {})
	if kept.large.state != gdLargeShown || kept.cf.Deferred || len(kept.hunks()) == 0 {
		t.Fatalf("an unchanged file lost its diff: state=%v deferred=%v", kept.large.state, kept.cf.Deferred)
	}

	edited := refresh(func(f *gitworkbench.ChangeFile) { f.Fingerprint = "edited" })
	if edited.large.state != gdLargeIdle || !edited.cf.Deferred || len(edited.hunks()) != 0 {
		t.Fatalf("an edited file kept a stale diff: state=%v deferred=%v hunks=%d", edited.large.state, edited.cf.Deferred, len(edited.hunks()))
	}

	// Show it again, then change the staging: the per-side hunks are stale.
	s.gdShowLarge(edited)
	gdLargeSettle(t, edited, queue)
	restaged := refresh(func(f *gitworkbench.ChangeFile) { f.Staged = !f.Staged })
	if restaged.large.state != gdLargeIdle || !restaged.cf.Deferred {
		t.Fatalf("a restaged file kept per-side hunks: state=%v deferred=%v", restaged.large.state, restaged.cf.Deferred)
	}
}

// A deferred file shows its known counts in the staged and unstaged views too.
func TestDeferredSideCountsUseTheKnownTotals(t *testing.T) {
	s, _ := gdLargeShell(t)
	big := gdLargeFile(t, s, "big.txt")
	big.setView(gdViewStaged)
	adds, dels := big.sideCounts()
	if adds != big.cf.Additions || dels != big.cf.Deletions || adds == 0 {
		t.Fatalf("staged-view counts = +%d -%d, want the file's +%d -%d", adds, dels, big.cf.Additions, big.cf.Deletions)
	}
}

// A deferred file is not read for its colors until it is shown.
func TestLoadOrderSkipsDeferredFilesUntilShown(t *testing.T) {
	s, queue := gdLargeShell(t)
	big := gdLargeFile(t, s, "big.txt")
	for _, f := range s.gdLoadOrder() {
		if f == big {
			t.Fatal("a deferred file was queued for content loading")
		}
	}
	tester := ui.NewTester(s.View, 1340, 800)
	tester.Frame()
	if err := tester.Click("Show"); err != nil {
		t.Fatal(err)
	}
	gdLargeSettle(t, big, queue)
	if !slices.Contains(s.gdLoadOrder(), big) {
		t.Fatal("a shown file must be queued for content loading")
	}
}

func TestLargeDiffFinishClassifiesErrors(t *testing.T) {
	f := &gdFile{cf: &gitworkbench.ChangeFile{Path: "x", Deferred: true}}
	f.finishLarge(gitworkbench.FileDiff{}, errors.New("git diff: signal: killed\nmore"))
	if f.large.state != gdLargeFailed || f.large.err != "git diff: signal: killed" {
		t.Fatalf("state=%v err=%q, want a one-line failure", f.large.state, f.large.err)
	}
	f.finishLarge(gitworkbench.FileDiff{}, &gitworkbench.Error{Class: gitworkbench.ErrTimeout, OpClass: gitworkbench.OpRead, Op: "diff"})
	if f.large.state != gdLargeFailed || f.large.err != "timed out" {
		t.Fatalf("state=%v err=%q, want a timeout", f.large.state, f.large.err)
	}
	f.finishLarge(gitworkbench.FileDiff{Hunks: []gitworkbench.Hunk{{}}}, nil)
	if f.large.state != gdLargeShown || f.cf.Deferred || len(f.cf.Hunks) != 1 {
		t.Fatalf("state=%v deferred=%v hunks=%d after a good read", f.large.state, f.cf.Deferred, len(f.cf.Hunks))
	}
}
