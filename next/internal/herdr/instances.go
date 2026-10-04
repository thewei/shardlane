package herdr

import (
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

// CreateInstance mirrors the current Shardlane Workspace-creation semantics:
// derive a filesystem-safe unique Herdr session name, start that named session,
// then persist the exact user-facing display name beside Herdr's own session data.
func (m *Manager) CreateInstance(displayName string) (Instance, error) {
	displayName = strings.TrimSpace(displayName)
	if displayName == "" {
		return Instance{}, fmt.Errorf("workspace name is required")
	}
	existing, err := m.ListInstances()
	if err != nil {
		return Instance{}, err
	}
	base := slugifyInstanceName(displayName)
	if base == "" {
		base = "workspace"
	}
	candidate := base
	suffix := len(existing) + 1
	for containsInstance(existing, candidate) {
		candidate = fmt.Sprintf("%s-%d", base, suffix)
		suffix++
	}
	if err := m.EnsureRunning(candidate); err != nil {
		return Instance{}, err
	}
	if err := writeDisplayName(candidate, displayName); err != nil {
		return Instance{}, err
	}
	return Instance{Name: candidate, DisplayName: displayName, Running: true}, nil
}

func (m *Manager) RenameInstance(session, displayName string) (Instance, error) {
	if err := guardLocalInstance(session, "renaming a session"); err != nil {
		return Instance{}, err
	}
	session = normalizeSession(session)
	displayName = strings.TrimSpace(displayName)
	if displayName == "" {
		return Instance{}, fmt.Errorf("workspace name is required")
	}
	if err := writeDisplayName(session, displayName); err != nil {
		return Instance{}, err
	}
	instances, err := m.ListInstances()
	if err != nil {
		return Instance{}, err
	}
	for _, instance := range instances {
		if instance.Name == session {
			instance.DisplayName = displayName
			return instance, nil
		}
	}
	return Instance{}, fmt.Errorf("Herdr workspace %q disappeared after rename", session)
}

func (m *Manager) DeleteInstance(session string) error {
	if err := guardLocalInstance(session, "deleting a session"); err != nil {
		return err
	}
	session = normalizeSession(session)
	if session == defaultSession {
		return fmt.Errorf("the default Herdr workspace cannot be deleted")
	}
	cli, err := resolveCLI()
	if err != nil {
		return err
	}
	if err := runSessionCommand(cli, "stop", session); err != nil {
		return err
	}
	// Herdr delete accepts only stopped sessions. Wait for the session socket
	// to disappear instead of relying on a fixed sleep used by the old UI.
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		if _, err := os.Stat(SocketPath(session)); os.IsNotExist(err) {
			break
		}
		time.Sleep(50 * time.Millisecond)
	}
	if err := runSessionCommand(cli, "delete", session); err != nil {
		return err
	}
	return nil
}

func runSessionCommand(cli, command, session string) error {
	cmd := exec.Command(cli, "session", command, session, "--json")
	cmd.Env = runtimeEnv("")
	output, err := cmd.CombinedOutput()
	if err != nil {
		message := strings.TrimSpace(string(output))
		if message == "" {
			message = err.Error()
		}
		return fmt.Errorf("herdr session %s %s: %s", command, session, message)
	}
	return nil
}

func containsInstance(instances []Instance, name string) bool {
	for _, instance := range instances {
		if instance.Name == name {
			return true
		}
	}
	return false
}

func slugifyInstanceName(name string) string {
	var builder strings.Builder
	for _, char := range strings.ToLower(strings.TrimSpace(name)) {
		if (char >= 'a' && char <= 'z') || (char >= '0' && char <= '9') {
			builder.WriteRune(char)
		} else {
			builder.WriteByte('-')
		}
	}
	return strings.Trim(builder.String(), "-")
}

func writeDisplayName(session, displayName string) error {
	dir := sessionDir(session)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return fmt.Errorf("create Herdr session metadata dir: %w", err)
	}
	body, err := json.MarshalIndent(map[string]any{
		"version":      1,
		"display_name": strings.TrimSpace(displayName),
	}, "", "  ")
	if err != nil {
		return err
	}
	body = append(body, '\n')
	tmp, err := os.CreateTemp(dir, ".workspace-*.json.tmp")
	if err != nil {
		return err
	}
	tmpPath := tmp.Name()
	defer func() { _ = os.Remove(tmpPath) }()
	if _, err := tmp.Write(body); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Sync(); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	if err := os.Rename(tmpPath, filepath.Join(dir, "workspace.json")); err != nil {
		return fmt.Errorf("install Herdr workspace metadata: %w", err)
	}
	return nil
}
