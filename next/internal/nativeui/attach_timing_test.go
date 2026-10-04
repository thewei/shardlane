package nativeui

import (
	"log/slog"
	"testing"
	"time"
)

/**
 * [INPUT]: attachFrameTiming 的 onRead/noteClosed 事件与注入时钟
 * [OUTPUT]: 首帧只记一次、settle 在 quiet 后恰好记一次、未出首帧即关闭记 Warn、非正数字节忽略
 * [POS]: attach_timing.go 的确定性回归；不发真实进程、不碰 pty
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

type timingLogEvent struct {
	level slog.Level
	msg   string
	args  map[string]any
}

func newCapturingTiming(started time.Time) (*attachFrameTiming, *[]timingLogEvent) {
	events := &[]timingLogEvent{}
	t := newAttachFrameTiming("inst", "pane-1", "term-1", 12, started)
	t.quiet = 10 * time.Millisecond
	t.emit = func(level slog.Level, msg string, args ...any) {
		m := map[string]any{}
		for i := 0; i+1 < len(args); i += 2 {
			if key, ok := args[i].(string); ok {
				m[key] = args[i+1]
			}
		}
		*events = append(*events, timingLogEvent{level: level, msg: msg, args: m})
	}
	return t, events
}

func countEvents(events []timingLogEvent, msg string) int {
	n := 0
	for _, e := range events {
		if e.msg == msg {
			n++
		}
	}
	return n
}

func waitFor(t *testing.T, timeout time.Duration, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		if cond() {
			return
		}
		time.Sleep(2 * time.Millisecond)
	}
	t.Fatalf("condition not met within %v", timeout)
}

func TestAttachFrameTimingFirstFrameLoggedOnce(t *testing.T) {
	started := time.Now()
	timing, events := newCapturingTiming(started)
	timing.onRead(4096, started.Add(50*time.Millisecond))
	timing.onRead(512, started.Add(55*time.Millisecond))
	timing.onRead(0, started.Add(56*time.Millisecond)) // ignored

	if got := countEvents(*events, "native attach first frame"); got != 1 {
		t.Fatalf("first frame logged %d times, want 1", got)
	}
	e := (*events)[0]
	if e.args["first_byte_ms"] != int64(50) {
		t.Fatalf("first_byte_ms = %v, want 50", e.args["first_byte_ms"])
	}
	if e.args["bytes"] != 4096 {
		t.Fatalf("bytes = %v, want 4096 (first burst only)", e.args["bytes"])
	}
	if e.args["spawn_ms"] != int64(12) || e.args["pane_id"] != "pane-1" || e.args["terminal_id"] != "term-1" {
		t.Fatalf("context attrs wrong: %v", e.args)
	}
	if e.level != slog.LevelInfo {
		t.Fatalf("level = %v, want Info", e.level)
	}
}

func TestAttachFrameTimingSettledAfterQuiet(t *testing.T) {
	started := time.Now()
	timing, events := newCapturingTiming(started)
	timing.onRead(1024, started.Add(30*time.Millisecond))
	timing.onRead(2048, started.Add(35*time.Millisecond))

	waitFor(t, 2*time.Second, func() bool {
		return countEvents(*events, "native attach settled") > 0
	})
	if got := countEvents(*events, "native attach settled"); got != 1 {
		t.Fatalf("settled logged %d times, want 1", got)
	}
	e := (*events)[len(*events)-1]
	if e.args["bytes"] != 3072 {
		t.Fatalf("settled bytes = %v, want 3072 (cumulative)", e.args["bytes"])
	}
}

func TestAttachFrameTimingClosedBeforeFirstFrame(t *testing.T) {
	started := time.Now()
	timing, events := newCapturingTiming(started)
	timing.noteClosed(started.Add(80 * time.Millisecond))

	if got := countEvents(*events, "native attach closed before first frame"); got != 1 {
		t.Fatalf("closed-before-first-frame logged %d times, want 1", got)
	}
	e := (*events)[len(*events)-1]
	if e.level != slog.LevelWarn {
		t.Fatalf("level = %v, want Warn", e.level)
	}
	if e.args["elapsed_ms"] != int64(80) {
		t.Fatalf("elapsed_ms = %v, want 80", e.args["elapsed_ms"])
	}

	// A conn that did produce a frame must not warn on close.
	started2 := time.Now()
	timing2, events2 := newCapturingTiming(started2)
	timing2.onRead(16, started2.Add(5*time.Millisecond))
	timing2.noteClosed(started2.Add(10 * time.Millisecond))
	if got := countEvents(*events2, "native attach closed before first frame"); got != 0 {
		t.Fatalf("closed-before-first-frame logged %d times after a frame, want 0", got)
	}
}
