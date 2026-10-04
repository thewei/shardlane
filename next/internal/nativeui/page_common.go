package nativeui

import (
	"strings"

	"github.com/egoist/mygo/ui"
)

// pageHeader is the shared page header band (Design System v2).
func pageHeader(c *ui.Context, title, subtitle string) {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	typ := Typography()
	sp := Spacing()
	ui.Column(c).
		Padding(sp.XL, sp.XL, sp.L).
		Gap(sp.XS).
		BorderWidth(0, 0, 1, 0).
		BorderColor(tokens.BorderSubtle).
		Children(func() {
			ui.Text(c, title).FontSize(typ.Title).Bold()
			if subtitle != "" {
				ui.Text(c, subtitle).FontSize(typ.BodySmall).TextColor(t.TextMuted)
			}
		})
}

func fallbackText(values ...string) string {
	for _, value := range values {
		if value = strings.TrimSpace(value); value != "" {
			return value
		}
	}
	return ""
}

func compactSearchPath(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return ""
	}
	parts := strings.Split(value, "/")
	if len(parts) > 3 && parts[1] == "Users" {
		return "~/" + strings.Join(parts[3:], "/")
	}
	return value
}
