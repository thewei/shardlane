package herdr

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

// Saved SSH machines are Herdr's own remote-connection machinery: one profile
// pins one device (SSH target) to one explicit remote Herdr session. This
// file only drives the installed `herdr machine` CLI — profiles are stored in
// Herdr's client state and are never mirrored into a Shardlane-side registry.

// Machine is one saved Herdr SSH machine profile. ID is Herdr's opaque
// profile id and is the only handle this client ever targets (`--machine
// <label-or-id>`); labels are display-only and may be non-unique.
type Machine struct {
	ID      string `json:"id"`
	Label   string `json:"label"`
	Target  string `json:"target"`
	Session string `json:"session"`
	Enabled bool   `json:"enabled"`
}

// MachineState is the latest `herdr machine status` verdict for one profile.
// Reachable means the remote Herdr server answered during that fresh check,
// not that any open client is connected. Attention means the check failed in
// a way that needs user action (auth, changed host key, unreachable host).
type MachineState struct {
	Reachable bool
	Attention bool
	Message   string
}

// ListMachines returns the saved SSH machine profiles from the Herdr CLI.
// The read is local (no SSH), so it is cheap enough for menu builds.
func (m *Manager) ListMachines() ([]Machine, error) {
	cli, err := resolveCLI()
	if err != nil {
		return nil, err
	}
	cmd := exec.Command(cli, "machine", "list", "--json")
	cmd.Stdin = nil
	out, err := cmd.Output()
	if err != nil {
		var exit *exec.ExitError
		if ok := asExitError(err, &exit); ok {
			return nil, fmt.Errorf("herdr machine list: %s", strings.TrimSpace(string(exit.Stderr)))
		}
		return nil, fmt.Errorf("herdr machine list: %w", err)
	}
	machines := decodeMachineList(out)
	m.machineMu.Lock()
	m.machineCache = machines
	m.machineCacheAt = time.Now()
	m.machineMu.Unlock()
	return machines, nil
}

// MachineStates runs one fresh non-interactive availability check for all
// saved machines and keys the verdicts by profile ID. The SSH round trip is
// slow (seconds); callers run it in the background and cache the result.
func (m *Manager) MachineStates() (map[string]MachineState, error) {
	cli, err := resolveCLI()
	if err != nil {
		return nil, err
	}
	cmd := exec.Command(cli, "machine", "status", "--json")
	cmd.Stdin = nil
	out, err := cmd.Output()
	if err != nil {
		var exit *exec.ExitError
		if ok := asExitError(err, &exit); ok {
			return nil, fmt.Errorf("herdr machine status: %s", strings.TrimSpace(string(exit.Stderr)))
		}
		return nil, fmt.Errorf("herdr machine status: %w", err)
	}
	return decodeMachineStates(out), nil
}

// RemoveMachine forgets one saved profile. Only that machine disconnects from
// the client; the remote session and its panes keep running.
func (m *Manager) RemoveMachine(id string) error {
	return m.runMachineCommand("remove", id)
}

// RenameMachine changes a profile's display label without reconnecting.
func (m *Manager) RenameMachine(id, label string) error {
	label = strings.TrimSpace(label)
	if label == "" {
		return fmt.Errorf("machine name is required")
	}
	return m.runMachineCommand("rename", id, "--label", label)
}

func (m *Manager) runMachineCommand(command string, args ...string) error {
	cli, err := resolveCLI()
	if err != nil {
		return err
	}
	argv := append([]string{"machine", command}, args...)
	cmd := exec.Command(cli, argv...)
	cmd.Env = runtimeEnv("")
	output, err := cmd.CombinedOutput()
	if err != nil {
		message := strings.TrimSpace(string(output))
		if message == "" {
			message = err.Error()
		}
		return fmt.Errorf("herdr machine %s: %s", command, message)
	}
	return nil
}

// OpenMachineAddInTerminal hands `herdr machine add` to the user's terminal:
// setup discovers remote sessions, may install/update the remote server, and
// always asks before replacing anything, so Herdr documents it as an
// interactive command ("non-interactive commands use default"). Shardlane
// opens Terminal.app running the exact command and reports back; the profile
// list refreshes when the user reopens the Session menu.
func (m *Manager) OpenMachineAddInTerminal(target string) error {
	target = strings.TrimSpace(target)
	if target == "" {
		return fmt.Errorf("SSH target is required")
	}
	cli, err := resolveCLI()
	if err != nil {
		return err
	}
	if runtime.GOOS != "darwin" {
		return fmt.Errorf("opening a terminal is only supported on macOS; run: %s machine add %s", cli, target)
	}
	script := fmt.Sprintf("#!/bin/zsh\nprintf 'Adding remote machine to Herdr...\\n\\n'\n%s machine add %s\nprintf '\\nSetup finished — you can close this window.\\n'\n", shellQuote(cli), shellQuote(target))
	path := filepath.Join(os.TempDir(), fmt.Sprintf("shardlane-machine-add-%d.command", os.Getpid()))
	if err := os.WriteFile(path, []byte(script), 0o700); err != nil {
		return fmt.Errorf("write machine add script: %w", err)
	}
	if out, err := exec.Command("open", "-a", "Terminal", path).CombinedOutput(); err != nil {
		return fmt.Errorf("open Terminal: %s", strings.TrimSpace(string(out)))
	}
	return nil
}

// ListAllInstances enumerates the local Herdr sessions plus one instance per
// saved machine profile. Machine entries never fail the enumeration: if the
// machine CLI read fails, the local list still comes back.
func (m *Manager) ListAllInstances() ([]Instance, error) {
	instances, err := m.ListInstances()
	if err != nil {
		return nil, err
	}
	machines, err := m.ListMachines()
	if err != nil {
		return instances, nil
	}
	for _, machine := range machines {
		machine := machine
		display := machine.Label
		if display == "" {
			display = machine.Target
		}
		instances = append(instances, Instance{
			Name:        MachineInstanceKey(machine.ID),
			DisplayName: display,
			Machine:     &machine,
		})
	}
	return instances, nil
}

func (m *Manager) machineByID(id string) (Machine, error) {
	machineMuTTL := 5 * time.Second
	m.machineMu.Lock()
	cached := m.machineCache
	fresh := time.Since(m.machineCacheAt) < machineMuTTL
	m.machineMu.Unlock()
	if !fresh {
		machines, err := m.ListMachines()
		if err != nil {
			return Machine{}, err
		}
		cached = machines
		m.machineMu.Lock()
		m.machineCache = machines
		m.machineCacheAt = time.Now()
		m.machineMu.Unlock()
	}
	for _, machine := range cached {
		if machine.ID == id {
			return machine, nil
		}
	}
	return Machine{}, fmt.Errorf("remote machine profile %q is no longer saved in Herdr; re-add it from the Session menu", id)
}

// decodeMachineList parses `herdr machine list --json`. The exact wire shape
// is not a published contract, so the decoder accepts the plausible shapes
// (a bare array, or an object wrapping one under "machines"/"profiles") and
// tolerates alias field names. Missing enabled flags default to enabled.
func decodeMachineList(out []byte) []Machine {
	machines := make([]Machine, 0)
	for _, doc := range decodeMachineDocuments(out) {
		id := machineString(doc, "id", "profile_id", "machine_id")
		if id == "" {
			continue
		}
		machine := Machine{
			ID:      id,
			Label:   machineString(doc, "label", "name", "display_label"),
			Target:  machineString(doc, "target", "ssh_target", "host"),
			Session: machineString(doc, "session", "remote_session"),
			Enabled: true,
		}
		if enabled, ok := machineBool(doc, "enabled"); ok {
			machine.Enabled = enabled
		} else if disabled, ok := machineBool(doc, "disabled"); ok {
			machine.Enabled = !disabled
		}
		if machine.Label == "" {
			machine.Label = machine.Target
		}
		machines = append(machines, machine)
	}
	return machines
}

// decodeMachineStates parses `herdr machine status --json` into per-profile
// verdicts, applying the same lenient shape/alias handling as the list
// decoder. A document with no recognizable reachability fact decodes to the
// zero state (unknown), never to a false "unreachable".
func decodeMachineStates(out []byte) map[string]MachineState {
	states := make(map[string]MachineState)
	for _, doc := range decodeMachineDocuments(out) {
		id := machineString(doc, "id", "profile_id", "machine_id")
		if id == "" {
			continue
		}
		state := MachineState{Message: machineString(doc, "error", "message", "last_error", "detail")}
		if reachable, ok := machineBool(doc, "reachable", "ok", "online"); ok {
			state.Reachable = reachable
		}
		status := strings.ToLower(machineString(doc, "state", "status"))
		switch {
		case status != "":
			switch {
			case strings.Contains(status, "reach") || strings.Contains(status, "ok"):
				state.Reachable = true
			case strings.Contains(status, "attention") || strings.Contains(status, "auth") ||
				strings.Contains(status, "error") || strings.Contains(status, "unavailable"):
				state.Attention = true
			}
		case state.Message != "":
			state.Attention = true
		}
		states[id] = state
	}
	return states
}

// decodeMachineDocuments returns the per-machine JSON objects from a machine
// CLI response: either a top-level array or an object wrapping the array in
// "machines" or "profiles". Anything else decodes to no documents.
func decodeMachineDocuments(out []byte) []map[string]any {
	var value any
	if err := json.Unmarshal(out, &value); err != nil {
		return nil
	}
	docs, ok := value.([]any)
	if !ok {
		object, ok := value.(map[string]any)
		if !ok {
			return nil
		}
		for _, key := range []string{"machines", "profiles"} {
			wrapped, ok := object[key].([]any)
			if ok {
				docs = wrapped
				break
			}
		}
		if docs == nil {
			return nil
		}
	}
	result := make([]map[string]any, 0, len(docs))
	for _, doc := range docs {
		if object, ok := doc.(map[string]any); ok {
			result = append(result, object)
		}
	}
	return result
}

func machineString(doc map[string]any, keys ...string) string {
	for _, key := range keys {
		if text, ok := doc[key].(string); ok && strings.TrimSpace(text) != "" {
			return strings.TrimSpace(text)
		}
	}
	return ""
}

func machineBool(doc map[string]any, keys ...string) (bool, bool) {
	for _, key := range keys {
		if value, ok := doc[key].(bool); ok {
			return value, true
		}
	}
	return false, false
}

// shellQuote single-quotes a value for the generated zsh script.
func shellQuote(value string) string {
	return "'" + strings.ReplaceAll(value, "'", `'\''`) + "'"
}

// asExitError is errors.As restricted to *exec.ExitError, kept as a tiny
// helper so callers stay flat.
func asExitError(err error, target **exec.ExitError) bool {
	for err != nil {
		if exit, ok := err.(*exec.ExitError); ok {
			*target = exit
			return true
		}
		unwrapper, ok := err.(interface{ Unwrap() error })
		if !ok {
			return false
		}
		err = unwrapper.Unwrap()
	}
	return false
}

// guardLocalInstance rejects instance-level operations that only make sense
// on this machine's Herdr CLI. Remote instances are pinned to their machine
// profile; their session lifecycle is managed on the remote device.
func guardLocalInstance(key, action string) error {
	if ParseRef(key).IsRemote() {
		return fmt.Errorf("%s works on this machine's sessions only; the remote session is managed by its machine profile", action)
	}
	return nil
}
