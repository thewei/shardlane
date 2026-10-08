package applog

import (
	"fmt"
	"os"
	"path/filepath"
	"sync"
)

// RollingFile is a small dependency-free size-based rotating writer.
// It keeps path, path.1 ... path.N and never intentionally grows the active
// file past maxBytes. One pathological over-sized write is truncated.
type RollingFile struct {
	mu       sync.Mutex
	path     string
	maxBytes int64
	backups  int
	file     *os.File
	size     int64
}

func NewRollingFile(path string, maxBytes int64, backups int) (*RollingFile, error) {
	if maxBytes <= 0 {
		return nil, fmt.Errorf("maxBytes must be positive")
	}
	if backups < 0 {
		return nil, fmt.Errorf("backups must not be negative")
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return nil, fmt.Errorf("create log parent: %w", err)
	}
	w := &RollingFile{path: path, maxBytes: maxBytes, backups: backups}
	if err := w.open(); err != nil {
		return nil, err
	}
	return w, nil
}

func (w *RollingFile) Path() string { return w.path }

func (w *RollingFile) Write(p []byte) (int, error) {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.file == nil {
		return 0, os.ErrClosed
	}

	original := len(p)
	if int64(len(p)) > w.maxBytes {
		// slog normally writes small JSON lines. Bound a pathological message
		// rather than allowing one value to defeat the disk cap.
		p = p[:w.maxBytes]
		if len(p) > 0 {
			p[len(p)-1] = '\n'
		}
	}
	if w.size > 0 && w.size+int64(len(p)) > w.maxBytes {
		if err := w.rotate(); err != nil {
			return 0, err
		}
	}
	n, err := w.file.Write(p)
	w.size += int64(n)
	if err != nil {
		return n, err
	}
	// Report the caller's write as consumed even if a pathological message
	// was truncated to enforce the disk bound.
	return original, nil
}

func (w *RollingFile) Close() error {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.file == nil {
		return nil
	}
	err := w.file.Close()
	w.file = nil
	return err
}

func (w *RollingFile) open() error {
	file, err := os.OpenFile(w.path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o644)
	if err != nil {
		return fmt.Errorf("open log file: %w", err)
	}
	info, err := file.Stat()
	if err != nil {
		_ = file.Close()
		return fmt.Errorf("stat log file: %w", err)
	}
	w.file = file
	w.size = info.Size()
	if w.size > w.maxBytes {
		return w.rotate()
	}
	return nil
}

func (w *RollingFile) rotate() error {
	if w.file != nil {
		if err := w.file.Close(); err != nil {
			return err
		}
		w.file = nil
	}
	if w.backups > 0 {
		_ = os.Remove(fmt.Sprintf("%s.%d", w.path, w.backups))
		for index := w.backups - 1; index >= 1; index-- {
			from := fmt.Sprintf("%s.%d", w.path, index)
			to := fmt.Sprintf("%s.%d", w.path, index+1)
			if _, err := os.Stat(from); err == nil {
				_ = os.Rename(from, to)
			}
		}
		if _, err := os.Stat(w.path); err == nil {
			if err := os.Rename(w.path, w.path+".1"); err != nil {
				return fmt.Errorf("rotate active log: %w", err)
			}
		}
	} else {
		_ = os.Remove(w.path)
	}
	w.size = 0
	return w.open()
}
