package nativeui

import (
	"github.com/egoist/mygo/ui"
)

// branchChip renders the interactive branch identity chip (plan §23).
// Without a bound repository it degrades to a muted non-chip label.
func (s *Shell) branchChip(c *ui.Context) {
	t := c.Theme()
	branch := s.currentBranchName()
	if s.git == nil || s.git.root == "" {
		return
	}
	if branch == "" {
		if s.git.rootBusy {
			ui.Text(c, "resolving…").FontSize(Typography().Micro).TextColor(t.TextMuted)
		}
		return
	}

	anchor := ui.Button(c, branch).FontSize(Typography().Micro)
	anchor.Background(t.Accent.Alpha(0.14)).Radius(Radius().Pill).Padding(2, 8).TextColor(t.Accent).
		MaxWidth(60) // a long branch name truncates instead of pushing the bar out
	toggleMenu(&s.branchMenuOpen, anchor)
	if s.branchMenuOpen {
		ui.Popover(c, anchor, &s.branchMenuOpen, func() {
			s.branchMenuPanel(c)
		})
	}
}

// branchMenuPanel lists local branches with switch/create actions
// (GWB-230..243 core: list/switch/create only — no delete/rename/remote).
func (s *Shell) branchMenuPanel(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	// Only the background here: ui.Popover already draws the rounded,
	// bordered panel, and nesting a second rounded container inside it
	// read as a double frame (2026-10-06 annotation A1).
	ui.Column(c).Width(240).MaxHeight(360).Padding(sp.S).Gap(sp.XS).
		Background(designTokens(t.Dark).Panel).Children(func() {
		ui.Text(c, "Branches").FontSize(Typography().Caption).FontWeight(650).TextColor(t.TextMuted)

		// New branch entry.
		ui.Row(c).FillWidth().Gap(sp.XS).AlignItems(ui.Center).Children(func() {
			ui.TextInput(c, &s.branchDraft).Grow(1).Placeholder("New branch name").Label("New branch name")
			if ui.PrimaryButton(c, "Create").FontSize(Typography().Micro).Clicked() {
				draft := s.branchDraft
				s.branchMenuOpen = false
				s.confirmCreateBranch(draft)
			}
		})

		ui.Divider(c)

		if s.git.branchesLoading {
			ui.Row(c).FillWidth().Padding(sp.XS, 0).Gap(sp.XS).AlignItems(ui.Center).Children(func() {
				ui.Spinner(c).Size(12, 12)
				ui.Text(c, "Loading branches…").FontSize(Typography().Caption).TextColor(t.TextMuted)
			})
			return
		}
		if len(s.git.branches) == 0 {
			ui.Text(c, "No local branches").FontSize(Typography().Caption).TextColor(t.TextMuted)
			return
		}
		for _, branch := range s.git.branches {
			b := branch
			current := b.Current
			ui.Row(c).FillWidth().Padding(sp.XS, sp.S).Radius(Radius().Control).
				Background(branchRowBackground(c, current)).Children(func() {
				label := b.Name
				if current {
					label = "✓ " + label
				}
				ui.Text(c, label).FontSize(Typography().BodySmall).Grow(1).SingleLine()
				if !current {
					if ui.Button(c, "Switch").FontSize(Typography().Micro).Clicked() {
						s.branchMenuOpen = false
						s.confirmSwitchBranch(b.Name)
					}
					if ui.Button(c, "Delete").FontSize(Typography().Micro).Clicked() {
						s.branchMenuOpen = false
						s.deleteBranch(b.Name)
					}
				}
			})
		}
		ui.Box(c).Height(4)
	})
}

func branchRowBackground(c *ui.Context, current bool) ui.Color {
	if current {
		return c.Theme().Accent.Alpha(0.12)
	}
	return ui.Transparent
}

// confirmSwitchBranch asks for explicit confirmation when the worktree is
// dirty (plan §23.2 preflight), then switches.
func (s *Shell) confirmSwitchBranch(name string) {
	if !s.dirtyWorktree() {
		s.switchBranch(name)
		return
	}
	s.confirmOpen = true
	s.confirmKind = "branch-switch"
	s.confirmTarget = name
	s.confirmTitle = "Switch branch with local changes?"
	s.confirmMessage = "Switching may carry your uncommitted changes to " + name +
		". Git will refuse if they conflict; running processes keep running."
}

// confirmCreateBranch validates the name then creates.
func (s *Shell) confirmCreateBranch(name string) {
	s.createBranch(name)
}

// branchStatusTone exposes the busy state to the header.
func (s *Shell) branchStatusTone() StatusTone {
	if s.git != nil && s.git.branchBusy {
		return ToneWorking
	}
	return ToneNeutral
}
