package nativeui

import (
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
)

// The gorex visual language (examples/gorex), adopted by the Shardlane
// workspace on 2026-10-06: a soft multi-stop gradient window background,
// translucent rounded terminal cards with in-card pane headers, and
// borderless round icon buttons. Presentation only — Herdr runtime facts,
// colors included, are never authored here; these palettes style surfaces,
// and the terminal palettes feed the official MyGo terminal plugin.

// gorexColors is the shared surface palette, in light and dark windows.
type gorexColors struct {
	// The window's background: a gradient from top through mid to bottom,
	// and a tint toward the right.
	bgTop, bgMid, bgLow, bgBottom, bgTint ui.Color

	card, cardFocused, cardBorder, cardBorderFocused ui.Color
	// cardTranslucent is the pane-card paper when the terminal background
	// itself is transparent (Settings → Terminal): a see-through fill that
	// lets the window gradient read through the undrawn terminal
	// background.
	cardTranslucent       ui.Color
	shadow, shadowFocused ui.Color

	text, textMuted, textFaint, iconMuted ui.Color
	hover, pressed                        ui.Color

	// tabActive fills a selected icon chip (the surface switch).
	tabActive ui.Color

	busy, attention ui.Color
}

var gorexLightColors = gorexColors{
	bgTop:    ui.Hex("#f6e7f6"),
	bgMid:    ui.Hex("#efe1e6"),
	bgLow:    ui.Hex("#eee0d3"),
	bgBottom: ui.Hex("#ece1bd"),
	bgTint:   ui.RGBA(242, 214, 222, 0.35),

	card:              ui.RGBA(255, 255, 255, 0.66),
	cardFocused:       ui.RGBA(255, 255, 255, 0.95),
	cardTranslucent:   ui.RGBA(255, 255, 255, 0.58),
	cardBorder:        ui.RGBA(255, 255, 255, 0.72),
	cardBorderFocused: ui.RGBA(255, 255, 255, 1),
	shadow:            ui.RGBA(60, 40, 50, 0.07),
	shadowFocused:     ui.RGBA(60, 40, 50, 0.12),

	text:      ui.Hex("#1d1d1f"),
	textMuted: ui.Hex("#636366"),
	textFaint: ui.Hex("#8e8e93"),
	iconMuted: ui.Hex("#8a8a8f"),
	hover:     ui.RGBA(0, 0, 0, 0.055),
	pressed:   ui.RGBA(0, 0, 0, 0.1),

	tabActive: ui.RGBA(255, 255, 255, 0.97),

	busy:      ui.Hex("#34a853"),
	attention: ui.Hex("#f59e0b"),
}

var gorexDarkColors = gorexColors{
	bgTop:    ui.Hex("#2c2131"),
	bgMid:    ui.Hex("#231e27"),
	bgLow:    ui.Hex("#221f22"),
	bgBottom: ui.Hex("#2a2619"),
	bgTint:   ui.RGBA(80, 40, 60, 0.25),

	card:              ui.RGBA(20, 20, 24, 0.5),
	cardFocused:       ui.RGBA(36, 36, 41, 0.94),
	cardTranslucent:   ui.RGBA(36, 36, 41, 0.6),
	cardBorder:        ui.RGBA(255, 255, 255, 0.05),
	cardBorderFocused: ui.RGBA(255, 255, 255, 0.14),
	shadow:            ui.RGBA(0, 0, 0, 0.25),
	shadowFocused:     ui.RGBA(0, 0, 0, 0.4),

	text:      ui.Hex("#f2f2f7"),
	textMuted: ui.Hex("#aeaeb2"),
	textFaint: ui.Hex("#8e8e93"),
	iconMuted: ui.Hex("#98989d"),
	hover:     ui.RGBA(255, 255, 255, 0.08),
	pressed:   ui.RGBA(255, 255, 255, 0.14),

	tabActive: ui.RGBA(255, 255, 255, 0.14),

	busy:      ui.Hex("#4ade80"),
	attention: ui.Hex("#fbbf24"),
}

func gorexColorsOf(dark bool) *gorexColors {
	if dark {
		return &gorexDarkColors
	}
	return &gorexLightColors
}

// Metrics of the gorex surfaces. gorexGap is the reference code's single
// spacing unit (its `gap`): the gutter between cards AND around them —
// every other spacing here derives from it.
const (
	gorexGap     = 8
	gorexCardR   = 12
	gorexHeaderH = 33
	gorexIconBtn = 30
	// Each card frame insets half the gutter, so adjacent cards keep a
	// full gorexGap between them; the canvas padding adds the other half,
	// completing the edge gutter (edges land on exactly gorexGap too).
	gorexFrameInset = gorexGap / 2
	gorexCardsPad   = gorexGap / 2
)

// paintGorexBackground paints the window's background gradient.
func paintGorexBackground(p *ui.Painter, r ui.Rect, k *gorexColors) {
	h1, h2 := r.H*0.38, r.H*0.32
	p.FillGradient(ui.Rect{X: r.X, Y: r.Y, W: r.W, H: h1 + 1}, ui.LinearGradient{From: k.bgTop, To: k.bgMid, Angle: 180}, 0)
	p.FillGradient(ui.Rect{X: r.X, Y: r.Y + h1, W: r.W, H: h2 + 1}, ui.LinearGradient{From: k.bgMid, To: k.bgLow, Angle: 180}, 0)
	p.FillGradient(ui.Rect{X: r.X, Y: r.Y + h1 + h2, W: r.W, H: r.H - h1 - h2}, ui.LinearGradient{From: k.bgLow, To: k.bgBottom, Angle: 180}, 0)
	clear := k.bgTint
	clear.A = 0
	p.FillGradient(r, ui.LinearGradient{From: clear, To: k.bgTint, Angle: 90, Start: 0.35, End: 1}, 0)
}

// gorexContentCard hosts one primary content surface (the Diff surface
// today) in a gorex card: the gradient shows around it at a full
// gorexGap, the card carries the surface.
func gorexContentCard(c *ui.Context, k *gorexColors, build func()) {
	ui.Box(c).Grow(1).MinWidth(0).Padding(gorexGap).Children(func() {
		card := ui.Column(c).Grow(1).MinWidth(0).Radius(gorexCardR).Clip().
			Background(k.cardFocused).Border(1, k.cardBorderFocused).
			Shadow(0, 1, 2, 0, k.shadowFocused).
			Shadow(0, 6, 22, -2, k.shadowFocused)
		card.Transition(ui.ElementTransition{Colors: true, Duration: 160 * time.Millisecond})
		card.Children(build)
	})
}

// gorexIconButton is a borderless button of an icon, with a face on hover;
// label names it for screen readers and its tooltip explains it. Selected
// keeps a quiet filled face, as the surface switch's current side does.
func gorexIconButton(c *ui.Context, k *gorexColors, svg *ui.SVG, label, tooltip string, selected bool, size, iconSize float32) ui.Element {
	b := ui.Box(c).Size(size, size).Center().Radius(size / 2.6).Cursor(ui.CursorPointer).Role(ui.RoleButton).Label(label)
	if tooltip != "" {
		b.Tooltip(tooltip)
	}
	switch {
	case b.Pressed():
		b.Background(k.pressed)
	case b.Hovered():
		b.Background(k.hover)
	case selected:
		b.Background(k.tabActive)
	}
	b.Transition(ui.ElementTransition{Colors: true, Duration: 120 * time.Millisecond})
	col := k.iconMuted
	if b.Hovered() || selected {
		col = k.text
	}
	b.Children(func() {
		ui.Icon(c, svg).Size(iconSize, iconSize).TextColor(col)
	})
	return b
}

// gorexActivityDot is the dot of something running: a solid dot in a soft
// halo. It does not animate, as drawing frames all along would cost more
// than it tells.
func gorexActivityDot(c *ui.Context, col ui.Color, size float32) ui.Element {
	e := ui.Box(c).Size(size+6, size+6).Margin(0, 0, 0, 1)
	e.Draw(func(p *ui.Painter, r ui.Rect) {
		cx, cy := r.X+r.W/2, r.Y+r.H/2
		halo := size/2 + 2.5
		p.Fill(ui.Rect{X: cx - halo, Y: cy - halo, W: 2 * halo, H: 2 * halo}, col.Alpha(0.22), halo)
		p.Fill(ui.Rect{X: cx - size/2, Y: cy - size/2, W: size, H: size}, col, size/2)
	})
	return e
}

// gorexAttentionDot is the plain dot of a Pane that needs someone.
func gorexAttentionDot(c *ui.Context, col ui.Color, tooltip string) ui.Element {
	return ui.Box(c).Size(7, 7).Radius(4).Background(col).Margin(0, 0, 0, 2).Tooltip(tooltip)
}

// gorexTerminalThemes are the terminals' colors: soft ink on the panes'
// paper, in both window kinds.
func gorexTerminalThemes() (light, dark *terminal.Theme) {
	return &terminal.Theme{
		Foreground: ui.Hex("#2a2d31"),
		Background: ui.Hex("#fbfbfb"),
		Cursor:     ui.Hex("#3a3d42"),
		Selection:  ui.RGBA(46, 111, 208, 0.2),
		Palette: [16]ui.Color{
			ui.Hex("#2a2d31"), ui.Hex("#c2465a"), ui.Hex("#4e8e5f"), ui.Hex("#b07a1e"),
			ui.Hex("#2e6fd0"), ui.Hex("#9050c8"), ui.Hex("#1e8c9c"), ui.Hex("#8a8d93"),
			ui.Hex("#6e7178"), ui.Hex("#d9566b"), ui.Hex("#5ba671"), ui.Hex("#c89026"),
			ui.Hex("#4a8fe0"), ui.Hex("#a56bd8"), ui.Hex("#2ba2b3"), ui.Hex("#b5b8be"),
		},
	}, &terminal.Theme{
		Foreground: ui.Hex("#e6e6ea"),
		Background: ui.Hex("#1e1e22"),
		Cursor:     ui.Hex("#e6e6ea"),
		Selection:  ui.RGBA(108, 178, 255, 0.28),
		Palette: [16]ui.Color{
			ui.Hex("#3a3a40"), ui.Hex("#ff6b7f"), ui.Hex("#7bd88f"), ui.Hex("#e5c07b"),
			ui.Hex("#6cb2ff"), ui.Hex("#c792ea"), ui.Hex("#56d4dd"), ui.Hex("#d0d0d6"),
			ui.Hex("#6e6e78"), ui.Hex("#ff8c9c"), ui.Hex("#9be8a8"), ui.Hex("#f2d28a"),
			ui.Hex("#8ec5ff"), ui.Hex("#d7a8f2"), ui.Hex("#7fe3ea"), ui.Hex("#ffffff"),
		},
	}
}

// applyGorexTerminalTheme points one attach's options at the gorex
// palettes. An explicit appearance pins one theme; the system default
// lets the plugin follow the window's appearance, as Ghostty's
// light:…,dark:… does.
func applyGorexTerminalTheme(options *terminal.Options, appearance string) {
	light, dark := gorexTerminalThemes()
	switch appearance {
	case "light":
		options.Theme = light
	case "dark":
		options.DarkTheme = dark
	default:
		options.Theme, options.DarkTheme = light, dark
	}
}

// tildePath shortens an absolute path under the home directory to a ~
// form, as pane headers and path rows show it.
func tildePath(path string) string {
	if path == "" {
		return ""
	}
	home, err := os.UserHomeDir()
	if err != nil || home == "" || !filepath.IsAbs(path) {
		return path
	}
	if path == home {
		return "~"
	}
	if rel, err := filepath.Rel(home, path); err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return filepath.Join("~", rel)
	}
	return path
}
