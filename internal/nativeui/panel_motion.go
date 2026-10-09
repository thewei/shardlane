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

// sidebarSlide mounts the sidebar — plus its zone gutter to the content
// column — inside the persistent slide host.
func (s *Shell) sidebarSlide(c *ui.Context) {
	host := ui.Row(c.Key("sidebar-slide")).Shrink(0).AlignItems(ui.Stretch)
	w := slideWidth(host, "width", s.sidebarCollapsed, sidebarWidth+gorexGap)
	if w <= panelSlideEpsilon {
		return
	}
	host.Width(w).Clip()
	host.Children(func() {
		s.sidebar(c)
		// The zone gutter between the sidebar and the content column
		// (2026-10-06): the sidebar has no card edge of its own, so the
		// cards' margins alone did not read as a gap. Sidebar-visible
		// content starts a full gorexGap further in.
		ui.Box(c).Width(gorexGap).Shrink(0).PassThrough()
	})
}

// panelMinCenterWidth reserves enough room for a real Terminal/Diff task
// while both navigation rails compete for space. This is presentation-only:
// a narrow window never writes the user's saved panel visibility preference.
const panelMinCenterWidth float32 = 560

// contextPanelFits is one pure width policy used by titlebar, shortcut, and
// panel host. It does not treat a saved open preference as a layout mandate.
func contextPanelFits(windowWidth float32, sidebarVisible bool, rightWidth int) bool {
	if windowWidth <= 0 {
		return true // no window geometry yet
	}
	used := float32(rightWidth)
	if sidebarVisible {
		used += sidebarWidth + gorexGap
	}
	return windowWidth-used >= panelMinCenterWidth
}

func (s *Shell) contextPanelFits(windowWidth float32) bool {
	return contextPanelFits(windowWidth, !s.sidebarCollapsed, s.rightPanel.width)
}

func (s *Shell) contextPanelVisible(windowWidth float32) bool {
	return s.contextPanelOpen() && s.contextPanelFits(windowWidth)
}

// rightPanelSlide mounts the panel only when the central task retains enough
// room. Auto-hidden panels restore on resize without altering their saved
// open state or Workspace/History context ownership.
func (s *Shell) rightPanelSlide(c *ui.Context) {
	host := ui.Row(c.Key("right-panel-slide")).Shrink(0).AlignItems(ui.Stretch)
	windowWidth, _ := c.Size()
	w := slideWidth(host, "width", !s.contextPanelVisible(windowWidth), float32(s.rightPanel.width))
	if w <= panelSlideEpsilon {
		return
	}
	host.Width(w).Clip()
	host.Children(func() {
		s.rightPanelContent(c)
	})
}
