package nativeui

import (
	"context"
	"os"
	"time"

	"github.com/wh-studio/herdr-client/next/internal/agent"
	"github.com/wh-studio/herdr-client/next/internal/conversation"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
	"github.com/wh-studio/herdr-client/next/internal/history"
)

// liveChatBackstop is the production live-tail timer backstop (0.6 §7.4):
// projection reconciles send the event wake; the timer only catches missed
// events. No high-frequency polling anywhere.
const liveChatBackstop = 2 * time.Second

// agentSessionIdentity returns the measured provider session identity of one
// live Agent from the current Herdr projection, failing closed when the
// agent is gone or its identity does not name a known provider.
func (s *Shell) agentSessionIdentity(key agent.AgentKey) (history.AgentID, *herdr.AgentSessionIdentity) {
	for _, runtimeAgent := range s.projection.Agents {
		if runtimeAgent.TerminalID != key.TerminalID || runtimeAgent.AgentSession == nil {
			continue
		}
		provider, ok := history.ParseAgentID(runtimeAgent.AgentSession.Agent)
		if !ok {
			return "", nil
		}
		return provider, runtimeAgent.AgentSession
	}
	return "", nil
}

// openChat routes to the Chat surface bound to one card through the single
// bind path: live agents stream their provider transcript tail; history
// conversations read through HistoryService. It activates WorkspaceSurfaceChat
// inside the workspace for a seamless in-situ experience.
func (s *Shell) openChat(card agent.AgentCardModel) {
	s.chatPaneID = card.PaneID
	if card.PaneID != "" {
		s.selectPane(card.PaneID)
	}
	if s.bindLiveAgent(card.Key) {
		if s.router.Path() != routeWorkspace {
			s.router.Push(routeWorkspace)
		}
		s.surface.openChat(s.workspaceContext())
		return
	}
	if card.ConversationID != "" {
		s.bindChatConversation(card.ConversationID)
		if s.router.Path() != routeWorkspace {
			s.router.Push(routeWorkspace)
		}
		s.surface.openChat(s.workspaceContext())
		return
	}
	// No live session identity and no history conversation: navigating to
	// /chat would render its empty state behind the user's back (2026-10-06
	// F58). Land on the agent's Terminal with a hint instead.
	s.status = "No chat source for this agent yet — opened its Terminal."
	s.pendingToast = s.status // the status strip only renders on the empty-terminal branch
	if s.router.Path() != routeWorkspace {
		s.router.Push(routeWorkspace)
	}
}

// bindLiveAgent streams one live Agent's provider-owned transcript into the
// Chat timeline (CONV-08, CONV-11..19): the measured session identity
// resolves to the exact source file and the incremental decoder pump owns
// the tail until the conversation switches. It reports whether the live
// binding took over; unresolvable or unsupported identities fall back to the
// history bind path instead of guessing a source.
func (s *Shell) bindLiveAgent(key agent.AgentKey) bool {
	provider, identity := s.agentSessionIdentity(key)
	if provider == "" || identity == nil {
		return false
	}
	fingerprint := conversation.SessionIdentity{
		Provider: string(provider),
		Kind:     identity.Kind,
		Source:   identity.Source,
		Value:    identity.Value,
	}.Fingerprint()
	if s.chatLiveCancel != nil && s.chatLiveFingerprint == fingerprint {
		// Already bound to this exact session; the pane rule still decides
		// whether the pending submission survives the rebind (0.6 §12).
		s.retainPendingEchoForPane(s.chatPaneID)
		return true
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return false
	}
	path, err := history.ResolveLiveSource(home, provider, identity.Kind, identity.Source, identity.Value)
	if err != nil {
		return false
	}
	decoder, err := history.NewLiveDecoder(provider)
	if err != nil {
		return false
	}

	s.stopLiveTail()
	if s.chatOpenCancel != nil {
		s.chatOpenCancel()
	}
	ctx, cancel := context.WithCancel(context.Background())
	generation := s.chatGen.Add(1)
	wake := make(chan struct{}, 1)
	backstop := liveChatBackstop
	if s.chatLiveBackstopOverride > 0 {
		backstop = s.chatLiveBackstopOverride
	}
	pump := history.NewLivePump(history.NewLiveWatcher(history.NewFileLiveSource(path), decoder), wake)
	pump.Backstop = backstop
	pump.OnSync = func(sync history.LiveSync, syncErr error) {
		if syncErr != nil || ctx.Err() != nil || !sync.HasUpdates() {
			return
		}
		s.applyOnWindow(func() {
			if ctx.Err() != nil || generation != s.chatGen.Load() {
				return
			}
			s.applyLiveSync(decoder)
		})
	}
	s.chatLiveCancel = cancel
	s.chatLiveWake = wake
	s.chatLivePump = pump
	s.chatLiveFingerprint = fingerprint
	s.chatLivePath = path
	s.chatConversationID = "live:" + string(provider) + ":" + key.TerminalID
	s.chatLoading = true
	s.chatTurns = nil
	// A same-pane rebind preserves the pending submission (0.6 §12); any
	// other pane's rebind dropped it at openChat's pane assignment.
	s.retainPendingEchoForPane(s.chatPaneID)
	s.appendPendingEchoTurn()
	// The first hydration runs on the bind lane (synchronous in headless
	// tests, window-marshalled in the app); the pump goroutine only reacts
	// to later wakes and the backstop.
	pump.PumpOnce()
	go pump.Run(ctx)
	return true
}

// wakeLiveTail nudges the bound live pump after a projection change (the
// event wake of 0.6 §7.4; the timer backstop stays the safety net).
// Non-blocking by contract: the UI lane never waits on the pump.
func (s *Shell) wakeLiveTail() {
	if s.chatLiveWake == nil {
		return
	}
	select {
	case s.chatLiveWake <- struct{}{}:
	default:
	}
}

// liveFingerprintForPane resolves the current occupant's session
// fingerprint for one pane — the enqueue-time identity coordinate the
// delivery worker revalidates against (0.6 §11.3).
func (s *Shell) liveFingerprintForPane(paneID string) string {
	for _, runtimeAgent := range s.projection.Agents {
		if runtimeAgent.PaneID != paneID || runtimeAgent.AgentSession == nil {
			continue
		}
		provider, ok := history.ParseAgentID(runtimeAgent.AgentSession.Agent)
		if !ok {
			return ""
		}
		return conversation.SessionIdentity{
			Provider: string(provider),
			Kind:     runtimeAgent.AgentSession.Kind,
			Source:   runtimeAgent.AgentSession.Source,
			Value:    runtimeAgent.AgentSession.Value,
		}.Fingerprint()
	}
	return ""
}

// stopLiveTail cancels a bound live pump and clears its binding facts.
func (s *Shell) stopLiveTail() {
	if s.chatLiveCancel != nil {
		s.chatLiveCancel()
		s.chatLiveCancel = nil
	}
	s.chatLiveWake = nil
	s.chatLivePump = nil
	s.chatLiveFingerprint = ""
	s.chatLivePath = ""
	s.chatLiveCommitted = 0
}

// retainPendingEchoForPane keeps a pending submission across a rebind only
// when the identity pins the same pane (0.6 §12, the History Composer
// continuation case); any other rebind drops the unrelated echo.
func (s *Shell) retainPendingEchoForPane(paneID string) {
	if s.chatPendingEcho != nil && s.chatPendingEcho.PaneID != paneID {
		s.chatPendingEcho = nil
	}
}

// applyLiveSync refolds the bounded live tail window into chat turns: the
// pending echo is reconciled against the committed projection first (0.6
// §12), then the windowed projection is mapped through the shared timeline
// derivation, the provisional streaming tail renders uncommitted, and an
// unconsumed pending submission renders as the tail row.
func (s *Shell) applyLiveSync(decoder *history.LiveDecoder) {
	s.chatLoading = false
	projection := decoder.Projection()
	s.chatLiveCommitted = len(projection)
	if echo, consumed := conversation.ReconcilePendingEcho(projection, s.chatPendingEcho); consumed {
		s.chatPendingEcho = echo
	}
	messages := projection
	if limit := conversation.MaxBeforeAfter * 2; len(messages) > limit {
		messages = messages[len(messages)-limit:]
	}
	items := conversation.ItemsFromTranscript(messages)
	if pending := decoder.PendingTail(); pending != nil {
		items = append(items, conversation.ProvisionalItem(*pending))
	}
	s.chatTurns = conversation.DeriveTimeline(items)
	s.appendPendingEchoTurn()
}

// appendPendingEchoTurn renders the unconsumed pending submission as the
// timeline tail; once consumed, the committed provider row is the only
// visible User row — never a duplicate.
func (s *Shell) appendPendingEchoTurn() {
	if s.chatPendingEcho == nil {
		return
	}
	s.chatTurns = append(s.chatTurns, conversation.TimelineTurn{
		UserRow: true,
		Text:    s.chatPendingEcho.Text,
	})
}
