package nativeui

/**
 * [INPUT]: 依赖 Shell 当前 Router/Workspace/Agent 投影、共享 agentCardRow 和 native Quick Panel
 * [OUTPUT]: 提供 Sidebar 的顶部视图标签条（Terminal/Changes/Sessions/Files/Hosts 五个等宽图标标签，Settings 路由不渲染）、Workspace 树、Git 引用导航、Sessions（项目头 + 范围行）/Settings 导航、Settings 的底部 Back 与其余所有路由的底部标准页脚（Session 切换 + Settings 钮，Terminal/Sessions/Files/Hosts/Changes 共用）与 Agent 行点击/右键动作
 * [POS]: 唯一左栏宿主，Git 引用树由 git_sidebar_tree 负责，Agent 详情复用 Quick Panel；视图切换在栏顶标签条（Settings 无），除 Settings 外全部路由共用页脚
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
	// beside the traffic lights above this body. The strip at the top owns
	// the view switch (2026-10-14) — except in Settings, where it is hidden
	// (2026-10-15 user request): Settings is no strip destination, and its
	// one way back is the Back foot at the bottom.
	ui.Column(c).Width(s.sidebarW).Grow(1).MinHeight(0).Shrink(0).Children(func() {
		if !s.sidebarInSettings() {
			s.sidebarViewTabs(c)
		}
		if s.sidebarInDiffMode() {
			// Git references are full-height navigation, not compressed toolbar chips.
			s.sidebarProjectHeader(c, s.git.root, "Refresh repository", s.refreshGitChanges)
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
			s.sidebarProjectHeader(c, s.rightPanel.currentRoot, "Refresh files", s.refreshFiles)
			ui.Divider(c)
			s.filesToolView(c)
		} else if isHistoryRoute(s.router.Path()) {
			// Sessions wears the same project/branch head as Git and Files;
			// under it the scope rows say whose sessions the list shows.
			s.sidebarProjectHeader(c, s.historyProjectRoot(), "Refresh sessions", s.refreshHistoryScan)
			ui.Divider(c)
			s.historyNav(c)
		} else if s.sidebarInnerPage() {
			s.sidebarInnerNav(c)
		} else if ui.Sidebar(c, &s.selected, func() {
			// The Agents/Pin/Workspace sections wear the same tree header as
			// the Git references and the Hosts groups.
			s.sidebarSection(c, sidebarSectionSpec{Key: "section:agents", Title: "Agents", Count: len(s.workbenchCards())}, &s.agentsOpen, func() {
				s.agentItems(c)
			})
			if pins := s.pinnedPaneIDs(); len(pins) > 0 {
				s.sidebarSection(c, sidebarSectionSpec{Key: "section:pin", Title: "Pin", Count: len(pins)}, &s.pinsOpen, func() {
					s.pinnedItemsView(c)
				})
			}
			s.workspaceSection(c)
			// The widget's own Surface fill is dropped (2026-10-06): the
			// Agents/Workspace blocks sit on the window gradient like the
			// rest of the chrome. Of the widget's uniform inset only the
			// top is zeroed (2026-10-13 user report): the sections start
			// flush under the band, while the sides and the foot keep the
			// framework spacing.
		}).Background(ui.Transparent).Padding(0, t.Space(2.5), t.Space(2.5)).
			Grow(1).Label("Shardlane sidebar").Changed() {
			s.handleSidebarSelection(s.selected)
		}

		// The sidebar foot. On the workspace sidebar the Workspace is the
		// bound Herdr instance, so switching belongs in the footer rather
		// than nesting Projects under a Workspace tree. Every other route —
		// the strip pages (Sessions, Files, Hosts) and the Git views on the
		// Changes tab — wears the same standard foot (2026-10-15 user
		// requests). Settings keeps its one way back instead: it is not a
		// strip destination and the strip is hidden above it.
		foot := func(content func()) {
			ui.Row(c).Height(42).Padding(5, 8).Gap(6).AlignItems(ui.Center).
				BorderWidth(1, 0, 0, 0).BorderColor(tokens.BorderSubtle).Children(content)
		}
		standardFoot := func() {
			foot(func() {
				s.sessionSelector(c)
				if gorexIconButton(c, gorexColorsOf(t.Dark), iconSettings, "Settings", "Settings", false, gorexIconBtn, gorexIconGlyph).Clicked() {
					s.router.Push(routeSettings)
				}
			})
		}
		if s.sidebarInSettings() {
			foot(func() { s.sidebarBackRow(c) })
		} else {
			standardFoot()
		}
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
// Settings, History (list, detail, by-project), Files or Hosts. Inner pages
// swap the sidebar body for their own navigation.
func (s *Shell) sidebarInnerPage() bool {
	path := s.router.Path()
	return isHistoryRoute(path) || path == routeFiles || path == routeHosts || s.sidebarInSettings()
}

// sidebarInSettings reports whether the route is a Settings page — the one
// inner page whose sidebar keeps a Back foot.
func (s *Shell) sidebarInSettings() bool {
	return strings.HasPrefix(s.router.Path(), "/settings/")
}

// sidebarViewTabs is the sidebar's first row: the five app-level views —
// Terminal, Changes, Sessions, Files, Hosts — as equal-width icon tabs, on
// every route except Settings (2026-10-15 user request: Settings' way out
// is the Back foot at the bottom, so it carries no destination strip). The
// lit tab is the page on screen. The Terminal tab is the way home from
// any page (and from the Chat face); pressing another lit tab again leaves
// for the Terminal too (from a nested Sessions page, first back to the
// list), so a view is always one click away and one click out. Accessible
// labels stay stable for shortcuts and assistive technologies; tooltips
// describe the action that will happen.
func (s *Shell) sidebarViewTabs(c *ui.Context) {
	k := gorexColorsOf(c.Theme().Dark)
	onTerminal, onChat, onChanges, onHistory := s.viewSwitchActive()
	historyTip := "Sessions"
	switch s.router.Path() {
	case routeHistory:
		historyTip = "Back to Terminal"
	default:
		if onHistory {
			historyTip = "All conversations"
		}
	}
	sp := Spacing()
	ui.Row(c).FillWidth().Padding(0, sidebarGutter, sp.S).Gap(sp.XS).Children(func() {
		if gorexIconTab(c, k, iconTerminal, "Terminal view", "Terminal", onTerminal || onChat).Clicked() {
			s.showViewTerminal()
		}
		if gorexIconTab(c, k, iconGit, "Changes view", "Changes", onChanges).Clicked() {
			s.showViewChanges()
		}
		if gorexIconTab(c, k, iconHistory, "History view", historyTip, onHistory).Clicked() {
			s.showViewHistory()
		}
		if gorexIconTab(c, k, iconFolder, "Files view", "Browse project files", s.router.Path() == routeFiles).Clicked() {
			s.showViewFiles()
		}
		if gorexIconTab(c, k, iconGlobe, "Hosts view", "SSH hosts and machines", s.router.Path() == routeHosts).Clicked() {
			s.showViewHosts()
		}
	})
}

// sidebarBackRow is Settings' one way back, labelled Back; its tooltip names
// the destination: the Workspace's Terminal, not the last visible
// Diff/Chat/Commit surface. The global router history shortcut remains
// independent of this explicit product navigation.
func (s *Shell) sidebarBackRow(c *ui.Context) {
	if navButton(c, iconArrowLeft, "Back", "", false).Tooltip("Back to Terminal").Clicked() {
		s.showViewTerminal()
	}
}

// sidebarInnerNav is the Settings and Hosts sidebar body: the page's
// navigation lives in the app sidebar while the route owns the content area
// (Files and Sessions carry the project header and build their own).
func (s *Shell) sidebarInnerNav(c *ui.Context) {
	ui.Column(c).Grow(1).MinHeight(0).Padding(14, 9, 9).Gap(3).Children(func() {
		switch path := s.router.Path(); {
		case strings.HasPrefix(path, "/settings/"):
			s.settingsNav(c)
		case path == routeHosts:
			s.hostsNav(c)
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
				ui.Text(c, "  "+pluralFiles(len(snap.Files))).FontSize(Typography().BodySmall).TextColor(t.TextMuted)
			}
		})

		ui.Row(c).FillWidth().Padding(8, 10, 10).Children(func() {
			if gdSearchInput(c, &s.changes.filter, "Filter files") {
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
			s.gdSectionHeader(c, "Unstaged Files", len(unstagedFiles), &s.unstagedOpen,
				"Stage", gdActionPaths(&s.unstagedChoice, unstagedFiles), s.stageFiles)
			if s.unstagedOpen && len(unstagedRows) > 0 {
				s.gdChangedTreeList(c, pal, unstagedRows, &s.unstagedList, &s.unstagedSelected,
					&s.unstagedChoice, gdViewUnstaged, func(row gitworkbench.FlatRow) { s.changesClickSide(row, gdViewUnstaged) })
			}

			if s.unstagedOpen && s.stagedOpen && len(stagedFiles) > 0 && len(unstagedFiles) > 0 {
				s.gdPaneDivider(c)
			}

			// The Staged pane sits below the divider at the dragged height.
			s.gdSectionHeader(c, "Staged Files", len(stagedFiles), &s.stagedOpen,
				"Unstage", gdActionPaths(&s.stagedChoice, stagedFiles), s.unstageFiles)
			if s.stagedOpen && len(stagedRows) > 0 {
				list := s.gdChangedTreeList(c, pal, stagedRows, &s.stagedList, &s.stagedSelected,
					&s.stagedChoice, gdViewStaged, func(row gitworkbench.FlatRow) { s.changesClickSide(row, gdViewStaged) })
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
		row := ui.Row(c).Gap(9).Padding(5, 9).Radius(treeRowRadius).AlignItems(ui.Center).MinWidth(0)
		muted, ref := t.TextMuted, pal.ref
		switch {
		case current && focused:
			row.Background(t.Accent).TextColor(t.AccentText)
			muted, ref = t.AccentText.Alpha(0.75), t.AccentText
		case current:
			// Unfocused selection shares the sidebar's token so the same
			// "this row is selected" tint reads identically across the
			// tree, the commits list and the changes tree (F155).
			row.Background(designTokens(t.Dark).SidebarSelect)
		case row.Hovered():
			row.Background(designTokens(t.Dark).SidebarHover)
		}
		if row.Clicked() {
			s.reviewCommit(cm.Hash)
		}
		// Long commit subjects preserve the full metadata in the tooltip.
		row.Tooltip(cm.Subject + "\n" + cm.Author + " · " + gdCommitTime(cm.Time) + "\n" + cm.Hash)
		row.ContextMenu(func(m *ui.Menu) {
			if m.Item("Cherry-pick Commit…").Chosen() {
				s.confirmCherryPickCommit(cm.Hash, cm.Subject)
			}
			if m.Item("Revert Commit…").Chosen() {
				s.confirmRevertCommit(cm.Hash, cm.Subject)
			}
			if m.Item("Copy Commit Hash").Chosen() {
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

// gdSectionHeader is one pane's title row: the shared tree header (chevron,
// semibold title, count) with the pane's Stage/Unstage action kept visible at
// its right edge.
func (s *Shell) gdSectionHeader(c *ui.Context, title string, count int, open *bool, actionLabel string, paths []string, action func([]string)) {
	spec := sidebarSectionSpec{Key: "changes:" + title, Title: title, Count: count, Closed: !*open}
	if count > 0 && !s.git.opBusy {
		spec.Trailing = func() {
			if sectionTextAction(c, actionLabel, actionLabel+" ("+pluralFiles(len(paths))+")").Clicked() {
				action(paths)
			}
		}
	}
	if _, clicked := s.sidebarSectionHeader(c, spec); clicked {
		*open = !*open
	}
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
	cards := s.workbenchCards()
	dupProjects := duplicateAgentProjectLabels(cards)
	for _, card := range cards {
		card := card
		label, _ := agentCardLabel(card, dupProjects)
		selected := card.PaneID != "" && card.PaneID == s.selectedPaneID
		row := s.agentCardRow(c, card, selected, dupProjects, nil)
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
	// No bare workspace push beside the selection: selectProject/selectTab/
	// selectPane record the landing as /workspace?pane=… themselves, so a
	// row click is one history entry, not two.
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
