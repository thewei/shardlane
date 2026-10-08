package settings

import "testing"

// MyGo v0.2.15 terminal transparency is opt-in: the persisted default and
// the zero value of a fresh store must both keep terminals opaque.
func TestTerminalTransparentDefaultsOff(t *testing.T) {
	if Default().Terminal.Transparent {
		t.Fatal("transparent must default to off")
	}
	var zero TerminalSettings
	if zero.Transparent {
		t.Fatal("zero-value TerminalSettings must stay opaque")
	}
}
