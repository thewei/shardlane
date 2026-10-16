package nativeui

/**
 * [INPUT]: 依赖 Herdr 投影存在性判定（projectExists/tabByID/paneByID/layoutForTab）、runtime 的 pane.zoom mutation 与 Shell 的终端同步
 * [OUTPUT]: 对外提供 Shell.selectProject/selectTab/selectPane（用户选择唯一收口：落地投影状态并把 /workspace?pane=… 位置 Push 进 Router 历史）、applyProjectSelection/applyTabSelection/applyPaneSelection（无历史推送的落地体）、syncSelectionLocation（Back/Forward/⌘[⌘] 走历史时把位置查询静默镜像回选择）、selectionLocation
 * [POS]: 选中状态与页面历史之间的唯一翻译层：用户选择→位置入栈，Router 走历史→选择恢复；reconcile/adopt 永远不经过这里，运行时对账不产生历史
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"log/slog"
	"net/url"

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
	s.applyProjectSelection(projectID)
	s.pushSelectionLocation()
}

// applyProjectSelection is selectProject without the history push: the quiet
// side of syncSelectionLocation.
func (s *Shell) applyProjectSelection(projectID string) {
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
	s.applyTabSelection(tabID)
	s.pushSelectionLocation()
}

// applyTabSelection is selectTab without the history push.
func (s *Shell) applyTabSelection(tabID string) {
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
	s.applyPaneSelection(paneID)
	s.pushSelectionLocation()
}

// applyPaneSelection is selectPane without the history push.
func (s *Shell) applyPaneSelection(paneID string) {
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
	// The pane's own project field is the first fact, but a pane without it
	// (a thin fixture, a degraded snapshot) must not wipe the project
	// selection: the Tab carries the parentage.
	projectID := pane.ProjectID
	if projectID == "" {
		if tab := tabByID(s.projection, pane.TabID); tab != nil {
			projectID = tab.ProjectID
		}
	}
	s.selectedProjectID = projectID
	s.selectedTabID = pane.TabID
	s.selectedPaneID = pane.ID
	s.setProjectExpanded(pane.ProjectID, true)
	s.setTabExpanded(pane.TabID, true)
	s.syncSelectionPresentation()
}

// selectionLocation renders the workspace location recording the current
// selection: project, tab and pane ride in the query together, so pane
// switches become history entries beside page changes and the chrome Back/
// Forward (and ⌘[ / ⌘]) walk them like page changes.
func (s *Shell) selectionLocation() string {
	q := url.Values{}
	if s.selectedProjectID != "" {
		q.Set("project", s.selectedProjectID)
	}
	if s.selectedTabID != "" {
		q.Set("tab", s.selectedTabID)
	}
	if s.selectedPaneID != "" {
		q.Set("pane", s.selectedPaneID)
	}
	if len(q) == 0 {
		return routeWorkspace
	}
	return routeWorkspace + "?" + q.Encode()
}

// pushSelectionLocation records the selection just applied as a history
// entry. Only the workspace's own browsing history records pane switches:
// the same select* calls made while another page owns the route (Files
// re-rooting, Connect flows) are context for that page, not navigation —
// pushing there would yank the user to /workspace. The Router no-ops a
// repeat of the current location, so re-selecting the same node adds
// nothing, and a push cuts the forward entries — the browser rule.
func (s *Shell) pushSelectionLocation() {
	if s.router.Path() != routeWorkspace {
		return
	}
	s.router.Push(s.selectionLocation())
}

// syncSelectionLocation mirrors a workspace location's selection query into
// the shell when the Router walks its history: the entry is the authority
// for what was selected then, so Back and Forward restore pane switches
// exactly as they were made. User selections flow the other way, through the
// select* push. The most specific node of the query decides; when it is
// already selected the mirror is done — never a per-frame re-apply. A node
// the runtime no longer has falls back to its recorded parent, so Back into
// a closed Pane still restores its Tab.
func (s *Shell) syncSelectionLocation() {
	if s.router.Path() != routeWorkspace {
		return
	}
	pane, tab, project := s.router.Query("pane"), s.router.Query("tab"), s.router.Query("project")
	if pane == "" && tab == "" && project == "" {
		return
	}
	if pane != "" {
		if pane == s.selectedPaneID {
			return
		}
		if paneByID(s.projection, pane) != nil {
			s.applyPaneSelection(pane)
			return
		}
	}
	if tab != "" {
		if tab == s.selectedTabID {
			return
		}
		if tabByID(s.projection, tab) != nil {
			s.applyTabSelection(tab)
			return
		}
	}
	if project != "" && project != s.selectedProjectID {
		if projectExists(s.projection, project) {
			s.applyProjectSelection(project)
		}
	}
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
