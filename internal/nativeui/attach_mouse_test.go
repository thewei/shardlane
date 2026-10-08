package nativeui

import (
	"strings"
	"testing"
)

// TestSGRMouseSplitterPinsTheWriteSideRouter pins the F144 real-terminal
// write path: typed bytes pass byte-exact, every complete SGR mouse
// sequence is extracted verbatim for routeAttachMouse, mixed input keeps
// its order, and nothing is ever held across writes (a bare ESC keypress
// must reach the daemon immediately).
func TestSGRMouseSplitter(t *testing.T) {
	var s sgrMouseSplitter

	plain, mouse := s.split([]byte("ls -la\r"))
	if string(plain) != "ls -la\r" || len(mouse) != 0 {
		t.Fatalf("plain keys: plain=%q mouse=%v", plain, mouse)
	}

	plain, mouse = s.split([]byte("\x1b[<64;10;5Mecho hi\r"))
	if string(plain) != "echo hi\r" {
		t.Fatalf("wheel + keys: plain=%q, want \"echo hi\\r\"", plain)
	}
	if len(mouse) != 1 || string(mouse[0]) != "\x1b[<64;10;5M" {
		t.Fatalf("wheel event = %v, want one verbatim SGR sequence", mouse)
	}

	// Interleaved and repeated events keep their order.
	plain, mouse = s.split([]byte("a\x1b[<0;1;1Mb\x1b[<32;2;2Mc\x1b[<0;3;3m"))
	if string(plain) != "abc" {
		t.Fatalf("interleaved plain = %q, want \"abc\"", plain)
	}
	if len(mouse) != 3 || string(mouse[1]) != "\x1b[<32;2;2M" {
		t.Fatalf("interleaved mouse = %v", mouse)
	}

	// Non-mouse escapes and a bare ESC pass through untouched.
	plain, _ = s.split([]byte("\x1b[?1049l\x1bOAI\x1b"))
	if string(plain) != "\x1b[?1049l\x1bOAI\x1b" {
		t.Fatalf("non-mouse escapes = %q, want byte-exact passthrough", plain)
	}

	// A mouse sequence cut short at a write edge degrades to plain bytes
	// instead of stalling the stream.
	plain, mouse = s.split([]byte("x\x1b[<64"))
	if string(plain) != "x\x1b[<64" || len(mouse) != 0 {
		t.Fatalf("truncated sequence: plain=%q mouse=%v, want plain passthrough", plain, mouse)
	}
}

// TestParseSGRMouse pins the decoder the router relies on.
func TestParseSGRMouse(t *testing.T) {
	cases := []struct {
		seq   string
		want  int
		wheel bool
		ok    bool
	}{
		{"\x1b[<64;10;5M", 64, true, true},
		{"\x1b[<65;1;1M", 65, true, true},
		{"\x1b[<0;10;5M", 0, false, true},
		{"\x1b[<32;11;5M", 32, false, true},
		{"\x1b[<0;10;5m", 0, false, true}, // release
		{"\x1b[<0;10;5x", 0, false, false},
		{"\x1b[<64", 0, false, false},
		{"hello", 0, false, false},
	}
	for _, tc := range cases {
		button, wheel, ok := parseSGRMouse(tc.seq)
		if ok != tc.ok || (ok && (button != tc.want || wheel != tc.wheel)) {
			t.Fatalf("parseSGRMouse(%q) = (%d,%v,%v), want (%d,%v,%v)",
				strings.ReplaceAll(tc.seq, "\x1b", "ESC"), button, wheel, ok, tc.want, tc.wheel, tc.ok)
		}
	}
}
