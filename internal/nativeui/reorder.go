package nativeui

/**
 * [INPUT]: 依赖 Herdr 权威排序、pane.swap、pane.move 协议以及 MyGo 原生 Drag/Drop 事件
 * [OUTPUT]: 提供 Workspace/Tab/Panes 安全拖放校验、目标索引计算、跨 Tab/Project 移动与卡片布局放置
 * [POS]: nativeui 拖拽语义桥接层；仅在 Drop 提交 Herdr mutation，预览不修改 runtime
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"sort"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

// treeNodeDrag is scoped to one Herdr instance and hierarchy node.
// It is the canonical drag payload shared across sidebar tree rows and terminal cards.
type treeNodeDrag struct {
	Instance string
	Kind     string // project, tab, pane
	ParentID string
	ID       string
}

type treeRowDrag struct {
	Value  treeNodeDrag
	Accept func(treeNodeDrag) bool
	Drop   func(treeNodeDrag, bool)
}

// treeDragFor binds a row's drop target to the current authoritative
// projection. Invalid/self drags have no preview or mutation.
func (s *Shell) treeDragFor(target treeNodeDrag) *treeRowDrag {
	return &treeRowDrag{
		Value: target,
		Accept: func(source treeNodeDrag) bool {
			return s.canReorderTree(source, target)
		},
		Drop: func(source treeNodeDrag, before bool) {
			s.reorderTree(source, target, before)
		},
	}
}

func (s *Shell) canReorderTree(from, to treeNodeDrag) bool {
	if s.loading || s.activeInstance == "" || from.Instance != s.activeInstance ||
		to.Instance != s.activeInstance || from.ID == to.ID {
		return false
	}
	switch from.Kind {
	case "project":
		return to.Kind == "project" &&
			containsProject(s.projection, from.ID) &&
			containsProject(s.projection, to.ID)
	case "tab":
		return to.Kind == "tab" &&
			from.ParentID == to.ParentID &&
			tabInProject(s.projection, from.ID, from.ParentID) &&
			tabInProject(s.projection, to.ID, to.ParentID)
	case "pane":
		if !paneBelongsToTab(s.projection, from.ID, from.ParentID) || tabZoomed(s.projection, from.ParentID) {
			return false
		}
		switch to.Kind {
		case "pane":
			return paneBelongsToTab(s.projection, to.ID, to.ParentID) && !tabZoomed(s.projection, to.ParentID)
		case "tab":
			return from.ParentID != to.ID && tabByID(s.projection, to.ID) != nil && !tabZoomed(s.projection, to.ID)
		case "project":
			return containsProject(s.projection, to.ID)
		}
	}
	return false
}

func containsProject(p herdr.Projection, id string) bool {
	for _, project := range p.Projects {
		if project.ID == id {
			return true
		}
	}
	return false
}

func tabInProject(p herdr.Projection, id, projectID string) bool {
	for _, tab := range p.Tabs {
		if tab.ID == id && tab.ProjectID == projectID {
			return true
		}
	}
	return false
}

func tabZoomed(p herdr.Projection, tabID string) bool {
	for _, layout := range p.Layouts {
		if layout.TabID == tabID {
			return layout.Zoomed
		}
	}
	return false
}

// dropInsertionIndex gives the final zero-based index AFTER removing the
// dragged item. No-op drops do not roundtrip to Herdr.
func dropInsertionIndex(ids []string, source, target string, before bool) (int, bool) {
	from, to := -1, -1
	for i, id := range ids {
		if id == source {
			from = i
		}
		if id == target {
			to = i
		}
	}
	if from < 0 || to < 0 || from == to {
		return 0, false
	}
	at := to
	if !before {
		at++
	}
	if from < at {
		at--
	}
	return at, at != from
}

func (s *Shell) reorderTree(from, to treeNodeDrag, before bool) {
	if !s.canReorderTree(from, to) {
		return
	}
	instance := s.activeInstance
	switch from.Kind {
	case "project":
		projects := orderedProjects(s.projection)
		ids := make([]string, 0, len(projects))
		for _, p := range projects {
			ids = append(ids, p.ID)
		}
		if index, ok := dropInsertionIndex(ids, from.ID, to.ID, before); ok {
			s.mutate(func() (herdr.Projection, error) {
				return s.runtime.MoveProject(instance, from.ID, uint(index))
			})
		}
	case "tab":
		tabs := tabsForProject(s.projection, from.ParentID)
		ids := make([]string, 0, len(tabs))
		for _, tab := range tabs {
			ids = append(ids, tab.ID)
		}
		if index, ok := dropInsertionIndex(ids, from.ID, to.ID, before); ok {
			s.mutate(func() (herdr.Projection, error) {
				return s.runtime.MoveTab(instance, from.ID, uint(index))
			})
		}
	case "pane":
		switch to.Kind {
		case "pane":
			if from.ParentID == to.ParentID {
				s.swapPaneDrop(from.ID, to.ID, from.ParentID)
			} else {
				split := "down"
				if before {
					split = "right"
				}
				s.movePaneDrop(from.ID, herdr.PaneMoveDestination{
					Type:         "tab",
					TabID:        to.ParentID,
					Split:        split,
					TargetPaneID: to.ID,
				})
			}
		case "tab":
			s.movePaneDrop(from.ID, herdr.PaneMoveDestination{
				Type:  "tab",
				TabID: to.ID,
				Split: "right",
			})
		case "project":
			s.movePaneDrop(from.ID, herdr.PaneMoveDestination{
				Type:        "new_tab",
				WorkspaceID: to.ID,
			})
		}
	}
}

type paneCardDrag struct {
	Instance string
	TabID    string
	PaneID   string
}

func (s *Shell) canDragPaneCard(paneID string) bool {
	return !s.loading && s.activeInstance != "" && s.selectedTabID != "" &&
		!tabZoomed(s.projection, s.selectedTabID) &&
		paneBelongsToTab(s.projection, paneID, s.selectedTabID)
}

func (s *Shell) canDropOnPaneCard(from treeNodeDrag, toPaneID, tabID string) bool {
	return !s.loading && s.activeInstance != "" &&
		from.Instance == s.activeInstance &&
		from.Kind == "pane" &&
		from.ID != toPaneID &&
		!tabZoomed(s.projection, tabID) &&
		paneBelongsToTab(s.projection, from.ID, from.ParentID) &&
		!tabZoomed(s.projection, from.ParentID) &&
		paneBelongsToTab(s.projection, toPaneID, tabID)
}

func (s *Shell) canSwapCard(from paneCardDrag, toPaneID, tabID string) bool {
	return s.canDropOnPaneCard(treeNodeDrag{
		Instance: from.Instance,
		Kind:     "pane",
		ParentID: from.TabID,
		ID:       from.PaneID,
	}, toPaneID, tabID)
}

type paneCardDropAction int

const (
	paneCardDropNone paneCardDropAction = iota
	paneCardDropSwap
	paneCardDropSplitRight
	paneCardDropSplitDown
	paneCardDropMoveTab
)

func resolvePaneCardDropAction(from treeNodeDrag, toPaneID, toTabID string, px, py, cardW, cardH float32) (paneCardDropAction, string) {
	if from.ParentID == toTabID {
		return paneCardDropSwap, "Swap positions"
	}
	if cardW > 0 && px > cardW*0.68 {
		return paneCardDropSplitRight, "Split right"
	}
	if cardH > 0 && py > cardH*0.68 {
		return paneCardDropSplitDown, "Split down"
	}
	return paneCardDropMoveTab, "Move to tab"
}

func (s *Shell) handlePaneCardDrop(from treeNodeDrag, toPaneID, toTabID string, action paneCardDropAction) {
	switch action {
	case paneCardDropSwap:
		s.swapPaneDrop(from.ID, toPaneID, toTabID)
	case paneCardDropSplitRight:
		s.movePaneDrop(from.ID, herdr.PaneMoveDestination{
			Type:         "tab",
			TabID:        toTabID,
			Split:        "right",
			TargetPaneID: toPaneID,
		})
	case paneCardDropSplitDown:
		s.movePaneDrop(from.ID, herdr.PaneMoveDestination{
			Type:         "tab",
			TabID:        toTabID,
			Split:        "down",
			TargetPaneID: toPaneID,
		})
	case paneCardDropMoveTab:
		s.movePaneDrop(from.ID, herdr.PaneMoveDestination{
			Type:         "tab",
			TabID:        toTabID,
			Split:        "right",
			TargetPaneID: toPaneID,
		})
	}
}

func (s *Shell) swapPaneDrop(from, to, tabID string) {
	if !s.canDropOnPaneCard(treeNodeDrag{
		Instance: s.activeInstance,
		Kind:     "pane",
		ParentID: tabID,
		ID:       from,
	}, to, tabID) {
		return
	}
	instance := s.activeInstance
	s.mutate(func() (herdr.Projection, error) {
		return s.runtime.SwapPanes(instance, from, to)
	})
}

func (s *Shell) movePaneDrop(paneID string, dest herdr.PaneMoveDestination) {
	instance := s.activeInstance
	s.mutate(func() (herdr.Projection, error) {
		return s.runtime.MovePane(instance, paneID, dest, true)
	})
	s.selectPane(paneID)
}

func orderedProjects(p herdr.Projection) []herdr.Project {
	result := append([]herdr.Project(nil), p.Projects...)
	sort.SliceStable(result, func(i, j int) bool {
		return result[i].Number < result[j].Number
	})
	return result
}

// sortPanesByLayout puts the Sidebar and breadcrumb in the spatial order of
// Herdr's actual split view; a pane swap stays visible after reconciliation.
// Geometry is read only. Missing layouts retain the snapshot's order.
func sortPanesByLayout(p herdr.Projection, tabID string, panes []herdr.Pane) {
	var layout *herdr.Layout
	for i := range p.Layouts {
		if p.Layouts[i].TabID == tabID {
			layout = &p.Layouts[i]
			break
		}
	}
	if layout == nil {
		return
	}
	rects := make(map[string]herdr.LayoutRect, len(layout.Panes))
	for _, pane := range layout.Panes {
		rects[pane.PaneID] = pane.Rect
	}
	sort.SliceStable(panes, func(i, j int) bool {
		a, okA := rects[panes[i].ID]
		b, okB := rects[panes[j].ID]
		if !okA || !okB {
			return okA && !okB
		}
		if a.Y != b.Y {
			return a.Y < b.Y
		}
		return a.X < b.X
	})
}
