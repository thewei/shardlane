// Package applog configures bounded structured application logging.
//
// [INPUT]: MyGo's per-app logs directory and SHARDLANE_LOG_LEVEL.
// [OUTPUT]: process-wide slog logger with size-bounded rotating JSON files.
// [POS]: infrastructure only; no product/runtime state lives here.
package applog

import (
	"errors"
	"fmt"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"sync"
)

const (
	DefaultMaxBytes = int64(2 * 1024 * 1024)
	DefaultBackups  = 3
	FileName        = "shardlane.log"
)

var (
	mu      sync.RWMutex
	current io.Closer
	path    string
)

// Setup installs the process logger. At the defaults the log directory is
// bounded to roughly 8 MiB: one 2 MiB active file plus three backups.
func Setup(dir string) (string, error) {
	if strings.TrimSpace(dir) == "" {
		return "", errors.New("log directory is empty")
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return "", fmt.Errorf("create log directory: %w", err)
	}

	writer, err := NewRollingFile(filepath.Join(dir, FileName), DefaultMaxBytes, DefaultBackups)
	if err != nil {
		return "", err
	}

	level := &slog.LevelVar{}
	level.Set(levelFromEnv(os.Getenv("SHARDLANE_LOG_LEVEL")))
	handler := slog.NewJSONHandler(writer, &slog.HandlerOptions{
		Level:     level,
		AddSource: level.Level() == slog.LevelDebug,
	})
	logger := slog.New(handler)
	slog.SetDefault(logger)

	mu.Lock()
	old := current
	current = writer
	path = writer.Path()
	mu.Unlock()
	if old != nil {
		_ = old.Close()
	}

	logger.Info("logging initialized",
		"path", writer.Path(),
		"max_bytes", DefaultMaxBytes,
		"backups", DefaultBackups,
		"configured_level", level.Level().String(),
	)
	return writer.Path(), nil
}

// Path returns the active log file, when Setup succeeded.
func Path() string {
	mu.RLock()
	defer mu.RUnlock()
	return path
}

// Close flushes/closes the active file. It is safe to call more than once.
func Close() error {
	mu.Lock()
	closer := current
	current = nil
	mu.Unlock()
	if closer == nil {
		return nil
	}
	return closer.Close()
}

func levelFromEnv(value string) slog.Level {
	switch strings.ToLower(strings.TrimSpace(value)) {
	case "debug":
		return slog.LevelDebug
	case "warn", "warning":
		return slog.LevelWarn
	case "error":
		return slog.LevelError
	default:
		return slog.LevelInfo
	}
}
