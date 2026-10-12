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
	settingsCard(c, "Commit message rules", func() {
		ui.TextArea(c, &s.settingsGitPromptDraft).FillWidth().Height(220).Label("Git commit rules")
		ui.Row(c).Gap(sp.S).Children(func() {
			if ui.PrimaryButton(c, "Save").Label("Save commit rules").Clicked() {
				draft := strings.TrimSpace(s.settingsGitPromptDraft)
				if len(draft) == 0 || len(draft) > 5000 {
					s.pendingToast = "Commit rules must contain 1–5000 characters."
					return
				}
				s.applySettings(func(v *settings.Settings) error { v.Workbench.CommitPrompt = draft; return nil })
				s.pendingToast = "Commit rules saved."
			}
			if ui.Button(c, "Restore Default").Label("Restore default commit rules").Clicked() {
				s.settingsGitPromptDraft = settings.DefaultCommitPrompt
				s.applySettings(func(v *settings.Settings) error { v.Workbench.CommitPrompt = ""; return nil })
				s.pendingToast = "Default commit rules restored."
			}
		})
	})
	settingsGroup(c, "Commit message generator",
		row("Pi path", "Leave empty to find pi automatically.", func() {
			ui.Row(c).Gap(sp.S).AlignItems(ui.Center).Children(func() {
				ui.TextInput(c, &s.settingsPiBinaryDraft).Width(220).Label("Pi executable path").Placeholder("pi")
				if ui.Button(c, "Save").Label("Save Pi path").Clicked() {
					draft := strings.TrimSpace(s.settingsPiBinaryDraft)
					if len(draft) > 1024 {
						s.pendingToast = "Pi path is too long."
						return
					}
					s.applySettings(func(v *settings.Settings) error { v.Workbench.PiExecutable = draft; return nil })
					s.pendingToast = "Pi path saved."
				}
			})
		}),
	)
}
