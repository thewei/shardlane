package nativeui

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
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
		MaxWidth(120).Tooltip(branch) // preserve full identity without crushing the repository toolbar
	toggleMenu(&s.branchMenuOpen, anchor)
	if s.branchMenuOpen {
		ui.Popover(c, anchor, &s.branchMenuOpen, func() {
			s.branchMenuPanel(c)
		})
	}
}

// matchingLocalBranches keeps the current branch visible first when
// it matches the query. The filter is local presentation only.
func matchingLocalBranches(branches []gitworkbench.Branch, filter string) []gitworkbench.Branch {
	query := strings.ToLower(strings.TrimSpace(filter))
	result := make([]gitworkbench.Branch, 0, len(branches))
	for _, branch := range branches {
		if branch.Current && strings.Contains(strings.ToLower(branch.Name), query) {
			result = append(result, branch)
		}
	}
	for _, branch := range branches {
		if !branch.Current && strings.Contains(strings.ToLower(branch.Name), query) {
			result = append(result, branch)
		}
	}
	return result
}

// branchMenuPanel offers searchable, bounded local branch navigation.
// Switching stays visible; merge/delete are in each branch's More menu.
func (s *Shell) branchMenuPanel(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	// Only the background here: ui.Popover already draws the rounded,
	// bordered panel, and nesting a second rounded container inside it
	// read as a double frame (2026-10-06 annotation A1).
	ui.Column(c).Width(340).Padding(sp.M).Gap(sp.S).
		Background(designTokens(t.Dark).Panel).Children(func() {
		matches := matchingLocalBranches(s.git.branches, s.branchFilter)
		ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
			ui.Text(c, "Branches").FontSize(Typography().Section).FontWeight(650).Grow(1)
			ui.Textf(c, "%d local", len(s.git.branches)).FontSize(Typography().Caption).TextColor(t.TextMuted)
		})
		ui.TextInput(c, &s.branchFilter).FillWidth().Height(30).
			Placeholder("Filter local branches").Label("Filter branches")
		ui.Scroll(c).FillWidth().Height(240).Children(func() {
			ui.Column(c).FillWidth().Gap(sp.XXS).Children(func() {
				if s.git.branchesLoading {
					ui.Row(c).FillWidth().Padding(sp.XS, 0).Gap(sp.XS).AlignItems(ui.Center).Children(func() {
						ui.Spinner(c).Size(12, 12)
						ui.Text(c, "Loading branches…").FontSize(Typography().Caption).TextColor(t.TextMuted)
					})
					return
				}
				if len(matches) == 0 {
					label := "No local branches"
					if strings.TrimSpace(s.branchFilter) != "" {
						label = "No matching branches"
					}
					ui.Text(c, label).FontSize(Typography().Caption).TextColor(t.TextMuted).Padding(sp.S)
				}
				for _, branch := range matches {
					b := branch
					current := b.Current
					ui.Row(c).FillWidth().Padding(sp.XS, sp.S).Radius(Radius().Control).
						Background(branchRowBackground(c, current)).Children(func() {
						label := b.Name
						if current {
							label = "✓ " + label
						}
						ui.Text(c, label).FontSize(Typography().BodySmall).Grow(1).MinWidth(0).SingleLine().Tooltip(b.Name)
						if !current {
							if ui.Button(c, "Switch").FontSize(Typography().Micro).Clicked() {
								s.branchMenuOpen = false
								s.confirmSwitchBranch(b.Name)
							}
							more := ui.Button(c, "More…").FontSize(Typography().Micro)
							more.Menu(func(m *ui.Menu) {
								if m.Item("Merge into current branch…").Chosen() {
									s.branchMenuOpen = false
									s.openMergeDialog(b.Name)
								}
								if m.Item("Delete branch…").Chosen() {
									s.branchMenuOpen = false
									s.deleteBranch(b.Name)
								}
							})
						}
					})
				}
			})
		})
		ui.Divider(c)
		ui.Row(c).FillWidth().Gap(sp.XS).AlignItems(ui.Center).Children(func() {
			ui.TextInput(c, &s.branchDraft).Grow(1).MinWidth(0).
				Placeholder("New branch name").Label("New branch name")
			create := ui.Button(c, "Create branch…").FontSize(Typography().Caption)
			create.Disabled(strings.TrimSpace(s.branchDraft) == "" || s.git.branchBusy)
			if create.Clicked() {
				draft := s.branchDraft
				s.branchMenuOpen = false
				s.confirmCreateBranch(draft)
			}
		})
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
	s.openConfirm("git-switch-branch", name, "Switch branch with local changes?",
		"Switching may carry your uncommitted changes to "+name+
			". Git will refuse if they conflict; running processes keep running.")
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
