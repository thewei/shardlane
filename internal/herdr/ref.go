package herdr

import (
	"strings"
)

// An instance key is the opaque string the shell stores as its active
// instance and passes back into every Manager call. Local keys are Herdr
// session names; remote keys name one saved SSH machine profile
// ("machine/<profile-id>"), which pins the instance to that profile's
// explicit remote session. ParseRef resolves a key for the data plane.

// Ref identifies the target of one instance key: a local Herdr session, or
// one saved SSH machine's remote session.
type Ref struct {
	// Session is the local Herdr session name ("" = default). Empty for
	// remote refs: the remote session comes from the machine profile.
	Session string
	// Machine is the saved SSH machine profile ID; non-empty means remote.
	Machine string
}

const machineKeyPrefix = "machine/"

// MachineInstanceKey builds the instance key for one saved machine profile.
func MachineInstanceKey(profileID string) string {
	return machineKeyPrefix + profileID
}

// ParseRef splits an instance key into its local/remote target.
func ParseRef(key string) Ref {
	if id, ok := strings.CutPrefix(strings.TrimSpace(key), machineKeyPrefix); ok && id != "" {
		return Ref{Machine: id}
	}
	return Ref{Session: normalizeSession(key)}
}

// IsRemote reports whether the ref targets a saved SSH machine.
func (r Ref) IsRemote() bool { return r.Machine != "" }

// reachableSocket ensures the instance's Herdr runtime is reachable and
// returns the RPC socket to use for it. Local sessions start their server
// through the Herdr CLI when needed; machine refs never start anything —
// they bring up the SSH bridge and answer only if the remote server is
// already running (the bridge never installs, starts, or restarts servers).
func (m *Manager) reachableSocket(key string) (string, error) {
	ref := ParseRef(key)
	if !ref.IsRemote() {
		if err := m.EnsureRunning(ref.Session); err != nil {
			return "", err
		}
		return SocketPath(ref.Session), nil
	}
	machine, err := m.machineByID(ref.Machine)
	if err != nil {
		return "", err
	}
	return m.ensureBridge(machine)
}

// socketFor resolves the instance's RPC socket without any reachability
// work: the local per-session socket, or the SSH bridge endpoint for a
// machine key (bringing the bridge up when it is not connected yet).
func (m *Manager) socketFor(key string) (string, error) {
	ref := ParseRef(key)
	if !ref.IsRemote() {
		return SocketPath(ref.Session), nil
	}
	machine, err := m.machineByID(ref.Machine)
	if err != nil {
		return "", err
	}
	return m.ensureBridge(machine)
}
