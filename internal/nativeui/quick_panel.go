package nativeui

import (
	"runtime"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 mygo 的 Flyout（Tray/Parent 锚定、Popover 系统材质、菜单式关闭与 Escape）、ui.Segmented、agent_card 共享卡片、tray_model 的计数
 * [OUTPUT]: 对外提供 Shell.QuickPanelView（Agent 概览或聚焦详情）、Shell.ToggleQuickPanel（托盘）、Shell.toggleAgentActivityPanel（标题栏按钮）、QuickPanelHeightFor（内容驱动高度纯函数）
 * [POS]: Agent 活动的唯一浮层：托盘/标题栏浏览概览，侧栏/列表卡片浏览精确 Agent；同一 Flyout 生命周期，锚定/贴边/翻转/关闭归框架
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// Floating Agent activity panel (0.7 §17.1/§17.2, 2026-10-11 revision): one
// MyGo flyout at a time — a system Popover on macOS — anchored beneath the
// menu-bar item, the titlebar's agent button, or a sidebar Agent card. It
// takes the keyboard while shown and closes like a menu: outside click,
// focus moving away, app deactivation, or Escape. One shared
// StatusCenterSnapshot, a status tab group switching the agent list, and the
// sidebar's compact agent cards. The height follows the content up to the
// fixed cap — beyond it the list scrolls inside.

// QuickPanelWidth is the §17.1 suggested panel width (360–420 DIP).
const QuickPanelWidth = 400

// QuickPanelHeight caps the content-driven height; the rows scroll inside
// beyond it. The flyout shrinks to fit a small work area on its own.
const QuickPanelHeight = 480

// quickPanelGap keeps breathing room between the anchor and the panel.
const quickPanelGap = 8

// quickPanelChromeHeight budgets the header summary + status tab group +
// scroll padding; quickPanelRowHeight is one agent card plus its gap;
// quickPanelEmptyHeight is the empty state's budget.
const (
	quickPanelChromeHeight = 180
	quickPanelRowHeight    = 44
	quickPanelEmptyHeight  = 72
)

// quickPanelMinHeight keeps the panel presentable when it is nearly empty.
const quickPanelMinHeight = 160

// The focused Agent card uses the same flyout but a shorter,
// action-oriented detail layout, rather than a second popup implementation.
const quickPanelAgentHeight = 328

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

// QuickPanelView renders the floating panel content (§17.2): over the
// popover's material on macOS the content draws no background of its own;
// elsewhere it draws the panel (Windows 11 rounds and outlines the flyout,
// so no radius or border there). The shared StatusCenterSnapshot feeds the
// summary header, the status tab group and the agent cards; a focused Agent
// swaps the whole body for its detail layout. Escape closes through the
// flyout itself (§17.1) — the view deliberately does not take it.
func (s *Shell) QuickPanelView(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	tokens := designTokens(t.Dark)
	c.Root().Background(ui.Transparent)
	body := ui.Column(c).Fill()
	if !c.Vibrancy() {
		body = body.Background(tokens.Content)
		if runtime.GOOS != "windows" {
			body = body.Radius(10).Border(1, tokens.BorderSubtle)
		}
	}
	body.Children(func() {
		snapshot := s.BuildStatusCenterSnapshot()
		if s.quickPanelAgent != nil {
			s.agentQuickPanelDetail(c, snapshot)
			return
		}
		entries := snapshot.Entries
		header := QuickPanelHeader(snapshot, !s.offline)
		if s.quickPanelTab == 1 {
			header = "Scripts and open ports"
		}
		ui.Column(c).FillWidth().Padding(sp.M, sp.L).Gap(sp.XS).
			BorderWidth(0, 0, 1, 0).BorderColor(tokens.BorderSubtle).Children(func() {
			ui.Text(c, "Agents & Services").FontSize(Typography().Heading).FontWeight(WeightSemibold)
			ui.Text(c, header).FontSize(Typography().Caption).TextColor(t.TextMuted).SingleLine()
		})
		ui.Row(c).FillWidth().Padding(sp.S, sp.L, 0).Children(func() {
			view := s.quickPanelTab
			if ui.Segmented(c, &view, "Agents", "Services").FillWidth().Changed() {
				s.quickPanelTab = view
				s.fitQuickPanel()
			}
		})
		if s.quickPanelTab == 1 {
			s.quickPanelServices(c)
			return
		}
		if s.offline {
			ui.Row(c).FillWidth().Padding(sp.M, sp.L, 0).Gap(sp.S).AlignItems(ui.Center).Children(func() {
				ui.Text(c, "Herdr disconnected").FontSize(Typography().Caption).Grow(1).TextColor(t.TextMuted)
				if ui.Button(c, "Reconnect").Clicked() {
					s.reloadInstances(false)
				}
			})
		}
		// N19 #5: a simple agent list, like the sidebar's — no stat tiles,
		// no status filter rail.
		ui.Scroll(c).Grow(1).Children(func() {
			ui.Column(c).FillWidth().Padding(sp.S, sp.L, sp.L).Gap(sp.S).Children(func() {
				if len(entries) == 0 {
					emptyState(c, "No active agents", "")
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
						key := entry.Card.Key
						s.quickPanelAgent = &key
						s.fitQuickPanel()
						s.invalidateQuickPanel()
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

// quickPanelDismissGrace is the window in which an anchor click right after
// the flyout dismissed still counts as "close": on macOS the outside press
// that dismissed the panel goes on to the element under it, so clicking the
// tray item or the titlebar button closes the panel first and only then
// delivers its action — without this grace the click would re-open the
// panel it meant to close.
const quickPanelDismissGrace = 250 * time.Millisecond

// quickPanelToggleCloses decides whether an anchor click closes the panel
// instead of opening it: either a panel is open, or one just dismissed.
func quickPanelToggleCloses(panel *mygo.Window, dismissedAt, now time.Time) bool {
	return panel != nil || now.Sub(dismissedAt) < quickPanelDismissGrace
}

// ToggleQuickPanel is the tray click action (§17.1 lifecycle): an open
// panel closes; otherwise one opens beneath the menu-bar item — the
// framework centers it under the icon and keeps it in the work area. The
// icon's own press does not dismiss a focusable flyout, so the click keeps
// toggling.
func (s *Shell) ToggleQuickPanel() {
	if s.trayHandle == nil {
		return
	}
	if quickPanelToggleCloses(s.quickPanel, s.quickPanelDismissedAt, time.Now()) {
		s.closeQuickPanel()
		return
	}
	s.quickPanelAgent = nil
	s.openQuickPanelFlyout(mygo.FlyoutOptions{
		Tray:      s.trayHandle,
		Placement: mygo.PlacementBottom,
		Gap:       quickPanelGap,
	})
}

// toggleAgentActivityPanel is the titlebar button's action: the same
// flyout, anchored beneath the button — ui.Element.Bounds is already the
// content-area rectangle the flyout anchor wants. No main window (headless
// tests, platform fallback) keeps the click a no-op.
func (s *Shell) toggleAgentActivityPanel(anchor ui.Element) {
	if s.win == nil {
		return
	}
	if quickPanelToggleCloses(s.quickPanel, s.quickPanelDismissedAt, time.Now()) {
		s.closeQuickPanel()
		return
	}
	s.quickPanelAgent = nil
	s.openQuickPanelFlyout(mygo.FlyoutOptions{
		Parent:    s.win,
		Anchor:    quickAnchorRect(anchor.Bounds()),
		Placement: mygo.PlacementBottom,
		Gap:       quickPanelGap,
	})
}

// quickAnchorRect converts the element bounds (content-area DIPs) into the
// flyout anchor rectangle.
func quickAnchorRect(b ui.Rect) mygo.Rectangle {
	return mygo.Rectangle{X: int(b.X), Y: int(b.Y), Width: int(b.W), Height: int(b.H)}
}

// openQuickPanelFlyout creates and shows the panel (§17.1): a focusable
// Popover — the system's material, arrow and animation on macOS — that
// closes as the user clicks elsewhere, another window takes the keyboard,
// the app deactivates, or Escape reaches it. A panel still open closes
// first; its OnClosed is identity-guarded so a replacement created here
// survives the stale callback.
func (s *Shell) openQuickPanelFlyout(base mygo.FlyoutOptions) {
	// Detach before closing: a programmatic Close fires OnClosed inline, and
	// the stale listener must not reset the selection the caller just set
	// (the agent-card switch path opens the replacement right here).
	if prev := s.quickPanel; prev != nil {
		s.quickPanel = nil
		prev.Close()
	}
	// The actionable rows' usage captions refresh right before showing,
	// forced past the throttle.
	s.refreshUsageProjection(true)
	base.Width = QuickPanelWidth
	base.Height = s.quickPanelContentHeight()
	base.Focusable = true
	base.Popover = true
	base.Shadow = true
	base.Content = ui.View(s.QuickPanelView)
	panel := mygo.NewFlyout(base)
	s.quickPanel = panel
	panel.OnClosed(func() {
		s.quickPanelDismissedAt = time.Now()
		if s.quickPanel == panel {
			s.quickPanel = nil
			s.quickPanelAgent = nil
		}
	})
}

// closeQuickPanel closes the open panel; the OnClosed listener resets the
// selection. Nil-safe (headless sessions, tests, already-closed panel).
func (s *Shell) closeQuickPanel() {
	if s.quickPanel != nil {
		s.quickPanel.Close()
	}
}

// hideQuickPanel is the action-button path (open Terminal / Chat /
// Inspector): close the panel and let OnClosed reset its state, so the
// content does not flicker to another layout during the close animation.
func (s *Shell) hideQuickPanel() {
	s.closeQuickPanel()
}

// invalidateQuickPanel repaints the open panel after an interaction or a
// background projection change.
func (s *Shell) invalidateQuickPanel() {
	if s.quickPanel != nil && s.quickPanel.IsVisible() {
		s.quickPanel.Invalidate()
	}
}

// quickPanelContentHeight is the panel height for the current content
// (§17.1): the focused detail layout, the fixed Services tab, or the
// overview's content-driven height.
func (s *Shell) quickPanelContentHeight() int {
	if s.quickPanelAgent != nil {
		return quickPanelAgentHeight
	}
	if s.quickPanelTab == 1 {
		return QuickPanelHeight
	}
	return QuickPanelHeightFor(len(s.filterStatusEntries(s.BuildStatusCenterSnapshot())))
}

// fitQuickPanel resizes the open flyout to the current content height; the
// framework re-places it against its anchor and the work area.
func (s *Shell) fitQuickPanel() {
	if s.quickPanel == nil {
		return
	}
	s.quickPanel.SetSize(QuickPanelWidth, s.quickPanelContentHeight())
}
