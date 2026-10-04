// Package scripts owns user-defined project script definitions and atomic
// persistence (0.9 P5 / WIX-070..085). Shardlane owns script metadata; Herdr
// owns all execution (Tab, Pane, PTY, process). Scripts never become
// standalone os/exec children managed by Shardlane.
package scripts

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"
)

// ScriptStatus classifies the runtime state of a script execution.
type ScriptStatus string

const (
	StatusStopped  ScriptStatus = "stopped"
	StatusStarting ScriptStatus = "starting"
	StatusRunning  ScriptStatus = "running"
	StatusFailed   ScriptStatus = "failed"
)

// ScriptDefinition captures user-authored commands and execution policy.
type ScriptDefinition struct {
	ID          string `json:"id"`
	ProjectPath string `json:"project_path"`
	Name        string `json:"name"`
	Command     string `json:"command"`
	OneShot     bool   `json:"one_shot"`
	Icon        string `json:"icon,omitempty"`
}

// ScriptRecord joins a definition with its active execution state.
type ScriptRecord struct {
	Definition  ScriptDefinition `json:"definition"`
	Status      ScriptStatus     `json:"status"`
	WorkspaceID string           `json:"workspace_id,omitempty"`
	PaneID      string           `json:"pane_id,omitempty"`
	PID         int              `json:"pid,omitempty"`
	Ports       []uint16         `json:"ports,omitempty"`
}

// Store provides atomic JSON persistence for script definitions.
type Store struct {
	mu   sync.Mutex
	path string
	list []ScriptDefinition
}

// NewStore opens or creates a scripts store at the given file path.
func NewStore(path string) (*Store, error) {
	s := &Store{path: path}
	if err := s.load(); err != nil && !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	return s, nil
}

func (s *Store) load() error {
	data, err := os.ReadFile(s.path)
	if err != nil {
		return err
	}
	var list []ScriptDefinition
	if err := json.Unmarshal(data, &list); err != nil {
		return err
	}
	s.list = list
	return nil
}

func (s *Store) save() error {
	if s.path == "" {
		return nil
	}
	data, err := json.MarshalIndent(s.list, "", "  ")
	if err != nil {
		return err
	}
	tmp := s.path + ".tmp"
	if err := os.MkdirAll(filepath.Dir(s.path), 0o755); err != nil {
		return err
	}
	if err := os.WriteFile(tmp, data, 0o600); err != nil {
		return err
	}
	return os.Rename(tmp, s.path)
}

// List returns all definitions scoped to a project path, or all if path is empty.
func (s *Store) List(projectPath string) []ScriptDefinition {
	s.mu.Lock()
	defer s.mu.Unlock()
	if projectPath == "" {
		out := make([]ScriptDefinition, len(s.list))
		copy(out, s.list)
		return out
	}
	var out []ScriptDefinition
	for _, script := range s.list {
		if script.ProjectPath == projectPath {
			out = append(out, script)
		}
	}
	return out
}

// Save inserts or updates a definition by ID, persisting atomically.
func (s *Store) Save(script ScriptDefinition) error {
	if script.ID == "" {
		return errors.New("script ID is required")
	}
	if script.Name == "" {
		return errors.New("script name is required")
	}
	if script.Command == "" {
		return errors.New("script command is required")
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	found := false
	for i, existing := range s.list {
		if existing.ID == script.ID {
			s.list[i] = script
			found = true
			break
		}
	}
	if !found {
		s.list = append(s.list, script)
	}

	return s.save()
}

// Delete removes a script definition by ID, persisting atomically.
func (s *Store) Delete(id string) error {
	s.mu.Lock()
	defer s.mu.Unlock()

	idx := -1
	for i, existing := range s.list {
		if existing.ID == id {
			idx = i
			break
		}
	}
	if idx == -1 {
		return fmt.Errorf("script not found: %s", id)
	}

	s.list = append(s.list[:idx], s.list[idx+1:]...)
	return s.save()
}

// Get finds a script definition by ID.
func (s *Store) Get(id string) (ScriptDefinition, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, script := range s.list {
		if script.ID == id {
			return script, true
		}
	}
	return ScriptDefinition{}, false
}
