package nativeui

/**
 * [INPUT]: Shell initial Herdr bootstrap gate, status/error and native theme
 * [OUTPUT]: Full-window centered loading or retryable error without workspace chrome
 * [POS]: Startup-only presentation; Herdr continues to own readiness and projection
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import "github.com/egoist/mygo/ui"

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
		if ui.PrimaryButton(c, "Retry connection").Clicked() {
			s.bootstrapPending = true
			s.reloadInstances(true)
		}
	})
}
