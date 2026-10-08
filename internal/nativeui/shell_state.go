package nativeui

// shellState is the unified lifecycle of the shell's Herdr connection. It is
// presentation-only: it classifies the most recent runtime interaction for the
// shell chrome and never carries runtime truth, which stays in the Herdr
// projection.
type shellState uint8

const (
	// stateLoading: a Herdr operation is in flight (bootstrap, instance
	// switch, mutation). Loading wins over a stale error it may resolve.
	stateLoading shellState = iota
	// stateReady: no error and the shell has nothing to reconcile.
	stateReady
	// stateDegraded: a presentation surface failed over a live Herdr
	// snapshot (for example a terminal attach), runtime itself reachable.
	stateDegraded
	// stateDisconnected: Herdr itself is unreachable (dial/list/bootstrap
	// failure).
	stateDisconnected
	// stateError: an operation failed and no Herdr snapshot is available
	// to present.
	stateError
)

func (st shellState) String() string {
	switch st {
	case stateLoading:
		return "loading"
	case stateReady:
		return "ready"
	case stateDegraded:
		return "degraded"
	case stateDisconnected:
		return "disconnected"
	default:
		return "error"
	}
}

type shellStateInput struct {
	Loading     bool
	ErrText     string
	Offline     bool
	HasSnapshot bool
}

// classifyShellState maps the last runtime interaction onto the lifecycle:
// work in flight wins, then a clean slate is ready, an unreachable Herdr is
// disconnected, a failed surface over a live snapshot is degraded, and any
// other failure is error.
func classifyShellState(in shellStateInput) shellState {
	switch {
	case in.Loading:
		return stateLoading
	case in.ErrText == "":
		return stateReady
	case in.Offline:
		return stateDisconnected
	case in.HasSnapshot:
		return stateDegraded
	default:
		return stateError
	}
}

func (s *Shell) shellState() shellState {
	return classifyShellState(shellStateInput{
		Loading:     s.loading,
		ErrText:     s.errText,
		Offline:     s.offline,
		HasSnapshot: s.projection.Protocol != 0,
	})
}
