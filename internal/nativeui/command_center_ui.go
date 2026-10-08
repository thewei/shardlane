package nativeui

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/plugins/glass"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/commandcenter"
	"github.com/wh-studio/herdr-client/internal/history"
)

// Command Center (0.9 P1) over the shared Action Registry (0.9 P2): the
// fast navigation/actions palette. Opening it performs zero filesystem,
// network or Herdr RPC work — every action is built from application
// snapshot facts and every result carries a typed stable target that the
// executor revalidates before acting. It is the app's single search
// surface: the standalone Search page was removed (2026-10-06 product
// decision) and ⌘K/⌘P/⌘⇧P all land here.

// Palette layout constants (DIP): mainstream command palettes run a
// 560-680 panel, a tall query field, and a capped scrolling list.
const (
	commandCenterWidth     = 560
	commandCenterListCap   = 384
	commandCenterMaxShown  = 100
	commandCenterPageSize  = 8
	commandCenterNoReveals = -1
)

// handleCommandCenterShortcuts opens the palette: Cmd/Ctrl+Shift+P → All,
// Cmd/Ctrl+P → Navigation.
func (s *Shell) handleCommandCenterShortcuts(c *ui.Context) {
	if s.commandCenterOpen {
		return
	}
	switch {
	case c.Shortcut(ui.Super|ui.Shift, ui.KeyP):
		s.openCommandCenter(commandcenter.ScopeAll)
	case c.Shortcut(ui.Super, ui.KeyP):
		s.openCommandCenter(commandcenter.ScopeNavigation)
	}
}

// openCommandCenter snapshots the palette index from application state and
// opens the overlay.
func (s *Shell) openCommandCenter(scope commandcenter.Scope) {
	s.commandCenterIndexCache = commandcenter.Build(s.buildCommandCenterActions())
	s.commandCenterScope = scope
	s.commandCenterQuery = ""
	s.commandCenterSelected = 0
	s.commandCenterReveal = commandCenterNoReveals
	s.commandCenterOpen = true
}

func (s *Shell) closeCommandCenter() {
	s.commandCenterOpen = false
	s.commandCenterQuery = ""
	s.commandCenterSelected = 0
	s.commandCenterReveal = commandCenterNoReveals
}

// buildCommandCenterActions derives the Action Registry from the current
// application snapshot and capabilities (0.9 P2): the single metadata source
// shared by the Command Center, the shortcut reference and native menus.
func (s *Shell) buildCommandCenterActions() []commandcenter.Action {
	actions := make([]commandcenter.Action, 0, 32)

	// Navigation: projects → tabs → panes, straight from the projection.
	for _, project := range s.projection.Projects {
		actions = append(actions, commandcenter.Action{
			ID: "nav.project." + project.ID, Title: "Project: " + project.Label,
			Section: "Navigation", Kind: commandcenter.TargetProject, TargetID: project.ID,
			ScopeFilter: commandcenter.ScopeNavigation, Available: true,
		})
	}
	for _, tab := range s.projection.Tabs {
		actions = append(actions, commandcenter.Action{
			ID: "nav.tab." + tab.ID, Title: "Tab: " + tab.Label,
			Section: "Navigation", Kind: commandcenter.TargetTab, TargetID: tab.ID,
			ScopeFilter: commandcenter.ScopeNavigation, Available: true,
		})
	}
	for _, pane := range s.projection.Panes {
		label := pane.Label
		if label == "" {
			label = pane.ID
		}
		actions = append(actions, commandcenter.Action{
			ID: "nav.pane." + pane.ID, Title: "Pane: " + label,
			Section: "Navigation", Kind: commandcenter.TargetPane, TargetID: pane.ID,
			ScopeFilter: commandcenter.ScopeNavigation, Available: pane.TerminalID != "",
		})
	}

	// Agents from the reconciled workbench.
	for _, card := range s.workbenchCards() {
		actions = append(actions, commandcenter.Action{
			ID: "agent.open." + card.Key.TerminalID, Title: "Agent: " + card.Title,
			Section: "Agents", Keywords: []string{"open agent", card.Provider.DisplayName()},
			Kind: commandcenter.TargetAgent, TargetID: card.Key.TerminalID,
			Available: true,
		})
	}

	// Recent History from the already-loaded management cache (no IO).
	for _, summary := range s.historyProjects {
		actions = append(actions, commandcenter.Action{
			ID: "history.open." + summary.Meta.Key, Title: "History: " + fallbackText(summary.Meta.Title, history.Untitled),
			Section: "History", Kind: commandcenter.TargetHistory, TargetID: summary.Meta.Key,
			Available: true,
		})
	}

	// App actions.
	actions = append(actions,
		commandcenter.Action{ID: "app.history", Title: "History", Section: "App",
			Kind: commandcenter.TargetRoute, TargetID: "/history", Available: true},
		commandcenter.Action{ID: "app.history-projects", Title: "History by Project", Section: "App",
			Keywords: []string{"projects"}, Kind: commandcenter.TargetRoute, TargetID: "/history-projects", Available: true},
		// One Settings entry: the old Appearance/General pair both pushed
		// /settings/general and read as two identical commands (F16).
		commandcenter.Action{ID: "app.settings", Title: "Settings", Section: "App",
			Keywords: []string{"preferences", "appearance", "theme", "density", "language"}, Kind: commandcenter.TargetSetting, TargetID: "/settings/general", Available: true},
		commandcenter.Action{ID: "app.toggle-right-panel", Title: "Toggle Right Panel", Section: "App", Shortcut: "⌥⌘B",
			Keywords: []string{"files", "services", "tools"}, Kind: commandcenter.TargetRoute, TargetID: "action:toggle-right-panel", Available: true},
		commandcenter.Action{ID: "app.open-files", Title: "Open Files Tool", Section: "App",
			Keywords: []string{"directory", "tree", "file preview"}, Kind: commandcenter.TargetRoute, TargetID: "action:open-files", Available: true},
	)
	return actions
}

// commandCenterGroup is one titled block of the palette list.
type commandCenterGroup struct {
	Section string
	Results []commandcenter.Result
}

// groupCommandCenterResults splits the ranked results into consecutive
// section groups: Query already sorts by section inside a match class, so
// one pass closes a group wherever the section changes.
func groupCommandCenterResults(results []commandcenter.Result) []commandCenterGroup {
	groups := make([]commandCenterGroup, 0, 4)
	for _, result := range results {
		if n := len(groups); n > 0 && groups[n-1].Section == result.Action.Section {
			groups[n-1].Results = append(groups[n-1].Results, result)
			continue
		}
		groups = append(groups, commandCenterGroup{
			Section: result.Action.Section,
			Results: []commandcenter.Result{result},
		})
	}
	return groups
}

// commandCenterOverlay renders the palette while open (0.9 P1): one
// DialogBase panel styled once — glass, a single corner radius, one
// border — carrying the query field, the grouped results and the key
// hints. No nested panel inside it: the old Modal-in-Column pair drew two
// rounded containers.
func (s *Shell) commandCenterOverlay(c *ui.Context) {
	if !s.commandCenterOpen {
		return
	}
	sp := Spacing()
	winW, winH := c.Size()
	ui.DialogBase(c, &s.commandCenterOpen, func(back, panel *ui.Element) {
		back.Background(ui.RGBA(0, 0, 0, 0.4))
		// Liquid Glass (MyGo 0.2.15): the palette frosts the dimmed page
		// under it instead of painting a flat panel — the macOS-26 overlay
		// look. glass.Regular keeps rows readable over any content.
		panel.Width(commandCenterWidth).MaxWidth(winW-2*sp.XL).MaxHeight(winH-2*sp.XL).
			Padding(sp.L).Gap(sp.S).Radius(Radius().Card).
			Material(glass.Glass{}).
			Border(1, designTokens(c.Theme().Dark).BorderSubtle).
			Shadow(0, 10, 30, 0, ui.RGBA(0, 0, 0, 0.3))
		// AutoFocus: the palette is keyboard-first from open (WIX-021) —
		// the field hands the focus to its editor (mygo field-focus seam).
		field := ui.SearchField(c, &s.commandCenterQuery).Label("Command Center query").
			MinHeight(Controls().PaletteField).AutoFocus()
		s.commandCenterQueryFocused = field.FocusWithin()
		if field.Changed() {
			s.commandCenterSelected = 0
			s.commandCenterReveal = 0
		}
		results := s.commandCenterIndexCache.Query(s.commandCenterQuery, s.commandCenterScope)
		if s.commandCenterSelected >= len(results) {
			s.commandCenterSelected = len(results) - 1
		}
		if s.commandCenterSelected < 0 {
			s.commandCenterSelected = 0
		}
		s.commandCenterListKeys(field, len(results))
		if field.Submitted() {
			// The submitted action recomputes from the CURRENT query: an
			// IME commit or fast typing may land in the same pass.
			s.commandCenterSubmit(s.commandCenterIndexCache.Query(s.commandCenterQuery, s.commandCenterScope))
		}
		total := len(results)
		if total > commandCenterMaxShown {
			results = results[:commandCenterMaxShown]
		}
		if total == 0 {
			ui.Text(c, "No matching commands").FontSize(Typography().Body).TextColor(c.Theme().TextMuted)
			return
		}
		// The grouped list scrolls past the cap; the selection reveals
		// itself only when the keys or a new query moved it — never while
		// the pointer wanders (ScrollIntoView every frame would fight the
		// user's scrolling).
		ui.Scroll(c).FillWidth().MaxHeight(commandCenterListCap).Children(func() {
			flat := 0
			for gi, group := range groupCommandCenterResults(results) {
				if gi > 0 {
					ui.Box(c).FillWidth().Height(1).Margin(0, sp.M).
						Background(designTokens(c.Theme().Dark).BorderSubtle)
				}
				ui.Row(c).FillWidth().Padding(sp.S, sp.M).Children(func() {
					ui.Text(c, strings.ToUpper(group.Section)).
						FontSize(Typography().Micro).TextColor(c.Theme().TextMuted)
				})
				for _, result := range group.Results {
					s.commandCenterRow(c, flat, result)
					flat++
				}
			}
		})
		shown := fmt.Sprintf("%d result%s", total, pluralS(total))
		if total > len(results) {
			shown = fmt.Sprintf("%d of %d results", len(results), total)
		}
		hint := "↑↓ select · Enter open · Esc close"
		if s.commandCenterScope == commandcenter.ScopeNavigation {
			hint = "Navigation — " + hint
		}
		ui.Text(c, shown+" · "+hint).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
	})
}

// commandCenterRow builds one result row: hover selects it, click runs
// it, and the row the keyboard moved to reveals itself in the list.
func (s *Shell) commandCenterRow(c *ui.Context, i int, result commandcenter.Result) {
	sp := Spacing()
	row := ui.Row(c).FillWidth().Padding(sp.S, sp.M).Gap(sp.S).AlignItems(ui.Center).
		Radius(Radius().Control)
	row.Children(func() {
		ui.Text(c, result.Action.Title).FontSize(Typography().Body).Grow(1).SingleLine()
		if result.Action.Shortcut != "" {
			ui.Text(c, result.Action.Shortcut).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
		}
	})
	if i == s.commandCenterSelected {
		row.Background(designTokens(c.Theme().Dark).StatusBackground(ToneInfo, c.Theme().Dark))
	}
	if i == s.commandCenterReveal {
		row.ScrollIntoView()
		s.commandCenterReveal = commandCenterNoReveals
	}
	if row.Hovered() {
		s.commandCenterSelected = i
	}
	if row.Clicked() {
		s.executeCommandCenterResult(result)
	}
}

// commandCenterListKeys claims the palette's list keys on the search
// field: the field owns the keyboard from open, so the claims ride its
// focus chain (the Combobox pattern) — the window-level fallback the old
// version used would never fire past a focused editor. Arrows wrap and
// PgUp/PgDn jump; every move reveals its row. Home/End stay with the
// caret: the editor takes them unconditionally, as mainstream palettes
// do while their input holds the focus.
func (s *Shell) commandCenterListKeys(field *ui.Element, n int) {
	if n == 0 {
		return
	}
	down := field.Shortcut(0, ui.KeyDown)
	up := field.Shortcut(0, ui.KeyUp)
	pageDown := field.Shortcut(0, ui.KeyPageDown)
	pageUp := field.Shortcut(0, ui.KeyPageUp)
	move := func(i int) {
		s.commandCenterSelected = i
		s.commandCenterReveal = i
	}
	switch {
	case down:
		move((s.commandCenterSelected + 1) % n)
	case up:
		move((s.commandCenterSelected - 1 + n) % n)
	case pageDown:
		move(min(s.commandCenterSelected+commandCenterPageSize, n-1))
	case pageUp:
		move(max(s.commandCenterSelected-commandCenterPageSize, 0))
	}
}

// commandCenterSubmit commits the selected result (the search field's
// Submitted path).
func (s *Shell) commandCenterSubmit(results []commandcenter.Result) {
	if s.commandCenterSelected < len(results) {
		s.executeCommandCenterResult(results[s.commandCenterSelected])
	}
}

// executeCommandCenterResult revalidates the typed stable target against
// the application snapshot, then navigates client-locally. A stale target
// surfaces recoverable text instead of acting (0.9 P1).
func (s *Shell) executeCommandCenterResult(result commandcenter.Result) {
	action := result.Action
	s.closeCommandCenter()
	switch action.Kind {
	case commandcenter.TargetPane:
		if paneByID(s.projection, action.TargetID) == nil {
			s.status = "That pane is gone — pick another from the Command Center."
			return
		}
		s.selectPane(action.TargetID)
		s.router.Push(routeWorkspace)
	case commandcenter.TargetTab:
		if !s.projectionHasTab(action.TargetID) {
			s.status = "That tab is gone — pick another from the Command Center."
			return
		}
		s.selectTab(action.TargetID)
		s.router.Push(routeWorkspace)
	case commandcenter.TargetProject:
		if !s.projectionHasProject(action.TargetID) {
			s.status = "That workspace is gone — pick another from the Command Center."
			return
		}
		s.selectProject(action.TargetID)
		s.router.Push(routeWorkspace)
	case commandcenter.TargetAgent:
		if card, ok := s.workbench.directory.Get(agent.AgentKey{InstanceID: s.activeInstance, TerminalID: action.TargetID}); ok {
			s.openAgentCard(card)
			return
		}
		s.status = "That agent is no longer live."
	case commandcenter.TargetHistory:
		s.router.Push("/history/" + action.TargetID)
	default:
		switch action.TargetID {
		case "action:toggle-right-panel":
			s.toggleRightPanel()
		case "action:open-files":
			s.openRightPanelSurface(SurfaceFiles)
		default:
			if action.TargetID != "" {
				s.router.Push(action.TargetID)
			}
		}
	}
}

// projectionHasTab / projectionHasProject revalidate navigation targets
// against the current snapshot (pure reads, no RPC).
func (s *Shell) projectionHasTab(tabID string) bool {
	for _, tab := range s.projection.Tabs {
		if tab.ID == tabID {
			return true
		}
	}
	return false
}

func (s *Shell) projectionHasProject(projectID string) bool {
	for _, project := range s.projection.Projects {
		if project.ID == projectID {
			return true
		}
	}
	return false
}
