package nativeui

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
)

/**
 * [INPUT]: 依赖共享 StatusCenterSnapshot、AgentCardModel 与 QuickPanel 原生窗口锚点
 * [OUTPUT]: 提供 Agent Activity 可点击统计、单 Agent 详情浮层和 Sidebar Agent 打开动作
 * [POS]: quick_panel 的可复用详情/概览内容；状态由同一目录投影生成，无第二套弹层窗口或运行时状态
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
		ui.Text(c, fmt.Sprintf("%d", count)).FontSize(Typography().Section + 4).
			FontWeight(700).TextColor(tokens.StatusColor(tone, c.Theme().Dark))
		ui.Text(c, label).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
	})
	if button.Clicked() {
		s.workbench.setFilterChoice(filter)
		s.positionQuickPanel()
	}
}

// openAgentQuickPanel anchors the existing status/tray popup beside the
// selected sidebar card. The headless/native-unavailable fallback retains
// the existing Open Agent behavior, without creating a second window.
func (s *Shell) openAgentQuickPanel(anchor ui.Element, card agent.AgentCardModel) {
	if s.quickPanel == nil || s.win == nil {
		s.openAgentCard(card)
		return
	}
	if s.quickPanel.IsVisible() && s.quickPanelAgent != nil && *s.quickPanelAgent == card.Key {
		s.hideQuickPanel()
		return
	}
	key := card.Key
	s.quickPanelAgent = &key
	rect := anchor.Bounds()
	win := s.win.Bounds()
	s.showQuickPanelAnchored(QuickRect{
		X: win.X + int(rect.X), Y: win.Y + int(rect.Y),
		Width: int(rect.W), Height: int(rect.H),
	})
}

// agentQuickPanelDetail uses an identity fence on every paint. If the Agent
// disappears between the click and rendering, no stale action is offered.
func (s *Shell) agentQuickPanelDetail(c *ui.Context, snapshot StatusCenterSnapshot) {
	t := c.Theme()
	tokens := s.tokens(t.Dark)
	sp := Spacing()
	ui.Scroll(c).Grow(1).Children(func() {
		ui.Column(c).FillWidth().Padding(sp.L).Gap(sp.L).
			Background(tokens.Content).Children(func() {
			if ui.Button(c, "← All Agents").Clicked() {
				s.quickPanelAgent = nil
				s.positionQuickPanel()
				s.invalidateQuickPanel()
			}
			var card *agent.AgentCardModel
			var needsInteraction bool
			for _, entry := range snapshot.Entries {
				if s.quickPanelAgent != nil && entry.Card.Key == *s.quickPanelAgent {
					value := entry.Card
					card = &value
					needsInteraction = entry.HasInteraction
					break
				}
			}
			if card == nil {
				ui.Text(c, "Agent unavailable").FontSize(Typography().Section).Bold()
				ui.Text(c, "This Agent is no longer present in the current session.").FontSize(Typography().BodySmall).TextColor(t.TextMuted)
				return
			}
			ui.Column(c).FillWidth().Gap(5).Children(func() {
				ui.Row(c).Gap(sp.S).AlignItems(ui.Center).Children(func() {
					markView(c, agentMark(card.Provider, t.Dark), 22, t.TextMuted)
					ui.Text(c, fallbackText(card.Title, card.ProjectName, "Agent")).FontSize(Typography().Section + 2).
						FontWeight(700).Grow(1).SingleLine().Tooltip(card.Title)
				})
				ui.Text(c, fmt.Sprintf("%s · %s", card.Provider.DisplayName(), agent.OperationalLabel(card.Attention))).
					FontSize(Typography().BodySmall).TextColor(t.TextMuted)
			})
			settingsCard(c, "Context", func() {
				if card.ProjectName != "" {
					historyFact(c, "Project", card.ProjectName, "")
				}
				historyFact(c, "Runtime", string(card.RuntimePhase), "")
				if card.Unread {
					historyFact(c, "Activity", "Unread update", "")
				}
				if card.ReviewPending {
					historyFact(c, "Review", "Waiting for review", "")
				}
				if usage := s.usageLineFor(card.Key); usage != "" {
					historyFact(c, "Usage", usage, "")
				}
			})
			ui.Row(c).FillWidth().Gap(sp.S).Children(func() {
				if ui.Button(c, "Open Terminal").Clicked() {
					s.hideQuickPanel()
					s.openAgentCard(*card)
				}
				chatAction := "Open Chat"
				if needsInteraction {
					chatAction = "Answer in Chat"
				}
				if ui.Button(c, chatAction).Clicked() {
					s.hideQuickPanel()
					s.openChat(*card)
				}
			})
			ui.Row(c).FillWidth().Gap(sp.S).Children(func() {
				if card.PaneID != "" && ui.Button(c, "Inspect Agent").Clicked() {
					s.hideQuickPanel()
					s.router.Push("/inspector/" + card.PaneID)
				}
				if card.ReviewPending && ui.Button(c, "Mark Reviewed").Clicked() {
					s.markAgentReviewed(card.Key)
					s.invalidateQuickPanel()
				}
			})
		})
	})
}
