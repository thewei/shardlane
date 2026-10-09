package nativeui

import (
	"time"

	"github.com/egoist/mygo/ui"
)

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

// rightPanelSlide mounts the Right Panel inside its persistent slide
// host. The center surface is never changed by panel open/close
// (GWB-074 regression contract) — this only animates the reveal.
func (s *Shell) rightPanelSlide(c *ui.Context) {
	host := ui.Row(c.Key("right-panel-slide")).Shrink(0).AlignItems(ui.Stretch)
	w := slideWidth(host, "width", !s.rightPanel.open, float32(s.rightPanel.width))
	if w <= panelSlideEpsilon {
		return
	}
	host.Width(w).Clip()
	host.Children(func() {
		s.rightPanelContent(c)
	})
}
