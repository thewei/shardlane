package nativeui

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

/**
 * [INPUT]: 依赖 gitService 的已缓存 ChangesSnapshot、现有 stage/unstage/commit/undo/stash 方法
 * [OUTPUT]: Diff Git Workbench 的主操作条和二级菜单
 * [POS]: nativeui 统一 Git 行为入口；Diff 拥有文件审阅，弹层拥有轻量操作，Terminal 不重复 Git toolbar
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// gitActionBarCompact keeps the Diff Review legible when the left rail and
// inspector consume much of the native window. This is presentation-only:
// moving actions to a menu never changes Git semantics or stored state.
const gitActionBarMinExpandedCenterWidth float32 = 760

func gitActionBarCompact(windowWidth float32, sidebarVisible, inspectorVisible bool, inspectorWidth int) bool {
	if windowWidth <= 0 {
		return false // no geometry established yet
	}
	center := windowWidth - 2*gorexGap
	if sidebarVisible {
		center -= sidebarWidth + gorexGap
	}
	if inspectorVisible {
		center -= float32(inspectorWidth) + gorexGap
	}
	return center < gitActionBarMinExpandedCenterWidth
}

// gitWorkbenchActionBar keeps common actions reachable without moving the
// user out of the main Diff Review. Less common actions live in a native
// menu, preserving keyboard focus and reducing visual clutter.
func (s *Shell) gitWorkbenchActionBar(c *ui.Context) {
	snap := s.gitSnapshot()
	if snap == nil || s.git == nil {
		return
	}
	sp := Spacing()
	t := c.Theme()
	width, _ := c.Size()
	compact := gitActionBarCompact(width, !s.sidebarCollapsed, s.contextPanelVisible(width), s.rightPanel.width)
	ui.Row(c).FillWidth().MinWidth(0).Gap(sp.S).AlignItems(ui.Center).Children(func() {
		// The branch selector already lives in the repo bar. Don't render
		// the same branch label a second time in the Diff toolbar.
		stage, unstage := gitActionPaths(snap)
		busySequencer := s.git.sequencerChecked && s.git.sequencer.Active()
		canStage := !s.git.opBusy && len(stage) > 0 && (!busySequencer || s.git.sequencer.Unmerged == 0)
		canUnstage := !s.git.opBusy && len(unstage) > 0 && !busySequencer
		if !compact {
			stageBtn := ui.Button(c, "Stage All").FontSize(Typography().Caption)
			stageBtn.Disabled(!canStage)
			if stageBtn.Clicked() {
				s.stageFiles(stage)
			}
			unstageBtn := ui.Button(c, "Unstage All").FontSize(Typography().Caption)
			unstageBtn.Disabled(!canUnstage)
			if unstageBtn.Clicked() {
				s.unstageFiles(unstage)
			}
		}
		ui.Box(c).Grow(1)
		if !compact {
			s.gdLayoutControl(c, gdPaletteFor(t))
		}
		refresh := ui.Button(c, "Refresh").FontSize(Typography().Caption)
		refresh.Disabled(s.git.opBusy)
		if refresh.Clicked() {
			s.refreshGitChanges()
		}
		commit := ui.PrimaryButton(c, "Commit…").FontSize(Typography().Caption)
		commit.Disabled(s.git.opBusy || s.git.committing || s.git.branchBusy || busySequencer || len(snap.Files) == 0)
		if commit.Clicked() {
			s.openCommitSurface()
		}
		more := ui.Button(c, "More…").FontSize(Typography().Caption).
			Tooltip("Fetch, Pull, Push, stash, undo, history and Git settings")
		more.Menu(func(m *ui.Menu) {
			if compact {
				if m.Item("Stage All").Disabled(!canStage).Chosen() {
					s.stageFiles(stage)
				}
				if m.Item("Unstage All").Disabled(!canUnstage).Chosen() {
					s.unstageFiles(unstage)
				}
				m.Separator()
			}
			if compact {
				if m.Item("Diff layout: Split / Unified").Chosen() {
					s.toggleDiffLayout()
				}
				m.Separator()
			}
			if m.Item("Refresh Changes").Chosen() {
				s.refreshGitChanges()
			}
			if m.Item("Fetch").Disabled(s.git.opBusy || busySequencer).Chosen() {
				s.fetchRepo()
			}
			if m.Item("Pull (fast-forward only)").Disabled(s.git.opBusy || busySequencer).Chosen() {
				s.pullRepo()
			}
			if m.Item("Push").Disabled(s.git.opBusy || busySequencer).Chosen() {
				s.pushRepo()
			}
			m.Separator()
			if m.Item("Stash All Changes").Disabled(s.git.opBusy || busySequencer || len(snap.Files) == 0).Chosen() {
				s.stashChanges()
			}
			if m.Item("Undo Last Commit…").Disabled(s.git.opBusy || busySequencer || snap.NoHead || snap.Branch == "").Chosen() {
				s.undoLastCommit()
			}
			if m.Item("View All Commits").Chosen() {
				s.commitsMode = true
			}
			m.Separator()
			if m.Item("Git Preferences…").Chosen() {
				s.router.Push("/settings/git")
			}
		})
	})
}

func gitActionPaths(snap *gitworkbench.ChangesSnapshot) (stage, unstage []string) {
	if snap == nil {
		return nil, nil
	}
	for _, file := range snap.Files {
		if file.Untracked || file.Unstaged {
			stage = append(stage, file.Path)
		}
		if file.Staged {
			unstage = append(unstage, file.Path)
		}
	}
	return stage, unstage
}
