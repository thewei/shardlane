package nativeui

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

/**
 * [INPUT]: 依赖 gitService 的已缓存 ChangesSnapshot、现有 stage/unstage/commit/undo/stash 方法
 * [OUTPUT]: Diff Git Workbench 的主操作条（Stage/Unstage All、Fetch/Pull/Push 图标）和 Git actions 下拉菜单
 * [POS]: nativeui 统一 Git 行为入口；Diff 拥有文件审阅，弹层拥有轻量操作，Terminal 不重复 Git toolbar
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// gitActionBarCompact keeps the Diff Review legible when the left rail and
// inspector consume much of the native window. This is presentation-only:
// moving actions to a menu never changes Git semantics or stored state.
const gitActionBarMinExpandedCenterWidth float32 = 760

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
	// The file navigator is a separate column inside the center: judge the
	// room the toolbar really has, wherever the navigator was dragged to.
	compact := s.gdColumnWidth(c) < gitActionBarMinExpandedCenterWidth
	ui.Row(c).FillWidth().MinWidth(0).Gap(sp.S).AlignItems(ui.Center).Children(func() {
		// The navigator closes by itself in a narrow window; the button at
		// the diff's edge is how it comes back.
		s.gitFilePaneToggle(c)
		// The branch selector already lives in the repo bar. Don't render
		// the same branch label a second time in the Diff toolbar.
		stage, unstage := gitActionPaths(snap)
		busySequencer := s.git.sequencerChecked && s.git.sequencer.Active()
		canStage := !s.git.opBusy && len(stage) > 0 && (!busySequencer || s.git.sequencer.Unmerged == 0)
		canUnstage := !s.git.opBusy && len(unstage) > 0 && !busySequencer
		if !compact {
			stageBtn := compactButton(c, "Stage All")
			stageBtn.Disabled(!canStage)
			if stageBtn.Clicked() {
				s.stageFiles(stage)
			}
			unstageBtn := compactButton(c, "Unstage All")
			unstageBtn.Disabled(!canUnstage)
			if unstageBtn.Clicked() {
				s.unstageFiles(unstage)
			}
		}
		ui.Box(c).Grow(1)
		// Fetch/Pull/Push are one tap away as icons; the rest of the
		// less-common actions live in the Git actions pull-down.
		s.gdSyncButtons(c, gdPaletteFor(t))
		// Split/Unified must remain reachable even when actions compact.
		s.gdLayoutControl(c, gdPaletteFor(t))
		// The local worktree's bottom composer is the single Commit entry.
		// This toolbar only carries review/batch actions and the pull-down.
		more := gdPulldown(c, iconActions, "Git actions")
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
			if m.Item("Stash All Changes").Disabled(s.git.opBusy || busySequencer || len(snap.Files) == 0).Chosen() {
				s.stashChanges()
			}
			if m.Item("Undo Last Commit…").Disabled(s.git.opBusy || busySequencer || snap.NoHead || snap.Branch == "").Chosen() {
				s.undoLastCommit()
			}
			if m.Item("View All Commits").Chosen() {
				s.showGitCommits()
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
