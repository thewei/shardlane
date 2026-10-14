package nativeui

import (
	"image"
	"math"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/settings"
)

// TestAppearanceSettingsRoundTrip pins P19: Density & HighContrast
// settings persist and can be mutated via Shell settings actions.
func TestAppearanceSettingsRoundTrip(t *testing.T) {
	shell := NewShell()

	// Initial defaults
	if shell.settings.General.Density != "default" {
		t.Fatalf("expected default density, got %q", shell.settings.General.Density)
	}
	if shell.settings.General.HighContrast {
		t.Fatal("expected high contrast to be false by default")
	}

	// Mutate to compact & high contrast
	shell.applySettings(func(s *settings.Settings) error {
		s.General.Density = "compact"
		s.General.HighContrast = true
		return nil
	})

	if shell.settings.General.Density != "compact" {
		t.Fatalf("expected density 'compact', got %q", shell.settings.General.Density)
	}
	if !shell.settings.General.HighContrast {
		t.Fatal("expected high contrast to be true")
	}

	// Mutate to comfortable
	shell.applySettings(func(s *settings.Settings) error {
		s.General.Density = "comfortable"
		return nil
	})

	if shell.settings.General.Density != "comfortable" {
		t.Fatalf("expected density 'comfortable', got %q", shell.settings.General.Density)
	}

	// Default opacity
	if shell.settings.General.EffectiveOpacity() != 1.0 {
		t.Fatalf("expected default opacity 1.0, got %v", shell.settings.General.EffectiveOpacity())
	}

	// Mutate opacity to 0.85
	shell.applySettings(func(s *settings.Settings) error {
		s.General.Opacity = 0.85
		return nil
	})

	if shell.settings.General.Opacity != 0.85 {
		t.Fatalf("expected opacity 0.85, got %v", shell.settings.General.Opacity)
	}
	if shell.settings.General.EffectiveOpacity() != 0.85 {
		t.Fatalf("expected effective opacity 0.85, got %v", shell.settings.General.EffectiveOpacity())
	}
}

// TestSidebarDensityVariations pins P19: the density setting selects one of
// the three sidebar densities. What each density does to a row is pinned by
// TestTreeRowHeightFollowsDensity, the one place density is spent.
func TestSidebarDensityVariations(t *testing.T) {
	shell := NewShell()
	for setting, want := range map[string]SidebarDensity{
		"default":     DensityDefault,
		"compact":     DensityCompact,
		"comfortable": DensityComfortable,
	} {
		shell.settings.General.Density = setting
		if got := shell.sidebarDensity(); got != want {
			t.Fatalf("density %q: got %s, want %s", setting, got, want)
		}
	}
}

// TestDesignTokensHighContrast pins P19: High Contrast strengthens borders
// and dividers in both light and dark themes.
func TestDesignTokensHighContrast(t *testing.T) {
	// Dark mode: standard vs high contrast
	normalDark := designTokensWithContrast(true, false)
	hcDark := designTokensWithContrast(true, true)

	if normalDark.BorderSubtle == hcDark.BorderSubtle {
		t.Fatal("high contrast dark must have distinct border subtle color")
	}
	if normalDark.TextMuted == hcDark.TextMuted {
		t.Fatal("high contrast dark must have enhanced muted text contrast")
	}

	// Light mode: standard vs high contrast
	normalLight := designTokensWithContrast(false, false)
	hcLight := designTokensWithContrast(false, true)

	if normalLight.BorderSubtle == hcLight.BorderSubtle {
		t.Fatal("high contrast light must have distinct border subtle color")
	}
	if normalLight.TextMuted == hcLight.TextMuted {
		t.Fatal("high contrast light must have enhanced muted text contrast")
	}

	// Terminal output preservation invariant: tokens call with high contrast
	// should not break shell background or content background
	shell := NewShell()
	shell.settings.General.HighContrast = true
	darkTokens := shell.tokens(true)
	if darkTokens.Content != designTokens(true).Content {
		t.Fatalf("content background mutated: %+v", darkTokens.Content)
	}
}

// contrastRatio is the WCAG contrast ratio of fg drawn over bg; a translucent
// fg is composited onto bg first.
func contrastRatio(fg, bg ui.Color) float64 {
	luminance := func(c ui.Color) float64 {
		channel := func(v uint8) float64 {
			x := float64(v) / 255
			if x <= 0.03928 {
				return x / 12.92
			}
			return math.Pow((x+0.055)/1.055, 2.4)
		}
		return 0.2126*channel(c.R) + 0.7152*channel(c.G) + 0.0722*channel(c.B)
	}
	a, b := luminance(fg.Over(bg)), luminance(bg)
	if a < b {
		a, b = b, a
	}
	return (a + 0.05) / (b + 0.05)
}

// withHighContrast runs fn with Increase contrast on, restoring the flag.
func withHighContrast(t *testing.T, on bool) {
	t.Helper()
	was := highContrast.Load()
	highContrast.Store(on)
	t.Cleanup(func() { highContrast.Store(was) })
}

// TestIncreaseContrastStrengthensEveryPalette pins what the setting promises
// — stronger borders and secondary text — for every color source the views
// read: the framework theme, the design tokens and the pane-card and
// diff-review palettes. Each reading must get stronger on its own (a source
// left behind would be hidden by a stronger neighbour) and, for text and
// control outlines, reach WCAG's floor: 7:1 for secondary text on its surface,
// 4.5:1 for the faintest tones, 3:1 for an outline.
func TestIncreaseContrastStrengthensEveryPalette(t *testing.T) {
	readings := []struct {
		name  string
		floor float64
		of    func(dark bool) float64
	}{
		{"theme secondary text", 7, func(dark bool) float64 {
			th := shardlaneTheme(dark)
			return contrastRatio(th.TextMuted, th.Background)
		}},
		{"theme control outline", 3, func(dark bool) float64 {
			th := shardlaneTheme(dark)
			return contrastRatio(th.Border, th.Background)
		}},
		{"token hairline", 0, func(dark bool) float64 {
			tk := designTokens(dark)
			return contrastRatio(tk.BorderSubtle, tk.Content)
		}},
		{"pane card secondary text", 7, func(dark bool) float64 {
			k := gorexColorsOf(dark)
			return contrastRatio(k.textMuted, k.cardPaper)
		}},
		{"pane card faint text", 4.5, func(dark bool) float64 {
			k := gorexColorsOf(dark)
			return contrastRatio(k.textFaint, k.cardPaper)
		}},
		{"pane card icon", 4.5, func(dark bool) float64 {
			k := gorexColorsOf(dark)
			return contrastRatio(k.iconMuted, k.cardPaper)
		}},
		{"pane card hairline", 0, func(dark bool) float64 {
			k := gorexColorsOf(dark)
			return contrastRatio(k.cardBorder, k.cardPaper)
		}},
		{"diff secondary text", 4.5, func(dark bool) float64 {
			pal := gdPaletteFor(shardlaneTheme(dark))
			return contrastRatio(pal.muted, pal.codeBg)
		}},
		{"diff line number", 4.5, func(dark bool) float64 {
			pal := gdPaletteFor(shardlaneTheme(dark))
			return contrastRatio(pal.lineNumber, pal.codeBg)
		}},
		{"diff gap label", 4.5, func(dark bool) float64 {
			pal := gdPaletteFor(shardlaneTheme(dark))
			return contrastRatio(pal.gapText, pal.gapBg)
		}},
		{"diff hairline", 0, func(dark bool) float64 {
			pal := gdPaletteFor(shardlaneTheme(dark))
			return contrastRatio(pal.cardBorder, pal.codeBg)
		}},
	}
	for _, dark := range []bool{false, true} {
		// The review caches highlighted spans per dark/light: whichever
		// palette is picked must still know which it is.
		for _, on := range []bool{false, true} {
			withHighContrast(t, on)
			if gdPaletteFor(shardlaneTheme(dark)).dark != dark {
				t.Errorf("dark=%v high contrast=%v: the diff palette misreports its scheme", dark, on)
			}
		}
		for _, r := range readings {
			withHighContrast(t, false)
			normal := r.of(dark)
			withHighContrast(t, true)
			strong := r.of(dark)
			if strong <= normal || strong < r.floor {
				t.Errorf("dark=%v %s: %.2f:1 normal, %.2f:1 increased (must be stronger and >= %.1f:1)", dark, r.name, normal, strong, r.floor)
			}
		}
	}
}

// TestIncreaseContrastIsPublishedEveryFrame pins the wiring: the palettes are
// functions of (dark) that never see the Shell, so the view publishes the
// setting each frame and turning it off again takes effect at once.
func TestIncreaseContrastIsPublishedEveryFrame(t *testing.T) {
	withHighContrast(t, false)
	shell := NewShell()
	tester := ui.NewTester(shell.View, 800, 600)
	tester.Frame()
	if highContrast.Load() {
		t.Fatal("contrast is raised before the setting is on")
	}
	shell.settings.General.HighContrast = true
	tester.Frame()
	if !highContrast.Load() || designTokens(true) != designTokensWithContrast(true, true) {
		t.Fatal("the setting did not reach the palettes")
	}
	shell.settings.General.HighContrast = false
	tester.Frame()
	if highContrast.Load() || designTokens(true) != designTokensWithContrast(true, false) {
		t.Fatal("turning the setting off did not reach the palettes")
	}
}

// TestIncreaseContrastChangesTheRenderedPage is the end-to-end liveness
// floor: before the setting reached the theme it moved a quarter of a percent
// of a page's pixels (a few hairlines); with it, borders, outlines and
// secondary text all change.
func TestIncreaseContrastChangesTheRenderedPage(t *testing.T) {
	for _, dark := range []bool{false, true} {
		var pages [2]image.Image
		for i, on := range []bool{false, true} {
			shell := NewShell()
			shell.settings.General.HighContrast = on
			shell.router.Push("/settings/general")
			tester := ui.NewTester(shell.View, 1280, 820)
			tester.SetDark(dark)
			time.Sleep(450 * time.Millisecond)
			for k := 0; k < 4; k++ {
				tester.Frame()
			}
			pages[i] = tester.Image()
		}
		bounds, changed := pages[0].Bounds(), 0
		for y := bounds.Min.Y; y < bounds.Max.Y; y++ {
			for x := bounds.Min.X; x < bounds.Max.X; x++ {
				r0, g0, b0, _ := pages[0].At(x, y).RGBA()
				r1, g1, b1, _ := pages[1].At(x, y).RGBA()
				if r0 != r1 || g0 != g1 || b0 != b1 {
					changed++
				}
			}
		}
		if share := 100 * float64(changed) / float64(bounds.Dx()*bounds.Dy()); share < 1 {
			t.Errorf("dark=%v: Increase contrast changed only %.2f%% of the Settings page, want >= 1%%", dark, share)
		}
	}
}
