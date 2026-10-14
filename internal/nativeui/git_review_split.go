package nativeui

/**
 * [INPUT]: Git commit/file selections, staged/unstaged tree, existing Godiff and Commit modal
 * [OUTPUT]: Fork-style Local Changes dual columns and All Commits history-over-review; the file navigator closes by itself in a narrow center, and a toolbar toggle brings it back
 * [POS]: Single native Git workspace surface; Herdr/runtime and Git mutations keep their owners
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

const (
	gitSplitMinTop          float32 = 170
	gitSplitMinBottom       float32 = 220
	gitSplitDefaultTop      float32 = 420
	gitFilePaneDefaultWidth float32 = 292
	gitFilePaneMinWidth     float32 = 205
	gitFileSplitterWidth    float32 = 6
	// gitFilePaneCollapseBelow is the width of the center (the window less
	// the sidebar) under which the file navigator closes by itself: open at
	// its default width, it would leave the diff under ~450 px.
	gitFilePaneCollapseBelow float32 = 750
)

// gitSplitReviewSurface intentionally has two different spatial hierarchies:
// Local Changes: file navigator | (diff + compact commit composer)
// All Commits: commit history above (changed-file navigator | read-only diff).
func (s *Shell) gitSplitReviewSurface(c *ui.Context) {
	t := c.Theme()
	ui.Column(c).Grow(1).MinWidth(0).MinHeight(0).Children(func() {
		if s.commitsMode {
			_, windowH := c.Size()
			maxTop := windowH - gitSplitMinBottom - 85
			if maxTop < gitSplitMinTop {
				maxTop = gitSplitMinTop
			}
			if s.gitUpperHeight == 0 {
				s.gitUpperHeight = gitSplitDefaultTop
			}
			upper := clampF(s.gitUpperHeight, gitSplitMinTop, maxTop)
			ui.Column(c).FillWidth().Height(upper).Shrink(0).MinHeight(0).Children(func() {
				s.gitCommitHistoryPane(c)
			})
			handle := ui.Row(c).Height(8).FillWidth().Shrink(0).Cursor(ui.CursorResizeNS).
				Label("Resize Git history and diff panes").Center().Children(func() {
				ui.Box(c).FillWidth().Height(1).Background(s.tokens(t.Dark).BorderSubtle)
			})
			if _, dy, held := handle.Dragged(); held {
				s.gitUpperHeight = clampF(upper+dy, gitSplitMinTop, maxTop)
			}
			s.gitCommitReview(c)
			return
		}
		s.gitLocalChangesReview(c)
	})
}

func (s *Shell) gitLocalChangesReview(c *ui.Context) {
	ui.Row(c).Grow(1).MinHeight(0).MinWidth(0).AlignItems(ui.Stretch).Children(func() {
		s.gitFileNavigationColumn(c, func() { s.sidebarFilesView(c) })
		ui.Column(c).Grow(1).MinHeight(0).MinWidth(0).Children(func() {
			ui.Column(c).Grow(1).MinHeight(0).MinWidth(0).Children(func() {
				s.gdDiffSurface(c)
			})
			s.gitQuickCommitComposer(c)
		})
	})
}

// gitFilePaneLimits is the navigator's width in a window this wide, and the
// widest it may be: the user's drag, kept clear of the sidebar and the diff.
func (s *Shell) gitFilePaneLimits(windowW float32) (width, maxWidth float32) {
	width = s.gitFilePaneWidth
	if width == 0 {
		width = gitFilePaneDefaultWidth
	}
	maxWidth = windowW - s.sidebarW - 270
	if maxWidth < gitFilePaneMinWidth {
		maxWidth = gitFilePaneMinWidth
	}
	return clampF(width, gitFilePaneMinWidth, maxWidth), maxWidth
}

// gitCenterWidth is the room the Git surface shares between its file
// navigator and its diff: the window less the sidebar, when it shows.
func (s *Shell) gitCenterWidth(windowW float32) float32 {
	if s.sidebarCollapsed {
		return windowW
	}
	return windowW - s.sidebarW
}

// gitFilePaneOpen tells whether the file navigator shows in a window this
// wide. A narrow center closes it by itself, to give the diff its width. The
// user's say is kept per width class: they open it in a narrow window, and
// close it in a wide one.
func (s *Shell) gitFilePaneOpen(windowW float32) bool {
	if s.gitCenterWidth(windowW) < gitFilePaneCollapseBelow {
		return s.gitFilePaneOpenNarrow
	}
	return !s.gitFilePaneClosedWide
}

// toggleGitFilePane opens or closes the navigator in the width class the
// window is in.
func (s *Shell) toggleGitFilePane(windowW float32) {
	if s.gitCenterWidth(windowW) < gitFilePaneCollapseBelow {
		s.gitFilePaneOpenNarrow = !s.gitFilePaneOpenNarrow
	} else {
		s.gitFilePaneClosedWide = !s.gitFilePaneClosedWide
	}
}

// gitFilePaneToggle is the toolbar button that shows or hides the navigator.
func (s *Shell) gitFilePaneToggle(c *ui.Context) {
	windowW, _ := c.Size()
	label := "Show file list"
	if s.gitFilePaneOpen(windowW) {
		label = "Hide file list"
	}
	if gdIconButton(c, iconPanel, label).Clicked() {
		s.toggleGitFilePane(windowW)
	}
}

// gitFileNavigationColumn is the reusable resize seam for the middle-left
// file tree. It does not reuse (or compete with) the app's Git reference rail.
// Closed, it leaves the whole width to the diff beside it.
func (s *Shell) gitFileNavigationColumn(c *ui.Context, body func()) {
	t := c.Theme()
	windowW, _ := c.Size()
	if !s.gitFilePaneOpen(windowW) {
		return
	}
	width, maxWidth := s.gitFilePaneLimits(windowW)
	ui.Column(c).Width(width).FillHeight().Shrink(0).MinHeight(0).Children(body)
	handle := ui.Box(c).Width(gitFileSplitterWidth).Shrink(0).Cursor(ui.CursorResizeEW).
		Label("Resize Git file navigator").Children(func() {
		ui.Box(c).Width(1).FillHeight().Background(s.tokens(t.Dark).BorderSubtle)
	})
	if dx, _, held := handle.Dragged(); held {
		s.gitFilePaneWidth = clampF(width+dx, gitFilePaneMinWidth, maxWidth)
	}
}

// Quick composer is an entry point into the SINGLE existing fenced Commit
// modal. It never calls git itself or duplicates the commit transaction.
func (s *Shell) gitQuickCommitComposer(c *ui.Context) {
	t := c.Theme()
	snap := s.gitSnapshot()
	if snap == nil || s.gdReviewingCommit() {
		return
	}
	ui.Column(c).FillWidth().Shrink(0).Padding(8, 12).
		BorderWidth(1, 0, 0, 0).BorderColor(s.tokens(t.Dark).BorderSubtle).Children(func() {
		contentColumn(c, formColumnWidth, func() {
			s.gitQuickCommitForm(c, snap)
		})
	})
}

// gitQuickCommitForm is the composer's fields and actions; the caller bounds
// its width so the inputs never run across a wide diff.
func (s *Shell) gitQuickCommitForm(c *ui.Context, snap *gitworkbench.ChangesSnapshot) {
	t := c.Theme()
	ui.Column(c).FillWidth().Gap(5).Children(func() {
		ui.TextInput(c, &s.gitQuickSubject).FillWidth().Height(Controls().RegularHeight).
			Placeholder("Commit subject").Label("Commit subject draft")
		ui.TextInput(c, &s.gitQuickBody).FillWidth().Height(Controls().RegularHeight).
			Placeholder("Description (optional)").Label("Commit description draft")
		ui.Row(c).FillWidth().Height(28).AlignItems(ui.Center).Gap(6).Children(func() {
			ui.Checkbox(c, &s.gitQuickAmend, "Amend")
			if s.piQuickRunning {
				ui.Spinner(c).Size(13, 13)
				ui.Text(c, "Generating subject…").FontSize(Typography().Micro).TextColor(t.TextMuted)
				if compactButton(c, "Cancel").Clicked() {
					s.cancelQuickAICommit()
				}
			}
			ui.Spacer(c)
			ui.Textf(c, "%d files", len(snap.Files)).FontSize(Typography().Micro).TextColor(t.TextMuted)
			blocked := s.git.opBusy || s.git.committing || s.git.branchBusy ||
				len(snap.Files) == 0 || (s.git.sequencerChecked && s.git.sequencer.Active())
			button := compactPrimaryButton(c, "Commit")
			button.Disabled(blocked || s.piQuickRunning)
			if button.Clicked() {
				s.openCommitSurface()
				s.surface.commit.Subject = s.gitQuickSubject
				s.surface.commit.Body = s.gitQuickBody
				s.surface.commit.Amend = s.gitQuickAmend
				// Do not overwrite a user-written amend message with HEAD.
				s.surface.commit.AmendPrefilled = s.gitQuickAmend &&
					(s.gitQuickSubject != "" || s.gitQuickBody != "")
			}
			// The dropdown is independent of Commit: it only requests a Pi
			// candidate and fills Subject after root/snapshot/draft checks.
			dropdown := gorexIconButton(c, gorexColorsOf(t.Dark), iconChevronDown, "Commit options", "Commit options",
				false, gorexPaneBtn, gorexPaneGlyph)
			dropdown.Menu(func(m *ui.Menu) {
				if m.Item("Draft Subject with Pi").Disabled(blocked || s.piQuickRunning).Chosen() {
					s.startQuickAICommit()
				}
			})
		})
		if s.piQuickError != "" {
			ui.Text(c, s.piQuickError).FontSize(Typography().BodySmall).TextColor(t.Warning).MaxWidth(600)
		}
	})
}

// showGitCommits switches both the upper navigator and lower reviewer.
func (s *Shell) showGitCommits() {
	s.commitsMode = true
	s.gitLowerTab = 1
	if s.git != nil && !s.gdReviewingCommit() && len(s.git.commits) > 0 {
		s.reviewCommit(s.git.commits[0].Hash)
	}
}

// gitCommitHistoryPane uses the wide top table as the screenshot's main
// commit navigator, with a compact graph cue and fixed author/hash/time cells.
func (s *Shell) gitCommitHistoryPane(c *ui.Context) {
	t := c.Theme()
	ui.Column(c).Grow(1).MinHeight(0).Children(func() {
		ui.Row(c).FillWidth().Height(36).Padding(6, 12).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Text(c, "All Commits").FontSize(Typography().Body).FontWeight(WeightSemibold)
			ui.Textf(c, "%d", len(s.git.commits)).FontSize(Typography().BodySmall).TextColor(t.TextMuted)
		})
		ui.Row(c).FillWidth().Padding(0, 12, 5).Children(func() {
			gdSearchInput(c, &s.gitCommitFilter, "Search commits", &s.git.gdTyping)
		})
		s.commitsPane(c, gdPaletteFor(t))
	})
}

func (s *Shell) gitCommitReview(c *ui.Context) {
	t := c.Theme()
	ui.Column(c).Grow(1).MinHeight(0).MinWidth(0).Children(func() {
		ui.Row(c).FillWidth().Height(36).Padding(0, 10).Gap(9).AlignItems(ui.Center).
			Background(t.Surface).Children(func() {
			// The reviewer's views are a segmented control like every other
			// one in the app — the native face, not a hand-painted tab.
			tab := s.gitLowerTab
			if ui.Segmented(c, &tab, "Commit", "Changes", "File Tree").Label("Commit view").Changed() {
				s.gitLowerTab = tab
			}
			if s.gitLowerTab == 1 {
				ui.Spacer(c)
				s.gitFilePaneToggle(c)
			}
		})
		if cm := s.git.commitMeta; cm != nil {
			ui.Row(c).FillWidth().Height(27).Padding(0, 12).Gap(10).AlignItems(ui.Center).
				BorderWidth(0, 0, 1, 0).BorderColor(s.tokens(t.Dark).BorderSubtle).Children(func() {
				ui.Text(c, cm.Author).FontSize(Typography().BodySmall).FontWeight(WeightSemibold)
				ui.Text(c, cm.Short).Font(gdCodeFont()).FontSize(Typography().Micro).TextColor(t.TextMuted)
				ui.Text(c, cm.Subject).FontSize(Typography().BodySmall).SingleLine().Grow(1).MinWidth(0)
				ui.Text(c, gdCommitTime(cm.Time)).FontSize(Typography().Micro).TextColor(t.TextMuted)
			})
		}
		if !s.gdReviewingCommit() {
			msg := "Select a commit to review its changes"
			if !s.git.commitsLoading && len(s.git.commits) == 0 {
				msg = "This repository has no commits yet"
			}
			s.gdEmptyPanel(c, gdPaletteFor(t), "No commit selected", msg, func() {
				if ui.PrimaryButton(c, "Open Terminal").Clicked() {
					s.showSurface(WorkspaceSurfaceTerminal)
				}
			})
			return
		}
		switch s.gitLowerTab {
		case 0:
			s.gitCommitDetails(c)
		case 2:
			s.gitCommitFiles(c)
		default:
			ui.Row(c).Grow(1).MinHeight(0).MinWidth(0).AlignItems(ui.Stretch).Children(func() {
				s.gitFileNavigationColumn(c, func() { s.gitCommitFiles(c) })
				ui.Column(c).Grow(1).MinWidth(0).MinHeight(0).Children(func() { s.gdDiffSurface(c) })
			})
		}
	})
}

func (s *Shell) gitCommitFiles(c *ui.Context) {
	t := c.Theme()
	snap := s.git.commitSnap
	ui.Column(c).Grow(1).MinHeight(0).MinWidth(0).Children(func() {
		ui.Row(c).FillWidth().Height(27).Padding(0, 10).AlignItems(ui.Center).Children(func() {
			ui.Text(c, "Changed Files").FontSize(Typography().BodySmall).FontWeight(WeightSemibold)
			if snap != nil {
				ui.Textf(c, "  %d", len(snap.Files)).FontSize(Typography().Micro).TextColor(t.TextMuted)
			}
		})
		ui.Row(c).FillWidth().Padding(0, 8, 3).Children(func() {
			gdSearchInput(c, &s.gitCommitFilesFilter, "Filter commit files", &s.git.gdTyping)
		})
		if snap == nil {
			ui.Column(c).Grow(1).Center().Children(func() { ui.Spinner(c).Size(15, 15) })
			return
		}
		rows := gitworkbench.Flatten(gitworkbench.FilterTree(
			gitworkbench.BuildTree(snap.Files, true), s.gitCommitFilesFilter), s.git.collapsedDirs)
		list := s.gdChangedTreeList(c, gdPaletteFor(t), rows, &s.changesList, &s.changesSelected,
			nil, gdViewCombined, func(row gitworkbench.FlatRow) {
				if row.Node.Dir {
					s.git.collapsedDirs[row.Node.Path] = !s.git.collapsedDirs[row.Node.Path]
				} else {
					s.revealDiffFile(row.Node.Path)
				}
			})
		list.Children(func() {
			if len(rows) == 0 {
				ui.Text(c, "No matching files").Padding(12).FontSize(Typography().BodySmall).TextColor(t.TextMuted)
			}
		})
	})
}

func (s *Shell) gitCommitDetails(c *ui.Context) {
	t := c.Theme()
	cm := s.git.commitMeta
	ui.Column(c).Grow(1).MinHeight(0).Padding(18).Gap(9).Children(func() {
		if cm == nil {
			ui.Text(c, "Reading commit…").FontSize(Typography().Body).TextColor(t.TextMuted)
			return
		}
		ui.Text(c, cm.Subject).FontSize(Typography().Section).FontWeight(WeightSemibold)
		ui.Text(c, "Author: "+cm.Author).FontSize(Typography().Body)
		ui.Text(c, "Commit: "+cm.Hash).Font(gdCodeFont()).FontSize(Typography().BodySmall).TextColor(t.TextMuted)
		ui.Text(c, gdCommitTime(cm.Time)).FontSize(Typography().BodySmall).TextColor(t.TextMuted)
		ui.Row(c).Gap(8).Children(func() {
			if ui.Button(c, "Copy Hash").Clicked() {
				s.copyToClipboard(cm.Hash)
			}
			if ui.Button(c, "Revert…").Clicked() {
				s.confirmRevertCommit(cm.Hash, cm.Subject)
			}
			if ui.Button(c, "Cherry-pick…").Clicked() {
				s.confirmCherryPickCommit(cm.Hash, cm.Subject)
			}
		})
	})
}
