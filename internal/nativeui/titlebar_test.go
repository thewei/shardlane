package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// TestTitlebarPinTogglesAlwaysOnTop pins the titlebar pin (2026-10-07): the
// button renders on every route's trailing edge and its click flips the
// Shell's always-on-top mirror. Headless sessions keep win nil, so the
// platform call is a guarded no-op and the click stays a pure state flip.
func TestTitlebarPinTogglesAlwaysOnTop(t *testing.T) {
	s := NewShell()
	s.loading = false
	tester := ui.NewTester(s.View, 1200, 800)
	if !tester.HasText(windowPinLabel) {
		t.Fatalf("titlebar missing pin button; texts=%q", tester.Texts())
	}
	if s.windowPinned {
		t.Fatal("window must start unpinned")
	}
	if err := tester.Click(windowPinLabel); err != nil {
		t.Fatal(err)
	}
	if !s.windowPinned {
		t.Fatal("pin click did not enable always-on-top")
	}
	if err := tester.Click(windowPinLabel); err != nil {
		t.Fatal(err)
	}
	if s.windowPinned {
		t.Fatal("repeat pin click did not disable always-on-top")
	}
}
