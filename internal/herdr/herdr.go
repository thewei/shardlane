// Package herdr is the Go-side adapter for the Herdr runtime.
//
// [INPUT]: Herdr CLI/session disk layout and local session sockets.
// [OUTPUT]: runtime instance discovery, bootstrap, native-TUI launch commands, the shared one-shot CLI runner (RunCLI), the typed old-protocol sentinel (ErrProtocolTooOld), the not-writable updater sentinel (ErrUpdateNotWritable + UpdateAsRootCommand), the live runtime probe (ProbeRuntime), and UpgradeRuntime (update → verify protocol → stop a stale server).
// [POS]: migration-era runtime adapter; Herdr remains the sole runtime authority.
package herdr

import (
	"context"
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
	MinSupportedProtocol = 22
	herdrListTimeout     = 6 * time.Second
	// HerdrUpdateTimeout bounds one `herdr update --handoff` run: the CLI
	// downloads and verifies a full release, which is slow on poor networks.
	HerdrUpdateTimeout = 10 * time.Minute
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
	ctx, cancel := context.WithTimeout(context.Background(), herdrListTimeout)
	defer cancel()
	cmd := exec.CommandContext(ctx, cli, "session", "list", "--json")
	cmd.Stdin = nil
	out, err := cmd.Output()
	if ctx.Err() != nil {
		return nil, fmt.Errorf("herdr session list timed out after %s: %w", herdrListTimeout, ctx.Err())
	}
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
	if result, err := pingRPC(socket); err == nil {
		// The socket answers, so a server is already running. It is either
		// compatible (done) or too old — never spawn a second server on top
		// of a live one; the bootstrap page offers Herdr's own updater.
		if err := protocolError(result.Protocol); err != nil {
			return err
		}
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
	shardlaneSock := strings.TrimSpace(os.Getenv("SHARDLANE_SOCKET_PATH"))
	if shardlaneSock == "" {
		if home, err := os.UserHomeDir(); err == nil {
			shardlaneSock = filepath.Join(home, ".shardlane", "run", "agent-status.sock")
		}
	}
	if shardlaneSock != "" {
		args = append(args, "SHARDLANE_SOCKET_PATH="+shardlaneSock)
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

// ErrProtocolTooOld marks a reachable Herdr server whose socket protocol
// predates the client's supported minimum. The fix is a Herdr upgrade
// (UpgradeRuntime), never a retry and never a second server.
var ErrProtocolTooOld = errors.New("incompatible Herdr protocol")

// ErrUpdateNotWritable marks Herdr's own updater failing because its
// install directory is not writable by the current user — a system-location
// install (for example /usr/bin). Only the user can supply the missing
// permissions; Shardlane never escalates from a GUI process.
var ErrUpdateNotWritable = errors.New("herdr update cannot write the install directory")

// UpdateAsRootCommand is the terminal command that supplies the missing
// permissions for a system-location install; surfaces offer it for copying
// when the in-place authorization did not happen.
const UpdateAsRootCommand = "sudo herdr update --handoff"

// RunAuthorizedUpdate runs the very same updater through the OS's own
// authorization mechanism, so a system-location install is upgraded in
// place after the user grants the permission in the platform's dialog
// (polkit on Linux, administrator prompt on macOS). Shardlane never sees
// or holds credentials. Platforms without a supported mechanism return an
// error and the caller falls back to the manual command.
func RunAuthorizedUpdate(ctx context.Context) error {
	cli, err := resolveCLI()
	if err != nil {
		return err
	}
	if abs, absErr := filepath.Abs(cli); absErr == nil {
		cli = abs
	}
	var label string
	var argv []string
	switch runtime.GOOS {
	case "linux":
		// --disable-internal-agent refuses fast when no session agent is
		// registered instead of hanging on a textual prompt we can't feed.
		label = "pkexec herdr update --handoff"
		argv = []string{"pkexec", "--disable-internal-agent", cli, "update", "--handoff"}
	case "darwin":
		label = "authorized herdr update --handoff"
		argv = []string{"osascript", "-e", fmt.Sprintf("do shell script %q with administrator privileges", cli+" update --handoff")}
	default:
		return fmt.Errorf("in-place authorization is not supported on %s", runtime.GOOS)
	}
	_, err = runCommand(ctx, label, argv, nil)
	return err
}

// updateNotWritable recognizes Herdr's own "install directory not writable"
// failure for system-location installs; the phrasing is the CLI's, matched
// loosely across platforms.
func updateNotWritable(err error) bool {
	msg := strings.ToLower(err.Error())
	return strings.Contains(msg, "not writable") || strings.Contains(msg, "permission denied")
}

// protocolError gates a reported protocol against the supported minimum.
func protocolError(got int) error {
	if got >= MinSupportedProtocol {
		return nil
	}
	return fmt.Errorf("%w: need %d+, got %d", ErrProtocolTooOld, MinSupportedProtocol, got)
}

// pingRPC performs one ping round-trip without the compatibility gate;
// reachability and compatibility stay separately observable.
func pingRPC(path string) (pingResult, error) {
	var result pingResult
	if err := callRPC(path, "ping", map[string]any{}, false, &result); err != nil {
		return pingResult{}, err
	}
	return result, nil
}

func ping(path string) (pingResult, error) {
	result, err := pingRPC(path)
	if err != nil {
		return pingResult{}, err
	}
	if err := protocolError(result.Protocol); err != nil {
		return pingResult{}, err
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

// ErrCLINotFound reports that no Herdr CLI could be located.
var ErrCLINotFound = errors.New("herdr CLI not found; install Herdr before starting Shardlane")

// RunCLI runs one Herdr CLI command and returns its stdout. It finds the
// binary the way every other call here does, so an app launched from Finder
// (minimal PATH) reaches the same `herdr` as the user's terminal. A failing
// command reports the CLI's own stderr.
func RunCLI(ctx context.Context, args []string) (string, error) {
	return runCLIEnv(ctx, args, nil)
}

// runCLIEnv is RunCLI with an extra environment overlay; the session-scoped
// `server stop` inside UpgradeRuntime is its only user today.
func runCLIEnv(ctx context.Context, args []string, env []string) (string, error) {
	cli, err := resolveCLI()
	if err != nil {
		return "", err
	}
	return runCommand(ctx, "herdr "+strings.Join(args, " "), append([]string{cli}, args...), env)
}

// runCommand runs one argv and reports failures as `<label>: <stderr>`.
func runCommand(ctx context.Context, label string, argv []string, env []string) (string, error) {
	cmd := exec.CommandContext(ctx, argv[0], argv[1:]...)
	cmd.Env = append(os.Environ(), env...)
	out, err := cmd.Output()
	if err != nil {
		var exit *exec.ExitError
		if errors.As(err, &exit) {
			if detail := strings.TrimSpace(string(exit.Stderr)); detail != "" {
				return "", fmt.Errorf("%s: %s", label, detail)
			}
		}
		return "", fmt.Errorf("%s: %w", label, err)
	}
	return string(out), nil
}

// UpgradeRuntime installs the latest Herdr through the CLI's own updater and
// returns only when the runtime can actually leave the old protocol behind.
// `herdr update --handoff` hands the running server to the new build live
// when it can; when an old server survives the handoff it is stopped through
// Herdr's own `server stop`, so the next bootstrap start brings up the
// upgraded binary instead of pinning the app on the bootstrap page. session
// selects which local server gets verified ("" = default).
func UpgradeRuntime(ctx context.Context, session string) error {
	if _, err := RunCLI(ctx, []string{"update", "--handoff"}); err != nil {
		if !updateNotWritable(err) {
			return err
		}
		// A system-location install: the only missing ingredient is the
		// user's authorization. Ask through the OS's own mechanism — the
		// polkit dialog on Linux — and the granted permission finishes the
		// very same updater in place. Only a dismissed or failed
		// authorization falls back to the manual way out.
		if authErr := RunAuthorizedUpdate(ctx); authErr != nil {
			return fmt.Errorf("%w: %w", ErrUpdateNotWritable, authErr)
		}
	}
	result, err := pingRPC(SocketPath(normalizeSession(session)))
	if err == nil && result.Protocol < MinSupportedProtocol {
		// A new binary is on disk but the old server kept the socket. Herdr's
		// own stop command is the only supported way down; the next
		// EnsureRunning then starts the upgraded server fresh.
		if _, err := runCLIEnv(ctx, []string{"server", "stop"}, sessionEnv(session)); err != nil {
			return fmt.Errorf("Herdr updated but the old server kept the socket and could not be stopped: %w", err)
		}
	}
	// No server answering, or it already speaks a supported protocol.
	return nil
}

// RuntimeProbe is one live ping result: the runtime identity without any
// compatibility gate, for surfaces that present it in user language.
type RuntimeProbe struct {
	Version  string
	Protocol int
}

// ProbeRuntime pings one local session's socket ("" = default) and reports
// the live runtime identity. Unreachable servers surface as errors;
// old-but-running ones still return their identity so the caller can offer
// the update. Remote machines are not probeable from here — their runtime
// lives behind the SSH bridge of an active instance.
func ProbeRuntime(session string) (RuntimeProbe, error) {
	result, err := pingRPC(SocketPath(normalizeSession(session)))
	if err != nil {
		return RuntimeProbe{}, err
	}
	return RuntimeProbe{Version: result.Version, Protocol: result.Protocol}, nil
}

// sessionEnv scopes the bare `herdr server` commands to one session the same
// way EnsureRunning starts them: HERDR_SESSION for anything but the default.
func sessionEnv(session string) []string {
	if normalizeSession(session) == defaultSession {
		return nil
	}
	return []string{"HERDR_SESSION=" + normalizeSession(session)}
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
		for _, dir := range []string{"/opt/homebrew/bin", "/usr/local/bin", filepath.Join(os.Getenv("HOME"), ".local/bin")} {
			path := filepath.Join(dir, "herdr")
			if info, err := os.Stat(path); err == nil && !info.IsDir() {
				return path, nil
			}
		}
		// LaunchServices has a minimal PATH. Probe known locations before
		// spawning a login shell; the latter may execute user dotfiles.
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		if out, err := exec.CommandContext(ctx, "/bin/zsh", "-lic", "command -v herdr").Output(); err == nil {
			if path := strings.TrimSpace(string(out)); path != "" {
				return path, nil
			}
		}
	}
	return "", ErrCLINotFound
}
