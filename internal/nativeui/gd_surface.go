package nativeui

import (
	"math"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/codehl"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// The diff review surface, ported from egoist/godiff diffview.go, view.go
// and find.go (commit 88b89e0): one virtualized list of file cards with
// sticky headers, gutters, syntax colors, word marks and gap expansion.

// gdLineSide is what one side of a line row shows.
type gdLineSide struct {
	present bool
	kind    gitworkbench.LineKind
	num     int
	text    string
	segs    []codehl.Seg
	words   []gitworkbench.WordRange
	hunk    int32
	index   int32 // the line's index in its hunk, -1 for expanded context
}

// gdDiffSurface renders the center Diff Review surface.
func (s *Shell) gdDiffSurface(c *ui.Context) {
	t := c.Theme()
	pal := gdPaletteFor(t)

	s.gdShortcuts(c)

	if s.git == nil || s.git.root == "" {
		cwd := s.selectedTabCWD()
		s.gdEmptyPanel(c, pal, "No Git repository", gdAbbreviateHome(cwd), func() {
			if ui.PrimaryButton(c, "Open Terminal").Clicked() {
				s.showSurface(WorkspaceSurfaceTerminal)
			}
		})
		return
	}
	if s.gitSnapshot() == nil {
		s.gdThinking(c)
		return
	}
	s.gdSetFiles()

	// A commit review never drifts; the drift banner is worktree-only.
	if s.git.staleBanner && !s.gdReviewingCommit() {
		s.gdChangedPill(c, pal)
	}

	find := ui.FindBar(c, &s.git.gdFinding, &s.git.gdQuery, len(s.git.gdMatches), &s.git.gdMatch).Label("Find in diffs")
	if find.FocusWithin() {
		s.git.gdTyping = true
	}
	if find.Changed() {
		s.gdShowMatch()
	}
	if s.git.gdQuery != s.git.gdMatchesFor && (s.git.gdFinding || s.git.gdMatchesFor != "") {
		s.git.gdRowsDirty = true
	}
	if !s.git.gdFinding && s.git.gdMatchesFor != "" {
		s.git.gdQuery = ""
		s.git.gdRowsDirty = true
	}

	if s.gdReviewingCommit() && s.git.commitSnap == nil {
		s.gdThinking(c)
		return
	}
	if len(s.git.gdFiles) == 0 {
		title, detail := "No local changes", gdAbbreviateHome(s.git.root)
		if s.gdReviewingCommit() {
			title, detail = "No changes in commit", s.git.commitHash
		}
		s.gdEmptyPanel(c, pal, title, detail, func() {
			if ui.PrimaryButton(c, "Open Terminal").Clicked() {
				s.showSurface(WorkspaceSurfaceTerminal)
			}
		})
		return
	}

	// The toolbar: the layout switch and the sync actions; the branch menu
	// lives in the sidebar's repo bar. A commit review is read-only, so the
	// sync actions only show for the worktree.
	if !s.gdReviewingCommit() {
		ui.Row(c).FillWidth().Padding(8, 12, 0).Gap(8).AlignItems(ui.Center).Children(func() {
			s.gdLayoutControl(c, pal)
			ui.Box(c).Grow(1)
			s.gdSyncButtons(c, pal)
		})
	}

	if s.git.gdRowsDirty {
		s.gdUpdateMatches()
		s.buildGdRows()
	}
	if len(s.git.gdRows) == 0 {
		title, detail := "No matching files", strings.TrimSpace(s.changes.filter)
		if s.gdSearching() {
			title, detail = "No matches in diffs", strings.TrimSpace(s.git.gdQuery)
		}
		s.gdEmptyPanel(c, pal, title, detail, nil)
		return
	}
	s.gdDiffList(c, pal)
}

// gdDiffList shows the files' cards, building the rows in view only.
func (s *Shell) gdDiffList(c *ui.Context, pal *gdPalette) {
	s.ensureGdListKeys()
	// The sheet is gone (2026-10-06): the files' cards sit on the gorex
	// card that hosts the surface; only the bands above file headers keep
	// the paper color, masking the lines that scroll under them.
	ui.List(c, &s.git.gdList, len(s.git.gdRows), func(i int) { s.gdDiffRow(c, pal, i) }).
		Grow(1).MinHeight(0).Padding(0, 12, 24).Label("Changes")

	// The file at the top follows the scrolling, and the tree with it.
	if first, _ := s.git.gdList.Visible(); first >= 0 && first < len(s.git.gdRows) && s.git.gdRows[first].kind != gdRowEnd {
		file := int(s.git.gdRows[first].file)
		if file != s.git.gdCurrent && timeSince(s.git.revealAt) > 1200*time.Millisecond {
			s.git.gdCurrent = file
			if top := s.git.gdFiles[file].cf.Path; top != "" {
				s.syncChangesSelectionFromDiff(top)
			}
		}
	}
}

// gdSyncButtons is the toolbar's fetch/pull/push cluster with the
// ahead/behind counts of the current branch.
func (s *Shell) gdSyncButtons(c *ui.Context, pal *gdPalette) {
	if s.git == nil || s.git.root == "" || s.currentBranchName() == "" {
		return
	}
	if st := s.git.upstream; st.OK && (st.Ahead > 0 || st.Behind > 0) {
		ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
			if st.Ahead > 0 {
				ui.Text(c, "↑"+itoa(st.Ahead)).Font(gdCodeFont()).FontSize(11).FontWeight(600).TextColor(pal.addText)
			}
			if st.Behind > 0 {
				ui.Text(c, "↓"+itoa(st.Behind)).Font(gdCodeFont()).FontSize(11).FontWeight(600).TextColor(pal.delText)
			}
		}).Tooltip("Ahead/behind " + st.Remote + "/" + st.Branch)
	}
	button := func(svg *ui.SVG, tip string, action func()) {
		b := gdIconButton(c, svg, tip)
		if s.git.opBusy {
			b.Disabled(true)
		}
		if b.Clicked() {
			action()
		}
	}
	button(iconRefresh, "Fetch", s.fetchRepo)
	button(iconArrowDown, "Pull (ff-only)", s.pullRepo)
	button(iconArrowUp, "Push", s.pushRepo)
}

// gdDiffRow draws one row of the surface.
func (s *Shell) gdDiffRow(c *ui.Context, pal *gdPalette, i int) {
	r := &s.git.gdRows[i]
	if r.kind == gdRowCommit {
		s.gdCommitCard(c, pal)
		return
	}
	f := s.git.gdFiles[r.file]
	switch r.kind {
	case gdRowHeader:
		s.gdFileHeader(c, pal, int(r.file), f)
	case gdRowNote:
		s.gdCard(c, pal).Padding(10, 16).Background(pal.gapBg).Children(func() {
			ui.Text(c, s.gdFileNote(f)).FontSize(12).TextColor(pal.gapText)
		})
	case gdRowGap:
		s.gdGapRow(c, pal, f, r)
	case gdRowLine:
		s.gdLineRow(c, pal, f, r)
	case gdRowEnd:
		if f.collapsed && !s.gdForceOpen(int(r.file)) {
			ui.Box(c).Height(0)
			return
		}
		ui.Box(c).Height(4).Background(pal.codeBg).
			BorderWidth(0, 1, 1, 1).BorderColor(pal.cardBorder).Radius(0, 0, gdCardRadius, gdCardRadius)
	}
}

// gdCard is a row inside a file's card, between its sides.
func (s *Shell) gdCard(c *ui.Context, pal *gdPalette) ui.Element {
	return ui.Row(c).Background(pal.codeBg).BorderWidth(0, 1, 0, 1).BorderColor(pal.cardBorder)
}

// gdForceOpen reports whether a collapsed file shows its lines anyway, as
// the files holding matches of the find bar do.
func (s *Shell) gdForceOpen(i int) bool {
	return s.gdSearching() && s.git.gdFileMatches[i]
}

// gdCodeFont is the family of the code.
func gdCodeFont() string { return "SF Mono, Menlo, monospace" }

// gdCodeSize is the size of the code, in points.
const gdCodeSize = 13

// gdLineHeight is the height of a line of code: 20 at 13.
func gdLineHeight() float32 {
	return float32(int(gdCodeSize * 20 / 13))
}

// gdCharWidth is the width of a column of code.
func (s *Shell) gdCharWidth(c *ui.Context) float32 {
	if s.git.gdCharW == 0 {
		width, _ := c.MeasureText(0, ui.Span{Text: strings.Repeat("0", 20), Font: gdCodeFont(), Size: gdCodeSize})
		s.git.gdCharW = width / 20
	}
	return s.git.gdCharW
}

// gdGutterWidth is the width of a column of line numbers.
func (s *Shell) gdGutterWidth(c *ui.Context, f *gdFile) float32 {
	return float32(f.metrics().digits)*s.gdCharWidth(c) + 18
}

// gdFileHeader draws one file's card header.
func (s *Shell) gdFileHeader(c *ui.Context, pal *gdPalette, idx int, f *gdFile) {
	t := c.Theme()
	cf := f.cf
	collapsed := f.collapsed && !s.gdForceOpen(idx)
	viewed := s.gdIsViewed(f)
	// The band above the header spaces the cards, and hides the lines
	// scrolling under it while it is pinned.
	band := ui.Column(c).Padding(12, 0, 0, 0).Background(pal.appBg)
	var h ui.Element
	band.Children(func() {
		h = ui.Row(c).Height(46).Padding(0, 8, 0, 6).Gap(8).Background(pal.headerBg).Border(1, pal.cardBorder)
	})
	if collapsed {
		h.Radius(gdCardRadius)
	} else {
		h.Radius(gdCardRadius, gdCardRadius, 0, 0)
	}
	if idx == s.git.gdCurrent && s.git.gdList.FocusWithin(c) {
		h.Border(1, t.Accent.Alpha(0.6))
	}
	h.ContextMenu(func(m *ui.Menu) {
		// Working-tree actions first, as Fork's file menu does; a commit
		// review is read-only and offers none of them.
		if !s.gdReviewingCommit() && cf.Untracked {
			if m.Item("Stage").Chosen() {
				s.stageFiles([]string{cf.Path})
			}
			if m.Item("Delete…").Chosen() {
				s.discardChanges([]string{cf.Path})
			}
		} else if !s.gdReviewingCommit() {
			if cf.Unstaged {
				if m.Item("Stage").Chosen() {
					s.stageFiles([]string{cf.Path})
				}
			}
			if cf.Staged {
				if m.Item("Unstage").Chosen() {
					s.unstageFiles([]string{cf.Path})
				}
			}
			if m.Item("Discard Changes…").Chosen() {
				s.discardChanges([]string{cf.Path})
			}
		}
		m.Separator()
		if m.Item("Open in Editor").Disabled(cf.Status == gitworkbench.StatusDeleted).Chosen() {
			s.openFileInEditor(cf.Path)
		}
		if m.Item("Copy Path").Chosen() {
			s.copyToClipboard(cf.Path)
		}
		if m.Item("Viewed").Checked(viewed).Chosen() {
			s.gdSetViewed(f, !viewed)
		}
		m.Separator()
		if m.Item("Collapse All").Chosen() {
			s.gdSetAllCollapsed(true)
		}
		if m.Item("Expand All").Chosen() {
			s.gdSetAllCollapsed(false)
		}
	})
	h.Children(func() {
		toggle := ui.ButtonBase(c).Gap(8).Grow(1).Shrink(1).MinWidth(0).Height(46).Label(cf.Path).Tooltip(cf.Path)
		if toggle.Clicked() {
			f.collapsed = !collapsed
			if !f.collapsed && s.gdSearching() {
				s.git.gdFileMatches[idx] = true
			}
			s.git.gdRowsDirty = true
		}
		toggle.FocusRing(false).Children(func() {
			chevron := ui.Box(c).Size(26, 26).Radius(13).Center().Shrink(0)
			if toggle.Hovered() {
				chevron.Background(pal.hover)
			}
			chevron.Children(func() {
				target := float32(0)
				if collapsed {
					target = -90
				}
				ic := ui.Icon(c, iconChevronDown).FontSize(15).TextColor(t.TextMuted)
				ic.Rotate(ic.Animate("rot", target, 160*time.Millisecond))
			})
			pathColor := t.Text
			if collapsed || viewed {
				pathColor = t.TextMuted
			}
			ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(1).Children(func() {
				dir, name := gdDirName(cf.Path)
				ui.RichText(c,
					ui.Span{Text: dir, Color: t.TextMuted},
					ui.Span{Text: name, Color: pathColor, Weight: 600},
				).Font(gdCodeFont()).FontSize(13).SingleLine()
				if cf.OldPath != "" && cf.OldPath != cf.Path {
					ui.Text(c, "from "+cf.OldPath).Font(gdCodeFont()).FontSize(11).TextColor(t.TextMuted).SingleLine()
				}
			})
		})
		// The copy button shows while the pointer is over the header; it
		// is always built, so that the elements after it keep their state.
		cp := gdIconButton(c, iconCopy, "Copy path")
		if !h.Hovered() && !cp.FocusVisible() {
			cp.Opacity(0)
		}
		if cp.Clicked() {
			s.copyToClipboard(cf.Path)
		}
		open := gdIconButton(c, iconOpen, "Open file in editor").Disabled(cf.Status == gitworkbench.StatusDeleted)
		if cf.Status == gitworkbench.StatusDeleted {
			open.Tooltip("Deleted files cannot be opened")
		}
		if open.Clicked() {
			s.openFileInEditor(cf.Path)
		}
		if adds, dels := f.sideCounts(); gdCountable(cf) && (adds > 0 || dels > 0) {
			ui.Row(c).Gap(8).Padding(4, 9).Radius(14).Background(pal.pill).Shrink(0).
				Tooltip(gdLines(adds, "added") + ", " + gdLines(dels, "removed")).Children(func() {
				ui.Text(c, "+"+gdThousands(adds)).Font(gdCodeFont()).FontSize(12).FontWeight(600).TextColor(pal.addText)
				ui.Text(c, "-"+gdThousands(dels)).Font(gdCodeFont()).FontSize(12).FontWeight(600).TextColor(pal.delText)
			})
		}
		if cf.Generated {
			ui.Text(c, "Generated").FontSize(11).FontWeight(600).Padding(4, 9).Radius(14).
				Background(pal.ref.Alpha(0.15)).TextColor(pal.ref).Shrink(0)
		}
		s.gdViewedButton(c, pal, f, viewed)
	})
}

// gdViewedButton marks a file viewed, which collapses it.
func (s *Shell) gdViewedButton(c *ui.Context, pal *gdPalette, f *gdFile, viewed bool) {
	t := c.Theme()
	on := viewed
	b := ui.CheckboxBase(c, &on).Gap(7).Height(30).Padding(0, 11).Radius(14).Shrink(0).
		Border(1, ui.RGBA(127, 127, 127, 0.22)).Label("Viewed")
	if b.Changed() {
		s.gdSetViewed(f, on)
	}
	if b.Hovered() {
		b.Background(pal.hover)
	}
	if viewed {
		b.Border(1, pal.viewed.Alpha(0.44)).TextColor(pal.viewed)
	}
	b.Children(func() {
		box := ui.Box(c).Size(15, 15).Radius(4).Center()
		if viewed {
			box.Background(pal.viewed).Children(func() {
				ui.Icon(c, iconCheck).FontSize(11).TextColor(pal.codeBg)
			})
		} else {
			box.Border(1.5, ui.RGBA(127, 127, 127, 0.45))
		}
		ui.Text(c, "Viewed").FontSize(12).FontWeight(600).TextColor(map[bool]ui.Color{true: pal.viewed, false: t.Text}[viewed])
	})
}

// gdSetAllCollapsed collapses or expands every file.
func (s *Shell) gdSetAllCollapsed(collapsed bool) {
	for _, f := range s.git.gdFiles {
		f.collapsed = collapsed
	}
	s.git.gdRowsDirty = true
}

// gdGapRow draws the collapsed unchanged lines with their expand controls.
func (s *Shell) gdGapRow(c *ui.Context, pal *gdPalette, f *gdFile, r *gdRow) {
	t := c.Theme()
	gutter := s.gdGutterWidth(c, f)
	if !s.gdSplitFile(f) && !f.oneSided() {
		gutter = 2*gutter - 4
	}
	first, last := int(r.gap) == 0, int(r.gap) == len(f.hunks())
	n := int(r.count)
	expand := func(top, bottom int) {
		if f.expanded == nil {
			f.expanded = map[int]gdGapShown{}
		}
		g := f.expanded[int(r.gap)]
		g.top += top
		g.bottom += bottom
		f.expanded[int(r.gap)] = g
		f.metricsDone = false
		s.git.gdRowsDirty = true
	}
	all := func() { expand(n, 0) }
	can := f.canExpand()
	s.gdCard(c, pal).Height(30).Background(pal.gapBg).Children(func() {
		ui.Row(c).Width(gutter + 4).Height(30).Shrink(0).Children(func() {
			button := func(svg *ui.SVG, tip string, action func()) {
				b := ui.ButtonBase(c).Grow(1).Height(30).Center().Label(tip).Tooltip(tip).TextColor(pal.gapText).Disabled(!can)
				b.BorderWidth(0, 2, 0, 0).BorderColor(pal.codeBg)
				if b.Hovered() {
					b.TextColor(t.Text).Background(pal.hover)
				}
				b.Children(func() { ui.Icon(c, svg).FontSize(14) })
				if b.Clicked() {
					action()
				}
			}
			switch {
			case n <= gdExpandStep && first:
				button(iconArrowUp, "Show the lines above", all)
			case n <= gdExpandStep && last:
				button(iconArrowDown, "Show the lines below", all)
			case n <= gdExpandStep:
				button(iconExpand, "Show the lines", all)
			default:
				if !first {
					button(iconArrowDown, "Show "+itoa(gdExpandStep)+" more lines", func() { expand(gdExpandStep, 0) })
				}
				if !last {
					button(iconArrowUp, "Show "+itoa(gdExpandStep)+" more lines", func() { expand(0, gdExpandStep) })
				}
			}
		})
		label := ui.ButtonBase(c).Padding(0, 12).Height(30).FocusRing(false).Disabled(!can).Children(func() {
			text := itoa(n) + " unmodified lines"
			if n == 1 {
				text = "1 unmodified line"
			}
			ui.Text(c, text).FontSize(12).TextColor(pal.gapText)
		})
		if label.Hovered() && can {
			label.Underline()
		}
		if label.Clicked() {
			switch {
			case n <= gdExpandStep:
				all()
			case first:
				expand(0, gdExpandStep)
			case last:
				expand(gdExpandStep, 0)
			default:
				expand(gdExpandStep, gdExpandStep)
			}
		}
		if n > gdExpandStep {
			b := ui.ButtonBase(c).Padding(3, 8).Radius(6).Disabled(!can).Children(func() {
				ui.Text(c, "Expand all").FontSize(12).FontWeight(600).TextColor(pal.gapText)
			})
			if b.Hovered() {
				b.Background(pal.hover)
			}
			if b.Clicked() {
				all()
			}
		}
		if h := int(r.gap); h < len(f.hunks()) && f.hunks()[h].Section != "" {
			ui.Text(c, f.hunks()[h].Section).Font(gdCodeFont()).FontSize(12).TextColor(pal.gapText).SingleLine().Grow(1).Shrink(1).MinWidth(0)
		}
	})
}

// gdSideOf returns a line of a row for the old or the new side.
func (s *Shell) gdSideOf(f *gdFile, r *gdRow, side gdSide) gdLineSide {
	if r.hunk < 0 {
		// Expanded context.
		ls := gdLineSide{present: true, kind: gitworkbench.KindContext, hunk: -1, index: -1}
		if side == gdSideOld {
			ls.num = int(r.old)
			ls.text = f.contextText(r.old, r.new)
			if int(r.old) <= len(f.oldHL) {
				ls.segs = f.oldHL[r.old-1]
			} else if int(r.new) <= len(f.newHL) {
				ls.segs = f.newHL[r.new-1]
			}
		} else {
			ls.num = int(r.new)
			ls.text = f.contextText(r.old, r.new)
			if int(r.new) <= len(f.newHL) {
				ls.segs = f.newHL[r.new-1]
			}
		}
		return ls
	}
	idx := r.a
	if side == gdSideNew && s.gdSplitFile(f) {
		idx = r.b
	}
	l := f.lineAt(r.hunk, idx)
	if l == nil {
		return gdLineSide{}
	}
	ls := gdLineSide{present: true, kind: l.Kind, text: l.Text, hunk: r.hunk, index: idx}
	if l.Kind == gitworkbench.KindDelete {
		ls.num = l.OldLine
		if l.OldLine <= len(f.oldHL) {
			ls.segs = f.oldHL[l.OldLine-1]
		}
	} else {
		ls.num = l.NewLine
		if side == gdSideOld && l.Kind == gitworkbench.KindContext {
			ls.num = l.OldLine
		}
		if l.NewLine > 0 && l.NewLine <= len(f.newHL) {
			ls.segs = f.newHL[l.NewLine-1]
		}
	}
	ls.words = f.words[[2]int{int(r.hunk), int(idx)}]
	return ls
}

// gdLineRow draws one line, or a pair of lines side by side.
func (s *Shell) gdLineRow(c *ui.Context, pal *gdPalette, f *gdFile, r *gdRow) {
	gutter := s.gdGutterWidth(c, f)
	lh := gdLineHeight()
	e := s.gdCard(c, pal).AlignItems(ui.Stretch).MinHeight(lh)
	hs := s.git.gdHScroll[f.cf.Path]
	if !s.git.gdWordWrap {
		e.HandleInput(func(ev ui.InputEvent) bool {
			if ev.Kind != ui.InputScroll || math.Abs(float64(ev.DX)) <= math.Abs(float64(ev.DY)) {
				return false
			}
			b := e.Bounds()
			code := b.W - 2*gutter - 40
			if s.gdSplitFile(f) {
				code = b.W/2 - gutter - 30
			}
			limit := max(float32(f.metrics().maxCols)*s.gdCharWidth(c)-code, 0)
			s.git.gdHScroll[f.cf.Path] = min(max(s.git.gdHScroll[f.cf.Path]+ev.DX, 0), limit)
			s.win.Invalidate()
			return true
		})
	}
	if !s.gdSplitFile(f) {
		side := gdSideNew
		side2 := s.gdSideOf(f, r, gdSideNew)
		if side2.kind == gitworkbench.KindDelete {
			side = gdSideOld
		}
		e.Children(func() {
			s.gdLineCell(c, pal, f, r, side2, side, gutter, hs, !f.oneSided())
		})
		return
	}
	e.Children(func() {
		left := s.gdSideOf(f, r, gdSideOld)
		right := s.gdSideOf(f, r, gdSideNew)
		ui.Row(c).Grow(1).Basis(0).MinWidth(0).AlignItems(ui.Stretch).Children(func() {
			s.gdLineCell(c, pal, f, r, left, gdSideOld, gutter, hs, false)
		})
		ui.Box(c).Width(1).Shrink(0).Background(pal.cardBorder)
		ui.Row(c).Grow(1).Basis(0).MinWidth(0).AlignItems(ui.Stretch).Children(func() {
			s.gdLineCell(c, pal, f, r, right, gdSideNew, gutter, hs, false)
		})
	})
}

// gdLineCell shows a line: its numbers and its code.
func (s *Shell) gdLineCell(c *ui.Context, pal *gdPalette, f *gdFile, r *gdRow, ls gdLineSide, sd gdSide, gutter, hs float32, unified bool) {
	lh := gdLineHeight()
	if !ls.present {
		ui.Box(c).Grow(1).Background(pal.emptySide).MinHeight(lh)
		return
	}
	bg, gutterBg, bar, numColor := pal.codeBg, pal.codeBg, ui.Transparent, pal.lineNumber
	var wordColor ui.Color
	switch ls.kind {
	case gitworkbench.KindAdd:
		bg, gutterBg, bar, numColor, wordColor = pal.addBg, pal.addGutter, pal.addBar, pal.addBar, pal.addWord
	case gitworkbench.KindDelete:
		bg, gutterBg, bar, numColor, wordColor = pal.delBg, pal.delGutter, pal.delBar, pal.delBar, pal.delWord
	}
	cell := ui.Row(c).Grow(1).MinWidth(0).AlignItems(ui.Stretch).Background(bg)
	if s.gdIsSelectedLine(f, ls) {
		cell.Background(gdBlend(bg, pal.selected))
		gutterBg = gdBlend(gutterBg, pal.selected)
	} else if cell.Hovered() {
		cell.Background(gdBlend(bg, pal.hover.Alpha(0.5)))
		gutterBg = gdBlend(gutterBg, pal.hover.Alpha(0.5))
	}
	cell.ContextMenu(func(m *ui.Menu) {
		if m.Item("Copy Line").Chosen() {
			s.copyToClipboard(ls.text)
		}
		m.Separator()
		if m.Item("Open in Editor").Disabled(f.cf.Status == gitworkbench.StatusDeleted).Chosen() {
			s.openFileInEditor(f.cf.Path)
		}
		if m.Item("Copy Path").Chosen() {
			s.copyToClipboard(f.cf.Path)
		}
	})
	cell.Children(func() {
		num := func(n int, width float32) {
			txt := ""
			if n > 0 {
				txt = itoa(n)
			}
			ui.Text(c, txt).Font(gdCodeFont()).FontSize(gdCodeSize-1).FixedLineHeight(lh).TextColor(numColor).
				Width(width).TextAlign(ui.End).Padding(0, 8, 0, 0).Shrink(0)
		}
		ui.Row(c).Shrink(0).AlignItems(ui.Start).Background(gutterBg).Children(func() {
			ui.Box(c).Width(4).AlignSelf(ui.Stretch).Background(bar)
			if unified {
				old, new := 0, 0
				if r.hunk < 0 {
					old, new = int(r.old), int(r.new)
				} else if l := f.lineAt(r.hunk, ls.index); l != nil {
					old, new = l.OldLine, l.NewLine
				}
				num(old, gutter-4)
				num(new, gutter-4)
			} else {
				num(ls.num, gutter-4)
			}
		})
		spans := s.gdLineSpans(f, ls, sd, pal, wordColor)
		// The margin keeps the clipped code clear of the line numbers.
		code := ui.Box(c).Grow(1).Basis(0).MinWidth(0).ClipX().Margin(0, 10)
		code.Children(func() {
			text := ui.RichText(c, spans...).Font(gdCodeFont()).
				FontSize(gdCodeSize).FixedLineHeight(lh).TextColor(pal.code)
			if !s.git.gdWordWrap {
				text.NoWrap().AlignSelf(ui.Start).Left(-hs)
			}
		})
	})
}

// gdLineSpans styles the code of a line: kept from frame to frame, apart from
// the lines holding matches of the find bar, whose marks move.
func (s *Shell) gdLineSpans(f *gdFile, ls gdLineSide, sd gdSide, pal *gdPalette, wordColor ui.Color) []ui.Span {
	marks := gdWordMarks(ls.text, ls.words, wordColor)
	if s.gdSearching() {
		if found := gdFindRanges(ls.text, s.git.gdQuery); found != nil {
			active := s.gdActiveMatch(f, ls)
			for _, rg := range found {
				color := pal.match
				if active {
					color = pal.matchNow
				}
				marks = append(marks, gdMark{start: rg[0], end: rg[1], color: color})
			}
			return gdCodeSpans(ls.text, ls.segs, marks, pal)
		}
	}
	key := gdSpanKey{hunk: ls.hunk, index: ls.index, num: int32(ls.num), side: sd, dark: pal == &gdDarkPalette}
	if spans, ok := f.spans[key]; ok {
		return spans
	}
	spans := gdCodeSpans(ls.text, ls.segs, marks, pal)
	if f.spans == nil {
		f.spans = map[gdSpanKey][]ui.Span{}
	}
	f.spans[key] = spans
	return spans
}

// gdCommitCard shows the message of the commit under review, above its
// changes, as Godiff's commit row does. The card is one row — text block
// left, back button right — because stacked children inside one virtualized
// list row measured short and let the button overlap the subject text.
func (s *Shell) gdCommitCard(c *ui.Context, pal *gdPalette) {
	t := c.Theme()
	cm := s.git.commitMeta
	if cm == nil {
		return
	}
	ui.Column(c).Padding(12, 16, 0).Children(func() {
		ui.Row(c).FillWidth().Padding(10, 16).Gap(12).AlignItems(ui.Center).
			Radius(gdCardRadius).Background(pal.headerBg).Border(1, pal.cardBorder).Children(func() {
			ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(1).Children(func() {
				ui.Text(c, cm.Subject).FontSize(14).FontWeight(600).SingleLine()
				ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
					ui.Textf(c, "%s committed %s", cm.Author, gdRelativeTime(cm.Time)).
						FontSize(11).TextColor(t.TextMuted).Shrink(0)
					ui.Spacer(c)
					ui.Text(c, cm.Short).Font(gdCodeFont()).FontSize(12).TextColor(pal.ref).Shrink(0)
				})
			})
			if ui.Button(c, "Back to Local Changes").FontSize(Typography().Micro).Shrink(0).Clicked() {
				s.reviewLocalChanges()
			}
		})
	})
}

// gdBlend lays a translucent color over another.
func gdBlend(base, over ui.Color) ui.Color { return over.Over(base) }

// openFileInEditor opens the file through argv, never a shell string
// (GWB-194). Falls back silently when no editor is configured.
func (s *Shell) openFileInEditor(relPath string) {
	full := joinRepoPath(s.gitRoot(), relPath)
	if full == "" {
		return
	}
	s.openExternalPath(full)
}

func joinRepoPath(root, rel string) string {
	if root == "" || rel == "" {
		return ""
	}
	if strings.Contains(rel, "\x00") {
		return ""
	}
	return root + "/" + rel
}

// gitRoot returns the bound repository root.
func (s *Shell) gitRoot() string {
	if s.git == nil {
		return ""
	}
	return s.git.root
}

// gdDirName splits a repo-relative path into its directory and its name.
func gdDirName(path string) (dir, name string) {
	if i := strings.LastIndexByte(path, '/'); i >= 0 {
		return path[:i+1], path[i+1:]
	}
	return "", path
}

// gdIconButton is a button showing an icon alone, as in a toolbar.
func gdIconButton(c *ui.Context, svg *ui.SVG, tip string) ui.Element {
	t := c.Theme()
	b := ui.ButtonBase(c).Size(28, 28).Radius(7).Center().Label(tip).Tooltip(tip).TextColor(t.TextMuted)
	if b.Pressed() {
		b.Background(ui.RGBA(127, 127, 127, 0.22))
	} else if b.Hovered() {
		b.Background(ui.RGBA(127, 127, 127, 0.13))
	}
	b.Children(func() { ui.Icon(c, svg).FontSize(16) })
	return b
}

// gdSearchInput is a field filtering a list, as the search fields of
// macOS: a magnifying glass, the text, and a button clearing it, as Escape
// does. It reports a change; typing reports a focused field.
func gdSearchInput(c *ui.Context, query *string, placeholder string, focus, typing *bool) bool {
	t := c.Theme()
	changed := false
	box := ui.Row(c).Height(28).Padding(0, 6, 0, 8).Gap(6).Radius(7).Background(ui.RGBA(127, 127, 127, 0.12))
	box.Children(func() {
		ui.Icon(c, iconSearch).FontSize(13).TextColor(t.TextMuted)
		in := ui.TextInputBase(c, query).Placeholder(placeholder).Label(placeholder).FontSize(13).Grow(1).MinWidth(0)
		if *focus {
			in.Focus()
			*focus = false
		}
		if in.Changed() {
			changed = true
		}
		if in.Focused() {
			box.Shadow(0, 0, 0, 3, t.Focus.Alpha(0.45))
			if typing != nil {
				*typing = true
			}
		}
		if *query != "" {
			clear := ui.ButtonBase(c).Size(16, 16).Radius(8).Center().Background(t.TextMuted.Alpha(0.45)).Label("Clear").FocusRing(false)
			clear.Children(func() { ui.Icon(c, iconClose).FontSize(10).TextColor(t.Background) })
			if clear.Clicked() || in.Shortcut(0, ui.KeyEscape) {
				*query = ""
				changed = true
			}
		}
	})
	return changed
}

// gdEmptyPanel says why there is nothing to show.
func (s *Shell) gdEmptyPanel(c *ui.Context, pal *gdPalette, title, detail string, actions func()) {
	t := c.Theme()
	ui.Column(c).Grow(1).Center().Padding(24).Children(func() {
		ui.Column(c).MaxWidth(520).Padding(28).Gap(10).Radius(16).Background(pal.headerBg).Border(1, pal.cardBorder).AlignItems(ui.Center).Children(func() {
			ui.Icon(c, iconFileDiff).FontSize(28).TextColor(t.TextMuted)
			ui.Text(c, title).FontSize(15).Bold().TextAlign(ui.Center)
			if detail != "" {
				ui.Text(c, detail).FontSize(13).Font(gdCodeFont()).TextColor(t.TextMuted).TextAlign(ui.Center)
			}
			if actions != nil {
				ui.Row(c).Gap(8).Margin(6, 0, 0).Children(actions)
			}
		})
	})
}

// gdThinking shows that the changes are loading.
func (s *Shell) gdThinking(c *ui.Context) {
	t := c.Theme()
	ui.Column(c).Grow(1).Center().Gap(8).Children(func() {
		ui.Spinner(c).Size(18, 18)
		label := ui.Text(c, "Thinking…").Italic().FontSize(13).Font(gdCodeFont()).TextColor(t.TextMuted)
		label.Opacity(0.5 + 0.5*label.Loop("pulse", 1600*time.Millisecond, ui.Bounce(ui.EaseInOut)))
	})
}

// gdChangedPill tells that the work tree changed since the review loaded.
func (s *Shell) gdChangedPill(c *ui.Context, pal *gdPalette) {
	t := c.Theme()
	ui.Row(c).Justify(ui.Center).Padding(8, 12, 0).Children(func() {
		ui.Row(c).Gap(6).Padding(5, 6, 5, 12).Radius(16).Background(pal.viewed.Alpha(0.1).Over(pal.codeBg)).
			Border(1, pal.viewed.Alpha(0.2)).Children(func() {
			ui.Text(c, "Local changes detected,").FontSize(13).FontWeight(600).TextColor(pal.viewed.Mix(t.Text, 0.6))
			ref := ui.ButtonBase(c).FocusRing(true).Children(func() {
				ui.Text(c, "refresh to see them.").FontSize(13).FontWeight(600).Underline().TextColor(pal.viewed.Mix(t.Text, 0.6))
			})
			if ref.Clicked() {
				s.refreshGitChanges()
			}
			if gdIconButton(c, iconClose, "Dismiss").Size(22, 22).Clicked() {
				s.git.staleBanner = false
			}
		})
	})
}

// gdLayoutControl switches between split and unified diffs.
func (s *Shell) gdLayoutControl(c *ui.Context, pal *gdPalette) {
	t := c.Theme()
	choice := 0
	if !s.git.splitLayout {
		choice = 1
	}
	seg := ui.SegmentedBase(c, &choice, 2)
	seg.Track.Padding(2).Gap(2).Radius(8).Background(ui.RGBA(127, 127, 127, 0.1)).Label("Diff layout").Children(func() {
		for i, it := range []struct {
			ic   *ui.SVG
			name string
		}{{iconSplit, "Split"}, {iconUnified, "Unified"}} {
			seg2 := seg.Segment(i).Size(30, 24).Radius(6).Center().Label(it.name).Tooltip(it.name).TextColor(t.TextMuted)
			if i == choice {
				seg2.Background(pal.headerBg).Shadow(0, 1, 2, 0, ui.RGBA(0, 0, 0, 0.12)).TextColor(t.Text)
			}
			seg2.Children(func() { ui.Icon(c, it.ic).FontSize(15) })
		}
	})
	if (choice == 0) != s.git.splitLayout {
		s.toggleDiffLayout()
	}
}

// gdShortcuts handles the surface's keys.
func (s *Shell) gdShortcuts(c *ui.Context) {
	typing := s.git.gdTyping
	s.git.gdTyping = false
	if typing || s.git.root == "" || s.gitSnapshot() == nil {
		// No diff list is up to move within (empty panel / still loading).
		return
	}
	if c.Shortcut(0, ui.KeyJ) || c.Shortcut(ui.Ctrl, ui.KeyDown) {
		s.gdNextHunk(1)
	}
	if c.Shortcut(0, ui.KeyK) || c.Shortcut(ui.Ctrl, ui.KeyUp) {
		s.gdNextHunk(-1)
	}
	if c.Shortcut(ui.Alt, ui.KeyZ) {
		s.git.gdWordWrap = !s.git.gdWordWrap
		s.git.gdRowsDirty = true
	}
}
