package nativeui

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
)

/**
 * [INPUT]: 依赖 gorex 图标按钮、showViewTerminal/showViewChat 切换、pane 卡头范式（paneCardHeader）
 * [OUTPUT]: 提供 surfaceToggle（Terminal⇄Chat 单一形态切换）、chatSurfaceHeader（Chat 卡统一头：agent 图标/名/状态点/双击 zoom）、paneSurfaceMemo 记忆与 openAgentFromSidebar 直跳
 * [POS]: N19 UI 批：切换按钮从窗口头移入两种卡片头并合一；侧栏 Agent 点击按记忆的表面直跳
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// surfaceToggle renders the single Terminal⇄Chat switch — one button, two
// shapes (N19 #7): the icon names the surface it lands on. It replaced the
// Terminal/Chat pair in the window header and lives in both card headers.
func (s *Shell) surfaceToggle(c *ui.Context, k *gorexColors, from WorkspaceSurfaceKind, paneID string) {
	if from == WorkspaceSurfaceChat {
		if gorexIconButton(c, k, iconTerminal, "Terminal", "Switch to Terminal", false, gorexIconBtn, 15).Clicked() {
			s.rememberPaneSurface(paneID, WorkspaceSurfaceTerminal)
			s.showViewTerminal()
		}
		return
	}
	if gorexIconButton(c, k, iconChat, "Chat", "Switch to Chat (⇧⌘C)", false, gorexIconBtn, 15).Clicked() {
		s.rememberPaneSurface(paneID, WorkspaceSurfaceChat)
		s.showViewChat()
	}
}

// rememberPaneSurface records which surface a pane was last used on, so a
// sidebar Agent click lands where the user left it (N19 #4).
func (s *Shell) rememberPaneSurface(paneID string, kind WorkspaceSurfaceKind) {
	if paneID == "" {
		return
	}
	if s.paneSurfaceMemo == nil {
		s.paneSurfaceMemo = map[string]WorkspaceSurfaceKind{}
	}
	s.paneSurfaceMemo[paneID] = kind
}

// openAgentFromSidebar is the sidebar Agent click (N19 #4): jump straight
// to the pane's remembered surface — Chat when the user left it in Chat,
// Terminal otherwise.
func (s *Shell) openAgentFromSidebar(card agent.AgentCardModel) {
	if s.paneSurfaceMemo[card.PaneID] == WorkspaceSurfaceChat {
		s.openChat(card)
		return
	}
	s.rememberPaneSurface(card.PaneID, WorkspaceSurfaceTerminal)
	s.openAgentCard(card)
}

// chatSurfaceHeader is the Chat card's header, unified with the terminal
// pane card (N19 #3/#8): the Agent's brand mark and name, the live status
// dot, the chat-specific disposition, and the single surface toggle.
// Double-click zooms exactly like the terminal card (N19 #6).
func (s *Shell) chatSurfaceHeader(c *ui.Context, k *gorexColors) {
	dark := c.Theme().Dark
	h := ui.Row(c).Height(gorexHeaderH).Padding(0, gorexGap, 0, 12).Gap(7).AlignItems(ui.Center).MinWidth(0)
	if h.DoubleClicked() && s.chatPaneID != "" {
		s.zoomPaneAction(s.chatPaneID)
	}
	h.Children(func() {
		mark := visualMark{svg: iconChat}
		title := "Agent Chat"
		if s.chatPaneID != "" {
			if m, ok := s.agentMarkForPane(s.chatPaneID, dark); ok {
				mark = m
			}
			if a := s.paneAgent(s.chatPaneID); a != nil && a.Name != "" {
				title = a.Name
			} else if card, hasCard := s.agentCardForPane(s.chatPaneID); hasCard {
				title = card.Title
			}
		}
		ui.Box(c).Size(15, 15).Shrink(0).Children(func() {
			markView(c, mark, 14.5, k.text)
		})
		ui.Text(c, title).FontSize(12.5).FontWeight(600).TextColor(k.text).
			SingleLine().Ellipsis("…").Shrink(0).MaxWidthPercent(60)
		s.agentStatusDotForPane(c, k, s.chatPaneID)
		if _, hasCard := s.agentCardForPane(s.chatPaneID); hasCard && s.chatConversationID != "" {
			statusPill(c, s.chatDispositionSummary(), dispositionTone(s.chatDispositionNow()))
		}
		ui.Spacer(c)
		s.surfaceToggle(c, k, WorkspaceSurfaceChat, s.chatPaneID)
	})
}

// agentStatusDotForPane renders the pane's Agent state as the pane card's
// status dot (chat header twin of paneStatusDot).
func (s *Shell) agentStatusDotForPane(c *ui.Context, k *gorexColors, paneID string) {
	a := s.paneAgent(paneID)
	if a == nil || a.Status == "" {
		return
	}
	switch normalizeRuntimeStatus(a.Status) {
	case opWorking:
		gorexActivityDot(c, k.busy, 6).Tooltip("Working")
	case opReadyForReview:
		gorexActivityDot(c, k.busy, 6).Tooltip("Ready for review")
	case opNeedsAttention:
		gorexAttentionDot(c, k.attention, "Needs attention")
	}
}
