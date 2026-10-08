package nativeui

import (
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 mygo/ui 的 Context/TitleBar（窗口控件带几何）、gorex_style 的图标按钮与配色、router 路由态
 * [OUTPUT]: 对外提供 Shell.titlebar（窗口标题栏：红绿灯对齐、双行面包屑、工具区）、titlebarHeight（灯带对齐的行高纯函数）、showViewTerminal/showViewChat/showViewChanges/showViewHistory（四个视图的唯一切换入口）、toggleWindowPinned（窗口置顶的唯一入口）
 * [POS]: nativeui 的窗口 chrome 所有者，被 shell.View 顶部消费；高度与 MyGo 红灯带对齐（F142），视图切换、Agent 活动浮层入口与窗口置顶在此收口
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// The title bar follows the gorex header (2026-10-06): it floats on the
// window's background gradient with no fill of its own, the identity sits
// in a host-chip under the traffic lights, and the trailing controls are
// borderless icon buttons. gorex's terminal tab strip is deliberately not
// carried over — the sidebar owns navigation (2026-10-06 product note).
func (s *Shell) titlebar(c *ui.Context) {
	t := c.Theme()
	k := gorexColorsOf(t.Dark)
	bar := c.TitleBar()
	// Two rows on the workspace route (2026-10-05 round two): the
	// switcher path above, the selected Pane's path below. Compact chrome
	// (2026-10-06, second pass): one icon-button row fits in 34, the two
	// breadcrumb rows in 42. The row never grows past bar.Height (2026-10-06
	// annotation F142): the traffic lights sit centered inside exactly
	// bar.Height (MyGo places them btnHeight + 2×TrafficLightPosition.Y
	// tall), so centering this row's content in the same height puts every
	// control on the lights' horizontal line — any extra slack pushed the
	// icons visibly below the lights and left a dead band under the bar.
	height := titlebarHeight(bar.Height, s.router.Path() == routeWorkspace)
	workspace := s.router.Path() == routeWorkspace
	left := max(bar.Left, 12) + 4
	ui.Row(c).
		Height(height).
		Padding(0, max(bar.Right, 10), 0, left).
		Gap(8).
		DragWindow().
		AlignItems(ui.Center).
		Children(func() {
			// The header's leading cluster (2026-10-06 A6-A9): the sidebar
			// collapse toggle.
			sidebarLabel := "Show Sidebar"
			if !s.sidebarCollapsed {
				sidebarLabel = "Hide Sidebar"
			}
			if gorexIconButton(c, k, iconPanel, sidebarLabel, sidebarLabel+" (⌘B)", false, gorexIconBtn, 16).Clicked() {
				s.toggleSidebar()
			}

			if workspace {
				ui.Column(c).Grow(1).MinWidth(0).Children(func() {
					s.breadcrumbRow(c)
					s.crumbPathRow(c)
				})
			} else {
				ui.Text(c, s.currentRouteTitle()).FontWeight(600).Grow(1).
					SingleLine().TextColor(k.text)
			}
			s.titlebarTools(c, k)
		})
}

// titlebarHeight keeps the bar at the window controls' own height: MyGo
// centers the traffic lights inside exactly bar.Height (button height +
// 2× TrafficLightPosition.Y), so centering the bar's content in that same
// height lines every control up with the lights (2026-10-06 annotation
// F142). The workspace route needs at least 42 for its two breadcrumb
// rows; other routes at least 34 for one row.
func titlebarHeight(barHeight float32, workspace bool) float32 {
	if workspace {
		return max(barHeight, 42)
	}
	return max(barHeight, 34)
}

// titlebarTools is the header's trailing edge. The Terminal/Chat/Changes/
// History view switch renders on every route (2026-10-07 product note): the
// buttons never change or disappear on inner pages, so a view is always one
// click away and clicking the active view exits back to Terminal. After it
// come the Agent activity button (floating panel), the Right Panel toggle,
// and the window pin (always on top).
func (s *Shell) titlebarTools(c *ui.Context, k *gorexColors) {
	s.titlebarViewSwitch(c, k)
	s.titlebarActivity(c, k)
	panelLabel := "Show Right Panel"
	if s.rightPanel.open {
		panelLabel = "Hide Right Panel"
	}
	if gorexIconButton(c, k, iconPanel, panelLabel, panelLabel+" (⌥⌘B)", s.rightPanel.open, gorexIconBtn, 16).Clicked() {
		s.toggleRightPanel()
	}
	if gorexIconButton(c, k, iconPin, windowPinLabel, windowPinLabel, s.windowPinned, gorexIconBtn, 15).Clicked() {
		s.toggleWindowPinned()
	}
}

// windowPinLabel names the titlebar pin for accessibility and its tooltip;
// the selected face carries the pinned state, as the view switch does.
const windowPinLabel = "Keep Window on Top"

// toggleWindowPinned flips the main window's always-on-top level. The Shell
// field mirrors the level for the pin's face; the window is nil in headless
// sessions (tests), where the flip stays a no-op on the platform side.
func (s *Shell) toggleWindowPinned() {
	s.windowPinned = !s.windowPinned
	if s.win != nil {
		s.win.SetAlwaysOnTop(s.windowPinned)
	}
}

// titlebarViewSwitch is the persistent Terminal/Chat/Changes/History switch.
// Terminal is the first-level view; Chat, Changes and History are mutually
// exclusive second-level views, and repeating the active view's click exits
// back to Terminal (2026-10-07 product note). The labels carry the "view"
// suffix so they never collide with same-named content rows (the Settings
// "Terminal" nav section, the History page title) in accessibility and
// tests; the tooltips stay the short names.
func (s *Shell) titlebarViewSwitch(c *ui.Context, k *gorexColors) {
	onTerminal, onChat, onChanges, onHistory := s.viewSwitchActive()
	if gorexIconButton(c, k, iconTerminal, "Terminal view", "Terminal view", onTerminal, gorexIconBtn, 15).Clicked() {
		s.showViewTerminal()
	}
	if gorexIconButton(c, k, iconChat, "Chat view", "Chat (⇧⌘C)", onChat, gorexIconBtn, 15).Clicked() {
		s.showViewChat()
	}
	if gorexIconButton(c, k, iconChanges, "Changes view", "Changes", onChanges, gorexIconBtn, 15).Clicked() {
		s.showViewChanges()
	}
	if gorexIconButton(c, k, iconHistory, "History view", "History", onHistory, gorexIconBtn, 15).Clicked() {
		s.showViewHistory()
	}
}

// viewSwitchActive reports which of the four header views is on screen:
// the three workspace surfaces answer through the surface state machine,
// every /history* route counts as the History view.
func (s *Shell) viewSwitchActive() (terminal, chat, changes, history bool) {
	path := s.router.Path()
	if strings.HasPrefix(path, "/history") {
		return false, false, false, true
	}
	if path != routeWorkspace {
		return false, false, false, false
	}
	switch s.surface.current() {
	case WorkspaceSurfaceTerminal:
		terminal = true
	case WorkspaceSurfaceChat:
		chat = true
	case WorkspaceSurfaceDiff, WorkspaceSurfaceCommit:
		changes = true
	}
	return terminal, chat, changes, history
}

// showViewTerminal lands on the first-level Terminal view from anywhere.
func (s *Shell) showViewTerminal() {
	if s.router.Path() != routeWorkspace {
		s.router.Push(routeWorkspace)
	}
	s.showSurface(WorkspaceSurfaceTerminal)
}

// showViewChat toggles the Chat view: repeating the click exits to Terminal.
func (s *Shell) showViewChat() {
	if _, chat, _, _ := s.viewSwitchActive(); chat {
		s.showViewTerminal()
		return
	}
	if s.router.Path() != routeWorkspace {
		s.router.Push(routeWorkspace)
	}
	s.showSurface(WorkspaceSurfaceChat)
}

// showViewChanges toggles the Changes view: repeating the click exits to
// Terminal.
func (s *Shell) showViewChanges() {
	if _, _, changes, _ := s.viewSwitchActive(); changes {
		s.showViewTerminal()
		return
	}
	if s.router.Path() != routeWorkspace {
		s.router.Push(routeWorkspace)
	}
	s.openChanges("")
}

// showViewHistory toggles the History view: repeating the click exits back
// to Terminal.
func (s *Shell) showViewHistory() {
	if _, _, _, history := s.viewSwitchActive(); history {
		s.showViewTerminal()
		return
	}
	s.router.Push(routeHistory)
}

// crumbPathRow shows the selected Pane's working directory under the
// switcher path, as muted small text behind a folder glyph.
func (s *Shell) crumbPathRow(c *ui.Context) {
	t := c.Theme()
	cwd := s.selectedTabCWD()
	if cwd == "" {
		ui.Box(c).Height(2).Grow(1).PassThrough()
		return
	}
	ui.Row(c).Height(12).MinWidth(0).Gap(4).AlignItems(ui.Center).Tooltip(cwd).Children(func() {
		ui.Icon(c, iconFolder).Size(9, 9).TextColor(t.TextMuted).Shrink(0)
		ui.Text(c, cwd).FontSize(Typography().Micro).TextColor(t.TextMuted).SingleLine()
	})
}

// titlebarActivity is the Agent activity control (2026-10-07): a persistent
// agent-icon button on every route — the floating agent panel it opens is
// all Agent activity, so it never disappears when the agents are quiet. The
// actionable count floats at its top-right corner in the strongest tone;
// a Reconnect button replaces it while Herdr is unreachable.
func (s *Shell) titlebarActivity(c *ui.Context, k *gorexColors) {
	if s.shellState() == stateDisconnected {
		if ui.Button(c, "Reconnect").Clicked() {
			s.reloadInstances(false)
		}
		return
	}

	summary := s.agentActivitySummary()
	label := "Agent Activity"
	var tone StatusTone
	total := 0
	if summary != nil {
		parts := make([]string, 0, 3)
		if summary.NeedsAttention > 0 {
			parts = append(parts, strconv.Itoa(summary.NeedsAttention)+" attention")
		}
		if summary.ReviewPending > 0 {
			parts = append(parts, strconv.Itoa(summary.ReviewPending)+" review")
		}
		if summary.Working > 0 {
			parts = append(parts, strconv.Itoa(summary.Working)+" working")
		}
		if len(parts) > 0 {
			label = strings.Join(parts, " · ")
		}
		switch {
		case summary.NeedsAttention > 0:
			tone = ToneAttention
		case summary.ReviewPending > 0:
			tone = ToneSuccess
		case summary.Working > 0:
			tone = ToneWorking
		}
		total = summary.NeedsAttention + summary.ReviewPending + summary.Working
	}

	var anchor *ui.Element
	ui.Box(c).Size(gorexIconBtn+3, gorexIconBtn+1).Children(func() {
		// The trigger's accessible label carries the actionable summary
		// when there is one ("1 attention · 1 review"), the plain name
		// otherwise; the badge floats the total at the top-right corner.
		anchor = gorexIconButton(c, k, iconAgentBot, label, label, false, gorexIconBtn, 16)
		if total > 0 {
			ui.Box(c).Absolute().Right(0).Top(0).Children(func() {
				dark := c.Theme().Dark
				textColor := ui.RGBA(255, 255, 255, 1)
				if dark {
					textColor = ui.Hex("#1d1d1f")
				}
				badge := ui.Box(c).MinWidth(13).Height(13).Padding(0, 3).Center().
					Radius(Radius().Row).
					Background(designTokens(dark).StatusColor(tone, dark))
				badge.Children(func() {
					ui.Text(c, strconv.Itoa(total)).FontSize(9).FontWeight(700).TextColor(textColor)
				})
			})
		}
	})
	if anchor.Clicked() {
		s.toggleAgentActivityPanel(anchor)
	}
}

// agentActivitySummary is the actionable summary behind the titlebar badge,
// or nil when the reconciled directory has not loaded yet.
func (s *Shell) agentActivitySummary() *StatusCenterSnapshot {
	if s.workbench.directory == nil {
		return nil
	}
	snapshot := s.BuildStatusCenterSnapshot()
	return &snapshot
}
