package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

/**
 * [INPUT]: 依赖 nativeui 的 Shell/breadcrumbSegments/breadcrumbSegmentsWithTheme, herdr
 * [OUTPUT]: 对外提供 TestBreadcrumbPaneSegmentListsTabPanes, TestBreadcrumbSegmentsVisualMarks
 * [POS]: nativeui 标题栏面包屑与切换器合约测试
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// TestBreadcrumbPaneSegmentListsTabPanes pins the pane switcher contract:
// the title-bar Pane segment carries the Tab id so its popover can list the
// Tab's Panes; carrying the Pane's own id filtered the list to nothing
// (2026-10-05 regression).
func TestBreadcrumbPaneSegmentListsTabPanes(t *testing.T) {
	s := NewShell()
	s.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "Shardlane", TabCount: 1}},
		Tabs:     []herdr.Tab{{ID: "t1", ProjectID: "w1", Label: "main", PaneCount: 2}},
		Panes: []herdr.Pane{
			{ID: "p1", TerminalID: "term1", ProjectID: "w1", TabID: "t1", Label: "left"},
			{ID: "p2", TerminalID: "term2", ProjectID: "w1", TabID: "t1", Label: "right"},
		},
	}
	s.selectedProjectID = "w1"
	s.selectedTabID = "t1"
	s.selectedPaneID = "p1"

	segments := s.breadcrumbSegments()
	if len(segments) != 3 {
		t.Fatalf("segments = %#v, want project/tab/pane", segments)
	}
	pane := segments[len(segments)-1]
	if pane.kind != "pane" {
		t.Fatalf("last segment kind = %q, want pane", pane.kind)
	}
	siblings := panesForTab(s.projection, pane.id)
	if len(siblings) != 2 {
		t.Fatalf("pane popover rows = %d, want 2 (segment id %q)", len(siblings), pane.id)
	}
}

func TestBreadcrumbSegmentsVisualMarks(t *testing.T) {
	s := NewShell()
	s.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "MyProject", TabCount: 1}},
		Tabs:     []herdr.Tab{{ID: "t1", ProjectID: "w1", Label: "Tab", PaneCount: 1}},
		Panes: []herdr.Pane{
			{ID: "p1", TerminalID: "term1", ProjectID: "w1", TabID: "t1", Label: "Terminal"},
		},
		Agents: []herdr.Agent{
			{PaneID: "p1", Kind: "claude-code", Name: "Claude Code", Status: "working"},
		},
	}
	s.selectedProjectID = "w1"
	s.selectedTabID = "t1"
	s.selectedPaneID = "p1"

	segments := s.breadcrumbSegmentsWithTheme(false)
	if len(segments) != 2 { // single pane tab only yields project and tab segments
		t.Fatalf("len(segments) = %d, want 2", len(segments))
	}

	tabSeg := segments[1]
	if tabSeg.label != "Claude Code" {
		t.Fatalf("tabSeg.label = %q, want 'Claude Code'", tabSeg.label)
	}
	if tabSeg.mark.bmp == nil && tabSeg.mark.svg == nil {
		t.Fatalf("tabSeg.mark should carry agent mark")
	}
}
