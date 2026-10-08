package nativeui

import (
	"testing"

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
}

// TestSidebarDensityVariations pins P19: sidebar density controls
// item vertical padding and row gap without breaking hierarchy.
func TestSidebarDensityVariations(t *testing.T) {
	shell := NewShell()

	// Default
	shell.settings.General.Density = "default"
	if d := shell.sidebarDensity(); d != DensityDefault {
		t.Fatalf("expected DensityDefault, got %s", d)
	}
	padDefault, gapDefault := shell.sidebarItemSpacing()

	// Compact
	shell.settings.General.Density = "compact"
	if d := shell.sidebarDensity(); d != DensityCompact {
		t.Fatalf("expected DensityCompact, got %s", d)
	}
	padCompact, gapCompact := shell.sidebarItemSpacing()

	// Comfortable
	shell.settings.General.Density = "comfortable"
	if d := shell.sidebarDensity(); d != DensityComfortable {
		t.Fatalf("expected DensityComfortable, got %s", d)
	}
	padComfortable, gapComfortable := shell.sidebarItemSpacing()

	// Compact must be strictly more compact than Default, which is strictly more compact than Comfortable
	if padCompact >= padDefault || gapCompact >= gapDefault {
		t.Fatalf("compact spacing (pad=%v, gap=%v) must be smaller than default (pad=%v, gap=%v)",
			padCompact, gapCompact, padDefault, gapDefault)
	}
	if padDefault >= padComfortable || gapDefault >= gapComfortable {
		t.Fatalf("default spacing (pad=%v, gap=%v) must be smaller than comfortable (pad=%v, gap=%v)",
			padDefault, gapDefault, padComfortable, gapComfortable)
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
	if darkTokens.Content != ui.Hex("#1c1c1e") {
		t.Fatalf("content background mutated: %+v", darkTokens.Content)
	}
}
