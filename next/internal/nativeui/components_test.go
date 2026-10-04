package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestRowTintSelectionOutranksHover(t *testing.T) {
	for _, dark := range []bool{false, true} {
		tokens := designTokens(dark)

		tint, ok := rowTint(tokens, true, true)
		if !ok || tint != tokens.SidebarSelect {
			t.Fatalf("dark=%v selected+hover tint = %v ok=%v, want SidebarSelect", dark, tint, ok)
		}
		tint, ok = rowTint(tokens, false, true)
		if !ok || tint != tokens.SidebarHover {
			t.Fatalf("dark=%v hover-only tint = %v ok=%v, want SidebarHover", dark, tint, ok)
		}
		if tint, ok := rowTint(tokens, false, false); ok {
			t.Fatalf("dark=%v plain row tint = %v ok=%v, want untinted", dark, tint, ok)
		}
	}
}

// TestTreeRowStatePresentation renders the shared tree row through the
// headless tester in its three appearance states: selected, hovered-untinted
// surface, and status badge.
func TestTreeRowStatePresentation(t *testing.T) {
	view := func(c *ui.Context) {
		treeRow(c, treeRowSpec{Key: "row:selected", Selected: true, Mark: visualMark{svg: iconFolder}, Label: "Row Selected", Status: "working"})
		treeRow(c, treeRowSpec{Key: "row:plain", Mark: visualMark{svg: iconTab}, Label: "Row Plain"})
		treeRow(c, treeRowSpec{Key: "row:status", Mark: visualMark{svg: iconAgent}, Label: "Row Blocked", Status: "blocked"})
	}
	tester := ui.NewTester(view, 400, 200)
	for _, want := range []string{"Row Selected", "Row Plain", "Row Blocked"} {
		if !tester.HasText(want) {
			t.Fatalf("tree row states missing %q; texts=%q", want, tester.Texts())
		}
	}
	// The status badge is the dot alone (2026-10-06 A10): the state's
	// words moved into the tooltip, so no status text renders.
	for _, statusText := range []string{"Working", "Needs attention"} {
		if tester.HasText(statusText) {
			t.Fatalf("tree row status badge rendered text %q; texts=%q", statusText, tester.Texts())
		}
	}
}

// TestTreeRowCornerMarkPrecedence pins the single icon-corner dot (2026-10-06
// annotation round A4): an Agent Pane's own state wins, the subtree activity
// rollup comes next, the raw Status is the fallback, and a quiet row shows
// nothing. Two dots used to stack at the row's trailing edge.
func TestTreeRowCornerMarkPrecedence(t *testing.T) {
	dot, detail, ok := treeRowSpec{Dot: opWorking}.cornerMark()
	if !ok || dot != ToneWorking || detail != operationalLabel(opWorking) {
		t.Fatalf("agent dot = %v %q ok=%v, want ToneWorking + label", dot, detail, ok)
	}
	idle := treeRowSpec{Dot: opIdle}
	if dot, _, ok := idle.cornerMark(); !ok || dot != ToneMuted {
		t.Fatalf("idle agent dot = %v ok=%v, want muted gray", dot, ok)
	}
	dot, detail, ok = treeRowSpec{Activity: sidebarActivity{On: true, Tone: ToneSuccess, Detail: "node on :3000"}}.cornerMark()
	if !ok || dot != ToneSuccess || detail != "node on :3000" {
		t.Fatalf("activity dot = %v %q ok=%v, want green service", dot, detail, ok)
	}
	paneBoth := treeRowSpec{Dot: opWorking, Activity: sidebarActivity{On: true, Tone: ToneSuccess}}
	if dot, _, ok := paneBoth.cornerMark(); !ok || dot != ToneWorking {
		t.Fatalf("pane row with both = %v ok=%v, want the agent state to win", dot, ok)
	}
	ancestorBoth := treeRowSpec{Status: "working", Activity: sidebarActivity{On: true, Tone: ToneSuccess}}
	if dot, _, ok := ancestorBoth.cornerMark(); !ok || dot != ToneSuccess {
		t.Fatalf("ancestor row with both = %v ok=%v, want the rollup to win", dot, ok)
	}
	statusOnly := treeRowSpec{Status: "working"}
	if dot, _, ok := statusOnly.cornerMark(); !ok || dot != ToneWorking {
		t.Fatalf("status fallback = %v ok=%v, want ToneWorking", dot, ok)
	}
	quiet := treeRowSpec{Status: "idle"}
	if _, _, ok := quiet.cornerMark(); ok {
		t.Fatalf("quiet row rendered a corner dot")
	}
}
