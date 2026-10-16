package terminal

import "testing"

// feedAll feeds the scanner one whole sequence and reports what it saw.
func feedAll(t *testing.T, chunks ...string) ([][]byte, bool) {
	t.Helper()
	var s osc7501Scanner
	var bodies [][]byte
	var query bool
	for _, c := range chunks {
		b, q := s.feed([]byte(c))
		bodies = append(bodies, b...)
		query = query || q
	}
	return bodies, query
}

func TestOSC7501Framing(t *testing.T) {
	report := "\x1b]7501;state=working:progress=40\x1b\\"
	bodies, query := feedAll(t, report)
	if query || len(bodies) != 1 || string(bodies[0]) != "state=working:progress=40" {
		t.Fatalf("ST form: bodies=%q query=%v", bodies, query)
	}

	bodies, query = feedAll(t, "\x1b]7501;state=done\x07")
	if query || len(bodies) != 1 || string(bodies[0]) != "state=done" {
		t.Fatalf("BEL form: bodies=%q query=%v", bodies, query)
	}

	// Split at every byte offset: the state machine must reassemble.
	seq := []byte(report)
	for split := 1; split < len(seq)-1; split++ {
		bodies, query = feedAll(t, string(seq[:split]), string(seq[split:]))
		if query || len(bodies) != 1 || string(bodies[0]) != "state=working:progress=40" {
			t.Fatalf("split at %d: bodies=%q query=%v", split, bodies, query)
		}
	}
}

func TestOSC7501Query(t *testing.T) {
	bodies, query := feedAll(t, "\x1b]7501;?\x1b\\")
	if !query || len(bodies) != 0 {
		t.Fatalf("query: bodies=%q query=%v", bodies, query)
	}
	if want := "\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\"; osc7501Reply != want {
		t.Fatalf("reply = %q, want %q", osc7501Reply, want)
	}
}

func TestOSC7501IgnoresOtherSequences(t *testing.T) {
	// Titles, notifications and an unknown OSC carry on; a 7501 among
	// them is still seen exactly once.
	stream := "\x1b]2;hello\x07\x1b]9;notify\x1b\\\x1b]9999;x\x07\x1b]7501;state=idle\x1b\\tail"
	bodies, query := feedAll(t, stream)
	if query || len(bodies) != 1 || string(bodies[0]) != "state=idle" {
		t.Fatalf("mixed stream: bodies=%q query=%v", bodies, query)
	}

	// An OSC number that merely starts with 7501 (75011) must not match.
	bodies, query = feedAll(t, "\x1b]75011;state=idle\x1b\\")
	if query || len(bodies) != 0 {
		t.Fatalf("75011: bodies=%q query=%v", bodies, query)
	}
}

func TestOSC7501CanceledAndCapped(t *testing.T) {
	// ESC inside a body cancels unless it is the ST.
	bodies, _ := feedAll(t, "\x1b]7501;state=wor\x1b[2Jking\x1b\\")
	if len(bodies) != 0 {
		t.Fatalf("canceled body leaked: %q", bodies)
	}

	// A body run past the cap is dropped whole.
	long := make([]byte, osc7501Max+64)
	for i := range long {
		long[i] = 'x'
	}
	bodies, _ = feedAll(t, "\x1b]7501;"+string(long)+"\x1b\\")
	if len(bodies) != 0 {
		t.Fatalf("over-cap body leaked: %d bytes", len(bodies[0]))
	}

	// Unterminated junk changes nothing afterward.
	bodies, query := feedAll(t, "\x1b]7501;state=working", "\x1b]7501;state=done\x1b\\")
	if query || len(bodies) != 1 || string(bodies[0]) != "state=done" {
		t.Fatalf("after unterminated: bodies=%q query=%v", bodies, query)
	}
}
