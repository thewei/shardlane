package ui

import (
	"math"
	"math/rand/v2"
	"strings"
	"testing"
	"unicode/utf8"
	"unsafe"

	"github.com/egoist/mygo/internal/text"
)

// checkBuffer compares b with the runes it should hold.
func checkBuffer(t *testing.T, b *buffer, want []rune) {
	t.Helper()
	if b.s != string(want) || b.n != len(want) {
		t.Fatalf("buffer holds %q (%d runes), want %q", b.s, b.n, string(want))
	}
	p := 0
	for i := 0; i <= len(want); i++ {
		if i == 0 || want[i-1] == '\n' {
			if p >= len(b.paras) || b.paras[p].rune != i || b.paras[p].byte != len(string(want[:i])) {
				t.Fatalf("paragraph %d does not start at rune %d: %+v", p, i, b.paras)
			}
			p++
		}
		if got := b.para(i); got != p-1 {
			t.Fatalf("rune %d is in paragraph %d, want %d", i, got, p-1)
		}
		if got := b.byteOf(i); got != len(string(want[:i])) {
			t.Fatalf("rune %d starts at byte %d, want %d", i, got, len(string(want[:i])))
		}
	}
	if p != len(b.paras) {
		t.Fatalf("%d paragraphs, want %d", len(b.paras), p)
	}
}

func TestBufferEdits(t *testing.T) {
	alphabet := []rune("ab \n\r€😀é_")
	rng := rand.New(rand.NewPCG(1, 2))
	var b buffer
	var want []rune
	b.set("")
	for range 3000 {
		a := rng.IntN(len(want) + 1)
		z := a + rng.IntN(min(len(want)-a, 6)+1)
		ins := make([]rune, rng.IntN(5))
		for i := range ins {
			ins[i] = alphabet[rng.IntN(len(alphabet))]
		}
		if got := b.slice(a, z); got != string(want[a:z]) {
			t.Fatalf("slice(%d, %d) = %q, want %q", a, z, got, string(want[a:z]))
		}
		b.replace(a, z, string(ins))
		want = append(want[:a:a], append(ins, want[z:]...)...)
		checkBuffer(t, &b, want)
	}
}

func TestBufferWords(t *testing.T) {
	s := "one, two_3\n  four\n\nfive"
	var b buffer
	b.set(s)
	var tb text.Boundaries
	tb.Reset([]rune(s))
	for i := 0; i <= b.n; i++ {
		if got, want := b.nextWord(i), tb.NextWord(i); got != want {
			t.Errorf("nextWord(%d) = %d, want %d", i, got, want)
		}
		if got, want := b.prevWord(i), tb.PrevWord(i); got != want {
			t.Errorf("prevWord(%d) = %d, want %d", i, got, want)
		}
		gs, ge := b.wordAt(i)
		ws, we := tb.WordAt(i)
		if gs != ws || ge != we {
			t.Errorf("wordAt(%d) = %d, %d, want %d, %d", i, gs, ge, ws, we)
		}
	}
}

func TestBufferGraphemes(t *testing.T) {
	s := "éx\r\n👍🏽\n\nz"
	var b buffer
	b.set(s)
	var tb text.Boundaries
	tb.Reset([]rune(s))
	var g graphemes
	for i := 0; i <= b.n; i++ {
		if got, want := g.next(&b, i), tb.NextGrapheme(i); got != want {
			t.Errorf("next(%d) = %d, want %d", i, got, want)
		}
		if got, want := g.prev(&b, i), tb.PrevGrapheme(i); got != want {
			t.Errorf("prev(%d) = %d, want %d", i, got, want)
		}
	}
}

func TestHeights(t *testing.T) {
	rng := rand.New(rand.NewPCG(3, 4))
	paras := make([]paragraph, 300)
	for i := range paras {
		if rng.IntN(3) > 0 {
			paras[i].h = float32(10 + rng.IntN(40))
		}
	}
	var h heights
	h.reset(paras)
	h.est = 17
	naiveTop := func(p int) float64 {
		var y float64
		for _, q := range paras[:p] {
			if q.h > 0 {
				y += float64(q.h)
			} else {
				y += h.est
			}
		}
		return y
	}
	for range 200 {
		p := rng.IntN(len(paras))
		if paras[p].h > 0 {
			nh := float32(10 + rng.IntN(40))
			h.add(p, float64(nh-paras[p].h), 0)
			paras[p].h = nh
		} else {
			paras[p].h = float32(10 + rng.IntN(40))
			h.add(p, float64(paras[p].h), -1)
		}
		for q := 0; q <= len(paras); q++ {
			if got, want := h.top(q), naiveTop(q); math.Abs(got-want) > 1e-6 {
				t.Fatalf("top(%d) = %v, want %v", q, got, want)
			}
		}
		for range 20 {
			y := rng.Float64() * (naiveTop(len(paras)) + 50)
			got := h.at(y)
			if got < len(paras)-1 && !(naiveTop(got) <= y && y < naiveTop(got+1)) || got == len(paras)-1 && y < naiveTop(got) {
				t.Fatalf("at(%v) = %d, whose top is %v", y, got, naiveTop(got))
			}
		}
	}
}

// TestTextAreaLaysOutAsWholeText checks that a text area laid out a
// paragraph at a time puts its carets where the text laid out whole has
// them.
func TestTextAreaLaysOutAsWholeText(t *testing.T) {
	s := strings.Repeat("A paragraph long enough to wrap in the text area, twice over at least, with words.\n\nshort\n", 4)
	tt := NewTester(func(c *Context) { TextArea(c, &s).Fill() }, 300, 2000)
	var e *Element
	for _, st := range tt.rt.states {
		if st.editor != nil {
			e = &Element{st: st}
		}
	}
	ed := e.st.editor
	params := ed.area.params
	params.Text = s
	whole := textSystem().Layout(params)
	for i := 0; i <= ed.buf.n; i++ {
		x, y, h := ed.area.caretAt(ed, i, 0)
		wx, wy, wh := whole.Caret(i)
		if x != wx || float32(y) != wy || h != wh {
			t.Fatalf("caret %d at %v, %v, %v; laid out whole, at %v, %v, %v", i, x, y, h, wx, wy, wh)
		}
	}
	if got := ed.area.hs.top(len(ed.buf.paras)); float32(got) != whole.Height {
		t.Errorf("the text is %v high, laid out whole %v", got, whole.Height)
	}
}

// TestTextAreaUndo edits a text area at random, then undoes every edit and
// redoes them.
func TestTextAreaUndo(t *testing.T) {
	s := "first line\nsecond line\nthird"
	tt := NewTester(func(c *Context) { TextArea(c, &s).Fill() }, 400, 300)
	tt.Press(20, 15)
	tt.Release(20, 15)
	var ed *editor
	for _, st := range tt.rt.states {
		if st.editor != nil {
			ed = st.editor
		}
	}
	rng := rand.New(rand.NewPCG(5, 6))
	// The text before each step of undo, which may change nothing, as
	// typing over a selection what it holds.
	texts := []string{s}
	keys := []Key{KeyLeft, KeyRight, KeyUp, KeyDown, KeyHome, KeyEnd}
	for range 60 {
		switch rng.IntN(4) {
		case 0:
			tt.Key(0, keys[rng.IntN(len(keys))])
			continue
		case 1:
			tt.Type("xy\nz")
		case 2:
			tt.Key(0, KeyBackspace)
		case 3:
			tt.Key(Shift, KeyLeft)
			tt.Key(Shift, KeyUp)
			tt.Type("é")
		}
		// Typing in a row makes one step: end it.
		tt.Key(0, KeyLeft)
		tt.Key(0, KeyRight)
		for len(texts) <= len(ed.undo) {
			texts = append(texts, s)
		}
	}
	final := s
	for i := len(texts) - 2; i >= 0; i-- {
		tt.Key(Cmd, KeyZ)
		if s != texts[i] {
			t.Fatalf("undo %d: %q, want %q", len(texts)-1-i, s, texts[i])
		}
	}
	for range len(texts) - 1 {
		tt.Key(Cmd|Shift, KeyZ)
	}
	if s != final {
		t.Errorf("redoing everything made %q, want %q", s, final)
	}
}

// TestTextAreaScrolls scrolls a long text area with the wheel, and keeps
// the caret in view as it moves.
func TestTextAreaScrolls(t *testing.T) {
	var b strings.Builder
	for i := range 2000 {
		b.WriteString("line ")
		b.WriteString(strings.Repeat("x", i%7))
		b.WriteByte('\n')
	}
	s := b.String()
	tt := NewTester(func(c *Context) { TextArea(c, &s).Fill() }, 400, 300)
	tt.Press(20, 15)
	tt.Release(20, 15)
	// A text area opens with the caret at the end, in view.
	tt.Key(Ctrl, KeyHome)
	var st *state
	for _, x := range tt.rt.states {
		if x.editor != nil {
			st = x
		}
	}
	a := st.editor.area
	tt.Scroll(100, 100, 0, 5000)
	tt.Frame()
	if st.scrollY < 4000 || a.scroll != st.scrollY {
		t.Fatalf("the wheel scrolled to %v (area %v)", st.scrollY, a.scroll)
	}
	if a.first == 0 {
		t.Error("the area lays out the paragraphs at the top, not in view")
	}
	// The view stays where the wheel put it: the caret, at the top, does
	// not bring it back.
	tt.Frame()
	if st.scrollY < 4000 {
		t.Fatalf("the view went back to %v", st.scrollY)
	}
	// Moving the caret shows it.
	tt.Key(0, KeyDown)
	if st.scrollY > 100 {
		t.Errorf("the caret moved out of view: the view is at %v", st.scrollY)
	}
	// The end of the text, with Cmd+Down (Ctrl+End elsewhere).
	tt.Key(Ctrl, KeyEnd)
	_, y, h := a.caretAt(st.editor, st.editor.caret, 0)
	if st.editor.caret != utf8.RuneCountInString(s) || y+float64(h) > a.scroll+float64(st.h)+1 || y < a.scroll {
		t.Errorf("caret %d at %v, the view from %v", st.editor.caret, y, a.scroll)
	}
	if a.laid > maxLaid+a.last-a.first+1 {
		t.Errorf("%d paragraphs keep their layouts", a.laid)
	}
}

// TestTextAreaSelectsAsWholeText checks that the selection painted a
// paragraph at a time covers what it covers in the text laid out whole,
// selected newlines and empty lines included.
func TestTextAreaSelectsAsWholeText(t *testing.T) {
	s := "A paragraph long enough to wrap in the text area, twice over.\n\nshort\n\nlast one"
	tt := NewTester(func(c *Context) { TextArea(c, &s).Fill() }, 260, 2000)
	var ed *editor
	for _, st := range tt.rt.states {
		if st.editor != nil {
			ed = st.editor
		}
	}
	a := ed.area
	params := a.params
	params.Text = s
	whole := textSystem().Layout(params)
	n := ed.buf.n
	for from := 0; from <= n; from++ {
		for to := from + 1; to <= n; to++ {
			want := whole.Selection(from, to)
			var got []text.Rect
			last := len(ed.buf.paras) - 1
			for i := range ed.buf.paras {
				start, end := ed.buf.paras[i].rune, ed.buf.end(i)
				if from > end || to < start || to == start && i > 0 && from < start {
					continue
				}
				l := a.paraLayout(ed, i)
				top := float32(a.hs.top(i))
				for _, r := range l.SelectionOn(max(from, start)-start, min(to, end)-start, to > end && i < last) {
					got = append(got, text.Rect{X: r.X, Y: r.Y + top, W: r.W, H: r.H})
				}
			}
			if len(got) != len(want) {
				t.Fatalf("selection %d-%d: %v, laid out whole %v", from, to, got, want)
			}
			for k := range got {
				if got[k] != want[k] {
					t.Fatalf("selection %d-%d: %v, laid out whole %v", from, to, got, want)
				}
			}
		}
	}
}

// TestTextAreaUndoAppChanges undoes typing that the app reformatted: the
// step takes the app's change back with the typing.
func TestTextAreaUndoAppChanges(t *testing.T) {
	s := ""
	tt := NewTester(func(c *Context) {
		TextArea(c, &s).Fill()
		s = strings.ToUpper(s)
	}, 400, 300)
	tt.Press(20, 15)
	tt.Release(20, 15)
	tt.Type("ab")
	tt.Frame()
	if s != "AB" {
		t.Fatalf("typed %q", s)
	}
	tt.Key(Cmd, KeyZ)
	if s != "" {
		t.Errorf("undo made %q", s)
	}
}

// textAreaState returns the state of the only text area of tt.
func textAreaState(tt *Tester) *state {
	for _, st := range tt.rt.states {
		if st.editor != nil && st.editor.area != nil {
			return st
		}
	}
	return nil
}

// TestTextAreaUndoAppLog checks that the last step of undo keeps one
// change however often the app sets the text after it, as a log does, and
// that undoing the step still takes the app's texts back.
func TestTextAreaUndoAppLog(t *testing.T) {
	s := ""
	tt := NewTester(func(c *Context) { TextArea(c, &s).Fill() }, 400, 300)
	tt.Press(20, 15)
	tt.Release(20, 15)
	tt.Type("x")
	tt.Frame()
	for range 100 {
		s += "a line of the log\n"
		tt.Frame()
	}
	ed := textAreaState(tt).editor
	if n := len(ed.undo[len(ed.undo)-1].changes); n != 1 {
		t.Errorf("the last step holds %d changes", n)
	}
	// Typing after the app's text is a change of its own.
	tt.Type("y")
	tt.Frame()
	if c := ed.undo[len(ed.undo)-1].changes; len(c) != 2 || c[1].inserted != "y" {
		t.Errorf("typing after the app's text made %+v", c[len(c)-1])
	}
	tt.Key(Cmd, KeyZ)
	if s != "" {
		t.Errorf("undo made %q", s)
	}
}

// TestTextAreaSharesValue checks that an edit leaving the text as it was
// still gives the value the text's string, which the next frames compare
// at once.
func TestTextAreaSharesValue(t *testing.T) {
	s := strings.Repeat("abc\n", 100) + "x"
	tt := NewTester(func(c *Context) { TextArea(c, &s).Fill() }, 400, 300)
	tt.Press(20, 15)
	tt.Release(20, 15)
	ed := textAreaState(tt).editor
	ed.anchor, ed.caret = ed.buf.n-1, ed.buf.n
	tt.Type("x")
	tt.Frame()
	if ed.buf.s != s || unsafe.StringData(ed.buf.s) != unsafe.StringData(s) {
		t.Error("the value does not share the text's string")
	}
}

// TestTextAreaRevealsWrapped checks that the caret comes into view when the
// paragraphs above it are taller than estimated, as long ones wrapping
// after short ones.
func TestTextAreaRevealsWrapped(t *testing.T) {
	s := strings.Repeat("short\n", 2000) + strings.Repeat(strings.Repeat("word ", 80)+"\n", 300) + "end"
	tt := NewTester(func(c *Context) { TextArea(c, &s).Fill() }, 400, 300)
	st := textAreaState(tt)
	ed, a := st.editor, st.editor.area
	check := func(what string) {
		t.Helper()
		_, y, h := a.caretAt(ed, ed.caret, 0)
		if y < a.scroll || y+float64(h) > a.scroll+float64(st.h) {
			t.Errorf("%s: the caret is at %v, the view from %v", what, y, a.scroll)
		}
	}
	check("open")
	tt.Press(20, 15)
	tt.Release(20, 15)
	tt.Key(Ctrl, KeyHome)
	check("Ctrl+Home")
	tt.Key(Ctrl, KeyEnd)
	check("Ctrl+End")
}

// TestTextAreaKeepsViewOnAppText checks that a text the app sets, as a log
// growing, leaves the view where the wheel put it.
func TestTextAreaKeepsViewOnAppText(t *testing.T) {
	s := strings.Repeat("a line of the log\n", 2000)
	tt := NewTester(func(c *Context) { TextArea(c, &s).Fill() }, 400, 300)
	tt.Press(20, 15)
	tt.Release(20, 15)
	tt.Key(Ctrl, KeyHome)
	st := textAreaState(tt)
	tt.Scroll(100, 100, 0, 5000)
	tt.Frame()
	s += "another line\n"
	tt.Frame()
	if st.scrollY != 5000 {
		t.Errorf("the view went to %v", st.scrollY)
	}
}

// TestTextAreaShowsThroughPadding checks that the paragraph showing in the
// padding above the view is laid out, as the text is drawn there.
func TestTextAreaShowsThroughPadding(t *testing.T) {
	s := strings.Repeat("line\n", 200)
	tt := NewTester(func(c *Context) { TextArea(c, &s).Fill() }, 400, 300)
	tt.Press(20, 15)
	tt.Release(20, 15)
	tt.Key(Ctrl, KeyHome)
	a := textAreaState(tt).editor.area
	tt.Scroll(100, 100, 0, 50*a.line.Height+2) // 2 DIPs into paragraph 50
	tt.Frame()
	if a.anchor != 50 || a.first != 49 {
		t.Errorf("the view starts in paragraph %d, the first laid out is %d", a.anchor, a.first)
	}
}

// TestTextAreaPassword checks that Password leaves a text area as it is,
// as multi-line fields have no password mode on any platform.
func TestTextAreaPassword(t *testing.T) {
	notes := "first\nsecond"
	tt := NewTester(func(c *Context) { TextArea(c, &notes).Password().Height(120) }, 400, 200)
	ed := textAreaState(tt).editor
	if ed.password {
		t.Fatal("Password made a text area a password field")
	}
	if l := ed.buf.paras[1].layout; l == nil || string(l.Runes) != "second" {
		t.Errorf("the second paragraph shows %v", l)
	}
}

// TestTextAreaInForm checks that a field lines its label up with the first
// line of its text area.
func TestTextAreaInForm(t *testing.T) {
	bio := "The first line"
	tt := NewTester(func(c *Context) {
		Form(c, func() {
			Field(c, "About you", func() { TextArea(c, &bio).Height(110) })
		})
	}, 500, 300)
	label, _ := tt.Find("About you")
	st := textAreaState(tt)
	if y := st.y + st.editor.originY; abs32(label.Y-y) > 0.5 {
		t.Errorf("the label is at %v, the first line of the text area at %v", label.Y, y)
	}
}
