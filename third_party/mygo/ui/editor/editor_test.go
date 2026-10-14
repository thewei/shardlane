package editor

import (
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

// editorTest is an editor in a tester, focused, with the text given; | in
// it marks the carets, and [ ] selections.
type editorTest struct {
	t  *testing.T
	st *State
	tt *ui.Tester
}

func newEditorTest(t *testing.T, lang, text string) *editorTest {
	t.Helper()
	plain, sels := parseCarets(text)
	st := &State{Buffer: NewBuffer(plain)}
	if lang != "" {
		st.Buffer.SetLanguage(LanguageNamed(lang))
	}
	ct := &editorTest{t: t, st: st}
	ct.tt = ui.NewTester(func(c *ui.Context) {
		View(c, st).Fill().AutoFocus()
	}, 600, 400)
	if len(sels) > 0 {
		st.SetSelections(sels...)
	}
	ct.tt.Frame()
	return ct
}

// parseCarets takes the carets and selections out of text.
func parseCarets(text string) (string, []Selection) {
	var b strings.Builder
	var sels []Selection
	open := -1
	for i := 0; i < len(text); i++ {
		switch text[i] {
		case '|':
			sels = append(sels, Selection{Anchor: b.Len(), Head: b.Len()})
		case '[':
			open = b.Len()
		case ']':
			sels = append(sels, Selection{Anchor: open, Head: b.Len()})
		default:
			b.WriteByte(text[i])
		}
	}
	return b.String(), sels
}

// show writes the text with its carets and selections, as parseCarets
// reads them.
func (ct *editorTest) show() string {
	text := ct.st.Buffer.String()
	sels := ct.st.Selections()
	var b strings.Builder
	for i := 0; i <= len(text); i++ {
		// A selection ending here first, then those starting.
		for _, s := range sels {
			if !s.Empty() && s.End() == i {
				b.WriteByte(']')
			}
		}
		for _, s := range sels {
			if s.Empty() && s.Head == i {
				b.WriteByte('|')
			} else if !s.Empty() && s.Start() == i {
				b.WriteByte('[')
			}
		}
		if i < len(text) {
			b.WriteByte(text[i])
		}
	}
	return b.String()
}

func (ct *editorTest) want(want string) {
	ct.t.Helper()
	if got := ct.show(); got != want {
		ct.t.Fatalf("got\n%s\nwant\n%s", got, want)
	}
}

func (ct *editorTest) key(mods ui.Modifiers, key ui.Key) { ct.tt.Key(mods, key) }
func (ct *editorTest) typ(s string) {
	for _, r := range s {
		ct.tt.Type(string(r))
	}
}
func (ct *editorTest) run(cmd Command) { ct.st.Run(cmd); ct.tt.Frame() }

func TestCodeEditorTyping(t *testing.T) {
	ct := newEditorTest(t, "go", "|")
	ct.typ("fmt.Println(")
	ct.want("fmt.Println(|)")
	ct.typ(`"hi`)
	ct.want(`fmt.Println("hi|")`)
	// Closers typed step over those typed.
	ct.typ(`")`)
	ct.want(`fmt.Println("hi")|`)
	ct.key(0, ui.KeyBackspace)
	ct.want(`fmt.Println("hi"|`)
	// Backspace takes an empty pair.
	ct = newEditorTest(t, "go", "x|")
	ct.typ("[")
	ct.want("x[|]")
	ct.key(0, ui.KeyBackspace)
	ct.want("x|")
	// A selection wraps in brackets.
	ct = newEditorTest(t, "go", "[abc]")
	ct.typ("(")
	ct.want("([abc])")
}

func TestCodeEditorNewline(t *testing.T) {
	ct := newEditorTest(t, "go", "func main() {|}")
	ct.key(0, ui.KeyEnter)
	ct.want("func main() {\n\t|\n}")
	ct.typ("x")
	ct.key(0, ui.KeyEnter)
	ct.want("func main() {\n\tx\n\t|\n}")
	// A line comment goes on.
	ct = newEditorTest(t, "go", "\t// a comment|")
	ct.key(0, ui.KeyEnter)
	ct.want("\t// a comment\n\t// |")
	// Python indents after a colon, with spaces.
	ct = newEditorTest(t, "python", "def f():|")
	ct.key(0, ui.KeyEnter)
	ct.want("def f():\n    |")
}

// TestCodeEditorOutdent checks that typing what closes a block outdents
// its line, as Zed's auto-indent on input does.
func TestCodeEditorOutdent(t *testing.T) {
	ct := newEditorTest(t, "go", "func main() {\n\tif x {\n\t\ty()\n\t\t|")
	ct.typ("}")
	ct.want("func main() {\n\tif x {\n\t\ty()\n\t}|")
	// Undo takes the bracket back with its outdent.
	ct.key(ui.Cmd, ui.KeyZ)
	ct.want("func main() {\n\tif x {\n\t\ty()\n\t\t|")
	// Not after other text, nor inside a string.
	ct = newEditorTest(t, "go", "func main() {\n\t\tx|")
	ct.typ("}")
	ct.want("func main() {\n\t\tx}|")
	ct = newEditorTest(t, "go", "f(`\n\t\t|")
	ct.typ("}")
	ct.want("f(`\n\t\t}|")
	// Each caret outdents its line.
	ct = newEditorTest(t, "go", "{\n\t{\n\t\t|\n\t\t|")
	ct.typ("}")
	ct.want("{\n\t{\n\t}|\n}|")
	// Words of the language's DecreaseIndent.
	ct = newEditorTest(t, "shell", "if x; then\n  echo\n  |")
	ct.typ("fi")
	ct.want("if x; then\n  echo\nfi|")
	ct = newEditorTest(t, "ruby", "if x\n  y\n  |")
	ct.typ("else")
	ct.want("if x\n  y\nelse|")
	ct.typ("\n  z\n  end")
	ct.want("if x\n  y\nelse\n  z\nend|")
}

// TestCodeEditorScrollbar checks that the scroll bar of a text shorter
// than the editor, which scrolls past its end, takes the pointer: an
// arrow over it, and dragging it scrolls rather than selects.
func TestCodeEditorScrollbar(t *testing.T) {
	ct := newEditorTest(t, "go", "|package main\n\nfunc main() {}\n")
	x := float32(600 - scrollbarWidth/2)
	ct.tt.Move(300, 5)
	if c := ct.tt.Cursor(); c != ui.CursorText {
		t.Fatalf("cursor over the text %v, want the I-beam", c)
	}
	ct.tt.Move(x, 5)
	if c := ct.tt.Cursor(); c != ui.CursorDefault {
		t.Fatalf("cursor over the scroll bar %v, want the arrow", c)
	}
	ct.tt.Press(x, 5)
	ct.tt.Move(x, 300)
	ct.tt.Release(x, 300)
	if ct.st.ed.scrollY <= 0 {
		t.Fatal("dragging the scroll bar did not scroll")
	}
	ct.want("|package main\n\nfunc main() {}\n")
	// The horizontal one shows for a line running under the vertical one.
	ct = newEditorTest(t, "", "|"+strings.Repeat("x", 200))
	y := float32(400 - scrollbarWidth/2)
	ct.tt.Move(300, y)
	if c := ct.tt.Cursor(); c != ui.CursorDefault {
		t.Fatalf("cursor over the horizontal scroll bar %v, want the arrow", c)
	}
	ct.tt.Press(300, y)
	ct.tt.Move(500, y)
	ct.tt.Release(500, y)
	if ct.st.ed.scrollX <= 0 {
		t.Fatal("dragging the horizontal scroll bar did not scroll")
	}
	ct.want("|" + strings.Repeat("x", 200))
}

func TestCodeEditorMultiCursor(t *testing.T) {
	ct := newEditorTest(t, "go", "foo := 1\nbar := foo\nfoo++")
	ct.st.Select(0, 0)
	// Select next occurrence: the word, then each next.
	ct.key(ui.Cmd, ui.KeyD)
	ct.want("[foo] := 1\nbar := foo\nfoo++")
	ct.key(ui.Cmd, ui.KeyD)
	ct.key(ui.Cmd, ui.KeyD)
	ct.want("[foo] := 1\nbar := [foo]\n[foo]++")
	ct.typ("x")
	ct.want("x| := 1\nbar := x|\nx|++")
	ct.key(0, ui.KeyEscape)
	ct.want("x := 1\nbar := x\nx|++")
	// Add carets below, and type at each.
	ct = newEditorTest(t, "", "|ab\ncd\nef")
	ct.key(ui.Cmd|ui.Alt, ui.KeyDown)
	ct.key(ui.Cmd|ui.Alt, ui.KeyDown)
	ct.typ("-")
	ct.want("-|ab\n-|cd\n-|ef")
	// Going back up takes the last added away.
	ct.key(ui.Cmd|ui.Alt, ui.KeyUp)
	ct.want("-|ab\n-|cd\n-ef")
	// Select all occurrences.
	ct = newEditorTest(t, "", "a b a c |a")
	ct.key(ui.Cmd|ui.Shift, ui.KeyL)
	ct.want("[a] b [a] c [a]")
}

func TestCodeEditorLines(t *testing.T) {
	ct := newEditorTest(t, "go", "one\ntw|o\nthree")
	ct.key(ui.Alt, ui.KeyUp)
	ct.want("tw|o\none\nthree")
	ct.key(ui.Alt, ui.KeyDown)
	ct.key(ui.Alt, ui.KeyDown)
	ct.want("one\nthree\ntw|o")
	ct.key(ui.Alt|ui.Shift, ui.KeyUp)
	ct.want("one\nthree\ntw|o\ntwo")
	// Deleting a line keeps the caret's column, as Zed's.
	ct.key(ui.Cmd|ui.Shift, ui.KeyK)
	ct.want("one\nthree\ntw|o")
	ct.key(ui.Ctrl, ui.KeyJ)
	ct.want("one\nthree\ntw|o")
	ct = newEditorTest(t, "go", "a|\n  b\nc")
	ct.key(ui.Ctrl, ui.KeyJ)
	ct.want("a |b\nc")
	// Toggle comments at the least indentation, and back.
	ct = newEditorTest(t, "go", "\tif x {\n\t\t[y()\n\t}]")
	ct.key(ui.Cmd, ui.KeySlash)
	if got := ct.st.Buffer.String(); got != "\tif x {\n\t// \ty()\n\t// }" {
		t.Fatalf("commented %q", got)
	}
	ct.key(ui.Cmd, ui.KeySlash)
	if got := ct.st.Buffer.String(); got != "\tif x {\n\t\ty()\n\t}" {
		t.Fatalf("uncommented %q", got)
	}
	// Indent and outdent.
	ct = newEditorTest(t, "python", "[a\nb]")
	ct.key(ui.Cmd, ui.KeyBracketRight)
	ct.want("[    a\n    b]")
	ct.key(ui.Cmd, ui.KeyBracketLeft)
	ct.want("[a\nb]")
}

func TestCodeEditorUndo(t *testing.T) {
	ct := newEditorTest(t, "", "|")
	ct.typ("abc")
	// Typing in a row is one step.
	ct.key(ui.Cmd, ui.KeyZ)
	ct.want("|")
	ct.key(ui.Cmd|ui.Shift, ui.KeyZ)
	ct.want("abc|")
	// A pause makes another.
	time.Sleep(groupTimeout + 50*time.Millisecond)
	ct.typ("d")
	ct.key(ui.Cmd, ui.KeyZ)
	ct.want("abc|")
	// Undo puts the selections back.
	ct = newEditorTest(t, "", "x [hello] y")
	ct.typ("b")
	ct.want("x b| y")
	ct.key(ui.Cmd, ui.KeyZ)
	ct.want("x [hello] y")
}

func TestCodeEditorClipboard(t *testing.T) {
	ct := newEditorTest(t, "", "one\ntw|o\nthree")
	// Copying a caret alone copies its line, which pastes above a line.
	ct.key(ui.Cmd, ui.KeyC)
	if got := ct.tt.Clipboard(); got != "two\n" {
		t.Fatalf("copied %q", got)
	}
	ct.st.Select(1, 1)
	ct.key(ui.Cmd, ui.KeyV)
	ct.want("two\no|ne\ntwo\nthree")
	// Cut of several selections pastes back piece by piece.
	ct = newEditorTest(t, "", "[a] [b] [c]")
	ct.key(ui.Cmd, ui.KeyX)
	ct.want("| | |")
	ct.key(ui.Cmd, ui.KeyV)
	ct.want("a| b| c|")
	// Pasting text from elsewhere.
	ct.tt.SetClipboard("zz")
	ct.key(ui.Cmd, ui.KeyV)
	ct.want("azz| bzz| czz|")
	// Pasting what was just copied over the selection still copied puts
	// the caret after it, and pasting again pastes it again.
	ct = newEditorTest(t, "", "say [hello] there")
	ct.key(ui.Cmd, ui.KeyC)
	ct.key(ui.Cmd, ui.KeyV)
	ct.want("say hello| there")
	ct.key(ui.Cmd, ui.KeyV)
	ct.want("say hellohello| there")
	// As does typing over a selection the character typed.
	ct = newEditorTest(t, "", "a[b]c")
	ct.typ("b")
	ct.want("ab|c")
}

func TestCodeEditorComposition(t *testing.T) {
	ct := newEditorTest(t, "", "a|b\nc|d")
	ct.tt.Compose("に", 1)
	ct.want("aに|b\ncに|d")
	ct.tt.Compose("にほ", 2)
	ct.want("aにほ|b\ncにほ|d")
	ct.tt.Type("日本")
	ct.want("a日本|b\nc日本|d")
	ct.key(ui.Cmd, ui.KeyZ)
	ct.want("a|b\nc|d")
}

func TestCodeEditorFind(t *testing.T) {
	ct := newEditorTest(t, "", "|cat Cat cat dog")
	ct.st.Find("cat")
	ct.tt.Frame()
	if n, cur := ct.st.FindMatches(); n != 3 || cur != 0 {
		t.Fatalf("matches %d, %d", n, cur)
	}
	ct.st.ed.find.caseSens = true
	ct.st.ed.find.stale = true
	if n, _ := ct.st.FindMatches(); n != 2 {
		t.Fatalf("case-sensitive matches %d", n)
	}
	ct.st.ed.find.replace = "cow"
	ct.st.ed.find.replaceAll(&ct.st.ed)
	if got := ct.st.Buffer.String(); got != "cow Cat cow dog" {
		t.Fatalf("replaced %q", got)
	}
	ct.st.ed.find.regex, ct.st.ed.find.query, ct.st.ed.find.replace, ct.st.ed.find.stale = true, `c(\w+)`, "<$1>", true
	ct.st.ed.find.replaceAll(&ct.st.ed)
	if got := ct.st.Buffer.String(); got != "<ow> Cat <ow> dog" {
		t.Fatalf("replaced with a regular expression %q", got)
	}
}

func TestCodeEditorFold(t *testing.T) {
	ct := newEditorTest(t, "go", "func a() {\n\tx()\n\ty()\n}\nfunc b() {}")
	h := &ct.st.ed.view.heights
	before := h.total()
	ct.st.Fold(0)
	ct.tt.Frame()
	if got := ct.st.Folded(); len(got) != 1 || got[0] != 0 {
		t.Fatalf("folded %v", got)
	}
	if h.total() != before-2*float64(ct.st.ed.view.lineHeight) {
		t.Fatalf("folded height %v of %v", h.total(), before)
	}
	// Moving down skips what the fold hides.
	ct.st.Select(0, 0)
	ct.key(0, ui.KeyDown)
	if p := ct.st.CursorPos(); p.Line != 3 {
		t.Fatalf("caret went to line %d", p.Line)
	}
	ct.run(UnfoldAll)
	if h.total() != before {
		t.Fatalf("unfolded height %v of %v", h.total(), before)
	}
}

func TestCodeEditorWords(t *testing.T) {
	ct := newEditorTest(t, "", "|fooBar_baz qux")
	ct.run(MoveSubwordRight)
	ct.want("foo|Bar_baz qux")
	ct.run(MoveSubwordRight)
	ct.want("fooBar|_baz qux")
	ct.run(MoveWordRight)
	ct.want("fooBar_baz| qux")
	ct.run(MoveWordLeft)
	ct.want("|fooBar_baz qux")
	// Selecting larger: the word, the string, its quotes, the brackets.
	ct = newEditorTest(t, "go", `f(x, "a b|c")`)
	ct.run(SelectLarger)
	ct.want(`f(x, "a [bc]")`)
	ct.run(SelectLarger)
	ct.want(`f(x, "[a bc]")`)
	ct.run(SelectLarger)
	ct.want(`f(x, ["a bc"])`)
	ct.run(SelectLarger)
	ct.want(`f([x, "a bc"])`)
	ct.run(SelectSmaller)
	ct.want(`f(x, ["a bc"])`)
	// A selection of a pair grows to the pair around it, and a caret
	// after a pair selects that pair, as Zed does.
	ct = newEditorTest(t, "go", `g(f[(x)])`)
	ct.run(SelectLarger)
	ct.want(`g([f(x)])`)
	ct.run(SelectLarger)
	ct.want(`g[(f(x))]`)
	ct = newEditorTest(t, "go", `g(f(x)|)`)
	ct.run(SelectLarger)
	ct.want(`g(f[(x)])`)
	// Moving to the bracket goes to the same side of the other one.
	ct = newEditorTest(t, "go", `g(f(x)|)`)
	ct.run(MoveToBracket)
	ct.want(`g(f|(x))`)
	ct.run(MoveToBracket)
	ct.want(`g(f(x)|)`)
	ct = newEditorTest(t, "go", `g(f(|x))`)
	ct.run(MoveToBracket)
	ct.want(`g(f(x|))`)
	ct.run(MoveToBracket)
	ct.want(`g(f(|x))`)
	// Case conversions.
	ct = newEditorTest(t, "", "[fooBarBaz]")
	ct.run(SnakeCase)
	ct.want("[foo_bar_baz]")
	ct.run(KebabCase)
	ct.want("[foo-bar-baz]")
	ct.run(PascalCase)
	ct.want("[FooBarBaz]")
}

// Two editors of one buffer follow each other's edits.
func TestCodeEditorSharedBuffer(t *testing.T) {
	buf := NewBuffer("hello world")
	a, b := &State{Buffer: buf}, &State{Buffer: buf}
	tt := ui.NewTester(func(c *ui.Context) {
		ui.Row(c).Fill().Children(func() {
			View(c, a).Grow(1).AutoFocus()
			View(c, b).Grow(1)
		})
	}, 800, 300)
	b.Select(6, 11)
	a.Select(0, 0)
	tt.Frame()
	tt.Type("¡")
	if got := buf.String(); got != "¡hello world" {
		t.Fatalf("text %q", got)
	}
	if s := b.Selection(); s.Start() != 8 || s.End() != 13 {
		t.Fatalf("other editor's selection %+v", s)
	}
}
