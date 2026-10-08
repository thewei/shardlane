package nativeui

import (
	"sort"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

func (s *Shell) projectItems(c *ui.Context) {
	dark := c.Theme().Dark
	if s.activeInstance == "" {
		ui.Text(c, "Choose a Workspace first").Padding(8, 10).TextColor(c.Theme().TextMuted)
		return
	}
	projects := s.projection.Projects
	activity := buildSidebarActivity(s.projection, s.serviceIndex)
	for _, project := range projects {
		project := project
		selectedProject := project.ID == s.selectedProjectID
		tabs := tabsForProject(s.projection, project.ID)
		s.sortTabsActionable(tabs)
		expandedProject := s.projectExpanded(project.ID)
		// The collapsed tally (2026-10-07): a folded project shows how
		// many panes it hides, so the nesting depth never hides the load.
		projectPaneCount := 0
		for _, tab := range tabs {
			projectPaneCount += len(panesForTab(s.projection, tab.ID))
		}
		projectCount := 0
		if !expandedProject {
			projectCount = projectPaneCount
		}
		glyph := iconFolder
		if expandedProject {
			glyph = iconFolderOpen
		}
		row := treeRow(c, treeRowSpec{
			Key:        "project:" + project.ID,
			Depth:      0,
			Selected:   selectedProject && s.selectedTabID == "",
			Expandable: len(tabs) > 0,
			Expanded:   expandedProject,
			Mark:       visualMark{svg: glyph},
			Label:      project.Label,
			Status:     project.AgentStatus,
			Activity:   activity[project.ID],
			Count:      projectCount,
			Overflow: func(m *ui.Menu) {
				s.projectMenuItems(m, project)
			},
		})
		if row.Clicked() {
			s.router.Push(routeWorkspace)
			if selectedProject {
				s.setProjectExpanded(project.ID, !expandedProject)
			} else {
				s.setProjectExpanded(project.ID, true)
				s.selectProject(project.ID)
			}
		}
		row.ContextMenu(func(m *ui.Menu) { s.projectMenuItems(m, project) })
		if !expandedProject {
			continue
		}

		for tabIndex, tab := range tabs {
			tab := tab
			lastTab := tabIndex == len(tabs)-1
			selectedTab := tab.ID == s.selectedTabID
			panes := panesForTab(s.projection, tab.ID)
			s.sortPanesActionable(panes)
			expandedTab := s.tabExpanded(tab.ID)
			// A folded tab shows its pane tally; an expanded one (and a
			// single-pane leaf) shows none.
			tabCount := 0
			if !expandedTab {
				tabCount = len(panes)
			}
			row := treeRow(c, treeRowSpec{
				Key:        "tab:" + tab.ID,
				Depth:      1,
				Last:       lastTab,
				Selected:   selectedTab && tab.PaneCount <= 1,
				Expandable: len(panes) > 1,
				Expanded:   expandedTab,
				Mark:       s.tabRowMark(panes),
				Label:      tab.Label,
				Status:     tab.AgentStatus,
				Activity:   activity[tab.ID],
				Count:      tabCount,
				Overflow: func(m *ui.Menu) {
					s.tabMenuItems(m, tab)
				},
			})
			if row.Clicked() {
				s.router.Push(routeWorkspace)
				if selectedTab && len(panes) > 1 {
					s.setTabExpanded(tab.ID, !expandedTab)
				} else if !selectedTab {
					s.setTabExpanded(tab.ID, true)
					s.selectTab(tab.ID)
				}
			}
			row.ContextMenu(func(m *ui.Menu) { s.tabMenuItems(m, tab) })
			if !expandedTab || len(panes) <= 1 {
				continue
			}

			for paneIndex, pane := range panes {
				pane := pane
				label := fallbackText(pane.Label, "Pane")
				mark, isAgent := s.agentMarkForPane(pane.ID, dark)
				if !isAgent {
					// A Pane running a recognizable program shows that
					// program's brand (2026-10-06 A11); the rest keep the
					// terminal glyph.
					if brand, ok := s.procMarkForPane(pane.ID); ok {
						mark = brand
					}
				}
				dot := opUnknown
				if isAgent {
					if a := s.paneAgent(pane.ID); a != nil {
						dot = normalizeRuntimeStatus(a.Status)
					}
				}
				row := treeRow(c, treeRowSpec{
					Key:               "pane:" + pane.ID,
					Depth:             2,
					AncestorContinues: []bool{!lastTab},
					Last:              paneIndex == len(panes)-1,
					Selected:          pane.ID == s.selectedPaneID,
					Mark:              mark,
					Dot:               dot,
					Label:             label,
					Status:            pane.AgentStatus,
					Activity:          activity[pane.ID],
					Overflow: func(m *ui.Menu) {
						s.paneOverflowItems(m, pane.ID, label)
					},
				})
				if row.Clicked() {
					s.router.Push(routeWorkspace)
					s.selectPane(pane.ID)
				}
				row.ContextMenu(func(m *ui.Menu) { s.paneOverflowItems(m, pane.ID, label) })
			}
		}
	}
	if len(projects) == 0 && !s.loading {
		ui.Text(c, "No Workspaces in this Session").Padding(8, 10).TextColor(c.Theme().TextMuted)
	}
}

// tabRowMark picks a collapsed Tab row's leading mark (2026-10-06 A5): a
// Tab presenting a single non-Agent Pane that runs a recognized program —
// a long-lived dev server, a database — shows that program's brand, so the
// running service is visible without expanding the Tab. Everything else
// keeps the plain tab glyph; the icon-corner dot carries the running state.
func (s *Shell) tabRowMark(panes []herdr.Pane) visualMark {
	if len(panes) == 1 && s.paneAgent(panes[0].ID) == nil {
		if brand, ok := s.procMarkForPane(panes[0].ID); ok {
			return brand
		}
	}
	return visualMark{svg: iconTab}
}

// projectMenuItems is the shared New Tab / Rename / Close builder — the
// row's context menu and its hover "..." overflow are the same actions.
func (s *Shell) projectMenuItems(m *ui.Menu, project herdr.Project) {
	if m.Item("New Tab").Chosen() {
		s.mutateSelecting(func() (herdr.Projection, error) {
			return s.runtime.CreateTab(s.activeInstance, project.ID, project.CWD)
		})
	}
	if m.Item("Rename Project…").Chosen() {
		s.openTextDialog("rename-project", project.ID, "Rename Project", "Name", project.Label)
	}
	m.Separator()
	if m.Item("Close Project…").Chosen() {
		s.openConfirm("close-project", project.ID, "Close Project?", "This closes the Herdr runtime workspace and its Tabs/Panes.")
	}
}

// tabMenuItems is the shared Rename / Close Tab builder.
func (s *Shell) tabMenuItems(m *ui.Menu, tab herdr.Tab) {
	if m.Item("Rename Tab…").Chosen() {
		s.openTextDialog("rename-tab", tab.ID, "Rename Tab", "Name", tab.Label)
	}
	m.Separator()
	if m.Item("Close Tab…").Chosen() {
		s.openConfirm("close-tab", tab.ID, "Close Tab?", "This closes the selected Herdr Tab and all Panes in it.")
	}
}

func tabsForProject(projection herdr.Projection, projectID string) []herdr.Tab {
	result := make([]herdr.Tab, 0, 4)
	for _, tab := range projection.Tabs {
		if tab.ProjectID == projectID {
			result = append(result, tab)
		}
	}
	return result
}

func panesForTab(projection herdr.Projection, tabID string) []herdr.Pane {
	result := make([]herdr.Pane, 0, 4)
	for _, pane := range projection.Panes {
		if pane.TabID == tabID {
			result = append(result, pane)
		}
	}
	return result
}

// paneNavRank is the actionable-first navigation rank of one pane
// (2026-10-07): a pane whose agent waits on the human — attention or
// ready-for-review — comes first, then working agents, then idle agents,
// and the plain/service panes last. The workspace grid, the Agents
// section and the Herdr layout stay untouched: this orders only the
// navigation lists (sidebar tree, breadcrumb switcher), where the
// actionable pane is the one the user wants to reach first.
func paneNavRank(hasAgent bool, state operationalState) int {
	if !hasAgent {
		return 3
	}
	switch state {
	case opNeedsAttention, opReadyForReview:
		return 0
	case opWorking:
		return 1
	default:
		return 2
	}
}

// paneNavRankOf returns the pane's navigation rank from the live
// projection.
func (s *Shell) paneNavRankOf(pane herdr.Pane) int {
	if a := s.paneAgent(pane.ID); a != nil {
		return paneNavRank(true, normalizeRuntimeStatus(a.Status))
	}
	return paneNavRank(false, opUnknown)
}

// sortPanesActionable orders pane rows actionable-first, keeping the
// projection order inside a rank (stable sort).
func (s *Shell) sortPanesActionable(panes []herdr.Pane) {
	sort.SliceStable(panes, func(i, j int) bool {
		return s.paneNavRankOf(panes[i]) < s.paneNavRankOf(panes[j])
	})
}

// sortTabsActionable orders a project's tabs by their most urgent pane,
// so the tab holding the working or blocked agent rises in the tree;
// pane-less tabs keep the tail. Stable, like the pane sort.
func (s *Shell) sortTabsActionable(tabs []herdr.Tab) {
	rank := make(map[string]int, len(tabs))
	for _, tab := range tabs {
		rank[tab.ID] = paneNavRank(false, opUnknown)
		for _, pane := range panesForTab(s.projection, tab.ID) {
			if p := s.paneNavRankOf(pane); p < rank[tab.ID] {
				rank[tab.ID] = p
			}
		}
	}
	sort.SliceStable(tabs, func(i, j int) bool {
		return rank[tabs[i].ID] < rank[tabs[j].ID]
	})
}
