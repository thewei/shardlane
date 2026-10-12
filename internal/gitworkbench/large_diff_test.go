package gitworkbench

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"
)

// numbered builds n lines "<prefix> <i>".
func numbered(prefix string, n int) string {
	var b strings.Builder
	for i := 0; i < n; i++ {
		fmt.Fprintf(&b, "%s %d\n", prefix, i)
	}
	return b.String()
}

// numberedWide is numbered with each line padded to about width bytes.
func numberedWide(prefix string, n, width int) string {
	var b strings.Builder
	pad := strings.Repeat("p", width)
	for i := 0; i < n; i++ {
		fmt.Fprintf(&b, "%s %d %s\n", prefix, i, pad)
	}
	return b.String()
}

func totalLines(hunks []Hunk) int {
	n := 0
	for _, h := range hunks {
		n += len(h.Lines)
	}
	return n
}

// seedLargeRepo commits big.txt (n "old" lines) and small.txt, then rewrites
// both in the working tree: big.txt changes in every line, small.txt in one.
func seedLargeRepo(t *testing.T, n int) (string, *Runner) {
	t.Helper()
	root, r := tempRepo(t)
	writeRepoFile(t, root, "big.txt", numbered("old", n))
	writeRepoFile(t, root, "small.txt", "a\nb\n")
	runGit(t, root, "add", ".")
	runGit(t, root, "commit", "-q", "-m", "seed")
	writeRepoFile(t, root, "big.txt", numbered("new", n))
	writeRepoFile(t, root, "small.txt", "a\nc\n")
	return root, r
}

// A file past LargeDiffLines changed lines waits behind a Show button: the
// counts stay, the patch and the hunks do not.
func TestSnapshotDefersLargeTrackedDiffs(t *testing.T) {
	n := LargeDiffLines
	root, r := seedLargeRepo(t, n)
	snap, err := r.Snapshot(context.Background(), root)
	if err != nil {
		t.Fatal(err)
	}
	big, ok := snap.ByPath("big.txt")
	if !ok {
		t.Fatal("big.txt missing from the snapshot")
	}
	if !big.Deferred || big.TooLarge {
		t.Fatalf("big.txt deferred=%v tooLarge=%v, want deferred only", big.Deferred, big.TooLarge)
	}
	if big.Additions != n || big.Deletions != n {
		t.Fatalf("big.txt counts = +%d -%d, want +%d -%d", big.Additions, big.Deletions, n, n)
	}
	if big.Hunks != nil || big.Patch != nil || big.StagedHunks != nil || big.UnstagedHunks != nil {
		t.Fatal("a deferred file kept its patch or hunks")
	}
	small, _ := snap.ByPath("small.txt")
	if small.Deferred || len(small.Hunks) != 1 {
		t.Fatalf("small.txt deferred=%v hunks=%d, want a parsed file", small.Deferred, len(small.Hunks))
	}
	if snap.TotalAdditions != n+1 || snap.TotalDeletions != n+1 {
		t.Fatalf("totals = +%d -%d, want +%d -%d", snap.TotalAdditions, snap.TotalDeletions, n+1, n+1)
	}
	if snap.DiffBase != "HEAD" {
		t.Fatalf("DiffBase = %q, want HEAD", snap.DiffBase)
	}
}

// Without a patch to digest, a deferred file's identity is its path, counts
// and the file's size and time: an edit that keeps the counts still moves it.
func TestDeferredFingerprintFollowsTheFile(t *testing.T) {
	n := LargeDiffLines
	root, r := seedLargeRepo(t, n)
	ctx := context.Background()
	first, _ := r.Snapshot(ctx, root)
	again, _ := r.Snapshot(ctx, root)
	a, _ := first.ByPath("big.txt")
	b, _ := again.ByPath("big.txt")
	if a.Fingerprint == "" || a.Fingerprint != b.Fingerprint {
		t.Fatalf("an untouched file changed identity: %q vs %q", a.Fingerprint, b.Fingerprint)
	}
	full := filepath.Join(root, "big.txt")
	writeRepoFile(t, root, "big.txt", numbered("NEW", n)) // same size, same counts
	if err := os.Chtimes(full, time.Now().Add(time.Hour), time.Now().Add(time.Hour)); err != nil {
		t.Fatal(err)
	}
	edited, _ := r.Snapshot(ctx, root)
	c, _ := edited.ByPath("big.txt")
	if c.Additions != a.Additions || c.Deletions != a.Deletions {
		t.Fatalf("fixture changed the counts: +%d -%d", c.Additions, c.Deletions)
	}
	if c.Fingerprint == a.Fingerprint {
		t.Fatal("editing a deferred file kept its fingerprint")
	}
}

// A patch past the per-file retention limit with few changed lines (long
// lines) is deferred too, where it used to be skipped for good.
func TestSnapshotDefersPatchesPastTheRetentionLimit(t *testing.T) {
	root, r := tempRepo(t)
	long := func(tag string) string {
		var b strings.Builder
		for i := 0; i < 30; i++ {
			fmt.Fprintf(&b, "%s %d %s\n", tag, i, strings.Repeat("x", 100<<10))
		}
		return b.String()
	}
	writeRepoFile(t, root, "wide.txt", long("old"))
	runGit(t, root, "add", ".")
	runGit(t, root, "commit", "-q", "-m", "seed")
	writeRepoFile(t, root, "wide.txt", long("new"))
	snap, err := r.Snapshot(context.Background(), root)
	if err != nil {
		t.Fatal(err)
	}
	wide, _ := snap.ByPath("wide.txt")
	if !wide.Deferred || wide.TooLarge {
		t.Fatalf("wide.txt deferred=%v tooLarge=%v, want deferred only", wide.Deferred, wide.TooLarge)
	}
	if wide.Additions != 30 || wide.Deletions != 30 {
		t.Fatalf("wide.txt counts = +%d -%d, want +30 -30", wide.Additions, wide.Deletions)
	}
	diff, err := r.ReadFileDiff(context.Background(), root, snap.DiffBase, wide)
	if err != nil {
		t.Fatal(err)
	}
	if got := totalLines(diff.Hunks); got != 60 {
		t.Fatalf("on-demand read returned %d lines, want 60", got)
	}
}

// One file with more diff than a read may return does not fail the whole
// snapshot: it is counted and left out of the bulk reads.
func TestSnapshotSurvivesADiffPastTheReadLimit(t *testing.T) {
	if testing.Short() {
		t.Skip("writes a 40 MiB diff")
	}
	n := 520_000 // ~22 MiB a side: the diff is past MaxReadOutput
	root, r := tempRepo(t)
	writeRepoFile(t, root, "big.txt", numberedWide("old", n, 30))
	writeRepoFile(t, root, "small.txt", "a\nb\n")
	runGit(t, root, "add", ".")
	runGit(t, root, "commit", "-q", "-m", "seed")
	writeRepoFile(t, root, "big.txt", numberedWide("new", n, 30))
	writeRepoFile(t, root, "small.txt", "a\nc\n")
	snap, err := r.Snapshot(context.Background(), root)
	if err != nil {
		t.Fatalf("snapshot failed: %v", err)
	}
	if snap.Err != "" {
		t.Fatalf("snapshot error: %s", snap.Err)
	}
	big, _ := snap.ByPath("big.txt")
	if !big.Deferred || big.Additions != n {
		t.Fatalf("big.txt deferred=%v additions=%d, want deferred with %d", big.Deferred, big.Additions, n)
	}
	small, _ := snap.ByPath("small.txt")
	if small.Deferred || len(small.Hunks) != 1 {
		t.Fatalf("small.txt must still parse: deferred=%v hunks=%d", small.Deferred, len(small.Hunks))
	}
}

// When even the files under the threshold add up to more than a read may
// return, nothing is read in bulk: every text file waits behind Show.
func TestSnapshotDefersEverythingWhenTheBulkReadOverflows(t *testing.T) {
	if testing.Short() {
		t.Skip("writes a 40 MiB diff")
	}
	root, r := tempRepo(t)
	wide := func(tag string) string {
		var b strings.Builder
		line := strings.Repeat("y", 3<<10)
		for i := 0; i < 2400; i++ {
			fmt.Fprintf(&b, "%s %d %s\n", tag, i, line)
		}
		return b.String()
	}
	for _, name := range []string{"a.txt", "b.txt", "c.txt"} {
		writeRepoFile(t, root, name, wide("old"))
	}
	writeRepoFile(t, root, "tiny.txt", "a\nb\n")
	runGit(t, root, "add", ".")
	runGit(t, root, "commit", "-q", "-m", "seed")
	for _, name := range []string{"a.txt", "b.txt", "c.txt"} {
		writeRepoFile(t, root, name, wide("new"))
	}
	writeRepoFile(t, root, "tiny.txt", "a\nc\n")
	snap, err := r.Snapshot(context.Background(), root)
	if err != nil {
		t.Fatalf("snapshot failed: %v", err)
	}
	for _, f := range snap.Files {
		if !f.Deferred || f.Hunks != nil {
			t.Fatalf("%s deferred=%v hunks=%d, want deferred", f.Path, f.Deferred, len(f.Hunks))
		}
	}
	if tiny, _ := snap.ByPath("tiny.txt"); tiny.Additions != 1 || tiny.Deletions != 1 {
		t.Fatalf("a deferred file keeps its counts: +%d -%d", tiny.Additions, tiny.Deletions)
	}
}

// ReadFileDiff reads what was deferred: the combined hunks, plus each side's
// when changes sit on both.
func TestReadFileDiffTracked(t *testing.T) {
	n := LargeDiffLines
	root, r := seedLargeRepo(t, n)
	ctx := context.Background()
	snap, _ := r.Snapshot(ctx, root)
	big, _ := snap.ByPath("big.txt")
	diff, err := r.ReadFileDiff(ctx, root, snap.DiffBase, big)
	if err != nil {
		t.Fatal(err)
	}
	if got := totalLines(diff.Hunks); got != 2*n {
		t.Fatalf("combined lines = %d, want %d", got, 2*n)
	}
	if diff.StagedHunks != nil || diff.UnstagedHunks != nil {
		t.Fatal("a file changed on one side only has no per-side hunks to read")
	}

	// Stage one rewrite, then rewrite again: changes on both sides.
	runGit(t, root, "add", "big.txt")
	writeRepoFile(t, root, "big.txt", numbered("newer", n))
	snap, _ = r.Snapshot(ctx, root)
	big, _ = snap.ByPath("big.txt")
	if !big.Staged || !big.Unstaged || !big.Deferred {
		t.Fatalf("fixture: staged=%v unstaged=%v deferred=%v", big.Staged, big.Unstaged, big.Deferred)
	}
	diff, err = r.ReadFileDiff(ctx, root, snap.DiffBase, big)
	if err != nil {
		t.Fatal(err)
	}
	if got := totalLines(diff.StagedHunks); got != 2*n {
		t.Fatalf("staged lines = %d, want %d", got, 2*n)
	}
	if got := totalLines(diff.UnstagedHunks); got != 2*n {
		t.Fatalf("unstaged lines = %d, want %d", got, 2*n)
	}
	if !strings.Contains(diff.UnstagedHunks[0].Lines[0].Text+diff.UnstagedHunks[0].Lines[n].Text, "newer") {
		t.Fatal("the unstaged side must compare the index with the working tree")
	}
}

// A renamed file past the threshold is kept out of the bulk reads under both
// names, or its source would arrive as one enormous deletion.
func TestSnapshotDefersLargeRenames(t *testing.T) {
	root, r := tempRepo(t)
	n := 20000
	writeRepoFile(t, root, "before.txt", numbered("keep", n))
	runGit(t, root, "add", ".")
	runGit(t, root, "commit", "-q", "-m", "seed")
	runGit(t, root, "mv", "before.txt", "after.txt")
	edited := numbered("keep", n/2) + numbered("changed", n/2) // ~half the lines change
	writeRepoFile(t, root, "after.txt", edited)
	ctx := context.Background()
	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	after, ok := snap.ByPath("after.txt")
	if !ok || after.OldPath != "before.txt" || !after.Deferred {
		t.Fatalf("after.txt ok=%v old=%q deferred=%v, want a deferred rename", ok, after.OldPath, after.Deferred)
	}
	diff, err := r.ReadFileDiff(ctx, root, snap.DiffBase, after)
	if err != nil {
		t.Fatal(err)
	}
	if len(diff.Hunks) == 0 || totalLines(diff.Hunks) >= 2*n {
		t.Fatalf("rename read returned %d lines, want a rename diff well under %d", totalLines(diff.Hunks), 2*n)
	}
}

func TestUntrackedFilesDeferBySizeAndLines(t *testing.T) {
	root, r := tempRepo(t)
	ctx := context.Background()
	hundred := strings.Repeat("z", 99) + "\n"
	writeRepoFile(t, root, "lines.txt", numbered("row", LargeDiffLines+1)) // many short lines
	writeRepoFile(t, root, "huge.txt", strings.Repeat(hundred, 120_000))   // ~12 MB: past the retention limits, within the read limit
	writeRepoFile(t, root, "beyond.txt", strings.Repeat(hundred, 180_000)) // ~18 MB: past even that
	writeRepoFile(t, root, "blob.bin", "BIN\x00"+strings.Repeat("q", 5<<20))
	snap, err := r.Snapshot(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	get := func(path string) ChangeFile {
		t.Helper()
		cf, ok := snap.ByPath(path)
		if !ok {
			t.Fatalf("%s missing", path)
		}
		return cf
	}
	if cf := get("lines.txt"); !cf.Deferred || cf.Additions != LargeDiffLines+1 || cf.Hunks != nil {
		t.Fatalf("lines.txt deferred=%v additions=%d hunks=%d", cf.Deferred, cf.Additions, len(cf.Hunks))
	}
	if cf := get("huge.txt"); !cf.Deferred || cf.TooLarge || cf.Additions != 0 {
		t.Fatalf("huge.txt deferred=%v tooLarge=%v additions=%d, want deferred and uncounted", cf.Deferred, cf.TooLarge, cf.Additions)
	}
	if cf := get("beyond.txt"); cf.Deferred || !cf.TooLarge {
		t.Fatalf("beyond.txt deferred=%v tooLarge=%v, want too large for good", cf.Deferred, cf.TooLarge)
	}
	if cf := get("blob.bin"); !cf.Binary || cf.Deferred || cf.TooLarge {
		t.Fatalf("blob.bin binary=%v deferred=%v tooLarge=%v, want a plain binary note", cf.Binary, cf.Deferred, cf.TooLarge)
	}

	diff, err := r.ReadFileDiff(ctx, root, snap.DiffBase, get("huge.txt"))
	if err != nil {
		t.Fatal(err)
	}
	if got := totalLines(diff.Hunks); got != 120_000 {
		t.Fatalf("huge.txt on demand = %d lines, want 120000 (past the bulk parse limit too)", got)
	}
	diff, err = r.ReadFileDiff(ctx, root, snap.DiffBase, get("lines.txt"))
	if err != nil || totalLines(diff.Hunks) != LargeDiffLines+1 {
		t.Fatalf("lines.txt on demand = %d lines, err %v", totalLines(diff.Hunks), err)
	}
	if _, err := r.ReadFileDiff(ctx, root, snap.DiffBase, get("beyond.txt")); !isTooLarge(err) {
		t.Fatalf("beyond.txt err = %v, want too large", err)
	}
}

func isTooLarge(err error) bool {
	var ge *Error
	return errors.As(err, &ge) && ge.Class == ErrTooLarge
}

// A commit's review defers the same way, and reads on demand against the
// range it was built from.
func TestCommitSnapshotDefersAndReadsOnDemand(t *testing.T) {
	n := LargeDiffLines
	root, r := tempRepo(t)
	ctx := context.Background()
	writeRepoFile(t, root, "big.txt", numbered("old", n))
	runGit(t, root, "add", ".")
	runGit(t, root, "commit", "-q", "-m", "seed")
	writeRepoFile(t, root, "big.txt", numbered("new", n))
	writeRepoFile(t, root, "small.txt", "a\n")
	runGit(t, root, "add", ".")
	runGit(t, root, "commit", "-q", "-m", "rewrite")
	head := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
	snap, err := r.CommitSnapshot(ctx, root, head)
	if err != nil {
		t.Fatal(err)
	}
	big, _ := snap.ByPath("big.txt")
	if !big.Deferred || big.Additions != n || big.Deletions != n || big.Hunks != nil {
		t.Fatalf("big.txt deferred=%v +%d -%d hunks=%d", big.Deferred, big.Additions, big.Deletions, len(big.Hunks))
	}
	if small, _ := snap.ByPath("small.txt"); small.Deferred || len(small.Hunks) != 1 {
		t.Fatalf("small.txt deferred=%v hunks=%d", small.Deferred, len(small.Hunks))
	}
	if !strings.HasSuffix(snap.DiffBase, ".."+head) {
		t.Fatalf("DiffBase = %q, want a range ending in %s", snap.DiffBase, head)
	}
	diff, err := r.ReadFileDiff(ctx, root, snap.DiffBase, big)
	if err != nil {
		t.Fatal(err)
	}
	if got := totalLines(diff.Hunks); got != 2*n {
		t.Fatalf("on-demand lines = %d, want %d", got, 2*n)
	}
}

// The pathspec tail names both sides of a renamed file, skips what is not
// past the threshold, and stops at maxDiffExcludes files.
func TestLargeDiffExcludes(t *testing.T) {
	entries := []porcelainEntry{
		{XY: "M ", Path: "huge.txt"},
		{XY: "M ", Path: "small.txt"},
		{XY: "R ", Path: "after.txt", OldPath: "before.txt"},
		{XY: "M ", Path: "image.png"},
	}
	numstat := map[string]numstatLine{
		"huge.txt":  {Add: LargeDiffLines, Del: 1},
		"small.txt": {Add: 3, Del: 2},
		"after.txt": {Add: LargeDiffLines, Del: LargeDiffLines},
		"image.png": {Binary: true},
	}
	got := largeDiffExcludes(entries, numstat)
	want := []string{"--", ".", ":(exclude,literal)huge.txt", ":(exclude,literal)after.txt", ":(exclude,literal)before.txt"}
	if !slices.Equal(got, want) {
		t.Fatalf("excludes = %q, want %q", got, want)
	}
	if got := largeDiffExcludes(entries[1:2], numstat); got != nil {
		t.Fatalf("a small file produced excludes: %q", got)
	}

	var many []porcelainEntry
	counts := map[string]numstatLine{}
	for i := 0; i < maxDiffExcludes+10; i++ {
		path := fmt.Sprintf("f%03d.txt", i)
		many = append(many, porcelainEntry{XY: "M ", Path: path})
		counts[path] = numstatLine{Add: 2 * LargeDiffLines}
	}
	if got := largeDiffExcludes(many, counts); len(got) != 2+maxDiffExcludes {
		t.Fatalf("excludes = %d entries, want the two-entry lead plus %d", len(got), maxDiffExcludes)
	}
}
