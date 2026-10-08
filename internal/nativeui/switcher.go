package nativeui

import (
	"fmt"

	"github.com/egoist/mygo/plugins/glass"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
)

// handleMRUShortcuts implements the Agent MRU switcher (0.5 §13): Ctrl-Tab
// opens or cycles forward, Ctrl-Shift-Tab cycles backward, Enter or click
// commits local navigation, Esc cancels. MyGo 0.2.7 exposes no view-visible
// Control-key release, so release-to-commit is adapted to Enter/click; the
// deviation is recorded in the 0.5 audit.
func (s *Shell) handleMRUShortcuts(c *ui.Context) {
	if !s.workbench.switcherOpen {
		if c.Shortcut(ui.Ctrl, ui.KeyTab) {
			s.openAgentSwitcher()
		}
	}
}

// openAgentSwitcher snapshots the current live MRU ordering.
func (s *Shell) openAgentSwitcher() {
	cards := s.AgentMRUList()
	if len(cards) == 0 {
		return
	}
	s.workbench.switcherOpen = true
	s.workbench.switcherIndex = 0
	if card, ok := s.workbench.directory.Get(selectedAgentKeyOf(s)); ok {
		s.workbench.switcherOrigin = card.Key
		s.workbench.switcherHad = true
	} else {
		s.workbench.switcherHad = false
	}
}

func selectedAgentKeyOf(s *Shell) agent.AgentKey {
	if pane := s.selectedPane(); pane != nil {
		return agent.AgentKey{InstanceID: s.activeInstance, TerminalID: pane.TerminalID}
	}
	return agent.AgentKey{}
}

func (s *Shell) closeAgentSwitcher(restore bool) {
	s.workbench.switcherOpen = false
	if restore && s.workbench.switcherHad {
		if card, ok := s.workbench.directory.Get(s.workbench.switcherOrigin); ok {
			s.openAgentCard(card)
		}
	}
}

func (s *Shell) commitAgentSwitcher() {
	cards := s.AgentMRUList()
	if s.workbench.switcherIndex < 0 || s.workbench.switcherIndex >= len(cards) {
		s.closeAgentSwitcher(false)
		return
	}
	card := cards[s.workbench.switcherIndex]
	s.workbench.switcherOpen = false
	s.openAgentCard(card)
}

// agentSwitcherOverlay renders the MRU overlay while it is open (0.5 §13):
// provider mark, title, compact status per row; hover or arrows highlight;
// click or Enter commits; Esc cancels.
func (s *Shell) agentSwitcherOverlay(c *ui.Context) {
	if !s.workbench.switcherOpen {
		return
	}
	cards := s.AgentMRUList()
	if len(cards) == 0 {
		s.workbench.switcherOpen = false
		return
	}
	sp := Spacing()
	ui.Modal(c, &s.workbench.switcherOpen, func() {
		// Cycle/commit/cancel keys are handled inside the modal context:
		// shortcuts registered outside it are not delivered while a modal
		// is up.
		s.handleSwitcherKeys(c)
		// Liquid Glass (MyGo 0.2.15), matching the Command Center palette.
		ui.Column(c).Width(460).Padding(sp.L).Gap(sp.XS).
			Radius(Radius().Card).Material(glass.Glass{}).
			Border(1, designTokens(c.Theme().Dark).BorderSubtle).Children(func() {
			ui.Text(c, "Switch Agent").FontSize(Typography().Section).FontWeight(650)
			ui.Text(c, fmt.Sprintf("Ctrl-Tab cycle · Enter commit · Esc cancel (%d agent%s)", len(cards), pluralS(len(cards)))).
				FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
			for index, card := range cards {
				index, card := index, card
				highlighted := index == s.workbench.switcherIndex
				var row *ui.Element
				ui.Box(c).Children(func() {
					row = ui.Row(c).FillWidth().Padding(sp.S, sp.M).Gap(sp.S).
						AlignItems(ui.Center).Radius(Radius().Control).Children(func() {
						providerBadge(c, card.Provider)
						ui.Text(c, card.Title).FontSize(Typography().Body).Grow(1).SingleLine()
						statusPill(c, agent.OperationalLabel(card.Attention), operationalTone(card.Attention))
					})
				})
				if highlighted {
					row.Background(designTokens(c.Theme().Dark).StatusBackground(ToneInfo, c.Theme().Dark))
				}
				if row.Hovered() {
					s.workbench.switcherIndex = index
				}
				if row.Clicked() {
					s.workbench.switcherIndex = index
					s.commitAgentSwitcher()
				}
			}
		})
	})
}

// handleSwitcherKeys runs inside the switcher modal: Ctrl-Tab cycles
// forward, Ctrl-Shift-Tab backward, Enter commits, Esc cancels.
func (s *Shell) handleSwitcherKeys(c *ui.Context) {
	cards := s.AgentMRUList()
	if len(cards) == 0 {
		s.workbench.switcherOpen = false
		return
	}
	switch {
	case c.Shortcut(ui.Ctrl|ui.Shift, ui.KeyTab):
		s.workbench.switcherIndex = (s.workbench.switcherIndex - 1 + len(cards)) % len(cards)
	case c.Shortcut(ui.Ctrl, ui.KeyTab):
		s.workbench.switcherIndex = (s.workbench.switcherIndex + 1) % len(cards)
	case c.Shortcut(0, ui.KeyEscape):
		s.closeAgentSwitcher(true)
	case c.Shortcut(0, ui.KeyEnter):
		s.commitAgentSwitcher()
	}
}
