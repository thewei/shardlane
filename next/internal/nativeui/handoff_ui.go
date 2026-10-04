package nativeui

import (
	"context"
	"strings"

	"github.com/wh-studio/herdr-client/next/internal/agent"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
	"github.com/wh-studio/herdr-client/next/internal/history"
)

// Live Handoff Native action (0.6 §19, HANDOFF-09): one user-triggered
// action per source conversation, one user-facing outcome per failure
// class, and a committed target that navigates client-locally instead of
// offering a blind retry. The source Agent is never stopped, closed, or
// mutated.

// handoffActionState is the headless state of one Handoff action result.
type handoffActionState struct {
	Message string
	// NavigatePane names the committed target pane to reconcile client-
	// locally (CreatedNeedsAttention / committed transfer); empty means no
	// navigation.
	NavigatePane string
}

// projectHandoffAction maps one engine result onto the exact user outcome.
// Every §19.6 failure class stays distinct.
func projectHandoffAction(outcome *agent.LiveHandoffOutcome, failure *agent.LiveHandoffFailure) handoffActionState {
	if failure == nil {
		if outcome != nil && outcome.SourceFidelity == agent.FidelityStableStat {
			return handoffActionState{Message: "Handed off; the source was quiescent (freshness not proven complete)."}
		}
		return handoffActionState{Message: "Handed off with a verified source flush."}
	}
	switch failure.Kind {
	case agent.HandoffSourceUnresolved:
		return handoffActionState{Message: "Handoff failed: the source agent could not be resolved."}
	case agent.HandoffWaitFailed:
		return handoffActionState{Message: "Handoff failed: waiting for the source turn did not complete."}
	case agent.HandoffSourceIdentityChanged:
		return handoffActionState{Message: "Handoff failed: the source conversation changed before the snapshot."}
	case agent.HandoffSourceBusy:
		return handoffActionState{Message: "Handoff failed: the source kept starting turns — retry when it settles."}
	case agent.HandoffSourceBlocked:
		return handoffActionState{Message: "Handoff failed: the source needs the Terminal first."}
	case agent.HandoffWaitForSourceFlush:
		return handoffActionState{Message: "Handoff failed: the source has not flushed the completed turn yet."}
	case agent.HandoffSnapshotFailed:
		return handoffActionState{Message: "Handoff failed: the source snapshot could not be read."}
	case agent.HandoffSourceHasPendingOp:
		return handoffActionState{Message: "Handoff failed: deliver or cancel the queued follow-up first."}
	case agent.HandoffCreatedNeedsAttention:
		return handoffActionState{
			Message:      "Handoff target was created but needs attention: " + failure.Detail,
			NavigatePane: failure.PaneID,
		}
	default:
		pane := ""
		if failure.Committed {
			pane = failure.PaneID
		}
		return handoffActionState{Message: "Handoff failed: " + failure.Detail, NavigatePane: pane}
	}
}

// requestLiveHandoff moves the bound live conversation to a new target
// provider through the fence engine. The pending-operation seam is the
// caller-owned queue (nil until the queue worker slice wires it); blocking
// engine work runs on the dispatch lane and lands through applyOnWindow.
func (s *Shell) requestLiveHandoff(card agent.AgentCardModel, target history.AgentID, instruction string) {
	if s.launch == nil || s.activeInstance == "" {
		return
	}
	provider, identity := s.agentSessionIdentity(card.Key)
	if provider == "" || identity == nil || card.PaneID == "" {
		s.chatOutcome = "Handoff failed: this agent has no resolvable live session."
		return
	}
	sourceID, sourcePath := splitHandoffIdentity(identity.Value)
	request := agent.LiveHandoffRequest{
		Source: agent.ContinuationSource{
			Agent:    provider,
			ID:       sourceID,
			FilePath: sourcePath,
		},
		SourcePaneID:       card.PaneID,
		Target:             target,
		SourceConversation: s.chatConversationID,
		Instruction:        instruction,
	}
	if runtimeAgent := s.projectionAgent(card.Key.TerminalID); runtimeAgent != nil {
		request.Source.ProjectPath = runtimeAgent.CWD
	}
	launch := s.launch
	instance := s.activeInstance
	paneID := card.PaneID
	pending := launch.PendingOperations()
	s.dispatch(func() {
		outcome, failure := launch.HandoffLiveAgent(context.Background(), instance, request, pending, agent.DefaultHandoffTiming())
		s.applyOnWindow(func() {
			if s.chatPaneID == paneID || failure != nil && failure.Committed {
				var outcomePtr *agent.LiveHandoffOutcome
				if failure == nil {
					outcomePtr = &outcome
				}
				state := projectHandoffAction(outcomePtr, failure)
				s.chatOutcome = state.Message
				if state.NavigatePane != "" {
					s.selectPane(state.NavigatePane)
				}
			}
		})
	})
}

// projectionAgent finds the runtime agent row for one terminal.
func (s *Shell) projectionAgent(terminalID string) *herdr.Agent {
	for i := range s.projection.Agents {
		if s.projection.Agents[i].TerminalID == terminalID {
			return &s.projection.Agents[i]
		}
	}
	return nil
}

// splitHandoffIdentity splits the typed locator "id:<native>"/"path:<file>"
// into the source facts; an untyped value is the native id.
func splitHandoffIdentity(value string) (string, string) {
	if path, ok := strings.CutPrefix(value, "path:"); ok {
		return "", path
	}
	if id, ok := strings.CutPrefix(value, "id:"); ok {
		return id, ""
	}
	return value, ""
}
