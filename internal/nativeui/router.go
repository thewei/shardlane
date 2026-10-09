package nativeui

/**
 * [INPUT]: 依赖 Shell 当前路由与 MyGo Router 单实例、History 页面及 Workspace 主表面
 * [OUTPUT]: 提供精确 History 归属策略、单一 Route View 和原生标题同步所需的标题
 * [POS]: 顶层页面路由所有者，控制一级页面身份，不重复实现内层 Surface Router
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"log/slog"
	"strings"

	"github.com/egoist/mygo/ui"
)

const (
	routeWorkspace = "/workspace"
	routeHistory   = "/history"
	routeSettings  = "/settings/general"
)

// isHistoryRoute is the one exact boundary for the History top-level view.
// Prefix matching without the slash would accidentally classify paths such
// as /history-old as History, despite the Router rendering Not Found.
func isHistoryRoute(path string) bool {
	return path == routeHistory || path == "/history-projects" ||
		strings.HasPrefix(path, routeHistory+"/")
}

func (s *Shell) routeView(c *ui.Context) {
	// Every route's content rides in a gorex card on the window gradient
	// (2026-10-06): the workspace route stages its own surfaces (terminal
	// pane cards, the diff content card), the others are hosted here. The
	// second-level views Chat/Changes live INSIDE /workspace as primary
	// surfaces (2026-10-07); History stays a route. The old /new-task and
	// /status-center pages are gone (2026-10-07 product decision).
	s.router.View(c, func(r *ui.Route) {
		switch {
		case r.Match(routeWorkspace):
			r.Title("Workspace")
			s.workspacePage(c)
		case r.Match(routeHistory):
			r.Title("History")
			gorexContentCard(c, gorexColorsOf(c.Theme().Dark), func() { s.historyPage(c) })
		case r.Match("/history/{id}"):
			r.Title("History Detail")
			gorexContentCard(c, gorexColorsOf(c.Theme().Dark), func() { s.historyDetailPage(c, r.Param("id")) })
		case r.Match("/inspector/{pane}"):
			r.Title("Agent Inspector")
			s.inspectorPage(c, r.Param("pane"))
		case r.Match("/history-projects"):
			r.Title("History by Project")
			gorexContentCard(c, gorexColorsOf(c.Theme().Dark), func() { s.historyProjectsPage(c) })
		case r.Match("/chat"):
			r.Title("Chat")
			s.chatPage(c)
		case r.Match("/settings/{section...}"):
			gorexContentCard(c, gorexColorsOf(c.Theme().Dark), func() { s.settingsLayout(c, r) })
		default:
			r.Title("Not Found")
			ui.Column(c).Grow(1).Center().Gap(8).Children(func() {
				ui.Text(c, "Page not found").Bold()
				if ui.Button(c, "Back to Workspace").Clicked() {
					s.router.Replace(routeWorkspace)
				}
			})
		}
	})
}

func (s *Shell) currentRouteTitle() string {
	switch path := s.router.Path(); {
	case path == routeWorkspace:
		return "Workspace"
	case path == routeHistory:
		return "History"
	case strings.HasPrefix(path, "/history/"):
		return "History Detail"
	case path == "/history-projects":
		return "History by Project"
	case strings.HasPrefix(path, "/inspector/"):
		return "Agent Inspector"
	case path == "/chat":
		return "Chat"
	case strings.HasPrefix(path, "/settings/"):
		return "Settings"
	default:
		return "Shardlane"
	}
}

func (s *Shell) syncRouteVisibility() {
	path := s.router.Path()
	if path == s.visibleRoute {
		return
	}
	wasWorkspace := s.visibleRoute == routeWorkspace
	nowWorkspace := path == routeWorkspace
	slog.Debug("native route changed", "from", s.visibleRoute, "to", path)
	s.visibleRoute = path
	s.syncWindowTitle()
	if wasWorkspace && !nowWorkspace {
		s.closeAllTerminals()
		return
	}
	if !wasWorkspace && nowWorkspace && s.activeInstance != "" {
		// Restore the prior surface only when its context is still valid;
		// otherwise land on Terminal (GWB-038).
		s.syncWorkspaceForSelection()
		if s.surface.current() == WorkspaceSurfaceTerminal {
			if err := s.syncTerminals(s.projection); err != nil {
				s.errText = err.Error()
				s.status = "Terminal attach failed"
			}
		}
	}
}
