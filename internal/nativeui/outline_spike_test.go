package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// DS-06 spike: evaluate official ui.Outline against the Shardlane
// Project→Tab→Pane tree requirements (client-local disclosure, selection,
// status presentation, context menus). The spike stays in the tree to
// record the parity evidence; the production decision is documented in the
// 0.4.0 audit.
type spikeItem struct {
	kind   string
	id     string
	label  string
	status string
}

func TestOutlineSpikeParity(t *testing.T) {
	state := &ui.OutlineState[spikeItem]{}
	roots := []spikeItem{
		{kind: "project", id: "wa", label: "Alpha"},
		{kind: "project", id: "wb", label: "Beta"},
	}
	children := func(item spikeItem) []spikeItem {
		switch item.kind {
		case "project":
			if item.id == "wa" {
				return []spikeItem{{kind: "tab", id: "ta", label: "alpha-main", status: "working"}}
			}
			return []spikeItem{{kind: "tab", id: "tb", label: "beta-main", status: "blocked"}}
		default:
			return nil
		}
	}
	view := func(c *ui.Context) {
		ui.Outline(c, state, roots, children, func(item spikeItem) {
			row := ui.ButtonBase(c).Label(item.label)
			row.Children(func() {
				ui.Text(c, item.label).Grow(1)
				if item.status != "" {
					st := normalizeRuntimeStatus(item.status)
					statusPill(c, operationalLabel(st), operationalTone(st))
				}
			})
			row.ContextMenu(func(m *ui.Menu) {
				m.Item("Close Project…")
			})
		})
	}
	tester := ui.NewTester(view, 500, 400)

	// Client-local disclosure: both projects open independently.
	state.Open.Add(roots[0])
	state.Open.Add(roots[1])
	tester.Frame()
	for _, want := range []string{"Alpha", "Beta", "alpha-main", "beta-main", "Working", "Needs attention"} {
		if !tester.HasText(want) {
			t.Fatalf("outline missing %q; texts=%q", want, tester.Texts())
		}
	}
	// Collapsing one project leaves the other open (presentation state).
	state.Open.Remove(roots[1])
	tester.Frame()
	if !tester.HasText("alpha-main") {
		t.Fatal("collapsing Beta collapsed Alpha")
	}
	if tester.HasText("beta-main") {
		t.Fatal("collapsing Beta left its Tab visible")
	}

	// Context menus attach to outline rows.
	if err := tester.RightClick("alpha-main"); err != nil {
		t.Fatal(err)
	}
	if menu := tester.Menu(); len(menu) == 0 {
		t.Fatal("outline row context menu did not open")
	}
	tester.CloseMenu()
}
