package editor

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
)

// Line backgrounds fill the rows of their lines across the text, not the
// gutter: those a fold hides take no rows, and a wrapped line's every row.
func TestLineBackgrounds(t *testing.T) {
	white := &Theme{Background: ui.RGB(255, 255, 255)}
	blue := ui.RGB(0, 0, 255)
	ct := newEditorTest(t, "go", "func a() {\n\tx()\n\ty()\n}\nfunc b() {}\nz|")
	ct.st.Theme, ct.st.NoActiveLine = white, true
	ct.st.LineBackgrounds = []LineBackground{{First: 0, Last: 4, Color: blue}}
	ct.tt.Frame()
	// filled reports whether x, in DIPs of the 600-wide window, is blue on
	// the row.
	filled := func(x float32, row int) bool {
		img := ct.tt.Image()
		s := float32(img.Bounds().Dx()) / 600
		c := img.RGBAAt(int(x*s), int((float32(row)*ct.st.ed.view.lineHeight+2)*s))
		return c.B > 200 && c.R < 60
	}
	for row, want := range []bool{true, true, true, true, true, false} {
		if filled(500, row) != want {
			t.Fatalf("row %d filled %v, want %v", row, !want, want)
		}
	}
	if filled(2, 0) {
		t.Fatal("the line background filled the gutter")
	}
	// Folded, line 0 hides lines 1 and 2.
	ct.st.Fold(0)
	ct.tt.Frame()
	for row, want := range []bool{true, true, true, false} {
		if filled(500, row) != want {
			t.Fatalf("folded, row %d filled %v, want %v", row, !want, want)
		}
	}
	// Wrapped, the line fills its rows.
	ct = newEditorTest(t, "", strings.Repeat("x", 150)+"\nz|")
	ct.st.Theme, ct.st.NoActiveLine, ct.st.SoftWrap = white, true, true
	ct.st.LineBackgrounds = []LineBackground{{First: 0, Last: 0, Color: blue}}
	ct.tt.Frame()
	ct.tt.Frame()
	if rows := len(ct.st.ed.view.line(0).rows); rows != 3 {
		t.Fatalf("the line wrapped into %d rows, want 3", rows)
	}
	for row, want := range []bool{true, true, true, false} {
		if filled(300, row) != want {
			t.Fatalf("wrapped, row %d filled %v, want %v", row, !want, want)
		}
	}
}
