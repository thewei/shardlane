package nativeui

/**
 * [INPUT]: row level, MyGo Context/Painter, TreeLine token 与 Shell 的 density 偏好
 * [OUTPUT]: 侧栏层级共用的对齐网格（treeGlyphX/treeGuideX）、导向线绘制、disclosure chevron、统一行构建器 sidebarTreeRow 与行高 token treeRowHeight
 * [POS]: Git 引用树与 Files 树共享的呈现层；数据、动作与菜单仍由各自的树持有
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
	// Dim mutes the title (symlinks).
	Dim bool
	// Trailing draws content at the row's right edge.
	Trailing func()
	// Menu is the row's context menu.
	Menu func(*ui.Menu)
}

// sidebarTreeRow draws one row of the grid and returns it for the caller's
// Clicked(). Git references and Files share it, so alignment, density, tints
// and guides cannot drift apart.
func (s *Shell) sidebarTreeRow(c *ui.Context, spec sidebarTreeSpec) ui.Element {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	label := spec.Label
	if label == "" {
		label = spec.Title
	}
	row := ui.ButtonBase(c.Key(spec.Key)).FillWidth().MinWidth(0).Height(s.treeRowHeight()).
		Padding(0, treeRowPad).Radius(4).FocusRing(false).AlignItems(ui.Center).
		Label(label).Tooltip(spec.Tooltip)
	glyph := t.TextMuted
	if spec.Current {
		row.Background(t.Accent.Alpha(.11)).TextColor(t.Accent)
		glyph = t.Accent
	} else if row.Hovered() {
		row.Background(t.SurfaceHover)
	}
	if spec.Menu != nil {
		row.ContextMenu(spec.Menu)
	}
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
	})
	return row
}
