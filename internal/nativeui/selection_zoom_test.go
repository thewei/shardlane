package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

// zoomFollowFixture builds one Project/Tab with two Panes; when zoomed the
// Tab's layout maximizes focused (p1 unless said otherwise).
func zoomFollowFixture(zoomed bool, focused string) herdr.Projection {
	return herdr.Projection{
		Protocol:         22,
		FocusedProjectID: "prj", FocusedTabID: "tab", FocusedPaneID: focused,
		Projects: []herdr.Project{
			{ID: "prj", Label: "Project", ActiveTabID: "tab", TabCount: 2},
		},
		Tabs: []herdr.Tab{
			{ID: "tab", ProjectID: "prj", PaneCount: 2},
			{ID: "other", ProjectID: "prj", PaneCount: 1},
		},
		Panes: []herdr.Pane{
			{ID: "p1", TerminalID: "t1", ProjectID: "prj", TabID: "tab"},
			{ID: "p2", TerminalID: "t2", ProjectID: "prj", TabID: "tab"},
		},
		Layouts: []herdr.Layout{{
			ProjectID: "prj", TabID: "tab", Zoomed: zoomed, FocusedPaneID: focused,
			Area:  herdr.LayoutRect{Width: 100, Height: 40},
			Panes: []herdr.LayoutPane{{PaneID: "p1", Focused: true}, {PaneID: "p2"}},
		}},
	}
}

// TestNavigationZoomMutationTable pins the 2026-10-13 zoom-follow decision:
// a child jump into an already maximized Tab moves the maximization to the
// clicked Pane (mode "on"), the parent Tab selection steps the maximized
// Pane down (mode "off"), and navigation never zooms an unzoomed Tab.
func TestNavigationZoomMutationTable(t *testing.T) {
	zoomed := zoomFollowFixture(true, "p1")
	split := zoomFollowFixture(false, "p1")

	cases := []struct {
		name       string
		projection herdr.Projection
		tabID      string
		paneID     string
		wantTarget string
		wantMode   string
		wantOK     bool
	}{
		{"child jump moves maximization", zoomed, "tab", "p2", "p2", "on", true},
		{"jump onto maximized pane owes nothing", zoomed, "tab", "p1", "", "", false},
		{"parent selection steps maximized pane down", zoomed, "tab", "", "p1", "off", true},
		{"unzoomed tab never zooms (child)", split, "tab", "p2", "", "", false},
		{"unzoomed tab never zooms (parent)", split, "tab", "", "", "", false},
		{"missing layout owes nothing", zoomed, "other", "p2", "", "", false},
	}
	for _, tc := range cases {
		target, mode, ok := navigationZoomMutation(tc.projection, tc.tabID, tc.paneID)
		if ok != tc.wantOK || target != tc.wantTarget || mode != tc.wantMode {
			t.Fatalf("%s: got (%q, %q, %v), want (%q, %q, %v)",
				tc.name, target, mode, ok, tc.wantTarget, tc.wantMode, tc.wantOK)
		}
	}
}

// TestSelectPaneZoomFollowSelectionCoherence pins the bug this seam fixed:
// selecting a non-maximized Pane of a zoomed Tab moves the local selection
// (headless runs drop the zoom RPC), and once the runtime truth lands with
// the maximization moved to the target, the zoomed invariant keeps that
// selection instead of snapping it back to the old maximized Pane.
func TestSelectPaneZoomFollowSelectionCoherence(t *testing.T) {
	s := NewShell()
	s.router.Replace(routeHistory) // presentation-only; no terminal attach
	s.projection = zoomFollowFixture(true, "p1")

	s.selectPane("p2")
	if s.selectedTabID != "tab" || s.selectedPaneID != "p2" {
		t.Fatalf("jump selection = %q/%q, want tab/p2", s.selectedTabID, s.selectedPaneID)
	}
	// The click owed exactly one zoom mutation: maximization onto p2.
	if target, mode, ok := navigationZoomMutation(s.projection, "tab", "p2"); !ok || target != "p2" || mode != "on" {
		t.Fatalf("zoom follow = (%q, %q, %v), want (p2, on, true)", target, mode, ok)
	}

	// The post-mutation snapshot arrives maximized on p2: the selection
	// must survive reconciliation (the old snap-back to the stale focused
	// Pane is exactly what made the jump look dead).
	s.applyProjection(zoomFollowFixture(true, "p2"), false)
	if s.selectedPaneID != "p2" || s.selectedTabID != "tab" {
		t.Fatalf("selection bounced after zoom-follow snapshot: %q/%q", s.selectedTabID, s.selectedPaneID)
	}
}

// TestSelectTabParentSelectionShowsWholeSplit pins the parent rule: after
// the runtime truth arrives unzoomed, the Tab selection stays on the Tab and
// the formerly maximized Pane is simply the highlighted Pane of the split.
func TestSelectTabParentSelectionShowsWholeSplit(t *testing.T) {
	s := NewShell()
	s.router.Replace(routeHistory)
	s.projection = zoomFollowFixture(true, "p1")

	s.selectTab("tab")
	if s.selectedTabID != "tab" {
		t.Fatalf("parent selection lost the tab: %q", s.selectedTabID)
	}
	if target, mode, ok := navigationZoomMutation(s.projection, "tab", ""); !ok || target != "p1" || mode != "off" {
		t.Fatalf("parent zoom follow = (%q, %q, %v), want (p1, off, true)", target, mode, ok)
	}

	s.applyProjection(zoomFollowFixture(false, "p1"), false)
	if s.selectedTabID != "tab" || s.selectedPaneID != "p1" {
		t.Fatalf("post-unzoom selection = %q/%q, want tab/p1", s.selectedTabID, s.selectedPaneID)
	}
}
