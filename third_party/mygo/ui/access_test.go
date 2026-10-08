package ui

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/internal/platform"
)

// node returns the node of the tree with role and a label containing
// label.
func node(t *testing.T, tree *platform.AccessTree, role platform.AccessRole, label string) platform.AccessNode {
	t.Helper()
	for _, n := range tree.Nodes {
		if n.Role == role && strings.Contains(n.Label, label) {
			return n
		}
	}
	var have []string
	for _, n := range tree.Nodes {
		have = append(have, n.Label)
	}
	t.Fatalf("no node of role %d labeled %q among %q", role, label, have)
	return platform.AccessNode{}
}

func TestAccessibilityTree(t *testing.T) {
	d := &demo{name: "Ada", volume: 30}
	tt := NewTester(d.view, 640, 600)
	if tt.h.access != nil {
		t.Fatal("a tree before assistive technology asked")
	}
	tt.send(platform.SurfaceEvent{Kind: platform.AccessibilityOn})
	tree := tt.h.access
	if tree == nil || len(tree.Nodes) == 0 {
		t.Fatal("no tree after AccessibilityOn")
	}
	inc := node(t, tree, platform.RoleButton, "Increment")
	if inc.Actions&platform.ActionPress == 0 || inc.Bounds.W <= 0 {
		t.Errorf("Increment: %+v", inc)
	}
	node(t, tree, platform.RoleText, "MyGo UI")
	agree := node(t, tree, platform.RoleCheckBox, "I agree")
	if agree.States&platform.AccessChecked != 0 {
		t.Error("the check box is checked")
	}
	node(t, tree, platform.RoleSwitch, "")
	node(t, tree, platform.RoleRadio, "Alpha")
	slider := node(t, tree, platform.RoleSlider, "")
	if slider.Min != 0 || slider.Max != 100 || slider.Now != 30 {
		t.Errorf("slider range %v..%v at %v", slider.Min, slider.Max, slider.Now)
	}
	field := node(t, tree, platform.RoleTextField, "")
	if field.Value != "Ada" || field.Actions&platform.ActionSetValue == 0 {
		t.Errorf("text field: %+v", field)
	}
	node(t, tree, platform.RolePopUpButton, "")
	list := node(t, tree, platform.RoleList, "")
	row := node(t, tree, platform.RoleText, "Row 3")
	for p := row.Parent; ; p = tree.Nodes[p].Parent {
		if p < 0 {
			t.Fatal("the row is not inside the list")
		}
		if tree.Nodes[p].ID == list.ID {
			break
		}
	}
	// Buttons name themselves after their text, which is not a node.
	for _, n := range tree.Nodes {
		if n.Role == platform.RoleText && n.Label == "Increment" {
			t.Error("the button's text is a node of its own")
		}
	}

	// Actions act as the pointer and the keyboard would.
	tt.send(platform.SurfaceEvent{Kind: platform.AccessAction, ID: inc.ID, Action: platform.AccessPress})
	if d.count != 1 {
		t.Errorf("pressing Increment: count %d", d.count)
	}
	tt.send(platform.SurfaceEvent{Kind: platform.AccessAction, ID: agree.ID, Action: platform.AccessPress})
	if !d.agree {
		t.Error("pressing the check box did not check it")
	}
	if n := node(t, tt.h.access, platform.RoleCheckBox, "I agree"); n.States&platform.AccessChecked == 0 {
		t.Error("the tree after the frame shows the check box unchecked")
	}
	tt.send(platform.SurfaceEvent{Kind: platform.AccessAction, ID: slider.ID, Action: platform.AccessIncrement})
	if d.volume <= 30 {
		t.Errorf("incrementing the slider: %v", d.volume)
	}
	if tt.h.access.Focus != slider.ID {
		t.Error("the slider has no focus after incrementing it")
	}
	tt.send(platform.SurfaceEvent{Kind: platform.AccessAction, ID: field.ID, Action: platform.AccessSetValue, Text: "Grace"})
	if d.name != "Grace" {
		t.Errorf("setting the text field: %q", d.name)
	}
	if f := node(t, tt.h.access, platform.RoleTextField, ""); f.Value != "Grace" || f.SelStart != 5 || f.SelEnd != 5 || tt.h.access.Focus != f.ID {
		t.Errorf("text field after setting it: %+v, focus %d", f, tt.h.access.Focus)
	}
}

// byID returns the node of the tree with id.
func byID(tree *platform.AccessTree, id uint64) (platform.AccessNode, bool) {
	for _, n := range tree.Nodes {
		if n.ID == id {
			return n, true
		}
	}
	return platform.AccessNode{}, false
}

func TestAccessibilityOfLists(t *testing.T) {
	sel := -1
	s := ListState{Selected: &sel}
	tt := NewTester(func(c *Context) {
		Button(c, "Before")
		List(c, &s, 1000, func(i int) { Textf(c, "Row %d", i).Height(30) }).Grow(1)
	}, 300, 400)
	tt.send(platform.SurfaceEvent{Kind: platform.AccessibilityOn})
	tree := tt.h.access
	// Each row says which of all it is, and that it can be chosen.
	item := node(t, tree, platform.RoleListItem, "Row 3")
	if item.PosInSet != 4 || item.SetSize != 1000 {
		t.Errorf("row 3 is %d of %d", item.PosInSet, item.SetSize)
	}
	if item.States&(platform.AccessSelectable|platform.AccessFocusable) != platform.AccessSelectable|platform.AccessFocusable ||
		item.Actions&(platform.ActionPress|platform.ActionFocus|platform.ActionScrollIntoView) == 0 {
		t.Errorf("row 3: %+v", item)
	}
	node(t, tree, platform.RoleText, "Row 3") // its content shows too
	if b := node(t, tree, platform.RoleButton, "Before"); b.Actions&platform.ActionScrollIntoView != 0 {
		t.Error("a button out of any scroll container scrolls into view")
	}
	// Focusing a row chooses it, the list taking the focus: assistive
	// technology follows the choice as the arrows move it.
	tt.send(platform.SurfaceEvent{Kind: platform.AccessAction, ID: item.ID, Action: platform.AccessFocus})
	tree = tt.h.access
	if sel != 3 || tree.Focus != item.ID || tt.rt.focused != s.frame.e.id {
		t.Fatalf("focused row 3: chose %d, focus on %d (row %d)", sel, tree.Focus, item.ID)
	}
	if n, _ := byID(tree, item.ID); n.States&platform.AccessChecked == 0 {
		t.Error("the chosen row is not chosen")
	}
	tt.Key(0, KeyDown)
	tree = tt.h.access
	if n, ok := byID(tree, tree.Focus); !ok || n.PosInSet != 5 || n.States&platform.AccessChecked == 0 {
		t.Errorf("Down: the focus is on %+v", n)
	}
	// The last row built, out of view, scrolls into view when assistive
	// technology asks, and the rows after it come.
	last := platform.AccessNode{}
	for _, n := range tree.Nodes {
		if n.Role == platform.RoleListItem && n.PosInSet > last.PosInSet {
			last = n
		}
	}
	if last.Bounds.Y+last.Bounds.H <= 400 {
		t.Fatalf("the last row built shows: %+v", last)
	}
	tt.send(platform.SurfaceEvent{Kind: platform.AccessAction, ID: last.ID, Action: platform.AccessScrollIntoView})
	tree = tt.h.access
	if n, ok := byID(tree, last.ID); !ok || n.Bounds.Y+n.Bounds.H > 400.01 {
		t.Errorf("scrolled into view: %+v (%v)", n, ok)
	}
	beyond := false
	for _, n := range tree.Nodes {
		beyond = beyond || n.Role == platform.RoleListItem && n.PosInSet > last.PosInSet
	}
	if !beyond {
		t.Error("no row after it came")
	}
}

func TestAccessibilityScrollsToARowNoLongerBuilt(t *testing.T) {
	// A frame that scrolled to a row built more rows around it than the
	// next builds: assistive technology may ask for one of those.
	var s ListState
	tt := NewTester(func(c *Context) {
		List(c, &s, 1000, func(i int) { Textf(c, "Row %d", i).Height(30) }).Grow(1)
	}, 300, 400)
	tt.send(platform.SurfaceEvent{Kind: platform.AccessibilityOn})
	s.ScrollTo(20, Center)
	tt.Frame()
	tree := tt.h.access
	last := platform.AccessNode{}
	for _, n := range tree.Nodes {
		if n.Role == platform.RoleListItem && n.PosInSet > last.PosInSet {
			last = n
		}
	}
	// The next frame builds the rows of the place alone.
	tt.send(platform.SurfaceEvent{Kind: platform.AccessAction, ID: last.ID, Action: platform.AccessScrollIntoView})
	if r, ok := rowBox(tt, &s, last.PosInSet-1); !ok || r.Y < 0 || r.Y+r.H > 400.01 {
		t.Errorf("row %d after asking to see it: %v (%v)", last.PosInSet-1, r, ok)
	}
}

func TestAccessibilityOfTables(t *testing.T) {
	sel := 2
	s := ListState{Selected: &sel}
	cols := []TableColumn{{Title: "Name"}, {Title: "Size", Width: 80}}
	tt := NewTester(func(c *Context) {
		Table(c, &s, cols, 500, func(row, col int) { Textf(c, "Cell %d %d", row, col) }).Grow(1)
	}, 400, 300)
	tt.send(platform.SurfaceEvent{Kind: platform.AccessibilityOn})
	tree := tt.h.access
	rows := 0
	for _, n := range tree.Nodes {
		if n.Role != platform.RoleRow {
			continue
		}
		if rows++; rows == 1 {
			if n.PosInSet != 0 {
				t.Errorf("the header is row %d", n.PosInSet)
			}
			continue
		}
		if n.SetSize != 500 || n.States&platform.AccessSelectable == 0 {
			t.Errorf("a row: %+v", n)
		}
		// A row holds its cells, not another row.
		for _, m := range tree.Nodes {
			if m.Parent >= 0 && tree.Nodes[m.Parent].ID == n.ID && m.Role != platform.RoleCell {
				t.Errorf("row %d holds a node of role %d", n.PosInSet, m.Role)
			}
		}
	}
	if rows < 5 {
		t.Errorf("%d rows", rows)
	}
	// The table has the focus for its rows.
	tt.Click("Cell 4 0")
	tree = tt.h.access
	if n, ok := byID(tree, tree.Focus); !ok || n.Role != platform.RoleRow || n.PosInSet != 5 {
		t.Errorf("clicked row 4: the focus is on %+v", n)
	}
}

func TestAccessibilityOfOverlays(t *testing.T) {
	open := true
	tt := NewTester(func(c *Context) {
		Button(c, "Behind")
		Modal(c, &open, func() {
			Text(c, "Sure?")
			Button(c, "OK")
		})
	}, 400, 300)
	tt.send(platform.SurfaceEvent{Kind: platform.AccessibilityOn})
	tree := tt.h.access
	dialog := node(t, tree, platform.RoleDialog, "")
	ok := node(t, tree, platform.RoleButton, "OK")
	if tree.Nodes[ok.Parent].ID != dialog.ID {
		t.Error("the dialog's button is not inside it")
	}
}

func TestRole(t *testing.T) {
	on := false
	tt := NewTester(func(c *Context) {
		b := Box(c).Size(20, 20).Focusable().Role(RoleSwitch).Label("Wi-Fi")
		if b.Clicked() {
			on = !on
		}
		Box(c).Size(20, 20).Label("hidden").Role(RoleNone)
	}, 100, 100)
	tt.send(platform.SurfaceEvent{Kind: platform.AccessibilityOn})
	sw := node(t, tt.h.access, platform.RoleSwitch, "Wi-Fi")
	for _, n := range tt.h.access.Nodes {
		if n.Label == "hidden" {
			t.Error("an element of RoleNone is in the tree")
		}
	}
	tt.send(platform.SurfaceEvent{Kind: platform.AccessAction, ID: sw.ID, Action: platform.AccessPress})
	if !on {
		t.Error("pressing the custom switch did not click it")
	}
}

// TestInputMethodContext checks that input methods see the text around
// the caret and replace what they typed, as macOS's press and hold does.
func TestInputMethodContext(t *testing.T) {
	d := &demo{name: "caf"}
	tt := NewTester(d.view, 640, 600)
	r, _ := tt.Find("I agree")
	tt.ClickAt(r.X+20, r.Y+40+r.H/2)
	tt.Key(0, KeyEnd)
	ime := tt.h.ime
	if !ime.Active || ime.Text != "caf" || ime.Start != 3 || ime.End != 3 {
		t.Fatalf("text input state %+v", ime)
	}
	// Typing e, then holding it: the input method composes over the e
	// it typed, then commits the accented letter in its place.
	tt.Type("e")
	if tt.h.ime.Text != "cafe" {
		t.Fatalf("after typing: %+v", tt.h.ime)
	}
	tt.send(platform.SurfaceEvent{Kind: platform.TextComposition, Text: "e", Caret: 1, Replace: true, From: 3, To: 4})
	if d.name != "caf" {
		t.Errorf("composing over the e: %q", d.name)
	}
	tt.send(platform.SurfaceEvent{Kind: platform.TextInput, Text: "é"})
	if d.name != "café" {
		t.Errorf("committing: %q", d.name)
	}
	// Replacements count from the start of the text the input method got,
	// which ends imeContext runes before the selection in long texts.
	d.name = strings.Repeat("x", 2*imeContext) + "ab"
	tt.Frame()
	tt.Key(0, KeyEnd)
	ime = tt.h.ime
	if len([]rune(ime.Text)) != imeContext || ime.Start != imeContext || !strings.HasSuffix(ime.Text, "xab") {
		t.Fatalf("long text: %d runes, selection %d", len([]rune(ime.Text)), ime.Start)
	}
	tt.send(platform.SurfaceEvent{Kind: platform.TextInput, Text: "B", Replace: true, From: imeContext - 1, To: imeContext})
	if !strings.HasSuffix(d.name, "xaB") {
		t.Errorf("replacing in a long text: ...%q", d.name[len(d.name)-5:])
	}
}
