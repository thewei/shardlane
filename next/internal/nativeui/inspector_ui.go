package nativeui

import (
	"context"
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/agent"
	"github.com/wh-studio/herdr-client/next/internal/history"
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
		title := "Agent Inspector"
		if ok {
			title = card.Title
		}
		pageHeader(c, title, "Read-only agent inspection: identity, status dimensions and bound conversation.")
		if s.inspector.errText != "" {
			inlineNotice(c, ToneError, s.inspector.errText, nil)
		}
		if s.inspector.loading {
			loadingState(c, "Loading inspector…")
			return
		}
		ui.Scroll(c).Grow(1).Children(func() {
			ui.Column(c).FillWidth().Padding(sp.M, sp.XL).Gap(sp.L).Children(func() {
				if ok {
					settingsCard(c, "Status dimensions", func() {
						formRow(c, "Runtime phase", string(card.RuntimePhase), func() {})
						formRow(c, "Sendability", string(card.Sendability), func() {})
						formRow(c, "Attention", agent.OperationalLabel(card.Attention), func() {})
						formRow(c, "Markers", markerText(card.Unread, card.ReviewPending), func() {})
					})
					settingsCard(c, "Location", func() {
						formRow(c, "Project", card.ProjectName, func() {})
						formRow(c, "Pane", card.PaneID, func() {})
						formRow(c, "Tab", card.TabID, func() {})
					})
					s.inspectorUsageCard(c, card)
				}
				if s.inspector.window != nil && len(s.inspector.window.Messages) > 0 {
					settingsCard(c, "Bound conversation (bounded window)", func() {
						for _, message := range s.inspector.window.Messages {
							if message.Text == "" {
								continue
							}
							ui.Row(c).FillWidth().Gap(sp.S).Children(func() {
								ui.Text(c, strings.ToUpper(string(message.Role))).FontSize(Typography().Micro).FontWeight(700).TextColor(t.TextMuted)
								preview, _ := clipPreview(message.Text, 200, 4)
								ui.Text(c, preview).FontSize(Typography().BodySmall).Grow(1)
							})
						}
					})
				}
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
	settingsCard(c, "Usage facts", func() {
		if !usage.HasFacts() {
			formRow(c, "Usage", "No provider-reported facts", func() {})
			return
		}
		if line := usage.FormatUsageLine(); line != "" {
			formRow(c, "Summary", line, func() {})
		}
		if usage.Model != "" {
			formRow(c, "Model", usage.Model, func() {})
		}
		if usage.Tokens != nil {
			formRow(c, "Tokens", fmt.Sprintf("%s (%d)", agent.FormatTokenCount(*usage.Tokens), *usage.Tokens), func() {})
		}
		if usage.Quota != nil {
			formRow(c, "Allowance", fmt.Sprintf("%s %d%% used", usage.Quota.Label, usage.Quota.UsedPercent), func() {})
		}
		formRow(c, "Source", string(usage.Source), func() {})
	})
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

// historyProjectsPage is the 0.8 project-grouped History management view:
// sessions grouped under their normalized project, bounded metadata only.
// The grouped list renders from cached state refreshed on the dispatch
// lane — no render-time IO (0.6 §19).
func (s *Shell) historyProjectsPage(c *ui.Context) {
	s.refreshHistoryProjects()
	sp := Spacing()
	ui.Column(c).Grow(1).MinWidth(0).Background(designTokens(c.Theme().Dark).Content).Children(func() {
		pageHeader(c, "History by Project", "Project-grouped conversation management over the disposable catalog.")
		if s.historyProjectsLoading {
			loadingState(c, "Loading history…")
			return
		}
		grouped := make(map[string][]history.SessionSummary)
		var order []string
		for _, summary := range s.historyProjects {
			key := summary.Meta.ProjectPath
			if key == "" {
				key = "unknown"
			}
			if _, seen := grouped[key]; !seen {
				order = append(order, key)
			}
			grouped[key] = append(grouped[key], summary)
		}
		ui.Scroll(c).Grow(1).Children(func() {
			ui.Column(c).FillWidth().Padding(0, sp.XL, sp.L).Gap(sp.L).Children(func() {
				if len(order) == 0 {
					emptyState(c, "No history yet", "Sessions appear here after the history scan indexes provider sources.")
					return
				}
				for _, key := range order {
					sessions := grouped[key]
					if len(sessions) == 0 {
						continue
					}
					projectName := sessions[0].Meta.ProjectName
					if projectName == "" {
						projectName = key
					}
					settingsCard(c, fmt.Sprintf("%s (%d)", projectName, len(sessions)), func() {
						for _, summary := range sessions {
							summary := summary
							ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
								// C81/F: the All-conversations row shows the provider
								// badge and a full-path tooltip; this grouped row was
								// the odd one out.
								providerBadge(c, summary.Meta.Agent)
								ui.Text(c, fallbackText(summary.Meta.Title, history.Untitled)).FontSize(Typography().Body).Grow(1).SingleLine().
									Tooltip(summary.Meta.Title)
								if stamp := historyTimestamp(summary.Meta.UpdatedAt); stamp != "" {
									ui.Text(c, stamp).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
								}
								if ui.Button(c, "Open").Clicked() {
									s.router.Push("/history/" + summary.Meta.Key)
								}
							})
						}
					})
				}
			})
		})
	})
}

// refreshHistoryProjects loads the bounded recent-session metadata on the
// dispatch lane once per session (single inflight; latest result wins) —
// route re-renders read the cache instead of re-querying per frame.
func (s *Shell) refreshHistoryProjects() {
	service := s.hist.service
	if service == nil || s.historyProjectsLoaded || s.historyProjectsInflight {
		return
	}
	s.historyProjectsInflight = true
	s.dispatch(func() {
		summaries, err := service.Recent(context.Background(), 100)
		s.applyOnWindow(func() {
			s.historyProjectsInflight = false
			s.historyProjectsLoaded = true
			if err != nil {
				return
			}
			s.historyProjects = summaries
			s.historyProjectsLoading = false
		})
	})
}
