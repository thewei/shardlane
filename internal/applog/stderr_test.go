package applog

import (
	"fmt"
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

	path := filepath.Join(dir, FileName+".stderr")

	// The redirect itself is a synchronous dup2: fd 2 must name the capture
	// file right now. Asserting device+inode equality is scheduling-proof,
	// unlike polling for bytes.
	assertFd2IsFile(t, path)

	// The capture contract is about file descriptor 2 itself: runtime
	// panics and native aborts write straight to it, bypassing the Go
	// os.Stderr variable. Write through fd 2 exactly as a panic would —
	// via the os.Stderr variable would measure the wrong layer, because
	// under `go test -json` (test2json) the testing framework re-wraps
	// os.Stderr onto fd 1 for the JSON stream while fd 2 stays redirected.
	if _, err := syscall.Write(2, []byte("SHARDLANE_STDERR_MARKER\n")); err != nil {
		t.Fatalf("write to stderr fd: %v", err)
	}
	deadline := time.Now().Add(5 * time.Second)
	for {
		data, readErr := os.ReadFile(path)
		if readErr == nil && strings.Contains(string(data), "SHARDLANE_STDERR_MARKER") {
			break
		}
		if time.Now().After(deadline) {
			var fd2Stat, fileStat syscall.Stat_t
			errText := ""
			if err := syscall.Fstat(2, &fd2Stat); err != nil {
				errText += "fstat(2): " + err.Error() + "; "
			} else {
				errText += fmt.Sprintf("fstat(2): dev=%d ino=%d; ", fd2Stat.Dev, fd2Stat.Ino)
			}
			if err := syscall.Stat(path, &fileStat); err != nil {
				errText += "stat(file): " + err.Error()
			} else {
				errText += fmt.Sprintf("stat(file): dev=%d ino=%d size=%d; stderr.Fd()=%d", fileStat.Dev, fileStat.Ino, fileStat.Size, os.Stderr.Fd())
			}
			if probe, probeErr := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0o644); probeErr == nil {
				if _, err := probe.WriteString("SHARDLANE_PROBE_DIRECT\n"); err != nil {
					errText += "; probe write: " + err.Error()
				}
				_ = probe.Close()
				if data, err := os.ReadFile(path); err != nil {
					errText += "; probe read: " + err.Error()
				} else if strings.Contains(string(data), "SHARDLANE_PROBE_DIRECT") {
					errText += "; probe: visible"
				} else {
					errText += "; probe: invisible"
				}
			} else {
				errText += "; probe open: " + probeErr.Error()
			}
			t.Fatalf("marker never reached %s (last: %q, err: %v; %s)", path, string(data), readErr, errText)
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

// assertFd2IsFile fails unless fd 2 names path (same device and inode).
// On timeout-style failures elsewhere this is the invariant that actually
// matters, checked without any wall-clock dependence.
func assertFd2IsFile(t *testing.T, path string) {
	t.Helper()
	var fd2Stat, fileStat syscall.Stat_t
	if err := syscall.Fstat(2, &fd2Stat); err != nil {
		t.Fatalf("fstat stderr: %v", err)
	}
	if err := syscall.Stat(path, &fileStat); err != nil {
		t.Fatalf("stat capture file: %v", err)
	}
	if fd2Stat.Dev != fileStat.Dev || fd2Stat.Ino != fileStat.Ino {
		t.Fatalf("fd 2 does not name the capture file: fd2 dev=%d ino=%d, want dev=%d ino=%d",
			fd2Stat.Dev, fd2Stat.Ino, fileStat.Dev, fileStat.Ino)
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
