package applog

import (
	"fmt"
	"os"
	"path/filepath"
	"syscall"
)

// CaptureStderr redirects the process stderr (file descriptor 2) into a
// bounded file next to the main log, so runtime panics and native aborts —
// which the Go runtime writes straight to fd 2, bypassing slog — leave a
// diagnosable trace instead of disappearing into /dev/null. The redirect is
// a plain dup2 onto an append-only file on purpose: a pipe tee would lose
// the final panic write when the process exits mid-drain.
//
// The file is bounded across launches: when it already exceeds the active
// log budget at startup it is rotated once (path → path.1). A single launch
// only ever appends what actually crashes, so no unbounded growth is
// possible without repeated crashes, and each launch re-bounds the file.
// The console stream is intentionally not preserved; packaged macOS runs
// have no console stderr, and interactive runs keep the structured log.
func CaptureStderr(dir string) (stop func(), err error) {
	if dir == "" {
		return nil, fmt.Errorf("stderr capture directory is empty")
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, fmt.Errorf("create stderr capture directory: %w", err)
	}
	path := filepath.Join(dir, FileName+".stderr")
	if info, statErr := os.Stat(path); statErr == nil && info.Size() > DefaultMaxBytes {
		_ = os.Remove(path + ".1")
		_ = os.Rename(path, path+".1")
	}
	file, err := os.OpenFile(path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o644)
	if err != nil {
		return nil, fmt.Errorf("open stderr capture file: %w", err)
	}
	saved, err := syscall.Dup(2)
	if err != nil {
		_ = file.Close()
		return nil, fmt.Errorf("save stderr: %w", err)
	}
	if err := syscall.Dup2(int(file.Fd()), 2); err != nil {
		_ = syscall.Close(saved)
		_ = file.Close()
		return nil, fmt.Errorf("redirect stderr: %w", err)
	}
	return func() {
		_ = syscall.Dup2(saved, 2)
		_ = syscall.Close(saved)
		_ = file.Close()
	}, nil
}
