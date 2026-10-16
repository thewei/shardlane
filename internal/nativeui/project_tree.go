package nativeui

/**
 * [INPUT]: 依赖 Herdr 权威 Workspace/Tab 顺序及 Pane layout、reorder.go 的受限拖动规则
 * [OUTPUT]: 提供侧栏 Workspace → Tab → Pane 交互树、排序与原生拖放入口
 * [POS]: nativeui 的运行时导航树；状态标识不再干扰用户持久化的拖动顺序
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

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
	projects := orderedProjects(s.projection)
	activity := buildSidebarActivity(s.projection, s.serviceIndex)
	for _, project := range projects {
		project := project
		selectedProject := project.ID == s.selectedProjectID
		tabs := tabsForProject(s.projection, project.ID)
		expandedProject := s.projectExpanded(project.ID)
		// The pane tally (2026-10-07, widened 2026-10-15 user request): the
		// project row shows how many panes run beneath it whether it is
		// folded or expanded, so the nesting depth never hides the load.
		projectPaneCount := 0
		for _, tab := range tabs {
			projectPaneCount += len(panesForTab(s.projection, tab.ID))
		}
		projectCount := projectPaneCount
		glyph := iconFolder
		if expandedProject {
			glyph = iconFolderOpen
		}
		row := treeRow(c, treeRowSpec{
			Height:     s.sidebarRowHeight(),
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
			Action:     &treeRowAction{Label: "New tab in " + project.Label, Glyph: iconPlus, Run: func() { s.newProjectTab(project) }},
			Drag:       s.treeDragFor(treeNodeDrag{s.activeInstance, "project", "", project.ID}),
		})
		if row.Clicked() {
			if selectedProject {
				s.setProjectExpanded(project.ID, !expandedProject)
			} else {
				// The selection lands as /workspace?project=… (one history
				// entry); no bare page push beside it.
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
			sortPanesByLayout(s.projection, tab.ID, panes)
			expandedTab := s.tabExpanded(tab.ID)
			// A folded tab shows its pane tally; an expanded one (and a
			// single-pane leaf) shows none.
			tabCount := 0
			if !expandedTab {
				tabCount = len(panes)
			}
			row := treeRow(c, treeRowSpec{
				Height:     s.sidebarRowHeight(),
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
				Drag:       s.treeDragFor(treeNodeDrag{s.activeInstance, "tab", project.ID, tab.ID}),
			})
			if row.Clicked() {
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
					if brand, ok := s.procMarkForPaneWithTheme(pane.ID, dark); ok {
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
					Height:            s.sidebarRowHeight(),
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
					Drag:              s.treeDragFor(treeNodeDrag{s.activeInstance, "pane", tab.ID, pane.ID}),
				})
				if row.Clicked() {
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

// newProjectTab opens a Tab in project: the project row's hover "+" and its
// context menu's New Tab share it.
func (s *Shell) newProjectTab(project herdr.Project) {
	s.mutateSelecting(func() (herdr.Projection, error) {
		return s.runtime.CreateTab(s.activeInstance, project.ID, project.CWD)
	})
}

// projectMenuItems is the shared New Tab / Rename / Close builder behind the
// project row's right-click menu.
func (s *Shell) projectMenuItems(m *ui.Menu, project herdr.Project) {
	if m.Item("New Tab").Chosen() {
		s.newProjectTab(project)
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
	sort.SliceStable(result, func(i, j int) bool {
		return result[i].Number < result[j].Number
	})
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
