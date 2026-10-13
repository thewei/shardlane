package nativeui

import (
	"time"

	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 MyGo Animate、Shell 侧栏显示状态/拖拽宽度及页面路由策略
 * [OUTPUT]: 提供侧栏的裁剪式宽度动画与尾缘拖拽调宽（sidebarLimits 钳制）
 * [POS]: shell.View 的布局动效层；侧栏宽度是本文件的呈现状态（Shell.sidebarW），会话持久化在 session.go
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// Side-panel slide motion (2026-10-06). The sidebar and the Right Panel
// collapse and expand with a width animation instead of popping. The
// design follows MyGo's CollapsibleBase recipe, adapted to width:
//
//   - a persistent host element (same key, same sibling position) owns
//     the animation state across frames and animates its width;
//   - the panel content inside builds at its full open width while the
//     host CLIPS it, so sidebar rows never re-wrap mid-slide and the
//     only layout surface that changes per frame is the host width;
//   - once fully closed the content does not build: hidden surfaces do
//     no presentation work (execution rules §8).
//
// The desktop's Reduce Motion preference lands the width at once — the
// framework handles that inside Animate — which is also what keeps the
// collapsed/expanded logic deterministic under the headless Tester
// (whose frames advance no real time).

const (
	// panelSlideDuration matches MyGo's default transition length: long
	// enough to read, short enough to stay out of the way.
	panelSlideDuration = 200 * time.Millisecond
	// panelSlideEpsilon: widths below this read as fully closed and skip
	// building content, so a resting collapsed panel costs nothing.
	panelSlideEpsilon = 0.5
	// sidebarMinWidth keeps the navigator and its foot usable; the drag
	// never narrows the sidebar past it.
	sidebarMinWidth = float32(200)
	// sidebarMaxWidth caps the drag on wide windows, where a window-relative
	// cap alone would let the sidebar swallow the center.
	sidebarMaxWidth = float32(400)
	// sidebarCenterFloor is the window width reserved for the content
	// column: the drag cap is the window less this floor.
	sidebarCenterFloor = float32(480)
	// sidebarResizeHit is the drag handle's width at the sidebar's trailing
	// edge; its trailing 1px is the visible border line, so the sidebar's
	// surface ends exactly at the line and nothing sidebar-colored shows
	// on the content side (2026-10-13 user report).
	sidebarResizeHit = float32(6)
)

// sidebarLimits bounds the sidebar's width in a window this wide. The
// maximum never drops below the minimum, so a tiny window keeps one honest
// clamp instead of a negative range.
func sidebarLimits(windowW float32) (minWidth, maxWidth float32) {
	maxWidth = sidebarMaxWidth
	if room := windowW - sidebarCenterFloor; maxWidth > room {
		maxWidth = room
	}
	if maxWidth < sidebarMinWidth {
		maxWidth = sidebarMinWidth
	}
	return sidebarMinWidth, maxWidth
}

// slideWidth drives one panel host's animated width. The host element
// must persist frame-to-frame; its first build seeds at the target, so
// the very first frame of a session never animates. While the resize
// handle is down the width snaps to the target instead: easing toward a
// target that moves every frame reads as the sidebar lagging the drag.
func slideWidth(host ui.Element, key any, closed bool, openWidth float32, dragging bool) float32 {
	target := openWidth
	if closed {
		target = 0
	}
	if dragging {
		return host.AnimateTo(key, target, ui.Animation{Duration: time.Millisecond})
	}
	return host.Animate(key, target, panelSlideDuration)
}

// sidebarSlide mounts the full-height sidebar — its title-bar band, then the
// navigator — inside the persistent slide host, sliding out to the left as it
// hides. With a vibrancy window the column draws nothing and the material
// shows through; without one it falls back to the opaque sidebar token. The
// trailing edge is a drag handle: dragging it re-widths the sidebar between
// sidebarLimits, remembered for the session.
func (s *Shell) sidebarSlide(c *ui.Context) {
	// The reference tree and Terminal must have identical sidebar geometry.
	full := s.sidebarW + sidebarResizeHit
	host := ui.Row(c.Key("sidebar-slide")).Shrink(0).AlignItems(ui.Stretch)
	w := slideWidth(host, "width", s.sidebarCollapsed, full, s.sidebarWDragging)
	s.sidebarSlideW = w
	if w <= panelSlideEpsilon {
		s.sidebarSlideW = 0
		return
	}
	host.Width(w).Clip()
	host.Children(func() {
		ui.Row(c).Width(full).Shrink(0).AlignItems(ui.Stretch).Margin(0, 0, 0, w-full).Children(func() {
			column := ui.Column(c).Width(s.sidebarW).Shrink(0)
			if !c.Vibrancy() {
				column.Background(s.tokens(c.Theme().Dark).Sidebar)
			}
			column.Children(func() {
				s.sidebarBand(c)
				s.sidebar(c)
			})
			// The line is the handle's trailing pixel: the border sits
			// flush against the content column, never with sidebar
			// material to its right (2026-10-13 user report).
			handle := ui.Box(c).Width(sidebarResizeHit).Shrink(0).Cursor(ui.CursorResizeEW).
				AlignItems(ui.End).Label("Resize sidebar").Children(func() {
				ui.Box(c).Width(1).FillHeight().Background(s.tokens(c.Theme().Dark).BorderSubtle)
			})
			dx, _, held := handle.Dragged()
			if held {
				windowW, _ := c.Size()
				minWidth, maxWidth := sidebarLimits(windowW)
				s.sidebarW = clampF(s.sidebarW+dx, minWidth, maxWidth)
			}
			// Read one frame late by design: the handle builds after the
			// width is applied, so the drag lands on the next frame — the
			// same cadence as the Git file navigator's splitter.
			s.sidebarWDragging = held
		})
	})
}
