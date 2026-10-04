package nativeui

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

// splitHeaderTestShell seeds one Tab with two side-by-side Panes (p1 left
// focused, p2 right unfocused) and attaches both surfaces, so the gorex
// pane cards and their hover action buttons build headlessly. Terminal
// conns are scripted pipes: no real `herdr terminal attach` children.
func splitHeaderTestShell(t *testing.T) (*Shell, *ui.Tester) {
	t.Helper()
	shell := NewShell()
	shell.activeInstance = "inst"
	shell.projection = herdr.Projection{
		FocusedProjectID: "prj", FocusedTabID: "tab", FocusedPaneID: "p1",
		Projects: []herdr.Project{{ID: "prj", Label: "herdr-client"}},
		Tabs:     []herdr.Tab{{ID: "tab", ProjectID: "prj", Label: "Coding"}},
		Panes: []herdr.Pane{
			{ID: "p1", TerminalID: "t1", ProjectID: "prj", TabID: "tab", Label: "p1"},
			{ID: "p2", TerminalID: "t2", ProjectID: "prj", TabID: "tab", Label: "p2"},
		},
		Layouts: []herdr.Layout{{
			ProjectID: "prj", TabID: "tab",
			Area: herdr.LayoutRect{Width: 120, Height: 40},
			Panes: []herdr.LayoutPane{
				{PaneID: "p1", Focused: true, Rect: herdr.LayoutRect{Width: 60, Height: 40}},
				{PaneID: "p2", Rect: herdr.LayoutRect{X: 60, Width: 60, Height: 40}},
			},
		}},
	}
	shell.selectedProjectID = "prj"
	shell.selectedTabID = "tab"
	shell.selectedPaneID = "p1"
	terms := map[string]*terminal.Terminal{}
	for _, key := range []string{"t1", "t2"} {
		conn := newScriptPipe("\x1b[2J\x1b[1;1H" + key)
		term, err := terminal.New(terminal.Options{Conn: conn})
		if err != nil {
			t.Skip("terminal library unavailable:", err)
		}
		terms[key] = term
		t.Cleanup(func() { _ = term.Close() })
	}
	shell.terminals["t1"] = &terminalSurface{key: "t1", paneID: "p1", term: terms["t1"], label: "p1"}
	shell.terminals["t2"] = &terminalSurface{key: "t2", paneID: "p2", term: terms["t2"], label: "p2"}
	tt := ui.NewTester(func(c *ui.Context) { shell.terminalCanvas(c) }, 1200, 800)
	tt.Frame()
	return shell, tt
}

// TestPaneHeaderButtonsRespondOnUnfocusedPane pins the 2026-10-06 annotation
// A2: in a split, hovering a non-active Pane's card header reveals its
// action buttons and clicking one must act on that Pane — the click path
// runs selectPane synchronously even though the Herdr mutation itself is
// dropped headlessly (win == nil), so the selection is the observable.
func TestPaneHeaderButtonsRespondOnUnfocusedPane(t *testing.T) {
	shell, tt := splitHeaderTestShell(t)

	// p2's card occupies x 600..1196; its header's zoom button sits third
	// from the right (Close 26, gap 1, Zoom 26) inside the 8pt right pad.
	// The headless frame loop is real-time driven: under full-suite load a
	// click can straddle a frame boundary, so push until it lands (the
	// clickChoice convention from sidebar_split_test.go).
	for attempt := 0; attempt < 5 && shell.selectedPaneID != "p2"; attempt++ {
		tt.Move(900, 24) // hover the p2 header so the buttons build
		tt.Frame()
		tt.Move(1144, 24) // rest on the zoom button
		tt.Frame()
		tt.ClickAt(1144, 24)
		tt.Frame()
	}
	if shell.selectedPaneID != "p2" {
		t.Fatalf("clicking the unfocused pane's header button did not select it: selected=%q", shell.selectedPaneID)
	}
}

// TestPaneHeaderButtonsRespondWithoutHoverFrame pins the plain click path:
// even with no prior hover frame, a click on the header (buttons not built
// yet) must still select the pane via the header row handler.
func TestPaneHeaderButtonsRespondWithoutHoverFrame(t *testing.T) {
	shell, tt := splitHeaderTestShell(t)
	for attempt := 0; attempt < 5 && shell.selectedPaneID != "p2"; attempt++ {
		tt.ClickAt(1144, 24)
		tt.Frame()
	}
	if shell.selectedPaneID != "p2" {
		t.Fatalf("plain header click did not select the pane: selected=%q", shell.selectedPaneID)
	}
}

// TestTitlebarHeightTracksTrafficLightBand pins the 2026-10-06 annotation
// A1/F142: the workspace bar stops growing past the window controls' band
// (MyGo centers the lights inside exactly bar.Height), so content stays on
// the lights' horizontal line instead of sagging below it with a dead band
// underneath. The 42 floor still fits the two breadcrumb rows.
func TestTitlebarHeightTracksTrafficLightBand(t *testing.T) {
	if got := titlebarHeight(46, true); got != 46 {
		t.Fatalf("workspace bar must stay at bar.Height=46, got %v", got)
	}
	if got := titlebarHeight(46, false); got != 46 {
		t.Fatalf("plain bar must stay at bar.Height=46, got %v", got)
	}
	if got := titlebarHeight(38, true); got != 42 {
		t.Fatalf("workspace bar keeps its 42 two-row floor, got %v", got)
	}
	if got := titlebarHeight(28, false); got != 34 {
		t.Fatalf("plain bar keeps its 34 one-row floor, got %v", got)
	}
}

// TestZoomActionSinglePaneMaximizesWorkspace pins the 2026-10-06 annotation
// A2/F143: with one Pane in the Tab, Herdr's zoom is a no-op (live probe:
// reason=single_pane), so the pane header's zoom button maximizes the
// workspace instead — both side rails collapse, and a second press brings
// them back. A split Tab keeps the Herdr zoom path and never touches the
// rails here.
func TestZoomActionSinglePaneMaximizesWorkspace(t *testing.T) {
	shell := NewShell()
	shell.activeInstance = "inst"
	shell.sidebarCollapsed = false
	shell.rightPanel.open = false
	shell.projection = herdr.Projection{
		FocusedProjectID: "prj", FocusedTabID: "tab", FocusedPaneID: "p1",
		Projects: []herdr.Project{{ID: "prj"}},
		Tabs:     []herdr.Tab{{ID: "tab", ProjectID: "prj"}},
		Panes:    []herdr.Pane{{ID: "p1", TerminalID: "t1", ProjectID: "prj", TabID: "tab"}},
	}
	shell.selectedPaneID = "p1"

	shell.zoomPaneAction("p1")
	if !shell.sidebarCollapsed || shell.rightPanel.open {
		t.Fatalf("single-pane zoom must hide both rails: sidebar=%v panel=%v", shell.sidebarCollapsed, shell.rightPanel.open)
	}
	shell.zoomPaneAction("p1")
	if shell.sidebarCollapsed || !shell.rightPanel.open {
		t.Fatalf("second single-pane zoom must restore both rails: sidebar=%v panel=%v", shell.sidebarCollapsed, shell.rightPanel.open)
	}

	// Split Tab: the Herdr mutation path runs (dropped headlessly at
	// win == nil) and the rails stay as they are.
	shell.rightPanel.open = false
	shell.projection.Layouts = []herdr.Layout{{
		ProjectID: "prj", TabID: "tab",
		Area: herdr.LayoutRect{Width: 100, Height: 40},
		Panes: []herdr.LayoutPane{
			{PaneID: "p1", Rect: herdr.LayoutRect{Width: 50, Height: 40}},
			{PaneID: "p2", Rect: herdr.LayoutRect{X: 50, Width: 50, Height: 40}},
		},
	}}
	shell.zoomPaneAction("p1")
	if shell.sidebarCollapsed || shell.rightPanel.open {
		t.Fatalf("split-tab zoom must go through Herdr, not the rails: sidebar=%v panel=%v", shell.sidebarCollapsed, shell.rightPanel.open)
	}
}

// TestPaneHeaderMoreButtonOpensPaneMenu pins the 2026-10-06 F148 request:
// a "Pane menu" ⋯ button sits right against the pane title, and its
// dropdown is the one paneOverflowItems menu (the actions the terminal
// surface's right click used to carry before real-terminal semantics made
// body right clicks program events). The button is always rendered — not
// hover-gated — so the menu anchor cannot vanish under the dropdown.
func TestPaneHeaderMoreButtonOpensPaneMenu(t *testing.T) {
	shell, tt := splitHeaderTestShell(t)

	if err := tt.Click("Pane menu"); err != nil {
		t.Fatalf("pane title ⋯ button missing: %v", err)
	}
	tt.Frame()
	menu := tt.Menu()
	joined := strings.Join(menu, ",")
	for _, want := range []string{"Split Right", "Split Down", "Zoom / Unzoom", "Rename Pane…", "Close Pane…"} {
		if !strings.Contains(joined, want) {
			t.Fatalf("pane ⋯ menu missing %q: %v", want, menu)
		}
	}
	tt.CloseMenu()
	if shell.selectedPaneID != "p1" {
		t.Fatalf("opening the menu must not steal the selection: %q", shell.selectedPaneID)
	}
}
