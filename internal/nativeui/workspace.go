/**
 * [INPUT]: 依赖 mygo/ui, internal/agent, WorkspacePrimarySurface 状态
 * [OUTPUT]: 对外提供 workspacePage, agentCardForPane
 * [POS]: Terminal/Chat/Diff/Commit 唯一主表面切换中心
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
package nativeui

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
)

// workspacePage renders the /workspace center through the single
// WorkspacePrimarySurface owner: Terminal, Chat, Diff, or Commit state.
// Commit displays Diff under its native modal editor; no Terminal frame or
// second Git transaction can receive input while the modal is active.
func (s *Shell) workspacePage(c *ui.Context) {
	t := c.Theme()
	ui.Column(c).Grow(1).MinWidth(0).Children(func() {
		if s.errText != "" {
			s.workspaceErrorBanner(c, t)
		}

		switch s.surface.current() {
		case WorkspaceSurfaceDiff:
			// Git references stay in the left sidebar. Commit/file navigation
			// occupies the upper center and Diff Review stays below.
			gorexContentCard(c, gorexColorsOf(t.Dark), func() { s.gitSplitReviewSurface(c) })
		case WorkspaceSurfaceCommit:
			// Commit is a modal above the same Git review, never a second editor.
			gorexContentCard(c, gorexColorsOf(t.Dark), func() { s.gitSplitReviewSurface(c) })
		case WorkspaceSurfaceChat:
			gorexContentCard(c, gorexColorsOf(t.Dark), func() { s.workspaceChatSurface(c) })
		default:
			// The Terminal find bar must mount above the canvas: ⌘F only
			// flips terminalFind.open, without this row the bar never
			// rendered (2026-10-06 F19).
			s.terminalFindBar(c)
			s.terminalSurface(c)
		}
	})
}

// workspaceErrorBanner is the shared reconnect banner above any surface.
func (s *Shell) workspaceErrorBanner(c *ui.Context, t *ui.Theme) {
	ui.Row(c).Padding(7, 12).Background(t.Danger.Alpha(0.12)).BorderWidth(0, 0, 1, 0).BorderColor(t.Danger.Alpha(0.35)).Children(func() {
		ui.Text(c, s.errText).TextColor(t.Danger).FontSize(Typography().BodySmall).Grow(1).SingleLine()
		if ui.Button(c, "Retry").Clicked() {
			s.reloadInstances(false)
		}
	})
}

// terminalSurface renders the Terminal primary surface. The terminal canvas
// exists ONLY while this surface is visible (GWB-040); attachments stay
// alive underneath (GWB-041) and receive no input while hidden (GWB-039).
func (s *Shell) terminalSurface(c *ui.Context) {
	t := c.Theme()
	if len(s.terminals) == 0 {
		ui.Column(c).Grow(1).Center().Gap(8).Children(func() {
			if s.shellState() == stateLoading {
				ui.Spinner(c).Size(24, 24)
			}
			if s.status != "" || s.shellState() != stateLoading {
				ui.Text(c, fallbackText(s.status, "No terminal open")).TextColor(t.TextMuted)
			}
		})
		return
	}
	s.terminalCanvas(c)
}

// showSurface switches the primary surface with the §6/§34 focus rules:
// leaving Terminal explicitly removes terminal focus; entering it restores
// pane focus and resyncs geometry (GWB-042).
func (s *Shell) showSurface(kind WorkspaceSurfaceKind) {
	if s.surface.current() == WorkspaceSurfaceCommit && kind != WorkspaceSurfaceCommit && s.holdCommitNavigation() {
		return
	}
	if s.surface.current() == kind {
		return
	}
	switch kind {
	case WorkspaceSurfaceTerminal:
		s.surface.resetToTerminal(s.workspaceContext())
		if err := s.syncTerminals(s.projection); err != nil {
			s.errText = err.Error()
		}
		s.syncGitContext()
	case WorkspaceSurfaceChat:
		s.openWorkspaceChat()
	case WorkspaceSurfaceDiff:
		if s.surface.current() == WorkspaceSurfaceTerminal {
			s.surface.openDiff(s.workspaceContext(), s.surface.diff.SelectedPath)
		} else {
			s.surface.openDiff(s.workspaceContext(), s.surface.diff.SelectedPath)
		}
		s.ensureGitSnapshot(false)
	case WorkspaceSurfaceCommit:
		s.openCommitSurface()
	}
}

// workspaceChatAvailable fences user-triggered Chat navigation to an
// actual Agent pane. A previously bound conversation for another pane does
// not qualify the currently selected plain shell as a Chat destination.
func (s *Shell) workspaceChatAvailable() bool {
	_, ok := s.agentCardForSelectedPane()
	return ok
}

func (s *Shell) chatUnavailableNotice() {
	s.pendingToast = "Chat needs an Agent pane. Select an Agent from the sidebar."
}

// openWorkspaceChat activates Chat only for an Agent-backed pane. The
// ordinary shell remains on its current surface; no stale Chat draft or
// conversation may be projected as though it belongs to that shell.
func (s *Shell) openWorkspaceChat() {
	if !s.workspaceChatAvailable() {
		s.chatUnavailableNotice()
		return
	}
	s.surface.openChat(s.workspaceContext())
	s.rebindWorkspaceChat()
}

// rebindWorkspaceChat reconciles the active chat session with the selected pane.
func (s *Shell) rebindWorkspaceChat() {
	if card, ok := s.agentCardForSelectedPane(); ok {
		if s.chatPaneID == card.PaneID && s.chatConversationID != "" {
			// A later direct live bind (sidebar path) supersedes the
			// sourceless latch.
			s.chatSourcelessPaneID = ""
			return
		}
		if s.chatPaneID == card.PaneID && s.chatSourcelessPaneID == card.PaneID {
			// The latch holds only while the pane still has no chat source:
			// once the agent establishes a session (card carries the
			// conversation identity), rebind for real.
			if card.ConversationID == "" {
				return
			}
			s.chatSourcelessPaneID = ""
		}
		s.chatPaneID = card.PaneID
		s.chatSourcelessPaneID = ""
		if card.Key.TerminalID != "" && s.bindLiveAgent(card.Key) {
			return
		}
		if card.ConversationID != "" {
			s.bindChatConversation(card.ConversationID)
			return
		}
		// An Agent without any chat source must not inherit the previous
		// pane's conversation: drop to the empty state. The render path
		// re-enters while the pane stays sourceless, so the notice is
		// latched per pane (the guard above exits early).
		s.stopLiveTail()
		s.chatConversationID = ""
		s.chatTurns = nil
		s.chatWindowMessages = nil
		s.chatWindowStart = 0
		s.chatHasOlder = false
		s.chatSourcelessPaneID = card.PaneID
		s.pendingToast = "No chat stream available for this session; use Terminal."
		s.publishChatBridge()
		return
	} else if s.selectedPaneID != "" && s.selectedPaneID != s.chatPaneID {
		s.chatPaneID = s.selectedPaneID
		s.chatSourcelessPaneID = ""
		s.stopLiveTail()
		s.chatConversationID = ""
		s.chatTurns = nil
		s.publishChatBridge()
	}
}

// toggleTerminalChat toggles between Terminal and Chat on the active workspace.
func (s *Shell) toggleTerminalChat() {
	if s.surface.current() != WorkspaceSurfaceChat && !s.workspaceChatAvailable() {
		s.chatUnavailableNotice()
		return
	}
	if s.router.Path() != routeWorkspace {
		s.router.Push(routeWorkspace)
	}
	if s.surface.current() == WorkspaceSurfaceChat {
		s.showSurface(WorkspaceSurfaceTerminal)
	} else {
		s.showSurface(WorkspaceSurfaceChat)
	}
}

// agentCardForSelectedPane finds the agent card associated with the selected pane.
func (s *Shell) agentCardForSelectedPane() (agent.AgentCardModel, bool) {
	targetPaneID := s.selectedPaneID
	if targetPaneID == "" {
		targetPaneID = s.projection.FocusedPaneID
	}
	return s.agentCardForPane(targetPaneID)
}

// agentCardForPane projects one pane's Agent card from the workbench
// directory, falling back to the raw projection.
func (s *Shell) agentCardForPane(targetPaneID string) (agent.AgentCardModel, bool) {
	if targetPaneID == "" {
		return agent.AgentCardModel{}, false
	}
	if s.workbench.directory != nil {
		for _, card := range s.workbench.directory.All() {
			if card.PaneID == targetPaneID {
				return card, true
			}
		}
	}
	for _, runtimeAgent := range s.projection.Agents {
		if runtimeAgent.PaneID == targetPaneID {
			provider := agentProvider(&runtimeAgent)
			key := agent.AgentKey{InstanceID: s.activeInstance, TerminalID: runtimeAgent.TerminalID}
			return agent.AgentCardModel{
				Key:           key,
				PaneID:        runtimeAgent.PaneID,
				TabID:         runtimeAgent.TabID,
				ProjectID:     runtimeAgent.ProjectID,
				Provider:      provider,
				ProviderLabel: provider.DisplayName(),
				Title:         agentTitleFrom(runtimeAgent),
				RuntimePhase:  agent.ParseRuntimePhase(runtimeAgent.Status, false),
			}, true
		}
	}
	return agent.AgentCardModel{}, false
}

// syncWorkspaceForSelection rebinds everything that follows the selected
// Tab: Git context, terminal sync and surface validity (GWB-031/033).
func (s *Shell) syncWorkspaceForSelection() {
	ctx := s.workspaceContext()
	if !s.surface.isValid(ctx) {
		// Context changed (project/tab/pane/instance): Terminal, per §6.
		s.surface.resetToTerminal(ctx)
	}
	s.syncGitContext()
	if s.surface.current() == WorkspaceSurfaceDiff || s.surface.current() == WorkspaceSurfaceCommit {
		s.ensureGitSnapshot(false)
	}
	if s.surface.current() == WorkspaceSurfaceChat {
		s.rebindWorkspaceChat()
	}
}
