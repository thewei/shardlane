package nativeui

/**
 * [INPUT]: 依赖 persisted Workbench Settings 与用户配置的 Pi executable/commit rules
 * [OUTPUT]: 对外提供 Shell.settingsGit 渲染 Git 提交规范与 Pi 工具偏好
 * [POS]: nativeui Settings 中的 Git 偏好配置子面板
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/settings"
)

func commitPromptPreference(value string) string {
	if strings.TrimSpace(value) == "" {
		return settings.DefaultCommitPrompt
	}
	return value
}

func (s *Shell) settingsGit(c *ui.Context) {
	sp := Spacing()
	ui.Column(c).FillWidth().Gap(sp.L).Children(func() {
		settingsCard(c, "Commit Message", func() {
			ui.Text(c, "Rules applied when generating commit message candidates for selected files.").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
			ui.TextArea(c, &s.settingsGitPromptDraft).FillWidth().Height(250).Label("Git commit rules")
			ui.Row(c).Gap(sp.S).Children(func() {
				if ui.PrimaryButton(c, "Save Commit Rules").Clicked() {
					draft := strings.TrimSpace(s.settingsGitPromptDraft)
					if len(draft) == 0 || len(draft) > 5000 {
						s.pendingToast = "Commit rules must contain 1–5000 characters."
						return
					}
					s.applySettings(func(v *settings.Settings) error { v.Workbench.CommitPrompt = draft; return nil })
					s.pendingToast = "Git commit rules saved."
				}
				if ui.Button(c, "Restore Default Rules").Clicked() {
					s.settingsGitPromptDraft = settings.DefaultCommitPrompt
					s.applySettings(func(v *settings.Settings) error { v.Workbench.CommitPrompt = ""; return nil })
					s.pendingToast = "Default commit rules restored."
				}
			})
		})
		settingsCard(c, "Pi CLI", func() {
			ui.Text(c, "One-shot invocation of the local Pi CLI to draft commit subjects without altering repository files or state.").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
			ui.Form(c, func() {
				ui.Field(c, "Pi executable", func() {
					ui.TextInput(c, &s.settingsPiBinaryDraft).FillWidth().Label("Pi executable path").
						Placeholder("Use pi on PATH, or set an absolute path")
				}).Description("Native apps may not inherit your shell PATH. Set an absolute path if Pi cannot be found.")
			})
			ui.Row(c).Gap(sp.S).Children(func() {
				if ui.PrimaryButton(c, "Save Pi Path").Clicked() {
					draft := strings.TrimSpace(s.settingsPiBinaryDraft)
					if len(draft) > 1024 {
						s.pendingToast = "Pi path is too long."
						return
					}
					s.applySettings(func(v *settings.Settings) error { v.Workbench.PiExecutable = draft; return nil })
					s.pendingToast = "Pi executable preference saved."
				}
				if ui.Button(c, "Use PATH").Clicked() {
					s.settingsPiBinaryDraft = ""
					s.applySettings(func(v *settings.Settings) error { v.Workbench.PiExecutable = ""; return nil })
				}
			})
		})
	})
}
