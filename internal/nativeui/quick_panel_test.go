package nativeui

import (
	"testing"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

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
		"Agents & Services",
		"1 needs attention · 1 working · 3 agents",
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
	if tester.HasText("No active agents") {
		t.Fatalf("panel shows the empty state despite live agents; texts=%q", tester.Texts())
	}
}


// TestQuickPanelCardClickOpensAgentDetail pins the two-level popover:
// selecting an Agent in the overview reveals its exact details without
// navigating away; explicit actions in the detail handle navigation.
func TestQuickPanelCardClickOpensAgentDetail(t *testing.T) {
	shell := quickPanelShell(t)
	shell.router.Replace("/history")
	tester := ui.NewTester(shell.QuickPanelView, QuickPanelWidth, QuickPanelHeight)

	if err := tester.Click("Ranger"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.quickPanelAgent == nil || shell.quickPanelAgent.TerminalID != "term-2" {
		t.Fatalf("Agent detail = %#v, want Ranger", shell.quickPanelAgent)
	}
	if shell.router.Path() != routeHistory || shell.selectedPaneID != "" {
		t.Fatalf("Agent preview unexpectedly navigated: route=%q pane=%q", shell.router.Path(), shell.selectedPaneID)
	}
	if !tester.HasText("Open Terminal") {
		t.Fatalf("focused action missing: %q", tester.Texts())
	}
}

// TestQuickPanelViewEmptyWithoutAgents pins the empty state: only a panel
// with no agents at all shows it.
func TestQuickPanelViewEmptyWithoutAgents(t *testing.T) {
	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.reconcileWorkbench()
	tester := ui.NewTester(shell.QuickPanelView, QuickPanelWidth, QuickPanelHeight)
	if !tester.HasText("No active agents") {
		t.Fatalf("empty panel missing the empty state; texts=%q", tester.Texts())
	}
}

// TestQuickPanelToggleWithoutPanel pins the §17.1 adapter rule: without a
// bound tray (and without a main window) the tray click (and the titlebar
// anchor click) is a safe no-op — the deleted in-window Status Center is no
// longer a fallback.
func TestQuickPanelToggleWithoutPanel(t *testing.T) {
	shell := quickPanelShell(t)
	shell.router.Replace("/history")
	shell.ToggleQuickPanel()
	if shell.router.Path() != "/history" {
		t.Fatalf("fallback route = %q, want no navigation", shell.router.Path())
	}
	shell.toggleAgentActivityPanel(ui.Element{})

	// hideQuickPanel is nil-safe without an open panel.
	bare := NewShell()
	bare.hideQuickPanel()
}

// TestQuickPanelToggleCloses pins the §17.1 anchor toggle against the
// pass-through click: on macOS the outside press that dismissed the flyout
// goes on to the element under it, so a click landing within the dismiss
// grace must still close (stay closed) rather than re-open.
func TestQuickPanelToggleCloses(t *testing.T) {
	now := time.Now()
	if !quickPanelToggleCloses(&mygo.Window{}, time.Time{}, now) {
		t.Fatal("open panel must toggle off")
	}
	if quickPanelToggleCloses(nil, time.Time{}, now) {
		t.Fatal("no open panel with no recent dismissal must toggle on")
	}
	if !quickPanelToggleCloses(nil, now.Add(-quickPanelDismissGrace/2), now) {
		t.Fatal("a click inside the dismiss grace must toggle off")
	}
	if quickPanelToggleCloses(nil, now.Add(-2*quickPanelDismissGrace), now) {
		t.Fatal("a click after the dismiss grace must toggle on")
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
