package nativeui

/**
 * [INPUT]: persisted Workbench Settings and user-edited Pi executable/commit rules
 * [OUTPUT]: Git & AI settings using shared page/header/card primitives
 * [POS]: Native Settings only; never invokes AI or Git while drawing
 * [PROTOCOL]: update this header when changing the file and check CLAUDE.md
 */

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/settings"
	"strings"
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
			ui.Text(c, "Applied to Pi suggestions for selected files. Pi generates only a candidate; Shardlane commits after you approve the message.").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
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
		settingsCard(c, "Pi Agent", func() {
			ui.Text(c, "Uses a one-shot, tool-free Pi CLI request. No Herdr pane or Agent session is created, and Pi cannot stage, modify files or commit. Your existing Pi login and model configuration are reused.").FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
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
