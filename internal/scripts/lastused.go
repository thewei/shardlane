// lastused.go persists the command a project ran last through the pane
// header's run control. Shardlane-owned cosmetics in the same class as the
// script definitions store: atomic JSON under the user-data directory, no
// runtime facts.
package scripts

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sync"
)

// LastUsedStore keeps one last-run command name per project root.
type LastUsedStore struct {
	mu   sync.Mutex
	path string
	m    map[string]string
}

// NewLastUsedStore opens or creates the last-used store at the given file
// path. An empty path keeps the store in memory (headless tests).
func NewLastUsedStore(path string) (*LastUsedStore, error) {
	s := &LastUsedStore{path: path, m: make(map[string]string)}
	if path == "" {
		return s, nil
	}
	data, err := os.ReadFile(path)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return s, nil
		}
		return nil, err
	}
	if err := json.Unmarshal(data, &s.m); err != nil {
		return nil, err
	}
	if s.m == nil {
		s.m = make(map[string]string)
	}
	return s, nil
}

// Get returns the last-run command name for root, or "".
func (s *LastUsedStore) Get(root string) string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.m[root]
}

// Set records the last-run command name for root, persisting atomically.
func (s *LastUsedStore) Set(root, name string) error {
	if root == "" || name == "" {
		return errors.New("root and name are required")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.m[root] = name
	return s.save()
}

func (s *LastUsedStore) save() error {
	if s.path == "" {
		return nil
	}
	data, err := json.MarshalIndent(s.m, "", "  ")
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
