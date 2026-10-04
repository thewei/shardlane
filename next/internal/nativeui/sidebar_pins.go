package nativeui

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/settings"
)

// Sidebar Pins (2026-10-05 sidebar review): pinned Herdr Panes of the
// active instance surface at the top of the sidebar; the section
// disappears entirely when nothing is pinned. Pins are cosmetic — Herdr
// stays the pane authority — and persist per instance in settings.json.

// pinnedPaneIDs lists the active instance's pins that are still live.
func (s *Shell) pinnedPaneIDs() []string {
	if s.activeInstance == "" {
		return nil
	}
	ids := s.settings.Workbench.PinnedPanes[s.activeInstance]
	live := ids[:0:0]
	for _, id := range ids {
		for _, pane := range s.projection.Panes {
			if pane.ID == id {
				live = append(live, id)
				break
			}
		}
	}
	return live
}

func (s *Shell) isPanePinned(paneID string) bool {
	return slices.Contains(s.pinnedPaneIDs(), paneID)
}

// togglePanePin pins/unpins through the application settings service so
// the choice survives restarts.
func (s *Shell) togglePanePin(paneID string) {
	if paneID == "" {
		return
	}
	instance := s.activeInstance
	pinned := s.isPanePinned(paneID)
	s.applySettings(func(st *settings.Settings) error {
		pins := st.Workbench.PinnedPanes
		if pins == nil {
			pins = map[string][]string{}
		}
		ids := append([]string(nil), pins[instance]...)
		if pinned {
			ids = slices.DeleteFunc(ids, func(id string) bool { return id == paneID })
		} else if !slices.Contains(ids, paneID) {
			ids = append(ids, paneID)
		}
		if len(ids) == 0 {
			delete(pins, instance)
		} else {
			pins[instance] = ids
		}
		st.Workbench.PinnedPanes = pins
		return nil
	})
}

// pinnedItemsView renders the Pin section: one two-line row per pinned
// Pane with the Agent brand when one is bound. The caller renders the
// section header only when this has rows.
func (s *Shell) pinnedItemsView(c *ui.Context) {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	for _, paneID := range s.pinnedPaneIDs() {
		paneID := paneID
		label := "Pane"
		cwd := ""
		if pane := paneByID(s.projection, paneID); pane != nil {
			if pane.Label != "" {
				label = pane.Label
			}
			cwd = pane.CWD
		}
		mark, isAgent := s.agentMarkForPane(paneID, t.Dark)
		if !isAgent {
			mark = visualMark{svg: iconPin}
		}
		selected := paneID == s.selectedPaneID
		var row *ui.Element
		ui.Box(c).Key("pin:" + paneID).Children(func() {
			row = ui.ButtonBase(c).
				FillWidth().
				Height(30).
				Radius(6).
				Padding(0, 8).
				Gap(8).
				AlignItems(ui.Center).
				Label(label)
			if tint, ok := rowTint(tokens, selected, row.Hovered()); ok {
				row.Background(tint)
			}
			row.Children(func() {
				ui.Box(c).Size(14, 14).Shrink(0).Children(func() {
					markView(c, mark, 14, t.TextMuted)
				})
				ui.Column(c).Grow(1).MinWidth(0).Gap(0).Children(func() {
					ui.Text(c, label).FontSize(Typography().Body).SingleLine()
					if cwd != "" {
						ui.Text(c, cwd).FontSize(9).TextColor(t.TextMuted).SingleLine()
					}
				})
			})
		})
		if row.Clicked() {
			s.router.Push(routeWorkspace)
			s.selectPane(paneID)
		}
		row.ContextMenu(func(m *ui.Menu) {
			if m.Item("Unpin").Chosen() {
				s.togglePanePin(paneID)
			}
			m.Separator()
			s.paneOverflowItems(m, paneID, label)
		})
	}
}
