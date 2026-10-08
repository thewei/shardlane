package gitworkbench

import (
	"strings"
	"testing"
	"unicode/utf8"
)

func TestParsePatchModified(t *testing.T) {
	patch := `diff --git a/src/main.go b/src/main.go
index 1234567..89abcde 100644
--- a/src/main.go
+++ b/src/main.go
@@ -1,4 +1,5 @@ package main
 context line
-deleted line
+added line
+second added
 another context
`
	set := parsePatchSet([]byte(patch))
	fp, ok := set["src/main.go"]
	if !ok {
		t.Fatalf("missing src/main.go in %v", set)
	}
	if fp.status != StatusModified {
		t.Fatalf("status = %v", fp.status)
	}
	if len(fp.hunks) != 1 {
		t.Fatalf("hunks = %d", len(fp.hunks))
	}
	h := fp.hunks[0]
	if h.OldStart != 1 || h.OldLines != 4 || h.NewStart != 1 || h.NewLines != 5 {
		t.Fatalf("hunk header = %+v", h)
	}
	if h.Section != "package main" {
		t.Fatalf("section = %q", h.Section)
	}
	lines := h.Lines
	if len(lines) != 5 {
		t.Fatalf("lines = %d", len(lines))
	}
	// line numbers
	if lines[0].Kind != KindContext || lines[0].OldLine != 1 || lines[0].NewLine != 1 {
		t.Fatalf("line0 = %+v", lines[0])
	}
	if lines[1].Kind != KindDelete || lines[1].OldLine != 2 || lines[1].NewLine != 0 {
		t.Fatalf("line1 = %+v", lines[1])
	}
	if lines[2].Kind != KindAdd || lines[2].NewLine != 2 || lines[2].OldLine != 0 {
		t.Fatalf("line2 = %+v", lines[2])
	}
	if lines[3].Kind != KindAdd || lines[3].NewLine != 3 {
		t.Fatalf("line3 = %+v", lines[3])
	}
	if lines[4].Kind != KindContext || lines[4].OldLine != 3 || lines[4].NewLine != 4 {
		t.Fatalf("line4 = %+v", lines[4])
	}
}

func TestParsePatchAddedDeletedFiles(t *testing.T) {
	patch := `diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,2 @@
+first
+second
diff --git a/old.txt b/old.txt
deleted file mode 100644
index 2222222..0000000
--- a/old.txt
+++ /dev/null
@@ -1,1 +0,0 @@
-gone
`
	set := parsePatchSet([]byte(patch))
	if fp := set["new.txt"]; fp == nil || fp.status != StatusAdded {
		t.Fatalf("new.txt = %+v", set["new.txt"])
	}
	if fp := set["old.txt"]; fp == nil || fp.status != StatusDeleted {
		t.Fatalf("old.txt = %+v", set["old.txt"])
	}
}

func TestParsePatchRenameNoContentChange(t *testing.T) {
	patch := `diff --git a/before.txt b/after.txt
similarity index 100%
rename from before.txt
rename to after.txt
`
	set := parsePatchSet([]byte(patch))
	fp, ok := set["after.txt"]
	if !ok {
		t.Fatal("after.txt missing")
	}
	if fp.status != StatusRenamed || fp.oldPath != "before.txt" {
		t.Fatalf("rename = %+v", fp)
	}
}

func TestParsePatchBinaryAndModeChange(t *testing.T) {
	patch := `diff --git a/logo.png b/logo.png
index 111..222 100644
GIT binary patch
literal 10
zcwm&%<br~

diff --git a/script.sh b/script.sh
old mode 100644
new mode 100755
`
	set := parsePatchSet([]byte(patch))
	if fp := set["logo.png"]; fp == nil || !fp.binary {
		t.Fatalf("logo.png binary = %+v", set["logo.png"])
	}
	if fp := set["script.sh"]; fp == nil || fp.status != StatusTypeChange {
		t.Fatalf("script.sh = %+v", set["script.sh"])
	}
}

func TestParsePatchNoNewlineMarker(t *testing.T) {
	patch := `diff --git a/eof.txt b/eof.txt
index aaa..bbb 100644
--- a/eof.txt
+++ b/eof.txt
@@ -1 +1 @@
-old without newline
\ No newline at end of file
+new without newline
\ No newline at end of file
`
	set := parsePatchSet([]byte(patch))
	fp := set["eof.txt"]
	if len(fp.hunks) != 1 || len(fp.hunks[0].Lines) != 2 {
		t.Fatalf("hunk = %+v", fp.hunks)
	}
	lines := fp.hunks[0].Lines
	if !lines[0].NoNewline || !lines[1].NoNewline {
		t.Fatalf("no-newline flags = %+v", lines)
	}
}

func TestParsePatchQuotedExoticPaths(t *testing.T) {
	patch := `diff --git "a/with \"quote\" and\ttab.txt" "b/with \"quote\" and\ttab.txt"
index aaa..bbb 100644
--- "a/with \"quote\" and\ttab.txt"
+++ "b/with \"quote\" and\ttab.txt"
@@ -1 +1 @@
-x
+y
`
	set := parsePatchSet([]byte(patch))
	found := false
	for key := range set {
		if strings.Contains(key, "quote") {
			found = true
		}
	}
	if !found {
		t.Fatalf("exotic path missing: %v", set)
	}
}

func TestParsePatchMultiHunk(t *testing.T) {
	patch := `diff --git a/multi.txt b/multi.txt
index aaa..bbb 100644
--- a/multi.txt
+++ b/multi.txt
@@ -1,3 +1,3 @@ first region
-a
+b
 context
@@ -50,3 +50,4 @@ second region
 x
-y
+p
+extra
 z
`
	set := parsePatchSet([]byte(patch))
	fp := set["multi.txt"]
	if len(fp.hunks) != 2 {
		t.Fatalf("hunks = %d", len(fp.hunks))
	}
	second := fp.hunks[1]
	if second.OldStart != 50 || second.NewStart != 50 {
		t.Fatalf("second hunk header = %+v", second)
	}
	last := second.Lines[len(second.Lines)-1]
	if last.Kind != KindContext || last.OldLine != 52 || last.NewLine != 53 {
		t.Fatalf("last line = %+v", last)
	}
}

func TestParsePatchPathologicalInput(t *testing.T) {
	// A hunk followed by more garbage lines than the cap must truncate,
	// not wedge.
	var buf strings.Builder
	buf.WriteString("diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,1 +1,1 @@\n")
	buf.WriteString(strings.Repeat("+garbage\n", maxPatchFileLines+10))
	set := parsePatchSet([]byte(buf.String()))
	fp, ok := set["x"]
	if !ok {
		t.Fatal("section missing")
	}
	if len(fp.hunks) == 0 {
		t.Fatal("no hunks parsed")
	}
	if len(fp.hunks[0].Lines) > maxPatchFileLines {
		t.Fatalf("line cap not enforced: %d", len(fp.hunks[0].Lines))
	}
}

func TestWordDiffBasic(t *testing.T) {
	oldRanges, newRanges, ok := WordDiff("return foo(bar)", "return foo(baz)")
	if !ok {
		t.Fatal("expected word diff")
	}
	if len(oldRanges) != 1 || len(newRanges) != 1 {
		t.Fatalf("ranges = %v / %v", oldRanges, newRanges)
	}
	if oldRanges[0].Start != 11 || oldRanges[0].End != 14 {
		t.Fatalf("old range = %v", oldRanges)
	}
	if newRanges[0].Start != 11 || newRanges[0].End != 14 {
		t.Fatalf("new range = %v", newRanges)
	}
}

func TestWordDiffUnicodeSafe(t *testing.T) {
	oldLine := "こんにちは世界の終わり"
	newLine := "こんにちはみんなの終わり"
	oldRanges, newRanges, ok := WordDiff(oldLine, newLine)
	if !ok {
		t.Fatal("expected unicode word diff")
	}
	for _, r := range append(oldRanges, newRanges...) {
		if r.Start > r.End {
			t.Fatalf("invalid range %v", r)
		}
		if r.End > utf8.RuneCountInString(newLine) && r.End > utf8.RuneCountInString(oldLine) {
			t.Fatalf("range %v exceeds rune count", r)
		}
	}
}

func TestWordDiffRewriteSuppressed(t *testing.T) {
	// Mostly rewritten: word marks would be noise.
	_, _, ok := WordDiff("alpha beta gamma delta epsilon", "one two three four five")
	if ok {
		t.Fatal("rewrite should suppress word diff")
	}
}

func TestWordDiffLongLineSuppressed(t *testing.T) {
	long := strings.Repeat("x", MaxWordDiffRunes+1)
	_, _, ok := WordDiff(long, long+"y")
	if ok {
		t.Fatal("over-length lines should suppress word diff")
	}
}

func TestWordDiffIdenticalSuppressed(t *testing.T) {
	_, _, ok := WordDiff("same line", "same line")
	if ok {
		t.Fatal("identical lines should not word-diff")
	}
}

func TestTreeBuildSortFilterFlatten(t *testing.T) {
	files := []ChangeFile{
		{Path: "src/components/button.tsx", Status: StatusModified, Additions: 12, Deletions: 4},
		{Path: "src/components/dialog.tsx", Status: StatusModified, Additions: 5, Deletions: 1},
		{Path: "README.md", Status: StatusModified, Additions: 1, Deletions: 0},
		{Path: "api/handlers/user.go", Status: StatusAdded, Additions: 30, Deletions: 0},
	}
	root := BuildTree(files, true)
	// compact: src/components stays two-level; api has single-child chain
	var names []string
	for _, c := range root.Children {
		names = append(names, c.Name)
	}
	if len(names) != 3 { // api, src, README.md — dirs first
		t.Fatalf("root children = %v", names)
	}
	if names[0] != "README.md" && !root.Children[0].Dir {
		t.Fatalf("dirs not first: %v", names)
	}

	// filter
	filtered := FilterTree(root, "button")
	if filtered == nil {
		t.Fatal("filter lost matches")
	}
	rows := Flatten(filtered, nil)
	if len(rows) != 2 { // src dir + button file (collapsed dirs auto-expanded)
		t.Fatalf("filtered rows = %d", len(rows))
	}

	// totals
	if root.Additions() != 48 || root.Deletions() != 5 {
		t.Fatalf("totals = %d/%d", root.Additions(), root.Deletions())
	}
	if root.FileCount() != 4 {
		t.Fatalf("file count = %d", root.FileCount())
	}

	// ancestors open on selection
	collapsed := map[string]bool{"src": true}
	EnsureAncestorsOpen(collapsed, "src/components/button.tsx")
	if len(collapsed) != 0 {
		t.Fatalf("ancestors still collapsed: %v", collapsed)
	}
}

func TestTreeOneChildCompaction(t *testing.T) {
	files := []ChangeFile{{Path: "deeply/nested/path/file.txt", Status: StatusModified}}
	root := BuildTree(files, true)
	if len(root.Children) != 1 {
		t.Fatal("expected one compacted child")
	}
	child := root.Children[0]
	if child.Name != "deeply/nested/path" || !child.Dir {
		t.Fatalf("compaction = %+v", child)
	}
}
