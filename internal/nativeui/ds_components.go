package nativeui

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

// Design System v2 shared page primitives (DS-03). Every surface composes
// these instead of restating sizes, radii, spacing and status colors.

// Readable column widths. A page's content never stretches past its cap: a
// wide window grows equal margins on both sides instead of long lines or a
// one-sided gap. The caps live here, once.
const (
	// formColumnWidth bounds Settings pages, inspectors and the commit form.
	formColumnWidth = settingsContentMaxWidth
	// listColumnWidth bounds list pages (Sessions): rows need more room than
	// a form to carry their metrics column.
	listColumnWidth float32 = 1040
)

// contentColumn lays body out in a column at most maxWidth wide, centered in
// whatever width the page has. A narrower page simply fills it.
func contentColumn(c *ui.Context, maxWidth float32, body func()) {
	ui.Column(c).FillWidth().AlignItems(ui.Center).Children(func() {
		ui.Column(c).FillWidth().MaxWidth(maxWidth).Children(body)
	})
}

// segmentedTrack dresses the track of a ui.SegmentedBase in ui.Segmented's own
// look — surface fill, 1px outline, concentric radii — and segmentFace one of
// its segments: the chosen one a chip raised from the track (the page color on
// the light theme's white track, the pressed surface on the dark one — what
// ui.Segmented itself paints), the hovered one a wash. A segmented control
// whose segments are icons or counts (ui.Segmented takes labels only) is built
// from these two, so every segmented control in the app shares one face and
// one height.
func segmentedTrack(c *ui.Context, tabs ui.SegmentedParts, label string) {
	t := c.Theme()
	tabs.Track.Label(label).AlignItems(ui.Center).Shrink(0).Padding(t.Space(0.5)).Gap(t.Space(0.5)).
		Radius(t.Radius).Background(t.Surface).Border(1, t.Border)
}

func segmentFace(c *ui.Context, tabs ui.SegmentedParts, i int, chosen bool, label string) ui.Element {
	t := c.Theme()
	seg := tabs.Segment(i).Padding(t.Space(1), t.Space(3)).Gap(Spacing().S).
		Radius(t.Radius - t.Space(0.5)).AlignItems(ui.Center).Label(label)
	switch {
	case chosen:
		chip := t.Background
		if t.Dark {
			chip = t.SurfacePressed
		}
		seg.Background(chip).Shadow(0, 1, 2, 0, ui.RGBA(0, 0, 0, 0.12))
	case seg.Hovered():
		seg.Background(t.SurfaceHover)
	}
	return seg
}

// pageTopInset is the gap between the title bar and a page's first row. The
// title bar owns every page title, so pages start with content and share
// this one inset instead of each spacing itself from a heading it lost.
func pageTopInset() float32 { return Spacing().L }

// sectionHeader labels one card/section.
func sectionHeader(c *ui.Context, label string) {
	typ := Typography()
	sp := Spacing()
	ui.Text(c, label).FontSize(typ.Micro).FontWeight(WeightBold).
		TextColor(c.Theme().TextMuted).Padding(0, 0, sp.XS)
}

// groupLabel is the quiet caption above a card: small, muted, medium weight.
func groupLabel(c *ui.Context, label string) {
	ui.Text(c, label).FontSize(Typography().BodySmall).FontWeight(WeightMedium).
		TextColor(c.Theme().TextMuted).Padding(0, 0, 0, Spacing().XS)
}

// settingsCard is the shared grouping card for free-form content: a quiet
// label above a padded, hairline-bordered panel.
func settingsCard(c *ui.Context, title string, body func()) {
	sp := Spacing()
	tokens := designTokens(c.Theme().Dark)
	ui.Column(c).FillWidth().Gap(sp.S).Children(func() {
		if title != "" {
			groupLabel(c, title)
		}
		ui.Column(c).FillWidth().Radius(Radius().Card).Background(tokens.Panel).
			Border(1, tokens.BorderSubtle).Padding(sp.L).Gap(sp.M).Children(body)
	})
}

// settingsRow is one line of a settingsGroup: an optional lead (an icon or
// brand mark), the label with an optional quiet note, then the trailing
// control. An attached row continues the row above it, with no divider.
type settingsRow struct {
	lead        func()
	label, note string
	control     func()
	attached    bool
}

// row builds a settingsRow.
func row(label, note string, control func()) settingsRow {
	return settingsRow{label: label, note: note, control: control}
}

// settingsGroup is the grouped list used by every settings-like surface: a
// quiet label above one rounded panel whose rows are split by inset
// hairlines, as in macOS' own Settings.
func settingsGroup(c *ui.Context, title string, rows ...settingsRow) {
	sp := Spacing()
	tokens := designTokens(c.Theme().Dark)
	ui.Column(c).FillWidth().Gap(sp.S).Children(func() {
		if title != "" {
			groupLabel(c, title)
		}
		ui.Column(c).FillWidth().Radius(Radius().Card).Background(tokens.Panel).
			Border(1, tokens.BorderSubtle).Clip().Children(func() {
			for i, r := range rows {
				if i > 0 && !r.attached {
					ui.Row(c).FillWidth().Height(1).Shrink(0).Children(func() {
						ui.Box(c).Width(rowInset).Shrink(0)
						ui.Box(c).Grow(1).FillHeight().Background(tokens.BorderSubtle)
					})
				}
				formRowLead(c, r.lead, r.label, r.note, r.control)
			}
		})
	})
}

// rowInset is the horizontal padding of a grouped row; dividers start at it.
const rowInset float32 = 14

// formRow is one labeled row: the label leads at regular weight, an optional
// note sits under it in muted caption type, and the control trails.
func formRow(c *ui.Context, label, detail string, control func()) {
	formRowLead(c, nil, label, detail, control)
}

// formRowLead is formRow with a leading icon or mark. The row is built
// control-first and laid out in reverse, so the control precedes its label in
// focus and reading order — the order macOS' own switch and field rows use.
func formRowLead(c *ui.Context, lead func(), label, detail string, control func()) {
	t := c.Theme()
	typ := Typography()
	sp := Spacing()
	ui.Row(c).FillWidth().MinHeight(Controls().RowHeight+8).Padding(sp.M, rowInset).Gap(sp.L).AlignItems(ui.Center).Reverse().Children(func() {
		if control != nil {
			control()
		}
		ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
			ui.Text(c, label).FontSize(typ.Body)
			if detail != "" {
				ui.Text(c, detail).FontSize(typ.Caption).TextColor(t.TextMuted)
			}
		})
		if lead != nil {
			lead()
		}
	})
}

// valueTextMaxWidth caps a fact row's value, so a long one gives way with an
// ellipsis instead of squeezing its label.
const valueTextMaxWidth float32 = 360

// valueText is the muted trailing value of a fact row: one selectable line
// that clips long values and keeps their full text in the tooltip.
func valueText(c *ui.Context, value string) {
	ui.Text(c, value).FontSize(Typography().Body).TextColor(c.Theme().TextMuted).
		SingleLine().Ellipsis("…").Shrink(1).MinWidth(0).MaxWidth(valueTextMaxWidth).Selectable().Tooltip(value)
}

// iconTile paints a glyph in white on a rounded system-color square: the
// colored marker of a destination or category.
func iconTile(c *ui.Context, glyph *ui.SVG, tone TileTone, size float32) {
	ui.Box(c).Size(size, size).Shrink(0).Radius(size * 0.26).
		Background(tone.Color(c.Theme().Dark)).Center().Children(func() {
		ui.Icon(c, glyph).Size(size*0.62, size*0.62).TextColor(ui.Hex("#ffffff")).PassThrough()
	})
}

// compactButton is the secondary action of dense surfaces — Git toolbars,
// list rows, popover menus: a regular Button at the footnote size. Surfaces
// never restate that size on a Button; the dense tier changes here, once.
func compactButton(c *ui.Context, label string) ui.Element {
	return ui.Button(c, label).FontSize(Typography().Micro)
}

// compactPrimaryButton is compactButton's accent-filled form.
func compactPrimaryButton(c *ui.Context, label string) ui.Element {
	return ui.PrimaryButton(c, label).FontSize(Typography().Micro)
}

// emptyState is the shared nothing-here state.
func emptyState(c *ui.Context, title, detail string) {
	t := c.Theme()
	typ := Typography()
	sp := Spacing()
	ui.Column(c).Grow(1).Center().Gap(sp.XS).Children(func() {
		ui.Text(c, title).FontWeight(WeightSemibold)
		if detail != "" {
			ui.Text(c, detail).FontSize(typ.BodySmall).TextColor(t.TextMuted)
		}
	})
}

// workspaceEmptyBanner is a compact, top-aligned empty state for the
// Terminal-adjacent Chat/Diff surfaces. It uses the same layout and tokens
// everywhere, instead of a centered modal-like card in the main canvas.
func workspaceEmptyBanner(c *ui.Context, glyph *ui.SVG, title, detail string, actions func()) {
	const emptyBannerMessageBasis float32 = 200 // the least width the message keeps before the actions wrap below it
	t := c.Theme()
	tokens := designTokens(t.Dark)
	sp := Spacing()
	ui.Column(c).Grow(1).MinWidth(0).Padding(sp.XL).Children(func() {
		// Wrap + Basis: Grow alone starts the message from no width, so the
		// row never wrapped and a narrow column squeezed it into single
		// letters beside the actions. With a basis the actions drop below it.
		ui.Row(c).FillWidth().Gap(sp.L).Padding(sp.L).Wrap().
			BorderWidth(0, 0, 1, 0).BorderColor(tokens.BorderSubtle).
			AlignItems(ui.Center).Children(func() {
			ui.Icon(c, glyph).Size(18, 18).TextColor(t.TextMuted).Shrink(0)
			ui.Column(c).Grow(1).Basis(emptyBannerMessageBasis).MinWidth(0).Gap(sp.XS).Children(func() {
				ui.Text(c, title).FontSize(Typography().Section).FontWeight(WeightSemibold)
				if detail != "" {
					ui.Text(c, detail).FontSize(Typography().BodySmall).TextColor(t.TextMuted)
				}
			})
			if actions != nil {
				ui.Row(c).Gap(sp.S).AlignItems(ui.Center).Children(actions)
			}
		})
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
func inlineNotice(c *ui.Context, tone StatusTone, text string, onRetry func()) ui.Element {
	t := c.Theme()
	dark := t.Dark
	typ := Typography()
	sp := Spacing()
	var view ui.Element
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
		ui.Text(c, text).FontSize(typ.Micro).FontWeight(WeightSemibold).TextColor(color)
	})
}

// providerBadge shows the provider display name with a stable neutral pill.
func providerBadge(c *ui.Context, agent history.AgentID) {
	statusPill(c, agent.DisplayName(), ToneNeutral)
}

// toolCard is the shared tool call/result card, reusable by History detail
// and future Chat.
func toolCard(c *ui.Context, name string, inputPreview string, output *string, isError bool) ui.Element {
	t := c.Theme()
	typ := Typography()
	sp := Spacing()
	var view ui.Element
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
				preview := clipPreview(*output, 400, 4)
				ui.Text(c, preview).FontSize(typ.Caption).TextColor(t.TextMuted)
			}
		})
	})
	return view
}
