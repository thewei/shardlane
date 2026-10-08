package herdr

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"
)

// SSH socket bridge: the data plane for saved-machine instances. Herdr's
// `--machine` prefix forwards single API commands over SSH, but not
// interactive terminal attachment or long-lived event streams — both are raw
// socket sessions. A forwarded unix socket makes a remote Herdr server
// addressable through the SAME client path as local ones: every RPC, event
// subscription, and `herdr terminal attach` child speaks to the bridge's
// local endpoint. This mirrors the proven Rust ssh_bridge. The bridge never
// starts or restarts a remote server; unreachable servers surface as errors.

type sshBridge struct {
	socket string
	cmd    *exec.Cmd
}

// bridges is process-global: the ssh children must outlive the goroutines
// that spawned them, and they are killed when the shell closes (the app's
// graceful shutdown). A crashed app can leave an `ssh -N` behind; it holds
// one idle TCP connection and its forwarded socket is replaced on rebuild.
var bridges = struct {
	mu      sync.Mutex
	byID    map[string]*sshBridge
	closing bool
}{byID: make(map[string]*sshBridge)}

// ensureBridge returns the local endpoint of the machine's SSH bridge,
// bringing one up (or replacing a dead one) when needed. The ping probe in
// the bring-up and the live-bridge check is what turns "remote server not
// running" into an error instead of a hang.
func (m *Manager) ensureBridge(machine Machine) (string, error) {
	if runtime.GOOS == "windows" {
		return "", fmt.Errorf("SSH socket bridging is not ported to Windows yet")
	}
	bridges.mu.Lock()
	defer bridges.mu.Unlock()
	if bridges.closing {
		return "", fmt.Errorf("Shardlane is shutting down")
	}
	if bridge, ok := bridges.byID[machine.ID]; ok {
		if _, err := ping(bridge.socket); err == nil {
			return bridge.socket, nil
		}
		_ = bridge.cmd.Process.Kill()
		_ = os.Remove(bridge.socket)
		delete(bridges.byID, machine.ID)
	}
	bridge, err := m.bringUpBridge(machine)
	if err != nil {
		return "", err
	}
	bridges.byID[machine.ID] = bridge
	return bridge.socket, nil
}

// bringUpBridge resolves the remote HOME, forwards a fresh local unix socket
// to the remote session's Herdr socket, and waits for a successful Herdr
// ping through the forward. Blocking; callers run it off the UI lane.
func (m *Manager) bringUpBridge(machine Machine) (*sshBridge, error) {
	home, err := remoteHome(machine.Target)
	if err != nil {
		return nil, fmt.Errorf("%s: %w", machine.Label, err)
	}
	local := bridgeSocketPath(machine.Target, machine.Session)
	if err := os.Remove(local); err != nil && !os.IsNotExist(err) {
		return nil, fmt.Errorf("%s: clean stale bridge socket: %w", machine.Label, err)
	}
	if err := os.MkdirAll(filepath.Dir(local), 0o700); err != nil {
		return nil, fmt.Errorf("%s: create bridge dir: %w", machine.Label, err)
	}
	forward := fmt.Sprintf("%s:%s", local, remoteSocketPath(home, machine.Session))
	var stderr strings.Builder
	cmd := exec.Command("ssh",
		"-N",
		"-o", "BatchMode=yes",
		"-o", "ExitOnForwardFailure=yes",
		"-o", "ServerAliveInterval=15",
		"-o", "ServerAliveCountMax=3",
		"-L", forward,
		machine.Target,
	)
	cmd.Stdin = nil
	cmd.Stderr = &stderr
	if err := cmd.Start(); err != nil {
		return nil, fmt.Errorf("%s: spawn ssh: %w", machine.Label, err)
	}
	live := false
	defer func() {
		if live {
			return
		}
		_ = cmd.Process.Kill()
		_, _ = cmd.Process.Wait()
		_ = os.Remove(local)
	}()

	var lastErr error
	deadline := time.Now().Add(10 * time.Second)
	for time.Now().Before(deadline) {
		if _, err := ping(local); err == nil {
			live = true
			return &sshBridge{socket: local, cmd: cmd}, nil
		} else {
			lastErr = err
		}
		// Signal 0: ssh exited early (ExitOnForwardFailure, auth refusal).
		if err := cmd.Process.Signal(nil); err != nil {
			break
		}
		time.Sleep(250 * time.Millisecond)
	}
	if detail := strings.TrimSpace(stderr.String()); detail != "" {
		lastErr = fmt.Errorf("%w (%s)", lastErr, detail)
	}
	return nil, fmt.Errorf("%s: remote Herdr at %s did not come up: %w", machine.Label, machine.Target, lastErr)
}

// CloseBridges kills every live SSH bridge. Called from the shell's close
// path; later ensureBridge calls fail instead of reconnecting.
func CloseBridges() {
	bridges.mu.Lock()
	defer bridges.mu.Unlock()
	bridges.closing = true
	for id, bridge := range bridges.byID {
		_ = bridge.cmd.Process.Kill()
		_ = os.Remove(bridge.socket)
		delete(bridges.byID, id)
	}
}

// remoteHome resolves the remote user's HOME over a non-interactive SSH run.
// Passphrase-protected keys must be loaded into ssh-agent beforehand — the
// same rule Herdr's own background connections follow.
func remoteHome(target string) (string, error) {
	cmd := exec.Command("ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", target, "echo $HOME")
	cmd.Stdin = nil
	out, err := cmd.Output()
	if err != nil {
		detail := err.Error()
		var exit *exec.ExitError
		if asExitError(err, &exit) {
			detail = strings.TrimSpace(string(exit.Stderr))
		}
		return "", fmt.Errorf("ssh %s failed: %s", target, detail)
	}
	home := strings.TrimSpace(string(out))
	if home == "" {
		return "", fmt.Errorf("remote $HOME resolved empty")
	}
	return home, nil
}

// bridgeSocketPath is the deterministic local endpoint for one (target,
// session) pair; rebuilding a bridge always recreates the same path.
func bridgeSocketPath(target, session string) string {
	return filepath.Join(bridgeDir(), bridgeSlug(target, session)+".sock")
}

func bridgeDir() string {
	home, err := os.UserHomeDir()
	if err != nil || home == "" {
		home = "."
	}
	return filepath.Join(home, ".shardlane", "ssh-bridges")
}

func bridgeSlug(target, session string) string {
	raw := target + "-" + normalizeSession(session)
	var builder strings.Builder
	for _, char := range strings.ToLower(raw) {
		if (char >= 'a' && char <= 'z') || (char >= '0' && char <= '9') {
			builder.WriteRune(char)
		} else {
			builder.WriteRune('_')
		}
	}
	return builder.String()
}

// remoteSocketPath mirrors the local SocketPath layout on the remote HOME.
func remoteSocketPath(home, session string) string {
	session = normalizeSession(session)
	if session == defaultSession {
		return filepath.Join(home, ".config", "herdr", "herdr.sock")
	}
	return filepath.Join(home, ".config", "herdr", "sessions", session, "herdr.sock")
}
