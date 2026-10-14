package editor

import (
	"math/rand/v2"
	"strconv"
	"strings"
	"testing"

	"github.com/egoist/mygo/internal/diff"
	"github.com/egoist/mygo/ui"
)

// checkHunks checks that hunks turn old's lines into new's.
func checkHunks(t *testing.T, old, nw string, hunks []diff.Hunk) {
	t.Helper()
	a, b := strings.Split(old, "\n"), strings.Split(nw, "\n")
	var out []string
	i, j := 0, 0
	for _, h := range hunks {
		if h.A0 < i || h.B0 < j || h.A0-i != h.B0-j {
			t.Fatalf("hunk %+v out of order after %d, %d: %+v", h, i, j, hunks)
		}
		for ; i < h.A0; i, j = i+1, j+1 {
			if a[i] != b[j] {
				t.Fatalf("unchanged old line %d %q is %q in new: %+v", i, a[i], b[j], hunks)
			}
			out = append(out, a[i])
		}
		out = append(out, b[h.B0:h.B1]...)
		i, j = h.A1, h.B1
	}
	if len(a)-i != len(b)-j {
		t.Fatalf("tails differ: %d, %d: %+v", len(a)-i, len(b)-j, hunks)
	}
	out = append(out, a[i:]...)
	if got := strings.Join(out, "\n"); got != nw {
		t.Fatalf("hunks make %q, want %q", got, nw)
	}
}

// Edits of the new text compare again only around them, and the hunks
// stay those of the texts.
func TestCodeDiffIncremental(t *testing.T) {
	r := rand.New(rand.NewPCG(9, 10))
	pieces := []string{"x", "\n", "", "line\n", "a\nb\n", "{", "}\n", "\n\n"}
	for round := range 300 {
		var sb strings.Builder
		for i := range 30 + r.IntN(30) {
			sb.WriteString([]string{"func a() {", "\treturn", "}", "", "x := 1", "y"}[i%6])
			sb.WriteByte('\n')
		}
		old := NewBuffer(sb.String())
		nw := NewBuffer(sb.String())
		var d diffModel
		d.set(old, nw)
		d.update()
		for range 20 {
			text := nw.String()
			at := r.IntN(len(text) + 1)
			end := min(len(text), at+r.IntN(8))
			nw.Edit(Edit{Start: at, End: end, Text: pieces[r.IntN(len(pieces))]})
			if r.IntN(3) == 0 {
				d.update()
				checkHunks(t, old.String(), nw.String(), d.hunks)
			}
		}
		d.update()
		checkHunks(t, old.String(), nw.String(), d.hunks)
		if round%10 == 0 {
			// A change of the old text compares the whole.
			old.Edit(Edit{Start: 0, End: 0, Text: "// new first line\n"})
			d.update()
			checkHunks(t, old.String(), nw.String(), d.hunks)
		}
	}
}

func TestCodeDiffRevertAccept(t *testing.T) {
	for _, c := range []struct{ old, new string }{
		{"a\nb\nc", "a\nx\nc"},
		{"a\nb\nc", "a\nc"},
		{"a\nc", "a\nb\nc"},
		{"a\nb", "a"},
		{"a", "a\nb"},
		{"b\na", "a"},
		{"", "a\nb"},
		{"a\nb", ""},
		{"a\nb\n", "a\nb\nc\n"},
	} {
		old, nw := NewBuffer(c.old), NewBuffer(c.new)
		var d diffModel
		d.set(old, nw)
		d.update()
		for i := 0; len(d.hunks) > 0 && i < 10; i++ {
			d.revert(0)
			d.update()
		}
		if nw.String() != c.old {
			t.Errorf("reverting %q to %q gave %q", c.new, c.old, nw.String())
		}
		old, nw = NewBuffer(c.old), NewBuffer(c.new)
		d = diffModel{}
		d.set(old, nw)
		d.update()
		for i := 0; len(d.hunks) > 0 && i < 10; i++ {
			d.accept(0)
			d.update()
		}
		if old.String() != c.new {
			t.Errorf("accepting %q over %q gave %q", c.new, c.old, old.String())
		}
	}
}

func TestDiffViewer(t *testing.T) {
	var oldText, newText strings.Builder
	for i := range 200 {
		line := "line " + strconv.Itoa(i)
		oldText.WriteString(line + "\n")
		if i == 100 {
			newText.WriteString("changed\n")
			continue
		}
		newText.WriteString(line + "\n")
	}
	for _, layout := range []DiffLayout{DiffSplit, DiffUnified} {
		d := &DiffState{Old: NewBuffer(oldText.String()), New: NewBuffer(newText.String()), Layout: layout, Editable: true}
		tt := ui.NewTester(func(c *ui.Context) {
			DiffView(c, d).Fill()
		}, 900, 500)
		tt.Frame()
		if hs := d.Hunks(); len(hs) != 1 || hs[0] != (DiffHunk{100, 101, 100, 101}) {
			t.Fatalf("hunks %+v", hs)
		}
		if a, del := d.Stats(); a != 1 || del != 1 {
			t.Fatalf("stats %d, %d", a, del)
		}
		// Unchanged lines collapse but 3 around the change: 97 before, 97
		// after, with the empty line after the last newline.
		gaps := d.gapList
		if len(gaps) != 2 || gaps[0].hidden != 97 || gaps[1].hidden != 97 {
			t.Fatalf("gaps %+v", gaps)
		}
		ed := &d.eds[1]
		total := ed.view.heights.total()
		lh := float64(ed.view.lineHeight)
		want := float64(3+1+3)*lh + 2*diffSeparatorHeight
		if layout == DiffUnified {
			want += lh
		}
		if total != want {
			t.Fatalf("%v: height %v, want %v", layout, total, want)
		}
		// Expanding shows the lines.
		d.expand(gaps[0].a, 1, true)
		tt.Frame()
		if d.gapList[0].hidden != 0 {
			t.Fatalf("expanded gap %+v", d.gapList[0])
		}
		// Editing the new side follows: reverting the change by hand.
		ed.sync()
		r := &ed.view.text
		st := &State{}
		_ = st
		a, z := r.LineStart(100), r.LineEnd(100)
		d.New.Edit(Edit{Start: a, End: z, Text: "line 100"})
		tt.Frame()
		if hs := d.Hunks(); len(hs) != 0 {
			t.Fatalf("hunks after reverting by hand %+v", hs)
		}
		d.New.Undo()
		tt.Frame()
		d.Revert(0)
		tt.Frame()
		if d.New.String() != d.Old.String() {
			t.Fatal("Revert left the texts different")
		}
	}
}

// A split view's sides are as high as each other, line for line.
func TestDiffViewerAlign(t *testing.T) {
	old := "a\nb\nc\nd\ne\nf\n"
	nw := "a\nB\nB2\nB3\nc\nf\ng\n"
	for _, wrap := range []bool{false, true} {
		d := &DiffState{Old: NewBuffer(old), New: NewBuffer(nw), ExpandUnchanged: true, Wrap: wrap,
			Annotations:      []DiffAnnotation{{Side: DiffNew, Line: 2, Height: 40}, {Side: DiffOld, Line: 0, Height: 25}},
			RenderAnnotation: func(c *ui.Context, a DiffAnnotation) { ui.Text(c, "note") }}
		tt := ui.NewTester(func(c *ui.Context) {
			DiffView(c, d).Fill()
		}, 600, 400)
		tt.Frame()
		ha, hb := &d.eds[0].view.heights, &d.eds[1].view.heights
		if ha.total() != hb.total() {
			t.Fatalf("wrap %v: heights %v and %v", wrap, ha.total(), hb.total())
		}
		// Unchanged lines line up.
		for _, p := range [][2]int{{0, 0}, {2, 4}, {5, 5}} {
			if ga, gb := ha.geometry(p[0]).rows, hb.geometry(p[1]).rows; ga != gb {
				t.Fatalf("wrap %v: old line %d at %v, new line %d at %v", wrap, p[0], ga, p[1], gb)
			}
		}
		if !tt.HasText("note") {
			t.Fatal("no annotation")
		}
	}
}

func TestDiffViewerLineSelection(t *testing.T) {
	var got DiffLineRange
	d := &DiffState{Old: NewBuffer("a\nb\nc\n"), New: NewBuffer("a\nx\nc\n"), Layout: DiffUnified, ExpandUnchanged: true,
		LineSelection: true, OnLineSelection: func(r DiffLineRange) { got = r }}
	tt := ui.NewTester(func(c *ui.Context) {
		DiffView(c, d).Fill()
	}, 600, 400)
	tt.Frame()
	ed := &d.eds[1]
	lh := ed.view.lineHeight
	// The number of new line 2, below the deleted line.
	y := float32(ed.view.heights.geometry(1).rows) + lh/2
	tt.ClickAt(ed.gutterW-8, y)
	if r, ok := d.SelectedLines(); !ok || r != (DiffLineRange{DiffNew, 1, 1}) || got != r {
		t.Fatalf("selected %+v %v, reported %+v", r, ok, got)
	}
	tt.ClickAtWith(ui.Shift, ed.gutterW-8, float32(ed.view.heights.geometry(2).rows)+lh/2)
	if r, _ := d.SelectedLines(); r != (DiffLineRange{DiffNew, 1, 2}) {
		t.Fatalf("extended %+v", r)
	}
}
