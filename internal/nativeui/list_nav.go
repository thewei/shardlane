package nativeui

/**
 * [INPUT]: 侧栏列表/树这一帧画出的可见行（key、层级、折叠态）与 MyGo 的真焦点、Element.Shortcut、ScrollIntoView、FocusGroup
 * [OUTPUT]: listNav —— 侧栏列表与树唯一的键盘模型：真焦点就是光标；j/k·↓/↑ 移动，h/l·←/→ 折叠展开或进出层级，o 打开，gg/G·Home/End 首尾，Ctrl-d/u 跳十行；进页时光标停在开场行但不亮，键盘用过它才亮（Tab 进入除外，MyGo 自带 FocusVisible）
 * [POS]: Files 树、Git 引用树、Hosts 列表共用（行经 sidebarTreeRow/sidebarSectionHeader 注册）；折叠、选择与动作仍归各树——nav 只移动焦点，并把 o/l/h 翻译成该行的"点击"（Enter/Space 本来就是点击）
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
)

const (
	// navStep is how far Ctrl-D / Ctrl-U move: about half a sidebar.
	navStep = 10
	// navChord is how long the first g of gg waits for the second.
	navChord = 700 * time.Millisecond
)

type navAction uint8

const (
	navDown navAction = iota + 1
	navUp
	navLeft
	navRight
	navOpen
	navChordG
	navTop
	navBottom
	navHalfDown
	navHalfUp
)

// navBindings are the keys a focused row answers. Arrows, Home and End are
// claimed too, so MyGo's own focus group (which wraps round) leaves them to
// this model; PageUp/PageDown stay with the scroll container.
var navBindings = []struct {
	mods ui.Modifiers
	key  ui.Key
	act  navAction
}{
	{0, ui.KeyJ, navDown}, {0, ui.KeyDown, navDown},
	{0, ui.KeyK, navUp}, {0, ui.KeyUp, navUp},
	{0, ui.KeyH, navLeft}, {0, ui.KeyLeft, navLeft},
	{0, ui.KeyL, navRight}, {0, ui.KeyRight, navRight},
	{0, ui.KeyO, navOpen},
	{0, ui.KeyG, navChordG},
	{ui.Shift, ui.KeyG, navBottom}, {0, ui.KeyEnd, navBottom},
	{0, ui.KeyHome, navTop},
	{ui.Ctrl, ui.KeyD, navHalfDown}, {ui.Ctrl, ui.KeyU, navHalfUp},
}

// navRow is one visible row of a list or tree as the keyboard sees it.
type navRow struct {
	Key string
	// Level is the row's depth under its section (or the top of the list).
	Level int
	// Section marks a group header: every row until the next header sits
	// under it.
	Section bool
	// Folder rows have children; Expanded is whether they show.
	Folder, Expanded bool
	// Pick is the row a list opens on: the open file, the selected host.
	Pick bool
	// parent is the enclosing folder or header, filled by the list.
	parent string
}

// listNav is the keyboard model of one sidebar list or tree. The focused row
// is the cursor — real MyGo focus, so Tab, clicks and these keys all agree —
// and a row answers the keys only while it holds the focus, which keeps a
// filter field above the tree typing normally. The list owns no fold or
// selection state: moving only focuses a row, and o / l / h on a row whose
// state asks for it come out as that row's click, which the tree already
// handles for the mouse. A cursor parked on a list the mouse just opened
// stays invisible — no row looks selected that is not — until a key of the
// list has moved it.
type listNav struct {
	// OnMove runs when the keyboard moves the cursor onto a row, for lists
	// whose selection follows it (Hosts).
	OnMove func(key string)

	prev, cur   []navRow // last pass's rows, the rows being built
	stack       []string // last row key at each level of the branch being built
	header      string   // key of the section header the next rows sit under
	want, apply string   // row to focus on the next / this pass
	cursor      string   // the row holding the focus in this pass, "" for none
	opener      string   // the cursor as the open dialog came up
	seen        uint64   // last pass that built the list
	entry       bool     // focus the opening row once the list is built
	shown       bool     // the keyboard has used the cursor: it wears its face
	chordAt     time.Time
}

// begin opens one build of the list. pass is the Shell's per-View counter: a
// list that was not built in the previous pass has just mounted, and
// autoFocus puts the cursor on its opening row.
func (n *listNav) begin(pass uint64, autoFocus bool) {
	if n.seen == 0 || n.seen+1 < pass {
		n.shown = false
		n.entry = n.entry || autoFocus
	}
	n.seen = pass
	n.prev, n.cur = n.cur, n.prev[:0]
	n.barrier()
	n.apply, n.want, n.cursor = n.want, "", ""
}

// barrier ends the section the following rows would otherwise sit under: a
// plain label between two groups of rows.
func (n *listNav) barrier() { n.header, n.stack = "", n.stack[:0] }

// finish closes the build: a fresh mount asks for the opening row to be
// focused in the next pass — once it has rows, a tree still reading its
// directory has none yet — and a wanted row that never appeared is dropped.
func (n *listNav) finish(c *ui.Context) {
	if n.entry {
		if key := n.openingRow(); key != "" {
			n.entry = false
			n.want = key
			c.AnimationFrame()
		}
	}
	if n.cursor == "" && n.want == "" && n.opener == "" {
		n.shown = false // the keyboard has left the list
	}
	n.apply = ""
}

// openingRow is the row a freshly mounted list puts the cursor on: its
// picked row, else the first row that is not a bare header.
func (n *listNav) openingRow() string {
	first := ""
	for _, r := range n.cur {
		if r.Pick {
			return r.Key
		}
		if first == "" && !r.Section {
			first = r.Key
		}
	}
	if first == "" && len(n.cur) > 0 {
		first = n.cur[0].Key
	}
	return first
}

// row registers one drawn row and reports whether the keyboard activated it
// this pass (the caller treats that as a click). Nil lists register nothing.
func (n *listNav) row(c *ui.Context, e ui.Element, r navRow) bool {
	if n == nil {
		return false
	}
	switch {
	case r.Section:
		n.header, n.stack = r.Key, n.stack[:0]
	case r.Level == 0 || len(n.stack) == 0:
		r.parent = n.header
		n.stack = append(n.stack[:0], r.Key)
	default:
		level := min(r.Level, len(n.stack))
		r.parent = n.stack[level-1]
		n.stack = append(n.stack[:level], r.Key)
	}
	n.cur = append(n.cur, r)
	if n.apply == r.Key {
		n.apply = ""
		e.Focus().ScrollIntoView()
	}
	if !e.Focused() {
		return false
	}
	n.cursor = r.Key
	return n.keys(c, e, r)
}

// showsCursor reports whether e, a row of this list, is the cursor and
// should look it: the keyboard has moved it, or MyGo says the focus arrived
// by Tab. Row keys that change things wait for it, so nothing acts on a
// cursor the user cannot see.
func (n *listNav) showsCursor(e ui.Element) bool {
	return e.Focused() && (n != nil && n.shown || e.FocusVisible())
}

// keys answers the focused row's keys. Every binding is asked on every pass,
// so each stays claimed for the next key press; the first one pressed wins.
func (n *listNav) keys(c *ui.Context, e ui.Element, r navRow) (activate bool) {
	var act navAction
	for _, b := range navBindings {
		if e.Shortcut(b.mods, b.key) && act == 0 {
			act = b.act
		}
	}
	if act != 0 {
		n.shown = true
	}
	if act != 0 && act != navChordG {
		n.chordAt = time.Time{}
	}
	switch act {
	case navDown:
		n.move(c, r.Key, 1)
	case navUp:
		n.move(c, r.Key, -1)
	case navHalfDown:
		n.move(c, r.Key, navStep)
	case navHalfUp:
		n.move(c, r.Key, -navStep)
	case navTop:
		n.edge(c, r.Key, false)
	case navBottom:
		n.edge(c, r.Key, true)
	case navChordG:
		if now := time.Now(); !n.chordAt.IsZero() && now.Sub(n.chordAt) < navChord {
			n.chordAt = time.Time{}
			n.edge(c, r.Key, false)
		} else {
			n.chordAt = now
		}
	case navLeft:
		if r.Folder && r.Expanded {
			return true // fold it
		}
		if r.parent != "" {
			n.to(c, r.parent)
		}
	case navRight:
		switch {
		case !r.Folder, !r.Expanded:
			return true // open the leaf, or unfold the folder
		default:
			if i := n.index(r.Key); i >= 0 && i+1 < len(n.prev) && n.prev[i+1].parent == r.Key {
				n.to(c, n.prev[i+1].Key)
			}
		}
	case navOpen:
		return true
	}
	return false
}

// index is the row's place in the last pass, -1 when it was not there.
func (n *listNav) index(key string) int {
	return slices.IndexFunc(n.prev, func(r navRow) bool { return r.Key == key })
}

// move steps the cursor delta rows, stopping at either end — a list is not a
// ring.
func (n *listNav) move(c *ui.Context, from string, delta int) {
	i := n.index(from)
	if i < 0 {
		return
	}
	if j := min(max(i+delta, 0), len(n.prev)-1); j != i {
		n.to(c, n.prev[j].Key)
	}
}

// edge moves the cursor to the first or last row.
func (n *listNav) edge(c *ui.Context, from string, last bool) {
	if len(n.prev) == 0 {
		return
	}
	j := 0
	if last {
		j = len(n.prev) - 1
	}
	if n.prev[j].Key != from {
		n.to(c, n.prev[j].Key)
	}
}

// to asks for the cursor on key in the next pass and, for lists whose
// selection follows the cursor, selects it.
func (n *listNav) to(c *ui.Context, key string) {
	n.want = key
	if n.OnMove != nil {
		n.OnMove(key)
	}
	c.AnimationFrame()
}

// listNavs are the Shell's keyboard-driven sidebar lists.
type listNavs struct {
	files, gitRefs, hosts listNav
}

// dialog is the Shell's report, on each change, of whether a dialog is up. A
// dialog's autofocused field takes the keyboard from the row that opened it,
// and MyGo hands focus back only to an element that is still there — so a
// list notes its cursor as the dialog opens and returns to it as it closes,
// and `a`, `r`, `d` do not end the keyboard's visit to the tree.
func (ns *listNavs) dialog(open bool, pass uint64) {
	for _, n := range []*listNav{&ns.files, &ns.gitRefs, &ns.hosts} {
		switch {
		case open && n.seen+1 >= pass:
			n.opener = n.cursor
		case !open && n.opener != "":
			n.want, n.opener = n.opener, ""
		}
	}
}

// navTrackDialogs reports dialog changes to the lists.
func (s *Shell) navTrackDialogs() {
	if open := s.dialogOpen || s.confirmOpen || s.mergeDialogOpen; open != s.navDialog {
		s.navDialog = open
		s.nav.dialog(open, s.navPass())
	}
}

// navPass is the per-View pass counter (history's detailFrame counts with it
// too): a list built in the previous pass is still the same mount.
func (s *Shell) navPass() uint64 { return s.hist.viewFrame }

// navList builds a list's rows inside box: one Tab stop for the whole list
// (MyGo's focus group; the keys inside are the model's own), and the rows
// register with nav as they draw.
func (s *Shell) navList(c *ui.Context, nav *listNav, autoFocus bool, box ui.Element, rows func()) {
	box.FocusGroup(ui.Vertical).Children(func() {
		nav.begin(s.navPass(), autoFocus)
		rows()
		nav.finish(c)
	})
}
