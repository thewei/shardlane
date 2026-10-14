package nativeui

import (
	"runtime"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
)

/**
 * [INPUT]: 依赖 mygo 的 Flyout（Tray/Parent 锚定、Popover 系统材质、菜单式关闭与 Escape）、ui.Segmented、agent_card 共享卡片、agent_quick_panel 的分组与条目行
 * [OUTPUT]: 对外提供 Shell.QuickPanelView（Agents 按状态分组的列表 / Services）、Shell.ToggleQuickPanel（托盘）、Shell.toggleAgentActivityPanel（标题栏按钮）、QuickPanelHeightFor（内容驱动高度纯函数）
 * [POS]: Agent 活动的唯一浮层：托盘/标题栏打开，一层到底——点击 Agent 直达其面板并收起浮层，没有第二层详情；同一 Flyout 生命周期，锚定/贴边/翻转/关闭归框架
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// Floating Agent activity panel (0.7 §17.1/§17.2, 2026-10-14 revision): one
// MyGo flyout at a time — a system Popover on macOS — anchored beneath the
// menu-bar item or the titlebar's agent button. It takes the keyboard while
// shown and closes like a menu: outside click, focus moving away, app
// deactivation, or Escape. One shared StatusCenterSnapshot, a two-tab switch
// (Agents, Services) and, under Agents, the sidebar's compact agent cards
// grouped by state. There is one layer: a click lands on the Agent's own
// panel and closes the flyout. The height follows the content up to the
// fixed cap — beyond it the list scrolls inside.

// QuickPanelWidth is the §17.1 suggested panel width (360–420 DIP).
const QuickPanelWidth = 400

// QuickPanelHeight caps the content-driven height; the rows scroll inside
// beyond it. The flyout shrinks to fit a small work area on its own.
const QuickPanelHeight = 480

// quickPanelGap keeps breathing room between the anchor and the panel.
const quickPanelGap = 8

// quickPanelChromeHeight budgets the tab switch plus the list's own padding;
// quickPanelGroupHeight is one status header; quickPanelRowHeight is one
// agent card; quickPanelUsageHeight is the usage caption under a card that
// has one; quickPanelEmptyHeight is the empty state's budget;
// quickPanelOfflineHeight is the Reconnect row's. They are measured from the
// rendered layout (TestQuickPanelHeightHoldsTheContent keeps them honest).
const (
	quickPanelChromeHeight  = 52
	quickPanelGroupHeight   = 25
	quickPanelRowHeight     = 38
	quickPanelUsageHeight   = 17
	quickPanelEmptyHeight   = 72
	quickPanelOfflineHeight = 36
)

// quickPanelMinHeight keeps the panel presentable when it is nearly empty.
const quickPanelMinHeight = 160

// QuickPanelHeightFor returns the content-driven panel height: chrome plus
// one header per status group, one row per listed agent and one caption per
// row that carries a usage line, clamped between the minimum and the fixed
// cap (the panel sizes to content, it is not a fixed-height window).
func QuickPanelHeightFor(rows, groups, captions int) int {
	budget := quickPanelChromeHeight + groups*quickPanelGroupHeight +
		rows*quickPanelRowHeight + captions*quickPanelUsageHeight
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
// so no radius or border there). A two-tab switch sits on top — the tabs name
// the panel, so it has no title of its own — over the Agents list grouped by
// state or the Services tab. Escape closes through the flyout itself
// (§17.1) — the view deliberately does not take it.
func (s *Shell) QuickPanelView(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	tokens := designTokens(t.Dark)
	c.Root().Background(ui.Transparent)
	body := ui.Column(c).Fill()
	if !c.Vibrancy() {
		body = body.Background(tokens.Content)
		if runtime.GOOS != "windows" {
			body = body.Radius(Radius().Card).Border(1, tokens.BorderSubtle)
		}
	}
	body.Children(func() {
		ui.Row(c).FillWidth().Padding(sp.M, sp.M, sp.XS).Children(func() {
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
			ui.Row(c).FillWidth().Padding(sp.S, sp.M, 0).Gap(sp.S).AlignItems(ui.Center).Children(func() {
				ui.Text(c, "Herdr disconnected").FontSize(Typography().Caption).Grow(1).TextColor(t.TextMuted)
				if ui.Button(c, "Reconnect").Clicked() {
					s.reloadInstances(false)
				}
			})
		}
		entries := s.filterStatusEntries(s.BuildStatusCenterSnapshot())
		ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
			ui.Column(c).FillWidth().Padding(0, sp.S, sp.M).Children(func() {
				if len(entries) == 0 {
					emptyState(c, "No active agents", "")
					return
				}
				cards := make([]agent.AgentCardModel, len(entries))
				for i, entry := range entries {
					cards[i] = entry.Card
				}
				dupProjects := duplicateAgentProjectLabels(cards)
				for _, group := range quickPanelGroups(entries) {
					tone := group.Tone
					quickPanelGroupHeader(c, group.Label, len(group.Entries), &tone)
					for _, entry := range group.Entries {
						s.quickPanelEntryRow(c, entry, dupProjects)
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
	// the stale listener must not clear the replacement opened right here.
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
		}
	})
}

// closeQuickPanel closes the open panel. Nil-safe (headless sessions, tests,
// already-closed panel).
func (s *Shell) closeQuickPanel() {
	if s.quickPanel != nil {
		s.quickPanel.Close()
	}
}

// hideQuickPanel is the navigation path (a row click, a menu action): close
// the panel before landing, so the content does not flicker to another
// layout during the close animation.
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
// (§17.1): the fixed Services tab, or the Agents list's content-driven
// height (the Reconnect row included while Herdr is unreachable).
func (s *Shell) quickPanelContentHeight() int {
	if s.quickPanelTab == 1 {
		return QuickPanelHeight
	}
	entries := s.filterStatusEntries(s.BuildStatusCenterSnapshot())
	captions := 0
	for _, entry := range entries {
		if s.usageLineFor(entry.Card.Key) != "" {
			captions++
		}
	}
	height := QuickPanelHeightFor(len(entries), len(quickPanelGroups(entries)), captions)
	if s.offline {
		height += quickPanelOfflineHeight
	}
	return min(height, QuickPanelHeight)
}

// fitQuickPanel resizes the open flyout to the current content height; the
// framework re-places it against its anchor and the work area.
func (s *Shell) fitQuickPanel() {
	if s.quickPanel == nil {
		return
	}
	s.quickPanel.SetSize(QuickPanelWidth, s.quickPanelContentHeight())
}
