package nativeui

import "github.com/egoist/mygo/ui"

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
		t.Background = ui.Hex("#fbfbfc")
		t.Surface = ui.Hex("#f4f4f5")
		t.SurfaceHover = ui.Hex("#ededee")
		t.SurfacePressed = ui.Hex("#e4e4e7")
		t.Border = ui.Hex("#e4e4e7")
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
		return &t
	}

	t := *ui.DarkTheme()
	t.Background = ui.Hex("#1c1c1e")
	t.Surface = ui.Hex("#252527")
	t.SurfaceHover = ui.Hex("#2c2c2f")
	t.SurfacePressed = ui.Hex("#343438")
	t.Border = ui.Hex("#38383d")
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
	return &t
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

// sidebarItemSpacing returns the item vertical padding and row gap for the active density.
func (s *Shell) sidebarItemSpacing() (itemPaddingY float32, gap float32) {
	switch s.sidebarDensity() {
	case DensityCompact:
		return 3, 1
	case DensityComfortable:
		return 8, 4
	default:
		return 5, 2
	}
}

// tokens returns the DesignTokens considering both dark theme and high contrast preference.
func (s *Shell) tokens(dark bool) DesignTokens {
	return designTokensWithContrast(dark, s.settings.General.HighContrast)
}

func designTokens(dark bool) DesignTokens {
	return designTokensWithContrast(dark, false)
}

func designTokensWithContrast(dark bool, highContrast bool) DesignTokens {
	if !dark {
		tokens := DesignTokens{
			Sidebar:       ui.RGBA(246, 246, 248, 0.84),
			SidebarHover:  ui.RGBA(0, 0, 0, 0.045),
			SidebarSelect: ui.RGBA(10, 132, 255, 0.13),
			Content:       ui.Hex("#fbfbfc"),
			Panel:         ui.Hex("#ffffff"),
			PanelHover:    ui.Hex("#f7f7f8"),
			BorderSubtle:  ui.RGBA(24, 24, 27, 0.09),
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
		Sidebar:       ui.RGBA(28, 28, 30, 0.78),
		SidebarHover:  ui.RGBA(255, 255, 255, 0.055),
		SidebarSelect: ui.RGBA(10, 132, 255, 0.18),
		Content:       ui.Hex("#1c1c1e"),
		Panel:         ui.Hex("#232326"),
		PanelHover:    ui.Hex("#29292d"),
		BorderSubtle:  ui.RGBA(255, 255, 255, 0.09),
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
