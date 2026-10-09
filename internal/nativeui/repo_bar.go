package nativeui

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// The diff-mode repo bar (the sidebar's top row): Branches, Tags, Stashes
// and Worktrees, each a chip opening its menu of operations.

// changesClickSide opens a tree row from one of the sidebar's split panes:
// files open the center review on that pane's side of the change.
func (s *Shell) changesClickSide(row gitworkbench.FlatRow, view gdViewSide) {
	if row.Node.Dir {
		path := row.Node.Path
		s.git.collapsedDirs[path] = !s.git.collapsedDirs[path]
		return
	}
	s.openChangesSide(row.Node.Path, view)
	s.revealDiffFile(row.Node.Path)
}

// openChangesSide opens the center review for path with the given view side.
func (s *Shell) openChangesSide(path string, view gdViewSide) {
	s.openChanges(path)
	if f, _ := s.gdFileForPath(path); f != nil {
		f.view = view
		s.git.gdRowsDirty = true
	}
}

// repoBar is the diff-mode top bar: the four repo menus. Every chip is
// width-capped so the 250px sidebar never clips the trailing chip (the
// Worktrees pill used to overflow past the panel edge).
func (s *Shell) repoBar(c *ui.Context) {
	ui.Row(c).FillWidth().Gap(4).AlignItems(ui.Center).Children(func() {
		s.branchChip(c)
		s.repoChip(c, "Tags", &s.tagsMenuOpen, len(s.git.tags), 46, s.tagsMenuPanel)
		s.repoChip(c, "Stashes", &s.stashesMenuOpen, len(s.git.stashes), 58, s.stashesMenuPanel)
		s.repoChip(c, "Worktrees", &s.worktreesMenuOpen, len(s.git.worktrees), 62, s.worktreesMenuPanel)
	})
}

// repoChip is a neutral pill opening one repo menu.
func (s *Shell) repoChip(c *ui.Context, label string, open *bool, count int, maxWidth float32, panel func(*ui.Context)) {
	t := c.Theme()
	text := label
	if count > 0 {
		text = fmt.Sprintf("%s %d", label, count)
	}
	anchor := ui.Button(c, text).FontSize(11).TextColor(t.TextMuted).MaxWidth(maxWidth).
		// The chip truncates before its count at 250px (F4/F114); the
		// tooltip keeps "Tags 7" recoverable.
		Tooltip(text)
	anchor.Background(ui.RGBA(127, 127, 127, 0.08)).Radius(Radius().Pill).Padding(2, 7).
		Border(1, ui.RGBA(127, 127, 127, 0.12))
	toggleMenu(open, anchor)
	if *open {
		// The backdrop closes it; the content must not touch *open while
		// building, or the menu would close the frame it opens.
		ui.Popover(c, anchor, open, func() {
			panel(c)
		})
	}
}

// repoMenuPanel is the shared popover body of the repo menus. It carries
// only the background: ui.Popover already draws the rounded, bordered
// panel, and nesting a second rounded container inside it read as a
// double frame (2026-10-06 annotation A1).
func (s *Shell) repoMenuPanel(c *ui.Context, build func()) {
	t := c.Theme()
	sp := Spacing()
	ui.Column(c).Width(280).MaxHeight(380).Padding(sp.S).Gap(sp.XS).
		Background(designTokens(t.Dark).Panel).Children(build)
}

// tagsMenuPanel lists the tags with create and delete.
func (s *Shell) tagsMenuPanel(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	s.repoMenuPanel(c, func() {
		ui.Text(c, "Tags").FontSize(Typography().Caption).FontWeight(650).TextColor(t.TextMuted)
		ui.Row(c).FillWidth().Gap(sp.XS).AlignItems(ui.Center).Children(func() {
			ui.TextInput(c, &s.tagDraft).Grow(1).Placeholder("New tag name").Label("New tag name")
			if ui.PrimaryButton(c, "Create").FontSize(Typography().Micro).Clicked() {
				draft := s.tagDraft
				s.tagsMenuOpen = false
				s.createTag(draft)
			}
		})
		ui.Divider(c)
		if len(s.git.tags) == 0 {
			ui.Text(c, "No tags").FontSize(Typography().Caption).TextColor(t.TextMuted)
			return
		}
		for _, tag := range s.git.tags {
			tg := tag
			ui.Row(c).FillWidth().Padding(sp.XS, sp.S).Gap(sp.XS).AlignItems(ui.Center).
				Radius(Radius().Control).Children(func() {
				ui.Icon(c, iconTag).FontSize(13).TextColor(t.TextMuted)
				ui.Text(c, tg.Name).Font(gdCodeFont()).FontSize(Typography().BodySmall).Grow(1).SingleLine()
				if ui.Button(c, "Delete").FontSize(Typography().Micro).Clicked() {
					s.tagsMenuOpen = false
					s.deleteTagConfirm(tg.Name)
				}
			})
		}
		ui.Box(c).Height(4)
	})
}

// stashesMenuPanel lists the stashes with stash/pop/drop.
func (s *Shell) stashesMenuPanel(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	s.repoMenuPanel(c, func() {
		ui.Text(c, "Stashes").FontSize(Typography().Caption).FontWeight(650).TextColor(t.TextMuted)
		ui.Row(c).FillWidth().Padding(sp.XS, sp.S).Children(func() {
			b := ui.Button(c, "Stash All Changes").FontSize(Typography().Micro)
			if s.git.opBusy || !s.dirtyWorktree() {
				b.Disabled(true)
			}
			if b.Clicked() {
				s.stashesMenuOpen = false
				s.stashChanges()
			}
		})
		if len(s.git.stashes) == 0 {
			ui.Text(c, "No stashes").FontSize(Typography().Caption).TextColor(t.TextMuted)
			return
		}
		for _, stash := range s.git.stashes {
			st := stash
			ui.Row(c).FillWidth().Padding(sp.XS, sp.S).Gap(sp.XS).AlignItems(ui.Center).Children(func() {
				ui.Text(c, st.Ref).Font(gdCodeFont()).FontSize(Typography().Micro).TextColor(t.TextMuted).Shrink(0)
				ui.Text(c, st.Subject).FontSize(Typography().BodySmall).Grow(1).MinWidth(0).SingleLine()
				if ui.Button(c, "Pop").FontSize(Typography().Micro).Clicked() {
					s.stashesMenuOpen = false
					s.popStash(st.Ref)
				}
				if ui.Button(c, "Drop").FontSize(Typography().Micro).Clicked() {
					s.stashesMenuOpen = false
					s.dropStash(st.Ref)
				}
			})
		}
		ui.Box(c).Height(4)
	})
}

// worktreesMenuPanel lists the work trees with add/open/copy.
func (s *Shell) worktreesMenuPanel(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	s.repoMenuPanel(c, func() {
		ui.Text(c, "Worktrees").FontSize(Typography().Caption).FontWeight(650).TextColor(t.TextMuted)
		ui.Row(c).FillWidth().Padding(sp.XS, sp.S).Children(func() {
			b := ui.Button(c, "Add Worktree…").FontSize(Typography().Micro)
			if s.git.opBusy {
				b.Disabled(true)
			}
			if b.Clicked() {
				s.worktreesMenuOpen = false
				s.openTextDialog("git-add-worktree", "", "Add Worktree",
					"Absolute path (a branch named after the folder is created from HEAD)", "")
			}
		})
		for _, wt := range s.git.worktrees {
			w := wt
			label := w.Branch
			switch {
			case w.Bare:
				label = "(bare)"
			case w.Detached:
				label = "(detached)"
			}
			ui.Row(c).FillWidth().Padding(sp.XS, sp.S).Gap(sp.XS).AlignItems(ui.Center).Children(func() {
				ui.Column(c).Grow(1).MinWidth(0).Gap(0).Children(func() {
					ui.Text(c, label).FontSize(Typography().BodySmall).SingleLine()
					ui.Text(c, w.Path).Font(gdCodeFont()).FontSize(9).TextColor(t.TextMuted).SingleLine()
				})
				if !w.Bare {
					if ui.Button(c, "Open").FontSize(Typography().Micro).Clicked() {
						s.worktreesMenuOpen = false
						s.openExternalPath(w.Path)
					}
				}
				if ui.Button(c, "Copy Path").FontSize(Typography().Micro).Clicked() {
					s.copyToClipboard(w.Path)
				}
			})
		}
		ui.Box(c).Height(4)
	})
}

// toggleMenu flips *open when e was clicked since the last frame. The
// click is consumed by the first pass that asks for it, so later passes
// of the same frame cannot flip it twice. Clicks() cannot be used here:
// the engine resets every element's click count at the end of each pass,
// so a count kept across frames would swallow every click after the
// first (the 2026-10-06 staged-files toggle could collapse but never
// re-expand).
func toggleMenu(open *bool, e ui.Element) {
	if e.Clicked() {
		*open = !*open
	}
}
