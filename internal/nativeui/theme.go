package nativeui

import (
	"sync/atomic"

	"github.com/egoist/mygo/ui"
)

// highContrast mirrors Settings → General → Increase contrast for the whole
// process. Colors reach the views through plain functions of (dark) — the
// theme, the design tokens and the two surface palettes — that most call
// sites read without a Shell at hand, so Shell.View publishes the window's
// one preference here before it builds the frame's theme.
var highContrast atomic.Bool

// Oklch accent colors (MyGo 0.2.15 ui.Oklch): the same Apple-blue family
// as the old sRGB hex values — each call was hill-climbed in a probe so
// its sRGB fallback renders within a step of the original hex — but they
// carry a wide-gamut component, so P3 Mac displays show the fuller blue.
var (
	accentLight        = ui.Oklch(0.6260, 0.1960, 254)   // was #0a84ff
	accentHoverLight   = ui.Oklch(0.5800, 0.2020, 254)   // was #0077ed
	accentPressedLight = ui.Oklch(0.5300, 0.1940, 254.8) // was #0067d8
	accentDark         = ui.Oklch(0.6260, 0.1960, 254)   // was #0a84ff
	accentHoverDark    = ui.Oklch(0.6870, 0.1720, 253.2) // was #409cff
	accentPressedDark  = ui.Oklch(0.5600, 0.1880, 255.2) // was #0070df
)

// DesignTokens are Shardlane's reusable presentation tokens. They deliberately
// stay close to macOS system surfaces and shadcn's neutral, low-contrast style.
type DesignTokens struct {
	// Sidebar is the opaque sidebar fill for windows that cannot show the
	// vibrancy material (non-macOS, headless); with vibrancy the sidebar
	// draws nothing and the material shows through.
	Sidebar       ui.Color
	SidebarHover  ui.Color
	SidebarSelect ui.Color
	Content       ui.Color
	Panel         ui.Color
	PanelHover    ui.Color
	BorderSubtle  ui.Color
	TreeLine      ui.Color
	Text          ui.Color
	TextMuted     ui.Color
	Accent        ui.Color
	Danger        ui.Color
}

// resolvedDark maps the user appearance preference onto the OS-reported
// scheme. Only an explicit Light/Dark choice overrides the system.
func (s *Shell) resolvedDark(systemDark bool) bool {
	switch s.settings.General.Appearance {
	case "light":
		return false
	case "dark":
		return true
	default:
		return systemDark
	}
}

func shardlaneTheme(dark bool) *ui.Theme {
	if !dark {
		t := *ui.LightTheme()
		t.Background = ui.Hex("#f5f5f7")
		t.Surface = ui.Hex("#ffffff")
		t.SurfaceHover = ui.Hex("#f2f2f5")
		t.SurfacePressed = ui.Hex("#e8e8ec")
		t.Border = ui.Hex("#dcdce1")
		t.Text = ui.Hex("#18181b")
		t.TextMuted = ui.Hex("#71717a")
		t.Accent = accentLight
		t.AccentHover = accentHoverLight
		t.AccentPressed = accentPressedLight
		t.AccentText = ui.Hex("#ffffff")
		t.Selection = ui.RGBA(10, 132, 255, 0.18)
		t.Focus = ui.RGBA(10, 132, 255, 0.45)
		t.Radius = 7
		t.Spacing = 3.5
		return strengthened(&t)
	}

	t := *ui.DarkTheme()
	t.Background = ui.Hex("#161618")
	t.Surface = ui.Hex("#252528")
	t.SurfaceHover = ui.Hex("#2d2d31")
	t.SurfacePressed = ui.Hex("#36363b")
	t.Border = ui.Hex("#3a3a40")
	t.Text = ui.Hex("#f4f4f5")
	t.TextMuted = ui.Hex("#98989f")
	t.Accent = accentDark
	t.AccentHover = accentHoverDark
	t.AccentPressed = accentPressedDark
	t.AccentText = ui.Hex("#ffffff")
	t.Warning = ui.Hex("#ffd60a")
	t.Success = ui.Hex("#30d158")
	t.Danger = ui.Hex("#ff453a")
	t.Selection = ui.RGBA(10, 132, 255, 0.28)
	t.Focus = ui.RGBA(10, 132, 255, 0.52)
	t.Radius = 7
	t.Spacing = 3.5
	return strengthened(&t)
}

// strengthened applies Increase contrast to the framework theme: the outline
// of every control and the secondary text tone, which is what the widgets and
// most of the views read.
func strengthened(t *ui.Theme) *ui.Theme {
	if !highContrast.Load() {
		return t
	}
	if t.Dark {
		t.Border, t.TextMuted = ui.Hex("#8e8e98"), ui.Hex("#d4d4d8")
	} else {
		t.Border, t.TextMuted = ui.Hex("#8b8b94"), ui.Hex("#3f3f46")
	}
	return t
}

// SidebarDensity configures vertical density and breathing room in the sidebar (0.9 P19).
type SidebarDensity string

const (
	DensityCompact     SidebarDensity = "compact"
	DensityDefault     SidebarDensity = "default"
	DensityComfortable SidebarDensity = "comfortable"
)

// sidebarDensity returns the active density preference with fallback to default.
func (s *Shell) sidebarDensity() SidebarDensity {
	switch s.settings.General.Density {
	case "compact":
		return DensityCompact
	case "comfortable":
		return DensityComfortable
	default:
		return DensityDefault
	}
}

// tokens returns the DesignTokens considering both dark theme and high contrast preference.
func (s *Shell) tokens(dark bool) DesignTokens {
	return designTokensWithContrast(dark, s.settings.General.HighContrast)
}

func designTokens(dark bool) DesignTokens {
	return designTokensWithContrast(dark, highContrast.Load())
}

func designTokensWithContrast(dark bool, highContrast bool) DesignTokens {
	if !dark {
		tokens := DesignTokens{
			Sidebar:       ui.Hex("#e9e9ed"),
			SidebarHover:  ui.RGBA(0, 0, 0, 0.05),
			SidebarSelect: ui.RGBA(10, 132, 255, 0.14),
			Content:       ui.Hex("#f5f5f7"),
			Panel:         ui.Hex("#ffffff"),
			PanelHover:    ui.Hex("#f2f2f5"),
			BorderSubtle:  ui.RGBA(0, 0, 0, 0.08),
			TreeLine:      ui.RGBA(113, 113, 122, 0.28),
			Text:          ui.Hex("#18181b"),
			TextMuted:     ui.Hex("#71717a"),
			Accent:        accentLight,
			Danger:        ui.Hex("#ff3b30"),
		}
		if highContrast {
			tokens.BorderSubtle = ui.RGBA(0, 0, 0, 0.35)
			tokens.TreeLine = ui.RGBA(0, 0, 0, 0.50)
			tokens.TextMuted = ui.Hex("#3f3f46")
		}
		return tokens
	}
	tokens := DesignTokens{
		Sidebar:       ui.Hex("#1f1f22"),
		SidebarHover:  ui.RGBA(255, 255, 255, 0.06),
		SidebarSelect: ui.RGBA(10, 132, 255, 0.22),
		Content:       ui.Hex("#161618"),
		Panel:         ui.Hex("#1f1f22"),
		PanelHover:    ui.Hex("#27272b"),
		BorderSubtle:  ui.RGBA(255, 255, 255, 0.08),
		TreeLine:      ui.RGBA(152, 152, 159, 0.26),
		Text:          ui.Hex("#f4f4f5"),
		TextMuted:     ui.Hex("#98989f"),
		Accent:        accentDark,
		Danger:        ui.Hex("#ff453a"),
	}
	if highContrast {
		tokens.BorderSubtle = ui.RGBA(255, 255, 255, 0.35)
		tokens.TreeLine = ui.RGBA(255, 255, 255, 0.45)
		tokens.TextMuted = ui.Hex("#d4d4d8")
	}
	return tokens
}
