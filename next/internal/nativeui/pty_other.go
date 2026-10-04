//go:build !darwin

package nativeui

import (
	"errors"
	"os"
)

// Shardlane is the macOS client; other platforms have no attach pty and
// the terminal falls back to the plugin's own unfiltered spawn.
func openPty(cols, rows uint16) (*os.File, string, error) {
	return nil, "", errors.New("attach pty unsupported on this platform")
}

func startPtyProcess(master *os.File, slavePath, path string, args, env []string, cols, rows int) (*os.Process, error) {
	return nil, errors.New("attach pty unsupported on this platform")
}

func setPtySize(fd uintptr, cols, rows int) error {
	return nil
}
