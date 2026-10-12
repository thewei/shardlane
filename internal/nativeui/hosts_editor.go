package nativeui

/**
 * [INPUT]: 依赖 internal/sshconfig（SaveFile 原子写回、EffectiveConfig 求值）、ui.TextArea、gdCodeFont
 * [OUTPUT]: hosts 页原始编辑器（打开态渲染、放弃确认、保存错误内联）与生效配置区块
 * [POS]: 无损原始编辑：编辑文本即文件字节，保存走 sshconfig 原子写 + .bak 备份；生效配置只展示系统 ssh -G 的输出
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"github.com/egoist/mygo/ui"
)

// hostsEditor is the raw text editor over one config file. What is on
// screen is exactly what a save writes — no re-serialization, so comments
// and unknown directives survive by construction.
func (s *Shell) hostsEditor(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()
	tokens := designTokens(t.Dark)
	ui.Column(c).FillWidth().Grow(1).MinWidth(0).MinHeight(0).Children(func() {
		ui.Row(c).FillWidth().Padding(sp.M, sp.L).Gap(sp.S).AlignItems(ui.Center).
			BorderWidth(0, 0, 1, 0).BorderColor(tokens.BorderSubtle).Children(func() {
			ui.Icon(c, iconFile).Size(14, 14).TextColor(t.TextMuted)
			ui.Text(c, compactSearchPath(s.hosts.editorFile)).FontSize(Typography().Caption).
				TextColor(t.TextMuted).Grow(1).MinWidth(0).SingleLine().Tooltip(s.hosts.editorFile)
			if s.hostsEditorDirty() {
				statusPill(c, "edited", ToneWorking)
			}
			if s.hosts.saving {
				ui.Spinner(c).Size(14, 14)
			}
			if compactButton(c, "Cancel").Clicked() {
				s.cancelHostEditor()
			}
			if compactPrimaryButton(c, "Save").Clicked() {
				s.saveHostEditor()
			}
		})
		if s.hosts.editorErr != "" {
			inlineNotice(c, ToneError, s.hosts.editorErr, nil)
		}
		if s.hosts.editorConfirmClose {
			ui.Row(c).FillWidth().Padding(sp.XS, sp.L).Gap(sp.S).AlignItems(ui.Center).
				Background(tokens.Panel).Children(func() {
				ui.Text(c, "Discard unsaved changes?").FontSize(Typography().BodySmall).Grow(1)
				if ui.Button(c, "Keep Editing").Clicked() {
					s.hosts.editorConfirmClose = false
				}
				if ui.Button(c, "Discard").Clicked() {
					s.closeHostEditor()
				}
			})
		}
		ui.Column(c).FillWidth().Grow(1).MinHeight(0).Padding(0, sp.L, sp.L).Children(func() {
			ui.TextArea(c, &s.hosts.editorText).Grow(1).FillWidth().MinHeight(0).
				Font(gdCodeFont()).Label("SSH config")
		})
	})
}

// hostsEditorDirty reports whether the editor text diverged from what was
// loaded (an empty loaded marker marks a not-yet-saved new config).
func (s *Shell) hostsEditorDirty() bool {
	return s.hosts.editing && s.hosts.editorText != s.hosts.editorLoaded
}

// cancelHostEditor asks once before dropping unsaved work; the confirmation
// is inline so the editor never loses its place.
func (s *Shell) cancelHostEditor() {
	if s.hostsEditorDirty() && !s.hosts.editorConfirmClose {
		s.hosts.editorConfirmClose = true
		return
	}
	s.closeHostEditor()
}

func (s *Shell) closeHostEditor() {
	s.hosts.editing = false
	s.hosts.editorConfirmClose = false
	s.hosts.editorErr = ""
}

// hostEffectiveSection is the on-demand `ssh -G` view for one connectable
// alias: the system client is the compatibility authority, the page only
// renders its output.
func (s *Shell) hostEffectiveSection(c *ui.Context, alias string) {
	if alias == "" {
		return
	}
	sp := Spacing()
	ui.Column(c).FillWidth().Gap(sp.S).Children(func() {
		ui.Row(c).FillWidth().Gap(sp.S).AlignItems(ui.Center).Children(func() {
			label := "Show effective config"
			if s.hosts.effOpen && s.hosts.effFor == alias {
				label = "Hide effective config"
			}
			if compactButton(c, label).Clicked() {
				s.toggleHostEffective(alias)
			}
			if s.hosts.effBusy && s.hosts.effFor == alias {
				ui.Spinner(c).Size(14, 14)
			}
		})
		if !s.hosts.effOpen || s.hosts.effFor != alias {
			return
		}
		if s.hosts.effErr != "" {
			inlineNotice(c, ToneError, s.hosts.effErr, func() { s.loadHostEffective(alias) })
			return
		}
		settingsCard(c, "Effective config (ssh -G)", func() {
			if len(s.hosts.eff) == 0 {
				ui.Text(c, "No output.").FontSize(Typography().BodySmall).TextColor(c.Theme().TextMuted)
			}
			const effectiveShown = 80
			shown := min(len(s.hosts.eff), effectiveShown)
			for _, kv := range s.hosts.eff[:shown] {
				formRow(c, kv.Key, kv.Value, nil)
			}
			if rest := len(s.hosts.eff) - shown; rest > 0 {
				ui.Textf(c, "… %d more", rest).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
			}
		})
	})
}
