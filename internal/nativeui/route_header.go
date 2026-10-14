package nativeui

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

/**
 * [INPUT]: 依赖 Shell 当前 Router 路径与 Workspace 表面、settingsSections 导航表、History 会话元数据、Files/Hosts 根路径与 headerActions
 * [OUTPUT]: 对外提供 routeHeaderSpec、Shell.routeHeader（Terminal/Chat 以外各页的标题栏身份）、Shell.routeHeaderBlock（单行大标题：灰色父级 › 标题 + Actions）
 * [POS]: 标题栏是每个页面标题与页面动作的唯一归属——内页只渲染内容，不再各自画页头（2026-10-14 产品决策）；内页标题统一是一行 Title 大字，没有 crumb 胶囊也没有第二行上下文，页面需要的路径/事实放在内容里或标题的悬停提示里；Terminal/Chat 仍是面包屑切换器
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// routeHeaderSpec is what the title bar shows on every page that is not a
// Terminal or Chat surface (whose identity is the project › tab › pane
// switcher): one large title — after a muted parent when the page opens
// from another — and the page's Actions menu. Pages render content only;
// a path or fact a page needs lives in its content, or in the title's
// tooltip, never in a second header row.
type routeHeaderSpec struct {
	parent   string           // the page this one opens from; empty for a top-level page
	onParent func()           // makes the parent a way back to it; nil leaves it plain text
	title    string           // the page itself
	titleTip string           // the full text on hover: the folder a page browses, the file it edits
	facts    string           // muted run after the title: what a session is about
	actions  func(m *ui.Menu) // the page's Actions pull-down; nil shows none
}

// routeHeader resolves the title bar's identity for the current route; the
// Terminal and Chat surfaces and unknown routes report false and keep their
// own bar.
func (s *Shell) routeHeader() (routeHeaderSpec, bool) {
	path := s.router.Path()
	switch {
	case path == routeWorkspace:
		// Changes is a page of its own over the workspace route.
		if _, _, changes, _ := s.viewSwitchActive(); changes {
			return routeHeaderSpec{title: "Changes", actions: s.surfaceOverflowItems}, true
		}
	case path == routeHistory || path == "/history-projects":
		return routeHeaderSpec{title: "Sessions"}, true
	case strings.HasPrefix(path, routeHistory+"/"):
		return s.sessionRouteHeader(strings.TrimPrefix(path, routeHistory+"/")), true
	case path == routeFiles:
		return s.filesRouteHeader(), true
	case path == routeHosts:
		return s.hostsRouteHeader(), true
	case strings.HasPrefix(path, "/settings/"):
		return routeHeaderSpec{parent: "Settings", title: settingsSectionFor(path).label}, true
	case strings.HasPrefix(path, "/inspector/"):
		return s.inspectorRouteHeader(strings.TrimPrefix(path, "/inspector/")), true
	}
	return routeHeaderSpec{}, false
}

// sessionRouteHeader names the open session in the bar: the Sessions parent
// leads back to the list, the facts run on after the title, and the
// session's action menu is the bar's Actions.
func (s *Shell) sessionRouteHeader(key string) routeHeaderSpec {
	spec := routeHeaderSpec{parent: "Sessions", onParent: s.showHistoryIndex, title: "Session"}
	meta := s.historyDetailMeta(key)
	if meta == nil {
		return spec
	}
	spec.title = fallbackText(meta.Title, history.Untitled)
	facts := []string{meta.ProjectName, meta.Agent.DisplayName()}
	if s.chatModel != "" && s.chatReadOnly && s.chatConversationID == key {
		facts = append(facts, s.chatModel)
	}
	spec.facts = strings.Join(nonEmptyStrings(facts), " · ")
	snapshot := *meta
	spec.actions = func(m *ui.Menu) { s.sessionMenu(m, snapshot, false) }
	return spec
}

// filesRouteHeader titles the Files page; the folder it browses is the
// title's tooltip, and a selected file adds the path actions to the bar's
// Actions menu.
func (s *Shell) filesRouteHeader() routeHeaderSpec {
	spec := routeHeaderSpec{title: "Files", titleTip: s.rightPanel.currentRoot}
	if selected := s.rightPanel.selectedFilePath; selected != "" {
		spec.actions = func(m *ui.Menu) { s.filesPathItems(m, selected) }
	}
	return spec
}

// hostsRouteHeader titles the Hosts page; the SSH config it indexes is the
// title's tooltip, and a selected host adds its action menu to the bar's
// Actions.
func (s *Shell) hostsRouteHeader() routeHeaderSpec {
	path := s.hosts.mainPath
	if path == "" {
		path = "~/.ssh/config"
	}
	spec := routeHeaderSpec{title: "Hosts", titleTip: path}
	if entry, ok := s.selectedHostEntry(); ok && !s.hosts.editing {
		spec.actions = func(m *ui.Menu) { s.hostEntryMenu(m, entry) }
	}
	return spec
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
	return routeHeaderSpec{title: title}
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

// routeHeaderBlock draws a route's title: one row of large type, the same
// size, weight and place on every page, so entering or leaving one never
// moves it. A parent reads as the muted lead-in (and, when it is a way back,
// takes the hover tint the breadcrumb's crumbs wear); a long title gives way
// with an ellipsis instead of pushing the Actions and the bar's tools out.
func (s *Shell) routeHeaderBlock(c *ui.Context, spec routeHeaderSpec) {
	t := c.Theme()
	k := gorexColorsOf(t.Dark)
	typ := Typography()
	sp := Spacing()
	ui.Row(c).FillWidth().MinWidth(0).Gap(sp.S).AlignItems(ui.Center).Children(func() {
		if spec.parent != "" {
			var parent ui.Element
			ui.Box(c).Shrink(0).Children(func() {
				if spec.onParent != nil {
					parent = ui.ButtonBase(c).Label(spec.parent)
				} else {
					parent = ui.Row(c).Label(spec.parent)
				}
				parent.Shrink(0).Radius(Radius().Row).Padding(0, sp.S)
				if spec.onParent != nil && parent.Hovered() {
					parent.Background(k.hover)
				}
				parent.Children(func() {
					ui.Text(c, spec.parent).FontSize(typ.Title).FontWeight(WeightMedium).
						TextColor(t.TextMuted).SingleLine()
				})
			})
			if spec.onParent != nil && parent.Clicked() {
				spec.onParent()
			}
			ui.Text(c, "›").FontSize(typ.Title).FontWeight(WeightMedium).TextColor(t.TextMuted).Shrink(0)
		}
		tip := spec.titleTip
		if tip == "" {
			tip = spec.title
		}
		ui.Text(c, spec.title).FontSize(typ.Title).FontWeight(WeightBold).
			SingleLine().Ellipsis("…").Shrink(1).MinWidth(0).Tooltip(tip)
		if spec.facts != "" {
			ui.Text(c, spec.facts).FontSize(typ.BodySmall).TextColor(t.TextMuted).
				SingleLine().Ellipsis("…").Shrink(2).MinWidth(0).Tooltip(spec.facts)
		}
		if spec.actions != nil {
			headerActions(c, spec.actions)
		}
	})
}

// headerDetailRow is the workspace breadcrumb's second row: a muted line (the
// selected Pane's directory) behind a small glyph. With nothing to say it
// keeps the row's height, so the crumbs above stay where they are.
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
