package nativeui

/**
 * [INPUT]: field label, value and optional full-value tooltip
 * [OUTPUT]: compact native read-only fact row in Agent detail cards
 * [POS]: reusable presentation primitive, independent from deleted right-side Inspector
 * [PROTOCOL]: update header and check CLAUDE.md
 */

import "github.com/egoist/mygo/ui"

func historyFact(c *ui.Context, label, value, tooltip string) {
	if value == "" {
		return
	}
	ui.Column(c).FillWidth().Gap(2).Children(func() {
		ui.Text(c, label).FontSize(Typography().Caption).TextColor(c.Theme().TextMuted)
		field := ui.Text(c, value).FontSize(Typography().BodySmall).SingleLine()
		if tooltip != "" {
			field.Tooltip(tooltip)
		} else {
			field.Tooltip(value)
		}
	})
}
