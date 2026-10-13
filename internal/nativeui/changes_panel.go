package nativeui

import (

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// changesPanelState is the Changes tool's presentation state (filter,
// footer totals are derived).
type changesPanelState struct {
	filter      string
	filterFocus bool
}

// gdChangedTreeList renders one Godiff-style changed-file tree list over the
// given state (the Right Panel tool and the Diff-mode sidebar panes each
// hold their own list state over the same tree data). onClick receives the
// activated row. choice, when not nil, lets Cmd-click and Shift-click choose
// several rows, held by path in the caller's selection.
func (s *Shell) gdChangedTreeList(c *ui.Context, pal *gdPalette, rows []gitworkbench.FlatRow, state *ui.ListState, selected *int, choice *ui.Selection[string], pane gdViewSide, onClick func(gitworkbench.FlatRow)) ui.Element {
	if choice != nil {
		// Rows identified by path: the choice and the list's place follow
		// their files as rows come and go.
		state.Key = func(i int) any {
			if i >= 0 && i < len(rows) {
				return rows[i].Node.Path
			}
			return i
		}
		state.Selection = choice
	}
	list := ui.List(c, state, len(rows), func(i int) {
		if i < 0 || i >= len(rows) {
			return
		}
		s.renderChangesRow(c, pal, rows[i], state, pane, choice, onClick)
	}).Grow(1).MinHeight(0).Padding(3, sidebarGutter)
	// A plain click opens the file: the list replaced the choice with that
	// one row. Choice-growing clicks (Cmd, Shift) stay preview-free.
	if changed := list.Changed(); changed && *selected >= 0 && *selected < len(rows) {
		row := rows[*selected]
		if !row.Node.Dir && (choice == nil || (choice.Len() == 1 && choice.Has(row.Node.Path))) {
			onClick(row)
		}
	}
	return list
}

// gdChangesFooter draws the Total row and the Commit action.
func (s *Shell) gdChangesFooter(c *ui.Context, pal *gdPalette, snap *gitworkbench.ChangesSnapshot) {
	t := c.Theme()
	ui.Row(c).FillWidth().MinHeight(40).Padding(6, 10).Gap(8).AlignItems(ui.Center).
		BorderWidth(1, 0, 0, 0).BorderColor(pal.cardBorder).Children(func() {
		if snap.Truncated {
			ui.Text(c, "partial").FontSize(Typography().Micro).TextColor(t.TextMuted)
		}
		ui.Row(c).Gap(6).Tooltip("Total change: " + gdLines(snap.TotalAdditions, "added") + ", " + gdLines(snap.TotalDeletions, "removed")).Children(func() {
			ui.Text(c, "Total:").FontSize(Typography().BodySmall).FontWeight(WeightSemibold).TextColor(t.TextMuted)
			ui.Text(c, "+"+gdThousands(snap.TotalAdditions)).Font(gdCodeFont()).FontSize(Typography().BodySmall).FontWeight(WeightSemibold).TextColor(pal.addText)
			ui.Text(c, "-"+gdThousands(snap.TotalDeletions)).Font(gdCodeFont()).FontSize(Typography().BodySmall).FontWeight(WeightSemibold).TextColor(pal.delText)
		})
		// Commit is deliberately available only once, in the center
		// Workbench toolbar. This footer is a read-only total summary.
	})
}

// currentChangesTree builds (or reuses) the filtered changed-path tree.
func (s *Shell) currentChangesTree(snap *gitworkbench.ChangesSnapshot) *gitworkbench.TreeNode {
	if s.changesTree != nil {
		return s.changesTree
	}
	tree := gitworkbench.BuildTree(snap.Files, true)
	s.changesTree = gitworkbench.FilterTree(tree, s.changes.filter)
	return s.changesTree
}

// renderChangesRow draws one compact tree row on the shared sidebar grid:
// chevron slot · icon · name · hover action · +N -N · status letter
// (GWB-141..143). Folders and files of one level share their icon and name
// columns, and guides run through the ancestors' chevron slots.
// state is the owning list, for the focused-selection tint. choice is the
// pane's multi-selection, whose rows tint like an extended selection.
func (s *Shell) renderChangesRow(c *ui.Context, pal *gdPalette, row gitworkbench.FlatRow, state *ui.ListState, pane gdViewSide, choice *ui.Selection[string], onClick func(gitworkbench.FlatRow)) {
	t := c.Theme()
	node := row.Node

	chosen := false
	if choice != nil && !node.Dir && node.File != nil {
		chosen = choice.Has(node.Path)
	}
	selected := false
	if !node.Dir && node.File != nil {
		selected = node.Path == s.surface.diff.SelectedPath
	}
	focused := state.FocusWithin(c)

	r := ui.Row(c).Height(s.treeRowHeight()).Padding(0, treeRowPad, 0, treeRowPad).AlignItems(ui.Center).Radius(4).MinWidth(0)
	textColor := t.Text
	muted := t.TextMuted
	switch {
	case chosen && focused:
		r.Background(t.Accent)
		textColor = t.AccentText
		muted = t.AccentText.Alpha(0.8)
	case chosen:
		r.Background(designTokens(t.Dark).SidebarSelect)
	case selected && focused:
		r.Background(t.Accent)
		textColor = t.AccentText
		muted = t.AccentText.Alpha(0.8)
	case selected:
		r.Background(designTokens(t.Dark).SidebarSelect)
	case r.Hovered():
		r.Background(designTokens(t.Dark).SidebarHover)
	}
	treeGuides(r, row.Depth, designTokens(t.Dark).TreeLine)
	r.TextColor(textColor).Children(func() {
		if row.Depth > 0 {
			ui.Box(c).Width(float32(row.Depth) * TreeIndentWidth).Shrink(0).PassThrough()
		}
		treeChevron(c, node.Dir, !s.git.collapsedDirs[node.Path], muted)
		ui.Box(c).Width(treeGlyphGap).Shrink(0).PassThrough()
		if node.Dir {
			ui.Icon(c, iconFolder).FontSize(treeGlyphW).TextColor(muted).Shrink(0)
			ui.Box(c).Width(treeTitleGap).Shrink(0).PassThrough()
			ui.Text(c, node.Name).FontSize(Typography().Body).SingleLine().Grow(1).Shrink(1).MinWidth(0).Tooltip(node.Path)
			if !s.gdReviewingCommit() && (pane == gdViewUnstaged || pane == gdViewStaged) {
				paths := gitDirectoryPaths(s.gitSnapshot(), node.Path, pane)
				if len(paths) > 0 {
					glyph, verb, action := iconPlus, "Stage", s.stageFiles
					if pane == gdViewStaged {
						glyph, verb, action = iconMinus, "Unstage", s.unstageFiles
					}
					button := ui.ButtonBase(c).Size(18, 18).Radius(4).Center().Shrink(0).TextColor(muted).
						Label(verb + " folder " + node.Path).Tooltip(verb + " " + node.Path + " (" + itoa(len(paths)) + " files)")
					if button.Hovered() {
						button.Background(pal.hover).TextColor(t.Text)
					}
					if !r.Hovered() && !button.Hovered() {
						button.Opacity(0)
					}
					button.Children(func() { ui.Icon(c, glyph).FontSize(12) })
					button.Disabled(s.git.opBusy)
					if button.Clicked() {
						action(paths)
					}
				}
			}
			return
		}
		cf := *node.File
		viewed := s.git.gdViewed[cf.Path] != "" && s.git.gdViewed[cf.Path] == cf.Fingerprint
		ui.Icon(c, iconFile).FontSize(treeGlyphW).TextColor(muted).Shrink(0)
		ui.Box(c).Width(treeTitleGap).Shrink(0).PassThrough()
		// Full path on hover (F113): the 250px pane truncates names so
		// service_index.go and service_index_test.go read as the same row.
		name := ui.Text(c, node.Name).FontSize(Typography().Body).SingleLine().Grow(1).Shrink(1).MinWidth(0).Tooltip(node.Path)
		if viewed && !(selected && focused) {
			name.TextColor(t.TextMuted)
		}
		ui.Row(c).Gap(5).AlignItems(ui.Center).Shrink(0).Children(func() {
			// The per-file Stage/Unstage button shows while the row is hovered,
			// as Fork's +/- does; which one depends on the pane.
			if action, glyph, verb := s.gdRowAction(&cf, pane); action != nil {
				b := ui.ButtonBase(c).Size(18, 18).Radius(4).Center().Shrink(0).
					Label(verb + " " + cf.Path).Tooltip(verb).TextColor(muted)
				if b.Hovered() {
					b.Background(pal.hover).TextColor(t.Text)
				}
				if !r.Hovered() && !b.Hovered() {
					b.Opacity(0)
				}
				b.Children(func() { ui.Icon(c, glyph).FontSize(12) })
				if b.Clicked() {
					action([]string{cf.Path})
				}
			}
			if gdCountable(&cf) && (cf.Additions > 0 || cf.Deletions > 0) {
				ui.Textf(c, "+%s -%s", gdCompact(cf.Additions), gdCompact(cf.Deletions)).
					Font(gdCodeFont()).FontSize(Typography().Micro).FontWeight(WeightSemibold).TextColor(muted).Shrink(0).
					Tooltip(gdLines(cf.Additions, "added") + ", " + gdLines(cf.Deletions, "removed"))
			}
			letter := gdStatusLetterColor(cf.Status, pal, t)
			switch {
			case selected && focused:
				letter = t.AccentText
			case viewed:
				letter = t.TextMuted
			}
			ui.Text(c, gdStatusLetter(&cf)).Font(gdCodeFont()).FontSize(Typography().BodySmall).FontWeight(WeightSemibold).TextColor(letter).
				Width(12).TextAlign(ui.Center).Shrink(0)
		})
	})
	// Directory disclosure is independent of List.Changed: selecting the
	// same directory twice does not change selection, so the old handler
	// could never reliably reopen a collapsed folder.
	if node.Dir && r.Clicked() {
		onClick(row)
	}
	if node.Dir && !s.gdReviewingCommit() && (pane == gdViewUnstaged || pane == gdViewStaged) {
		paths := gitDirectoryPaths(s.gitSnapshot(), node.Path, pane)
		r.ContextMenu(func(m *ui.Menu) {
			if choice != nil && m.Item("Select all files in folder").Disabled(len(paths) == 0).Chosen() {
				for _, path := range paths {
					choice.Add(path)
				}
			}
			if pane == gdViewUnstaged && m.Item("Stage folder ("+itoa(len(paths))+" files)").Disabled(len(paths) == 0 || s.git.opBusy).Chosen() {
				s.stageFiles(paths)
			}
			if pane == gdViewStaged && m.Item("Unstage folder ("+itoa(len(paths))+" files)").Disabled(len(paths) == 0 || s.git.opBusy).Chosen() {
				s.unstageFiles(paths)
			}
		})
	}
	if !node.Dir && node.File != nil {
		cf := *node.File
		r.ContextMenu(func(m *ui.Menu) {
			s.gdRowContextMenu(m, &cf)
		})
	}
}

// gdRowAction returns the hover action of a pane's file row: Stage in the
// unstaged pane, Unstage in the staged pane; nil in the combined tree.
func (s *Shell) gdRowAction(cf *gitworkbench.ChangeFile, pane gdViewSide) (func([]string), *ui.SVG, string) {
	if s.gdReviewingCommit() {
		return nil, nil, ""
	}
	switch pane {
	case gdViewUnstaged:
		return s.stageFiles, iconPlus, "Stage"
	case gdViewStaged:
		return s.unstageFiles, iconMinus, "Unstage"
	default:
		if cf.Unstaged {
			return s.stageFiles, iconPlus, "Stage"
		}
		if cf.Staged {
			return s.unstageFiles, iconMinus, "Unstage"
		}
		return nil, nil, ""
	}
}

// gdRowContextMenu is a tree row's right-click menu: the working-tree file
// operations, as Fork's lists offer them.
func (s *Shell) gdRowContextMenu(m *ui.Menu, cf *gitworkbench.ChangeFile) {
	commit := s.gdReviewingCommit()
	if !commit {
		if cf.Untracked {
			if m.Item("Stage").Chosen() {
				s.stageFiles([]string{cf.Path})
			}
			if m.Item("Delete…").Chosen() {
				s.discardChanges([]string{cf.Path})
			}
		} else {
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
			m.Separator()
		}
	}
	if !commit && cf.Status != gitworkbench.StatusDeleted {
		if m.Item("Open in Editor").Chosen() {
			s.openFileInEditor(cf.Path)
		}
	}
	if m.Item("Copy Path").Chosen() {
		s.copyToClipboard(cf.Path)
	}
}

// changesClick handles row activation: dir toggles collapse, file opens the
// center diff (GWB-145).
func (s *Shell) changesClick(row gitworkbench.FlatRow) {
	if row.Node.Dir {
		path := row.Node.Path
		s.git.collapsedDirs[path] = !s.git.collapsedDirs[path]
		return
	}
	s.openChanges(row.Node.Path)
	// Right→center: switch to Diff and reveal the card (plan §20).
	s.revealDiffFile(row.Node.Path)
}

// syncChangesSelectionFromDiff expands ancestors and scrolls the selected
// tree row into view (center→right, GWB-181) — guarded against oscillation.
func (s *Shell) syncChangesSelectionFromDiff(topPath string) {
	if s.git == nil || topPath == "" || s.diffScrollGuardActive() {
		return
	}
	if topPath == s.surface.diff.SelectedPath {
		return
	}
	s.surface.diff.SelectedPath = topPath
	gitworkbench.EnsureAncestorsOpen(s.git.collapsedDirs, topPath)
	s.changesTree = nil
	if snap := s.gitSnapshot(); snap != nil {
		rows := gitworkbench.Flatten(s.currentChangesTree(snap), s.git.collapsedDirs)
		for i, row := range rows {
			if !row.Node.Dir && row.Node.Path == topPath {
				s.changesList.ScrollIntoView(i)
				break
			}
		}
	}
}

// diffScrollGuardActive suppresses center→panel sync for a short window
// after a programmatic scroll (oscillation guard, GWB-182).
func (s *Shell) diffScrollGuardActive() bool {
	return s.git != nil && timeSince(s.git.revealAt) < DiffRevealGuardWindow
}
