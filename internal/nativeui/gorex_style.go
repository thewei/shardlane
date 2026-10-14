package nativeui

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"

	"github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 github.com/egoist/mygo/plugins/terminal, github.com/egoist/mygo/ui, os, path/filepath, runtime
 * [OUTPUT]: 对外提供 gorexColors/gorexColorsOf/gorexContentCard/gorexToggleFace/gorexIconButton/gorexIconTab/gorexActivityDot/gorexAttentionDot/shortDir/samePath/tildePath
 * [POS]: nativeui 的窗口 chrome 与终端卡片视觉实现（扁平中性面、细线图标按钮），被 workspace/terminal/titlebar 等模块消费
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// The workspace chrome (gorex heritage, flattened on 2026-10-12): a flat
// neutral canvas under a vibrancy sidebar, terminal cards with in-card pane
// headers and a hairline edge, and borderless icon buttons. No gradients,
// no shadows — depth comes from surface contrast alone. Presentation only:
// Herdr runtime facts, colors included, are never authored here; these
// palettes style surfaces, and the terminal palettes feed the official MyGo
// terminal plugin.

// gorexColors is the shared surface palette, in light and dark windows.
type gorexColors struct {
	// cardPaper is the pane card's opaque surface; cardBorder its hairline.
	cardPaper, cardBorder ui.Color
	// cardTranslucent is the pane-card paper when the terminal background
	// itself is transparent (Settings → Terminal): a see-through fill that
	// lets what is behind the window read through the undrawn terminal
	// background.
	cardTranslucent ui.Color

	text, textMuted, textFaint, iconMuted ui.Color
	hover, pressed                        ui.Color

	// tabActive fills a selected icon chip (the surface switch).
	tabActive ui.Color

	busy, attention ui.Color
}

var gorexLightColors = gorexColors{
	cardPaper:       ui.Hex("#ffffff"),
	cardTranslucent: ui.RGBA(255, 255, 255, 0.58),
	cardBorder:      ui.RGBA(0, 0, 0, 0.09),

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
	cardPaper:       ui.Hex("#1e1e22"),
	cardTranslucent: ui.RGBA(30, 30, 34, 0.6),
	cardBorder:      ui.RGBA(255, 255, 255, 0.08),

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

// Increase contrast keeps every surface and strengthens what is drawn on it:
// the card's hairline and the secondary text and icon tones.
var (
	gorexLightHighContrast = func() gorexColors {
		k := gorexLightColors
		k.cardBorder = ui.RGBA(0, 0, 0, 0.35)
		k.textMuted, k.textFaint, k.iconMuted = ui.Hex("#3f3f46"), ui.Hex("#52525b"), ui.Hex("#52525b")
		return k
	}()
	gorexDarkHighContrast = func() gorexColors {
		k := gorexDarkColors
		k.cardBorder = ui.RGBA(255, 255, 255, 0.35)
		k.textMuted, k.textFaint, k.iconMuted = ui.Hex("#d4d4d8"), ui.Hex("#b0b0b8"), ui.Hex("#c4c4cc")
		return k
	}()
)

func gorexColorsOf(dark bool) *gorexColors {
	switch {
	case dark && highContrast.Load():
		return &gorexDarkHighContrast
	case dark:
		return &gorexDarkColors
	case highContrast.Load():
		return &gorexLightHighContrast
	}
	return &gorexLightColors
}

// Metrics of the gorex surfaces. gorexGap is the reference code's single
// spacing unit (its `gap`): the gutter between cards AND around them —
// every other spacing here derives from it.
const (
	gorexGap     = 8
	gorexCardR   = 10 // Radius().Card: pane cards and grouped panels share it
	gorexHeaderH = 34
	// Toolbar buttons are gorexIconBtn with a gorexIconGlyph glyph; the
	// denser pane-header and find-bar buttons use the smaller pair, and the
	// actions that appear inside a hovered list or tree row the smallest.
	gorexIconBtn   = 28
	gorexIconGlyph = 16
	gorexPaneBtn   = 24
	gorexPaneGlyph = 14
	gorexRowBtn    = 20
	gorexRowGlyph  = 12
	// Each card frame insets half the gutter, so adjacent cards keep a
	// full gorexGap between them; the canvas padding adds the other half,
	// completing the edge gutter (edges land on exactly gorexGap too).
	gorexFrameInset = gorexGap / 2
	gorexCardsPad   = gorexGap / 2

	// Split-divider drag gestures dispatch pane.resize in ratio points.
	// Gestures below the floor keep accumulating instead of spamming the
	// daemon with invisible nudges; a single dispatch never exceeds the
	// cap (the daemon itself clamps ratios to [0.1, 0.9]).
	resizeGestureFloor = float32(0.005)
	resizeDispatchCap  = float32(0.8)
)

// gorexContentCard hosts one primary content surface (Diff, Chat, History,
// Files, Settings) as the flat page itself: full-bleed in the content column,
// on the canvas color. There is no floating card, border or shadow around a
// page — only terminal panes are cards.
func gorexContentCard(c *ui.Context, k *gorexColors, build func()) {
	ui.Column(c).Grow(1).MinWidth(0).Clip().
		Background(designTokens(c.Theme().Dark).Content).Children(build)
}

// gorexToggleFace is the one set of state faces a button wears: pressed,
// hovered and — for a toggle that is on — the quiet filled chip. Icon
// buttons, the surface switch and text toggles all take it from here, so
// "on" reads the same on every page (2026-10-14 highlight audit: toggles had
// grown a blue tint, an accent tint and this chip). List and tree rows keep
// the sidebar selection tint instead — a row is selected, a button is on.
func gorexToggleFace(b ui.Element, k *gorexColors, on bool) {
	switch {
	case b.Pressed():
		b.Background(k.pressed)
	case b.Hovered():
		b.Background(k.hover)
	case on:
		b.Background(k.tabActive)
	}
}

// gorexIconButton is a borderless button of an icon, with a face on hover;
// label names it for screen readers and its tooltip explains it. Selected
// keeps a quiet filled face, as the surface switch's current side does.
func gorexIconButton(c *ui.Context, k *gorexColors, svg *ui.SVG, label, tooltip string, selected bool, size, iconSize float32) ui.Element {
	b := ui.Box(c).Size(size, size).Center().Radius(size / 2.6).Cursor(ui.CursorPointer).Role(ui.RoleButton).Label(label)
	return gorexIconFace(c, b, k, svg, tooltip, selected, iconSize)
}

// gorexIconTab is gorexIconButton for a strip of equal choices (the sidebar's
// view tabs): the same faces, but each tab takes an equal share of its row
// instead of a fixed square.
func gorexIconTab(c *ui.Context, k *gorexColors, svg *ui.SVG, label, tooltip string, selected bool) ui.Element {
	b := ui.Box(c).Grow(1).MinWidth(0).Height(gorexIconBtn).Center().Radius(Radius().Row).
		Cursor(ui.CursorPointer).Role(ui.RoleButton).Label(label)
	return gorexIconFace(c, b, k, svg, tooltip, selected, gorexIconGlyph)
}

// gorexIconFace dresses an icon button's box: tooltip, state face, and the
// glyph that darkens on hover or when on.
func gorexIconFace(c *ui.Context, b ui.Element, k *gorexColors, svg *ui.SVG, tooltip string, selected bool, iconSize float32) ui.Element {
	if tooltip != "" {
		b.Tooltip(tooltip)
	}
	gorexToggleFace(b, k, selected)
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

// gorexPulldown is gorexIconButton's pull-down sibling: an icon with a small
// chevron, as a native toolbar pop-up button reads, in the same faces. The
// caller attaches the Menu.
func gorexPulldown(c *ui.Context, k *gorexColors, svg *ui.SVG, label string, height, iconSize float32) ui.Element {
	b := ui.Row(c).Height(height).Padding(0, 6).Gap(3).AlignItems(ui.Center).Justify(ui.Center).
		Radius(height / 2.6).Cursor(ui.CursorPointer).Role(ui.RoleButton).Label(label).Tooltip(label)
	gorexToggleFace(b, k, false)
	b.Transition(ui.ElementTransition{Colors: true, Duration: 120 * time.Millisecond})
	col := k.iconMuted
	if b.Hovered() {
		col = k.text
	}
	b.Children(func() {
		ui.Icon(c, svg).Size(iconSize, iconSize).TextColor(col)
		ui.Icon(c, iconChevronDown).Size(9, 9).TextColor(col)
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
		Background: ui.Hex("#ffffff"),
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

// shortDir shows a directory as shells do, from ~, and only its last
// parts when it is deep, handling cross-platform separators.
func shortDir(d string) string {
	if d == "" {
		return ""
	}
	sep := string(filepath.Separator)
	if home, _ := os.UserHomeDir(); home != "" {
		if samePath(d, home) {
			return "~"
		}
		if len(d) > len(home) && samePath(d[:len(home)], home) && d[len(home)] == filepath.Separator {
			d = "~" + d[len(home):]
		}
	}
	parts := strings.Split(d, sep)
	if len(parts) > 5 || (len(d) > 48 && len(parts) > 3) {
		return "…" + sep + strings.Join(parts[len(parts)-2:], sep)
	}
	return d
}

// samePath reports whether two paths are the same, without case on Windows.
func samePath(a, b string) bool {
	if runtime.GOOS == "windows" {
		return strings.EqualFold(a, b)
	}
	return a == b
}
