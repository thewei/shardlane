package agentstatus

import "testing"

// mustParse parses a body or fails the test.
func mustParse(t *testing.T, body string) Report {
	t.Helper()
	rep, ok := Parse([]byte(body))
	if !ok {
		t.Fatalf("Parse(%q) rejected", body)
	}
	return rep
}

func TestParseSpecExamples(t *testing.T) {
	// The spec's own worked example: blocked on a terraform apply, with
	// the message base64-encoded.
	rep := mustParse(t, "state=blocked:kind=permission:app=terraform:msg=QXBwbHkgMyB0byBhZGQsIDEgdG8gY2hhbmdlLCAwIHRvIGRlc3Ryb3k/")
	if rep.State != StateBlocked || rep.Kind != KindPermission || rep.App != "terraform" {
		t.Fatalf("example: %+v", rep)
	}
	if rep.Msg != "Apply 3 to add, 1 to change, 0 to destroy?" {
		t.Fatalf("msg = %q", rep.Msg)
	}
	if rep.Progress != nil {
		t.Fatalf("progress = %v, want nil", *rep.Progress)
	}

	rep = mustParse(t, "state=working:progress=50")
	if rep.Progress == nil || *rep.Progress != 50 {
		t.Fatalf("progress = %v", rep.Progress)
	}

	rep = mustParse(t, "state=clear")
	if rep.State != StateClear {
		t.Fatalf("clear: %+v", rep)
	}
}

func TestParsePairRules(t *testing.T) {
	// Unknown keys ignored, last value wins, malformed pairs skipped and
	// the rest of the report still processed.
	rep := mustParse(t, "future=x:state=idle:state=working:nonsense:empty=:UPPER=x:state")
	if rep.State != StateWorking {
		t.Fatalf("state = %q", rep.State)
	}

	// Whitespace around keys and values is removed.
	rep = mustParse(t, "state = done : app = cargo ")
	if rep.State != StateDone || rep.App != "cargo" {
		t.Fatalf("trim: %+v", rep)
	}

	// A value byte outside the set skips just that pair.
	rep = mustParse(t, "app=bad app:state=done")
	if rep.App != "" || rep.State != StateDone {
		t.Fatalf("value set: %+v", rep)
	}
}

func TestParseWholeReportDiscards(t *testing.T) {
	// A missing or unrecognized state ignores the report; it never
	// becomes idle.
	for _, body := range []string{"", "app=cargo", "state=thinking"} {
		if rep, ok := Parse([]byte(body)); ok {
			t.Fatalf("%q: accepted as %+v", body, rep)
		}
	}
	// A value byte outside the set is a pair-level fault: the id pair is
	// skipped and the report applies without an id.
	rep, ok := Parse([]byte("state=done:id=has space"))
	if !ok || rep.ID != "" {
		t.Fatalf("id with space: ok=%v rep=%+v, want pair skipped", ok, rep)
	}
	// A syntactically valid but grammar-violating id discards the report.
	for _, id := range []string{
		"a//b",                                // empty segment
		"a/b/c/d/e/f/g/h/i",   // 9 levels
		"/leading",                            // empty first segment
		"thissegmentiswaytoolongtobeallowedbythespec", // 45 bytes
	} {
		if _, ok := Parse([]byte("state=done:id=" + id)); ok {
			t.Fatalf("bad id %q accepted", id)
		}
	}
	// Characters outside the value set are a pair-level fault: the pair
	// is skipped and the rest applies.
	rep, ok = Parse([]byte("state=done:msg=!!!not-base64!!!"))
	if !ok || rep.Msg != "" {
		t.Fatalf("charset fault: ok=%v rep=%+v, want pair skipped", ok, rep)
	}
	// In-set bytes that still fail base64 discard the report whole.
	if _, ok := Parse([]byte("state=done:msg=abcde")); ok {
		t.Fatal("bad base64 accepted")
	}
	if _, ok := Parse([]byte("state=done:msg=YQpu")); ok { // "a\nb"
		t.Fatal("control character accepted")
	}
	// Nothing from a discarded report may apply.
	if rep, ok := Parse([]byte("state=done:msg=YQpu")); ok || rep.State != "" {
		t.Fatalf("discarded report leaked %+v", rep)
	}
}

func TestParseFieldRules(t *testing.T) {
	// progress only with working/blocked; anything invalid is absent.
	rep := mustParse(t, "state=done:progress=40")
	if rep.Progress != nil {
		t.Fatal("progress survived done")
	}
	rep = mustParse(t, "state=working:progress=101")
	if rep.Progress != nil {
		t.Fatal("progress 101 accepted")
	}
	rep = mustParse(t, "state=working:progress=abc")
	if rep.Progress != nil {
		t.Fatal("progress abc accepted")
	}

	// kind only with blocked; unknown kinds treated as absent.
	rep = mustParse(t, "state=working:kind=permission")
	if rep.Kind != "" {
		t.Fatal("kind survived working")
	}
	rep = mustParse(t, "state=blocked:kind=LaterKind")
	if rep.Kind != "" {
		t.Fatal("unknown kind accepted")
	}

	// app outside the character set or cap treated as absent.
	rep = mustParse(t, "state=done:app=" + string(make([]byte, 0)) + "claude-code")
	if rep.App != "claude-code" {
		t.Fatalf("app = %q", rep.App)
	}
	rep = mustParse(t, "state=done:app=thisappnameisfarlongerthanthirtytwobytes")
	if rep.App != "" {
		t.Fatal("over-cap app accepted")
	}

	// base64 padding is optional.
	rep = mustParse(t, "state=done:msg=SGVsbG8") // "Hello" unpadded
	if rep.Msg != "Hello" {
		t.Fatalf("unpadded msg = %q", rep.Msg)
	}
}

func TestParseLimits(t *testing.T) {
	big := make([]byte, 300)
	for i := range big {
		big[i] = 'a'
	}
	if _, ok := Parse([]byte("state=done:title=" + string(big))); ok {
		t.Fatal("over-cap title accepted")
	}
}
