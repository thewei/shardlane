//go:build darwin

package nativeui

import (
	"errors"
	"fmt"
	"os"
	"strings"
	"syscall"
	"unsafe"
)

// openPty allocates a macOS pseudo-terminal pair. Shardlane is a macOS
// product; the terminal plugin's own pty is internal, so the attach
// transport grows the minimal darwin implementation it needs.
func openPty(cols, rows uint16) (*os.File, string, error) {
	if cols == 0 {
		cols = 80
	}
	if rows == 0 {
		rows = 24
	}
	master, err := os.OpenFile("/dev/ptmx", os.O_RDWR|syscall.O_NOCTTY, 0)
	if err != nil {
		return nil, "", fmt.Errorf("open /dev/ptmx: %w", err)
	}
	masterFd := master.Fd()
	if _, _, errno := syscall.Syscall(syscall.SYS_IOCTL, masterFd,
		uintptr(syscall.TIOCPTYGRANT), 0); errno != 0 {
		master.Close()
		return nil, "", fmt.Errorf("grantpt: %w", errno)
	}
	if _, _, errno := syscall.Syscall(syscall.SYS_IOCTL, masterFd,
		uintptr(syscall.TIOCPTYUNLK), 0); errno != 0 {
		master.Close()
		return nil, "", fmt.Errorf("unlockpt: %w", errno)
	}
	var name [128]byte
	if _, _, errno := syscall.Syscall(syscall.SYS_IOCTL, masterFd,
		uintptr(syscall.TIOCPTYGNAME), uintptr(unsafe.Pointer(&name[0]))); errno != 0 {
		master.Close()
		return nil, "", fmt.Errorf("ptsname: %w", errno)
	}
	slavePath := strings.TrimRight(string(name[:]), "\x00")
	if slavePath == "" {
		master.Close()
		return nil, "", errors.New("ptsname returned an empty path")
	}
	return master, slavePath, nil
}

// startPtyProcess runs path on the pty's slave as a session leader with
// it as the controlling terminal, so resize ioctls deliver SIGWINCH. The
// initial grid is set on the slave: the darwin master side rejects
// TIOCSWINSZ (ENOTTY), the tty line discipline lives on the slave.
func startPtyProcess(master *os.File, slavePath, path string, args, env []string, cols, rows int) (*os.Process, error) {
	slave, err := syscall.Open(slavePath, syscall.O_RDWR|syscall.O_NOCTTY, 0)
	if err != nil {
		return nil, fmt.Errorf("open %s: %w", slavePath, err)
	}
	if err := setPtySize(uintptr(slave), cols, rows); err != nil {
		syscall.Close(slave)
		return nil, err
	}
	slaveFile := os.NewFile(uintptr(slave), slavePath)
	attr := &os.ProcAttr{
		Files: []*os.File{slaveFile, slaveFile, slaveFile},
		Sys: &syscall.SysProcAttr{
			Setsid:  true, // new session, making the slave its ctty
			Setctty: true,
			Ctty:    0,
		},
	}
	proc, err := os.StartProcess(path, args, attr)
	// The child holds its own descriptors; ours are done either way.
	slaveFile.Close()
	if err != nil {
		return nil, err
	}
	return proc, nil
}

type winsize struct {
	rows, cols, xpixel, ypixel uint16
}

func setPtySize(fd uintptr, cols, rows int) error {
	if cols <= 0 || rows <= 0 {
		return nil
	}
	ws := winsize{rows: uint16(rows), cols: uint16(cols)}
	if _, _, errno := syscall.Syscall(syscall.SYS_IOCTL, fd,
		uintptr(syscall.TIOCSWINSZ), uintptr(unsafe.Pointer(&ws))); errno != 0 {
		return fmt.Errorf("tiocswinsz: %w", errno)
	}
	return nil
}
