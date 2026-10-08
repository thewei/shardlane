package nativeui

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

// Design System v2 shared page primitives (DS-03). Every surface composes
// these instead of restating sizes, radii, spacing and status colors.

// appPage is the standard content page: tokenized header plus body.
func appPage(c *ui.Context, title, subtitle string, body func()) {
	ui.Column(c).Grow(1).MinWidth(0).Background(designTokens(c.Theme().Dark).Content).Children(func() {
		pageHeader(c, title, subtitle)
		body()
	})
}

// sectionHeader labels one card/section.
func sectionHeader(c *ui.Context, label string) {
	typ := Typography()
	sp := Spacing()
	ui.Text(c, label).FontSize(typ.Micro).FontWeight(700).
		TextColor(c.Theme().TextMuted).Padding(0, 0, sp.XS)
}

// settingsCard is the shared settings/grouping card.
func settingsCard(c *ui.Context, title string, body func()) {
	t := c.Theme()
	typ := Typography()
	sp := Spacing()
	ui.Column(c).FillWidth().Radius(Radius().Card).Background(designTokens(t.Dark).Panel).
		Border(1, designTokens(t.Dark).BorderSubtle).Padding(sp.L).Gap(sp.M).Children(func() {
		if title != "" {
			ui.Text(c, title).FontSize(typ.Section).FontWeight(650)
		}
		body()
	})
}

// formRow is one labeled preference/control row.
func formRow(c *ui.Context, label, detail string, control func()) {
	t := c.Theme()
	typ := Typography()
	sp := Spacing()
	ui.Row(c).FillWidth().Gap(sp.L).AlignItems(ui.Center).Children(func() {
		ui.Column(c).Width(240).Shrink(0).Gap(2).Children(func() {
			ui.Text(c, label).FontSize(typ.Body).FontWeight(600)
			if detail != "" {
				ui.Text(c, detail).FontSize(typ.Caption).TextColor(t.TextMuted)
			}
		})
		control()
	})
}

// emptyState is the shared nothing-here state.
func emptyState(c *ui.Context, title, detail string) {
	t := c.Theme()
	typ := Typography()
	sp := Spacing()
	ui.Column(c).Grow(1).Center().Gap(sp.XS).Children(func() {
		ui.Text(c, title).Bold()
		if detail != "" {
			ui.Text(c, detail).FontSize(typ.BodySmall).TextColor(t.TextMuted)
		}
	})
}

// loadingState is the shared in-flight state.
func loadingState(c *ui.Context, text string) {
	t := c.Theme()
	sp := Spacing()
	ui.Column(c).Grow(1).Center().Gap(sp.M).Children(func() {
		ui.Spinner(c).Size(24, 24)
		ui.Text(c, text).TextColor(t.TextMuted)
	})
}

// inlineNotice is the shared one-line status notice; a non-nil onRetry adds
// the retry action. It reports the element so callers can place it.
func inlineNotice(c *ui.Context, tone StatusTone, text string, onRetry func()) *ui.Element {
	t := c.Theme()
	dark := t.Dark
	typ := Typography()
	sp := Spacing()
	var view *ui.Element
	ui.Box(c).Children(func() {
		view = ui.Row(c).FillWidth().Padding(sp.S, sp.L).
			Background(designTokens(dark).StatusBackground(tone, dark)).
			BorderWidth(0, 0, 1, 0).
			BorderColor(designTokens(dark).StatusColor(tone, dark).Alpha(0.35)).
			Gap(sp.M).AlignItems(ui.Center).Children(func() {
			statusGlyph(c, tone)
			ui.Text(c, text).FontSize(typ.BodySmall).
				TextColor(designTokens(dark).StatusColor(tone, dark)).Grow(1).SingleLine()
			if onRetry != nil {
				if ui.Button(c, "Retry").Clicked() {
					onRetry()
				}
			}
		})
	})
	return view
}

// statusGlyph is the shared semantic status dot.
func statusGlyph(c *ui.Context, tone StatusTone) {
	dark := c.Theme().Dark
	ui.Box(c).Size(8, 8).Radius(Radius().Control).
		Background(designTokens(dark).StatusColor(tone, dark)).Shrink(0)
}

// statusPill is the shared semantic status badge: one vocabulary for Agent
// rows, aggregates, Titlebar Activity and future Chat.
func statusPill(c *ui.Context, text string, tone StatusTone) {
	dark := c.Theme().Dark
	tokens := designTokens(dark)
	typ := Typography()
	sp := Spacing()
	color := tokens.StatusColor(tone, dark)
	ui.Row(c).Gap(sp.XXS).AlignItems(ui.Center).
		Radius(Radius().Row).Padding(sp.XXS, sp.S).
		Background(tokens.StatusBackground(tone, dark)).Shrink(0).Children(func() {
		ui.Box(c).Size(6, 6).Radius(3).Background(color).Shrink(0)
		ui.Text(c, text).FontSize(typ.Micro).FontWeight(650).TextColor(color)
	})
}

// providerBadge shows the provider display name with a stable neutral pill.
func providerBadge(c *ui.Context, agent history.AgentID) {
	statusPill(c, agent.DisplayName(), ToneNeutral)
}

// agentRow is the shared Agent row: label plus derived status pill.
func agentRow(c *ui.Context, label string, status string) *ui.Element {
	sp := Spacing()
	typ := Typography()
	tone := operationalTone(normalizeRuntimeStatus(status))
	var view *ui.Element
	ui.Box(c).Children(func() {
		view = ui.Row(c).FillWidth().Padding(sp.XS, sp.S).Gap(sp.S).AlignItems(ui.Center).Children(func() {
			ui.Text(c, fallbackText(label, "Agent")).FontSize(typ.Body).Grow(1).SingleLine()
			statusPill(c, operationalLabel(normalizeRuntimeStatus(status)), tone)
		})
	})
	return view
}

// toolCard is the shared tool call/result card, reusable by History detail
// and future Chat.
func toolCard(c *ui.Context, name string, inputPreview string, output *string, isError bool) *ui.Element {
	t := c.Theme()
	typ := Typography()
	sp := Spacing()
	var view *ui.Element
	ui.Box(c).Children(func() {
		view = ui.Column(c).FillWidth().Padding(7, 9).Gap(3).
			Radius(Radius().Control).Background(t.Surface).Children(func() {
			ui.Row(c).Gap(sp.S).Children(func() {
				statusPill(c, fallbackText(name, "tool"), ToneInfo)
				if isError {
					statusPill(c, "error", ToneError)
				}
			})
			ui.Text(c, fallbackText(inputPreview, name)).FontSize(typ.Caption).TextColor(t.TextMuted).SingleLine()
			if output != nil && *output != "" {
				preview, _ := clipPreview(*output, 400, 4)
				ui.Text(c, preview).FontSize(typ.Caption).TextColor(t.TextMuted)
			}
		})
	})
	return view
}
