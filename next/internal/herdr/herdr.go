// Package herdr is the Go-side adapter for the Herdr runtime.
//
// [INPUT]: Herdr CLI/session disk layout and local session sockets.
// [OUTPUT]: runtime instance discovery, bootstrap, and native-TUI launch commands.
// [POS]: migration-era runtime adapter; Herdr remains the sole runtime authority.
package herdr

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"
)

const (
	defaultSession       = "default"
	minSupportedProtocol = 22
)

var nestedTUIEnvKeys = []string{
	"HERDR_ENV",
	"HERDR_BIN_PATH",
	"HERDR_WORKSPACE_ID",
	"HERDR_TAB_ID",
	"HERDR_PANE_ID",
	"HERDR_STARTUP_CWD",
	"HERDR_CLIENT_SOCKET_PATH",
	"HERDR_SESSION",
	"HERDR_SOCKET_PATH",
}

// Instance is one Herdr session. In Shardlane's multi-instance model an
// instance is a top-level Workspace; projects/tabs/panes remain Herdr-owned.
// Machine instances are remote: one saved SSH machine profile pins the
// instance to that profile's explicit remote Herdr session, and Name is the
// opaque machine instance key, not a session name.
type Instance struct {
	Name        string   `json:"name"`
	DisplayName string   `json:"display_name"`
	Running     bool     `json:"running"`
	Default     bool     `json:"default"`
	Machine     *Machine `json:"machine,omitempty"`
}

type sessionListResponse struct {
	Sessions []struct {
		Name    string `json:"name"`
		Running bool   `json:"running"`
		Default bool   `json:"default"`
	} `json:"sessions"`
}

// Manager resolves and talks to the installed Herdr CLI without owning any
// Herdr runtime state itself.
type Manager struct {
	// machineCache backs machineByID: the watcher's retry loop resolves its
	// machine profile on every reconnect attempt, and each miss would spawn
	// one CLI process. UI-facing list reads stay uncached and always fresh.
	machineMu      sync.Mutex
	machineCache   []Machine
	machineCacheAt time.Time
}

func NewManager() *Manager { return &Manager{} }

// ListInstances asks Herdr itself for the instance list. No client-side
// registry is introduced by the Go migration.
func (m *Manager) ListInstances() ([]Instance, error) {
	cli, err := resolveCLI()
	if err != nil {
		return nil, err
	}
	cmd := exec.Command(cli, "session", "list", "--json")
	cmd.Stdin = nil
	out, err := cmd.Output()
	if err != nil {
		var exit *exec.ExitError
		if errors.As(err, &exit) {
			return nil, fmt.Errorf("herdr session list: %s", strings.TrimSpace(string(exit.Stderr)))
		}
		return nil, fmt.Errorf("herdr session list: %w", err)
	}
	var wire sessionListResponse
	if err := json.Unmarshal(out, &wire); err != nil {
		return nil, fmt.Errorf("decode herdr session list: %w", err)
	}
	instances := make([]Instance, 0, len(wire.Sessions))
	for _, session := range wire.Sessions {
		name := strings.TrimSpace(session.Name)
		if name == "" {
			continue
		}
		display := readDisplayName(name)
		if display == "" {
			display = name
		}
		instances = append(instances, Instance{
			Name:        name,
			DisplayName: display,
			Running:     session.Running,
			Default:     session.Default,
		})
	}
	return instances, nil
}

// EnsureRunning starts the selected Herdr session only when its socket is not
// accepting connections. Starting Herdr is still delegated to the Herdr CLI.
// Machine (remote) refs never reach here — the bridge never starts servers.
func (m *Manager) EnsureRunning(session string) error {
	if err := guardLocalInstance(session, "starting a Herdr session"); err != nil {
		return err
	}
	session = normalizeSession(session)
	socket := SocketPath(session)
	if _, err := ping(socket); err == nil {
		return nil
	}
	cli, err := resolveCLI()
	if err != nil {
		return err
	}
	cmd := exec.Command(cli, "server")
	cmd.Env = runtimeEnv(session)
	cmd.Stdin = nil
	cmd.Stdout = nil
	cmd.Stderr = nil
	if err := cmd.Start(); err != nil {
		return fmt.Errorf("start herdr server for %q: %w", session, err)
	}
	_ = cmd.Process.Release()

	deadline := time.Now().Add(5 * time.Second)
	var lastErr error
	for time.Now().Before(deadline) {
		if _, err := ping(socket); err == nil {
			return nil
		} else {
			lastErr = err
		}
		time.Sleep(100 * time.Millisecond)
	}
	return fmt.Errorf("herdr session %q did not become ready on %s: %w", session, socket, lastErr)
}

// TerminalAttachCommand builds the native MyGo terminal child command for one
// Herdr-owned pane terminal. It uses Herdr's supported direct-attach client so
// Shardlane renders no Herdr shell chrome and owns no PTY/process runtime.
// Remote instances attach through their SSH bridge's local endpoint.
func (m *Manager) TerminalAttachCommand(session, terminalID string) ([]string, error) {
	session = strings.TrimSpace(session)
	terminalID = strings.TrimSpace(terminalID)
	if terminalID == "" {
		return nil, errors.New("Herdr terminal id is empty")
	}
	socket, err := m.socketFor(session)
	if err != nil {
		return nil, err
	}
	cli, err := resolveCLI()
	if err != nil {
		return nil, err
	}
	if runtime.GOOS == "windows" {
		return nil, errors.New("native Herdr terminal command is not ported to Windows yet")
	}
	return terminalAttachArgs(cli, socket, ConfigPath(), terminalID), nil
}

func terminalAttachArgs(cli, socketPath, configPath, terminalID string) []string {
	args := []string{"/usr/bin/env"}
	for _, key := range nestedTUIEnvKeys {
		args = append(args, "-u", key)
	}
	return append(args,
		"HERDR_SOCKET_PATH="+socketPath,
		"HERDR_CONFIG_PATH="+configPath,
		cli, "terminal", "attach", terminalID,
	)
}

// SocketPath returns Herdr's canonical per-session local socket path.
func SocketPath(session string) string {
	session = normalizeSession(session)
	base := configDir()
	if session == defaultSession {
		return filepath.Join(base, "herdr.sock")
	}
	return filepath.Join(base, "sessions", session, "herdr.sock")
}

// ConfigPath returns the user's canonical Herdr config without rewriting it.
func ConfigPath() string {
	if path := strings.TrimSpace(os.Getenv("HERDR_CONFIG_PATH")); path != "" {
		return path
	}
	return filepath.Join(configDir(), "config.toml")
}

func normalizeSession(session string) string {
	session = strings.TrimSpace(session)
	if session == "" {
		return defaultSession
	}
	return session
}

func configDir() string {
	if runtime.GOOS == "windows" {
		if appData := os.Getenv("APPDATA"); appData != "" {
			return filepath.Join(appData, "herdr")
		}
	}
	home, err := os.UserHomeDir()
	if err != nil || home == "" {
		home = "."
	}
	return filepath.Join(home, ".config", "herdr")
}

func sessionDir(session string) string {
	if normalizeSession(session) == defaultSession {
		return configDir()
	}
	return filepath.Join(configDir(), "sessions", normalizeSession(session))
}

func readDisplayName(session string) string {
	data, err := os.ReadFile(filepath.Join(sessionDir(session), "workspace.json"))
	if err != nil {
		return ""
	}
	var value struct {
		DisplayName string `json:"display_name"`
	}
	if json.Unmarshal(data, &value) != nil {
		return ""
	}
	return strings.TrimSpace(value.DisplayName)
}

type pingResult struct {
	Version      string             `json:"version"`
	Protocol     int                `json:"protocol"`
	Capabilities serverCapabilities `json:"capabilities"`
}

type serverCapabilities struct {
	EndpointProtocolGeneration *uint32 `json:"endpoint_protocol_generation"`
	SurfaceInterest            bool    `json:"surface_interest"`
	HealthCheck                bool    `json:"health_check"`
}

func ping(path string) (pingResult, error) {
	var result pingResult
	if err := callRPC(path, "ping", map[string]any{}, false, &result); err != nil {
		return pingResult{}, err
	}
	if result.Protocol < minSupportedProtocol {
		return pingResult{}, fmt.Errorf("incompatible Herdr protocol: need %d+, got %d", minSupportedProtocol, result.Protocol)
	}
	return result, nil
}

func runtimeEnv(session string) []string {
	blocked := map[string]struct{}{
		"HERDR_SESSION":     {},
		"HERDR_SOCKET_PATH": {},
	}
	env := make([]string, 0, len(os.Environ())+1)
	for _, item := range os.Environ() {
		key, _, _ := strings.Cut(item, "=")
		if _, skip := blocked[key]; !skip {
			env = append(env, item)
		}
	}
	if normalizeSession(session) != defaultSession {
		env = append(env, "HERDR_SESSION="+normalizeSession(session))
	}
	return env
}

func resolveCLI() (string, error) {
	if path := strings.TrimSpace(os.Getenv("HERDR_BIN_PATH")); path != "" {
		if info, err := os.Stat(path); err == nil && !info.IsDir() {
			return path, nil
		}
	}
	if path, err := exec.LookPath("herdr"); err == nil {
		return path, nil
	}
	if runtime.GOOS != "windows" {
		if out, err := exec.Command("/bin/zsh", "-lic", "command -v herdr").Output(); err == nil {
			if path := strings.TrimSpace(string(out)); path != "" {
				return path, nil
			}
		}
		for _, dir := range []string{"/opt/homebrew/bin", "/usr/local/bin"} {
			path := filepath.Join(dir, "herdr")
			if info, err := os.Stat(path); err == nil && !info.IsDir() {
				return path, nil
			}
		}
	}
	return "", errors.New("herdr CLI not found; install Herdr before starting Shardlane")
}
