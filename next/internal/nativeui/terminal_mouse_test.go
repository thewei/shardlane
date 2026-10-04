package nativeui

import (
	"os"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

// scriptPipe is a terminal.Conn playing the daemon side of an attach:
// Read returns scripted stream bytes (what `herdr terminal attach`
// renders) byte-exact — tracking sets included, per the F144 real-terminal
// round — then blocks until closed; Write captures what the view sends to
// the daemon (typed keys and reported SGR mouse events alike).
type scriptPipe struct {
	mu     sync.Mutex
	script [][]byte
	sent   []byte
	closed chan struct{}
}

func newScriptPipe(chunks ...string) *scriptPipe {
	p := &scriptPipe{closed: make(chan struct{})}
	for _, chunk := range chunks {
		p.script = append(p.script, []byte(chunk))
	}
	return p
}

func (p *scriptPipe) Read(buf []byte) (int, error) {
	for {
		p.mu.Lock()
		if len(p.script) > 0 {
			chunk := p.script[0]
			p.script = p.script[1:]
			n := copy(buf, chunk)
			p.mu.Unlock()
			if n > 0 {
				return n, nil
			}
			continue
		}
		p.mu.Unlock()
		<-p.closed
		return 0, os.ErrClosed
	}
}

func (p *scriptPipe) Write(b []byte) (int, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.sent = append(p.sent, b...)
	return len(b), nil
}

func (p *scriptPipe) take(want int) string {
	deadline := time.Now().Add(time.Second)
	for {
		p.mu.Lock()
		out := string(p.sent)
		p.mu.Unlock()
		if len(out) >= want || time.Now().After(deadline) {
			return out
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func (p *scriptPipe) Close() error {
	select {
	case <-p.closed:
	default:
		close(p.closed)
	}
	return nil
}

func (p *scriptPipe) drainSent() {
	p.mu.Lock()
	p.sent = nil
	p.mu.Unlock()
}

// trackedPipeFeed plays the daemon's startup render: its forced
// mouse-tracking enables plus content. With the F144 real-terminal round
// those sets reach the emulator byte-exact.
const trackedPipeFeed = "\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h\x1b[2J\x1b[1;1Hhello"

// TestTrackingPassesThroughAndPointerEventsAreReported pins the 2026-10-06
// F144 real-terminal contract (user-approved): the daemon's forced
// mouse-tracking enables reach the emulator untouched, so the view reports
// pointer events — a plain wheel over the pane leaves as SGR mouse bytes
// into the conn (routeAttachMouse turns them into pane.scroll / send_text
// in production) — and a right click is a program event, not the app's
// pane menu.
func TestTrackingPassesThroughAndPointerEventsAreReported(t *testing.T) {
	conn := newScriptPipe(trackedPipeFeed)
	term, err := terminal.New(terminal.Options{Conn: conn})
	if err != nil {
		t.Skip("terminal library unavailable:", err)
	}
	defer term.Close()

	shell := NewShell()
	shell.terminals["term-1"] = &terminalSurface{
		key: "term-1", paneID: "p1", term: term, label: "shell",
		area: herdr.LayoutRect{Width: 1, Height: 1}, rect: herdr.LayoutRect{Width: 1, Height: 1}, focused: true,
	}
	tt := ui.NewTester(func(c *ui.Context) { shell.terminalCanvas(c) }, 1200, 800)
	// Let the reader goroutine consume the scripted stream.
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		tt.Frame()
		time.Sleep(10 * time.Millisecond)
	}

	conn.drainSent()
	tt.Scroll(600, 400, 0, -240)
	tt.Frame()
	sent := conn.take(1)
	if !strings.Contains(sent, "\x1b[<64;") && !strings.Contains(sent, "\x1b[<65;") {
		t.Fatalf("wheel was not reported into the conn as SGR mouse bytes: %q", sent)
	}

	conn.drainSent()
	tt.RightClickAt(600, 400)
	tt.Frame()
	sent = conn.take(1)
	if sent == "" {
		t.Fatalf("right click was not reported into the conn")
	}
	if menu := tt.Menu(); len(menu) != 0 {
		t.Fatalf("right click opened an app menu over the terminal: %v", menu)
	}
}

// TestRouteAttachMouseMovesHerdrViewport pins the ordinary-pane half of
// the router: one reported wheel event moves Herdr's authoritative scroll
// viewport by exactly one row, clamped to [0, scrollMax], and idles at the
// edges; non-wheel events fabricate nothing.
func TestRouteAttachMouseMovesHerdrViewport(t *testing.T) {
	shell := NewShell()
	shell.activeInstance = "inst"
	surface := &terminalSurface{key: "t1", paneID: "w1:p1", scrollOffset: 3, scrollMax: 4}
	type scrollCall struct {
		paneID string
		offset int
	}
	var calls []scrollCall
	shell.paneScrollSink = func(paneID string, offset int) {
		calls = append(calls, scrollCall{paneID, offset})
	}

	shell.routeAttachMouse(surface, "\x1b[<64;10;5M") // wheel up → 4
	shell.routeAttachMouse(surface, "\x1b[<64;10;5M") // wheel up → clamped 4 (no call)
	shell.routeAttachMouse(surface, "\x1b[<65;10;5M") // wheel down → 3
	if len(calls) != 2 {
		t.Fatalf("pane.scroll calls = %#v, want two", calls)
	}
	if calls[0].paneID != "w1:p1" || calls[0].offset != 4 {
		t.Fatalf("wheel up: %v, want w1:p1@4", calls[0])
	}
	if calls[1].offset != 3 {
		t.Fatalf("wheel down: %v, want offset 3", calls[1])
	}
	if surface.scrollOffset != 3 {
		t.Fatalf("mirror offset = %d, want 3", surface.scrollOffset)
	}

	// Press/drag over an ordinary pane fabricates no viewport mutation.
	shell.routeAttachMouse(surface, "\x1b[<0;10;5M")
	shell.routeAttachMouse(surface, "\x1b[<32;11;5M")
	if len(calls) != 2 {
		t.Fatalf("press/drag must not touch the viewport: %v", calls)
	}
}

// TestRouteAttachMouseForwardsAgentPaneEvents pins the Agent TUI half:
// every reported mouse event (wheel, press, drag) goes to the program
// verbatim via pane.send_text — the TUI tracks the mouse and the attach
// channel drops such bytes — while Herdr's viewport path stays untouched.
func TestRouteAttachMouseForwardsAgentPaneEvents(t *testing.T) {
	shell := NewShell()
	shell.activeInstance = "inst"
	surface := &terminalSurface{key: "t1", paneID: "w1:p1", agent: "pi"}
	var sent []string
	shell.agentMouseSink = func(paneID, text string) {
		if paneID != "w1:p1" {
			t.Fatalf("send pane = %q, want w1:p1", paneID)
		}
		sent = append(sent, text)
	}
	var scrolls []int
	shell.paneScrollSink = func(paneID string, offset int) {
		scrolls = append(scrolls, offset)
	}

	for _, seq := range []string{"\x1b[<64;10;5M", "\x1b[<0;10;5M", "\x1b[<32;11;5M"} {
		shell.routeAttachMouse(surface, seq)
	}
	if len(sent) != 3 || sent[0] != "\x1b[<64;10;5M" || sent[2] != "\x1b[<32;11;5M" {
		t.Fatalf("agent pane mouse forwards = %q, want three verbatim events", sent)
	}
	if len(scrolls) != 0 {
		t.Fatalf("agent pane must not move Herdr's viewport: %v", scrolls)
	}
}

// TestRouteAttachMouseScrollbackTUIWinsTheViewport pins the 2026-10-06 agy
// fix: the wheel follows Herdr's scroll reality, not the agent label. agy
// is agent-classified but scrollback-backed (live probe: max_offset_from_
// bottom 82–221; the Herdr TUI only scrolls its viewport, never forwarding
// wheel), so its wheel must move the authoritative viewport — sending SGR
// into the program scrolls nothing because agy does not track the mouse.
func TestRouteAttachMouseScrollbackTUIWinsTheViewport(t *testing.T) {
	shell := NewShell()
	shell.activeInstance = "inst"
	surface := &terminalSurface{key: "t1", paneID: "w44:pA", agent: "agy", scrollMax: 82}
	var sent []string
	shell.agentMouseSink = func(paneID, text string) { sent = append(sent, text) }
	var calls []int
	shell.paneScrollSink = func(paneID string, offset int) { calls = append(calls, offset) }

	shell.routeAttachMouse(surface, "\x1b[<64;10;5M") // wheel up → 1
	shell.routeAttachMouse(surface, "\x1b[<64;10;5M") // → 2
	if len(calls) != 2 || calls[0] != 1 || calls[1] != 2 {
		t.Fatalf("scrollback-backed agent pane wheel = %v, want viewport steps 1,2", calls)
	}
	if len(sent) != 0 {
		t.Fatalf("scrollback-backed agent pane must not receive SGR wheel: %v", sent)
	}
	shell.routeAttachMouse(surface, "\x1b[<65;10;5M") // wheel down → back toward live
	if len(calls) != 3 || calls[2] != 1 {
		t.Fatalf("wheel down = %v, want one step back to 1", calls)
	}
}

// TestLocalDragSelectKeepsDragsAndMenuLocal pins the F146/F147 contract for
// scrollback-backed panes (shells, agy): with Options.LocalDragSelect the
// plain drag runs the view's local selection gesture — zero SGR mouse
// bytes leave through the conn — and the right click opens the terminal's
// own copy menu instead of being reported into the daemon's dead end.
// Without the option (alt-screen agent TUIs) the same gestures report.
func TestLocalDragSelectKeepsDragsAndMenuLocal(t *testing.T) {
	conn := newScriptPipe(trackedPipeFeed)
	term, err := terminal.New(terminal.Options{Conn: conn, LocalDragSelect: true})
	if err != nil {
		t.Skip("terminal library unavailable:", err)
	}
	defer term.Close()

	shell := NewShell()
	shell.terminals["term-1"] = &terminalSurface{
		key: "term-1", paneID: "p1", term: term, label: "shell",
		area: herdr.LayoutRect{Width: 1, Height: 1}, rect: herdr.LayoutRect{Width: 1, Height: 1}, focused: true,
	}
	tt := ui.NewTester(func(c *ui.Context) { shell.terminalCanvas(c) }, 1200, 800)
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		tt.Frame()
		time.Sleep(10 * time.Millisecond)
	}

	conn.drainSent()
	// A plain three-step drag: selection is local, nothing is reported.
	tt.Press(500, 300)
	tt.Move(520, 300)
	tt.Move(540, 300)
	tt.Release(540, 300)
	tt.Frame()
	sent := conn.take(1)
	if sent != "" {
		t.Fatalf("plain drag was reported despite LocalDragSelect: %q", sent)
	}

	// The right click opens the terminal's own menu (Copy/Paste/Select All)
	// — the context menu the annotation asked for.
	tt.RightClickAt(600, 400)
	tt.Frame()
	menu := tt.Menu()
	joined := strings.Join(menu, ",")
	for _, want := range []string{"Copy", "Paste"} {
		if !strings.Contains(joined, want) {
			t.Fatalf("right click menu missing %q: %v", want, menu)
		}
	}
	if sent := conn.take(1); sent != "" {
		t.Fatalf("right click was reported despite LocalDragSelect: %q", sent)
	}
	tt.CloseMenu()

	// The wheel still reports (routeAttachMouse owns its meaning).
	conn.drainSent()
	tt.Scroll(600, 400, 0, -240)
	tt.Frame()
	sent = conn.take(1)
	if !strings.Contains(sent, "\x1b[<64;") && !strings.Contains(sent, "\x1b[<65;") {
		t.Fatalf("wheel stopped reporting under LocalDragSelect: %q", sent)
	}
}
