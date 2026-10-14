package editor

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
)

const testPatch = `diff --git a/greet.go b/greet.go
index 3b18e51..a2e8f0c 100644
--- a/greet.go
+++ b/greet.go
@@ -1,5 +1,6 @@ package main
 package main

-func greet() string {
-	return "hello"
+// greet greets.
+func greet(name string) string {
+	return "hello " + name
 }
diff --git a/old.txt b/new.txt
similarity 90%
rename from old.txt
rename to new.txt
--- a/old.txt
+++ b/new.txt
@@ -1 +1 @@
-a
\ No newline at end of file
+b
diff --git a/added.md b/added.md
new file mode 100644
--- /dev/null
+++ b/added.md
@@ -0,0 +1,2 @@
+# Title
+text
`

func TestParsePatch(t *testing.T) {
	files, err := ParsePatch(testPatch)
	if err != nil {
		t.Fatal(err)
	}
	if len(files) != 3 {
		t.Fatalf("%d files", len(files))
	}
	f := files[0]
	if f.OldName != "greet.go" || f.NewName != "greet.go" || len(f.Hunks) != 1 {
		t.Fatalf("first file %+v", f)
	}
	h := f.Hunks[0]
	if h.OldStart != 1 || h.OldLines != 5 || h.NewStart != 1 || h.NewLines != 6 || h.Section != "package main" || len(h.Lines) != 8 {
		t.Fatalf("hunk %+v", h)
	}
	old := "package main\n\nfunc greet() string {\n\treturn \"hello\"\n}\n"
	got, err := f.Apply(old)
	if err != nil {
		t.Fatal(err)
	}
	want := "package main\n\n// greet greets.\nfunc greet(name string) string {\n\treturn \"hello \" + name\n}\n"
	if got != want {
		t.Fatalf("applied %q", got)
	}
	r := files[1]
	if r.OldName != "old.txt" || r.NewName != "new.txt" {
		t.Fatalf("rename %+v", r)
	}
	if got, err := r.Apply("a"); err != nil || got != "b\n" {
		t.Fatalf("applied rename %q, %v", got, err)
	}
	n := files[2]
	if n.OldName != "" || n.NewName != "added.md" || n.NewMode != "100644" {
		t.Fatalf("new file %+v", n)
	}
	if got, err := n.Apply(""); err != nil || got != "# Title\ntext\n" {
		t.Fatalf("applied new file %q, %v", got, err)
	}
	if _, err := f.Apply("something else\n"); err == nil {
		t.Fatal("applied to the wrong text")
	}
}

// A patch shows without the files, numbered as they are.
func TestDiffViewerPatch(t *testing.T) {
	files, _ := ParsePatch(`--- a/x.go
+++ b/x.go
@@ -10,3 +10,3 @@ func f() {
 	a()
-	b()
+	c()
 	d()
@@ -40,2 +40,3 @@
 	e()
+	f()
 }
`)
	d := &DiffState{}
	d.SetPatch(files[0])
	tt := ui.NewTester(func(c *ui.Context) {
		DiffView(c, d).Fill()
	}, 800, 400)
	tt.Frame()
	if hs := d.Hunks(); len(hs) != 2 {
		t.Fatalf("hunks %+v", hs)
	}
	if n := d.sides[1].number(3); n != 40 {
		t.Fatalf("line 3 of the new text is line %d of the file", n)
	}
	if n := d.sides[0].number(1); n != 11 {
		t.Fatalf("line 1 of the old text is line %d of the file", n)
	}
	if d.New.Language() == nil || !strings.HasPrefix(d.sides[1].blks[0].id.(string), "@@ -10,3 +10,3 @@") {
		t.Fatalf("language %v, blocks %+v", d.New.Language(), d.sides[1].blks)
	}
}
