package nativeui

/**
 * [INPUT]: 依赖 mygo/ui 的 Menu/Element、internal/herdr 的 pane 变更 RPC（SplitPane/TogglePaneZoom）、dialogs 的确认通道
 * [OUTPUT]: 对外提供 headerOverflow（⋯ 菜单）、splitPaneAction/zoomPaneAction/closePaneAction（pane 动作唯一实现）、paneOverflowItems/surfaceOverflowItems（菜单构建器）
 * [POS]: nativeui 的 pane 动作单一来源：卡片头按钮、右键菜单、侧栏行、标题栏溢出全部走这里；单 Pane 的 zoom 语义（两侧栏最大化，F143）在此收口
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"log/slog"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// headerOverflow is the ⋯ button after the title-bar breadcrumb path: one
// menu holding the current surface's actions (2026-10-05 header review).
// Pane items come from paneOverflowItems — the same builder behind the
// tree rows' hover "..." and the pane right-click menu.
func (s *Shell) headerOverflow(c *ui.Context) {
	k := gorexColorsOf(c.Theme().Dark)
	var trigger ui.Element
	ui.Box(c).Children(func() {
		b := gorexIconButton(c, k, iconEllipsis, "More actions", "More actions", false, 28, 14)
		trigger = b
	})
	trigger.Menu(func(m *ui.Menu) {
		s.surfaceOverflowItems(m)
	})
}

// splitPaneAction splits one Pane; the card header's buttons and the pane
// menu share it. The Info lines are the click-path evidence for device
// acceptance (2026-10-06 annotation A2/F143): if a header button dies on
// device, the log shows whether the shared action ran at all.
func (s *Shell) splitPaneAction(paneID, direction string) {
	slog.Info("pane action", "action", "split", "pane_id", paneID, "direction", direction)
	s.mutateSelecting(func() (herdr.Projection, error) {
		return s.runtime.SplitPane(s.activeInstance, paneID, direction)
	})
}

// zoomPaneAction toggles one Pane's zoom; the card header and the pane
// menu share it. On a split Tab that zooms the Pane through Herdr. On a
// single-Pane Tab Herdr answers reason=single_pane and nothing moves
// (probed live 2026-10-06), so the button maximizes the workspace instead:
// both side rails collapse so the pane fills the window, and a second
// press brings them back (2026-10-06 annotation A2/F143).
func (s *Shell) zoomPaneAction(paneID string) {
	if s.tabPaneCount(paneID) <= 1 {
		slog.Info("pane action", "action", "zoom-rails", "pane_id", paneID)
		if !s.sidebarCollapsed || s.rightPanel.open {
			s.sidebarCollapsed = true
			if s.rightPanel.open {
				s.toggleRightPanel()
			}
		} else {
			s.sidebarCollapsed = false
			s.toggleRightPanel()
		}
		return
	}
	slog.Info("pane action", "action", "zoom", "pane_id", paneID)
	s.mutate(func() (herdr.Projection, error) {
		return s.runtime.TogglePaneZoom(s.activeInstance, paneID)
	})
}

// tabPaneCount resolves how many Panes Herdr lays out beside paneID's own:
// the breadth of its Tab's layout, or one when no layout reports otherwise
// (the unit-rect fallback in visiblePaneGeometry).
func (s *Shell) tabPaneCount(paneID string) int {
	pane := paneByID(s.projection, paneID)
	if pane == nil {
		return 1
	}
	if layout, ok := layoutForTab(s.projection, pane.TabID); ok && len(layout.Panes) > 0 {
		return len(layout.Panes)
	}
	return 1
}

// closePaneAction routes one Pane's close through the confirm dialog; the
// card header and the pane menu share it.
func (s *Shell) closePaneAction(paneID string) {
	slog.Info("pane action", "action", "close-confirm", "pane_id", paneID)
	s.openConfirm("close-pane", paneID, "Close Pane?", "This closes the Herdr Pane and its running terminal process.")
}

// paneOverflowItems is the ONE pane menu builder (2026-10-05 round two):
// the attached pane's right-click menu, the project tree row's hover
// "..." and the title-bar overflow's pane section all render these items.
func (s *Shell) paneOverflowItems(m *ui.Menu, paneID, label string) {
	if s.isPanePinned(paneID) {
		if m.Item("Unpin").Chosen() {
			s.togglePanePin(paneID)
		}
	} else if m.Item("Pin Pane").Chosen() {
		s.togglePanePin(paneID)
	}
	if m.Item("Split Right").Chosen() {
		s.splitPaneAction(paneID, "right")
	}
	if m.Item("Split Down").Chosen() {
		s.splitPaneAction(paneID, "down")
	}
	zoomLabel := "Zoom / Unzoom"
	if layout, ok := s.selectedLayout(); ok && paneID == s.selectedPaneID && layout.Zoomed {
		zoomLabel = "Unzoom"
	}
	if m.Item(zoomLabel).Chosen() {
		s.zoomPaneAction(paneID)
	}
	if m.Item("Rename Pane…").Chosen() {
		s.openTextDialog("rename-pane", paneID, "Rename Pane", "Name", label)
	}
	m.Separator()
	if m.Item("Close Pane…").Chosen() {
		s.closePaneAction(paneID)
	}
}

// surfaceOverflowItems fills the title-bar "..." with the current
// surface's actions (plan §8.2..8.4).
func (s *Shell) surfaceOverflowItems(m *ui.Menu) {
	switch s.surface.current() {
	case WorkspaceSurfaceDiff:
		s.diffOverflowItems(m)
		return
	case WorkspaceSurfaceCommit:
		s.commitOverflowItems(m)
		return
	case WorkspaceSurfaceChat:
		s.chatOverflowItems(m)
		return
	}
	if s.selectedPaneID != "" {
		label := "Pane"
		if pane := s.selectedPane(); pane != nil && pane.Label != "" {
			label = pane.Label
		}
		s.paneOverflowItems(m, s.selectedPaneID, label)
		m.Separator()
	}
	if s.selectedProjectID != "" {
		if m.Item("New Tab").Chosen() {
			cwd := ""
			if project := s.selectedProject(); project != nil {
				cwd = project.CWD
			}
			s.mutateSelecting(func() (herdr.Projection, error) {
				return s.runtime.CreateTab(s.activeInstance, s.selectedProjectID, cwd)
			})
		}
	}
	if m.Item("Refresh Runtime").Chosen() {
		s.reloadInstances(false)
	}
}

// diffOverflowItems mirrors the diff actions of the removed content
// header (plan §8.3).
func (s *Shell) diffOverflowItems(m *ui.Menu) {
	if m.Item("Refresh").Chosen() {
		s.refreshGitChanges()
	}
	findLabel := "Find"
	if s.git != nil && s.git.gdFinding {
		findLabel = "Hide Find"
	}
	if m.Item(findLabel).Chosen() {
		if s.git != nil {
			s.git.gdFinding = !s.git.gdFinding
			if !s.git.gdFinding {
				s.git.gdMatchesFor = "\x00"
				s.git.gdRowsDirty = true
			}
		}
	}
	layoutLabel := "Split Layout"
	if s.git != nil && s.git.splitLayout {
		layoutLabel = "Unified Layout"
	}
	if m.Item(layoutLabel).Chosen() {
		s.toggleDiffLayout()
	}
	if m.Item("Open Editor").Chosen() {
		if cf, ok := s.selectedChangeFile(); ok {
			s.openFileInEditor(cf.Path)
		}
	}
	m.Separator()
	if m.Item("Commit…").Chosen() {
		s.showSurface(WorkspaceSurfaceCommit)
	}
	if m.Item("Back to Terminal").Chosen() {
		s.showSurface(WorkspaceSurfaceTerminal)
	}
}

// commitOverflowItems mirrors the commit actions (plan §8.4).
func (s *Shell) commitOverflowItems(m *ui.Menu) {
	if m.Item("Back to Changes").Chosen() {
		s.cancelCommitSurface()
	}
	if s.surface.commit.InFlight {
		m.Item("Committing…")
	}
}

// chatOverflowItems mirrors the Chat surface actions.
func (s *Shell) chatOverflowItems(m *ui.Menu) {
	if m.Item("Back to Terminal").Chosen() {
		s.showSurface(WorkspaceSurfaceTerminal)
	}
	if s.chatQueuedText != "" {
		if m.Item("Cancel Follow-Up").Chosen() {
			s.cancelChatFollowUp()
		}
	}
	m.Separator()
	if s.selectedPaneID != "" {
		label := "Pane"
		if pane := s.selectedPane(); pane != nil && pane.Label != "" {
			label = pane.Label
		}
		s.paneOverflowItems(m, s.selectedPaneID, label)
	}
}
