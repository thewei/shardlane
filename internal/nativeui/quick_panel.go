package nativeui

import (
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 mygo 的第二个无边框窗口与 Screen 工作区、ui.Segmented、agent_card 共享卡片、tray_model 的计数
 * [OUTPUT]: 对外提供 Shell.QuickPanelView（Agent 活动浮层内容）、AttachQuickPanel、ToggleQuickPanel（托盘锚点）、Shell.toggleAgentActivityPanel（标题栏按钮锚点）、QuickPanelHeightFor（内容驱动高度纯函数）
 * [POS]: Agent 活动的唯一浮层（2026-10-07）：托盘与标题栏按钮共用；高度随内容伸缩、封顶后内部滚动；状态标签组切换列表
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// Floating Agent activity panel (0.7 §17.1/§17.2, 2026-10-07 revision): a
// dedicated frameless MyGo window anchored beneath the menu-bar item or the
// titlebar's agent button, never in the Dock/taskbar, hidden on blur and
// Escape. One shared StatusCenterSnapshot, a status tab group switching the
// agent list, and the sidebar's compact agent cards. The height follows the
// content up to the fixed cap — beyond it the list scrolls inside.

// QuickPanelWidth is the §17.1 suggested panel width (360–420 DIP).
const QuickPanelWidth = 400

// QuickPanelHeight caps the content-driven height; the rows scroll inside
// beyond it. It is clamped to the display work area at anchor time.
const QuickPanelHeight = 480

// quickPanelGap keeps breathing room between the anchor and the panel.
const quickPanelGap = 8

// quickPanelChromeHeight budgets the header summary + status tab group +
// scroll padding; quickPanelRowHeight is one agent card plus its gap;
// quickPanelEmptyHeight is the empty state's budget.
const (
	quickPanelChromeHeight = 116
	quickPanelRowHeight    = 44
	quickPanelEmptyHeight  = 72
)

// quickPanelMinHeight keeps the panel presentable when it is nearly empty.
const quickPanelMinHeight = 160

// QuickPanelHeightFor returns the content-driven panel height: chrome plus
// one row per listed agent, clamped between the minimum and the fixed cap
// (2026-10-07: the panel is no longer a fixed-height window).
func QuickPanelHeightFor(rows int) int {
	budget := quickPanelChromeHeight + rows*quickPanelRowHeight
	if rows == 0 {
		budget = quickPanelChromeHeight + quickPanelEmptyHeight
	}
	if budget < quickPanelMinHeight {
		budget = quickPanelMinHeight
	}
	if budget > QuickPanelHeight {
		budget = QuickPanelHeight
	}
	return budget
}

// QuickRect is the pure geometry rectangle (mygo.Rectangle coordinates).
type QuickRect struct {
	X, Y, Width, Height int
}

// QuickPanelBounds computes the §17.1 screen-safe panel origin anchored
// beneath the anchor rect: horizontally centered on it, clamped into
// the work area, and flipped above when the panel would overflow the
// work-area bottom. A degenerate work area falls back to the raw anchor.
func QuickPanelBounds(anchor, workArea QuickRect, width, height int) (int, int) {
	x := anchor.X + anchor.Width/2 - width/2
	y := anchor.Y + anchor.Height + quickPanelGap
	if workArea.Width <= 0 || workArea.Height <= 0 {
		return x, y
	}
	// Horizontal clamp.
	if x+width > workArea.X+workArea.Width {
		x = workArea.X + workArea.Width - width
	}
	if x < workArea.X {
		x = workArea.X
	}
	// Vertical: flip above the anchor when the bottom would overflow.
	if y+height > workArea.Y+workArea.Height {
		y = anchor.Y - quickPanelGap - height
	}
	if y+height > workArea.Y+workArea.Height {
		y = workArea.Y + workArea.Height - height
	}
	if y < workArea.Y {
		y = workArea.Y
	}
	return x, y
}

// quickRectOf converts the mygo geometry types.
func quickRectOf(r mygo.Rectangle) QuickRect {
	return QuickRect{X: r.X, Y: r.Y, Width: r.Width, Height: r.Height}
}

// QuickPanelView renders the floating panel content (§17.2, 2026-10-07):
// the product name and shared summary header, a status tab group switching
// the agent list, and the sidebar's compact agent cards over the shared
// StatusCenterSnapshot. Escape hides the panel (§17.1).
func (s *Shell) QuickPanelView(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	if c.Shortcut(0, ui.KeyEscape) {
		s.hideQuickPanel()
	}
	snapshot := s.BuildStatusCenterSnapshot()
	entries := s.filterStatusEntries(snapshot)
	header := QuickPanelHeader(snapshot, !s.offline)
	ui.Column(c).Grow(1).Background(designTokens(t.Dark).Content).Children(func() {
		ui.Column(c).FillWidth().Padding(sp.M, sp.L).Gap(sp.XS).
			BorderWidth(0, 0, 1, 0).BorderColor(designTokens(t.Dark).BorderSubtle).Children(func() {
			ui.Text(c, "Shardlane").FontSize(Typography().Body + 1).FontWeight(700)
			ui.Text(c, header).FontSize(Typography().Caption).TextColor(t.TextMuted).SingleLine()
		})
		// The status tab group switches the agent list (2026-10-07); a
		// change resizes the panel window to the new content height.
		ui.Row(c).FillWidth().Padding(sp.S, sp.L, 0).Children(func() {
			chosen := s.workbench.filterChoice()
			if ui.Segmented(c, &chosen, workbenchFilterLabels...).Changed() {
				s.workbench.setFilterChoice(chosen)
				s.positionQuickPanel()
			}
		})
		ui.Scroll(c).Grow(1).Children(func() {
			ui.Column(c).FillWidth().Padding(sp.S, sp.L, sp.L).Gap(sp.S).Children(func() {
				if len(entries) == 0 {
					emptyState(c, "No agents here", "Agents in this status will appear here.")
					return
				}
				for _, entry := range entries {
					entry := entry
					var row ui.Element
					ui.Box(c).Children(func() {
						row = s.agentCardRow(c, entry.Card, false)
						// §17.2: the usage caption renders under the card
						// when the cached History-meta projection has facts.
						if usage := s.usageLineFor(entry.Card.Key); usage != "" {
							ui.Box(c).Children(func() {
								ui.Text(c, usage).FontSize(Typography().Caption).
									TextColor(t.TextMuted).SingleLine()
							})
						}
					})
					if row.Clicked() {
						s.hideQuickPanel()
						if entry.HasInteraction {
							s.openChat(entry.Card)
							return
						}
						s.openAgentCard(entry.Card)
					}
				}
			})
		})
	})
}

// filterStatusEntries narrows the shared snapshot to the active tab-group
// choice; the snapshot order (interactions first, then attention > review >
// working > idle) is kept.
func (s *Shell) filterStatusEntries(snapshot StatusCenterSnapshot) []StatusCenterEntry {
	if s.workbench.filterAll {
		return snapshot.Entries
	}
	entries := make([]StatusCenterEntry, 0, len(snapshot.Entries))
	for _, entry := range snapshot.Entries {
		if entry.Bucket == s.workbench.filter {
			entries = append(entries, entry)
		}
	}
	return entries
}

// AttachQuickPanel binds the §17.1 floating panel window: blur hides it,
// and a close request (e.g. Cmd-W from the Window menu) is downgraded to
// hide so the reusable panel survives. A real quit must NOT be downgraded:
// the application quit aborts when any window prevents its close, and a
// prevented close here left a windowless process holding the
// single-instance lock — ⌘Q never quit and relaunching surfaced nothing
// (2026-10-07 user report). MarkQuitting flips the flag from the quit path.
func (s *Shell) AttachQuickPanel(window *mygo.Window) {
	s.quickPanel = window
	window.OnBlur(func() {
		if window.IsVisible() {
			s.quickPanelBlurHideAt = time.Now()
		}
		s.hideQuickPanel()
	})
	window.OnClose(func(e *mygo.CloseEvent) {
		s.quickPanelCloseRequest(e)
	})
}

// quickPanelCloseRequest applies the panel's close policy: while the
// application is quitting the close goes through (mygo aborts the whole
// quit when any window prevents its close), any other close request —
// ⌘W, the system send-to-close — is downgraded to hide so the reusable
// panel survives.
func (s *Shell) quickPanelCloseRequest(e *mygo.CloseEvent) {
	if s.quitting {
		return
	}
	e.PreventDefault()
	s.hideQuickPanel()
}

// quickPanelToggleGrace is the window in which an anchor click right after
// a blur-hide still counts as "close": clicking the tray item or the
// titlebar button first strips the panel's key status (blur-hide) and only
// then delivers its action, so without this grace the click would re-open
// the panel it meant to close.
const quickPanelToggleGrace = 250 * time.Millisecond

// quickPanelToggleOff decides whether the anchor click should close the
// panel instead of opening it: either it is still visible, or blur just
// hid it.
func quickPanelToggleOff(visible bool, blurHideAt, now time.Time) bool {
	return visible || now.Sub(blurHideAt) < quickPanelToggleGrace
}

// hideQuickPanel hides the panel when visible (blur/Escape lifecycle).
func (s *Shell) hideQuickPanel() {
	if s.quickPanel != nil && s.quickPanel.IsVisible() {
		s.quickPanel.Hide()
	}
}

// invalidateQuickPanel repaints the floating panel while it is visible.
// The panel window is created hidden and receives no input between user
// interactions, so MyGo never repaints it on its own: without this push it
// would keep showing the frame it first built at startup.
func (s *Shell) invalidateQuickPanel() {
	if s.quickPanel != nil && s.quickPanel.IsVisible() {
		s.quickPanel.Invalidate()
	}
}

// quickPanelAnchor / quickPanelWorkArea remember where the panel was last
// opened from, so a tab-group change can re-fit the height in place.
type quickPanelPlacement struct {
	anchor   QuickRect
	workArea QuickRect
}

// ToggleQuickPanel is the tray click action (§17.1 lifecycle): a visible
// panel hides; otherwise the panel anchors beneath the menu-bar item.
func (s *Shell) ToggleQuickPanel(trayBounds mygo.Rectangle) {
	s.toggleQuickPanelAnchored(quickRectOf(trayBounds))
}

// toggleAgentActivityPanel is the titlebar button's action: the same
// floating panel, anchored beneath the button (2026-10-07). The element
// bounds are window coordinates; the window bounds make them screen
// coordinates. No attached panel (headless tests, platform fallback) keeps
// the click a no-op.
func (s *Shell) toggleAgentActivityPanel(anchor ui.Element) {
	if s.quickPanel == nil || s.win == nil {
		return
	}
	if quickPanelToggleOff(s.quickPanel.IsVisible(), s.quickPanelBlurHideAt, time.Now()) {
		s.hideQuickPanel()
		return
	}
	rect := anchor.Bounds()
	win := s.win.Bounds()
	s.toggleQuickPanelAnchored(QuickRect{
		X:      win.X + int(rect.X),
		Y:      win.Y + int(rect.Y),
		Width:  int(rect.W),
		Height: int(rect.H),
	})
}

// toggleQuickPanelAnchored shows the panel beneath the given screen anchor
// or hides it (§17.1 toggle). The actionable rows' usage captions refresh
// right before showing, forced past the throttle.
func (s *Shell) toggleQuickPanelAnchored(anchor QuickRect) {
	if s.quickPanel == nil {
		return
	}
	if quickPanelToggleOff(s.quickPanel.IsVisible(), s.quickPanelBlurHideAt, time.Now()) {
		s.hideQuickPanel()
		return
	}
	s.refreshUsageProjection(true)
	display := mygo.Screen.DisplayNearestPoint(mygo.Point{
		X: anchor.X + anchor.Width/2,
		Y: anchor.Y + anchor.Height/2,
	})
	s.quickPanelPlacement = quickPanelPlacement{anchor: anchor, workArea: quickRectOf(display.WorkArea)}
	s.positionQuickPanel()
	// Show() does not repaint: rebuild the frame from the current snapshot
	// so the panel never opens on a stale (e.g. startup) render.
	s.quickPanel.Invalidate()
	s.quickPanel.Show()
	s.quickPanel.Focus()
}

// positionQuickPanel resizes the panel window to the current content
// height, anchored at its remembered placement. No-op without a panel.
func (s *Shell) positionQuickPanel() {
	if s.quickPanel == nil {
		return
	}
	height := QuickPanelHeightFor(len(s.filterStatusEntries(s.BuildStatusCenterSnapshot())))
	x, y := QuickPanelBounds(s.quickPanelPlacement.anchor, s.quickPanelPlacement.workArea, QuickPanelWidth, height)
	s.quickPanel.SetBounds(mygo.Rectangle{X: x, Y: y, Width: QuickPanelWidth, Height: height})
}
