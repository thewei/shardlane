package nativeui

/**
 * [INPUT]: Shell 启动门状态（bootstrapPending/Failed、errText、herdrUpgradeable/Updating/UpdateErr/UpdateNotWrbl）、MyGo 原生按钮与主题
 * [OUTPUT]: 全窗口居中的加载页与可重试错误页；本地 Herdr 过旧时提供 in-place「Update Herdr」更新流，失败持久可见并给出系统安装的终端出路
 * [POS]: 仅启动期呈现；就绪判定与投影仍归 Herdr，更新语义完全委托 herdr CLI 自身，GUI 永不提权
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// bootstrapPage is intentionally not the regular workspace empty state.
// Until the first projection is reconciled, no sidebar, Git menu, file tree
// or Terminal can be mounted. Later mutations keep the normal app visible.
func (s *Shell) bootstrapPage(c *ui.Context) {
	t := c.Theme()
	ui.Column(c).Fill().Center().Gap(12).Padding(32).Children(func() {
		if s.bootstrapPending {
			ui.Spinner(c).Size(27, 27)
			ui.Text(c, "Loading Herdr workspaces…").FontSize(Typography().Section).FontWeight(WeightSemibold).TextColor(t.Text)
			return
		}
		ui.Text(c, "Unable to load workspaces").FontSize(Typography().Heading).FontWeight(WeightSemibold)
		if s.errText != "" {
			ui.Text(c, s.errText).FontSize(Typography().Body).TextColor(t.TextMuted).MaxWidth(560)
		}
		if s.herdrUpgradeable {
			ui.Text(c, "This Herdr is older than Shardlane supports. Update it; Shardlane reconnects on its own.").FontSize(Typography().Body).TextColor(t.TextMuted).MaxWidth(560)
			if s.herdrUpdating {
				ui.Spinner(c).Size(27, 27)
				ui.Text(c, "Updating Herdr…").FontSize(Typography().Section).FontWeight(WeightSemibold).TextColor(t.Text)
				return
			}
			if ui.PrimaryButton(c, "Update Herdr").Clicked() {
				s.startHerdrUpgrade()
			}
		}
		if ui.Button(c, "Retry Connection").Clicked() {
			s.bootstrapPending = true
			s.reloadInstances(true)
		}
		// A failed update stays visible after the attempt ends — the page
		// never returns to silence. When even the OS authorization did not
		// happen, the terminal command is the way out.
		if s.herdrUpdateErr != "" {
			if s.herdrUpdateNotWrbl {
				ui.Text(c, "Updating Herdr needs the administrator permission, and it wasn't granted. Update it from a terminal instead, then press Retry Connection.").FontSize(Typography().Body).TextColor(t.TextMuted).MaxWidth(560)
				if ui.Button(c, "Copy Update Command").Clicked() {
					s.copyToClipboard(herdr.UpdateAsRootCommand)
				}
			} else {
				ui.Text(c, s.herdrUpdateErr).FontSize(Typography().Caption).TextColor(t.TextMuted).MaxWidth(560)
			}
		}
	})
}
