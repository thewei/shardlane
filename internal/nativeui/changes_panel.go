package nativeui

import (
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// changesPanelState is the Changes tool's presentation state (filter,
// footer totals are derived).
type changesPanelState struct {
	filter      string
	filterFocus bool
}

// changesToolView renders the Right Panel Changes surface: a Godiff-style
// changed-file tree and footer — it never renders code diff lines
// (plan §10.3).
func (s *Shell) changesToolView(c *ui.Context) {
	t := c.Theme()
	pal := gdPaletteFor(t)

	if s.git == nil || s.git.root == "" {
		if s.git != nil && s.git.rootBusy {
			ui.Column(c).Grow(1).Center().Gap(6).Children(func() {
				ui.Spinner(c).Size(16, 16)
				ui.Text(c, "Resolving repository…").FontSize(Typography().Caption).TextColor(t.TextMuted)
			})
			return
		}
		s.gdEmptyPanel(c, pal, "No Git Repository", "The selected tab's directory is not inside a Git repository.", nil)
		return
	}

	snap := s.gitSnapshot()
	if snap == nil {
		ui.Column(c).Grow(1).Center().Gap(6).Children(func() {
			if s.git.refreshing {
				ui.Spinner(c).Size(16, 16)
			}
			ui.Text(c, "Reading changes…").FontSize(Typography().Caption).TextColor(t.TextMuted)
		})
		return
	}

	ui.Column(c).Grow(1).MinHeight(0).Children(func() {
		// Filter field, as macOS search fields (GWB-140).
		ui.Row(c).FillWidth().Padding(6, 10, 2).Children(func() {
			if gdSearchInput(c, &s.changes.filter, "Filter files", &s.changes.filterFocus, &s.git.gdTyping) {
				s.changesTree = nil // rebuilt below on this pass
				s.git.gdRowsDirty = true
			}
		})

		// Changed-file tree (GWB-141..145). List selection drives
		// dir-collapse/file-open through changesClick. The combined tree
		// chooses no rows beyond the opened file, so no multi-selection.
		tree := s.currentChangesTree(snap)
		rows := gitworkbench.Flatten(tree, s.git.collapsedDirs)
		list := s.gdChangedTreeList(c, pal, rows, &s.changesList, &s.changesListEl, &s.changesSelected,
			nil, gdViewCombined, s.changesClick)
		list.Children(func() {
			if len(rows) == 0 {
				msg := "No changed files"
				if strings.TrimSpace(s.changes.filter) != "" {
					msg = "No matching files"
				}
				ui.Text(c, msg).FontSize(12).TextColor(t.TextMuted).Padding(12)
			}
		})

		s.gdChangesFooter(c, pal, snap)
	})
}

// gdChangedTreeList renders one Godiff-style changed-file tree list over the
// given state (the Right Panel tool and the Diff-mode sidebar panes each
// hold their own list state over the same tree data). onClick receives the
// activated row. choice, when not nil, lets Cmd-click and Shift-click choose
// several rows, held by path in the caller's selection.
func (s *Shell) gdChangedTreeList(c *ui.Context, pal *gdPalette, rows []gitworkbench.FlatRow, state *ui.ListState, el **ui.Element, selected *int, choice *ui.Selection[string], pane gdViewSide, onClick func(gitworkbench.FlatRow)) *ui.Element {
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
		s.renderChangesRow(c, pal, rows[i], *el, pane, choice)
	}).Grow(1).MinHeight(0).Padding(3, 8).Gap(3)
	*el = list
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
			ui.Text(c, "Total:").FontSize(11).FontWeight(600).TextColor(t.TextMuted)
			ui.Text(c, "+"+gdThousands(snap.TotalAdditions)).Font(gdCodeFont()).FontSize(11).FontWeight(600).TextColor(pal.addText)
			ui.Text(c, "-"+gdThousands(snap.TotalDeletions)).Font(gdCodeFont()).FontSize(11).FontWeight(600).TextColor(pal.delText)
		})
		ui.Spacer(c)
		b := ui.Button(c, "").Children(func() {
			ui.Icon(c, iconCommit).FontSize(14)
			ui.Text(c, "Commit").SingleLine()
		})
		if b.Clicked() {
			s.showSurface(WorkspaceSurfaceCommit)
		}
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

// renderChangesRow draws one compact Godiff tree row:
// chevron · icon · name · +N -N · status letter (GWB-141..143).
// listEl is the owning list, for the focused-selection tint. choice is the
// pane's multi-selection, whose rows tint like an extended selection.
func (s *Shell) renderChangesRow(c *ui.Context, pal *gdPalette, row gitworkbench.FlatRow, listEl *ui.Element, pane gdViewSide, choice *ui.Selection[string]) {
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
	focused := listEl != nil && listEl.FocusWithin()

	r := ui.Row(c).Height(28).Padding(0, 8, 0, 6+float32(row.Depth)*TreeIndentWidth).Gap(5).Radius(6).MinWidth(0)
	textColor := t.Text
	muted := t.TextMuted
	switch {
	case chosen && focused:
		r.Background(t.Accent)
		textColor = t.AccentText
		muted = t.AccentText.Alpha(0.8)
	case chosen:
		r.Background(ui.RGBA(127, 127, 127, 0.2))
	case selected && focused:
		r.Background(t.Accent)
		textColor = t.AccentText
		muted = t.AccentText.Alpha(0.8)
	case selected:
		r.Background(ui.RGBA(127, 127, 127, 0.2))
	case r.Hovered():
		r.Background(ui.RGBA(127, 127, 127, 0.08))
	}
	r.TextColor(textColor).Children(func() {
		arrow := ui.Box(c).Size(14, 14).Center().Shrink(0)
		if node.Dir {
			arrow.Children(func() {
				ic := ui.Icon(c, iconChevronDown).FontSize(12).TextColor(muted)
				target := float32(0)
				if s.git.collapsedDirs[node.Path] {
					target = -90
				}
				ic.Rotate(ic.Animate("rot", target, 150*time.Millisecond))
			})
			ui.Icon(c, iconFolder).FontSize(14).TextColor(muted)
			ui.Text(c, node.Name).FontSize(13).SingleLine().Grow(1).Shrink(1).MinWidth(0).Tooltip(node.Path)
			return
		}
		cf := *node.File
		viewed := s.git.gdViewed[cf.Path] != "" && s.git.gdViewed[cf.Path] == cf.Fingerprint
		ui.Icon(c, iconFile).FontSize(14).TextColor(muted)
		// Full path on hover (F113): the 250px pane truncates names so
		// service_index.go and service_index_test.go read as the same row.
		name := ui.Text(c, node.Name).FontSize(13).SingleLine().Grow(1).Shrink(1).MinWidth(0).Tooltip(node.Path)
		if viewed && !(selected && focused) {
			name.TextColor(t.TextMuted)
		}
		// The per-file Stage/Unstage button shows while the row is hovered,
		// as Fork's +/- does; which one depends on the pane.
		if action, tip := s.gdRowAction(&cf, pane); action != nil {
			b := ui.ButtonBase(c).Size(16, 16).Radius(4).Center().Shrink(0).
				Label(tip).Tooltip(tip).TextColor(muted)
			if b.Hovered() {
				b.Background(pal.hover).TextColor(t.Text)
			}
			if !r.Hovered() && !b.Hovered() {
				b.Opacity(0)
			}
			b.Children(func() { ui.Text(c, tip).FontSize(12) })
			if b.Clicked() {
				action([]string{cf.Path})
			}
		}
		if gdCountable(&cf) && (cf.Additions > 0 || cf.Deletions > 0) {
			ui.Textf(c, "+%s -%s", gdCompact(cf.Additions), gdCompact(cf.Deletions)).
				Font(gdCodeFont()).FontSize(10).FontWeight(600).TextColor(muted).Shrink(0).
				Tooltip(gdLines(cf.Additions, "added") + ", " + gdLines(cf.Deletions, "removed"))
		}
		letter := gdStatusLetterColor(cf.Status, pal, t)
		switch {
		case selected && focused:
			letter = t.AccentText
		case viewed:
			letter = t.TextMuted
		}
		ui.Text(c, gdStatusLetter(&cf)).Font(gdCodeFont()).FontSize(11).FontWeight(700).TextColor(letter).
			Width(12).TextAlign(ui.Center).Shrink(0)
	})
	if !node.Dir && node.File != nil {
		cf := *node.File
		r.ContextMenu(func(m *ui.Menu) {
			s.gdRowContextMenu(m, &cf)
		})
	}
}

// gdRowAction returns the hover action of a pane's file row: Stage in the
// unstaged pane, Unstage in the staged pane; nil in the combined tree.
func (s *Shell) gdRowAction(cf *gitworkbench.ChangeFile, pane gdViewSide) (func([]string), string) {
	if s.gdReviewingCommit() {
		return nil, ""
	}
	switch pane {
	case gdViewUnstaged:
		return s.stageFiles, "+"
	case gdViewStaged:
		return s.unstageFiles, "\u2212"
	default:
		if cf.Unstaged {
			return s.stageFiles, "+"
		}
		if cf.Staged {
			return s.unstageFiles, "\u2212"
		}
		return nil, ""
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
