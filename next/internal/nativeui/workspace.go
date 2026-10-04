package nativeui

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/agent"
	"github.com/wh-studio/herdr-client/next/internal/history"
)

// workspacePage renders the /workspace center through the single
// WorkspacePrimarySurface owner (plan §5): Terminal, Diff or Commit —
// never two at once. The Terminal surface stays transparent so the
// gorex background gradient shows between the pane cards, and the Diff
// surface rides in a gorex card on the same gradient.
func (s *Shell) workspacePage(c *ui.Context) {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	ui.Column(c).Grow(1).MinWidth(0).Children(func() {
		if s.errText != "" {
			s.workspaceErrorBanner(c, t)
		}

		switch s.surface.current() {
		case WorkspaceSurfaceDiff:
			// The diff content sits in a gorex card on the gradient; its
			// old opaque app sheet is gone (2026-10-06).
			gorexContentCard(c, gorexColorsOf(t.Dark), func() { s.gdDiffSurface(c) })
		case WorkspaceSurfaceCommit:
			ui.Column(c).Grow(1).MinWidth(0).Background(tokens.Content).Children(func() {
				s.commitSurface(c)
			})
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
		ui.Text(c, s.errText).TextColor(t.Danger).FontSize(11).Grow(1).SingleLine()
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
			ui.Text(c, s.status).TextColor(t.TextMuted)
		})
		return
	}
	s.terminalCanvas(c)
}

// showSurface switches the primary surface with the §6/§34 focus rules:
// leaving Terminal explicitly removes terminal focus; entering it restores
// pane focus and resyncs geometry (GWB-042).
func (s *Shell) showSurface(kind WorkspaceSurfaceKind) {
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

// openWorkspaceChat activates the Chat primary surface and binds the
// selected pane's agent if available.
func (s *Shell) openWorkspaceChat() {
	s.surface.openChat(s.workspaceContext())
	s.rebindWorkspaceChat()
}

// rebindWorkspaceChat reconciles the active chat session with the selected pane.
func (s *Shell) rebindWorkspaceChat() {
	if card, ok := s.agentCardForSelectedPane(); ok {
		if s.chatPaneID == card.PaneID && s.chatConversationID != "" {
			return
		}
		s.chatPaneID = card.PaneID
		if card.Key.TerminalID != "" && s.bindLiveAgent(card.Key) {
			return
		}
		if card.ConversationID != "" {
			s.bindChatConversation(card.ConversationID)
			return
		}
	} else if s.selectedPaneID != "" && s.selectedPaneID != s.chatPaneID {
		s.chatPaneID = s.selectedPaneID
		s.stopLiveTail()
		s.chatConversationID = ""
		s.chatTurns = nil
	}
}

// toggleTerminalChat toggles between Terminal and Chat on the active workspace.
func (s *Shell) toggleTerminalChat() {
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
			provider, _ := history.ParseAgentID(runtimeAgent.Kind)
			if provider == "" && runtimeAgent.AgentSession != nil {
				provider, _ = history.ParseAgentID(runtimeAgent.AgentSession.Agent)
			}
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
