package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/settings"
)

func TestAppearanceMapping(t *testing.T) {
	cases := []struct {
		value string
		index int
	}{
		{"system", 0},
		{"light", 1},
		{"dark", 2},
		{"bogus", 0},
	}
	for _, tc := range cases {
		if got := appearanceIndex(tc.value); got != tc.index {
			t.Fatalf("appearanceIndex(%q) = %d, want %d", tc.value, got, tc.index)
		}
	}
	if got := appearanceFromIndex(1); got != "light" {
		t.Fatalf("appearanceFromIndex(1) = %q", got)
	}
	if got := appearanceFromIndex(9); got != "system" {
		t.Fatalf("out-of-range appearance = %q", got)
	}
}

func TestResolvedDarkFollowsPreference(t *testing.T) {
	s := NewShell()
	s.settings.General.Appearance = "light"
	if s.resolvedDark(true) {
		t.Fatal("light preference must override dark system")
	}
	s.settings.General.Appearance = "dark"
	if !s.resolvedDark(false) {
		t.Fatal("dark preference must override light system")
	}
	s.settings.General.Appearance = "system"
	if !s.resolvedDark(true) || s.resolvedDark(false) {
		t.Fatal("system preference must follow the OS scheme")
	}
}

func TestTerminalOptionsFromSettings(t *testing.T) {
	value := settings.TerminalSettings{
		FontFamily:  "JetBrains Mono",
		FontSize:    14,
		LineHeight:  1.2,
		Scrollback:  32 * 1024 * 1024,
		OptionAsAlt: false,
	}
	options := terminalOptionsFromSettings(value)
	if options.Font.Family != "JetBrains Mono, Menlo, Cascadia Mono, JetBrainsMono Nerd Font Mono, SauceCodePro Nerd Font Mono, Hack Nerd Font Mono, Symbols Nerd Font, monospace" {
		t.Fatalf("font stack = %q", options.Font.Family)
	}
	if options.Font.Size != 14 || options.Font.LineHeight != 1.2 {
		t.Fatalf("font metrics = %+v", options.Font)
	}
	if options.Scrollback != 32*1024*1024 || options.OptionAsAlt {
		t.Fatalf("terminal options = %+v", options)
	}
}

func TestSettingsGeneralPageUpdatesService(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.router.Replace("/settings/general")
	tester := ui.NewTester(s.View, 1200, 800)

	if !tester.HasText("Appearance") || !tester.HasText("Restore window position") || !tester.HasText("Window opacity") {
		t.Fatalf("general page missing controls; texts=%q", tester.Texts())
	}
	if err := tester.Click("Light"); err != nil {
		t.Fatal(err)
	}
	if got := s.settingsService.Current().General.Appearance; got != "light" {
		t.Fatalf("appearance after click = %q, want light", got)
	}
	if s.settings.General.Appearance != "light" {
		t.Fatal("presentation snapshot was not refreshed")
	}
	if err := tester.Click("Restore window position"); err != nil {
		t.Fatal(err)
	}
	if s.settingsService.Current().General.RestoreWindow {
		t.Fatal("restore window toggle did not persist false")
	}
}

// The classic/rich chat switch is an escape hatch for an implementation
// detail, so it lives under Diagnostics; General only holds preferences about
// how Shardlane looks and behaves.
func TestRichChatSwitchLivesInDiagnostics(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.router.Replace("/settings/general")
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	if tester.HasText("Rich chat view") {
		t.Fatalf("General still carries the chat implementation switch; texts=%q", tester.Texts())
	}

	s.router.Replace("/settings/diagnostics")
	tester = ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	if !tester.HasText("Rich chat view") {
		t.Fatalf("Diagnostics is missing the chat switch; texts=%q", tester.Texts())
	}
	before := s.settingsService.Current().General.WebViewChat
	if err := tester.Click("Rich chat view"); err != nil {
		t.Fatal(err)
	}
	if got := s.settingsService.Current().General.WebViewChat; got == before {
		t.Fatalf("toggling Rich chat view kept WebViewChat = %v", got)
	}
}

func TestSettingsTerminalPageUpdatesService(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.router.Replace("/settings/terminal")
	tester := ui.NewTester(s.View, 1200, 800)

	for _, want := range []string{"Font family", "Font size", "Line height", "Scrollback", "Use Option as Alt"} {
		if !tester.HasText(want) {
			t.Fatalf("terminal page missing %q; texts=%q", want, tester.Texts())
		}
	}
	if err := tester.Click("Use Option as Alt"); err != nil {
		t.Fatal(err)
	}
	if s.settingsService.Current().Terminal.OptionAsAlt {
		t.Fatal("option-as-alt toggle did not persist false")
	}
}
