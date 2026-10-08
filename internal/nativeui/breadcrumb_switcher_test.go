package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

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
