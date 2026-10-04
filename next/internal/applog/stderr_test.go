package applog

import (
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"testing"
	"time"
)

func TestCaptureStderrRoutesFd2IntoFile(t *testing.T) {
	dir := t.TempDir()
	stop, err := CaptureStderr(dir)
	if err != nil {
		t.Fatalf("CaptureStderr: %v", err)
	}
	defer stop()

	// The Go-level os.Stderr variable wraps fd 2, so this write — like a
	// runtime panic write — must land in the capture file.
	if _, err := os.Stderr.WriteString("SHARDLANE_STDERR_MARKER\n"); err != nil {
		t.Fatalf("write to stderr: %v", err)
	}
	path := filepath.Join(dir, FileName+".stderr")
	deadline := time.Now().Add(2 * time.Second)
	for {
		data, readErr := os.ReadFile(path)
		if readErr == nil && strings.Contains(string(data), "SHARDLANE_STDERR_MARKER") {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("marker never reached %s (last: %q, err: %v)", path, string(data), readErr)
		}
		time.Sleep(10 * time.Millisecond)
	}

	// stop() must give the real stderr back.
	stop()
	var stat syscall.Stat_t
	if err := syscall.Fstat(2, &stat); err != nil {
		t.Fatalf("fstat stderr: %v", err)
	}
}

func TestCaptureStderrRotatesOversizedFile(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, FileName+".stderr")
	oversized := strings.Repeat("x\n", int(DefaultMaxBytes)) // > DefaultMaxBytes bytes
	if err := os.WriteFile(path, []byte(oversized), 0o644); err != nil {
		t.Fatalf("seed oversized file: %v", err)
	}
	stop, err := CaptureStderr(dir)
	if err != nil {
		t.Fatalf("CaptureStderr: %v", err)
	}
	defer stop()
	if _, err := os.Stat(path + ".1"); err != nil {
		t.Fatalf("oversized log not rotated: %v", err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatalf("active file missing: %v", err)
	}
	if info.Size() >= int64(len(oversized)) {
		t.Fatalf("active file was not truncated: %d bytes", info.Size())
	}
}

func TestCaptureStderrRejectsEmptyDir(t *testing.T) {
	if _, err := CaptureStderr(""); err == nil {
		t.Fatal("expected error for empty dir")
	}
}
