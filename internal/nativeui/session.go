package nativeui

import (
	"sort"
	"strings"

	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/settings"
)

// The session snapshot (settings.Session) remembers the shell's navigation
// and panel cosmetics across launches. It is captured when the window hides
// (close button) and when the shell closes (menu Quit), and restored as
// launch hints in NewShell. Herdr stays the runtime authority: instance/
// project/tab/pane entries are hints that reconcileLocalSelection and
// preferredInstanceName re-validate against the live projection.

// persistableRoute maps the current router path onto the route restored at
// next launch. Stable top-level routes and Settings sections come back
// verbatim; context-scoped detail routes (a specific history session, agent
// inspector pane, or chat binding) normalize to the workspace page because
// their targets may no longer exist.
func persistableRoute(path string) string {
	switch path {
	case routeWorkspace, routeHistory, "/history-projects":
		return path
	}
	if strings.HasPrefix(path, "/settings/") {
		return path
	}
	return routeWorkspace
}

// restorableRoute reports whether a persisted route can come back verbatim.
// Routes removed from the app (e.g. the deleted New Task / Status Center
// pages) normalize to the workspace instead of rendering a Not Found page.
func restorableRoute(path string) bool {
	return persistableRoute(path) == path
}

// sessionHasState reports whether a snapshot carries anything worth
// restoring. A fresh install stores the zero value, and the shell must keep
// its first-launch defaults instead of applying the zero booleans.
func sessionHasState(session settings.SessionSettings) bool {
	return session.Route != "" || session.Instance != "" ||
		session.ProjectID != "" || session.ExpandedProjects != nil ||
		session.ExpandedTabs != nil
}

// sortedSetKeys returns the true keys of an expansion map in stable order.
func sortedSetKeys(expanded map[string]bool) []string {
	keys := make([]string, 0, len(expanded))
	for id, on := range expanded {
		if on {
			keys = append(keys, id)
		}
	}
	sort.Strings(keys)
	return keys
}

// clampSessionWidth bounds a restored right-panel width to the drag limits.
func clampSessionWidth(width int) int {
	if width < MinRightPanelWidth {
		return MinRightPanelWidth
	}
	if width > MaxRightPanelWidth {
		return MaxRightPanelWidth
	}
	return width
}

// captureSession reads the shell's navigation and panel cosmetics into a
// snapshot. Pure read over presentation state; safe from any lane that
// already owns the shell.
func (s *Shell) captureSession() settings.SessionSettings {
	return settings.SessionSettings{
		Route:             persistableRoute(s.router.Path()),
		Instance:          s.activeInstance,
		ProjectID:         s.selectedProjectID,
		TabID:             s.selectedTabID,
		PaneID:            s.selectedPaneID,
		ProjectsOpen:      s.projectsOpen,
		AgentsOpen:        s.agentsOpen,
		PinsOpen:          s.pinsOpen,
		ExpandedProjects:  sortedSetKeys(s.expandedProjects),
		ExpandedTabs:      sortedSetKeys(s.expandedTabs),
		RightPanelOpen:    s.rightPanel.open,
		RightPanelSurface: string(s.rightPanel.surface),
		RightPanelWidth:   s.rightPanel.width,
	}
}

// SaveSession persists the current session snapshot through the settings
// service. Called when the main window hides (close button) and from
// Shell.Close (menu Quit).
func (s *Shell) SaveSession() {
	snapshot := s.captureSession()
	s.applySettings(func(v *settings.Settings) error {
		v.Session = snapshot
		return nil
	})
}

// restoreSession applies the persisted session snapshot as launch hints.
// Called once from NewShell, before the window attaches: the instance hint
// steers reloadInstances, the selection hints survive until the first
// projection arrives and reconcileLocalSelection validates them, and the
// route hint replaces the default workspace page before the first frame.
func (s *Shell) restoreSession() {
	session := s.settings.Session
	if !sessionHasState(session) {
		return
	}
	if session.Route != "" && session.Route != s.visibleRoute && restorableRoute(session.Route) {
		s.router.Replace(session.Route)
		s.visibleRoute = session.Route
	}
	if session.Instance != "" {
		s.activeInstance = session.Instance
	}
	s.selectedProjectID = session.ProjectID
	s.selectedTabID = session.TabID
	s.selectedPaneID = session.PaneID
	s.projectsOpen = session.ProjectsOpen
	s.agentsOpen = session.AgentsOpen
	s.pinsOpen = session.PinsOpen
	for _, id := range session.ExpandedProjects {
		s.expandedProjects[id] = true
	}
	for _, id := range session.ExpandedTabs {
		s.expandedTabs[id] = true
	}
	s.rightPanel.open = session.RightPanelOpen
	switch RightPanelSurface(session.RightPanelSurface) {
	case SurfaceChanges, SurfaceFiles, SurfaceServices:
		s.rightPanel.surface = RightPanelSurface(session.RightPanelSurface)
	}
	if session.RightPanelWidth != 0 {
		s.rightPanel.width = clampSessionWidth(session.RightPanelWidth)
	}
}

// preferredInstanceName resolves the startup instance: the session hint when
// it is still listed, else the Herdr-flagged default, else the first
// instance. A stale hint (renamed or removed instance) never blocks startup.
func preferredInstanceName(instances []herdr.Instance, hint string) string {
	if hint != "" {
		for _, instance := range instances {
			if instance.Name == hint {
				return hint
			}
		}
	}
	for _, instance := range instances {
		if instance.Default {
			return instance.Name
		}
	}
	if len(instances) > 0 {
		return instances[0].Name
	}
	return ""
}
