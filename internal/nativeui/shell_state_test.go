package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

func TestClassifyShellState(t *testing.T) {
	cases := []struct {
		name string
		in   shellStateInput
		want shellState
	}{
		{"fresh shell", shellStateInput{}, stateReady},
		{"loading wins over a stale error", shellStateInput{Loading: true, ErrText: "boom"}, stateLoading},
		{"healthy with a live snapshot", shellStateInput{HasSnapshot: true}, stateReady},
		{"herdr unreachable", shellStateInput{ErrText: "dial tcp: refused", Offline: true, HasSnapshot: true}, stateDisconnected},
		{"surface failure over a live snapshot", shellStateInput{ErrText: "attach failed", HasSnapshot: true}, stateDegraded},
		{"failure without a snapshot", shellStateInput{ErrText: "dial tcp: refused"}, stateError},
	}
	for _, tc := range cases {
		if got := classifyShellState(tc.in); got != tc.want {
			t.Fatalf("%s: classifyShellState(%+v) = %s, want %s", tc.name, tc.in, got, tc.want)
		}
	}
}

// TestShellStateMapsShellFields pins the mapping from the Shell's runtime
// bookkeeping onto the lifecycle without needing a live Herdr socket.
func TestShellStateMapsShellFields(t *testing.T) {
	s := NewShell()
	s.loading = true
	if got := s.shellState(); got != stateLoading {
		t.Fatalf("initial shell state = %s, want loading", got)
	}

	s.loading = false
	s.projection.Protocol = 22
	if got := s.shellState(); got != stateReady {
		t.Fatalf("healthy shell state = %s, want ready", got)
	}

	s.errText = "terminal attach failed"
	if got := s.shellState(); got != stateDegraded {
		t.Fatalf("attach failure state = %s, want degraded", got)
	}

	s.offline = true
	if got := s.shellState(); got != stateDisconnected {
		t.Fatalf("dial failure state = %s, want disconnected", got)
	}

	s.projection = herdr.Projection{}
	s.offline = false
	if got := s.shellState(); got != stateError {
		t.Fatalf("failure without snapshot state = %s, want error", got)
	}
}
