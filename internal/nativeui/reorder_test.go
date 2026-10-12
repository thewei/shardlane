package nativeui

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

func TestDropInsertionIndex(t *testing.T) {
	ids := []string{"a", "b", "c", "d"}
	for _, tc := range []struct {
		from, to string
		before   bool
		index    int
		ok       bool
	}{
		{"a", "c", true, 1, true},
		{"a", "c", false, 2, true},
		{"d", "b", true, 1, true},
		{"d", "b", false, 2, true},
		{"a", "b", true, 0, false},
		{"b", "a", false, 1, false},
		{"a", "a", true, 0, false},
		{"missing", "c", true, 0, false},
	} {
		got, ok := dropInsertionIndex(ids, tc.from, tc.to, tc.before)
		if got != tc.index || ok != tc.ok {
			t.Errorf("drop %s on %s before=%v: (%d,%v), want (%d,%v)", tc.from, tc.to, tc.before, got, ok, tc.index, tc.ok)
		}
	}
}

func TestReorderOnlyWithinAuthoritativeParentAndSession(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.activeInstance = "local-1"
	s.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1"}, {ID: "w2"}},
		Tabs: []herdr.Tab{
			{ID: "t1", ProjectID: "w1"},
			{ID: "t2", ProjectID: "w1"},
			{ID: "t3", ProjectID: "w2"},
		},
		Panes: []herdr.Pane{
			{ID: "p1", TabID: "t1"},
			{ID: "p2", TabID: "t1"},
			{ID: "p3", TabID: "t2"},
		},
		Layouts: []herdr.Layout{{TabID: "t1", Panes: []herdr.LayoutPane{{PaneID: "p1"}, {PaneID: "p2"}}}},
	}
	tree := func(kind, parent, id string) treeNodeDrag { return treeNodeDrag{"local-1", kind, parent, id} }
	for _, tc := range []struct {
		from, to treeNodeDrag
		ok       bool
	}{
		{tree("project", "", "w1"), tree("project", "", "w2"), true},
		{tree("tab", "w1", "t1"), tree("tab", "w1", "t2"), true},
		{tree("tab", "w1", "t1"), tree("tab", "w2", "t3"), false},
		{tree("pane", "t1", "p1"), tree("pane", "t1", "p2"), true},
		{tree("pane", "t1", "p1"), tree("pane", "t2", "p3"), true},  // moving pane to another tab's pane
		{tree("pane", "t1", "p1"), tree("tab", "w1", "t2"), true},   // moving pane to another tab
		{tree("pane", "t1", "p1"), tree("tab", "w1", "t1"), false},  // pane already in t1
		{tree("pane", "t1", "p1"), tree("project", "", "w2"), true}, // moving pane to project as new tab
		{tree("pane", "t1", "p1"), tree("pane", "t1", "p1"), false}, // self drop
		{treeNodeDrag{"other", "project", "", "w1"}, tree("project", "", "w2"), false},
		{tree("tab", "w1", "t1"), tree("project", "", "w2"), false},
	} {
		if got := s.canReorderTree(tc.from, tc.to); got != tc.ok {
			t.Errorf("canReorderTree(%+v, %+v)=%v, want %v", tc.from, tc.to, got, tc.ok)
		}
	}
	if !s.canSwapCard(paneCardDrag{"local-1", "t1", "p1"}, "p2", "t1") {
		t.Fatal("same-tab canvas card swap rejected")
	}
	if !s.canDropOnPaneCard(treeNodeDrag{"local-1", "pane", "t1", "p1"}, "p3", "t2") {
		t.Fatal("cross-tab canvas card drop rejected")
	}
	s.projection.Layouts[0].Zoomed = true
	if s.canReorderTree(tree("pane", "t1", "p1"), tree("pane", "t1", "p2")) ||
		s.canSwapCard(paneCardDrag{"local-1", "t1", "p1"}, "p2", "t1") {
		t.Fatal("zoomed layout must not accept swaps")
	}
}

func TestResolvePaneCardDropAction(t *testing.T) {
	sameTab := treeNodeDrag{"local-1", "pane", "t1", "p1"}
	diffTab := treeNodeDrag{"local-1", "pane", "t2", "p9"}

	// Same tab always swaps positions regardless of coordinates
	act, label := resolvePaneCardDropAction(sameTab, "p2", "t1", 100, 100, 300, 200)
	if act != paneCardDropSwap || label != "Swap positions" {
		t.Fatalf("same tab action = %v, %q; want swap", act, label)
	}

	// Different tab: center moves to tab
	act, label = resolvePaneCardDropAction(diffTab, "p1", "t1", 100, 50, 300, 200)
	if act != paneCardDropMoveTab || label != "Move to tab" {
		t.Fatalf("diff tab center action = %v, %q; want move", act, label)
	}

	// Different tab: right edge splits right
	act, label = resolvePaneCardDropAction(diffTab, "p1", "t1", 250, 50, 300, 200)
	if act != paneCardDropSplitRight || label != "Split right" {
		t.Fatalf("diff tab right action = %v, %q; want split right", act, label)
	}

	// Different tab: bottom edge splits down
	act, label = resolvePaneCardDropAction(diffTab, "p1", "t1", 50, 180, 300, 200)
	if act != paneCardDropSplitDown || label != "Split down" {
		t.Fatalf("diff tab bottom action = %v, %q; want split down", act, label)
	}
}

func TestNativeTreeFollowsHerdrOrderAndLayout(t *testing.T) {
	p := herdr.Projection{
		Projects: []herdr.Project{{ID: "w2", Number: 2}, {ID: "w1", Number: 1}},
		Tabs: []herdr.Tab{
			{ID: "t2", ProjectID: "w1", Number: 2},
			{ID: "t1", ProjectID: "w1", Number: 1},
		},
		Panes: []herdr.Pane{
			{ID: "p3", TabID: "t1"}, {ID: "p1", TabID: "t1"}, {ID: "p2", TabID: "t1"},
		},
		Layouts: []herdr.Layout{{
			TabID: "t1", Panes: []herdr.LayoutPane{
				{PaneID: "p1", Rect: herdr.LayoutRect{X: 0, Y: 0}},
				{PaneID: "p2", Rect: herdr.LayoutRect{X: 50, Y: 0}},
				{PaneID: "p3", Rect: herdr.LayoutRect{X: 0, Y: 50}},
			},
		}},
	}
	projects := orderedProjects(p)
	if projects[0].ID != "w1" || projects[1].ID != "w2" {
		t.Fatalf("workspace ordering = %+v", projects)
	}
	tabs := tabsForProject(p, "w1")
	if tabs[0].ID != "t1" || tabs[1].ID != "t2" {
		t.Fatalf("tab ordering = %+v", tabs)
	}
	panes := panesForTab(p, "t1")
	sortPanesByLayout(p, "t1", panes)
	got := []string{panes[0].ID, panes[1].ID, panes[2].ID}
	if !slices.Equal(got, []string{"p1", "p2", "p3"}) {
		t.Fatalf("layout ordering = %v", got)
	}
}

// The same MyGo treeRow used by the sidebar must turn a native pointer drag
// into a drop, not a row click, and expose a live insertion preview.
func TestTreeRowNativeDragPreviewAndDrop(t *testing.T) {
	var moves []string
	var preview bool
	const height = 28
	view := func(c *ui.Context) {
		ui.Column(c).Width(240).Children(func() {
			for _, id := range []string{"Alpha", "Beta", "Gamma"} {
				id := id
				value := treeNodeDrag{"s", "project", "", id}
				r := treeRow(c, treeRowSpec{
					Key: id, Label: id,
					Drag: &treeRowDrag{
						Value:  value,
						Accept: func(from treeNodeDrag) bool { return from.ID != id && from.Kind == "project" },
						Drop: func(from treeNodeDrag, before bool) {
							moves = append(moves, from.ID+"->"+id)
							preview = before
						},
					},
				})
				_ = r
			}
		})
	}
	tt := ui.NewTester(view, 300, 200)
	tt.Frame()
	from, ok := tt.Find("Alpha")
	if !ok {
		t.Fatal("missing Alpha drag source")
	}
	to, ok := tt.Find("Gamma")
	if !ok {
		t.Fatal("missing Gamma drop target")
	}
	tt.Press(from.X+18, from.Y+float32(height)/2)
	tt.Move(from.X+30, from.Y+float32(height)/2+5)
	tt.Move(to.X+20, to.Y+4)
	tt.Release(to.X+20, to.Y+4)
	if !slices.Equal(moves, []string{"Alpha->Gamma"}) || !preview {
		t.Fatalf("drag result = %v before=%v", moves, preview)
	}
}
