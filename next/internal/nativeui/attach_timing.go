package nativeui

import (
	"context"
	"log/slog"
	"sync"
	"time"
)

/**
 * [INPUT]: attach_conn 的 Read(n)/Close 事件、newAttachConn 的 spawn 起止时刻
 * [OUTPUT]: attachFrameTiming —— 一次 attach 的启动分段计时日志：Info "native attach first frame"
 *           （spawn_ms / first_byte_ms / 首帧字节数）、Info "native attach settled"（快照串 quiet 后的
 *           settle_ms / 累计字节数）、Warn "native attach closed before first frame"（未收到首帧即关闭）
 * [POS]: attach 传输的诊断观测层：只记录耗时与字节计数，绝不记录终端字节内容（execution rules §9）；
 *           计时在锁内完成、settle 检测由一次性 AfterFunc 异步触发，热路径无阻塞 IO
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// attachSettleQuiet is the read-idle gap after which the initial snapshot
// burst is considered settled. A pane with a busy program never goes quiet
// and simply never logs the settled line; the first-frame line always does.
const attachSettleQuiet = 300 * time.Millisecond

// attachFrameTiming segments one attach's startup latency: process spawn,
// daemon first frame, and the settle of the initial snapshot burst. It logs
// durations and byte counts only — never terminal bytes.
type attachFrameTiming struct {
	// emit is the sink (slog in production, captured in tests).
	emit func(level slog.Level, msg string, args ...any)
	// quiet is the read-idle gap marking the snapshot burst settled.
	quiet time.Duration

	mu       sync.Mutex
	context  []any // instance/pane_id/terminal_id attrs appended to every line
	created  time.Time
	spawnMs  int64
	firstAt  time.Time
	firstN   int
	totalN   int
	settled  bool
	settleTk *time.Timer
}

// newAttachFrameTiming starts timing one attach from the moment its spawn
// began (before LookPath/pty), so every segment sums to the wall time the
// pane card stayed blank.
func newAttachFrameTiming(instance, paneID, terminalID string, spawnMs int64, started time.Time) *attachFrameTiming {
	return &attachFrameTiming{
		emit: func(level slog.Level, msg string, args ...any) {
			slog.Log(context.Background(), level, msg, args...)
		},
		quiet:   attachSettleQuiet,
		context: []any{"instance", instance, "pane_id", paneID, "terminal_id", terminalID},
		created: started,
		spawnMs: spawnMs,
	}
}

// onRead records one successful daemon-side Read. The first nonzero read is
// the attach's first frame; afterwards each read rearms the one-shot settle
// timer.
func (t *attachFrameTiming) onRead(n int, now time.Time) {
	if n <= 0 {
		return
	}
	t.mu.Lock()
	defer t.mu.Unlock()
	t.totalN += n
	if t.firstAt.IsZero() {
		t.firstAt = now
		t.firstN = n
		t.emit(slog.LevelInfo, "native attach first frame", append(t.attrs(),
			"first_byte_ms", now.Sub(t.created).Milliseconds(),
			"bytes", n)...)
	}
	if t.settleTk == nil {
		t.settleTk = time.AfterFunc(t.quiet, t.settle)
	} else {
		t.settleTk.Reset(t.quiet)
	}
}

// settle logs the snapshot burst as settled once reads went quiet.
func (t *attachFrameTiming) settle() {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.settled || t.firstAt.IsZero() {
		return
	}
	t.settled = true
	t.emit(slog.LevelInfo, "native attach settled", append(t.attrs(),
		"settle_ms", time.Since(t.created).Milliseconds(),
		"bytes", t.totalN)...)
}

// noteClosed stops the settle timer and warns when the conn was torn down
// without ever producing a frame (daemon gone, CLI failure, hung handshake).
func (t *attachFrameTiming) noteClosed(now time.Time) {
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.settleTk != nil {
		t.settleTk.Stop()
	}
	if t.firstAt.IsZero() {
		t.emit(slog.LevelWarn, "native attach closed before first frame", append(t.attrs(),
			"elapsed_ms", now.Sub(t.created).Milliseconds())...)
	}
}

func (t *attachFrameTiming) attrs() []any {
	return append([]any{"spawn_ms", t.spawnMs}, t.context...)
}
