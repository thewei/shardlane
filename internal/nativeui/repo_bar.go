// [INPUT]: Git staged-file row selections and existing tag/stash/worktree operations.
// [OUTPUT]: Git file click handlers and group operation popovers.
// [POS]: Shared popover behavior, no repository chips or competing sidebar navigation.
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
package nativeui

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// Git tree reuse of the existing branch/tag/stash/worktree operation popovers.
// The former cramped repository chips no longer render.

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
		ui.Text(c, "Tags").FontSize(Typography().Caption).FontWeight(WeightSemibold).TextColor(t.TextMuted)
		ui.Row(c).FillWidth().Gap(sp.XS).AlignItems(ui.Center).Children(func() {
			ui.TextInput(c, &s.tagDraft).Grow(1).Placeholder("New tag name").Label("New tag name")
			if compactPrimaryButton(c, "Create").Clicked() {
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
				if compactButton(c, "Delete").Clicked() {
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
		ui.Text(c, "Stashes").FontSize(Typography().Caption).FontWeight(WeightSemibold).TextColor(t.TextMuted)
		ui.Row(c).FillWidth().Padding(sp.XS, sp.S).Children(func() {
			b := compactButton(c, "Stash All Changes")
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
				if compactButton(c, "Pop").Clicked() {
					s.stashesMenuOpen = false
					s.popStash(st.Ref)
				}
				if compactButton(c, "Drop").Clicked() {
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
		ui.Text(c, "Worktrees").FontSize(Typography().Caption).FontWeight(WeightSemibold).TextColor(t.TextMuted)
		ui.Row(c).FillWidth().Padding(sp.XS, sp.S).Children(func() {
			b := compactButton(c, "Add Worktree…")
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
					ui.Text(c, w.Path).Font(gdCodeFont()).FontSize(Typography().Micro).TextColor(t.TextMuted).SingleLine()
				})
				if !w.Bare {
					if compactButton(c, "Open").Clicked() {
						s.worktreesMenuOpen = false
						s.openExternalPath(w.Path)
					}
				}
				if compactButton(c, "Copy Path").Clicked() {
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
