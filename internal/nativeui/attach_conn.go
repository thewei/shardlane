package nativeui

import (
	"bytes"
	"errors"
	"log/slog"
	"os"
	"os/exec"
	"sync"
	"syscall"
	"time"
)

/**
 * [INPUT]: 依赖 pty_darwin 的 openPty/startPtyProcess、attach_mouse 的 sgrMouseSplitter、Shell.routeAttachMouse 回调
 * [OUTPUT]: 对外提供 newAttachConn/attachConn（io.ReadWriteCloser + Resize）：`herdr terminal attach` 的 pty 连接，含 Write 侧 SGR 鼠标事件分流；conn 的 Read/Close 事件接入 attachFrameTiming（attach_timing.go）产出首帧/settle 分段计时
 * [POS]: nativeui 的 attach 传输层（F144 真终端改造）：Read 原样透传 daemon 渲染（tracking 下发不再剥离），Write 转发键入字节并把 SGR 鼠标事件交给 onMouse 路由
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// attachConn runs `herdr terminal attach` on a local pty and is the MyGo
// terminal's Conn, with real-terminal semantics (2026-10-06 F144,
// user-approved): what the daemon renders is passed through byte-exact —
// including its forced mouse-tracking sets, which are now legitimate
// because the view reports pointer events back. What is typed goes to the
// child unchanged, and SGR mouse sequences are split out of the write
// stream and handed to onMouse (the daemon's attach input drops mouse
// bytes; the client routes them itself — attach_mouse.go). Resize follows
// the view grid so the daemon sizes the pane to match.
type attachConn struct {
	proc      *os.Process
	master    *os.File
	slavePath string

	splitter sgrMouseSplitter
	onMouse  func(seq string)
	timing   *attachFrameTiming

	closeOnce sync.Once
	closeErr  error
}

// newAttachConn spawns the given command (an `herdr terminal attach …`
// argv) on a fresh pty with a terminal environment. instance/paneID/
// terminalID only label the timing logs (attach_timing.go).
func newAttachConn(command []string, instance, paneID, terminalID string) (*attachConn, error) {
	if len(command) == 0 {
		return nil, errors.New("attach command is empty")
	}
	started := time.Now()
	path, err := exec.LookPath(command[0])
	if err != nil {
		return nil, err
	}
	master, slavePath, err := openPty(80, 24)
	if err != nil {
		return nil, err
	}
	env := append(os.Environ(), "TERM=xterm-256color", "COLORTERM=truecolor")
	proc, err := startPtyProcess(master, slavePath, path, append([]string{command[0]}, command[1:]...), env, 80, 24)
	if err != nil {
		master.Close()
		return nil, err
	}
	conn := &attachConn{
		proc:      proc,
		master:    master,
		slavePath: slavePath,
		timing:    newAttachFrameTiming(instance, paneID, terminalID, time.Since(started).Milliseconds(), started),
	}
	slog.Debug("native attach conn started", "argv0", command[0], "pid", proc.Pid, "spawn_ms", time.Since(started).Milliseconds())
	return conn, nil
}

// Read returns daemon bytes as rendered — the renderer's forced
// mouse-tracking sets included: with real-terminal semantics the emulator
// keeps them so the view reports pointer events back through Write.
func (c *attachConn) Read(p []byte) (int, error) {
	n, err := c.master.Read(p)
	if n > 0 {
		c.timing.onRead(n, time.Now())
	}
	return n, err
}

// Write forwards typed bytes to the daemon untouched and hands complete
// SGR mouse sequences to onMouse instead (routeAttachMouse). Without a
// router wired, sequences fall through — the daemon drops them, exactly
// the pre-F144 behavior.
func (c *attachConn) Write(p []byte) (int, error) {
	plain, mouse := c.splitter.split(p)
	if len(plain) > 0 {
		if _, err := c.master.Write(plain); err != nil {
			return 0, err
		}
	}
	if len(mouse) > 0 {
		if c.onMouse != nil {
			for _, seq := range mouse {
				c.onMouse(string(seq))
			}
		} else if _, err := c.master.Write(bytes.Join(mouse, nil)); err != nil {
			return 0, err
		}
	}
	return len(p), nil
}

// Resize tells the daemon the view's grid. The winsize ioctl goes to the
// slave side (the darwin master rejects it); the kernel then SIGWINCHes
// the child's session.
func (c *attachConn) Resize(cols, rows int) error {
	if cols <= 0 || rows <= 0 {
		return nil
	}
	slave, err := syscall.Open(c.slavePath, syscall.O_RDWR|syscall.O_NOCTTY, 0)
	if err != nil {
		return err
	}
	defer syscall.Close(slave)
	return setPtySize(uintptr(slave), cols, rows)
}

func (c *attachConn) Close() error {
	c.closeOnce.Do(func() {
		c.timing.noteClosed(time.Now())
		if c.proc != nil {
			_ = c.proc.Kill()
			_, _ = c.proc.Wait()
		}
		c.closeErr = c.master.Close()
		slog.Debug("native attach conn closed")
	})
	return c.closeErr
}
