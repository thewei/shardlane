package agentstatus

import (
	"testing"
	"time"
)

// hook feeds a hook status and returns what the caller should apply.
func hook(s *Store, pane, status string, at time.Time) (string, bool) {
	return s.ApplyHookStatus(pane, status, at)
}

func TestStoreProtocolWinsWhileTransient(t *testing.T) {
	s := NewStore()
	// Native working report, then the same agent's own hook fires
	// working seconds later: the hook is dropped, the protocol stream
	// stays authoritative.
	if st, ok := s.ApplyProgramReport("p1", []byte("state=working:app=claude-code"), base); !ok || st != "working" {
		t.Fatalf("report: %q ok=%v", st, ok)
	}
	if st, applied := hook(s, "p1", "working", base.Add(time.Second)); applied {
		t.Fatalf("hook passed mid-flight: %q", st)
	}
	// A blocked transition arrives through the protocol; the hook's
	// blocked is still dropped.
	s.ApplyProgramReport("p1", []byte("state=blocked:kind=permission"), base.Add(2*time.Second))
	if st, applied := hook(s, "p1", "blocked", base.Add(3*time.Second)); applied {
		t.Fatalf("hook passed blocked: %q", st)
	}
	if st := s.Display("p1"); st != "blocked" {
		t.Fatalf("display = %q", st)
	}
}

func TestStoreHookTakesOverAfterSurvivor(t *testing.T) {
	s := NewStore()
	// A native session ends with done; the record survives.
	s.ApplyProgramReport("p1", []byte("state=done:msg=RG9uZQ=="), base)
	// A hook-only program (an older agent) starts in the same pane:
	// nothing transient stands, so the hook takes the pane back.
	if st, applied := hook(s, "p1", "working", base.Add(5*time.Second)); !applied || st != "working" {
		t.Fatalf("hook takeover: %q applied=%v", st, applied)
	}
	// From here the protocol stream re-takes the pane the moment it
	// speaks, and hooks are suppressed again mid-flight.
	s.ApplyProgramReport("p1", []byte("state=working"), base.Add(6*time.Second))
	if _, applied := hook(s, "p1", "blocked", base.Add(7*time.Second)); applied {
		t.Fatal("hook suppressed expectation failed")
	}
}

func TestStoreReleasedDropsTransient(t *testing.T) {
	s := NewStore()
	s.ApplyProgramReport("p1", []byte("state=working"), base)
	// released mirrors process exit: it always passes and clears the
	// transient record.
	if st, applied := hook(s, "p1", "released", base.Add(time.Second)); !applied || st != "released" {
		t.Fatalf("released: %q applied=%v", st, applied)
	}
	// With the working record gone, the next hook event applies.
	if _, applied := hook(s, "p1", "idle", base.Add(2*time.Second)); !applied {
		t.Fatal("hook after released dropped")
	}
}

func TestStoreStaleTransientYields(t *testing.T) {
	s := NewStore()
	// A crash leaves a working record behind and no clear ever comes.
	s.ApplyProgramReport("p1", []byte("state=working"), base)
	// Hooks within the authority window stay suppressed.
	if _, applied := hook(s, "p1", "working", base.Add(time.Minute)); applied {
		t.Fatal("hook passed inside the window")
	}
	// Past the window the protocol speaker is gone: hooks flow again.
	if st, applied := hook(s, "p1", "idle", base.Add(3*time.Minute)); !applied || st != "idle" {
		t.Fatalf("stale takeover: %q applied=%v", st, applied)
	}
}

func TestStoreInvalidBodyNoop(t *testing.T) {
	s := NewStore()
	s.ApplyProgramReport("p1", []byte("state=working"), base)
	if _, ok := s.ApplyProgramReport("p1", []byte("state=thinking"), base); ok {
		t.Fatal("invalid body accepted")
	}
	// The pane still shows the last valid state.
	if st := s.Display("p1"); st != "working" {
		t.Fatalf("display = %q", st)
	}
}

func TestStorePaneClosed(t *testing.T) {
	s := NewStore()
	s.ApplyProgramReport("p1", []byte("state=working"), base)
	s.PaneClosed("p1")
	// A fresh pane starts from whichever source speaks first; hooks are
	// no longer suppressed by the dead pane's records.
	if _, applied := hook(s, "p1", "idle", base.Add(time.Second)); !applied {
		t.Fatal("closed pane still suppresses hooks")
	}
	if st := s.Display("p1"); st != "idle" {
		t.Fatalf("display = %q", st)
	}
}

