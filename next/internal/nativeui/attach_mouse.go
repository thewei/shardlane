package nativeui

import (
	"strconv"
	"strings"
)

/**
 * [INPUT]: 依赖 terminalSurface 的 paneID/agent/scrollOffset/scrollMax、terminal_scroll 的 pane.scroll/send_text 派发通道
 * [OUTPUT]: 对外提供 sgrMouseSplitter（Write 侧 SGR 鼠标事件流式分离）、parseSGRMouse、Shell.routeAttachMouse（真终端语义路由）
 * [POS]: nativeui 的 attach 鼠标语义收口（F144 真终端改造）：客户端像真终端一样把指针事件交给程序/herdr——普通 pane 滚轮走 pane.scroll 权重视口，Agent TUI 的事件原样 pane.send_text
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// sgrMouseSplitter splits an outgoing attach byte stream (what the MyGo
// terminal view writes: typed keys plus, with the daemon's forced mouse
// tracking, SGR pointer events) into plain bytes and complete SGR mouse
// sequences. Plain bytes go to the daemon's PTY stdin untouched; every
// mouse sequence goes to the router (routeAttachMouse) instead, because
// the daemon's attach input drops mouse bytes (probed 2026-10-06) and the
// client owes them their real-terminal meaning itself.
//
// The view encodes one event per write, so sequences are atomic in
// practice; a sequence cut across writes degrades to plain bytes (the
// daemon drops them, exactly today's behavior) rather than stalling typed
// input — a bare ESC keypress must never wait in a pending buffer.
type sgrMouseSplitter struct {
	pending []byte
}

const sgrMouseMaxLen = 32

// split consumes one more write; results are valid until the next call.
func (s *sgrMouseSplitter) split(chunk []byte) (plain []byte, mouse [][]byte) {
	s.pending = s.pending[:0]
	buf := s.pending
	flush := func() { plain = append(plain, buf...); buf = buf[:0] }
	for i := 0; i < len(chunk); {
		b := chunk[i]
		switch {
		case len(buf) == 0 && b == 0x1b:
			buf = append(buf, b)
		case len(buf) == 0:
			plain = append(plain, b)
		case len(buf) >= 3 && buf[1] == '[' && buf[2] == '<':
			buf = append(buf, b)
			if b == 'M' || b == 'm' {
				seq := make([]byte, len(buf))
				copy(seq, buf)
				mouse = append(mouse, seq)
				buf = buf[:0]
			} else if len(buf) > sgrMouseMaxLen || !(b == ';' || (b >= '0' && b <= '9')) {
				flush()
			}
		default:
			buf = append(buf, b)
			if (len(buf) == 2 && b != '[') || (len(buf) == 3 && b != '<') {
				flush()
			}
		}
		i++
	}
	// Never hold typed input across writes: an unfinished escape at the
	// chunk edge goes out as plain bytes.
	plain = append(plain, buf...)
	s.pending = buf[:0]
	return plain, mouse
}

// parseSGRMouse decodes one SGR mouse sequence ("\x1b[<b;x;yM" or …m)
// into its button code. wheel is true for 64/65 (wheel up/down).
func parseSGRMouse(seq string) (button int, wheel bool, ok bool) {
	rest, ok := strings.CutPrefix(seq, "\x1b[<")
	if !ok || len(rest) < 2 {
		return 0, false, false
	}
	final := rest[len(rest)-1]
	if final != 'M' && final != 'm' {
		return 0, false, false
	}
	params := strings.Split(rest[:len(rest)-1], ";")
	if len(params) != 3 {
		return 0, false, false
	}
	button, err := strconv.Atoi(params[0])
	if err != nil {
		return 0, false, false
	}
	return button, button >= 64, true
}

// routeAttachMouse gives one SGR mouse event reported by the terminal view
// its real-terminal meaning (2026-10-06 F144, user-approved): the client
// behaves like any terminal hosting Herdr. The wheel follows Herdr's own
// scroll reality, not the agent label: a pane whose projection reports
// scrollback range (shells, and scrollback-backed TUIs like agy — probed
// live: agy carries max_offset_from_bottom 82–221 and the Herdr TUI only
// ever scrolls its viewport, never forwarding wheel) moves the
// authoritative viewport one row per event (pane.scroll); an Agent TUI
// with no scrollback range is a true alt-screen program (pi) and gets
// every mouse event verbatim via pane.send_text — the attach channel
// drops such bytes, probed 2026-10-06. Presses and drags follow the same
// split: only alt-screen Agent TUIs consume them client-side, and the
// client does not fabricate pane semantics for ordinary panes. Runs on
// the window's UI lane.
func (s *Shell) routeAttachMouse(surface *terminalSurface, seq string) {
	if surface == nil || surface.paneID == "" || s.activeInstance == "" {
		return
	}
	button, wheel, ok := parseSGRMouse(seq)
	if !ok {
		return
	}
	altScreenAgent := surface.agent != "" && surface.scrollMax == 0
	if !wheel {
		if altScreenAgent {
			s.queueAgentMouse(surface.paneID, seq)
		}
		return
	}
	if altScreenAgent {
		s.queueAgentMouse(surface.paneID, seq)
		return
	}
	offset := int(surface.scrollOffset)
	if button == 64 { // wheel up: further back, offset_from_bottom grows
		offset++
	} else {
		offset--
	}
	if max := int(surface.scrollMax); offset > max {
		offset = max
	}
	if offset < 0 {
		offset = 0
	}
	if uint64(offset) == surface.scrollOffset {
		return
	}
	surface.scrollOffset = uint64(offset)
	s.dispatchPaneScroll(surface.paneID, offset)
}
