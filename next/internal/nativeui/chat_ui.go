package nativeui

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/agent"
	"github.com/wh-studio/herdr-client/next/internal/conversation"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
	"github.com/wh-studio/herdr-client/next/internal/history"
)

// chatTimelineRow renders one derived timeline turn with the ChatGPT
// contract (0.6 §13): narration always visible, thinking consolidated in a
// Collapsible, tool runs as compact chips (grouped when 2+), failed turns
// keep the partial scene.
func (s *Shell) chatTimelineRow(c *ui.Context, turn conversation.TimelineTurn, turnIndex int) {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	sp := Spacing()
	ui.Column(c).FillWidth().Padding(sp.M, sp.L).Gap(sp.S).
		Radius(Radius().Row).Background(tokens.Panel).Border(1, tokens.BorderSubtle).Children(func() {
		switch {
		case turn.UserRow:
			ui.Row(c).Gap(sp.S).AlignItems(ui.Center).Children(func() {
				ui.Text(c, "USER").FontSize(Typography().Micro).FontWeight(700).TextColor(t.TextMuted)
			})
			ui.Text(c, turn.Text).FontSize(Typography().Body)
			return
		default:
			ui.Row(c).Gap(sp.S).AlignItems(ui.Center).Children(func() {
				ui.Text(c, "ASSISTANT").FontSize(Typography().Micro).FontWeight(700).TextColor(t.TextMuted)
				if turn.Failed {
					statusPill(c, "Stopped", ToneError)
				}
			})
			if turn.Narration != "" {
				ui.Text(c, turn.Narration).FontSize(Typography().Body)
			}
			if turn.Thinking != nil && *turn.Thinking != "" {
				open := s.chatTurnPartOpen(turnIndex, "thinking")
				ui.Collapsible(c, "Thinking", &open, func() {
					ui.Text(c, *turn.Thinking).FontSize(Typography().BodySmall).TextColor(t.TextMuted)
				})
				s.setChatTurnPartOpen(turnIndex, "thinking", open)
			}
			if len(turn.ToolRuns) > 0 {
				summary := "Tool activity"
				if turn.Compacted {
					summary = "Tool activity (grouped)"
				}
				open := s.chatTurnPartOpen(turnIndex, "tools")
				ui.Collapsible(c, summary, &open, func() {
					for _, run := range turn.ToolRuns {
						toolCard(c, run.Name, run.InputPreview, run.Output, run.IsError)
					}
				})
				s.setChatTurnPartOpen(turnIndex, "tools", open)
			}
		}
	})
}

// chatComposerRow renders the prompt input, outcome banner, and Send/Queue controls.
func (s *Shell) chatComposerRow(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	ui.Column(c).Padding(sp.M, sp.XL, sp.L).Gap(sp.S).BorderWidth(1, 0, 0, 0).
		BorderColor(designTokens(c.Theme().Dark).BorderSubtle).Children(func() {
		// Outcome line (F65): delivery failures, queue refusals and the
		// "do not resend" warning were written but never rendered.
		if s.chatOutcome != "" {
			ui.Row(c).FillWidth().Padding(sp.S, sp.M).Background(t.Danger.Alpha(0.12)).
				Radius(Radius().Control).Children(func() {
				ui.Text(c, s.chatOutcome).FontSize(Typography().Caption).TextColor(t.Danger).Grow(1)
			})
		}
		ui.Row(c).Gap(sp.M).AlignItems(ui.End).Children(func() {
			// The prompt input itself (F71): chatDraft was read by
			// sendChatPrompt and cleared after send, but no element ever
			// bound it — the composer had no field to type into.
			field := ui.TextArea(c, &s.chatDraft).Grow(1).Height(64).
				Placeholder("Follow up on this conversation…").Label("Chat prompt")
			// Enter sends (chat convention) but only when the live
			// disposition is SentNow; multiline TextArea otherwise keeps
			// Enter as a newline, and queued/terminal-needed states stay
			// on the explicit buttons (2026-10-06 F82).
			if field.Shortcut(0, ui.KeyEnter) && s.chatDisposition == conversation.DispositionSentNow {
				s.sendChatPrompt()
			}
			queued := s.chatQueuedText != ""
			switch {
			case queued:
				ui.Text(c, "Queued after turn").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
				if ui.Button(c, "Cancel").Clicked() {
					s.cancelChatFollowUp()
				}
			case s.chatDisposition == conversation.DispositionSentNow:
				// Empty drafts render disabled: sendChatPrompt would
				// no-op, and a live Send on an empty box reads as broken
				// (2026-10-06 F100). While an input method is composing,
				// Send stays disabled too: a click would commit a
				// half-typed composition as the prompt.
				if ui.PrimaryButton(c, "Send").Disabled(strings.TrimSpace(s.chatDraft) == "" || field.Composing()).Clicked() {
					s.sendChatPrompt()
				}
			default:
				ui.PrimaryButton(c, "Send").Disabled(true)
			}
		})
	})
}

// workspaceChatSurface renders the Chat view as a first-class WorkspacePrimarySurface
// directly inside the /workspace center canvas.
func (s *Shell) workspaceChatSurface(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	card, hasCard := s.agentCardForSelectedPane()
	if hasCard && (s.chatPaneID != card.PaneID || (s.chatConversationID == "" && !s.chatLoading)) {
		s.rebindWorkspaceChat()
	}

	ui.Column(c).Grow(1).MinWidth(0).Background(designTokens(t.Dark).Content).Children(func() {
		// Header band within the workspace card
		ui.Row(c).FillWidth().Padding(sp.S, sp.XL).Gap(sp.M).AlignItems(ui.Center).
			BorderWidth(0, 0, 1, 0).BorderColor(designTokens(t.Dark).BorderSubtle).Children(func() {
			if hasCard {
				providerBadge(c, card.Provider)
				ui.Text(c, card.Title).FontSize(Typography().Body).FontWeight(600).SingleLine()
				if s.chatConversationID != "" {
					summary := s.chatDispositionSummary()
					statusPill(c, summary, dispositionTone(s.chatDisposition))
					ui.Text(c, s.chatConversationID).FontSize(Typography().Micro).TextColor(t.TextMuted).SingleLine()
				}
			} else {
				ui.Icon(c, iconChat).Size(15, 15).TextColor(t.TextMuted)
				ui.Text(c, "Agent Chat").FontSize(Typography().Body).FontWeight(600).SingleLine()
			}
			ui.Spacer(c)
			if ui.Button(c, "Terminal").Clicked() {
				s.showSurface(WorkspaceSurfaceTerminal)
			}
		})

		if s.chatLoading {
			loadingState(c, "Loading conversation…")
			return
		}

		if s.chatConversationID == "" {
			if hasCard {
				emptyState(c, "Waiting for agent session…",
					fmt.Sprintf("Agent %q in this pane has not established a typed session yet.", card.Title))
			} else {
				emptyState(c, "No agent in this pane",
					"This pane is running a standard shell. Select an agent pane or launch a new task to view its conversation.")
			}
			return
		}

		ui.Scroll(c).Grow(1).Children(func() {
			ui.Column(c).FillWidth().Padding(sp.M, sp.XL).Gap(sp.S).Children(func() {
				for i, turn := range s.chatTurns {
					s.chatTimelineRow(c, turn, i)
				}
			})
		})
		s.chatComposerRow(c)
	})
}

// chatPage is the /chat Native Conversation surface (0.6 §20): bounded
// timeline, composer, queued follow-up indicator. It binds to one
// conversation id at a time; live decoding is the 0.6 live-source lane.
func (s *Shell) chatPage(c *ui.Context) {
	sp := Spacing()
	ui.Column(c).Grow(1).MinWidth(0).Background(designTokens(c.Theme().Dark).Content).Children(func() {
		pageHeader(c, "Chat", "One conversation surface over the Herdr-owned live agent session.")
		if s.chatConversationID == "" {
			emptyState(c, "No conversation selected", "Open a conversation from History or an Agent card to bind this surface.")
			return
		}
		if s.chatLoading {
			loadingState(c, "Loading conversation…")
			return
		}
		ui.Row(c).Padding(0, sp.XL, sp.S).Gap(sp.M).AlignItems(ui.Center).Children(func() {
			summary := s.chatDispositionSummary()
			statusPill(c, summary, dispositionTone(s.chatDisposition))
			ui.Text(c, s.chatConversationID).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted).SingleLine()
		})
		ui.Scroll(c).Grow(1).Children(func() {
			ui.Column(c).FillWidth().Padding(sp.M, sp.XL).Gap(sp.S).Children(func() {
				for i, turn := range s.chatTurns {
					s.chatTimelineRow(c, turn, i)
				}
			})
		})
		s.chatComposerRow(c)
	})
}

// sendChatPrompt routes through the disposition decided from live
// sendability at commit time: SentNow delivers via the canonical
// agent.prompt transaction; MidTurn queues; NeedsTerminal/unknown are
// refused.
func (s *Shell) sendChatPrompt() {
	text := strings.TrimSpace(s.chatDraft)
	if text == "" || s.launch == nil || s.activeInstance == "" || s.chatPaneID == "" {
		return
	}
	disposition := conversation.DispositionFor(agent.ClassifySendability(
		agent.ParseRuntimePhase(string(s.chatPhase), s.chatLaunchPending)))
	switch disposition {
	case conversation.DispositionSentNow:
		requestID := fmt.Sprintf("chat-%s-%d", s.chatPaneID, time.Now().UnixNano())
		err := s.launch.PromptLiveAgent(context.Background(), s.activeInstance, s.chatPaneID, text)
		s.chatAfterPrompt(requestID, text, disposition, err)
	case conversation.DispositionQueuedAfterTurn:
		s.enqueueChatFollowUp(text)
	default:
		s.chatOutcome = "This agent needs the terminal; prompts stay disabled."
	}
}

// enqueueChatFollowUp queues the follow-up in the real delivery ledger
// (0.6 §11): identity-proven at enqueue so the worker's occupant
// revalidation has its coordinate; an already-queued conversation keeps
// its single slot.
func (s *Shell) enqueueChatFollowUp(text string) {
	fingerprint := s.liveFingerprintForPane(s.chatPaneID)
	if fingerprint == "" {
		s.chatOutcome = "Cannot queue: the live session identity is unresolved."
		return
	}
	item := conversation.QueueItem{
		RequestID:      fmt.Sprintf("chat-queue-%s-%d", s.chatPaneID, time.Now().UnixNano()),
		ConversationID: conversation.ConversationID(s.chatConversationID),
		AgentPaneID:    s.chatPaneID,
		Fingerprint:    fingerprint,
		Text:           text,
		Instance:       s.activeInstance,
	}
	if err := s.launch.EnqueueFollowUp(item); err != nil {
		s.chatOutcome = "Queue full: " + err.Error()
		return
	}
	s.chatQueuedText = text
	s.chatDraft = ""
}

// cancelChatFollowUp cancels the queued follow-up in the ledger (available
// only before the delivery commit) and clears the composer chip.
func (s *Shell) cancelChatFollowUp() {
	if s.launch != nil && s.chatConversationID != "" {
		if _, err := s.launch.Queue().Cancel(conversation.ConversationID(s.chatConversationID)); err != nil && s.chatQueuedText != "" {
			// The delivery already committed; the chip must not pretend
			// otherwise.
			s.chatOutcome = "The follow-up already delivered."
			return
		}
	}
	s.chatQueuedText = ""
}

func (s *Shell) chatAfterPrompt(requestID, text string, disposition conversation.PromptDisposition, err error) {
	if err != nil {
		var uncertain *herdr.DeliveryUncertainError
		if errors.As(err, &uncertain) {
			// Uncertainty never pretends the text was unsent nor auto-retries.
			s.chatOutcome = "Prompt delivery uncertain — do not resend; check the agent."
			return
		}
		s.chatOutcome = "Prompt failed: " + err.Error()
		return
	}
	s.chatDraft = ""
	s.chatOutcome = ""
	s.chatLastRequestID = requestID
	// Pending submission echo (0.6 §12): a temporary local User row bound to
	// the pane and the live baseline. Only a bound live tail can reconcile
	// it, so history-bound sends show no temporary row; a queued prompt is
	// not a submission and creates none.
	if s.chatLivePump != nil {
		s.chatPendingEcho = &conversation.PendingEcho{
			Text:     text,
			Baseline: s.chatLiveCommitted,
			PaneID:   s.chatPaneID,
		}
		s.appendPendingEchoTurn()
	}
}

// bindChatConversation binds one conversation and fills the timeline from
// the read-only HistoryService bounded window (CONV-07/CONV-09): latest
// generation wins, stale loads cannot apply.
func (s *Shell) bindChatConversation(conversationID string) {
	if conversationID == "" || s.hist.service == nil {
		return
	}
	if s.chatConversationID == conversationID && (s.chatLoading || len(s.chatTurns) > 0) {
		return
	}
	s.stopLiveTail()
	// A history-bound surface cannot reconcile a pending submission against
	// the provider source, so the echo drops (0.6 §12 rebind rule).
	s.chatPendingEcho = nil
	if s.chatOpenCancel != nil {
		s.chatOpenCancel()
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.chatOpenCancel = cancel
	generation := s.chatGen.Add(1)
	s.chatConversationID = conversationID
	s.chatLoading = true
	s.chatTurns = nil
	service := s.hist.service

	s.dispatch(func() {
		window, err := service.Open(ctx, history.ConversationWindowRequest{ConversationID: conversationID})
		s.applyOnWindow(func() {
			if generation != s.chatGen.Load() {
				return
			}
			s.chatLoading = false
			if err != nil {
				s.chatOutcome = "Load failed: " + err.Error()
				return
			}
			s.chatTurns = conversation.DeriveTimeline(conversation.ItemsFromTranscript(window.Messages))
		})
	})
}

// chatDispositionSummary summarizes the current composer disposition.
func (s *Shell) chatDispositionSummary() string {
	switch s.chatDisposition {
	case conversation.DispositionSentNow:
		return "Ready"
	case conversation.DispositionQueuedAfterTurn:
		return "Working — prompt will queue"
	case conversation.DispositionNeedsTerminal:
		return "Blocked — use the terminal"
	default:
		return "Unknown state"
	}
}

func dispositionTone(disposition conversation.PromptDisposition) StatusTone {
	switch disposition {
	case conversation.DispositionSentNow:
		return ToneSuccess
	case conversation.DispositionQueuedAfterTurn:
		return ToneWorking
	case conversation.DispositionNeedsTerminal:
		return ToneAttention
	default:
		return ToneMuted
	}
}

// chatTurnPartOpen reads the per-turn disclosure state (F94): the shared
// booleans made every Thinking/Tools collapsible move together.
func (s *Shell) chatTurnPartOpen(turnIndex int, part string) bool {
	return s.chatTurnExpanded[fmt.Sprintf("%d:%s", turnIndex, part)]
}

func (s *Shell) setChatTurnPartOpen(turnIndex int, part string, open bool) {
	if s.chatTurnExpanded == nil {
		s.chatTurnExpanded = make(map[string]bool)
	}
	s.chatTurnExpanded[fmt.Sprintf("%d:%s", turnIndex, part)] = open
}
