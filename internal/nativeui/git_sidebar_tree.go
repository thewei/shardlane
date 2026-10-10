package nativeui

/**
 * [INPUT]: Git metadata snapshot, existing Git mutations/confirmations and native Sidebar controls
 * [OUTPUT]: Collapsible Worktrees/Branches/Remotes/Tags/Stashes navigation in the Git left rail
 * [POS]: Git sidebar presentation only; all Git truth and operations stay in gitworkbench
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"github.com/egoist/mygo/ui"
	"strings"
)

func (s *Shell) gitSidebarHeader(c *ui.Context) {
	t := c.Theme()
	ui.Row(c).FillWidth().Gap(7).AlignItems(ui.Center).Children(func() {
		ui.Icon(c, iconBranch).FontSize(13).TextColor(t.TextMuted)
		ui.Column(c).Grow(1).MinWidth(0).Gap(0).Children(func() {
			ui.Text(c, s.gitRepoName()).FontSize(12).FontWeight(650).SingleLine()
			if branch := s.currentBranchName(); branch != "" {
				ui.Text(c, branch).FontSize(10).TextColor(t.TextMuted).SingleLine().Tooltip(branch)
			}
		})
		if ui.Button(c, "↻").FontSize(13).Tooltip("Refresh repository").Clicked() {
			s.refreshGitChanges()
		}
	})
}

func (s *Shell) gitRepositorySidebar(c *ui.Context) {
	t := c.Theme()
	ui.Column(c).Grow(1).MinHeight(0).Padding(7, 5, 0).Gap(5).Children(func() {
		s.gitNavigationRow(c, "Local Changes", !s.commitsMode, func() {
			s.commitsMode = false
			s.reviewLocalChanges()
		})
		s.gitNavigationRow(c, "All Commits", s.commitsMode, func() {
			s.showGitCommits()
		})
		ui.Row(c).FillWidth().Padding(4, 4, 2).Children(func() {
			filter := ui.TextInput(c, &s.gitRefsFilter).FillWidth().Height(28).
				Placeholder("Filter references").Label("Filter Git references")
			if filter.Changed() {
				// A new search starts expanded, without carrying stale
				// search-scoped folds into a different result set.
				for key := range s.gitTreeClosed {
					if strings.HasPrefix(key, "search/") {
						delete(s.gitTreeClosed, key)
					}
				}
			}
		})
		if s.git.root == "" {
			ui.Text(c, "No Git repository").Padding(10).FontSize(11).TextColor(t.TextMuted)
			return
		}
		ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
			ui.Column(c).FillWidth().Padding(0, 2, 12).Gap(2).Children(func() {
				s.gitSidebarSection(c, "Worktrees", len(s.git.worktrees), &s.worktreesMenuOpen, s.worktreesMenuPanel, func() {
					shown := 0
					for _, wt := range s.git.worktrees {
						label := wt.Branch
						if label == "" {
							label = "(detached)"
						}
						if wt.Bare {
							label = "(bare)"
						}
						if !gitRefMatches(s.gitRefsFilter, label, wt.Path) {
							continue
						}
						shown++
						worktree := wt
						s.gitTreeRow(c, label, wt.Path, 1, false, false, false, func() {
							if !worktree.Bare {
								s.openExternalPath(worktree.Path)
							}
						}, func(m *ui.Menu) {
							if m.Item("Copy path").Chosen() {
								s.copyToClipboard(worktree.Path)
							}
							if !worktree.Bare && m.Item("Open in Finder").Chosen() {
								s.openExternalPath(worktree.Path)
							}
						})
					}
					s.gitEmptySection(c, shown)
				})
				s.gitSidebarSection(c, "Branches", len(s.git.branches), &s.branchMenuOpen, s.branchMenuPanel, func() {
					refs := make([]string, 0, len(s.git.branches))
					for _, b := range s.git.branches {
						refs = append(refs, b.Name)
					}
					s.gitRenderRefTree(c, "local", gitRefTree(refs, s.gitRefsFilter), 1, func(full string) {
						for _, branch := range s.git.branches {
							if branch.Name == full && !branch.Current {
								s.confirmSwitchBranch(full)
								return
							}
						}
					}, func(full string, m *ui.Menu) {
						var current bool
						for _, branch := range s.git.branches {
							if branch.Name == full {
								current = branch.Current
								break
							}
						}
						if !current {
							if m.Item("Switch to branch").Chosen() {
								s.confirmSwitchBranch(full)
							}
							if m.Item("Merge into current…").Chosen() {
								s.openMergeDialog(full)
							}
							if m.Item("Delete branch…").Chosen() {
								s.deleteBranch(full)
							}
						}
						if m.Item("Copy branch name").Chosen() {
							s.copyToClipboard(full)
						}
					})
				})
				s.gitSidebarSection(c, "Remotes", len(s.git.remoteBranches), nil, nil, func() {
					refs := make([]string, 0, len(s.git.remoteBranches))
					for _, b := range s.git.remoteBranches {
						refs = append(refs, b.Remote+"/"+b.Name)
					}
					s.gitRenderRefTree(c, "remote", gitRefTree(refs, s.gitRefsFilter), 1, func(full string) {
						s.copyToClipboard(full)
						s.pendingToast = "Copied remote-tracking ref"
					}, func(full string, m *ui.Menu) {
						if m.Item("Copy remote-tracking ref").Chosen() {
							s.copyToClipboard(full)
						}
					})
				})
				s.gitSidebarSection(c, "Tags", len(s.git.tags), &s.tagsMenuOpen, s.tagsMenuPanel, func() {
					refs := make([]string, 0, len(s.git.tags))
					for _, tag := range s.git.tags {
						refs = append(refs, tag.Name)
					}
					s.gitRenderRefTree(c, "tag", gitRefTree(refs, s.gitRefsFilter), 1, func(full string) {
						s.copyToClipboard(full)
					}, func(full string, m *ui.Menu) {
						if m.Item("Copy tag name").Chosen() {
							s.copyToClipboard(full)
						}
						if m.Item("Delete tag…").Chosen() {
							s.deleteTagConfirm(full)
						}
					})
				})
				s.gitSidebarSection(c, "Stashes", len(s.git.stashes), &s.stashesMenuOpen, s.stashesMenuPanel, func() {
					shown := 0
					for _, stash := range s.git.stashes {
						if !gitRefMatches(s.gitRefsFilter, stash.Ref, stash.Subject) {
							continue
						}
						shown++
						st := stash
						s.gitTreeRow(c, st.Ref+"  "+st.Subject, st.Subject, 1, false, false, false, func() {
							s.copyToClipboard(st.Ref)
						}, func(m *ui.Menu) {
							if m.Item("Pop stash").Chosen() {
								s.popStash(st.Ref)
							}
							if m.Item("Drop stash…").Chosen() {
								s.dropStash(st.Ref)
							}
							if m.Item("Copy stash ref").Chosen() {
								s.copyToClipboard(st.Ref)
							}
						})
					}
					s.gitEmptySection(c, shown)
				})
			})
		})
	})
}

func (s *Shell) gitNavigationRow(c *ui.Context, title string, active bool, action func()) {
	t := c.Theme()
	row := ui.ButtonBase(c.Key("git-nav:"+title)).FillWidth().Height(28).Padding(0, 8).Gap(8).Radius(5).
		Label(title).FocusRing(false).AlignItems(ui.Center)
	if active {
		row.Background(t.Accent.Alpha(.15)).TextColor(t.Accent)
	} else if row.Hovered() {
		row.Background(t.SurfaceHover)
	}
	row.Children(func() {
		svg := iconHistory
		if title == "Local Changes" {
			svg = iconFolder
		}
		ui.Icon(c, svg).FontSize(13)
		ui.Text(c, title).FontSize(12).FontWeight(600)
	})
	if row.Clicked() {
		action()
	}
}

func (s *Shell) gitSidebarSection(c *ui.Context, name string, count int, popup *bool, panel func(*ui.Context), children func()) {
	t := c.Theme()
	key := s.gitFoldKey("section/" + name)
	closed := s.gitTreeClosed[key]
	ui.Row(c.Key("git-section:" + name)).FillWidth().Height(26).Gap(3).AlignItems(ui.Center).Children(func() {
		toggle := ui.ButtonBase(c).Grow(1).MinWidth(0).Padding(0, 4).Gap(6).
			Label("Toggle " + name).FocusRing(false).AlignItems(ui.Center)
		if toggle.Hovered() {
			toggle.Background(t.SurfaceHover)
		}
		toggle.Children(func() {
			ch := iconChevronDown
			if closed {
				ch = iconChevron
			}
			ui.Icon(c, ch).FontSize(11).TextColor(t.TextMuted)
			ui.Text(c, name).FontSize(12).FontWeight(650).Grow(1)
			ui.Textf(c, "%d", count).FontSize(10).TextColor(t.TextMuted)
		})
		if toggle.Clicked() {
			s.gitTreeClosed[key] = !s.gitTreeClosed[key]
		}
		if popup != nil && panel != nil {
			button := ui.Button(c, "···").Label(name+" actions").FontSize(12).Tooltip(name+" actions").
				Padding(1, 4).Background(ui.Transparent)
			toggleMenu(popup, button)
			if *popup {
				ui.Popover(c, button, popup, func() { panel(c) })
			}
		}
	})
	if !closed {
		children()
	}
}
