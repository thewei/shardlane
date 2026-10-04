package settings

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
)

const FileName = "settings.json"

type FileStore struct {
	path string
}

func NewFileStore(dir string) *FileStore {
	return &FileStore{path: filepath.Join(dir, FileName)}
}

func NewFileStoreAt(path string) *FileStore {
	return &FileStore{path: path}
}

func (s *FileStore) Path() string { return s.path }

func (s *FileStore) Load(ctx context.Context) (Settings, error) {
	if err := ctx.Err(); err != nil {
		return Settings{}, err
	}
	data, err := os.ReadFile(s.path)
	if errors.Is(err, os.ErrNotExist) {
		return Default(), nil
	}
	if err != nil {
		return Default(), fmt.Errorf("read settings: %w", err)
	}

	value := Default()
	if err := json.Unmarshal(data, &value); err != nil {
		return Default(), fmt.Errorf("decode settings: %w", err)
	}
	if value.SchemaVersion == 0 {
		value.SchemaVersion = CurrentSchemaVersion
	}
	if value.SchemaVersion > CurrentSchemaVersion {
		return Default(), fmt.Errorf("settings schema %d is newer than supported %d", value.SchemaVersion, CurrentSchemaVersion)
	}
	return value, nil
}

func (s *FileStore) Save(ctx context.Context, value Settings) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if value.SchemaVersion == 0 {
		value.SchemaVersion = CurrentSchemaVersion
	}
	if value.SchemaVersion != CurrentSchemaVersion {
		return fmt.Errorf("cannot save settings schema %d; expected %d", value.SchemaVersion, CurrentSchemaVersion)
	}
	if err := os.MkdirAll(filepath.Dir(s.path), 0o755); err != nil {
		return fmt.Errorf("create settings directory: %w", err)
	}

	data, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return fmt.Errorf("encode settings: %w", err)
	}
	data = append(data, '\n')

	temp, err := os.CreateTemp(filepath.Dir(s.path), ".settings-*.tmp")
	if err != nil {
		return fmt.Errorf("create settings temp file: %w", err)
	}
	tempPath := temp.Name()
	cleanup := func() {
		_ = temp.Close()
		_ = os.Remove(tempPath)
	}
	if err := temp.Chmod(0o600); err != nil {
		cleanup()
		return fmt.Errorf("chmod settings temp file: %w", err)
	}
	if _, err := temp.Write(data); err != nil {
		cleanup()
		return fmt.Errorf("write settings temp file: %w", err)
	}
	if err := temp.Sync(); err != nil {
		cleanup()
		return fmt.Errorf("sync settings temp file: %w", err)
	}
	if err := temp.Close(); err != nil {
		_ = os.Remove(tempPath)
		return fmt.Errorf("close settings temp file: %w", err)
	}
	if err := ctx.Err(); err != nil {
		_ = os.Remove(tempPath)
		return err
	}
	if err := os.Rename(tempPath, s.path); err != nil {
		_ = os.Remove(tempPath)
		return fmt.Errorf("replace settings file: %w", err)
	}
	return nil
}
