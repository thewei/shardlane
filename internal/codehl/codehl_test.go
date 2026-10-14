package codehl

import (
	"strings"
	"sync"
	"testing"

	"github.com/egoist/mygo/ui/editor"
)

func TestLines(t *testing.T) {
	lines := []string{"package main", "", "// hi", `func main() { println("x", 42) }`}
	segs := Lines("main.go", lines)
	if len(segs) != len(lines) {
		t.Fatalf("got %d lines", len(segs))
	}
	has := func(row int, text string, class Class) bool {
		for _, s := range segs[row] {
			if lines[row][s.Start:s.End] == text && s.Class == class {
				return true
			}
		}
		return false
	}
	if !has(0, "package", Keyword) {
		t.Errorf("line 0: %+v", segs[0])
	}
	if !has(2, "// hi", Comment) {
		t.Errorf("line 2: %+v", segs[2])
	}
	if !has(3, `"x"`, String) || !has(3, "42", Number) {
		t.Errorf("line 3: %+v", segs[3])
	}
	if Lines("notes.unknownext", lines) != nil {
		t.Error("plain text highlighted")
	}
}

// A block comment carries its state to the next line, as the editor's own
// view of the same file does.
func TestLinesCarryStateAcrossLines(t *testing.T) {
	lines := []string{"/* one", "two", "three */ x := 1"}
	segs := Lines("main.go", lines)
	if len(segs) != len(lines) {
		t.Fatalf("got %d lines", len(segs))
	}
	for row, want := range []string{"/* one", "two", "three */"} {
		found := false
		for _, s := range segs[row] {
			if lines[row][s.Start:s.End] == want && s.Class == Comment {
				found = true
			}
		}
		if !found {
			t.Errorf("line %d lost the comment: %+v", row, segs[row])
		}
	}
}

// The languages a diff most often shows are colored, by name as well as by
// extension; anything past MaxBytes is left plain.
func TestLinesColorTheCommonLanguages(t *testing.T) {
	for _, tc := range []struct{ name, line string }{
		{"a/main.go", `func main() {}`},
		{"app.ts", `const x: number = 1`},
		{"types.d.ts", `export type A = string`},
		{"main.rs", `fn main() { let x = 1; }`},
		{"tool.py", `def f(): return 1`},
		{"run.sh", `echo "hi"`},
		{"Makefile", `all: build # note`},
		{"Dockerfile", `FROM scratch`},
		{"data.json", `{"a": 1}`},
		{"notes.md", `# Title`},
	} {
		segs := Lines(tc.name, []string{tc.line})
		if len(segs) != 1 || len(segs[0]) == 0 {
			t.Errorf("%s: %q is not colored: %+v", tc.name, tc.line, segs)
		}
	}
	if got := Lines("big.go", []string{strings.Repeat("a", MaxBytes+1)}); got != nil {
		t.Errorf("a file past MaxBytes was tokenized (%d lines)", len(got))
	}
}

// KindClasses is the one table both the review and the Files code view read:
// a kind belongs to one class, ClassOf agrees with the table, and only the
// kinds the review never colored read as plain by default.
func TestKindClassesAreOneToOne(t *testing.T) {
	seen := map[editor.TokenKind]Class{}
	for _, group := range KindClasses {
		if len(group.Kinds) == 0 {
			t.Errorf("class %d has no kinds", group.Class)
		}
		for _, kind := range group.Kinds {
			if prev, dup := seen[kind]; dup {
				t.Errorf("kind %d is in classes %d and %d", kind, prev, group.Class)
			}
			seen[kind] = group.Class
			if got := ClassOf(kind); got != group.Class {
				t.Errorf("ClassOf(%d) = %d, table says %d", kind, got, group.Class)
			}
		}
	}
	for kind := editor.TokenText; kind <= editor.TokenInvalid; kind++ {
		if _, ok := seen[kind]; ok {
			continue
		}
		if kind != editor.TokenEmphasis && kind != editor.TokenStrong {
			t.Errorf("token kind %d has no class: its code would read as plain by accident", kind)
		}
		if ClassOf(kind) != Plain {
			t.Errorf("unlisted kind %d is not plain", kind)
		}
	}
}

// The review colors files from background workers, several at a time.
func TestLinesAreSafeAcrossGoroutines(t *testing.T) {
	lines := strings.Split(strings.Repeat("func f() { return \"x\" + 1 } // c\n", 40), "\n")
	var wg sync.WaitGroup
	for range 8 {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for range 20 {
				if segs := Lines("x.go", lines); len(segs) != len(lines) || len(segs[0]) == 0 {
					t.Error("lines lost their colors under concurrency")
					return
				}
			}
		}()
	}
	wg.Wait()
}
