package terminal

import (
	"bytes"
	"encoding/base64"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/plugins/terminal/internal/vt"
	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui"
	"golang.org/x/image/font/gofont/gomono"
	"golang.org/x/image/font/gofont/gomonobold"

	// MyGo's own strings in Traditional Chinese, which a test shows.
	_ "github.com/egoist/mygo/locales/zh-Hant"
)

// loadLib loads libghostty-vt, or skips the test where it cannot.
func loadLib(t *testing.T) {
	t.Helper()
	if testing.Short() && os.Getenv("MYGO_GHOSTTY_VT") == "" {
		t.Skip("downloads libghostty-vt")
	}
	if err := Load(); err != nil {
		t.Fatal(err)
	}
}

// waitFor runs frames until cond holds: what a program writes arrives
// when it does, on the terminal's reader. The deadline is a hang guard.
func waitFor(t *testing.T, tt *ui.Tester, what string, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(time.Minute)
	for !cond() {
		if time.Now().After(deadline) {
			t.Fatalf("timed out waiting for %s", what)
		}
		time.Sleep(10 * time.Millisecond)
		tt.Frame()
	}
}

func shell(t *testing.T, opts Options) *Terminal {
	t.Helper()
	if runtime.GOOS == "windows" {
		t.Skip("uses sh")
	}
	if opts.Command == nil {
		opts.Command = []string{"/bin/sh"}
	}
	opts.Env = append(opts.Env, "PS1=$ ", "ENV=")
	term, err := New(opts)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { term.Close() })
	return term
}

func save(t *testing.T, tt *ui.Tester, name string) {
	dir := os.Getenv("MYGO_TEST_IMAGES")
	if dir == "" {
		return
	}
	f, err := os.Create(dir + "/" + name + ".png")
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	png.Encode(f, tt.Image())
}

func TestTypeInShell(t *testing.T) {
	loadLib(t)
	term := shell(t, Options{})
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 480, 240)
	waitFor(t, tt, "the prompt", func() bool { return strings.Contains(term.Text(), "$") })
	for _, r := range "echo hi there" {
		tt.Type(string(r))
	}
	tt.Key(0, ui.KeyEnter)
	waitFor(t, tt, "the output", func() bool { return strings.Contains(term.Text(), "\nhi there\n") })
	if cols, rows := term.Size(); cols < 40 || rows < 10 {
		t.Errorf("size = %d×%d", cols, rows)
	}
	save(t, tt, "shell")
}

// pipe is a Conn whose output the test writes with Feed, and which keeps
// what the terminal sends.
type pipe struct {
	closed chan struct{}
	mu     sync.Mutex
	sent   []byte
}

func newPipe() *pipe { return &pipe{closed: make(chan struct{})} }

func (p *pipe) Read([]byte) (int, error) {
	<-p.closed
	return 0, os.ErrClosed
}

func (p *pipe) Write(b []byte) (int, error) {
	p.mu.Lock()
	p.sent = append(p.sent, b...)
	p.mu.Unlock()
	return len(b), nil
}

// take returns what term sent since the last take: all that it queued
// for its Conn before take, which waits until the queue wrote it (see
// written), and nothing queued after.
func (p *pipe) take(term *Terminal) string {
	written(term)
	p.mu.Lock()
	defer p.mu.Unlock()
	s := string(p.sent)
	p.sent = nil
	return s
}

// written returns once the input queue of term, which writes to its Conn
// (or program) from a goroutine of its own, wrote what was queued before,
// and ran what was queued to run once that was written, as the statuses
// of a paste: it queues a marker, which the queue reaches in order. A
// closed queue writes nothing more.
func written(term *Terminal) {
	done := make(chan struct{})
	q := &term.in
	q.mu.Lock()
	q.init()
	if q.closed {
		q.mu.Unlock()
		return
	}
	q.queue = append(q.queue, queued{done: func() { close(done) }})
	q.cond.Signal()
	q.mu.Unlock()
	select {
	case <-done:
	case <-time.After(time.Minute): // a hang guard
		panic("the terminal's input queue wrote nothing for a minute")
	}
}
func (p *pipe) Close() error {
	select {
	case <-p.closed:
	default:
		close(p.closed)
	}
	return nil
}

func TestPaint(t *testing.T) {
	loadLib(t)
	term, err := New(Options{Conn: newPipe()})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	var b strings.Builder
	for i := range 16 {
		b.WriteString("\x1b[48;5;" + itoa(i) + "m  ")
	}
	b.WriteString("\x1b[0m\r\n")
	for i := range 32 {
		b.WriteString("\x1b[48;2;" + itoa(i*8) + ";100;" + itoa(255-i*8) + "m ")
	}
	b.WriteString("\x1b[0m\r\n")
	b.WriteString("\x1b[1mbold\x1b[0m \x1b[3mitalic\x1b[0m \x1b[1;3mboth\x1b[0m \x1b[2mfaint\x1b[0m \x1b[4munder\x1b[0m \x1b[4:3mcurly\x1b[0m \x1b[21mdouble\x1b[0m \x1b[9mstrike\x1b[0m \x1b[7minverse\x1b[0m\r\n")
	b.WriteString("\x1b[31mred \x1b[32mgreen \x1b[33myellow \x1b[34mblue \x1b[35mmagenta \x1b[36mcyan \x1b[91mbright\x1b[0m\r\n")
	b.WriteString("wide: 世界 こんにちは 한국어 | emoji: 👍🏽 🇯🇵 👨‍👩‍👧 | combining: é́ ä\r\n")
	b.WriteString("┌──┬──┐ ╭──╮ ╔══╦══╗ ┏━━┓ █▓▒░ ▀▄▌▐ ▖▗▘▝\r\n")
	b.WriteString("│ab│cd│ │  │ ║  ║  ║ ┃  ┃ \r\n")
	b.WriteString("├──┼──┤ ╰──╯ ╠══╬══╣ ┗━━┛ ╱╲╳\r\n")
	b.WriteString("└──┴──┘      ╚══╩══╝\r\n")
	b.WriteString("ligatures? -> => != === <= >=  www  fi fl")
	term.Feed([]byte(b.String()))
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 640, 280)
	tt.SetScale(2)
	save(t, tt, "paint")
}

func itoa(i int) string {
	return strings.TrimSpace(strings.Repeat(" ", 0) + string(rune('0'+i/100%10)) + string(rune('0'+i/10%10)) + string(rune('0'+i%10)))
}

func TestScrollback(t *testing.T) {
	loadLib(t)
	term, err := New(Options{Conn: newPipe()})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	var b strings.Builder
	for i := range 100 {
		b.WriteString("line " + itoa(i) + "\r\n")
	}
	term.Feed([]byte(b.String()))
	tt.Frame()
	offset := func() uint64 {
		term.mu.Lock()
		defer term.mu.Unlock()
		return term.term.Scrollbar().Offset
	}
	bottom := offset()
	tt.Scroll(100, 100, 0, -40) // a notch of a mouse wheel up
	if got := offset(); got != bottom-3 {
		t.Errorf("after scrolling a notch up, offset = %d, want %d", got, bottom-3)
	}
	tt.Key(ui.Shift, ui.KeyPageUp)
	if got := offset(); got >= bottom-3 {
		t.Errorf("Shift+Page Up did not scroll: offset = %d", got)
	}
	save(t, tt, "scrolled")
	// Typing goes back to the bottom.
	tt.Type("x")
	if got := offset(); got != bottom {
		t.Errorf("after typing, offset = %d, want %d", got, bottom)
	}
}

// A snapshot of a terminal no view shows restores what it shows in
// another, as a server keeping sessions sends it to a window attaching.
func TestSnapshot(t *testing.T) {
	loadLib(t)
	newTerm := func() *Terminal {
		term, err := New(Options{Conn: newPipe()})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { term.Close() })
		term.Resize(40, 6)
		return term
	}
	a := newTerm()
	for i := range 10 {
		a.Feed([]byte("line " + itoa(i) + " \x1b[31mred\x1b[0m \x1b[1;4mbold\x1b[0m\r\n"))
	}
	a.Feed([]byte("prompt % \x1b[?2004h"))
	snap := a.Snapshot()
	b := newTerm()
	b.Feed(snap)
	// The cursor and the modes come too: what follows lands alike.
	a.Feed([]byte("x"))
	b.Feed([]byte("x"))
	if a.Text() != b.Text() {
		t.Errorf("restored text\n%q, want\n%q", b.Text(), a.Text())
	}
	b.mu.Lock()
	paste := b.term.Mode(vt.ModeBracketedPaste)
	b.mu.Unlock()
	if !paste {
		t.Error("bracketed paste was not restored")
	}
	// A full-screen program's screen, at a new size.
	a.Feed([]byte("\x1b[?1049h\x1b[H\x1b[2Jfull screen\x1b[3;5Hhere"))
	a.Resize(30, 5)
	c := newTerm()
	c.Resize(30, 5)
	c.Feed(a.Snapshot())
	c.Feed([]byte("!"))
	if got := c.Text(); !strings.Contains(got, "full screen") || !strings.Contains(got, "here!") {
		t.Errorf("restored alternate screen %q", got)
	}
}

// cellCenter returns the middle of a cell of the view, in DIPs.
func cellCenter(term *Terminal, col, row int) (float32, float32) {
	v := term.v
	return (float32(v.ox) + (float32(col)+0.5)*float32(v.cellW)) / v.scale, (float32(v.oy) + (float32(row)+0.5)*float32(v.cellH)) / v.scale
}

func TestSelectAndCopy(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	term, err := New(Options{Conn: conn})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	term.Feed([]byte("hello world\r\nsecond line"))
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	copyMods := ui.Ctrl | ui.Shift
	if runtime.GOOS == "darwin" {
		copyMods = ui.Super
	}
	x0, y0 := cellCenter(term, 0, 0)
	x1, y1 := cellCenter(term, 4, 0)
	tt.Press(x0-2, y0)
	tt.Move(x1, y1)
	tt.Move(x1+2, y1)
	tt.Release(x1+2, y1)
	tt.Key(copyMods, ui.KeyC)
	if got := tt.Clipboard(); got != "hello" {
		t.Errorf("dragging copied %q", got)
	}
	// A double click selects a word, a triple click a line.
	x, y := cellCenter(term, 8, 0)
	tt.Press(x, y)
	tt.Release(x, y)
	tt.Press(x, y)
	tt.Release(x, y)
	tt.Key(copyMods, ui.KeyC)
	if got := tt.Clipboard(); got != "world" {
		t.Errorf("double click copied %q", got)
	}
	x, y = cellCenter(term, 2, 1)
	for range 3 {
		tt.Press(x, y)
		tt.Release(x, y)
	}
	tt.Key(copyMods, ui.KeyC)
	if got := tt.Clipboard(); got != "second line" {
		t.Errorf("triple click copied %q", got)
	}
	save(t, tt, "selected")
	// The Edit menu's Copy and Select All.
	tt.Key(0, ui.KeyEscape) // typing clears the selection
}

func TestPaste(t *testing.T) {
	loadLib(t)
	term := shell(t, Options{Command: []string{"/bin/cat"}})
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	tt.SetClipboard("pasted text")
	pasteMods := ui.Ctrl | ui.Shift
	if runtime.GOOS == "darwin" {
		pasteMods = ui.Super
	}
	tt.Key(pasteMods, ui.KeyV)
	tt.Key(0, ui.KeyEnter)
	waitFor(t, tt, "the echo of cat", func() bool { return strings.Count(term.Text(), "pasted text") == 2 })
}

func TestKeys(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	term, err := New(Options{Conn: conn})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	key := func(mods ui.Modifiers, k ui.Key, text, want string) {
		t.Helper()
		if text != "" {
			tt.TypeKey(mods, k, text)
		} else {
			tt.Key(mods, k)
		}
		if got := conn.take(term); got != want {
			t.Errorf("%v %v %q sent %q, want %q", mods, k, text, got, want)
		}
	}
	key(0, ui.KeyA, "a", "a")
	key(ui.Shift, ui.KeyA, "A", "A")
	key(ui.Ctrl, ui.KeyC, "", "\x03")
	key(0, ui.KeyEnter, "", "\r")
	key(0, ui.KeyBackspace, "", "\x7f")
	key(0, ui.KeyUp, "", "\x1b[A")
	key(ui.Shift, ui.KeyTab, "", "\x1b[Z")
	key(0, ui.KeyF5, "", "\x1b[15~")
	if runtime.GOOS != "darwin" {
		key(ui.Alt, ui.KeyB, "", "\x1bb")
	}
	// Text without its key, as input methods and Linux send it.
	tt.Type("世界")
	if got := conn.take(term); got != "世界" {
		t.Errorf("typed %q", got)
	}
	// Programs that ask for the Kitty keyboard protocol get it.
	term.Feed([]byte("\x1b[>1u"))
	key(0, ui.KeyEscape, "", "\x1b[27u")
	key(ui.Ctrl, ui.KeyC, "", "\x1b[99;5u")
	// Programs in cursor key mode get application keys.
	term.Feed([]byte("\x1b[<u\x1b[?1h"))
	key(0, ui.KeyUp, "", "\x1bOA")
}

func TestComposition(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	term, err := New(Options{Conn: conn})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	term.Feed([]byte("abc"))
	tt.Frame()
	caret, ok := tt.TextCaret()
	x, y := cellCenter(term, 3, 0)
	if !ok || !caret.Contains(x, y) {
		t.Errorf("input methods compose at %+v (%v), not in the cell of the cursor at %v, %v", caret, ok, x, y)
	}
	tt.Compose("ni", 2)
	save(t, tt, "composing")
	if got := conn.take(term); got != "" {
		t.Errorf("composing sent %q", got)
	}
	tt.Compose("你", 1)
	tt.Type("你")
	if got := conn.take(term); got != "你" {
		t.Errorf("committing sent %q", got)
	}
}

func TestMouse(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	term, err := New(Options{Conn: conn})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	term.Feed([]byte("\x1b[?1002h\x1b[?1006h"))
	tt.Frame()
	x, y := cellCenter(term, 2, 3)
	tt.Press(x, y)
	if got, want := conn.take(term), "\x1b[<0;3;4M"; got != want {
		t.Errorf("press sent %q, want %q", got, want)
	}
	x2, y2 := cellCenter(term, 5, 3)
	tt.Move(x2, y2)
	if got, want := conn.take(term), "\x1b[<32;6;4M"; got != want {
		t.Errorf("drag sent %q, want %q", got, want)
	}
	tt.Release(x2, y2)
	if got, want := conn.take(term), "\x1b[<0;6;4m"; got != want {
		t.Errorf("release sent %q, want %q", got, want)
	}
	tt.Scroll(x, y, 0, -40)
	if got := conn.take(term); !strings.HasPrefix(got, "\x1b[<64;3;4M") {
		t.Errorf("wheel sent %q", got)
	}
	// Focus reports.
	term.Feed([]byte("\x1b[?1004h"))
	tt.Frame()
}

func TestExit(t *testing.T) {
	loadLib(t)
	exited := make(chan int, 1)
	var titles []string
	var mu sync.Mutex
	term := shell(t, Options{
		Command: []string{"/bin/sh", "-c", "printf '\\033]2;the title\\007'; echo bye; exit 3"},
		OnExit:  func(code int) { exited <- code },
		OnTitle: func(s string) { mu.Lock(); titles = append(titles, s); mu.Unlock() },
	})
	select {
	case code := <-exited:
		if code != 3 {
			t.Errorf("exit code = %d", code)
		}
	case <-time.After(time.Minute): // a hang guard
		t.Fatal("no exit")
	}
	<-term.Done()
	if term.ExitCode() != 3 || !strings.Contains(term.Text(), "bye\n[Process exited with code 3]") {
		t.Errorf("code %d, text %q", term.ExitCode(), term.Text())
	}
	mu.Lock()
	defer mu.Unlock()
	if len(titles) != 1 || titles[0] != "the title" || term.Title() != "the title" {
		t.Errorf("titles = %q", titles)
	}
}

func TestMenus(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	term, err := New(Options{Conn: conn})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	term.Feed([]byte("some text"))
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	// The Edit menu's roles.
	tt.Command("selectAll")
	tt.Command("copy")
	if got := tt.Clipboard(); got != "some text" {
		t.Errorf("Select All and Copy copied %q", got)
	}
	tt.SetClipboard("pasted")
	tt.Command("paste")
	if got := conn.take(term); got != "pasted" {
		t.Errorf("Paste sent %q", got)
	}
	// The context menu.
	x, y := cellCenter(term, 1, 0)
	tt.RightClickAt(x, y)
	if got := strings.Join(tt.Menu(), ","); !strings.Contains(got, "Copy") || !strings.Contains(got, "Clear Scrollback") {
		t.Errorf("context menu = %q", got)
	}
	tt.SetClipboard("again")
	if err := tt.ChooseMenuItem("Paste"); err != nil {
		t.Fatal(err)
	}
	if got := conn.take(term); got != "again" {
		t.Errorf("the menu's Paste sent %q", got)
	}
	// In the window's language.
	tt.SetLocale("zh-Hant")
	tt.RightClickAt(x, y)
	if got := strings.Join(tt.Menu(), ","); !strings.Contains(got, "複製") || !strings.Contains(got, "清除捲動緩衝區") {
		t.Errorf("context menu in Traditional Chinese = %q", got)
	}
}

func TestCopyOnSelect(t *testing.T) {
	loadLib(t)
	for _, on := range []CopyOnSelect{CopyOnSelectNone, CopyOnSelectClipboard} {
		conn := newPipe()
		term, err := New(Options{Conn: conn, CopyOnSelect: on})
		if err != nil {
			t.Fatal(err)
		}
		term.Feed([]byte("hello world"))
		tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
		x0, y0 := cellCenter(term, 0, 0)
		x1, y1 := cellCenter(term, 4, 0)
		tt.Press(x0-2, y0)
		tt.Move(x1+2, y1)
		tt.Release(x1+2, y1)
		want := ""
		if on == CopyOnSelectClipboard {
			want = "hello"
		}
		if got := tt.Clipboard(); got != want {
			t.Errorf("CopyOnSelect %v: selecting copied %q, want %q", on, got, want)
		}
		tt.Command("selectAll")
		if on == CopyOnSelectClipboard {
			want = "hello world"
		}
		if got := tt.Clipboard(); got != want {
			t.Errorf("CopyOnSelect %v: Select All copied %q, want %q", on, got, want)
		}
		term.Close()
	}
}

func TestSetCopyOnSelect(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	term, err := New(Options{Conn: conn})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	term.Feed([]byte("hello world"))
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	sel := func(from, to int) {
		x0, y0 := cellCenter(term, from, 0)
		x1, y1 := cellCenter(term, to, 0)
		tt.Press(x0-2, y0)
		tt.Move(x1+2, y1)
		tt.Release(x1+2, y1)
	}
	term.SetCopyOnSelect(CopyOnSelectClipboard)
	sel(0, 4)
	if got := tt.Clipboard(); got != "hello" {
		t.Errorf("turned on, selecting copied %q, want %q", got, "hello")
	}
	term.SetCopyOnSelect(CopyOnSelectNone)
	sel(6, 10)
	if got := tt.Clipboard(); got != "hello" {
		t.Errorf("turned off, selecting left %q on the clipboard, want %q", got, "hello")
	}
}

// TestCopyAndPasteKeys: Windows Terminal's keys, performable Control-C
// and -V, Control-Insert and Shift-Insert, and a secondary click that
// copies or pastes.
func TestCopyAndPasteKeys(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	term, err := New(Options{Conn: conn, RightClick: RightClickCopyOrPaste, Keybinds: []Keybind{
		{Mods: ui.Ctrl, Key: ui.KeyC, Action: ActionCopy, Performable: true},
		{Mods: ui.Ctrl, Key: ui.KeyV, Action: ActionPaste, Performable: true},
		{Mods: ui.Ctrl, Key: ui.KeyInsert, Action: ActionCopy},
		{Mods: ui.Shift, Key: ui.KeyInsert, Action: ActionPaste},
	}})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	term.Feed([]byte("some text"))
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	// Control-C copies a selection and lets go of it; without one it
	// interrupts.
	tt.Command("selectAll")
	tt.Key(ui.Ctrl, ui.KeyC)
	if got := tt.Clipboard(); got != "some text" {
		t.Errorf("Control-C copied %q", got)
	}
	if got := conn.take(term); got != "" {
		t.Errorf("Control-C with a selection sent %q", got)
	}
	tt.Key(ui.Ctrl, ui.KeyC)
	if got := conn.take(term); got != "\x03" {
		t.Errorf("Control-C without a selection sent %q", got)
	}
	tt.SetClipboard("by V")
	tt.Key(ui.Ctrl, ui.KeyV)
	if got := conn.take(term); got != "by V" {
		t.Errorf("Control-V sent %q", got)
	}
	// With nothing to paste, Control-V reaches the program.
	tt.SetClipboard("")
	tt.Key(ui.Ctrl, ui.KeyV)
	if got := conn.take(term); got != "\x16" {
		t.Errorf("Control-V with an empty clipboard sent %q", got)
	}
	tt.Command("selectAll")
	tt.Key(ui.Ctrl, ui.KeyInsert)
	if got := tt.Clipboard(); got != "some text" {
		t.Errorf("Control-Insert copied %q", got)
	}
	tt.SetClipboard("by Insert")
	tt.Key(ui.Shift, ui.KeyInsert)
	if got := conn.take(term); got != "by Insert" {
		t.Errorf("Shift-Insert sent %q", got)
	}
	// A secondary click copies a selection, else pastes, and shows no menu.
	x, y := cellCenter(term, 1, 0)
	tt.SetClipboard("")
	tt.Command("selectAll")
	tt.RightClickAt(x, y)
	if got := tt.Clipboard(); got != "some text" {
		t.Errorf("a secondary click over a selection copied %q", got)
	}
	tt.RightClickAt(x, y)
	if got := conn.take(term); got != "some text" {
		t.Errorf("a secondary click without a selection sent %q", got)
	}
	if menu := tt.Menu(); menu != nil {
		t.Errorf("a secondary click showed the menu %q", menu)
	}
}

// TestUnboundKeys: keys bound to ActionNone reach the program, as do
// Control-C and -V outside Windows, and a secondary click shows the menu.
func TestUnboundKeys(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	term, err := New(Options{Conn: conn, Keybinds: []Keybind{
		{Mods: ui.Ctrl, Key: ui.KeyC}, {Mods: ui.Ctrl, Key: ui.KeyV},
	}})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	term.Feed([]byte("some text"))
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	tt.Command("selectAll")
	tt.SetClipboard("clipboard")
	tt.Key(ui.Ctrl, ui.KeyC)
	if got := conn.take(term); got != "\x03" {
		t.Errorf("Control-C sent %q", got)
	}
	tt.Key(ui.Ctrl, ui.KeyV)
	if got := conn.take(term); got != "\x16" {
		t.Errorf("Control-V sent %q", got)
	}
	if got := tt.Clipboard(); got != "clipboard" {
		t.Errorf("the clipboard became %q", got)
	}
	x, y := cellCenter(term, 1, 0)
	tt.RightClickAt(x, y)
	if tt.Menu() == nil {
		t.Error("a secondary click showed no menu")
	}
}

// TestKeybinds: Options.Keybinds replace the defaults of their keys, in
// order, ActionNone unbinds, and NoDefaultKeybinds starts from none.
func TestKeybinds(t *testing.T) {
	copyKey := Keybind{Mods: ui.Ctrl | ui.Shift, Key: ui.KeyC, Action: ActionCopy}
	if runtime.GOOS == "darwin" {
		copyKey.Mods = ui.Super
	}
	if !slices.Contains(DefaultKeybinds(), copyKey) {
		t.Errorf("the defaults have no %v", copyKey)
	}
	win := defaultKeybinds("windows")
	for _, k := range []Keybind{
		{Mods: ui.Ctrl, Key: ui.KeyC, Action: ActionCopy, Performable: true},
		{Mods: ui.Ctrl, Key: ui.KeyV, Action: ActionPaste, Performable: true},
		{Mods: ui.Shift, Key: ui.KeyInsert, Action: ActionPaste},
	} {
		if !slices.Contains(win, k) {
			t.Errorf("Windows' defaults have no %v", k)
		}
	}
	if slices.ContainsFunc(defaultKeybinds("linux"), func(k Keybind) bool { return k.Mods == ui.Ctrl && k.Key == ui.KeyC }) {
		t.Error("Linux binds Control-C")
	}
	opts := Options{Keybinds: []Keybind{
		{Mods: copyKey.Mods, Key: ui.KeyC, Action: ActionPaste},
		{Mods: ui.Alt, Key: ui.KeyC, Action: ActionCopy},
		{Mods: ui.Alt, Key: ui.KeyC, Action: ActionSelectAll},
		{Mods: copyKey.Mods, Key: ui.KeyA},
	}}
	b := keybinds(&opts)
	term := &Terminal{binds: b}
	if k, ok := term.binding(copyKey.Mods, ui.KeyC); !ok || k.Action != ActionPaste {
		t.Errorf("the copy key is bound to %v, %v", k, ok)
	}
	if k, _ := term.binding(ui.Alt, ui.KeyC); k.Action != ActionSelectAll {
		t.Errorf("a key bound twice does %v", k.Action)
	}
	if _, ok := term.binding(copyKey.Mods, ui.KeyA); ok {
		t.Error("ActionNone left the key bound")
	}
	if len(b) != len(DefaultKeybinds()) {
		t.Errorf("%d bindings, want %d", len(b), len(DefaultKeybinds()))
	}
	opts.NoDefaultKeybinds = true
	if b := keybinds(&opts); len(b) != 2 {
		t.Errorf("without the defaults, bound %v", b)
	}
}

// TestKeybindActions: the defaults select all, clear the screen above the
// prompt, scroll the scrollback but for a full-screen program, and show in
// the context menu.
func TestKeybindActions(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	term, err := New(Options{Conn: conn, Keybinds: []Keybind{
		{Mods: ui.Alt, Key: ui.KeyK, Action: ActionClearScreen, Performable: true},
		{Mods: ui.Alt, Key: ui.KeyA, Action: ActionSelectAll},
		{Mods: ui.Alt, Key: ui.KeyX, Action: ActionIgnore},
	}})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	var b strings.Builder
	for i := range 100 {
		b.WriteString("line " + itoa(i) + "\r\n")
	}
	b.WriteString("$ ls")
	term.Feed([]byte(b.String()))
	tt.Frame()
	offset := func() uint64 {
		term.mu.Lock()
		defer term.mu.Unlock()
		return term.term.Scrollbar().Offset
	}
	bottom := offset()
	tt.Key(ui.Shift, ui.KeyPageUp)
	if offset() >= bottom {
		t.Error("Shift-Page Up did not scroll")
	}
	tt.Key(ui.Shift, ui.KeyPageDown)
	if offset() != bottom {
		t.Error("Shift-Page Down did not scroll back")
	}
	tt.Key(ui.Alt, ui.KeyX)
	if got := conn.take(term); got != "" {
		t.Errorf("an ignored key sent %q", got)
	}
	tt.Key(ui.Alt, ui.KeyA)
	tt.Command("copy")
	if got := tt.Clipboard(); !strings.HasPrefix(got, "line 000\n") {
		t.Errorf("Select All copied %.20q", got)
	}
	tt.Key(ui.Alt, ui.KeyK)
	tt.Frame()
	if got := strings.TrimSpace(term.Text()); got != "$ ls" {
		t.Errorf("clearing left %q", got)
	}
	if x, y := term.term.Cursor(); x != 4 || y != 0 {
		t.Errorf("the cursor is at %d,%d after clearing, want 4,0", x, y)
	}
	// A full-screen program gets the keys.
	term.Feed([]byte("\x1b[?1049h"))
	tt.Frame()
	tt.Key(ui.Shift, ui.KeyPageUp)
	if got := conn.take(term); got != "\x1b[5;2~" {
		t.Errorf("Shift-Page Up on the alternate screen sent %q", got)
	}
	tt.Key(ui.Alt, ui.KeyK)
	if got := conn.take(term); got == "" {
		t.Error("a performable clear took the key on the alternate screen")
	}
	// The context menu shows the keys.
	x, y := cellCenter(term, 1, 0)
	tt.RightClickAt(x, y)
	if got := strings.Join(tt.Menu(), ","); !strings.Contains(got, "Copy") {
		t.Errorf("context menu = %q", got)
	}
}

func TestAppearance(t *testing.T) {
	loadLib(t)
	term, err := New(Options{Conn: newPipe()})
	if err != nil {
		t.Fatal(err)
	}
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill() }, 400, 200)
	bg := func() [3]uint8 {
		c := tt.Image().RGBAAt(200, 190)
		return [3]uint8{c.R, c.G, c.B}
	}
	light := bg()
	tt.SetDark(true)
	if dark := bg(); dark == light || dark[0] > 64 {
		t.Errorf("the background did not follow the dark appearance: %v, then %v", light, dark)
	}
	cols, _ := term.Size()
	term.SetFont(Font{Size: 26})
	tt.Frame()
	if c2, _ := term.Size(); c2 >= cols {
		t.Errorf("a bigger font left %d columns, from %d", c2, cols)
	}
	// A closed terminal shows nothing, and takes no input.
	term.Close()
	tt.Frame()
	tt.Type("x")
	if term.Text() != "" {
		t.Error("a closed terminal has text")
	}
}

func TestThemes(t *testing.T) {
	loadLib(t)
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	latte, err := GhosttyTheme("Catppuccin Latte")
	if err != nil {
		t.Fatal(err)
	}
	mocha, _ := GhosttyTheme("Catppuccin Mocha")
	term, err := New(Options{Conn: newPipe(), Theme: latte, DarkTheme: mocha, NoBlink: true})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	// A block under the cursor, and two below it, before a space.
	term.Feed([]byte("\x1b[2;1H██ x\x1b[1;1H█\x1b[D"))
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	at := func(x, y float32) ui.Color {
		s := term.v.scale
		c := tt.Image().RGBAAt(int(x*s), int(y*s))
		return ui.RGB(c.R, c.G, c.B)
	}
	cell := func(col, row int) ui.Color { return at(cellCenter(term, col, row)) }
	near := func(name string, got, want ui.Color) {
		t.Helper()
		d := func(a, b uint8) int { return max(int(a)-int(b), int(b)-int(a)) }
		if d(got.R, want.R) > 2 || d(got.G, want.G) > 2 || d(got.B, want.B) > 2 {
			t.Errorf("%s is %v, want %v", name, got, want)
		}
	}
	near("the light background", at(200, 190), latte.Background)
	tt.SetDark(true)
	near("the dark background", at(200, 190), mocha.Background)

	th := &Theme{
		Foreground: ui.Hex("#c0c0c0"), Background: ui.Hex("#101010"),
		Cursor: ui.Hex("#ff0000"), CursorText: ui.Hex("#00ff00"),
		Selection: ui.Hex("#0000ff"), SelectionText: ui.Hex("#ffff00"),
	}
	term.SetTheme(th, nil)
	tt.Frame()
	near("the background", at(200, 190), th.Background)
	near("the text", cell(0, 1), th.Foreground)
	near("the text under the cursor", cell(0, 0), th.CursorText)
	tt.Command("selectAll")
	tt.Frame()
	near("the selected text", cell(0, 1), th.SelectionText)
	near("the selection", cell(2, 1), th.Selection)
	near("the selected text under the cursor", cell(0, 0), th.CursorText)
}

func TestFont(t *testing.T) {
	loadLib(t)
	for _, f := range [][]byte{gomono.TTF, gomonobold.TTF} {
		if err := ui.RegisterFont(f, "MyGo Test Mono"); err != nil {
			t.Fatal(err)
		}
	}
	// render returns the size of a cell in a font, and the ink of a row of
	// dark text on white.
	render := func(f Font) (w, h int, ink int) {
		f.Family, f.Size = "MyGo Test Mono", 14
		term, err := New(Options{Conn: newPipe(), Font: f, Theme: LightTheme(), NoBlink: true})
		if err != nil {
			t.Fatal(err)
		}
		defer term.Close()
		term.Feed([]byte("MMMMMMMMMM\r\n"))
		tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill() }, 400, 120)
		tt.SetScale(2)
		v := term.v
		img := tt.Image()
		for y := v.oy; y < v.oy+v.cellH; y++ {
			for x := v.ox; x < v.ox+10*v.cellW; x++ {
				ink += 255 - int(img.RGBAAt(x, y).G)
			}
		}
		return v.cellW, v.cellH, ink
	}
	w, h, ink := render(Font{})
	if w2, h2, _ := render(Font{LineHeight: 1.5}); w2 != w || h2 < h*3/2-1 || h2 > h*3/2+1 {
		t.Errorf("rows 1.5 times as high are %dx%d, from %dx%d", w2, h2, w, h)
	}
	if _, _, bold := render(Font{Weight: 700}); bold < ink*11/10 {
		t.Errorf("weight 700 has ink %d, 400 %d", bold, ink)
	}
	_, _, thick := render(Font{Thicken: true})
	if runtime.GOOS == "darwin" && thick < ink*103/100 {
		t.Errorf("thickened text has ink %d, else %d", thick, ink)
	} else if runtime.GOOS != "darwin" && thick != ink {
		t.Errorf("thickening changed text off macOS: ink %d, else %d", thick, ink)
	}
	if got := featureList([]string{"-calt", "+ss01", " cv05=2 ", "liga=0", ""}); got != "calt=0,ss01,cv05=2,liga=0" {
		t.Errorf("features are %q", got)
	}
	k := (&Font{Weight: 300, Features: []string{"-calt"}}).key(1)
	if f := k.variant(1); f.Weight != 700 || f.Features != "calt=0" {
		t.Errorf("the bold of weight 300 is %+v", f)
	}
	if f := (&Font{Weight: 800}).key(1).variant(3); f.Weight != 900 || !f.Italic {
		t.Errorf("the bold italic of weight 800 is %+v", f)
	}
}

func TestWindowsShell(t *testing.T) {
	if runtime.GOOS != "windows" {
		t.Skip("cmd.exe")
	}
	loadLib(t)
	term, err := New(Options{Command: []string{"cmd.exe", "/c", "echo hello from cmd"}})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill() }, 640, 300)
	select {
	case <-term.Done():
	case <-time.After(time.Minute): // a hang guard
		t.Fatal("cmd did not exit")
	}
	tt.Frame()
	if !strings.Contains(term.Text(), "hello from cmd") {
		t.Errorf("text = %q", term.Text())
	}
}

func TestStartsAtTheViewsSize(t *testing.T) {
	loadLib(t)
	term := shell(t, Options{Command: []string{"/bin/sh", "-c", "stty size; sleep 5"}})
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill() }, 300, 200)
	cols, rows := term.Size()
	want := fmt.Sprintf("%d %d", rows, cols)
	waitFor(t, tt, "stty", func() bool { return strings.Contains(term.Text(), " ") })
	if got := strings.TrimSpace(term.Text()); got != want {
		t.Errorf("the program saw %q, not the view's %q", got, want)
	}
}

func TestBlurEndsSelecting(t *testing.T) {
	loadLib(t)
	term, err := New(Options{Conn: newPipe()})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	term.Feed([]byte("hello world"))
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	x0, y0 := cellCenter(term, 0, 0)
	x1, y1 := cellCenter(term, 4, 0)
	tt.Press(x0-2, y0)
	tt.Move(x1+2, y1)
	tt.SetFocused(false)
	if term.v.selecting {
		t.Error("still selecting after the window lost the keyboard")
	}
	tt.SetFocused(true)
	x2, _ := cellCenter(term, 9, 0)
	tt.Move(x2, y1)
	tt.Command("copy")
	if got := tt.Clipboard(); got != "hello" {
		t.Errorf("the selection followed the pointer with no button held: %q", got)
	}
}

func TestKeyWithoutText(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	term, err := New(Options{Conn: conn})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	// A key that types text, whose text never comes, goes alone when
	// released.
	tt.Key(0, ui.KeyB)
	if got := conn.take(term); got != "b" {
		t.Errorf("sent %q", got)
	}
}

func TestCloseKillsWhatIgnoresHangups(t *testing.T) {
	loadLib(t)
	term := shell(t, Options{Command: []string{"/bin/sh", "-c", "trap '' HUP; echo ready; sleep 30"}})
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill() }, 300, 200)
	waitFor(t, tt, "the program", func() bool { return strings.Contains(term.Text(), "ready") })
	term.Close()
	select {
	case <-term.Done():
	case <-time.After(time.Minute): // a hang guard: Close kills it after 2 s
		t.Fatal("Done was not closed: the program outlived Close")
	}
}

// TestCommandClickLinks: Command+click hands URLs and printed paths to
// Options.OpenLink, and URLs it leaves to the system.
func TestCommandClickLinks(t *testing.T) {
	loadLib(t)
	var opened []string
	term, err := New(Options{Conn: newPipe(), OpenLink: func(link string) bool {
		opened = append(opened, link)
		return link != "https://x.org"
	}})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	term.Feed([]byte("see src/app.go:12 or https://x.org\r\nplain words"))
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	tt.Frame()
	for _, col := range []int{6, 24} {
		x, y := cellCenter(term, col, 0)
		tt.ClickAtWith(ui.Cmd, x, y)
	}
	x, y := cellCenter(term, 2, 1)
	tt.ClickAtWith(ui.Cmd, x, y)
	if want := []string{"src/app.go:12", "https://x.org"}; !slices.Equal(opened, want) {
		t.Errorf("opened %q, want %q", opened, want)
	}
	if u := tt.OpenedURLs(); !slices.Equal(u, []string{"https://x.org"}) {
		t.Errorf("system opened %q", u)
	}
}

// TestCommandClickLinkWithoutOpenLink: without Options.OpenLink, the system
// opens hyperlinks of any scheme, paths are no links, the pointer is a hand
// over a link while Command is held, and a click on a link goes before a
// program that takes the mouse, its release too.
func TestCommandClickLinkWithoutOpenLink(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	term, err := New(Options{Conn: conn})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	term.Feed([]byte("\x1b]8;;vscode://file/a.go\x1b\\edit\x1b]8;;\x1b\\ src/app.go:12"))
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	tt.Frame()
	x, y := cellCenter(term, 1, 0)
	tt.Move(x, y)
	if c := tt.Cursor(); c != ui.CursorText {
		t.Errorf("the cursor is %v over a link without Command", c)
	}
	// Command pressed and let go of with the pointer still.
	tt.HoldModifiers(ui.Cmd)
	if c := tt.Cursor(); c != ui.CursorPointer {
		t.Errorf("the cursor is %v over a link with Command", c)
	}
	tt.HoldModifiers(0)
	if c := tt.Cursor(); c != ui.CursorText {
		t.Errorf("the cursor is %v once Command is let go of", c)
	}
	term.Feed([]byte("\x1b[?1002h\x1b[?1006h"))
	tt.Frame()
	tt.ClickAtWith(ui.Cmd, x, y)
	x2, y2 := cellCenter(term, 8, 0)
	tt.ClickAtWith(ui.Cmd, x2, y2)
	if u := tt.OpenedURLs(); !slices.Equal(u, []string{"vscode://file/a.go"}) {
		t.Errorf("system opened %q", u)
	}
	// The click on the path, no link without OpenLink, went to the
	// program; the one on the link did not, nor its release.
	got := conn.take(term) // two reports, 22 bytes with Control
	if strings.Count(got, "\x1b[<") != 2 || strings.Count(got, ";9;1M") != 1 || strings.Count(got, ";9;1m") != 1 {
		t.Errorf("sent %q, want the press and the release of the path's click", got)
	}
}

// TestOnPasteTakesAPaste: Options.OnPaste sees a paste and the clipboard's
// text, and a true result pastes nothing.
func TestOnPasteTakesAPaste(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	var seen []string
	take := true
	term, err := New(Options{Conn: conn, OnPaste: func(d transfer.Data) bool {
		text, _ := d.Read(transfer.Text)
		png, _ := d.Read(transfer.PNG)
		seen = append(seen, string(text)+string(png))
		return take
	}})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	tt.SetClipboard("hello")
	tt.Frame()
	paste := ui.Super
	if runtime.GOOS != "darwin" {
		paste = ui.Ctrl | ui.Shift
	}
	tt.Key(paste, ui.KeyV)
	tt.Command("paste") // the Edit menu's Paste
	tt.Frame()
	if !slices.Equal(seen, []string{"hello", "hello"}) {
		t.Fatalf("OnPaste saw %q", seen)
	}
	if got := conn.take(term); len(got) != 0 {
		t.Errorf("a taken paste sent %q", got)
	}
	take = false
	tt.Key(paste, ui.KeyV)
	tt.Frame()
	if got := conn.take(term); got != "hello" || len(seen) != 3 {
		t.Errorf("a paste OnPaste leaves sent %q (seen %q)", got, seen)
	}
	// It sees every format, as an image.
	take = true
	tt.SetClipboardData(transfer.New(transfer.NewItem(transfer.Bytes(transfer.PNG, []byte("PNG")))))
	tt.Key(paste, ui.KeyV)
	if seen[len(seen)-1] != "PNG" {
		t.Errorf("OnPaste saw %q of an image", seen[len(seen)-1])
	}
}

// TestOnKeyTakesAKey: Options.OnKey sees a key press before the terminal
// encodes it, in order with the keys after it; a true result sends
// nothing, not even the text the key types.
func TestOnKeyTakesAKey(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	var seen []ui.Key
	var term *Terminal
	term, err := New(Options{Conn: conn, OnKey: func(mods ui.Modifiers, key ui.Key) bool {
		seen = append(seen, key)
		if mods == ui.Ctrl && key == ui.KeyV || key == ui.KeyX {
			term.Send([]byte("<taken>")) // in order with what is typed
			return true
		}
		return false
	}})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	tt.Frame()
	tt.Key(ui.Ctrl, ui.KeyV)
	tt.Key(0, ui.KeyTab)
	tt.TypeKey(0, ui.KeyX, "x")
	tt.TypeKey(0, ui.KeyY, "y")
	tt.Frame()
	if got := conn.take(term); got != "<taken>\t<taken>y" {
		t.Errorf("sent %q, want the taken keys' marks in order with Tab and y", got)
	}
	if !slices.Equal(seen, []ui.Key{ui.KeyV, ui.KeyTab, ui.KeyX, ui.KeyY}) {
		t.Errorf("OnKey saw %v", seen)
	}
}

// TestOnScrollTakesTheWheel: Options.OnScroll sees the wheel's whole rows
// before the scrollback moves, unless a program takes the wheel: as the
// mouse, or as the arrows of a full-screen program.
func TestOnScrollTakesTheWheel(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	var seen []int
	term, err := New(Options{Conn: conn, OnScroll: func(rows int) bool {
		seen = append(seen, rows)
		return true
	}})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	var b strings.Builder
	for i := range 100 {
		b.WriteString("line " + itoa(i) + "\r\n")
	}
	term.Feed([]byte(b.String()))
	tt.Frame()
	offset := func() uint64 {
		term.mu.Lock()
		defer term.mu.Unlock()
		return term.term.Scrollbar().Offset
	}
	bottom := offset()
	tt.Scroll(100, 100, 0, -40) // a notch up
	tt.Scroll(100, 100, 0, 40)  // and down
	if !slices.Equal(seen, []int{-3, 3}) {
		t.Errorf("OnScroll saw %v, want [-3 3]", seen)
	}
	if got := offset(); got != bottom {
		t.Errorf("a taken scroll moved the scrollback: offset = %d, want %d", got, bottom)
	}
	// The program takes the mouse: the wheel goes to it.
	term.Feed([]byte("\x1b[?1002h\x1b[?1006h"))
	tt.Frame()
	tt.Scroll(100, 100, 0, -40)
	if len(seen) != 2 {
		t.Errorf("OnScroll saw %v while the program took the mouse", seen)
	}
	if got := conn.take(term); !strings.HasPrefix(got, "\x1b[<64;") {
		t.Errorf("wheel sent %q, want a mouse report", got)
	}
	// A full-screen program gets it as arrows, unless it turns that off.
	term.Feed([]byte("\x1b[?1002l\x1b[?1006l\x1b[?1049h"))
	tt.Frame()
	tt.Scroll(100, 100, 0, -40)
	if len(seen) != 2 {
		t.Errorf("OnScroll saw %v on a full-screen program's screen", seen)
	}
	if got := conn.take(term); strings.Count(got, "A") != 3 {
		t.Errorf("wheel sent %q, want three arrows up", got)
	}
	term.Feed([]byte("\x1b[?1007l"))
	tt.Frame()
	tt.Scroll(100, 100, 0, -40)
	if !slices.Equal(seen, []int{-3, 3, -3}) {
		t.Errorf("OnScroll saw %v without alternate scroll", seen)
	}
}

// A program that reads the clipboard itself (mode 5522) is told what a
// paste offers and reads the image with the event's password; a
// terminal that did not paste, as another window of the program, answers
// nothing; without the mode the text is pasted, bracketed.
func TestPasteImage(t *testing.T) {
	loadLib(t)
	image := bytes.Repeat([]byte("\x89PNG\r\n\x1a\nxyz"), 1000)
	conn := newPipe()
	var mu sync.Mutex
	var statuses []string
	var term *Terminal
	var err error
	var last PasteProgress
	term, err = New(Options{Conn: conn, OnPasteStatus: func(p PasteProgress) {
		term.Title() // the terminal is not locked
		mu.Lock()
		statuses = append(statuses, fmt.Sprint(p.Status, p.Sent == p.Total && p.Total > 0))
		if p.Status == PasteSent {
			last = p
		}
		mu.Unlock()
	}})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	tt.SetClipboardData(transfer.New(transfer.NewItem(transfer.Bytes(transfer.Text, []byte("some text")), transfer.Bytes(transfer.PNG, image))))
	pasteMods := ui.Ctrl | ui.Shift
	if runtime.GOOS == "darwin" {
		pasteMods = ui.Super
	}

	term.Feed([]byte("\x1b[?5522h"))
	tt.Key(pasteMods, ui.KeyV)
	event := conn.take(term)
	_, after, ok := strings.Cut(event, ":pw=")
	if !ok || !strings.HasSuffix(event, "\x1b\\") {
		t.Fatalf("paste sent %q, want a paste event", event)
	}
	pw := after[:strings.Index(after, "\x1b")]
	listing, _ := base64.StdEncoding.DecodeString(event[strings.Index(event, "Lg==:pw="+pw+";")+len("Lg==:pw="+pw+";") : strings.Index(event, "\x1b\\\x1b]5522;type=read:status=DONE")])
	if string(listing) != "text/plain image/png\n" {
		t.Errorf("offered %q", listing)
	}

	name := base64.StdEncoding.EncodeToString([]byte("Paste event"))
	mime := base64.StdEncoding.EncodeToString([]byte("image/png"))
	read := "\x1b]5522;type=read:pw=" + pw + ":name=" + name + ";" + mime + "\x1b\\"
	term.Feed([]byte(read))
	reply := conn.take(term)
	var got []byte
	for _, pkt := range strings.Split(reply, "\x1b\\") {
		if _, data, ok := strings.Cut(pkt, "status=DATA:mime="+mime+";"); ok {
			b, err := base64.StdEncoding.DecodeString(data)
			if err != nil {
				t.Fatal(err)
			}
			got = append(got, b...)
		}
	}
	if !bytes.Equal(got, image) || !strings.HasSuffix(reply, "status=DONE\x1b\\") {
		t.Errorf("the program read %d bytes of %d; reply ends %q", len(got), len(image), reply[max(0, len(reply)-40):])
	}
	// The statuses of the pieces ran on the queue, before take's marker.
	mu.Lock()
	if last.Total != int64(len(reply)) {
		t.Errorf("sent %d bytes of %d, but the reply is %d", last.Sent, last.Total, len(reply))
	}
	mu.Unlock()

	// Another terminal of the same program (a mirror) sees the read too,
	// and must not answer it.
	conn2 := newPipe()
	other, err := New(Options{Conn: conn2})
	if err != nil {
		t.Fatal(err)
	}
	defer other.Close()
	other.Feed([]byte("\x1b[?5522h" + read))
	if sent := conn2.take(other); sent != "" {
		t.Errorf("a terminal that did not paste answered %q", sent)
	}

	// Without the mode, the text, bracketed when asked.
	term.Feed([]byte("\x1b[?5522l\x1b[?2004h"))
	tt.Key(pasteMods, ui.KeyV)
	if sent := conn.take(term); sent != "\x1b[200~some text\x1b[201~" {
		t.Errorf("without the mode, pasted %q", sent)
	}
	// Only an image, to a program that takes bracketed pastes: an empty
	// one, which it reads as a paste of the image, as Claude Code does.
	tt.SetClipboardData(transfer.New(transfer.NewItem(transfer.Bytes(transfer.PNG, image))))
	tt.Key(pasteMods, ui.KeyV)
	if sent := conn.take(term); sent != "\x1b[200~\x1b[201~" {
		t.Errorf("with only an image and bracketed paste, pasted %q", sent)
	}
	// Only an image, and no program to read it: nothing, and told so.
	term.Feed([]byte("\x1b[?2004l"))
	tt.Key(pasteMods, ui.KeyV)
	if sent := conn.take(term); sent != "" {
		t.Errorf("with only an image and no mode, pasted %q", sent)
	}

	mu.Lock()
	seen := strings.Join(statuses, " ")
	mu.Unlock()
	// Offered, sending from nothing, then pieces, the last of them all sent;
	// then the paste of no text.
	if !strings.HasPrefix(seen, fmt.Sprint(PasteOffered, false)+" "+fmt.Sprint(PasteSending, false)) ||
		!strings.Contains(seen, fmt.Sprint(PasteSent, true)) || !strings.HasSuffix(seen, fmt.Sprint(PasteNoText, false)) {
		t.Errorf("statuses %q", seen)
	}
}

// TestOnZoom: Options.OnZoom takes the wheel with Control (Command on
// macOS) in notches, rolling up positive, before OnScroll and a program
// that takes the mouse, and the font size keys.
func TestOnZoom(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	var zooms []Zoom
	var scrolls []int
	term, err := New(Options{
		Conn:     conn,
		OnZoom:   func(z Zoom) bool { zooms = append(zooms, z); return true },
		OnScroll: func(rows int) bool { scrolls = append(scrolls, rows); return true },
	})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	tt.Frame()
	zoomMod := ui.Cmd
	wheel := func(mods ui.Modifiers, dy float32, precise bool) {
		term.v.scroll(ui.InputEvent{Kind: ui.InputScroll, DY: dy, Mods: mods, Precise: precise})
	}
	steps := func() []int {
		var s []int
		for _, z := range zooms {
			s = append(s, z.Steps)
		}
		return s
	}
	wheel(zoomMod, -40, false) // a notch up
	wheel(zoomMod, 40, false)  // and down
	wheel(zoomMod, -120, false)
	if !slices.Equal(steps(), []int{1, -1, 1}) {
		t.Errorf("notches zoomed %v, want [1 -1 1]: one step an event", steps())
	}
	// A trackpad's moves add up to notches of 40.
	zooms = nil
	for range 6 {
		wheel(zoomMod, -15, true)
	}
	if !slices.Equal(steps(), []int{1, 1}) {
		t.Errorf("a trackpad zoomed %v, want [1 1]", steps())
	}
	if scrolls != nil {
		t.Errorf("OnScroll saw %v while zooming", scrolls)
	}
	// Without the modifier the wheel scrolls.
	zooms = nil
	wheel(0, -40, false)
	if zooms != nil || !slices.Equal(scrolls, []int{-3}) {
		t.Errorf("a plain notch zoomed %v and scrolled %v", zooms, scrolls)
	}
	// A program that takes the mouse gets no report of a zoom.
	term.Feed([]byte("\x1b[?1002h\x1b[?1006h"))
	tt.Frame()
	wheel(zoomMod, -40, false)
	if !slices.Equal(steps(), []int{1}) {
		t.Errorf("zoomed %v while the program took the mouse", steps())
	}
	if got := conn.take(term); got != "" {
		t.Errorf("a zoom sent %q to the program", got)
	}
	// The font size keys.
	zooms = nil
	tt.Key(ui.Cmd, ui.KeyEqual)
	tt.Key(ui.Cmd, ui.KeyMinus)
	tt.Key(ui.Cmd, ui.Key0)
	if !slices.Equal(zooms, []Zoom{{Steps: 1}, {Steps: -1}, {Reset: true}}) {
		t.Errorf("the keys zoomed %v", zooms)
	}
	if got := conn.take(term); got != "" {
		t.Errorf("the zoom keys sent %q to the program", got)
	}
	if term.v.font.size != 13 {
		t.Errorf("a zoom OnZoom took changed the font to %v", term.v.font.size)
	}
}

// TestZoom: without OnZoom the terminal zooms its font a DIP a step, and
// back to its font's size, which SetFont changes.
func TestZoom(t *testing.T) {
	loadLib(t)
	term, err := New(Options{Conn: newPipe(), Font: Font{Size: 14}})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	tt.Frame()
	cols, _ := term.Size()
	size := func() float32 { tt.Frame(); return term.v.font.size }
	term.v.scroll(ui.InputEvent{Kind: ui.InputScroll, DY: -40, Mods: ui.Cmd})
	tt.Key(ui.Cmd, ui.KeyEqual)
	if got := size(); got != 16 {
		t.Errorf("zoomed in twice to %v, want 16", got)
	}
	if c, _ := term.Size(); c >= cols {
		t.Errorf("%d columns at a larger size, %d before", c, cols)
	}
	tt.Key(ui.Cmd, ui.KeyMinus)
	if got := size(); got != 15 {
		t.Errorf("zoomed out to %v, want 15", got)
	}
	tt.Key(ui.Cmd, ui.Key0)
	if got := size(); got != 14 {
		t.Errorf("reset to %v, want 14", got)
	}
	tt.Key(ui.Cmd, ui.KeyEqual)
	term.SetFont(Font{Size: 20})
	if got := size(); got != 20 {
		t.Errorf("SetFont after a zoom gave %v, want 20", got)
	}
	for range 30 {
		tt.Key(ui.Cmd, ui.KeyMinus)
	}
	if got := size(); got != 1 {
		t.Errorf("zoomed out to %v, want 1 at the least", got)
	}
}

// TestPasteData: a program that reads the clipboard is offered every
// format, a file list as text/uri-list, and reads a lazy one only when it
// asks; a paste it does not read is told after a moment, and still read.
func TestPasteData(t *testing.T) {
	loadLib(t)
	notTakenAfter = 50 * time.Millisecond
	defer func() { notTakenAfter = 5 * time.Second }()
	conn := newPipe()
	var mu sync.Mutex
	var statuses []PasteStatus
	notTaken := make(chan struct{})
	term, err := New(Options{Conn: conn, OnPasteStatus: func(p PasteProgress) {
		mu.Lock()
		statuses = append(statuses, p.Status)
		mu.Unlock()
		if p.Status == PasteNotTaken {
			close(notTaken)
		}
	}})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	read := 0
	path := filepath.Join(t.TempDir(), "a.png")
	files, err := transfer.FileData(path)
	if err != nil {
		t.Fatal(err)
	}
	// The file URL as FileData wrote it: Windows paths have a drive letter.
	uri, err := files.Items()[0].Read(transfer.FileList)
	if err != nil {
		t.Fatal(err)
	}
	d := transfer.New(append(files.Items(), transfer.NewItem(
		transfer.Lazy("image/jpeg", func() ([]byte, error) { read++; return []byte("JPEG"), nil }),
		transfer.Bytes(transfer.Text, []byte("a.png")),
	))...)
	term.Feed([]byte("\x1b[?5522h"))
	term.PasteData(d)
	event := conn.take(term)
	_, after, ok := strings.Cut(event, ":pw=")
	if !ok {
		t.Fatalf("paste sent %q, want a paste event", event)
	}
	pw := after[:strings.Index(after, "\x1b")]
	start := strings.Index(event, "Lg==:pw="+pw+";") + len("Lg==:pw="+pw+";")
	listing, _ := base64.StdEncoding.DecodeString(event[start : strings.Index(event[start:], "\x1b")+start])
	if string(listing) != "text/uri-list image/jpeg text/plain\n" {
		t.Errorf("offered %q", listing)
	}
	select {
	case <-notTaken:
	case <-time.After(time.Minute):
		t.Fatal("no PasteNotTaken")
	}
	if read != 0 {
		t.Error("the lazy format was read before the program asked")
	}
	mu.Lock()
	if !slices.Equal(statuses, []PasteStatus{PasteOffered, PasteNotTaken}) {
		t.Errorf("statuses %v before the read", statuses)
	}
	mu.Unlock()
	b64 := base64.StdEncoding.EncodeToString
	term.Feed([]byte("\x1b]5522;type=read:pw=" + pw + ":name=" + b64([]byte("Paste event")) + ";" + b64([]byte("image/jpeg text/uri-list")) + "\x1b\\"))
	reply := conn.take(term)
	if read != 1 || !strings.Contains(reply, b64([]byte("JPEG"))) || !strings.Contains(reply, b64(uri)) {
		t.Errorf("read %d times, replied %q", read, reply)
	}
}

// TestNoClipboardWrite: programs write to the clipboard (OSC 52) unless
// NoClipboardWrite.
func TestNoClipboardWrite(t *testing.T) {
	loadLib(t)
	for _, deny := range []bool{false, true} {
		term, err := New(Options{Conn: newPipe(), NoClipboardWrite: deny})
		if err != nil {
			t.Fatal(err)
		}
		tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
		term.Feed([]byte("\x1b]52;c;" + base64.StdEncoding.EncodeToString([]byte("copied")) + "\x1b\\"))
		tt.Frame()
		want := "copied"
		if deny {
			want = ""
		}
		if got := tt.Clipboard(); got != want {
			t.Errorf("NoClipboardWrite %v: a program copied %q", deny, got)
		}
		term.Close()
	}
}

// TestClipboardReadAllow: with ClipboardRead ClipboardAllow the terminal
// answers a program's read of the clipboard (OSC 52) with it, from the
// view, on the main thread.
func TestClipboardReadAllow(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	term, err := New(Options{Conn: conn, ClipboardRead: ClipboardAllow})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	tt.SetClipboard("hello")
	term.Feed([]byte("\x1b]52;c;?\x1b\\"))
	tt.Frame()
	want := "\x1b]52;c;" + base64.StdEncoding.EncodeToString([]byte("hello")) + "\x1b\\"
	if got := conn.take(term); got != want {
		t.Errorf("the program read %q, want %q", got, want)
	}
}

// TestClipboardReadDeny: reads are refused by default and with
// ClipboardDeny: an empty OSC 52 reply, without reading the clipboard.
func TestClipboardReadDeny(t *testing.T) {
	loadLib(t)
	for _, opts := range []Options{{}, {ClipboardRead: ClipboardDeny}} {
		conn := newPipe()
		opts.Conn = conn
		term, err := New(opts)
		if err != nil {
			t.Fatal(err)
		}
		tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
		tt.SetClipboard("secret")
		term.Feed([]byte("\x1b]52;c;?\x1b\\"))
		tt.Frame()
		if got := conn.take(term); got != "\x1b]52;c;\x1b\\" {
			t.Errorf("ClipboardRead %v: the program read %q", opts.ClipboardRead, got)
		}
		if got := tt.Clipboard(); got != "secret" {
			t.Errorf("the read changed the clipboard to %q", got)
		}
		term.Close()
	}
}

// TestClipboardReadAsk: with ClipboardRead ClipboardAsk,
// Options.OnClipboardRequest sees the read, with what it asks for, and
// Allow or Deny answers it, in the hook or later, from any goroutine.
func TestClipboardReadAsk(t *testing.T) {
	loadLib(t)
	for _, answer := range []int8{1, 2, 0, 3} {
		conn := newPipe()
		var requests []ClipboardRequest
		var answerNow func()
		term, err := New(Options{
			Conn:          conn,
			ClipboardRead: ClipboardAsk,
			OnClipboardRequest: func(req ClipboardRequest) {
				requests = append(requests, req)
				switch answer {
				case 1:
					req.Allow()
				case 2:
					req.Deny()
				case 0:
					answerNow = req.Allow // later, from the test's goroutine
				case 3:
					answerNow = req.Deny
				}
			},
		})
		if err != nil {
			t.Fatal(err)
		}
		tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
		tt.SetClipboard("hello")
		term.Feed([]byte("\x1b]52;c;?\x1b\\"))
		tt.Frame()
		if len(requests) != 1 {
			t.Fatalf("the hook saw %d requests", len(requests))
		}
		if req := requests[0]; !slices.Equal(req.Mimes, []string{"text/plain"}) || req.List || req.Text != "hello" {
			t.Errorf("the hook saw %+v", req)
		}
		if answer == 0 || answer == 3 {
			answerNow()
			tt.Frame()
		}
		want := "\x1b]52;c;\x1b\\"
		if answer == 1 || answer == 0 {
			want = "\x1b]52;c;aGVsbG8=\x1b\\"
		}
		if got := conn.take(term); got != want {
			t.Errorf("answer %d: the program read %q, want %q", answer, got, want)
		}
		term.Close()
	}
}

// TestClipboardReadOrder: reads are answered in the order they came, since
// an OSC 52 reply doesn't tell which read it answers: one the app is asked
// about holds back those after it.
func TestClipboardReadOrder(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	var requests []ClipboardRequest
	term, err := New(Options{
		Conn:               conn,
		ClipboardRead:      ClipboardAsk,
		OnClipboardRequest: func(req ClipboardRequest) { requests = append(requests, req) },
	})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	tt.SetClipboard("hello")
	term.Feed([]byte("\x1b]52;c;?\x1b\\\x1b]52;p;?\x1b\\"))
	tt.Frame()
	if len(requests) != 1 || requests[0].Primary {
		t.Fatalf("asked %+v before the first read was answered", requests)
	}
	requests[0].Deny()
	tt.Frame()
	if got := conn.take(term); got != "\x1b]52;c;\x1b\\" {
		t.Errorf("the first read was answered %q", got)
	}
	if len(requests) != 2 || !requests[1].Primary {
		t.Fatalf("the second read was not asked about: %+v", requests)
	}
	requests[1].Allow()
	tt.Frame()
	want := "\x1b]52;p;aGVsbG8=\x1b\\"
	if got := conn.take(term); got != want {
		t.Errorf("the second read was answered %q, want %q", got, want)
	}
}

// TestClipboardReadPrompt: without OnClipboardRequest the terminal asks
// with its own prompt, whose default button (Enter) allows and whose
// Escape denies, keeping the read pending across frames until then.
func TestClipboardReadPrompt(t *testing.T) {
	loadLib(t)
	conn := newPipe()
	term, err := New(Options{Conn: conn, ClipboardRead: ClipboardAsk})
	if err != nil {
		t.Fatal(err)
	}
	defer term.Close()
	tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
	tt.SetClipboard("hello")
	term.Feed([]byte("\x1b]52;c;?\x1b\\"))
	tt.Frame() // the prompt opens
	if got := conn.take(term); got != "" {
		t.Fatalf("the prompt answered %q on its own", got)
	}
	if !tt.HasText("hello") {
		t.Error("the prompt does not show the clipboard's text")
	}
	tt.Key(0, ui.KeyEnter)
	want := "\x1b]52;c;aGVsbG8=\x1b\\"
	if got := conn.take(term); got != want {
		t.Errorf("Enter allowed the read: %q, want %q", got, want)
	}
	// Escape dismisses the alert, which denies the read.
	term.Feed([]byte("\x1b]52;c;?\x1b\\"))
	tt.Frame()
	tt.Key(0, ui.KeyEscape)
	if got := conn.take(term); got != "\x1b]52;c;\x1b\\" {
		t.Errorf("Escape answered %q, want an empty clipboard", got)
	}
}

// TestClipboardReadKitty: the Kitty clipboard protocol's reads (OSC 5522)
// follow the same policy: allowed reads are answered with the text, and
// refused ones with EPERM.
func TestClipboardReadKitty(t *testing.T) {
	loadLib(t)
	read := "\x1b]5522;type=read;" + base64.StdEncoding.EncodeToString([]byte("text/plain")) + "\x1b\\"
	for _, policy := range []ClipboardAccess{ClipboardAllow, ClipboardDeny} {
		conn := newPipe()
		term, err := New(Options{Conn: conn, ClipboardRead: policy})
		if err != nil {
			t.Fatal(err)
		}
		tt := ui.NewTester(func(c *ui.Context) { View(c, term).Fill().AutoFocus() }, 400, 200)
		tt.SetClipboard("hello")
		term.Feed([]byte("\x1b[?5522h"))
		term.Feed([]byte(read))
		tt.Frame()
		want := "\x1b]5522;type=read:status=EPERM\x1b\\"
		if policy == ClipboardAllow {
			mime := base64.StdEncoding.EncodeToString([]byte("text/plain"))
			want = "\x1b]5522;type=read:status=OK\x1b\\" +
				"\x1b]5522;type=read:status=DATA:mime=" + mime + ";aGVsbG8=\x1b\\" +
				"\x1b]5522;type=read:status=DONE\x1b\\"
		}
		if got := conn.take(term); got != want {
			t.Errorf("ClipboardRead %v: the program read %q, want %q", policy, got, want)
		}
		term.Close()
	}
}
