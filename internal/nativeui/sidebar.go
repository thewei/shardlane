package nativeui

/**
 * [INPUT]: 依赖 Shell 当前 Router/Workspace/Agent 投影、共享 agentCardRow 和 native Quick Panel
 * [OUTPUT]: 提供 Sidebar 的 Workspace 树、Git 引用导航、History/Settings 导航、内页底部返回控件（取代 Workspace 切换页脚）与 Agent 行点击/右键动作
 * [POS]: 唯一左栏宿主，Git 引用树由 git_sidebar_tree 负责，Agent 详情复用 Quick Panel；内页返回统一落在页脚
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

func (s *Shell) sidebar(c *ui.Context) {
	t := c.Theme()
	tokens := s.tokens(t.Dark)
	// No fill of its own: the sidebar column (sidebarSlide) is the vibrancy
	// material or its opaque fallback, one continuous surface with the band
	// beside the traffic lights above this body. The header's trailing
	// cluster owns the view switch (2026-10-07).
	ui.Column(c).Width(sidebarWidth).Grow(1).MinHeight(0).Shrink(0).Children(func() {
		if s.sidebarInDiffMode() {
			// Git references are full-height navigation, not compressed toolbar chips.
			ui.Row(c).FillWidth().Padding(8, 9, 6).Children(func() {
				s.gitSidebarHeader(c)
			})
			ui.Divider(c)
		}
		// No divider on the plain workspace sidebar: with no row above it
		// the line hung orphaned between the title bar and the tree
		// (2026-10-07 user report) — the sidebar and the header are one
		// continuous gradient surface there.

		// The sidebar body is route-scoped: Diff/Commit use the Git reference
		// tree, inner pages use their own navigation, and the default remains
		// the canonical Herdr runtime navigator.
		if s.sidebarInDiffMode() {
			s.gitRepositorySidebar(c)
		} else if s.router.Path() == routeFiles {
			s.syncRightPanelRoot()
			s.filesToolView(c)
		} else if s.sidebarInnerPage() {
			s.sidebarInnerNav(c)
		} else if ui.Sidebar(c, &s.selected, func() {
			// The Agents/Pin/Workspace sections share one header builder so
			// every section title reads at the same compact scale
			// (2026-10-06: Agents previously used mygo's larger
			// SidebarSection title and disagreed with Workspace below it).
			sidebarSection(c, "Agents", &s.agentsOpen, true, nil, func() {
				s.agentItems(c)
			})
			if pins := s.pinnedPaneIDs(); len(pins) > 0 {
				sidebarSection(c, "Pin", &s.pinsOpen, false, nil, func() {
					s.pinnedItemsView(c)
				})
			}
			s.workspaceSection(c)
			// The widget's own Surface fill is dropped (2026-10-06): the
			// Agents/Workspace blocks sit on the window gradient like the
			// rest of the chrome.
		}).Background(ui.Transparent).Grow(1).Label("Shardlane sidebar").Changed() {
			s.handleSidebarSelection(s.selected)
		}

		// The sidebar foot. Inner pages put their one way back here — the same
		// spot on every page — in place of the Workspace switch. On the
		// workspace sidebar the Workspace is the bound Herdr instance, so
		// switching belongs in the footer rather than nesting Projects under
		// a Workspace tree.
		ui.Row(c).Height(42).Padding(5, 8).Gap(6).AlignItems(ui.Center).
			BorderWidth(1, 0, 0, 0).BorderColor(tokens.BorderSubtle).Children(func() {
			if s.sidebarInnerPage() {
				s.sidebarBackRow(c)
				return
			}
			s.sessionSelector(c)
			if iconButton(c, iconSettings, "Settings").Clicked() {
				s.router.Push(routeSettings)
			}
		})
	})
}

// sidebarInDiffMode reports whether the sidebar shows the Godiff Files
// navigator: on /workspace with the Diff or Commit surface visible.
func (s *Shell) sidebarInDiffMode() bool {
	if s.git == nil || s.router.Path() != routeWorkspace {
		return false
	}
	switch s.surface.current() {
	case WorkspaceSurfaceDiff, WorkspaceSurfaceCommit:
		return true
	}
	return false
}

// sidebarInnerPage reports whether the current route is an inner page:
// Settings, History (list, detail, by-project) or Files. Inner pages swap
// the sidebar body for their own navigation and pin the back control to the
// sidebar foot.
func (s *Shell) sidebarInnerPage() bool {
	path := s.router.Path()
	return isHistoryRoute(path) || path == routeFiles || strings.HasPrefix(path, "/settings/")
}

// sidebarBackRow names the destination precisely. A session detail returns
// to the Sessions list; every other inner page returns to the Workspace's
// Terminal, not the last visible Diff/Chat/Commit surface. The global router
// history shortcut remains independent of this explicit product navigation.
func (s *Shell) sidebarBackRow(c *ui.Context) {
	if strings.HasPrefix(s.router.Path(), routeHistory+"/") {
		if navButton(c, iconArrowLeft, "Back to Sessions", "", false).Clicked() {
			s.showHistoryIndex()
		}
		return
	}
	if navButton(c, iconArrowLeft, "Back to Terminal", "", false).Clicked() {
		s.showViewTerminal()
	}
}

// sidebarInnerNav is the inner-page sidebar body: the page's navigation and
// filters live in the app sidebar while the route owns the content area.
func (s *Shell) sidebarInnerNav(c *ui.Context) {
	ui.Column(c).Grow(1).MinHeight(0).Padding(14, 9, 9).Gap(3).Children(func() {
		switch path := s.router.Path(); {
		case strings.HasPrefix(path, "/settings/"):
			s.settingsNav(c)
		default:
			s.historyNav(c)
		}
	})
}

// sidebarFilesView renders staged/unstaged file lists in the UPPER CENTER
// Git pane. The left sidebar manages repository references independently.
func (s *Shell) sidebarFilesView(c *ui.Context) {
	t := c.Theme()
	pal := gdPaletteFor(t)

	snap := s.gdActiveSnapshot()
	ui.Column(c).Grow(1).MinHeight(0).Children(func() {
		ui.Row(c).FillWidth().Padding(6, 12, 2).AlignItems(ui.Center).Children(func() {
			ui.Text(c, "Local Changes").FontSize(Typography().Body).FontWeight(WeightSemibold)
			if snap != nil {
				ui.Textf(c, "  %d files", len(snap.Files)).FontSize(Typography().BodySmall).TextColor(t.TextMuted)
			}
		})

		ui.Row(c).FillWidth().Padding(8, 10, 10).Children(func() {
			if gdSearchInput(c, &s.changes.filter, "Filter files", &s.changes.filterFocus, &s.git.gdTyping) {
				s.changesTree = nil
				s.git.gdRowsDirty = true
			}
		})

		if s.git.root == "" {
			ui.Column(c).Grow(1).Center().Padding(16).Children(func() {
				ui.Text(c, "No Git repository").FontSize(Typography().Body).TextColor(t.TextMuted)
			})
			return
		}
		if snap == nil {
			ui.Column(c).Grow(1).Center().Padding(16).Gap(6).Children(func() {
				if s.git.refreshing {
					ui.Spinner(c).Size(16, 16)
				}
				ui.Text(c, "Reading changes…").FontSize(Typography().Body).TextColor(t.TextMuted)
			})
			return
		}

		unstagedFiles, stagedFiles := s.splitSections(snap)
		unstagedRows := s.sectionRows(unstagedFiles)
		stagedRows := s.sectionRows(stagedFiles)
		// Files leave a pane as they are staged, unstaged or filtered away;
		// a choice keyed by a path the pane no longer shows must not quietly
		// feed the pane's action.
		pruneChoice(&s.unstagedChoice, unstagedFiles)
		pruneChoice(&s.stagedChoice, stagedFiles)

		ui.Column(c).Grow(1).MinHeight(0).Children(func() {
			// The Unstaged pane (untracked files included) takes the space
			// above the divider. The pane action targets the Cmd/Shift
			// selection when there is one, else the whole pane.
			s.gdSectionHeader(c, pal, "Unstaged Files", len(unstagedFiles), &s.unstagedOpen,
				"Stage", gdActionPaths(&s.unstagedChoice, unstagedFiles), s.stageFiles)
			if s.unstagedOpen && len(unstagedRows) > 0 {
				list := s.gdChangedTreeList(c, pal, unstagedRows, &s.unstagedList, &s.unstagedSelected,
					&s.unstagedChoice, gdViewUnstaged, func(row gitworkbench.FlatRow) { s.changesClickSide(row, gdViewUnstaged) })
				list.Children(func() {
					if len(unstagedRows) == 0 {
						ui.Text(c, "No unstaged files").FontSize(Typography().Body).TextColor(t.TextMuted).Padding(12)
					}
				})
			}

			if s.unstagedOpen && s.stagedOpen && len(stagedFiles) > 0 && len(unstagedFiles) > 0 {
				s.gdPaneDivider(c)
			}

			// The Staged pane sits below the divider at the dragged height.
			s.gdSectionHeader(c, pal, "Staged Files", len(stagedFiles), &s.stagedOpen,
				"Unstage", gdActionPaths(&s.stagedChoice, stagedFiles), s.unstageFiles)
			if s.stagedOpen && len(stagedRows) > 0 {
				list := s.gdChangedTreeList(c, pal, stagedRows, &s.stagedList, &s.stagedSelected,
					&s.stagedChoice, gdViewStaged, func(row gitworkbench.FlatRow) { s.changesClickSide(row, gdViewStaged) })
				list.Children(func() {
					if len(stagedRows) == 0 {
						ui.Text(c, "No staged files").FontSize(Typography().Body).TextColor(t.TextMuted).Padding(12)
					}
				})
				if s.unstagedOpen && len(unstagedFiles) > 0 {
					list.Height(clampF(s.stagedPaneHeight, 60, s.gitUpperHeight-138))
				} else {
					list.Grow(1).MinHeight(0)
				}
			}
		})

		// The godiff footer: totals and the Commit action.
		s.gdChangesFooter(c, pal, snap)
	})
}

// commitsPane is the All Commits pane: the history of HEAD, filtered by the
// search field, each row opening the commit's diff in the review.
func (s *Shell) commitsPane(c *ui.Context, pal *gdPalette) {
	t := c.Theme()

	var entries []gitworkbench.CommitInfo
	q := strings.ToLower(strings.TrimSpace(s.gitCommitFilter))
	for _, cm := range s.git.commits {
		if q == "" ||
			strings.Contains(strings.ToLower(cm.Subject), q) ||
			strings.Contains(strings.ToLower(cm.Author), q) ||
			strings.HasPrefix(cm.Hash, q) {
			entries = append(entries, cm)
		}
	}
	if s.gdReviewingCommit() {
		s.commitsSelected = -1
		for i, cm := range entries {
			if cm.Hash == s.git.commitHash {
				s.commitsSelected = i
			}
		}
	}

	graphs := gitHistoryGraph(s.git.commits)
	graphByHash := make(map[string]gitGraphRow, len(graphs))
	for i, cm := range s.git.commits {
		graphByHash[cm.Hash] = graphs[i]
	}
	focused := s.commitsList.FocusWithin(c)
	if s.git.commitsLoading && len(entries) == 0 {
		ui.Column(c).Grow(1).Center().Padding(16).Children(func() {
			ui.Spinner(c).Size(16, 16)
		})
		return
	}
	list := ui.List(c, &s.commitsList, len(entries), func(i int) {
		if i < 0 || i >= len(entries) {
			return
		}
		cm := entries[i]
		current := s.gdReviewingCommit() && s.git.commitHash == cm.Hash
		row := ui.Row(c).Gap(9).Padding(5, 9).Radius(5).AlignItems(ui.Center).MinWidth(0)
		muted, ref := t.TextMuted, pal.ref
		switch {
		case current && focused:
			row.Background(t.Accent).TextColor(t.AccentText)
			muted, ref = t.AccentText.Alpha(0.75), t.AccentText
		case current:
			row.Background(ui.RGBA(127, 127, 127, 0.2))
		case row.Hovered():
			row.Background(ui.RGBA(127, 127, 127, 0.08))
		}
		if row.Clicked() {
			s.reviewCommit(cm.Hash)
		}
		// Long commit subjects preserve the full metadata in the tooltip.
		row.Tooltip(cm.Subject + "\n" + cm.Author + " · " +
			cm.Time.Format("2006-01-02 15:04") + "\n" + cm.Hash)
		row.ContextMenu(func(m *ui.Menu) {
			if m.Item("Cherry-pick commit…").Chosen() {
				s.confirmCherryPickCommit(cm.Hash, cm.Subject)
			}
			if m.Item("Revert commit…").Chosen() {
				s.confirmRevertCommit(cm.Hash, cm.Subject)
			}
			if m.Item("Copy commit hash").Chosen() {
				s.copyToClipboard(cm.Hash)
			}
		})
		row.Children(func() {
			gitGraphGlyph(c, graphByHash[cm.Hash])
			// Real local/remote-tracking decorations follow cached ref
			// object hashes, never guessed from the commit subject.
			labels := 0
			for _, branch := range s.git.branches {
				if branch.HeadShort != "" && strings.HasPrefix(cm.Hash, branch.HeadShort) && labels < 2 {
					ui.Text(c, branch.Name).FontSize(Typography().Micro).FontWeight(WeightSemibold).
						Padding(2, 5).Radius(3).Background(t.Accent.Alpha(0.18)).
						TextColor(t.Accent).MaxWidth(140).SingleLine().Tooltip(branch.Name)
					labels++
				}
			}
			for _, branch := range s.git.remoteBranches {
				if branch.HeadShort != "" && strings.HasPrefix(cm.Hash, branch.HeadShort) && labels < 2 {
					name := branch.Remote + "/" + branch.Name
					ui.Text(c, name).FontSize(Typography().Micro).FontWeight(WeightSemibold).
						Padding(2, 5).Radius(3).Background(t.SurfaceHover).
						TextColor(ref).MaxWidth(140).SingleLine().Tooltip(name)
					labels++
				}
			}
			ui.Text(c, cm.Subject).FontSize(Typography().Body).Grow(1).MinWidth(0).SingleLine()
			ui.Text(c, cm.Author).FontSize(Typography().BodySmall).TextColor(muted).Width(90).SingleLine()
			ui.Text(c, cm.Short).Font(gdCodeFont()).FontSize(Typography().BodySmall).TextColor(ref).Width(64).SingleLine()
			ui.Text(c, gdRelativeTime(cm.Time)).FontSize(Typography().BodySmall).TextColor(muted).Width(62).SingleLine()
		})
	}).Grow(1).MinHeight(0).Padding(4, 8).Gap(4)
	s.commitsList.Key = func(i int) any {
		if i >= 0 && i < len(entries) {
			return entries[i].Hash
		}
		return nil
	}
	list.Children(func() {
		if len(entries) == 0 {
			ui.Text(c, "No matching commits").FontSize(Typography().Body).TextColor(t.TextMuted).Padding(12)
		}
	})
}

// gdRelativeTime writes how long ago t was: just now, 5m ago, 3d ago.
func gdRelativeTime(t time.Time) string {
	d := timeNow().Sub(t)
	switch {
	case d < time.Minute:
		return "just now"
	case d < time.Hour:
		return fmt.Sprintf("%dm ago", int(d/time.Minute))
	case d < 24*time.Hour:
		return fmt.Sprintf("%dh ago", int(d/time.Hour))
	case d < 30*24*time.Hour:
		return fmt.Sprintf("%dd ago", int(d/(24*time.Hour)))
	case d < 365*24*time.Hour:
		return fmt.Sprintf("%dmo ago", int(d/(30*24*time.Hour)))
	}
	return fmt.Sprintf("%dy ago", int(d/(365*24*time.Hour)))
}

// splitSections partitions the snapshot's files into the unstaged pane
// (worktree changes and untracked files) and the staged pane.
func (s *Shell) splitSections(snap *gitworkbench.ChangesSnapshot) (unstaged, staged []gitworkbench.ChangeFile) {
	for i := range snap.Files {
		cf := snap.Files[i]
		switch {
		case cf.Untracked:
			unstaged = append(unstaged, cf)
		default:
			if cf.Unstaged {
				unstaged = append(unstaged, cf)
			}
			if cf.Staged {
				staged = append(staged, cf)
			}
		}
	}
	return unstaged, staged
}

// sectionRows builds the collapsible tree rows of one pane, honoring the
// shared filter and directory-collapse state.
func (s *Shell) sectionRows(files []gitworkbench.ChangeFile) []gitworkbench.FlatRow {
	tree := gitworkbench.FilterTree(gitworkbench.BuildTree(files, true), s.changes.filter)
	return gitworkbench.Flatten(tree, s.git.collapsedDirs)
}

// panePaths lists a pane's file paths in snapshot order.
func panePaths(files []gitworkbench.ChangeFile) []string {
	paths := make([]string, 0, len(files))
	for _, cf := range files {
		paths = append(paths, cf.Path)
	}
	return paths
}

// pruneChoice drops chosen paths the pane no longer shows, so a staged,
// unstaged or filtered-away file cannot linger in its old pane's choice.
func pruneChoice(choice *ui.Selection[string], files []gitworkbench.ChangeFile) {
	if choice.Len() == 0 {
		return
	}
	shown := make(map[string]bool, len(files))
	for _, cf := range files {
		shown[cf.Path] = true
	}
	for path := range choice.All() {
		if !shown[path] {
			choice.Remove(path)
		}
	}
}

// gdActionPaths returns the paths a pane's Stage/Unstage action targets:
// the Cmd/Shift selection when there is one, else every file of the pane.
func gdActionPaths(choice *ui.Selection[string], files []gitworkbench.ChangeFile) []string {
	if choice.Len() == 0 {
		return panePaths(files)
	}
	paths := make([]string, 0, choice.Len())
	for _, cf := range files {
		if choice.Has(cf.Path) {
			paths = append(paths, cf.Path)
		}
	}
	return paths
}

// gdSectionHeader is one pane's title row with its Stage/Unstage action.
func (s *Shell) gdSectionHeader(c *ui.Context, pal *gdPalette, title string, count int, open *bool, actionLabel string, paths []string, action func([]string)) {
	t := c.Theme()
	ui.Row(c).FillWidth().Padding(4, 10, 2).Gap(6).AlignItems(ui.Center).Children(func() {
		// The title toggles the pane's collapse, chevron rotating as in
		// the file cards.
		toggle := ui.ButtonBase(c).Height(20).Padding(0, 4).Radius(5).Gap(4).
			Label("Toggle " + title).FocusRing(false).TextColor(t.TextMuted)
		if toggle.Hovered() {
			toggle.Background(pal.hover)
		}
		toggle.Children(func() {
			ic := ui.Icon(c, iconChevronDown).FontSize(11).TextColor(t.TextMuted)
			target := float32(0)
			if !*open {
				target = -90
			}
			ic.Rotate(ic.Animate("rot", target, 150*time.Millisecond))
		})
		toggleMenu(open, toggle)
		ui.Text(c, title).FontSize(Typography().BodySmall).FontWeight(WeightSemibold).LetterSpacing(0.4).TextColor(t.TextMuted)
		if count > 0 {
			ui.Text(c, itoa(count)).FontSize(Typography().BodySmall).FontWeight(WeightSemibold).TextColor(t.TextMuted)
		}
		ui.Spacer(c)
		if count == 0 || s.git.opBusy {
			return
		}
		b := ui.ButtonBase(c).Height(20).Padding(0, 7).Radius(5).Gap(4).
			Label(actionLabel).Tooltip(actionLabel + " (" + itoa(len(paths)) + " files)").TextColor(t.TextMuted)
		if b.Hovered() {
			b.Background(pal.hover).TextColor(t.Text)
		}
		b.Children(func() {
			ui.Text(c, actionLabel).FontSize(Typography().Micro).FontWeight(WeightSemibold)
		})
		if b.Clicked() {
			action(paths)
		}
	})
}

// gdPaneDivider is the draggable line between the two panes: dragging it
// hands the space to the pane above or below. Dragging down grows the
// unstaged pane above, so the staged pane below shrinks by the same delta.
func (s *Shell) gdPaneDivider(c *ui.Context) {
	pal := gdPaletteFor(c.Theme())
	divider := ui.Box(c).Height(7).Shrink(0).Cursor(ui.CursorResizeNS).Label("Resize panes").
		Children(func() {
			ui.Box(c).Height(1).Margin(3, 8).Background(pal.cardBorder)
		})
	if _, dy, held := divider.Dragged(); held {
		s.stagedPaneHeight = clampF(s.stagedPaneHeight-dy, 80, 4000)
	}
}

// clampF bounds a pane dimension.
func clampF(v, lo, hi float32) float32 {
	if v < lo {
		return lo
	}
	if v > hi {
		return hi
	}
	return v
}

func (s *Shell) activeInstanceInfo() *herdr.Instance {
	for index := range s.instances {
		if s.instances[index].Name == s.activeInstance {
			return &s.instances[index]
		}
	}
	return nil
}

func (s *Shell) agentItems(c *ui.Context) {
	if len(s.projection.Agents) == 0 {
		ui.Text(c, "No active agents").Padding(Spacing().M, Spacing().L).TextColor(c.Theme().TextMuted)
		return
	}
	// Two-line Agent cards (2026-10-05 round two) via the shared card
	// builder (2026-10-07): the same rows the agent activity panel shows.
	for _, card := range s.workbenchCards() {
		card := card
		label := card.ProjectName
		if label == "" {
			label = card.Title
		}
		selected := card.PaneID != "" && card.PaneID == s.selectedPaneID
		row := s.agentCardRow(c, card, selected)
		if row.Clicked() {
			// N19 #4: a sidebar Agent click jumps straight to the pane's
			// remembered surface (Chat or Terminal) instead of the detail
			// popover; the popover stays on the tray and header entry
			// points.
			s.openAgentFromSidebar(card)
		}
		row.ContextMenu(func(m *ui.Menu) {
			if m.Item("Open Chat").Chosen() {
				s.openChat(card)
			}
			if card.PaneID != "" {
				s.paneOverflowItems(m, card.PaneID, label)
			}
		})
	}
}

func (s *Shell) handleSidebarSelection(selected string) {
	kind, id, ok := strings.Cut(selected, ":")
	if !ok || id == "" {
		return
	}
	s.router.Push(routeWorkspace)
	switch kind {
	case "instance":
		s.openInstance(id)
	case "project":
		s.selectProject(id)
	case "tab":
		s.selectTab(id)
	case "pane", "agent":
		s.selectPane(id)
	case "recent":
		s.router.Push("/history/" + id)
	}
}
