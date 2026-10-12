package nativeui

import (
	"log/slog"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

// reconcileLocalSelection preserves this window's semantic selection across
// runtime snapshots. Herdr focused_* fields are bootstrap/fallback facts only.
func (s *Shell) reconcileLocalSelection(next herdr.Projection) {
	previousProject := s.selectedProjectID
	previousTab := s.selectedTabID

	if !projectExists(next, s.selectedProjectID) {
		s.selectedProjectID = fallbackProjectID(next)
	}
	if !tabBelongsToProject(next, s.selectedTabID, s.selectedProjectID) {
		s.selectedTabID = fallbackTabID(next, s.selectedProjectID)
	}
	if !paneBelongsToTab(next, s.selectedPaneID, s.selectedTabID) {
		s.selectedPaneID = fallbackPaneID(next, s.selectedTabID)
	}
	if layout, ok := layoutForTab(next, s.selectedTabID); ok && layout.Zoomed && layout.FocusedPaneID != "" {
		if paneBelongsToTab(next, layout.FocusedPaneID, s.selectedTabID) {
			s.selectedPaneID = layout.FocusedPaneID
		}
	}

	s.reconcileTreeExpansion(previousProject, s.selectedProjectID, true)
	s.reconcileTreeExpansion(previousTab, s.selectedTabID, false)
}

func (s *Shell) adoptRuntimeSelection(next herdr.Projection) {
	s.selectedProjectID = next.FocusedProjectID
	s.selectedTabID = next.FocusedTabID
	s.selectedPaneID = next.FocusedPaneID
	s.reconcileLocalSelection(next)
}

func (s *Shell) clearLocalSelection() {
	s.selectedProjectID = ""
	s.selectedTabID = ""
	s.selectedPaneID = ""
}

func (s *Shell) selectProject(projectID string) {
	defer s.syncWorkspaceForSelection()
	if !projectExists(s.projection, projectID) {
		return
	}
	s.selectedProjectID = projectID
	s.selectedTabID = ""
	s.selectedPaneID = ""
	s.reconcileLocalSelection(s.projection)
	s.syncSelectionPresentation()
}

func (s *Shell) selectTab(tabID string) {
	defer s.syncWorkspaceForSelection()
	tab := tabByID(s.projection, tabID)
	if tab == nil {
		return
	}
	// Parent selection shows the whole split: when the Tab is maximized,
	// its maximized Pane steps down (zoom seam; unzoomed Tabs owe nothing).
	if target, mode, ok := navigationZoomMutation(s.projection, tab.ID, ""); ok {
		s.runNavigationZoom(target, mode)
	}
	s.selectedProjectID = tab.ProjectID
	s.selectedTabID = tab.ID
	s.selectedPaneID = ""
	s.reconcileLocalSelection(s.projection)
	s.syncSelectionPresentation()
}

func (s *Shell) selectPane(paneID string) {
	defer s.syncWorkspaceForSelection()
	pane := paneByID(s.projection, paneID)
	if pane == nil {
		s.selectedPaneID = paneID
		return
	}
	// Zoom-follow (2026-10-13 decision): a jump into an already maximized
	// Tab moves the maximization to the clicked Pane — Herdr's zoomed layout
	// shows exactly one Pane, so without this the target stays invisible
	// beneath the old maximized Pane and the next snapshot snaps the
	// selection back. Unzoomed Tabs are never zoomed by navigation.
	if target, mode, ok := navigationZoomMutation(s.projection, pane.TabID, pane.ID); ok {
		s.runNavigationZoom(target, mode)
	}
	s.selectedProjectID = pane.ProjectID
	s.selectedTabID = pane.TabID
	s.selectedPaneID = pane.ID
	s.setProjectExpanded(pane.ProjectID, true)
	s.setTabExpanded(pane.TabID, true)
	s.syncSelectionPresentation()
}

// navigationZoomMutation resolves the pane.zoom a navigation click owes.
// Child jump (paneID != ""): a zoomed Tab keeps maximizing exactly one Pane,
// so the maximization moves to the clicked target (mode "on"). Parent Tab
// selection (paneID == ""): the maximized Pane steps down so the whole split
// shows (mode "off"). Non-zoomed Tabs and no-op jumps owe nothing. Modes are
// the live PaneZoomMode enum (toggle|on|off).
func navigationZoomMutation(projection herdr.Projection, tabID, paneID string) (target string, mode string, ok bool) {
	layout, exists := layoutForTab(projection, tabID)
	if !exists || !layout.Zoomed {
		return "", "", false
	}
	if paneID != "" {
		if layout.FocusedPaneID == paneID {
			return "", "", false
		}
		return paneID, "on", true
	}
	if layout.FocusedPaneID == "" {
		return "", "", false
	}
	return layout.FocusedPaneID, "off", true
}

// runNavigationZoom submits the navigation seam's zoom mutation. mutate
// drops it headlessly (no window / instance), keeping selection tests
// presentation-local.
func (s *Shell) runNavigationZoom(target, mode string) {
	slog.Info("pane action", "action", "zoom-follow", "pane_id", target, "mode", mode)
	s.mutate(func() (herdr.Projection, error) {
		return s.runtime.ZoomPane(s.activeInstance, target, mode)
	})
}

func (s *Shell) syncSelectionPresentation() {
	s.selected = selectionSidebarKey(s.projection, s.selectedProjectID, s.selectedTabID, s.selectedPaneID)
	if s.win == nil || s.activeInstance == "" || s.router.Path() != routeWorkspace {
		return
	}
	if err := s.syncTerminals(s.projection); err != nil {
		s.errText = err.Error()
		s.status = "Terminal attach failed"
	}
}

func (s *Shell) selectedProject() *herdr.Project {
	for index := range s.projection.Projects {
		if s.projection.Projects[index].ID == s.selectedProjectID {
			return &s.projection.Projects[index]
		}
	}
	return nil
}

func (s *Shell) selectedTab() *herdr.Tab {
	return tabByID(s.projection, s.selectedTabID)
}

func (s *Shell) selectedPane() *herdr.Pane {
	return paneByID(s.projection, s.selectedPaneID)
}

func (s *Shell) selectedLayout() (herdr.Layout, bool) {
	return layoutForTab(s.projection, s.selectedTabID)
}

// selectedTabCWD resolves the active tool root for the right panel (0.9
// P3/WIX-041): selected Pane cwd → first Pane in Tab cwd → Project cwd.
// A Project does not have one universal cwd; tools strictly follow the
// Tab's active working directory.
func (s *Shell) selectedTabCWD() string {
	if pane := s.selectedPane(); pane != nil && pane.CWD != "" {
		return pane.CWD
	}
	for _, pane := range s.projection.Panes {
		if pane.TabID == s.selectedTabID && pane.CWD != "" {
			return pane.CWD
		}
	}
	if project := s.selectedProject(); project != nil && project.CWD != "" {
		return project.CWD
	}
	return ""
}

func fallbackProjectID(projection herdr.Projection) string {
	if projectExists(projection, projection.FocusedProjectID) {
		return projection.FocusedProjectID
	}
	if len(projection.Projects) > 0 {
		return projection.Projects[0].ID
	}
	return ""
}

func fallbackTabID(projection herdr.Projection, projectID string) string {
	for _, project := range projection.Projects {
		if project.ID == projectID && tabBelongsToProject(projection, project.ActiveTabID, projectID) {
			return project.ActiveTabID
		}
	}
	if tabBelongsToProject(projection, projection.FocusedTabID, projectID) {
		return projection.FocusedTabID
	}
	for _, tab := range projection.Tabs {
		if tab.ProjectID == projectID {
			return tab.ID
		}
	}
	return ""
}

func fallbackPaneID(projection herdr.Projection, tabID string) string {
	if layout, ok := layoutForTab(projection, tabID); ok {
		if paneBelongsToTab(projection, layout.FocusedPaneID, tabID) {
			return layout.FocusedPaneID
		}
	}
	if paneBelongsToTab(projection, projection.FocusedPaneID, tabID) {
		return projection.FocusedPaneID
	}
	for _, pane := range projection.Panes {
		if pane.TabID == tabID {
			return pane.ID
		}
	}
	return ""
}

func projectExists(projection herdr.Projection, projectID string) bool {
	if projectID == "" {
		return false
	}
	for _, project := range projection.Projects {
		if project.ID == projectID {
			return true
		}
	}
	return false
}

func tabBelongsToProject(projection herdr.Projection, tabID, projectID string) bool {
	tab := tabByID(projection, tabID)
	return tab != nil && tab.ProjectID == projectID
}

func paneBelongsToTab(projection herdr.Projection, paneID, tabID string) bool {
	pane := paneByID(projection, paneID)
	return pane != nil && pane.TabID == tabID
}

func tabByID(projection herdr.Projection, tabID string) *herdr.Tab {
	for index := range projection.Tabs {
		if projection.Tabs[index].ID == tabID {
			return &projection.Tabs[index]
		}
	}
	return nil
}

func paneByID(projection herdr.Projection, paneID string) *herdr.Pane {
	for index := range projection.Panes {
		if projection.Panes[index].ID == paneID {
			return &projection.Panes[index]
		}
	}
	return nil
}

func layoutForTab(projection herdr.Projection, tabID string) (herdr.Layout, bool) {
	for _, layout := range projection.Layouts {
		if layout.TabID == tabID {
			return layout, true
		}
	}
	return herdr.Layout{}, false
}

func selectionSidebarKey(projection herdr.Projection, projectID, tabID, paneID string) string {
	if tabID != "" {
		for _, tab := range projection.Tabs {
			if tab.ID == tabID {
				if tab.PaneCount > 1 && paneID != "" {
					return "pane:" + paneID
				}
				return "tab:" + tabID
			}
		}
		return "tab:" + tabID
	}
	if projectID != "" {
		return "project:" + projectID
	}
	return ""
}
