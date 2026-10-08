package ui

import (
	"fmt"
	"strings"
	"testing"
)

// codeLines returns n lines of code, as a text area of a code editor or a
// log viewer holds.
func codeLines(n int) string {
	var b strings.Builder
	for i := range n {
		fmt.Fprintf(&b, "\tif err := step%d(ctx, items[%d]); err != nil { return err } // line %d\n", i%97, i%13, i)
	}
	return b.String()
}

// textAreaTester shows a text area of lines lines filling a 1200×800
// window, with the keyboard focus and the caret in its first line.
func textAreaTester(lines int) (*Tester, *string) {
	s := codeLines(lines)
	tt := NewTester(func(c *Context) { TextArea(c, &s).Fill() }, 1200, 800)
	tt.Press(40, 20)
	tt.Release(40, 20)
	return tt, &s
}

// benchLines are the sizes of the texts of the benchmarks, in lines.
var benchLines = []int{1000, 10000, 100000, 1000000}

// BenchmarkTextAreaType types a letter into a text area of many lines: a
// frame that edits the text, lays it out and paints it.
func BenchmarkTextAreaType(b *testing.B) {
	for _, n := range benchLines {
		b.Run(fmt.Sprint(n), func(b *testing.B) {
			tt, _ := textAreaTester(n)
			for b.Loop() {
				tt.Type("a")
			}
		})
	}
}

// BenchmarkTextAreaSteady draws frames of a text area of many lines in
// which nothing changed, as the caret blinking asks for.
func BenchmarkTextAreaSteady(b *testing.B) {
	for _, n := range benchLines {
		b.Run(fmt.Sprint(n), func(b *testing.B) {
			tt, _ := textAreaTester(n)
			for b.Loop() {
				tt.Frame()
			}
		})
	}
}

// BenchmarkTextAreaOpen gives a text area a new text of many lines, as an
// app opening a file does, and draws the frame showing it.
func BenchmarkTextAreaOpen(b *testing.B) {
	for _, n := range benchLines {
		b.Run(fmt.Sprint(n), func(b *testing.B) {
			tt, s := textAreaTester(n)
			texts := [2]string{codeLines(n) + "// the end", *s}
			i := 0
			for b.Loop() {
				*s = texts[i%2]
				i++
				tt.Frame()
			}
		})
	}
}
