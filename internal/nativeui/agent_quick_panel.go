package nativeui

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
)

/**
 * [INPUT]: 依赖共享 StatusCenterSnapshot/StatusCenterEntry、agentCardRow、surface_header 的 Chat/Terminal 记忆与 quick panel 的 Flyout 关闭
 * [OUTPUT]: 提供 Agent 活动浮层的分组（quickPanelGroups）、分组标题（quickPanelGroupHeader）、条目行（quickPanelEntryRow：点击直达对应面板、行尾「标记已审阅」、右键菜单）与 openQuickPanelEntry/openAgentTerminal 直达动作；另保留历史遗留的可点击统计
 * [POS]: quick_panel 的内容构件；状态由同一目录投影生成，点击不再进入第二层浮层，无第二套弹层窗口或运行时状态
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// quickPanelSummary turns passive counts into compact, actionable filters
// without introducing new state classifiers. Every tile uses the same
// StatusCenterSnapshot and filterChoice backing the segmented control.
func (s *Shell) quickPanelSummary(c *ui.Context, snapshot StatusCenterSnapshot) {
	sp := Spacing()
	ui.Row(c).FillWidth().Padding(sp.M, sp.L, 0).Gap(sp.S).Children(func() {
		s.quickPanelStat(c, "Attention", snapshot.NeedsAttention, ToneAttention, 1)
		s.quickPanelStat(c, "Review", snapshot.ReviewPending, ToneWarning, 2)
		s.quickPanelStat(c, "Working", snapshot.Working, ToneWorking, 3)
	})
}

func (s *Shell) quickPanelStat(c *ui.Context, label string, count int, tone StatusTone, filter int) {
	tokens := s.tokens(c.Theme().Dark)
	selected := s.workbench.filterChoice() == filter
	bg := tokens.Panel
	if selected || count > 0 {
		bg = tokens.StatusBackground(tone, c.Theme().Dark)
	}
	button := ui.ButtonBase(c).Grow(1).MinWidth(0).Radius(Radius().Row).
		Padding(Spacing().M).Background(bg).
		Border(1, tokens.BorderSubtle).Gap(4).
		Label(fmt.Sprintf("%s (%d)", label, count)).
		Tooltip("Show " + label + " agents")
	button.Children(func() {
		ui.Text(c, fmt.Sprintf("%d", count)).FontSize(Typography().Heading).
			FontWeight(WeightSemibold).TextColor(tokens.StatusColor(tone, c.Theme().Dark))
		ui.Text(c, label).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
	})
	if button.Clicked() {
		s.workbench.setFilterChoice(filter)
		s.fitQuickPanel()
	}
}

// quickPanelGroup is one status section of the Agents list: the entries of a
// directory bucket, under the label and dot colour that name the state.
type quickPanelGroup struct {
	Label   string
	Tone    StatusTone
	Entries []StatusCenterEntry
}

// quickPanelGroups splits the snapshot's entries into status sections in the
// directory's priority order — Needs attention, Review, Working, Idle —
// leaving out the empty ones. Each section keeps the snapshot's own order.
func quickPanelGroups(entries []StatusCenterEntry) []quickPanelGroup {
	groups := []quickPanelGroup{
		{Label: "Needs attention", Tone: ToneAttention},
		{Label: "Review", Tone: ToneSuccess},
		{Label: "Working", Tone: ToneWorking},
		{Label: "Idle", Tone: ToneMuted},
	}
	for _, entry := range entries {
		i := int(entry.Bucket)
		if i < 0 || i >= len(groups) {
			i = len(groups) - 1
		}
		groups[i].Entries = append(groups[i].Entries, entry)
	}
	kept := groups[:0]
	for _, group := range groups {
		if len(group.Entries) > 0 {
			kept = append(kept, group)
		}
	}
	return kept
}

// quickPanelGroupHeader is the panel's one section header: an optional state
// dot, the label and a count. The Services tab's sections wear it too, so
// both tabs read the same.
func quickPanelGroupHeader(c *ui.Context, label string, count int, dot *StatusTone) {
	t := c.Theme()
	sp := Spacing()
	ui.Row(c).FillWidth().Padding(sp.M, sp.M, sp.XS).Gap(sp.S).AlignItems(ui.Center).Children(func() {
		if dot != nil {
			ui.Box(c).Size(7, 7).Radius(3.5).Shrink(0).
				Background(designTokens(t.Dark).StatusColor(*dot, t.Dark))
		}
		ui.Text(c, label).FontSize(Typography().BodySmall).FontWeight(WeightMedium).
			TextColor(t.TextMuted).SingleLine()
		ui.Text(c, itoa(count)).FontSize(Typography().Caption).TextColor(t.TextMuted).SingleLine()
	})
}

// quickPanelEntryRow is one Agent in the panel: the shared card, a click that
// goes straight to the panel it belongs to (no second layer), the usage line
// beneath when the History meta has one, and the rest of the actions in the
// right-click menu. A row waiting for review carries its one quick action —
// mark reviewed — at the trailing edge.
func (s *Shell) quickPanelEntryRow(c *ui.Context, entry StatusCenterEntry, dupProjects map[string]bool) {
	t := c.Theme()
	card := entry.Card
	var row ui.Element
	ui.Box(c).FillWidth().Children(func() {
		row = s.agentCardRow(c, card, false, dupProjects, func() {
			if !card.ReviewPending {
				return
			}
			k := gorexColorsOf(t.Dark)
			if gorexIconButton(c, k, iconCheck, "Mark Reviewed", "Mark Reviewed", false, 20, 12).Clicked() {
				s.markAgentReviewed(card.Key)
				s.fitQuickPanel()
			}
		})
		// §17.2: the usage caption renders under the card when the cached
		// History-meta projection has facts, lined up with the card's text.
		if usage := s.usageLineFor(card.Key); usage != "" {
			ui.Box(c).Padding(0, 0, Spacing().XS, agentCardTextInset).Children(func() {
				ui.Text(c, usage).FontSize(Typography().Caption).
					TextColor(t.TextMuted).SingleLine()
			})
		}
	})
	if row.Clicked() {
		s.openQuickPanelEntry(entry)
	}
	row.ContextMenu(func(m *ui.Menu) { s.quickPanelEntryMenu(m, entry) })
}

// quickPanelEntryMenu holds what a row's click does not: the other panel, the
// inspector and the review mark.
func (s *Shell) quickPanelEntryMenu(m *ui.Menu, entry StatusCenterEntry) {
	card := entry.Card
	if m.Item("Open Terminal").Chosen() {
		s.hideQuickPanel()
		s.openAgentTerminal(card)
	}
	chat := "Open Chat"
	if entry.HasInteraction {
		chat = "Answer in Chat"
	}
	if m.Item(chat).Chosen() {
		s.hideQuickPanel()
		s.openChat(card)
	}
	if card.PaneID != "" && m.Item("Details").Chosen() {
		s.hideQuickPanel()
		s.router.Push("/inspector/" + card.PaneID)
	}
	if card.ReviewPending {
		m.Separator()
		if m.Item("Mark Reviewed").Chosen() {
			s.markAgentReviewed(card.Key)
			s.fitQuickPanel()
		}
	}
}

// openQuickPanelEntry is a row click: close the panel and land on the panel
// the Agent needs you at — its Chat when it waits on an interaction, its
// Terminal when it is blocked or failed, otherwise the surface you last used
// on that pane (the Terminal by default, as a sidebar click does).
func (s *Shell) openQuickPanelEntry(entry StatusCenterEntry) {
	card := entry.Card
	s.hideQuickPanel()
	switch statusCenterRecommendedDestination(entry) {
	case actionOpenChat:
		s.openChat(card)
	case agent.ActionOpenTerminal:
		s.openAgentTerminal(card)
	default:
		if s.paneSurfaceMemo[card.PaneID] == WorkspaceSurfaceChat {
			s.openChat(card)
			return
		}
		s.openAgentTerminal(card)
	}
}

// openAgentTerminal selects the Agent's pane and shows the Terminal — from
// whichever page or surface the panel was opened on.
func (s *Shell) openAgentTerminal(card agent.AgentCardModel) {
	s.rememberPaneSurface(card.PaneID, WorkspaceSurfaceTerminal)
	s.openAgentCard(card)
	s.showViewTerminal()
}
