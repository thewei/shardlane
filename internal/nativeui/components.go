package nativeui

import (
	"strings"

	"github.com/egoist/mygo/ui"
)

func iconButton(c *ui.Context, glyph *ui.SVG, label string) *ui.Element {
	t := c.Theme()
	b := ui.ButtonBase(c).
		Size(28, 28).
		Radius(6).
		Label(label).
		Tooltip(label)
	if b.Hovered() {
		b.Background(t.SurfaceHover)
	}
	b.Children(func() {
		ui.Icon(c, glyph).Size(14, 14).TextColor(t.TextMuted)
	})
	return b
}

// rowTint is the shared background policy for selectable rows: selection
// outranks hover, and an untinted row reports ok=false so callers keep the
// surface transparent.
func rowTint(tokens DesignTokens, selected, hovered bool) (ui.Color, bool) {
	switch {
	case selected:
		return tokens.SidebarSelect, true
	case hovered:
		return tokens.SidebarHover, true
	default:
		return ui.Color{}, false
	}
}

func navButton(c *ui.Context, glyph *ui.SVG, label, shortcut string, selected bool) *ui.Element {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	b := ui.ButtonBase(c).
		FillWidth().
		Height(31).
		PaddingX(8).
		Gap(7).
		Radius(6).
		Label(label)
	if tint, ok := rowTint(tokens, selected, b.Hovered()); ok {
		b.Background(tint)
	}
	b.Children(func() {
		if glyph != nil {
			ui.Icon(c, glyph).Size(14, 14).TextColor(t.TextMuted)
		}
		ui.Text(c, label).FontSize(11).Grow(1).SingleLine()
		if shortcut != "" {
			ui.Text(c, shortcut).FontSize(10).TextColor(t.TextMuted).SingleLine()
		}
	})
	return b
}

func sectionLabel(c *ui.Context, label string) {
	ui.Text(c, label).
		Padding(7, 8, 4).
		FontSize(9.5).
		FontWeight(650).
		TextColor(c.Theme().TextMuted)
}

func panelCard(c *ui.Context, children func()) *ui.Element {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	return ui.Column(c).
		Radius(8).
		Background(tokens.Panel).
		Border(1, tokens.BorderSubtle).
		Padding(12).
		Gap(8).
		Children(children)
}

type treeRowSpec struct {
	Key               string
	Depth             int
	AncestorContinues []bool
	Last              bool
	Selected          bool
	Expandable        bool
	Expanded          bool
	Mark              visualMark
	Label             string
	Status            string
	// Dot renders the bound Agent's state as a small dot at the icon's
	// corner (agent Panes); opUnknown leaves the icon plain.
	Dot operationalState
	// Activity rolls subtree state into the same icon-corner dot on
	// ancestor rows: a service (green) or agents running below this row
	// (2026-10-06 sidebar service rollup), with a tooltip naming what runs.
	Activity sidebarActivity
	// Overflow builds the row's "..." menu, shown at the trailing edge
	// under the pointer (2026-10-05 sidebar review round two). It shares
	// the builders behind the title-bar overflow menu.
	Overflow func(m *ui.Menu)
	// Count is the number of panes beneath a collapsed nesting row
	// (2026-10-07 user request): more than one shows as tiny muted digits
	// at the trailing edge, yielding to the hover controls. Callers pass
	// it only while the row is collapsed — an expanded row shows its
	// children, not a tally. 0 and 1 render nothing.
	Count int
	// Height overrides the row height; 0 uses the density default.
	Height float32
}

// designRowHeight resolves the row height token.
func designRowHeight(override float32) float32 {
	if override > 0 {
		return override
	}
	return 28
}

// sidebarRowHeight returns the density-token row height for the canonical
// sidebar (GWB-063): compact 26 / default 28 / comfortable 32 DIP.
func (s *Shell) sidebarRowHeight() float32 {
	switch s.sidebarDensity() {
	case DensityCompact:
		return 26
	case DensityComfortable:
		return 32
	default:
		return 28
	}
}

// sidebarTopAction is one compact icon control in the sidebar top actions
// row (GWB-060): icon-only with accessible label + tooltip.
func sidebarTopAction(c *ui.Context, glyph *ui.SVG, label, shortcut string, selected bool) *ui.Element {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	b := ui.ButtonBase(c).
		Size(28, 28).
		Radius(6).
		Label(label).
		Tooltip(strings.TrimSpace(label + " " + shortcut))
	if tint, ok := rowTint(tokens, selected, b.Hovered()); ok {
		b.Background(tint)
	}
	b.Children(func() {
		ui.Icon(c, glyph).Size(14, 14).TextColor(t.TextMuted).PassThrough()
	})
	return b
}

// cornerMark resolves the ONE state dot a row shows at its leading icon's
// top-right corner (2026-10-06 annotation round): an Agent Pane's own state
// first (gray means idle, as in the Agents section), then the rolled-up
// activity (a service or agents running beneath an ancestor row), then the
// raw runtime Status. The trailing status dot and the bottom-right activity
// dot this replaces used to stack at the row's right edge, misaligned and
// competing.
func (spec treeRowSpec) cornerMark() (StatusTone, string, bool) {
	if spec.Dot != opUnknown {
		tone := operationalTone(spec.Dot)
		if spec.Dot == opIdle {
			tone = ToneMuted
		}
		return tone, operationalLabel(spec.Dot), true
	}
	if spec.Activity.On {
		return spec.Activity.Tone, spec.Activity.Detail, true
	}
	if state := normalizeRuntimeStatus(spec.Status); state != opIdle && state != opUnknown {
		return operationalTone(state), operationalLabel(state), true
	}
	return ToneNeutral, "", false
}

// treeRow draws a compact Finder/Xcode-style hierarchy row with connector
// guides. Children are pointer-pass-through so clicks/context menus belong to
// the whole row rather than only its empty padding.
func treeRow(c *ui.Context, spec treeRowSpec) *ui.Element {
	t := c.Theme()
	tokens := designTokens(t.Dark)
	var row *ui.Element
	height := designRowHeight(spec.Height)
	ui.Box(c).Key(spec.Key).Children(func() {
		row = ui.ButtonBase(c).
			FillWidth().
			Height(height).
			Radius(6).
			Padding(0, 7, 0, 6).
			Label(spec.Label)
		if tint, ok := rowTint(tokens, spec.Selected, row.Hovered()); ok {
			row.Background(tint)
		}
		row.Draw(func(p *ui.Painter, r ui.Rect) {
			if spec.Depth <= 0 {
				return
			}
			const step float32 = 14
			const origin float32 = 9
			mid := r.Y + r.H/2
			for level, continues := range spec.AncestorContinues {
				if !continues {
					continue
				}
				x := r.X + origin + float32(level)*step
				p.Line(x, r.Y, x, r.Y+r.H, 1, tokens.TreeLine)
			}
			x := r.X + origin + float32(spec.Depth-1)*step
			bottom := r.Y + r.H
			if spec.Last {
				bottom = mid
			}
			p.Line(x, r.Y, x, bottom, 1, tokens.TreeLine)
			p.Line(x, mid, x+8, mid, 1, tokens.TreeLine)
		})
		row.Children(func() {
			// Indent only — the trailing chevron replaced the leading one,
			// so rows sit flush with the section edge (2026-10-05 round 2).
			ui.Box(c).Width(float32(spec.Depth) * 14).Shrink(0).PassThrough()
			if spec.Mark.svg != nil || spec.Mark.bmp != nil {
				// The 16pt frame holds the 14pt mark with room for the single
				// state dot overlaid at the mark's top-right corner
				// (2026-10-06 annotation round): absolutely positioned on the
				// icon, never floated at the row's trailing edge.
				ui.Box(c).Size(16, 16).Shrink(0).PassThrough().Children(func() {
					markView(c, spec.Mark, 14, t.TextMuted)
					if tone, detail, ok := spec.cornerMark(); ok {
						dot := ui.Box(c).Absolute().Left(9).Top(0).Size(7, 7).Radius(3.5).
							Background(tokens.StatusColor(tone, t.Dark))
						if detail != "" {
							dot.Tooltip(detail)
						}
					}
				})
				// Breathe between the glyph and its label (2026-10-05).
				ui.Box(c).Width(5).Shrink(0).PassThrough()
			}
			ui.Text(c, spec.Label).FontSize(11).Grow(1).SingleLine().PassThrough()
			hovered := row.Hovered()
			// The collapsed pane tally (2026-10-07): tiny muted digits at
			// the trailing edge; under the pointer it steps aside for the
			// chevron and the "..." menu.
			if spec.Count > 1 && !hovered {
				ui.Text(c, itoa(spec.Count)).Label(itoa(spec.Count) + " panes").
					FontSize(Typography().Micro).TextColor(t.TextMuted).
					Shrink(0).PassThrough()
			}
			if spec.Expandable && hovered {
				glyph := iconChevron
				if spec.Expanded {
					glyph = iconChevronDown
				}
				ui.Icon(c, glyph).Size(11, 11).TextColor(t.TextMuted).Shrink(0).PassThrough()
			}
			if spec.Overflow != nil && hovered {
				// The row's "..." — same builders as the title-bar overflow.
				more := ui.ButtonBase(c).
					Size(18, 18).
					Radius(4).
					Label("More actions for " + spec.Label).
					Shrink(0)
				if more.Hovered() {
					more.Background(t.SurfaceHover)
				}
				more.Children(func() {
					ui.Icon(c, iconEllipsis).Size(11, 11).TextColor(t.TextMuted)
				})
				more.Menu(spec.Overflow)
			}
		})
	})
	if spec.Activity.On && spec.Activity.Detail != "" {
		row.Tooltip(spec.Activity.Detail)
	}
	return row
}
