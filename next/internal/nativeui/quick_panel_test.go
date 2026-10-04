package nativeui

import (
	"testing"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/herdr"
)

func mygoRectangle(x, y, width, height int) mygo.Rectangle {
	return mygo.Rectangle{X: x, Y: y, Width: width, Height: height}
}

// TestQuickPanelBounds pins the §17.1 anchoring geometry: centered beneath
// the anchor rect, clamped into the work area, flipped above when the bottom
// would overflow, and sane on a degenerate work area.
func TestQuickPanelBounds(t *testing.T) {
	workArea := QuickRect{X: 0, Y: 25, Width: 1440, Height: 900}

	// Centered beneath the anchor.
	anchor := QuickRect{X: 700, Y: 0, Width: 22, Height: 24}
	x, y := QuickPanelBounds(anchor, workArea, QuickPanelWidth, QuickPanelHeight)
	if x != 700+22/2-QuickPanelWidth/2 || y != 24+quickPanelGap {
		t.Fatalf("centered anchor = %d,%d", x, y)
	}

	// Right-edge anchor: clamped into the work area.
	anchor = QuickRect{X: 1430, Y: 0, Width: 22, Height: 24}
	x, _ = QuickPanelBounds(anchor, workArea, QuickPanelWidth, QuickPanelHeight)
	if x != workArea.Width-QuickPanelWidth {
		t.Fatalf("right clamp = %d, want %d", x, workArea.Width-QuickPanelWidth)
	}

	// Left-edge anchor: clamped to the work-area origin.
	anchor = QuickRect{X: 0, Y: 0, Width: 22, Height: 24}
	x, _ = QuickPanelBounds(anchor, workArea, QuickPanelWidth, QuickPanelHeight)
	if x != 0 {
		t.Fatalf("left clamp = %d", x)
	}

	// Dock-adjacent anchor near the bottom: the panel flips above it.
	anchor = QuickRect{X: 700, Y: 880, Width: 22, Height: 24}
	_, y = QuickPanelBounds(anchor, workArea, QuickPanelWidth, QuickPanelHeight)
	if want := 880 - quickPanelGap - QuickPanelHeight; y != want {
		t.Fatalf("flip-above = %d, want %d", y, want)
	}

	// Degenerate work area: raw anchor, no crash.
	x, y = QuickPanelBounds(anchor, QuickRect{}, QuickPanelWidth, QuickPanelHeight)
	if x != 700+11-QuickPanelWidth/2 || y != 880+24+quickPanelGap {
		t.Fatalf("degenerate anchor = %d,%d", x, y)
	}
}

// TestQuickPanelHeightFor pins the 2026-10-07 content-driven height: the
// panel grows with its row count up to the fixed cap, sizes to the empty
// state when there are no rows, and never exceeds the cap (the list scrolls
// inside beyond it).
func TestQuickPanelHeightFor(t *testing.T) {
	empty := quickPanelChromeHeight + quickPanelEmptyHeight
	if got := QuickPanelHeightFor(0); got != empty {
		t.Fatalf("empty height = %d, want %d", got, empty)
	}
	one := quickPanelChromeHeight + quickPanelRowHeight
	if got := QuickPanelHeightFor(1); got != one {
		t.Fatalf("one-row height = %d, want %d", got, one)
	}
	three := quickPanelChromeHeight + 3*quickPanelRowHeight
	if got := QuickPanelHeightFor(3); got != three {
		t.Fatalf("three-row height = %d, want %d", got, three)
	}
	if got := QuickPanelHeightFor(50); got != QuickPanelHeight {
		t.Fatalf("capped height = %d, want %d", got, QuickPanelHeight)
	}
	if !(QuickPanelHeightFor(2) < QuickPanelHeightFor(4)) {
		t.Fatal("height must grow with the row count")
	}
}

// TestQuickPanelHeader pins the §17.2 header summary sharing the §17.4
// segment source with the tray: panel wording "review", the full Agent
// count appended, quiet reads Ready, disconnection wins.
func TestQuickPanelHeader(t *testing.T) {
	counts := TrayCounts{Attention: 2, Review: 1, Working: 3}
	snapshot := StatusCenterSnapshot{
		NeedsAttention: 2, ReviewPending: 1, Working: 3,
		Entries: make([]StatusCenterEntry, 6),
	}
	if got := QuickPanelHeader(snapshot, true); got != "2 need attention · 1 review · 3 working · 6 agents" {
		t.Fatalf("header = %q", got)
	}
	// The tray keeps its own wording from the same segments.
	if got := TrayToolTip(counts, true); got != "Shardlane — 2 need attention, 1 for review, 3 working" {
		t.Fatalf("tray tooltip = %q", got)
	}
	if got := QuickPanelHeader(StatusCenterSnapshot{Entries: make([]StatusCenterEntry, 1)}, true); got != "1 agent" {
		t.Fatalf("single-agent header = %q", got)
	}
	if got := QuickPanelHeader(StatusCenterSnapshot{}, true); got != "Ready" {
		t.Fatalf("quiet header = %q", got)
	}
	if got := QuickPanelHeader(snapshot, false); got != "Herdr disconnected" {
		t.Fatalf("disconnected header = %q", got)
	}
}

// quickPanelShell builds a shell whose projection carries a working, a
// blocked and an idle agent — the full list the panel's tab group filters.
func quickPanelShell(t *testing.T) *Shell {
	t.Helper()
	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.projection = herdr.Projection{
		Agents: []herdr.Agent{
			{TerminalID: "term-1", PaneID: "p1", Status: "working", Name: "Scout"},
			{TerminalID: "term-2", PaneID: "p2", Status: "blocked", Name: "Ranger"},
			{TerminalID: "term-3", PaneID: "p3", Status: "idle", Name: "Sleeper"},
		},
	}
	shell.reconcileWorkbench()
	return shell
}

// TestQuickPanelViewRendersAgentsWithStatusTabGroup pins the 2026-10-07
// panel contract: the shared snapshot rendered as the sidebar's compact
// agent cards under a status tab group — no complex per-row action buttons,
// no status words in the rows.
func TestQuickPanelViewRendersAgentsWithStatusTabGroup(t *testing.T) {
	shell := quickPanelShell(t)
	tester := ui.NewTester(shell.QuickPanelView, QuickPanelWidth, QuickPanelHeight)
	for _, want := range []string{
		"Shardlane",
		"1 needs attention · 1 working · 3 agents",
		"All", "Attention", "Review", "Working", "Idle",
		"Scout", "Ranger", "Sleeper",
	} {
		if !tester.HasText(want) {
			t.Fatalf("panel missing %q; texts=%q", want, tester.Texts())
		}
	}
	// The rows are the sidebar's simple cards: the state is the corner dot,
	// so the old per-row action buttons and status pills are gone.
	for _, gone := range []string{"Open Terminal", "Open Agent", "Open Chat", "Mark reviewed"} {
		if tester.HasText(gone) {
			t.Fatalf("panel row still shows %q; texts=%q", gone, tester.Texts())
		}
	}
	if tester.HasText("No agents here") {
		t.Fatalf("panel shows the empty state despite live agents; texts=%q", tester.Texts())
	}
}

// TestQuickPanelStatusTabFilters pins the tab group: Attention keeps only
// the blocked agent, Idle only the idle one, All restores every agent.
func TestQuickPanelStatusTabFilters(t *testing.T) {
	shell := quickPanelShell(t)
	tester := ui.NewTester(shell.QuickPanelView, QuickPanelWidth, QuickPanelHeight)

	if err := tester.Click("Attention"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("Ranger") {
		t.Fatalf("attention filter hid the blocked agent; texts=%q", tester.Texts())
	}
	if tester.HasText("Scout") || tester.HasText("Sleeper") {
		t.Fatalf("attention filter kept other agents; texts=%q", tester.Texts())
	}

	if err := tester.Click("Idle"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("Sleeper") || tester.HasText("Ranger") {
		t.Fatalf("idle filter wrong; texts=%q", tester.Texts())
	}

	if err := tester.Click("All"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	for _, want := range []string{"Scout", "Ranger", "Sleeper"} {
		if !tester.HasText(want) {
			t.Fatalf("all filter missing %q; texts=%q", want, tester.Texts())
		}
	}
}

// TestQuickPanelCardClickNavigates pins the row action: clicking an agent
// card hides the panel and lands on the agent's owning pane, client-locally
// (no Herdr focus RPC).
func TestQuickPanelCardClickNavigates(t *testing.T) {
	shell := quickPanelShell(t)
	shell.router.Replace("/history")
	tester := ui.NewTester(shell.QuickPanelView, QuickPanelWidth, QuickPanelHeight)

	if err := tester.Click("Ranger"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.selectedPaneID != "p2" {
		t.Fatalf("selected pane = %q, want p2", shell.selectedPaneID)
	}
	if shell.router.Path() != routeWorkspace {
		t.Fatalf("route = %q, want the workspace", shell.router.Path())
	}
}

// TestQuickPanelViewEmptyWithoutAgents pins the empty state: only a panel
// with no agents at all shows it.
func TestQuickPanelViewEmptyWithoutAgents(t *testing.T) {
	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.reconcileWorkbench()
	tester := ui.NewTester(shell.QuickPanelView, QuickPanelWidth, QuickPanelHeight)
	if !tester.HasText("No agents here") {
		t.Fatalf("empty panel missing the empty state; texts=%q", tester.Texts())
	}
}

// TestToggleQuickPanelWithoutPanel pins the §17 adapter rule: without an
// attached panel window the tray click (and the titlebar anchor click) is a
// safe no-op — the deleted in-window Status Center is no longer a fallback.
func TestToggleQuickPanelWithoutPanel(t *testing.T) {
	shell := quickPanelShell(t)
	shell.router.Replace("/history")
	shell.ToggleQuickPanel(mygoRectangle(700, 0, 22, 24))
	if shell.router.Path() != "/history" {
		t.Fatalf("fallback route = %q, want no navigation", shell.router.Path())
	}
	shell.toggleAgentActivityPanel(nil)

	// With a panel attached, hideQuickPanel is nil-safe pre-attach and the
	// Escape path in the view never panics.
	bare := NewShell()
	bare.hideQuickPanel()
	escapeTester := ui.NewTester(bare.QuickPanelView, QuickPanelWidth, QuickPanelHeight)
	escapeTester.Frame()
}

// TestQuickPanelToggleOff pins the §17.1 anchor toggle against the
// focus-stealing click: a click lands after the panel already lost key
// status (blur-hide), and must still close rather than re-open.
func TestQuickPanelToggleOff(t *testing.T) {
	now := time.Now()
	if !quickPanelToggleOff(true, time.Time{}, now) {
		t.Fatal("visible panel must toggle off")
	}
	if quickPanelToggleOff(false, time.Time{}, now) {
		t.Fatal("hidden panel with no recent blur must toggle on")
	}
	if !quickPanelToggleOff(false, now.Add(-quickPanelToggleGrace/2), now) {
		t.Fatal("a click inside the blur grace must toggle off")
	}
	if quickPanelToggleOff(false, now.Add(-2*quickPanelToggleGrace), now) {
		t.Fatal("a click after the blur grace must toggle on")
	}
}

// TestFilterStatusEntries keeps a direct pin on the tab-group projection:
// snapshot order survives, the bucket filter narrows it.
func TestFilterStatusEntries(t *testing.T) {
	shell := quickPanelShell(t)
	snapshot := shell.BuildStatusCenterSnapshot()
	if got := len(shell.filterStatusEntries(snapshot)); got != 3 {
		t.Fatalf("all entries = %d, want 3", got)
	}
	shell.workbench.setFilterChoice(1) // Attention
	entries := shell.filterStatusEntries(snapshot)
	if len(entries) != 1 || entries[0].Card.Title != "Ranger" {
		t.Fatalf("attention entries = %+v", entries)
	}
	shell.workbench.setFilterChoice(4) // Idle
	entries = shell.filterStatusEntries(snapshot)
	if len(entries) != 1 || entries[0].Card.Title != "Sleeper" {
		t.Fatalf("idle entries = %+v", entries)
	}
}

// TestQuickPanelCloseYieldsToQuit pins the ⌘Q fix (2026-10-07 user
// report): the panel downgrades ordinary close requests to hide, but a
// close that arrives while the application is quitting goes through —
// a prevented close during quit aborts the whole application quit in
// mygo's prepareQuit and strands a windowless process on the
// single-instance lock.
func TestQuickPanelCloseYieldsToQuit(t *testing.T) {
	shell := quickPanelShell(t)

	ordinary := &mygo.CloseEvent{}
	shell.quickPanelCloseRequest(ordinary)
	if !ordinary.DefaultPrevented() {
		t.Fatal("an ordinary close must be downgraded to hide")
	}

	shell.MarkQuitting()
	quitting := &mygo.CloseEvent{}
	shell.quickPanelCloseRequest(quitting)
	if quitting.DefaultPrevented() {
		t.Fatal("the close must go through while the application is quitting")
	}
}
