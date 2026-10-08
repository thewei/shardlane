package ui

import (
	"runtime"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/egoist/mygo/internal/scene"
	"github.com/egoist/mygo/internal/text"
)

type editKind uint8

const (
	editKey editKind = iota
	editInsert
	editCompose
	editCommand
)

type editEvent struct {
	kind  editKind
	mods  Modifiers
	key   Key
	text  string
	caret int
	// replace makes an insertion or a composition take the runes from to
	// to instead of the selection, as an input method asked.
	replace  bool
	from, to int
}

// change is an edit of the text: the runes at at held removed, and hold
// inserted since. A text the app set is a change of the whole text (app),
// which typing after it does not grow.
type change struct {
	at                int
	removed, inserted string
	app               bool
}

// undoStep is what undoing takes back: the changes of an edit, or of
// typing in a row, and where the caret and the selection were before and
// after them.
type undoStep struct {
	changes                 []change
	caret, anchor           int
	caretAfter, anchorAfter int
}

// before returns the text before the step's changes, from the text after
// them.
func (s *undoStep) before(text string) string {
	if len(s.changes) > 0 && s.changes[0].app {
		return s.changes[0].removed
	}
	for i := len(s.changes) - 1; i >= 0; i-- {
		c := s.changes[i]
		at := runeOffset(text, c.at)
		text = text[:at] + c.removed + text[at+len(c.inserted):]
	}
	return text
}

// editor is the state of a text input: the text, the caret, the selection,
// the composition of an input method, undo history and what the last frame
// laid out.
type editor struct {
	buf           buffer
	graphemes     graphemes
	caret, anchor int
	compose       string
	composeCaret  int
	queue         []editEvent
	multiline     bool
	readOnly      bool   // selectable text: selected and copied, not edited
	source        string // the text of selectable text
	password      bool
	// leaveEmptyBackspace leaves Backspace to shortcuts while the text is
	// empty, as a token field's input does to take out a token.
	leaveEmptyBackspace bool
	placeholder         string
	// layout is what the last frame laid out of a single-line input or a
	// selectable text; area lays out a text area (textarea.go).
	layout *text.Layout
	area   *area
	// display maps runes of the text to runes of the layout, which shows
	// bullets for passwords and holds the composition.
	scrollX          float32
	originX, originY float32 // content box, relative to the element
	desiredX         float32
	hasDesired       bool
	undo, redo       []undoStep
	lastEdit         time.Time
	coalesce         bool
	dragging         bool
	dragUnit         int // 1 rune, 2 word, 3 line
	dragStart        [2]int
	pressMods        Modifiers
	contentW         float32
}

func newEditor() *editor { return &editor{} }

// setText gives the editor a text the app set. Undoing the last step takes
// it back with the step, as when the app reformats what the user typed:
// the step becomes one change, from the text before it, so that it keeps
// two texts however often the app sets one, as a log does.
func (ed *editor) setText(s string) {
	if n := len(ed.undo); n > 0 {
		step := &ed.undo[n-1]
		step.changes = append(step.changes[:0], change{removed: step.before(ed.buf.s), inserted: s, app: true})
	}
	ed.redo = ed.redo[:0]
	ed.buf.set(s)
	ed.caret = min(ed.caret, ed.buf.n)
	ed.anchor = min(ed.anchor, ed.buf.n)
	if n := len(ed.undo); n > 0 {
		ed.undo[n-1].caretAfter, ed.undo[n-1].anchorAfter = ed.caret, ed.anchor
	}
}

func (ed *editor) String() string { return ed.buf.s }

func (ed *editor) selection() (int, int) {
	if ed.caret < ed.anchor {
		return ed.caret, ed.anchor
	}
	return ed.anchor, ed.caret
}

func (ed *editor) selectAll() { ed.anchor, ed.caret = 0, ed.buf.n }

// wants reports whether the editor handles a key, rather than letting it
// reach shortcuts.
func (ed *editor) wants(k keyEvent) bool {
	m := k.mods &^ Shift
	if ed.readOnly {
		// Selecting and copying; the arrows without Shift scroll.
		switch k.key {
		case KeyA, KeyC:
			return k.mods == Cmd
		case KeyLeft, KeyRight, KeyUp, KeyDown, KeyHome, KeyEnd:
			return k.mods&Shift != 0
		}
		return false
	}
	switch k.key {
	case KeyBackspace:
		// Empty, a token field's input leaves it to take out a token.
		return !ed.leaveEmptyBackspace || ed.buf.n > 0 || m != 0
	case KeyLeft, KeyRight:
		// But Alt and an arrow, which go back and forward outside of macOS,
		// where they move by words.
		return m != Alt || runtime.GOOS == "darwin"
	case KeyHome, KeyEnd, KeyDelete:
		return true
	case KeyUp, KeyDown, KeyPageUp, KeyPageDown:
		return ed.multiline || m != 0
	case KeyEnter:
		return m == 0
	case KeyTab, KeyEscape:
		return false
	case KeyF1, KeyF2, KeyF3, KeyF4, KeyF5, KeyF6, KeyF7, KeyF8, KeyF9, KeyF10, KeyF11, KeyF12, KeyBack, KeyForward:
		// They type nothing: they go to shortcuts, as F3 finding the next
		// match.
		return false
	case KeyA, KeyC, KeyX, KeyV, KeyZ, KeyY:
		if m == Cmd {
			return true
		}
	}
	if m == Ctrl && runtime.GOOS == "darwin" {
		if _, ok := emacsKeys[k.key]; ok || k.key == KeyA || k.key == KeyE || k.key == KeyK {
			return true
		}
	}
	// Printable keys type text, which comes as TextInput.
	return m == 0 || m == Alt && runtime.GOOS == "darwin"
}

// record starts a step of undo before an edit; typing in a row makes one
// step.
func (ed *editor) record(typing bool) {
	now := time.Now()
	if typing && ed.coalesce && now.Sub(ed.lastEdit) < time.Second && len(ed.undo) > 0 {
		ed.lastEdit = now
		return
	}
	ed.undo = append(ed.undo, undoStep{caret: ed.caret, anchor: ed.anchor})
	if len(ed.undo) > 200 {
		ed.undo = ed.undo[1:]
	}
	ed.redo = ed.redo[:0]
	ed.coalesce = typing
	ed.lastEdit = now
}

// edit replaces the runes from a to z with s, and tells the area.
func (ed *editor) edit(a, z int, s string) {
	first, old, after := ed.buf.replace(a, z, s)
	if ed.area != nil {
		ed.area.edited(&ed.buf, first, old, after)
	}
}

// replace replaces the runes from start to end with s, as the last step
// of undo notes, and puts the caret after it.
func (ed *editor) replace(start, end int, s string) {
	if !ed.multiline {
		s = strings.Map(func(r rune) rune {
			if r == '\n' || r == '\r' {
				return ' '
			}
			return r
		}, s)
	}
	if n := len(ed.undo); n > 0 {
		step := &ed.undo[n-1]
		c := change{at: start, removed: ed.buf.slice(start, end), inserted: s}
		// Typing grows the text the step inserted.
		grown := false
		if k := len(step.changes); k > 0 && c.removed == "" {
			if last := &step.changes[k-1]; !last.app && last.at+utf8.RuneCountInString(last.inserted) == start {
				last.inserted += s
				grown = true
			}
		}
		if !grown {
			step.changes = append(step.changes, c)
		}
	}
	ed.edit(start, end, s)
	ed.caret = start + utf8.RuneCountInString(s)
	ed.anchor = ed.caret
	if n := len(ed.undo); n > 0 {
		ed.undo[n-1].caretAfter, ed.undo[n-1].anchorAfter = ed.caret, ed.anchor
	}
}

// takeBack undoes the last step of undo, or redoes the last undone with
// redo.
func (ed *editor) takeBack(redo bool) {
	from, to := &ed.undo, &ed.redo
	if redo {
		from, to = to, from
	}
	n := len(*from)
	if n == 0 {
		return
	}
	step := (*from)[n-1]
	*from = (*from)[:n-1]
	if redo {
		for _, c := range step.changes {
			ed.edit(c.at, c.at+utf8.RuneCountInString(c.removed), c.inserted)
		}
		ed.caret, ed.anchor = step.caretAfter, step.anchorAfter
	} else {
		for i := len(step.changes) - 1; i >= 0; i-- {
			c := step.changes[i]
			ed.edit(c.at, c.at+utf8.RuneCountInString(c.inserted), c.removed)
		}
		ed.caret, ed.anchor = step.caret, step.anchor
	}
	*to = append(*to, step)
	ed.coalesce = false
	if ed.area != nil {
		ed.area.reveal = true
	}
}

func (ed *editor) insert(s string) {
	if ed.readOnly {
		return
	}
	s = strings.ReplaceAll(s, "\r\n", "\n")
	a, b := ed.selection()
	ed.record(a == b && utf8.RuneCountInString(s) == 1)
	ed.replace(a, b, s)
	ed.hasDesired = false
}

func (ed *editor) deleteRange(a, b int) {
	if a == b || ed.readOnly {
		return
	}
	ed.record(false)
	ed.replace(a, b, "")
	ed.hasDesired = false
}

func (ed *editor) move(to int, extend bool) {
	to = max(0, min(to, ed.buf.n))
	ed.caret = to
	if !extend {
		ed.anchor = to
	}
	ed.coalesce = false
	if ed.area != nil {
		ed.area.reveal = true
	}
}

// lineEdges returns the edges of the visual line holding rune i.
func (ed *editor) lineEdges(i int) (int, int) {
	if ed.area != nil {
		return ed.area.lineEdges(ed, i)
	}
	if ed.layout == nil || len(ed.layout.Lines) == 0 {
		return 0, ed.buf.n
	}
	li := ed.layout.LineAt(ed.displayIndex(i))
	line := ed.layout.Lines[li]
	return ed.textIndex(line.Start), ed.textIndex(line.End)
}

// displayIndex maps a rune of the text to the layout, which holds the
// composition at the caret.
func (ed *editor) displayIndex(i int) int {
	if ed.compose != "" && i > ed.caret {
		return i + utf8.RuneCountInString(ed.compose)
	}
	return i
}

func (ed *editor) textIndex(i int) int {
	if ed.compose != "" {
		n := utf8.RuneCountInString(ed.compose)
		switch {
		case i > ed.caret+n:
			return i - n
		case i > ed.caret:
			return ed.caret
		}
	}
	return min(i, ed.buf.n)
}

// vertical moves the caret up or down lines, keeping its x.
func (ed *editor) vertical(lines int, extend bool) {
	if a := ed.area; a != nil {
		x, y, h := a.caretAt(ed, ed.caret, 0)
		if !ed.hasDesired {
			ed.desiredX, ed.hasDesired = x, true
		}
		target := y + float64(h)/2 + float64(lines)*float64(h)
		switch {
		case target < 0:
			ed.move(0, extend)
		case target > a.hs.top(len(ed.buf.paras)):
			ed.move(ed.buf.n, extend)
		default:
			ed.move(a.indexAt(ed, ed.desiredX, target), extend)
		}
		return
	}
	if ed.layout == nil {
		return
	}
	x, y, h := ed.layout.Caret(ed.displayIndex(ed.caret))
	if !ed.hasDesired {
		ed.desiredX, ed.hasDesired = x, true
	}
	target := y + h/2 + float32(lines)*h
	if target < 0 {
		ed.move(0, extend)
		return
	}
	if target > ed.layout.Height {
		ed.move(ed.buf.n, extend)
		return
	}
	ed.move(ed.textIndex(ed.layout.IndexAt(ed.desiredX, target)), extend)
}

// emacsKeys are the Control keys of macOS text fields that act as other
// keys, after Emacs.
var emacsKeys = map[Key]Key{KeyB: KeyLeft, KeyF: KeyRight, KeyP: KeyUp, KeyN: KeyDown, KeyH: KeyBackspace, KeyD: KeyDelete}

func (ed *editor) key(c *Context, st *state, k editEvent) {
	shift := k.mods&Shift != 0
	m := k.mods &^ Shift
	mac := runtime.GOOS == "darwin"
	if mac && m == Ctrl {
		if to, ok := emacsKeys[k.key]; ok {
			k.key, m = to, 0
		} else if ed.emacsKey(k.key, shift) {
			return
		}
	}
	word := (!mac && m == Ctrl) || (mac && m == Alt)
	b := &ed.buf
	a, z := ed.selection()
	switch k.key {
	case KeyLeft, KeyRight:
		left := k.key == KeyLeft
		switch {
		case mac && m == Super:
			s, e := ed.lineEdges(ed.caret)
			if left {
				ed.move(s, shift)
			} else {
				ed.move(e, shift)
			}
		case word && left:
			ed.move(b.prevWord(ed.caret), shift)
		case word:
			ed.move(b.nextWord(ed.caret), shift)
		case a != z && !shift && left:
			ed.move(a, false)
		case a != z && !shift:
			ed.move(z, false)
		case left:
			ed.move(ed.graphemes.prev(b, ed.caret), shift)
		default:
			ed.move(ed.graphemes.next(b, ed.caret), shift)
		}
		ed.hasDesired = false
		return
	case KeyUp, KeyDown:
		if mac && m == Super || !ed.multiline {
			if k.key == KeyUp {
				ed.move(0, shift)
			} else {
				ed.move(b.n, shift)
			}
			return
		}
		d := 1
		if k.key == KeyUp {
			d = -1
		}
		ed.vertical(d, shift)
		return
	case KeyPageUp, KeyPageDown:
		n := max(int(st.h/max(ed.lineHeight(), 1))-1, 1)
		if k.key == KeyPageUp {
			n = -n
		}
		ed.vertical(n, shift)
		return
	case KeyHome, KeyEnd:
		if m == Ctrl || !ed.multiline {
			if k.key == KeyHome {
				ed.move(0, shift)
			} else {
				ed.move(b.n, shift)
			}
			return
		}
		s, e := ed.lineEdges(ed.caret)
		if k.key == KeyHome {
			ed.move(s, shift)
		} else {
			ed.move(e, shift)
		}
		return
	case KeyBackspace:
		switch {
		case a != z:
			ed.deleteRange(a, z)
		case mac && m == Super:
			s, _ := ed.lineEdges(ed.caret)
			ed.deleteRange(s, ed.caret)
		case word:
			ed.deleteRange(b.prevWord(ed.caret), ed.caret)
		case ed.caret > 0:
			ed.record(true)
			ed.replace(ed.graphemes.prev(b, ed.caret), ed.caret, "")
		}
		ed.hasDesired = false
		return
	case KeyDelete:
		switch {
		case a != z:
			ed.deleteRange(a, z)
		case word:
			ed.deleteRange(ed.caret, b.nextWord(ed.caret))
		case ed.caret < b.n:
			ed.deleteRange(ed.caret, ed.graphemes.next(b, ed.caret))
		}
		return
	case KeyEnter:
		if ed.multiline {
			ed.insert("\n")
		} else {
			st.submitted, st.submitMods = true, k.mods
			c.rt.consumed = true
		}
		return
	}
	if m != Cmd {
		return
	}
	switch k.key {
	case KeyA:
		ed.selectAll()
	case KeyC:
		ed.command(c, "copy")
	case KeyX:
		ed.command(c, "cut")
	case KeyV:
		ed.command(c, "paste")
	case KeyZ:
		if shift {
			ed.command(c, "redo")
		} else {
			ed.command(c, "undo")
		}
	case KeyY:
		ed.command(c, "redo")
	}
}

// emacsKey performs Control-A, -E and -K of macOS text fields, which move
// to the start and end of the paragraph and delete to its end, and reports
// whether key was one of them.
func (ed *editor) emacsKey(key Key, shift bool) bool {
	p := ed.buf.para(ed.caret)
	start, end := ed.buf.paras[p].rune, ed.buf.end(p)
	switch key {
	case KeyA:
		ed.move(start, shift)
	case KeyE:
		ed.move(end, shift)
	case KeyK:
		if end == ed.caret && end < ed.buf.n {
			end++ // at the end, join the next paragraph
		}
		ed.anchor = ed.caret
		ed.deleteRange(ed.caret, end)
	default:
		return false
	}
	ed.hasDesired = false
	return true
}

func (ed *editor) command(c *Context, name string) {
	a, b := ed.selection()
	h := c.rt.host
	if ed.readOnly && name != "copy" && name != "selectAll" {
		return
	}
	switch name {
	case "copy":
		if a != b && !ed.password {
			h.writeClipboard(ed.buf.slice(a, b))
		}
	case "cut":
		if a != b && !ed.password {
			h.writeClipboard(ed.buf.slice(a, b))
			ed.deleteRange(a, b)
		}
	case "paste":
		if s := h.readClipboard(); s != "" {
			ed.insert(s)
		}
	case "selectAll":
		ed.selectAll()
	case "delete":
		ed.deleteRange(a, b)
	case "undo":
		ed.takeBack(false)
	case "redo":
		ed.takeBack(true)
	}
}

// press puts the caret where the pointer went down, at (x, y) relative to
// the element; double and triple clicks select words and lines.
func (ed *editor) press(x, y float32, clicks, button int) {
	if button != 0 || !ed.laidOut() {
		return
	}
	ed.commitCompose()
	i := ed.hit(x, y)
	ed.dragging = true
	ed.hasDesired = false
	ed.dragUnit = min(clicks, 3)
	switch ed.dragUnit {
	case 1:
		ed.move(i, ed.pressMods&Shift != 0)
	case 2:
		s, e := ed.buf.wordAt(i)
		ed.anchor, ed.caret = s, e
	default:
		if ed.multiline {
			s, e := ed.lineEdges(i)
			ed.anchor, ed.caret = s, e
		} else {
			ed.selectAll()
		}
	}
	ed.dragStart = [2]int{ed.anchor, ed.caret}
	if ed.area != nil {
		ed.area.reveal = true
	}
}

func (ed *editor) release() { ed.dragging = false }

// laidOut reports whether a frame laid the editor's text out.
func (ed *editor) laidOut() bool { return ed.layout != nil || ed.area != nil && ed.area.version != 0 }

// firstLine returns the first line of the text, as last laid out, or nil.
func (ed *editor) firstLine() *text.Line {
	switch {
	case ed.area != nil && ed.area.version != 0:
		return ed.area.firstLine(&ed.buf)
	case ed.layout != nil && len(ed.layout.Lines) > 0:
		return &ed.layout.Lines[0]
	}
	return nil
}

func (ed *editor) hit(x, y float32) int {
	if a := ed.area; a != nil {
		return a.indexAt(ed, x-ed.originX, float64(y-ed.originY)+a.scroll)
	}
	if ed.layout == nil {
		return 0
	}
	return ed.textIndex(ed.layout.IndexAt(x-ed.originX+ed.scrollX, y-ed.originY))
}

// drag extends the selection to the pointer.
func (ed *editor) drag(x, y float32) {
	i := ed.hit(x, y)
	switch ed.dragUnit {
	case 2:
		s, e := ed.buf.wordAt(i)
		if i < ed.dragStart[0] {
			ed.anchor, ed.caret = ed.dragStart[1], s
		} else {
			ed.anchor, ed.caret = ed.dragStart[0], max(e, ed.dragStart[1])
		}
	case 3:
	default:
		ed.caret = i
	}
	if ed.area != nil {
		ed.area.reveal = true
	}
}

func (ed *editor) commitCompose() {
	if ed.compose != "" {
		ed.compose = ""
	}
}

func (ed *editor) lineHeight() float32 {
	if ed.area != nil && ed.area.line.Height > 0 {
		return ed.area.line.Height
	}
	if ed.layout != nil && len(ed.layout.Lines) > 0 {
		return ed.layout.Lines[0].Height
	}
	return 18
}

// caretRect returns the caret's box relative to the surface, where input
// methods show their candidates.
func (ed *editor) caretRect(st *state) Rect {
	if a := ed.area; a != nil && a.version != 0 {
		x, y, h := a.caretAt(ed, ed.caret, ed.composeCaret)
		return Rect{st.x + ed.originX + x, st.y + ed.originY + float32(y-a.scroll), 1, h}
	}
	if ed.layout == nil {
		return Rect{st.x, st.y, 1, st.h}
	}
	x, y, h := ed.layout.Caret(ed.displayIndex(ed.caret) + ed.composeCaret)
	return Rect{st.x + ed.originX + x - ed.scrollX, st.y + ed.originY + y, 1, h}
}

// process applies the input queued for the editor.
func (ed *editor) process(c *Context, e *Element) {
	st := e.st
	for _, ev := range ed.queue {
		if ev.replace {
			ed.anchor, ed.caret = min(ev.from, ed.buf.n), min(ev.to, ed.buf.n)
		}
		switch ev.kind {
		case editKey:
			ed.commitCompose()
			ed.key(c, st, ev)
		case editInsert:
			ed.compose = ""
			ed.insert(ev.text)
		case editCompose:
			if ed.readOnly {
				break
			}
			if ed.area != nil {
				ed.area.reveal = true
			}
			ed.compose = ev.text
			ed.composeCaret = max(0, min(ev.caret, utf8.RuneCountInString(ev.text)))
			if ev.text != "" {
				a, b := ed.selection()
				if a != b {
					ed.deleteRange(a, b)
				}
			}
		case editCommand:
			ed.commitCompose()
			ed.command(c, ev.text)
		}
	}
	ed.queue = ed.queue[:0]
	if ed.dragging && st.pressed {
		rt := c.rt
		ed.drag(rt.pointerX-st.x, rt.pointerY-st.y)
	}
}

// TextInput creates a single-line text input editing *value.
func TextInput(c *Context, value *string) *Element { return textInput(c, value, false) }

// TextArea creates a multi-line text input editing *value.
func TextArea(c *Context, value *string) *Element { return textInput(c, value, true) }

func textInput(c *Context, value *string, multiline bool) *Element {
	t := c.theme
	e := textInputBase(c, value, multiline)
	e.Padding(t.Space(1.5), t.Space(2.5)).Radius(t.Radius).Background(t.Surface).Border(1, t.Border)
	if multiline {
		e.MinHeight(t.Space(20))
	}
	e.styleFn = func(e *Element) { inputBorder(t, e, e) }
	return e
}

func textInputBase(c *Context, value *string, multiline bool) *Element {
	e := c.newElement(kindInput)
	e.flags |= flagEditable | flagFocusable | flagHover
	e.widget = "TextInput"
	if multiline {
		e.widget = "TextArea"
		// It scrolls its text as a scroll container does its children.
		e.flags |= flagScrollY
	}
	st := e.st
	if st.editor == nil {
		st.editor = newEditor()
		st.editor.setText(*value)
		st.editor.caret, st.editor.anchor = st.editor.buf.n, st.editor.buf.n
	}
	ed := st.editor
	ed.multiline = multiline
	if multiline && ed.area == nil {
		ed.area = &area{reveal: true}
	}
	// The input shares the string of *value: the same string is equal at
	// once, whatever its length.
	if ed.buf.s != *value {
		ed.setText(*value)
		ed.compose = ""
	}
	focused := c.rt.focused == e.id
	// Input queued while the input had the focus applies even when the
	// focus left before this frame, as with text typed right before Tab.
	if focused || len(ed.queue) > 0 {
		version := ed.buf.version
		ed.process(c, e)
		if ed.buf.version != version {
			if ed.buf.s != *value {
				st.changed = true
				c.rt.consumed = true
			}
			// Equal or not, the value shares the text's string again.
			*value = ed.buf.s
		}
	}
	if !focused {
		ed.compose = ""
	}
	// ReadOnly says again for the next frame's input, as the frame builds.
	ed.readOnly = false
	return e
}

// Placeholder shows s in an empty text input.
func (e *Element) Placeholder(s string) *Element {
	if e.st.editor != nil {
		e.st.editor.placeholder = s
	}
	return e
}

// ReadOnly makes a text input show its text without letting the user
// change it: the text can still be selected and copied, from the keyboard
// too, as it takes the focus, without a caret; assistive technology reads
// it as read-only.
func (e *Element) ReadOnly(on bool) *Element {
	if ed := e.st.editor; ed != nil && e.flags&flagEditable != 0 {
		ed.readOnly = on
		if on {
			ed.compose = ""
		}
	}
	return e
}

// Composing reports whether an input method composes text in a text
// input, as Pinyin before a candidate is chosen: its value holds the text
// once composed. Keys typed meanwhile are the input method's, as Enter
// choosing a candidate: they submit nothing and press no shortcut.
func (e *Element) Composing() bool {
	ed := e.st.editor
	return ed != nil && ed.compose != ""
}

// Password hides what a text input holds. It does nothing to a text area:
// as on every platform, only single-line fields hide their text.
func (e *Element) Password() *Element {
	if ed := e.st.editor; ed != nil && !ed.multiline {
		ed.password = true
	}
	return e
}

// Selectable lets the user select the text of a Text element, by dragging
// over it, double-clicking a word or triple-clicking a line, and copy it:
// a click gives it the keyboard focus, for Shift with the arrows and
// Cmd+C, and its context menu has Copy and Select All.
func (e *Element) Selectable() *Element {
	if e.kind != kindText {
		return e
	}
	e.flags |= flagSelectable
	st := e.st
	ed := st.editor
	if ed == nil {
		ed = newEditor()
		ed.readOnly, ed.multiline = true, true
		st.editor = ed
	}
	if ed.source != e.text {
		ed.source = e.text
		ed.setText(e.text)
		ed.caret, ed.anchor = 0, 0
	}
	if e.c.rt.focused == e.id || len(ed.queue) > 0 {
		ed.process(e.c, e)
	}
	return e
}

// displayText returns what the input shows: the text with the
// composition at the caret, bullets for a password.
func (ed *editor) displayText() string {
	t := ed.buf.s
	if ed.compose != "" {
		at := ed.buf.byteOf(ed.caret)
		t = t[:at] + ed.compose + t[at:]
	}
	if ed.password {
		return strings.Repeat("•", utf8.RuneCountInString(t))
	}
	return t
}

func (e *Element) inputParams(width float32) text.Params {
	p := e.textParams(width)
	p.Text = e.st.editor.displayText()
	p.KeepSpaces = true
	p.MaxLines = 0
	if !e.st.editor.multiline {
		p.Width = 0
	}
	return p
}

func (e *Element) inputHeight() float32 {
	ed := e.st.editor
	if ed.area != nil {
		// As high as its paragraphs unwrapped, without laying them out.
		p := e.textParams(0)
		p.Text = ""
		line := textSystem().Layout(p).Lines[0].Height
		return float32(max(len(ed.buf.paras), 3)) * line
	}
	l := textSystem().Layout(e.inputParams(0))
	return l.Lines[0].Height
}

func (e *Element) layoutInput(cw, ch float32) {
	ed := e.st.editor
	if a := ed.area; a != nil {
		ed.contentW = cw
		ed.originX, ed.originY = e.contentX(), e.contentY()
		a.layout(e, cw, ch)
		return
	}
	l := textSystem().Layout(e.inputParams(cw))
	ed.layout = l
	ed.contentW = cw
	ed.originX, ed.originY = e.contentX(), e.contentY()
	// A single line, centered vertically in a taller box: text areas lay
	// out in area.
	if len(l.Lines) > 0 {
		ed.originY += max((ch-l.Lines[0].Height)/2, 0)
	}
	// Keep the caret in view.
	x, _, _ := l.Caret(ed.displayIndex(ed.caret) + ed.composeCaret)
	if x-ed.scrollX < 0 {
		ed.scrollX = x
	} else if x-ed.scrollX > cw-1 {
		ed.scrollX = x - cw + 1
	}
	ed.scrollX = max(0, min(ed.scrollX, max(l.Width-cw+1, 0)))
}

func (e *Element) paintInput(p *Painter) {
	ed := e.st.editor
	l := ed.layout
	t := e.c.theme
	if l == nil && ed.area == nil {
		return
	}
	box := e.contentBox()
	clip := Rect{e.x + e.border[3], e.y + e.border[0], e.w - e.border[1] - e.border[3], e.h - e.border[0] - e.border[2]}
	saved := p.clip
	p.pushClip(clip, [4]float32{})
	ox, oy := e.x+ed.originX-ed.scrollX, e.y+ed.originY
	focused := e.Focused()
	ts := e.resolvedText()
	if ed.buf.n == 0 && ed.compose == "" && ed.placeholder != "" {
		pl := textSystem().Layout(text.Params{Text: ed.placeholder, Style: text.Style{Family: ts.family, Size: ts.size, Weight: ts.weight, LineHeight: ts.lineHeight}, Width: box.W})
		p.textLayout(pl, ox, oy, t.TextMuted, ts, nil)
	}
	if ed.area != nil {
		ed.area.paint(e, p, e.x+ed.originX, e.y+ed.originY)
		p.popClip()
		p.clip = saved
		return
	}
	if a, b := ed.selection(); a != b && focused {
		for _, r := range l.Selection(ed.displayIndex(a), ed.displayIndex(b)) {
			p.Fill(Rect{ox + r.X, oy + r.Y, r.W, r.H}, t.Selection, 0)
		}
	}
	p.textLayout(l, ox, oy, ts.color, ts, nil)
	if ed.compose != "" {
		start := ed.caret
		end := start + utf8.RuneCountInString(ed.compose)
		for _, r := range l.Selection(start, end) {
			p.Fill(Rect{ox + r.X, oy + r.Y + r.H - 2, r.W, 1}, ts.color, 0)
		}
	}
	if focused && !ed.readOnly {
		rt := e.c.rt
		phase := time.Since(rt.blinkStart)
		const blink = 530 * time.Millisecond
		if (phase/blink)%2 == 0 {
			x, y, h := l.Caret(ed.displayIndex(ed.caret) + ed.composeCaret)
			p.s.Ops = append(p.s.Ops, scene.Op{Kind: scene.OpFill, Rect: p.snap(Rect{ox + x, oy + y, 0, h}), Color: t.Accent.scene(), Wide: p.wide(t.Accent, Color{}, Color{}), Opacity: p.opacity})
			op := &p.s.Ops[len(p.s.Ops)-1]
			op.Rect.W = max(round(p.scale), 1)
		}
		if phase < 30*time.Second {
			e.c.After(blink - phase%blink)
		}
	}
	p.popClip()
	p.clip = saved
}
