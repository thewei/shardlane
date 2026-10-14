package nativeui

import (
	"os"
	"testing"
)

// No test reaches the real pasteboard or Finder: MyGo's desktop calls wait on
// an application loop a test never starts, and a developer's clipboard is not
// a fixture. Tests that care what was copied or revealed capture it with
// captureClipboard / captureReveal.
func TestMain(m *testing.M) {
	writeClipboard = func(string) error { return nil }
	revealInFinder = func(string) {}
	os.Exit(m.Run())
}

// captureClipboard replaces the clipboard writer for one test and returns
// what was written to it, in order.
func captureClipboard(t *testing.T) *[]string {
	t.Helper()
	var got []string
	prev := writeClipboard
	writeClipboard = func(text string) error { got = append(got, text); return nil }
	t.Cleanup(func() { writeClipboard = prev })
	return &got
}

// captureReveal replaces Finder reveal for one test and returns the paths it
// was asked to show.
func captureReveal(t *testing.T) *[]string {
	t.Helper()
	var got []string
	prev := revealInFinder
	revealInFinder = func(path string) { got = append(got, path) }
	t.Cleanup(func() { revealInFinder = prev })
	return &got
}
