package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/wh-studio/herdr-client/next/internal/settings"
)

func TestShortcutRegistryCoversRequiredShellActions(t *testing.T) {
	required := map[shortcutAction]bool{
		shortcutSearch:   false,
		shortcutSettings: false,
		shortcutRefresh:  false,
		shortcutBack:     false,
		shortcutForward:  false,
	}
	seenChord := make(map[[2]uint16]bool)
	for _, binding := range defaultShortcutBindings {
		chord := [2]uint16{uint16(binding.Modifiers), uint16(binding.Key)}
		if seenChord[chord] {
			t.Fatalf("duplicate shortcut chord: modifiers=%d key=%d", binding.Modifiers, binding.Key)
		}
		seenChord[chord] = true
		if _, ok := required[binding.Action]; ok {
			required[binding.Action] = true
		}
	}
	for action, found := range required {
		if !found {
			t.Fatalf("required shortcut action %d is not registered", action)
		}
	}
}

func TestShortcutBackForwardUsesRouterHistory(t *testing.T) {
	s := NewShell()
	s.loading = false
	tester := ui.NewTester(s.View, 1200, 800)

	tester.Key(ui.Cmd, ui.KeyComma)
	if got := s.router.Path(); got != routeSettings {
		t.Fatalf("settings route = %q", got)
	}
	tester.Key(ui.Cmd, ui.KeyBracketLeft)
	if got := s.router.Path(); got != routeWorkspace {
		t.Fatalf("back route = %q", got)
	}
	tester.Key(ui.Cmd, ui.KeyBracketRight)
	if got := s.router.Path(); got != routeSettings {
		t.Fatalf("forward route = %q", got)
	}
}

// TestShortcutBindingsRespectPersistedSettings pins F11 (2026-10-06): the
// persisted shortcut settings are authoritative; unparsable values fall back
// to the default instead of dropping the action.
func TestShortcutBindingsRespectPersistedSettings(t *testing.T) {
	value := settings.ShortcutSettings{
		Search:   "Ctrl+F3",
		Settings: "",          // unset → default
		Refresh:  "not a key", // unparsable → default
		Back:     "Cmd+[",
		Forward:  "Cmd+]",
	}
	bindings := shortcutBindings(value)
	byAction := map[shortcutAction]shortcutBinding{}
	for _, binding := range bindings {
		byAction[binding.Action] = binding
	}

	if got := byAction[shortcutSearch]; got.Modifiers != ui.Ctrl || got.Key != ui.KeyF3 {
		t.Fatalf("Search binding = %+v, want Ctrl+F3", got)
	}
	if got := byAction[shortcutSettings]; got.Key != ui.KeyComma || got.Modifiers != ui.Cmd {
		t.Fatalf("Settings binding = %+v, want the Cmd+, default", got)
	}
	if got := byAction[shortcutRefresh]; got.Key != ui.KeyR || got.Modifiers != ui.Cmd {
		t.Fatalf("Refresh binding = %+v, want the Cmd+R default", got)
	}
	if got := byAction[shortcutBack]; got.Key != ui.KeyBracketLeft {
		t.Fatalf("Back binding = %+v, want Cmd+[", got)
	}
}
