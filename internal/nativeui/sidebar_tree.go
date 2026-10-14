package nativeui

/**
 * [INPUT]: row level, MyGo Context/Painter, TreeLine token 与 Shell 的 density 偏好
 * [OUTPUT]: 侧栏层级共用的对齐网格（treeGlyphX/treeGuideX）、导向线绘制、disclosure chevron、统一行构建器 sidebarTreeRow、可折叠分组头 sidebarSectionHeader、行高 token treeRowHeight，以及键盘光标脸与两个构建器返回的 clicked（鼠标点击或键盘激活）
 * [POS]: Git 引用树、Files 树与 Hosts 列表共享的呈现层；数据、折叠状态、动作与菜单仍由各自的树持有，键盘移动归 listNav（list_nav.go）
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// One grid for every sidebar hierarchy. A row reads
//
//	[pad][indent × level][chevron slot][gap][glyph][gap][title]
//
// Leaves keep the chevron slot blank, so siblings share one title column; a
// section header uses the same slot + gap before its title, so a first-level
// glyph sits exactly under the section's title and a first-level folder
// chevron under the section chevron. The guide of each ancestor runs through
// the middle of that ancestor's chevron slot.
const (
	// sidebarGutter is the left/right inset shared by the header, switch,
	// filter and tree of the Git and Files sidebars, so their edges line up.
	sidebarGutter float32 = 8

	treeRowPad   float32 = 6
	treeChevronW float32 = 12
	treeGlyphGap float32 = 4
	treeGlyphW   float32 = 12
	treeTitleGap float32 = 5

	// treeRowRadius is the corner of every selectable sidebar row: tree rows,
	// Changes rows and the commit list.
	treeRowRadius float32 = 4
)

func treeChevronX(level int) float32 { return treeRowPad + float32(level)*TreeIndentWidth }

// treeGlyphX is the left edge of a row's glyph at level. At level 0 it is
// also where a section title starts.
func treeGlyphX(level int) float32 { return treeChevronX(level) + treeChevronW + treeGlyphGap }

// treeGuideX is where the guide of ancestor level k runs.
func treeGuideX(k int) float32 { return treeChevronX(k) + treeChevronW/2 }

// treeRowHeight is the one density token behind the Git and Files trees:
// compact 22 / default 24 / comfortable 28 DIP — two tighter than the
// Workspace rows, whose marks carry a state dot.
func (s *Shell) treeRowHeight() float32 { return s.sidebarRowHeight() - 2 }

// treeGuides paints the vertical guide of every ancestor level behind a row.
// Rows stack without gaps, so the lines read as one continuous rule.
func treeGuides(row ui.Element, level int, color ui.Color) {
	if level <= 0 {
		return
	}
	row.Draw(func(p *ui.Painter, r ui.Rect) {
		for k := 0; k < level; k++ {
			x := r.X + treeGuideX(k)
			p.Line(x, r.Y, x, r.Y+r.H, 1, color)
		}
	})
}

// treeChevron fills the fixed chevron slot: a disclosure chevron for folders
// and sections, blank for leaves.
func treeChevron(c *ui.Context, folder, expanded bool, color ui.Color) {
	slot := ui.Box(c).Size(treeChevronW, treeChevronW).Center().Shrink(0).PassThrough()
	if !folder {
		return
	}
	slot.Children(func() {
		ic := ui.Icon(c, iconChevronDown).FontSize(10).TextColor(color)
		target := float32(0)
		if !expanded {
			target = -90
		}
		ic.Rotate(ic.Animate("rot", target, 150*time.Millisecond))
	})
}

// sidebarTreeSpec describes one row of a sidebar hierarchy.
type sidebarTreeSpec struct {
	Key      string
	Title    string
	Label    string // accessibility label; the title when empty
	Tooltip  string
	Level    int
	Folder   bool
	Expanded bool
	Glyph    *ui.SVG
	// Current marks the checked-out branch or the open file: accent tinted.
	Current bool
	// Selected is the row's selection face — the one every navigation list
	// wears (SidebarSelect). Current stays a state mark, not a selection.
	Selected bool
	// Action is the row's one hover icon at the trailing edge; its slot is
	// reserved while set, so the title never moves under the pointer.
	Action *treeRowAction
	// Dim mutes the title (symlinks).
	Dim bool
	// Trailing draws content at the row's right edge.
	Trailing func()
	// Menu is the row's context menu.
	Menu func(*ui.Menu)
	// Nav is the list's keyboard model; nil leaves the row to the mouse.
	Nav *listNav
}

// sidebarTreeRow draws one row of the grid and returns it with whether it was
// clicked. clicked also reports the keyboard's activation (o, l and h on a
// row whose fold state asks for it), so a tree answers the mouse and the keys
// in one place; Enter and Space already are clicks. Git references, Files and
// Hosts share it, so alignment, density, tints and guides cannot drift apart.
// The row holding the keyboard focus — the list's cursor — wears the
// selection face once the keyboard has used it.
func (s *Shell) sidebarTreeRow(c *ui.Context, spec sidebarTreeSpec) (ui.Element, bool) {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	label := spec.Label
	if label == "" {
		label = spec.Title
	}
	row := ui.ButtonBase(c.Key(spec.Key)).FillWidth().MinWidth(0).Height(s.treeRowHeight()).
		Padding(0, treeRowPad).Radius(treeRowRadius).FocusRing(false).AlignItems(ui.Center).
		Label(label).Tooltip(spec.Tooltip)
	glyph := t.TextMuted
	switch {
	case spec.Current:
		row.Background(t.Accent.Alpha(.11)).TextColor(t.Accent)
		glyph = t.Accent
	case spec.Selected, spec.Nav.showsCursor(row):
		row.Background(tokens.SidebarSelect)
	case row.Hovered():
		row.Background(t.SurfaceHover)
	}
	if spec.Menu != nil {
		row.ContextMenu(spec.Menu)
	}
	activated := spec.Nav.row(c, row, navRow{
		Key: spec.Key, Level: spec.Level, Folder: spec.Folder, Expanded: spec.Expanded,
		Pick: spec.Current || spec.Selected,
	})
	treeGuides(row, spec.Level, tokens.TreeLine)
	row.Children(func() {
		if spec.Level > 0 {
			ui.Box(c).Width(float32(spec.Level) * TreeIndentWidth).Shrink(0).PassThrough()
		}
		treeChevron(c, spec.Folder, spec.Expanded, t.TextMuted)
		ui.Box(c).Width(treeGlyphGap).Shrink(0).PassThrough()
		ui.Icon(c, spec.Glyph).FontSize(treeGlyphW).TextColor(glyph).Shrink(0)
		ui.Box(c).Width(treeTitleGap).Shrink(0).PassThrough()
		title := ui.Text(c, spec.Title).FontSize(Typography().Body).SingleLine().Grow(1).MinWidth(0)
		if spec.Dim && !spec.Current {
			title.TextColor(t.TextMuted)
		}
		if spec.Trailing != nil {
			spec.Trailing()
		}
		if spec.Action != nil {
			ui.Box(c).Size(gorexRowBtn+2, gorexRowBtn).Shrink(0).Center().Children(func() {
				if !row.Hovered() {
					return
				}
				action := gorexIconButton(c, gorexColorsOf(t.Dark), spec.Action.Glyph, spec.Action.Label, spec.Action.tooltip(), false, gorexRowBtn, gorexRowGlyph)
				if action.Clicked() {
					spec.Action.Run()
				}
			})
		}
	})
	return row, activated || row.Clicked()
}

// sidebarSectionSpec describes one collapsible group header of a sidebar
// hierarchy: the Git references' Branches/Tags/… and the Hosts' groups.
type sidebarSectionSpec struct {
	Key    string
	Title  string
	Count  int
	Closed bool
	// Tooltip is the full text of a clipped title (a file group's path).
	Tooltip string
	// Action is the header's one hover icon (a "+"); its slot is always
	// reserved so the count never jumps. The same action belongs in Menu.
	Action *treeRowAction
	// Menu is the header's context menu.
	Menu func(*ui.Menu)
	// Nav is the list's keyboard model; nil leaves the header to the mouse.
	Nav *listNav
}

// sidebarSectionHeader draws one group header: chevron, semibold title and
// the count. It returns the row with whether it was clicked (mouse or
// keyboard) — the fold state and the rows beneath stay the caller's.
func (s *Shell) sidebarSectionHeader(c *ui.Context, spec sidebarSectionSpec) (ui.Element, bool) {
	t := c.Theme()
	ui.Box(c).Height(4).Shrink(0)
	row := ui.ButtonBase(c.Key(spec.Key)).FillWidth().Height(s.treeRowHeight()+2).
		Padding(0, treeRowPad).Radius(treeRowRadius).FocusRing(false).AlignItems(ui.Center).
		Label("Toggle " + spec.Title).Tooltip(spec.Tooltip)
	switch {
	case spec.Nav.showsCursor(row):
		row.Background(designTokens(t.Dark).SidebarSelect)
	case row.Hovered():
		row.Background(t.SurfaceHover)
	}
	if spec.Menu != nil {
		row.ContextMenu(spec.Menu)
	}
	activated := spec.Nav.row(c, row, navRow{Key: spec.Key, Section: true, Folder: true, Expanded: !spec.Closed})
	row.Children(func() {
		treeChevron(c, true, !spec.Closed, t.TextMuted)
		ui.Box(c).Width(treeGlyphGap).Shrink(0).PassThrough()
		ui.Text(c, spec.Title).FontSize(Typography().Body).FontWeight(WeightSemibold).Grow(1).MinWidth(0).SingleLine()
		ui.Textf(c, "%d", spec.Count).FontSize(Typography().Micro).TextColor(t.TextMuted).Shrink(0)
		ui.Box(c).Size(gorexRowBtn+2, gorexRowBtn).Shrink(0).Center().Children(func() {
			if spec.Action == nil || !row.Hovered() {
				return
			}
			add := gorexIconButton(c, gorexColorsOf(t.Dark), spec.Action.Glyph, spec.Action.Label, spec.Action.tooltip(), false, gorexRowBtn, gorexRowGlyph)
			if add.Clicked() {
				spec.Action.Run()
			}
		})
	})
	return row, activated || row.Clicked()
}
