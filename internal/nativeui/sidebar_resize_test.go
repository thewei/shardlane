package nativeui

/**
 * [INPUT]: headless MyGo sidebar slide host、原生指针事件与会话快照
 * [OUTPUT]: 侧栏拖拽调宽的钳制契约（sidebarLimits、clampSessionSidebarWidth）与拖拽/恢复回归
 * [POS]: 侧栏尾缘拖拽的呈现契约测试
 * [PROTOCOL]: update header and check CLAUDE.md
 */

import (
	"math"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/app"
	"github.com/wh-studio/herdr-client/internal/settings"
)

func TestSidebarLimitsKeepContentUsable(t *testing.T) {
	minWidth, maxWidth := sidebarLimits(1200)
	if minWidth != sidebarMinWidth || maxWidth != sidebarMaxWidth {
		t.Fatalf("wide window limits = %v/%v, want %v/%v", minWidth, maxWidth, sidebarMinWidth, sidebarMaxWidth)
	}
	// A narrow window caps the drag at the room the content column keeps.
	if _, maxWidth = sidebarLimits(800); maxWidth != 800-sidebarCenterFloor {
		t.Fatalf("narrow window cap = %v, want %v", maxWidth, 800-sidebarCenterFloor)
	}
	// A tiny window floors the cap at the minimum instead of inverting it.
	if minWidth, maxWidth = sidebarLimits(400); minWidth != sidebarMinWidth || maxWidth != sidebarMinWidth {
		t.Fatalf("tiny window limits = %v/%v, want %v/%v", minWidth, maxWidth, sidebarMinWidth, sidebarMinWidth)
	}
}

func TestSidebarDragResizesAndClamps(t *testing.T) {
	s := slideTestShell(t)
	tester := ui.NewTester(s.View, 1200, 800)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	tester.Frame()
	handle, ok := tester.Find("Resize sidebar")
	if !ok {
		t.Fatalf("missing sidebar resize handle; texts=%q", tester.Texts())
	}
	if s.sidebarW != sidebarWidth {
		t.Fatalf("sidebar starts at %v, want the default %v", s.sidebarW, sidebarWidth)
	}

	// Dragging right widens the sidebar; with Reduce Motion the rendered
	// edge follows the dragged width in the same frame.
	x, y := handle.X+handle.W/2, handle.Y+handle.H/2
	tester.Press(x, y)
	tester.Move(x+40, y)
	tester.Release(x+40, y)
	tester.Frame()
	if s.sidebarW <= sidebarWidth {
		t.Fatalf("dragging right should widen the sidebar, got %v", s.sidebarW)
	}
	handle, ok = tester.Find("Resize sidebar")
	if !ok || math.Abs(float64(handle.X-s.sidebarW)) > 1 {
		t.Fatalf("rendered edge at %v (ok=%v) does not follow dragged width %v", handle, ok, s.sidebarW)
	}

	// Both clamps hold no matter how far the drag goes.
	handle, _ = tester.Find("Resize sidebar")
	x, y = handle.X+handle.W/2, handle.Y+handle.H/2
	tester.Press(x, y)
	tester.Move(x+2000, y)
	tester.Release(x+2000, y)
	tester.Frame()
	if s.sidebarW != sidebarMaxWidth {
		t.Fatalf("oversized drag landed at %v, want the cap %v", s.sidebarW, sidebarMaxWidth)
	}
	handle, _ = tester.Find("Resize sidebar")
	x, y = handle.X+handle.W/2, handle.Y+handle.H/2
	tester.Press(x, y)
	tester.Move(x-2000, y)
	tester.Release(x-2000, y)
	tester.Frame()
	if s.sidebarW != sidebarMinWidth {
		t.Fatalf("undersized drag landed at %v, want the floor %v", s.sidebarW, sidebarMinWidth)
	}
}

func TestSessionSidebarWidthRoundTripAndClamp(t *testing.T) {
	store := settings.NewMemoryStore(settings.Default())
	first := NewShell(WithSettings(app.NewSettingsService(store)))
	first.sidebarW = 350
	first.SaveSession()
	second := NewShell(WithSettings(app.NewSettingsService(store)))
	if second.sidebarW != 350 {
		t.Fatalf("sidebar width not restored: %v, want 350", second.sidebarW)
	}

	if got := clampSessionSidebarWidth(5000); got != int(sidebarMaxWidth) {
		t.Fatalf("oversized restore = %d, want %d", got, int(sidebarMaxWidth))
	}
	if got := clampSessionSidebarWidth(50); got != int(sidebarMinWidth) {
		t.Fatalf("undersized restore = %d, want %d", got, int(sidebarMinWidth))
	}
	if got := clampSessionSidebarWidth(0); got != int(sidebarMinWidth) {
		t.Fatalf("zero restore = %d, want %d (restore itself skips 0)", got, int(sidebarMinWidth))
	}
}
