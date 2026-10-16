package agentstatus

import (
	"testing"
	"time"
)

// base is the epoch every test measures from.
var base = time.Unix(1700000000, 0)

func TestSetReplaceAndClear(t *testing.T) {
	var s Set
	base64of := func(s string) string { return "state=" + s }

	s.Apply(mustParse(t, "state=working:app=claude-code:msg=U3RhcnRlZA=="), base)
	// A report replaces its record completely: the missing key is gone.
	s.Apply(mustParse(t, "state=working"), base.Add(time.Second))
	if rep, ok := s.Display(); !ok || rep.App != "" || rep.Msg != "" {
		t.Fatalf("replace: %+v ok=%v", rep, ok)
	}

	// Children under a/b, plus an unrelated one.
	s.Apply(mustParse(t, "state=done:id=a"), base.Add(2*time.Second))
	s.Apply(mustParse(t, "state=working:id=a/b"), base.Add(3*time.Second))
	s.Apply(mustParse(t, "state=idle:id=other"), base.Add(4*time.Second))

	// clear with an id removes the record and everything beneath it.
	// The root working record is untouched and still displays.
	s.Apply(mustParse(t, base64of("clear")+":id=a"), base.Add(5*time.Second))
	if rep, ok := s.Display(); !ok || rep.State != StateWorking {
		t.Fatalf("after clear a: %+v ok=%v, want root working", rep, ok)
	}
	// A blocked child surfaces even over a working root: it is what the
	// user must act on.
	s.Apply(mustParse(t, "state=blocked:id=other"), base.Add(5500*time.Millisecond))
	if rep, _ := s.Display(); rep.State != StateBlocked || rep.ID != "other" {
		t.Fatalf("blocked child: %+v", rep)
	}

	// clear without an id removes every record.
	s.Apply(mustParse(t, "state=clear"), base.Add(6*time.Second))
	if _, ok := s.Display(); ok {
		t.Fatal("clear all left records")
	}
}

func TestSetDropTransientAndReset(t *testing.T) {
	var s Set
	s.Apply(mustParse(t, "state=working"), base)
	s.Apply(mustParse(t, "state=blocked:id=child"), base)
	s.Apply(mustParse(t, "state=done:id=kept"), base)

	s.DropTransient()
	if rep, _ := s.Display(); rep.State != StateDone {
		t.Fatalf("drop transient: %+v, want done", rep)
	}
	if s.Transient() {
		t.Fatal("transient survived")
	}

	s.Reset()
	if s.Transient() {
		t.Fatal("reset left records")
	}
}

func TestSetDisplayRankAndInheritance(t *testing.T) {
	var s Set
	// No root: the display choice picks blocked over working, and the
	// app comes from the nearest ancestor that has one.
	s.Apply(mustParse(t, "state=working:id=deploy:app=cargo"), base)
	s.Apply(mustParse(t, "state=blocked:id=deploy/eu:kind=permission"), base)
	rep, ok := s.Display()
	if !ok || rep.State != StateBlocked || rep.App != "cargo" || rep.Kind != KindPermission {
		t.Fatalf("display: %+v ok=%v", rep, ok)
	}

	// The display rank applies to the root too: a blocked record beats a
	// done root; the app still comes from the nearest ancestor.
	s.Apply(mustParse(t, "state=done:app=deploy"), base.Add(time.Second))
	rep, _ = s.Display()
	if rep.State != StateBlocked || rep.ID != "deploy/eu" || rep.App != "cargo" {
		t.Fatalf("rank display: %+v", rep)
	}

	// With the urgent child gone the ranks resume: deploy's working
	// beats the done root until it too clears; then the root remains.
	s.Apply(mustParse(t, "state=clear:id=deploy/eu"), base.Add(1500*time.Millisecond))
	rep, _ = s.Display()
	if rep.State != StateWorking || rep.App != "cargo" {
		t.Fatalf("mid display: %+v", rep)
	}
	// Clearing deploy leaves only the root's own done summary.
	s.Apply(mustParse(t, "state=clear:id=deploy"), base.Add(1600*time.Millisecond))
	rep, _ = s.Display()
	if rep.State != StateDone || rep.App != "deploy" {
		t.Fatalf("root display: %+v", rep)
	}

	// A child without an ancestor app keeps its app empty.
	s.Apply(mustParse(t, "state=clear:id=deploy"), base.Add(2*time.Second))
	s.Apply(mustParse(t, "state=clear"), base.Add(3*time.Second))
	s.Apply(mustParse(t, "state=idle:id=solo"), base.Add(4*time.Second))
	if rep, _ := s.Display(); rep.App != "" || rep.State != StateIdle {
		t.Fatalf("solo: %+v", rep)
	}
}

func TestSetLRUCap(t *testing.T) {
	var s Set
	for i := 0; i < maxRecords; i++ {
		s.Apply(mustParse(t, "state=working:id=r"+string(rune('a'+i%26))+string(rune('a'+i/26))), base.Add(time.Duration(i)*time.Second))
	}
	first := "r" + string(rune('a')) + string(rune('a')) // id of the first report
	if _, ok := s.records[first]; !ok {
		t.Fatalf("%s missing before cap", first)
	}
	// One more record evicts the least recently updated.
	s.Apply(mustParse(t, "state=working:id=fresh"), base.Add(maxRecords*time.Second))
	if _, ok := s.records[first]; ok {
		t.Fatalf("%s survived eviction", first)
	}
	if _, ok := s.records["fresh"]; !ok {
		t.Fatal("fresh record missing")
	}
}

func TestSetLRUEvictsLeastRecent(t *testing.T) {
	var s Set
	// Spec LRU: the record updated least recently is removed, root
	// included. Fill to the cap with the root strictly oldest.
	s.Apply(mustParse(t, "state=working:app=claude-code"), base)
	for i := 0; i < maxRecords-1; i++ {
		id := "c" + string(rune('a'+i%26)) + string(rune('a'+i/26))
		s.Apply(mustParse(t, "state=working:id="+id), base.Add(time.Duration(i+1)*time.Second))
	}
	if _, ok := s.records[""]; !ok {
		t.Fatal("root missing before cap")
	}
	// Two more records, two evictions: the stale root first, then the
	// oldest child.
	s.Apply(mustParse(t, "state=working:id=fresh"), base.Add(maxRecords*time.Second))
	s.Apply(mustParse(t, "state=working:id=fresh2"), base.Add(maxRecords*time.Second+time.Second))
	if _, ok := s.records[""]; ok {
		t.Fatal("stale root survived eviction")
	}
	if _, ok := s.records["caa"]; ok { // the oldest child
		t.Fatal("oldest child survived eviction")
	}
	if _, ok := s.records["fresh"]; !ok {
		t.Fatal("fresh record missing")
	}

	// A root kept current is never the least recently updated.
	var w Set
	w.Apply(mustParse(t, "state=working:app=deploy"), base)
	for i := 0; i < maxRecords-1; i++ {
		id := "d" + string(rune('a'+i%26)) + string(rune('a'+i/26))
		w.Apply(mustParse(t, "state=working:id="+id), base.Add(time.Duration(i)*time.Second))
	}
	w.Apply(mustParse(t, "state=working"), base.Add(maxRecords*time.Second)) // root refresh
	w.Apply(mustParse(t, "state=working:id=fresh3"), base.Add(maxRecords*time.Second+time.Second))
	if _, ok := w.records[""]; !ok {
		t.Fatal("current root evicted")
	}
}
