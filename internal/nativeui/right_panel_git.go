package nativeui

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

/**
 * [INPUT]: 当前 Repo snapshot、Diff 选择、Commit 元数据与共享 Design Tokens
 * [OUTPUT]: Git 专属 Review Inspector（工作树或选中 Commit 上下文）
 * [POS]: Right Panel 的 SurfaceChanges 在 Git 主工作表面的上下文投影，不再复制左侧文件导航
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// gitReviewInspector uses only the cached Git review projection; rendering
// performs no Git IO and cannot mutate repository state. The left sidebar
// already owns changed-file navigation, so the right panel explains the
// selected object instead of rendering another file tree.
func (s *Shell) gitReviewInspector(c *ui.Context) {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	sp := Spacing()
	ui.Column(c).Grow(1).MinHeight(0).Children(func() {
		ui.Row(c).FillWidth().Padding(sp.L).Gap(sp.S).AlignItems(ui.Center).
			BorderWidth(0, 0, 1, 0).BorderColor(tokens.BorderSubtle).Children(func() {
			ui.Icon(c, iconFileDiff).Size(14, 14).TextColor(t.TextMuted)
			ui.Text(c, "Review Inspector").FontSize(Typography().Section).FontWeight(650).Grow(1)
			if s.gdReviewingCommit() {
				ui.Text(c, "READ ONLY").FontSize(Typography().Micro).TextColor(t.TextMuted)
			}
		})
		ui.Scroll(c).Grow(1).Children(func() {
			ui.Column(c).FillWidth().Padding(sp.L).Gap(sp.L).Children(func() {
				if s.git == nil || s.git.root == "" {
					s.historyOverviewCard(c, "No Git repository", "Choose a Git-backed Project or Tab.")
					return
				}
				if s.gdReviewingCommit() {
					s.gitCommitInspector(c)
				} else {
					s.gitWorktreeInspector(c)
				}
			})
		})
	})
}

func (s *Shell) gitWorktreeInspector(c *ui.Context) {
	snap := s.gitSnapshot()
	if snap == nil {
		s.historyOverviewCard(c, "Reading changes", "Repository summary is loading.")
		return
	}
	s.gitSequencerInspector(c)
	settingsCard(c, "Repository", func() {
		historyFact(c, "Branch", fallbackText(snap.Branch, "Detached HEAD"), "")
		historyFact(c, "Changed files", fmt.Sprintf("%d", len(snap.Files)), "")
		historyFact(c, "Lines added", fmt.Sprintf("+%d", snap.TotalAdditions), "")
		historyFact(c, "Lines deleted", fmt.Sprintf("-%d", snap.TotalDeletions), "")
		if st := s.git.upstream; st.OK {
			historyFact(c, "Upstream", st.Remote+"/"+st.Branch, "")
			historyFact(c, "Ahead / Behind", fmt.Sprintf("%d / %d", st.Ahead, st.Behind), "")
		}
	})
	selected := s.gitSelectedReviewFile()
	if selected != nil {
		settingsCard(c, "Selected file", func() {
			historyFact(c, "Path", selected.Path, selected.Path)
			historyFact(c, "Status", selected.Status.Letter(), "")
			if selected.Staged {
				historyFact(c, "Index", "Staged", "")
			}
			if selected.Unstaged || selected.Untracked {
				historyFact(c, "Working tree", "Unstaged changes", "")
			}
			historyFact(c, "Change", fmt.Sprintf("+%d  -%d", selected.Additions, selected.Deletions), "")
			if ui.Button(c, "Copy file path").Clicked() {
				s.copyToClipboard(selected.Path)
			}
		})
	}
	// Worktree actions belong to the central toolbar and the left
	// navigation; the right Inspector is selected-object context only.
}

func (s *Shell) gitCommitInspector(c *ui.Context) {
	if s.git.commitMeta != nil {
		meta := s.git.commitMeta
		settingsCard(c, "Commit", func() {
			ui.Text(c, meta.Subject).FontSize(Typography().Body).FontWeight(650)
			historyFact(c, "Hash", meta.Hash, meta.Hash)
			historyFact(c, "Author", meta.Author, "")
			historyFact(c, "Date", meta.Time.Format("2006-01-02 15:04"), "")
			if ui.Button(c, "Copy commit hash").Clicked() {
				s.copyToClipboard(meta.Hash)
			}
			if ui.Button(c, "Revert commit…").Clicked() {
				s.confirmRevertCommit(meta.Hash, meta.Subject)
			}
			if ui.Button(c, "Cherry-pick commit…").Clicked() {
				s.confirmCherryPickCommit(meta.Hash, meta.Subject)
			}
		})
	} else {
		s.historyOverviewCard(c, "Commit review", "Loading details for selected commit "+s.git.commitHash)
	}
	if snap := s.gdActiveSnapshot(); snap != nil {
		settingsCard(c, "Change summary", func() {
			historyFact(c, "Files", fmt.Sprintf("%d", len(snap.Files)), "")
			historyFact(c, "Added / Removed", fmt.Sprintf("+%d / -%d", snap.TotalAdditions, snap.TotalDeletions), "")
		})
	}
}

func (s *Shell) gitSelectedReviewFile() *gitworkbench.ChangeFile {
	if s.git == nil {
		return nil
	}
	path := s.surface.diff.SelectedPath
	if path == "" && s.git.gdCurrent >= 0 && s.git.gdCurrent < len(s.git.gdFiles) &&
		s.git.gdFiles[s.git.gdCurrent] != nil && s.git.gdFiles[s.git.gdCurrent].cf != nil {
		path = s.git.gdFiles[s.git.gdCurrent].cf.Path
	}
	if snap := s.gitSnapshot(); snap != nil {
		if cf, found := snap.ByPath(path); found {
			return &cf
		}
	}
	return nil
}
