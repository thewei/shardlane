package nativeui

import (
	"context"
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/history"
)

// inspectorState is the 0.8 Agent Inspector presentation state: one
// selected agent card plus its bounded, read-only transcript window.
type inspectorState struct {
	agentKey agent.AgentKey
	convoID  string
	window   *history.TranscriptWindow
	loading  bool
	errText  string
}

// inspectorPage shows one live Agent with its identity facts and the bounded
// transcript window of a bound conversation (0.8: read-only inspector — no
// runtime authority, no terminal-output inference).
func (s *Shell) inspectorPage(c *ui.Context, paneID string) {
	s.loadInspector(paneID)
	t := c.Theme()
	sp := Spacing()
	ui.Column(c).Grow(1).MinWidth(0).Background(designTokens(t.Dark).Content).Children(func() {
		card, ok := s.workbench.directory.Get(agent.AgentKey{InstanceID: s.activeInstance, TerminalID: s.inspector.agentKey.TerminalID})
		if s.inspector.errText != "" {
			inlineNotice(c, ToneError, s.inspector.errText, nil)
		}
		if s.inspector.loading {
			loadingState(c, "Loading…")
			return
		}
		ui.Scroll(c).Grow(1).Children(func() {
			contentColumn(c, formColumnWidth, func() {
				ui.Column(c).FillWidth().Padding(pageTopInset(), sp.XL, sp.XL).Gap(sp.XL).Children(func() {
					if ok {
						settingsGroup(c, "Status",
							row("Runtime", "", func() { valueText(c, string(card.RuntimePhase)) }),
							row("Can receive input", "", func() { valueText(c, string(card.Sendability)) }),
							row("Attention", "", func() { valueText(c, agent.OperationalLabel(card.Attention)) }),
							row("Markers", "", func() { valueText(c, markerText(card.Unread, card.ReviewPending)) }),
						)
						settingsGroup(c, "Location",
							row("Project", "", func() { valueText(c, card.ProjectName) }),
							row("Pane", "", func() { valueText(c, card.PaneID) }),
							row("Tab", "", func() { valueText(c, card.TabID) }),
						)
						s.inspectorUsageCard(c, card)
					}
					if s.inspector.window != nil && len(s.inspector.window.Messages) > 0 {
						settingsCard(c, "Conversation", func() {
							for _, message := range s.inspector.window.Messages {
								if message.Text == "" {
									continue
								}
								ui.Row(c).FillWidth().Gap(sp.S).Children(func() {
									ui.Text(c, strings.ToUpper(string(message.Role))).FontSize(Typography().Micro).FontWeight(WeightSemibold).TextColor(t.TextMuted)
									preview := clipPreview(message.Text, 200, 4)
									ui.Text(c, preview).FontSize(Typography().BodySmall).Grow(1)
								})
							}
						})
					}
				})
			})
		})
	})
}

func markerText(unread, review bool) string {
	switch {
	case unread && review:
		return "unread · review pending"
	case unread:
		return "unread"
	case review:
		return "review pending"
	default:
		return "none"
	}
}

// inspectorUsageCard renders the 0.8 usage-facts display from the §17.6
// lightweight projection: model/token facts the History adapters already
// parsed, from the bound conversation's metadata only (no transcript
// parse, no render-time IO). Unprovable facts stay absent — never guessed.
func (s *Shell) inspectorUsageCard(c *ui.Context, card agent.AgentCardModel) {
	if s.inspector.window == nil {
		return
	}
	usage := agent.BuildAgentUsageSnapshot(card.Key, s.inspector.window.Meta)
	if !usage.HasFacts() {
		settingsGroup(c, "Usage", row("Usage", "", func() { valueText(c, "Not reported") }))
		return
	}
	var rows []settingsRow
	if line := usage.FormatUsageLine(); line != "" {
		rows = append(rows, row("Summary", "", func() { valueText(c, line) }))
	}
	if usage.Model != "" {
		rows = append(rows, row("Model", "", func() { valueText(c, usage.Model) }))
	}
	if usage.Tokens != nil {
		text := fmt.Sprintf("%s (%d)", agent.FormatTokenCount(*usage.Tokens), *usage.Tokens)
		rows = append(rows, row("Tokens", "", func() { valueText(c, text) }))
	}
	if usage.Quota != nil {
		text := fmt.Sprintf("%s %d%% used", usage.Quota.Label, usage.Quota.UsedPercent)
		rows = append(rows, row("Allowance", "", func() { valueText(c, text) }))
	}
	rows = append(rows, row("Source", "", func() { valueText(c, string(usage.Source)) }))
	settingsGroup(c, "Usage", rows...)
}

// loadInspector resolves the agent card and, when a conversation is bound,
// loads its bounded window through the read-only HistoryService.
func (s *Shell) loadInspector(paneID string) {
	if s.inspector.loading {
		return
	}
	var key agent.AgentKey
	for _, card := range s.workbenchCards() {
		if card.PaneID == paneID {
			key = card.Key
			break
		}
	}
	if s.inspector.agentKey == key && s.inspector.window != nil {
		return
	}
	s.inspector = inspectorState{agentKey: key, loading: true}
	conversationID := s.chatConversationID
	service := s.hist.service

	s.dispatch(func() {
		var window *history.TranscriptWindow
		if service != nil && conversationID != "" {
			bound, boundErr := service.Open(context.Background(), history.ConversationWindowRequest{
				ConversationID: conversationID,
			})
			if boundErr == nil {
				window = &bound
			}
			// An unbound or unloadable conversation is not an inspector
			// error: identity dimensions still render without a transcript.
		}
		s.applyOnWindow(func() {
			if s.inspector.agentKey != key {
				return
			}
			s.inspector.loading = false
			s.inspector.window = window
			s.inspector.convoID = conversationID
		})
	})
}
