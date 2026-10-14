package nativeui

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

/**
 * [INPUT]: 依赖 Shell 当前 Router 路径、settingsSections 导航表、History 会话元数据、Files/Hosts 根路径与 crumbButton/headerActions
 * [OUTPUT]: 对外提供 routeHeaderSpec、Shell.routeHeader（非 Workspace 路由的标题栏身份）、Shell.routeHeaderBlock（与 Workspace 面包屑同一版式的两行标题块）、headerDetailRow（第二行弱化的上下文）
 * [POS]: 标题栏是每个页面标题与页面动作的唯一归属——内页只渲染内容，不再各自画页头（2026-10-14 产品决策）；进出内页时标题行落在同一位置
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// routeHeaderSpec is what the title bar shows on a route that is not the
// workspace (whose identity is the project › tab › pane switcher): the
// crumbs, the muted context line under them and the page's Actions menu.
// Pages render content only — the title, the path it is about and the
// actions that used to open each page differently all live here.
type routeHeaderSpec struct {
	parent    crumbSegment // the page this one opens from; an empty label means top-level
	onParent  func()       // makes the parent crumb a way back to it; nil leaves it plain text
	current   crumbSegment // the page itself, in the white pill
	detail    string       // muted context line (a path, a session's facts); empty keeps the line's room
	detailTip string       // the full text when detail is shortened
	// detailGlyph leads the context line; nil is the folder glyph the
	// workspace path line uses.
	detailGlyph *ui.SVG
	actions     func(m *ui.Menu) // the page's Actions pull-down; nil shows none
}

// routeHeader resolves the title bar's identity for the current route; the
// workspace route and unknown routes report false and keep their own bar.
func (s *Shell) routeHeader(dark bool) (routeHeaderSpec, bool) {
	path := s.router.Path()
	switch {
	case path == routeHistory || path == "/history-projects":
		return routeHeaderSpec{current: crumbSegment{label: "Sessions", mark: visualMark{svg: iconHistory}}}, true
	case strings.HasPrefix(path, routeHistory+"/"):
		return s.sessionRouteHeader(strings.TrimPrefix(path, routeHistory+"/"), dark), true
	case path == routeFiles:
		return s.filesRouteHeader(), true
	case path == routeHosts:
		return s.hostsRouteHeader(), true
	case strings.HasPrefix(path, "/settings/"):
		section := settingsSectionFor(path)
		return routeHeaderSpec{
			parent:  crumbSegment{label: "Settings", mark: visualMark{svg: iconSettings}},
			current: crumbSegment{label: section.label, mark: visualMark{svg: section.icon}},
		}, true
	case strings.HasPrefix(path, "/inspector/"):
		return s.inspectorRouteHeader(strings.TrimPrefix(path, "/inspector/")), true
	}
	return routeHeaderSpec{}, false
}

// sessionRouteHeader names the open session in the bar: the crumb leads back
// to the Sessions list, the context line carries its facts, and the session's
// action menu is the bar's Actions.
func (s *Shell) sessionRouteHeader(key string, dark bool) routeHeaderSpec {
	spec := routeHeaderSpec{
		parent:   crumbSegment{label: "Sessions", mark: visualMark{svg: iconHistory}},
		onParent: s.showHistoryIndex,
		current:  crumbSegment{label: "Session", mark: visualMark{svg: iconHistory}},
	}
	meta := s.historyDetailMeta(key)
	if meta == nil {
		return spec
	}
	spec.current = crumbSegment{label: fallbackText(meta.Title, history.Untitled), mark: agentMark(meta.Agent, dark)}
	facts := []string{meta.ProjectName, meta.Agent.DisplayName()}
	if s.chatModel != "" && s.chatReadOnly && s.chatConversationID == key {
		facts = append(facts, s.chatModel)
	}
	spec.detail = strings.Join(nonEmptyStrings(facts), " · ")
	snapshot := *meta
	spec.actions = func(m *ui.Menu) { s.sessionMenu(m, snapshot, false) }
	return spec
}

// filesRouteHeader titles the Files page with the folder it browses; a
// selected file adds the path actions to the bar's Actions menu.
func (s *Shell) filesRouteHeader() routeHeaderSpec {
	spec := routeHeaderSpec{current: crumbSegment{label: "Files", mark: visualMark{svg: iconFolder}}}
	if root := s.rightPanel.currentRoot; root != "" {
		spec.detail, spec.detailTip = shortDir(root), root
	}
	if selected := s.rightPanel.selectedFilePath; selected != "" {
		spec.actions = func(m *ui.Menu) { s.filesPathItems(m, selected) }
	}
	return spec
}

// hostsRouteHeader titles the Hosts page with the SSH config it indexes.
func (s *Shell) hostsRouteHeader() routeHeaderSpec {
	path := s.hosts.mainPath
	if path == "" {
		path = "~/.ssh/config"
	}
	return routeHeaderSpec{
		current:     crumbSegment{label: "Hosts", mark: visualMark{svg: iconGlobe}},
		detail:      compactSearchPath(path),
		detailTip:   path,
		detailGlyph: iconFile,
	}
}

// inspectorRouteHeader titles the Agent inspector with the agent it shows.
func (s *Shell) inspectorRouteHeader(paneID string) routeHeaderSpec {
	title := "Agent Inspector"
	for _, card := range s.workbenchCards() {
		if card.PaneID == paneID {
			title = fallbackText(card.Title, title)
			break
		}
	}
	return routeHeaderSpec{current: crumbSegment{label: title, mark: visualMark{svg: iconAgent}}}
}

// settingsSectionFor resolves a /settings/... path to its navigation entry,
// General for the unknown ones (the page the layout falls back to).
func settingsSectionFor(path string) settingsSection {
	for _, section := range settingsSections {
		if section.path == path {
			return section
		}
	}
	return settingsSections[0]
}

// routeHeaderBlock draws a route's title block in the workspace breadcrumb's
// own two-row shape: the crumbs (and the page's Actions) above, the muted
// context line below. The title row therefore sits exactly where the
// workspace breadcrumb sits, so entering or leaving a page never moves it.
func (s *Shell) routeHeaderBlock(c *ui.Context, spec routeHeaderSpec) {
	ui.Row(c).Height(20).FillWidth().MinWidth(0).Gap(2).AlignItems(ui.Center).Children(func() {
		if spec.parent.label != "" {
			parent := crumbButton(c, spec.parent, crumbOpts{plain: spec.onParent == nil})
			if spec.onParent != nil && parent.Clicked() {
				spec.onParent()
			}
		}
		crumbButton(c, spec.current, crumbOpts{last: true, fit: true, plain: true})
		if spec.actions != nil {
			headerActions(c, spec.actions)
		}
	})
	headerDetailRow(c, spec.detailGlyph, spec.detail, spec.detailTip)
}

// headerDetailRow is the title bar's second row: a muted line (a path, a
// session's facts) behind a small glyph. With nothing to say it keeps the
// row's height, so the title above stays where it is on every route.
func headerDetailRow(c *ui.Context, glyph *ui.SVG, text, tooltip string) {
	if text == "" {
		ui.Box(c).Height(12).PassThrough()
		return
	}
	if glyph == nil {
		glyph = iconFolder
	}
	if tooltip == "" {
		tooltip = text
	}
	t := c.Theme()
	ui.Row(c).Height(12).MinWidth(0).Gap(4).AlignItems(ui.Center).Tooltip(tooltip).Children(func() {
		ui.Icon(c, glyph).Size(9, 9).TextColor(t.TextMuted).Shrink(0)
		ui.Text(c, text).FontSize(Typography().Micro).TextColor(t.TextMuted).SingleLine().
			Ellipsis("…").Shrink(1).MinWidth(0)
	})
}
