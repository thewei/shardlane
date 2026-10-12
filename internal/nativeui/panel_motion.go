package nativeui

import (
	"time"

	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 MyGo Animate、Shell 侧栏显示状态及 contextPanelForRoute 页面策略
 * [OUTPUT]: 提供侧栏/右侧上下文面板的裁剪式宽度动画
 * [POS]: shell.View 的布局动效层；无右侧上下文的页面不创建无关工具面板
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
)

// slideWidth drives one panel host's animated width. The host element
// must persist frame-to-frame; its first build seeds at the target, so
// the very first frame of a session never animates.
func slideWidth(host ui.Element, key any, closed bool, openWidth float32) float32 {
	target := openWidth
	if closed {
		target = 0
	}
	return host.Animate(key, target, panelSlideDuration)
}

// sidebarSlide mounts the full-height sidebar — its title-bar band, then the
// navigator — inside the persistent slide host, sliding out to the left as it
// hides. With a vibrancy window the column draws nothing and the material
// shows through; without one it falls back to the opaque sidebar token. A
// hairline marks its trailing edge.
func (s *Shell) sidebarSlide(c *ui.Context) {
	// The reference tree and Terminal must have identical sidebar geometry.
	const full = sidebarWidth + 1
	host := ui.Row(c.Key("sidebar-slide")).Shrink(0).AlignItems(ui.Stretch)
	w := slideWidth(host, "width", s.sidebarCollapsed, full)
	s.sidebarSlideW = w
	if w <= panelSlideEpsilon {
		s.sidebarSlideW = 0
		return
	}
	host.Width(w).Clip()
	host.Children(func() {
		ui.Row(c).Width(full).Shrink(0).AlignItems(ui.Stretch).Margin(0, 0, 0, w-full).Children(func() {
			column := ui.Column(c).Width(sidebarWidth).Shrink(0)
			if !c.Vibrancy() {
				column.Background(s.tokens(c.Theme().Dark).Sidebar)
			}
			column.Children(func() {
				s.sidebarBand(c)
				s.sidebar(c)
			})
			ui.Box(c).Width(1).Background(s.tokens(c.Theme().Dark).BorderSubtle)
		})
	})
}
