package nativeui

import (
	"runtime"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 mygo/ui 的 Context/TitleBar（窗口控件带几何）、gorex_style 的图标按钮与配色、router 路由态、runtime/time
 * [OUTPUT]: 对外提供 Shell.titlebar（窗口标题栏：红绿灯对齐、身份区——Terminal/Chat 为面包屑 + cwd 双行，Changes 与各内页为 route_header.go 的单行 Title 大字标题、工具区、Windows 控件）、titlebarHeight（灯带对齐的行高纯函数，所有路由同一 42 下限）、showViewTerminal/showViewChat/showViewChanges/showViewHistory（四个视图的唯一切换入口）、toggleWindowPinned（窗口置顶的唯一入口）
 * [POS]: nativeui 的窗口 chrome 所有者，被 shell.View 顶部消费；高度与 MyGo 红灯带对齐（F142），页面标题与页面动作的唯一归属（2026-10-14：内页只画内容），视图切换、Agent 活动浮层入口与窗口置顶在此收口
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// The content column's title bar (2026-10-12): the sidebar owns the left of
// the window top to bottom, so this bar only spans the content — the page's
// identity and the trailing tools, on the canvas color with no fill of its
// own. The identity is the one place a page is named (2026-10-14): the
// Terminal and Chat show their switcher path over the selected Pane's
// directory, Changes and every other route one large title with its Actions
// menu (route_header.go). Pages render content only, so no two pages open
// with a different header of their own.
func (s *Shell) titlebar(c *ui.Context) {
	t := c.Theme()
	k := gorexColorsOf(t.Dark)
	bar := c.TitleBar()
	// The row never grows past bar.Height (2026-10-06 annotation F142): the
	// traffic lights sit centered inside exactly bar.Height (MyGo places
	// them btnHeight + 2×TrafficLightPosition.Y tall), so centering this
	// row's content in the same height puts every control on the lights'
	// horizontal line.
	height := titlebarHeight(bar.Height)
	// While the sidebar slides shut the content column reaches the window's
	// left edge: its content then clears the traffic lights and the toggle.
	left := max(gorexGap*2, sidebarToggleClearance(bar)-s.sidebarSlideW)
	ui.Row(c).
		Height(height).
		Padding(0, max(bar.Right, 12), 0, left).
		Gap(8).
		DragWindow().
		AlignItems(ui.Center).
		Children(func() {
			if spec, ok := s.routeHeader(); ok {
				ui.Column(c).Grow(1).MinWidth(0).Children(func() {
					s.routeHeaderBlock(c, spec)
				})
			} else if s.router.Path() == routeWorkspace {
				ui.Column(c).Grow(1).MinWidth(0).Children(func() {
					s.breadcrumbRow(c)
					s.crumbPathRow(c)
				})
			} else {
				ui.Box(c).Grow(1).PassThrough()
			}
			s.titlebarTools(c, k)
		})
}

// sidebarToggleX is where the toggle sits: right after the traffic lights.
func sidebarToggleX(bar ui.TitleBar) float32 { return max(bar.Left, 12) + 4 }

// sidebarToggleClearance is how far from the window's left edge the content
// must start to leave the lights and the toggle uncovered.
func sidebarToggleClearance(bar ui.TitleBar) float32 {
	return sidebarToggleX(bar) + gorexIconBtn + 8
}

// sidebarBand is the sidebar's top inset: the strip beside the traffic
// lights, draggable like the rest of the title bar.
func (s *Shell) sidebarBand(c *ui.Context) {
	ui.Box(c).FillWidth().Shrink(0).Height(max(c.TitleBar().Height, 38)).DragWindow()
}

// sidebarToggle is the one show/hide control of the sidebar. It floats at a
// fixed spot after the traffic lights and never moves with the slide, so the
// pointer always finds it where it was.
func (s *Shell) sidebarToggle(c *ui.Context) {
	k := gorexColorsOf(c.Theme().Dark)
	bar := c.TitleBar()
	label := "Show Sidebar"
	if !s.sidebarCollapsed {
		label = "Hide Sidebar"
	}
	y := (max(bar.Height, 38) - gorexIconBtn) / 2
	ui.Box(c).Absolute().Left(sidebarToggleX(bar)).Top(y).Children(func() {
		if gorexIconButton(c, k, iconPanel, label, label+" (⌘B)", false, gorexIconBtn, gorexIconGlyph).Clicked() {
			s.toggleSidebar()
		}
	})
}

// titlebarHeight keeps the bar at the window controls' own height: MyGo
// centers the traffic lights inside exactly bar.Height (button height +
// 2× TrafficLightPosition.Y), so centering the bar's content in that same
// height lines every control up with the lights (2026-10-06 annotation
// F142). The 42 floor is the same on every route, so the identity never
// moves between pages.
func titlebarHeight(barHeight float32) float32 {
	return max(barHeight, 42)
}

// titlebarTools is the header's trailing edge: the Agent activity button
// (floating panel), the window pin (always on top) and, on Windows, the
// window controls. The view switch (Changes/Sessions/Files/Hosts) lives at
// the top of the sidebar (sidebar.go sidebarViewTabs).
func (s *Shell) titlebarTools(c *ui.Context, k *gorexColors) {
	s.titlebarActivity(c, k)
	if gorexIconButton(c, k, iconPin, windowPinLabel, windowPinLabel, s.windowPinned, gorexIconBtn, gorexIconGlyph).Clicked() {
		s.toggleWindowPinned()
	}
	s.windowControls(c, k)
}

// ownControls indicates that the title bar draws custom window controls: on
// Windows, where native controls do not blend with the gorex gradient header.
var ownControls = runtime.GOOS == "windows"

// closeRed is Windows 11's close button hover face.
var closeRed = ui.Hex("#c42b1c")

// windowControls draws minimize, maximize/restore, and close buttons on platforms
// without macOS traffic lights (such as Windows).
func (s *Shell) windowControls(c *ui.Context, k *gorexColors) {
	win := s.win
	if !ownControls || win == nil || win.IsFullScreen() {
		return
	}
	maximized := win.IsMaximized()
	ui.Row(c).Gap(2).AlignItems(ui.Center).Shrink(0).Children(func() {
		ui.Box(c).Size(1, 16).Margin(0, 6).Background(k.textFaint.Alpha(0.25))
		if gorexIconButton(c, k, iconMinus, "Minimize", "Minimize", false, gorexIconBtn, gorexIconGlyph).Clicked() {
			win.Minimize()
		}
		glyph, label := iconSquare, "Maximize"
		if maximized {
			glyph, label = iconCopy, "Restore"
		}
		if gorexIconButton(c, k, glyph, label, label, false, gorexIconBtn, 13).Clicked() {
			if maximized {
				win.Unmaximize()
			} else {
				win.Maximize()
			}
		}
		b := ui.Box(c).Size(gorexIconBtn, gorexIconBtn).Center().Radius(gorexIconBtn / 2.6).
			Cursor(ui.CursorPointer).Role(ui.RoleButton).Label("Close").Tooltip("Close")
		col := k.iconMuted
		if b.Pressed() {
			b.Background(closeRed.Alpha(0.8))
			col = ui.Hex("#ffffff")
		} else if b.Hovered() {
			b.Background(closeRed)
			col = ui.Hex("#ffffff")
		}
		b.Transition(ui.ElementTransition{Colors: true, Duration: 120 * time.Millisecond})
		b.Children(func() {
			ui.Icon(c, iconClose).Size(16, 16).TextColor(col)
		})
		if b.Clicked() {
			win.Close()
		}
	})
}

// windowPinLabel names the titlebar pin for accessibility and its tooltip;
// the selected face carries the pinned state, as the sidebar's view tabs do.
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

// viewSwitchActive reports which of the four header views is on screen:
// the three workspace surfaces answer through the surface state machine,
// every /history* route counts as the History view.
func (s *Shell) viewSwitchActive() (terminal, chat, changes, history bool) {
	path := s.router.Path()
	if isHistoryRoute(path) {
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
	if s.holdCommitNavigation() {
		return
	}
	if s.router.Path() != routeWorkspace {
		s.router.Push(routeWorkspace)
	}
	s.showSurface(WorkspaceSurfaceTerminal)
}

// showViewChat toggles the Chat view: repeating the click exits to Terminal.
func (s *Shell) showViewChat() {
	if s.holdCommitNavigation() {
		return
	}
	if _, chat, _, _ := s.viewSwitchActive(); chat {
		s.showViewTerminal()
		return
	}
	// Check before changing the route: Settings/History should never be
	// replaced by an empty Chat because a plain shell is selected.
	if !s.workspaceChatAvailable() {
		s.chatUnavailableNotice()
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
	fromCommit := s.surface.current() == WorkspaceSurfaceCommit
	if s.holdCommitNavigation() {
		return
	}
	if fromCommit {
		// Closing a pristine Commit dialog restores the previous Diff.
		// Do not interpret that transition as a second Changes click and
		// unexpectedly switch all the way to Terminal.
		return
	}
	if _, _, changes, _ := s.viewSwitchActive(); changes {
		s.showViewTerminal()
		return
	}
	if s.router.Path() != routeWorkspace {
		s.router.Push(routeWorkspace)
	}
	s.openChanges("")
}

// showHistoryIndex is the single parent-navigation intent for History.
// Nested History pages never fall back to the previous unrelated route.
func (s *Shell) showHistoryIndex() {
	if s.router.Path() == routeHistory {
		return
	}
	s.router.Push(routeHistory)
}

// showViewHistory: from another page open the History index; from a nested
// History route return to the index; only from the index itself toggle back
// to Terminal (consistent with the header's "active view" toggle contract).
func (s *Shell) showViewHistory() {
	if s.holdCommitNavigation() {
		return
	}
	if s.router.Path() == routeHistory {
		s.showViewTerminal()
		return
	}
	s.showHistoryIndex()
}

// crumbPathRow shows the selected Pane's working directory under the
// switcher path, as muted small text behind a folder glyph.
func (s *Shell) crumbPathRow(c *ui.Context) {
	cwd := s.selectedTabCWD()
	if cwd == "" {
		headerDetailRow(c, nil, "", "")
		return
	}
	headerDetailRow(c, nil, shortDir(cwd), cwd)
}

// titlebarActivity is the Agent activity control (2026-10-07): a persistent
// agent-icon button on every route — the floating agent panel it opens is
// all Agent activity, so it never disappears when the agents are quiet. The
// actionable count floats at its top-right corner in the strongest tone;
// a Reconnect button replaces it while Herdr is unreachable.
func (s *Shell) titlebarActivity(c *ui.Context, k *gorexColors) {
	// The Agent button also owns Services. It must stay available when
	// Herdr disconnects so file-free service info and reconnect remain
	// discoverable instead of replacing the entire destination.
	disconnected := s.shellState() == stateDisconnected
	summary := s.agentActivitySummary()
	if disconnected {
		summary = nil
	}
	label := "Agents & Services"
	if disconnected {
		label = "Agents & Services · Disconnected"
	}
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

	var anchor ui.Element
	ui.Box(c).Size(gorexIconBtn+3, gorexIconBtn+1).Children(func() {
		// The trigger's accessible label carries the actionable summary
		// when there is one ("1 attention · 1 review"), the plain name
		// otherwise; the badge floats the total at the top-right corner.
		anchor = gorexIconButton(c, k, iconActivity, label, label, false, gorexIconBtn, gorexIconGlyph)
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
					ui.Text(c, strconv.Itoa(total)).FontSize(Typography().Micro).FontWeight(WeightSemibold).TextColor(textColor)
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
