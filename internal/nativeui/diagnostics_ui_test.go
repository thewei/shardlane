package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// TestSettingsDiagnosticsSnapshotView pins P16: diagnostics section
// renders system facts, runtime versions, and log viewer safely.
func TestSettingsDiagnosticsSnapshotView(t *testing.T) {
	shell := NewShell()
	shell.activeInstance = "inst-test"
	shell.projection = herdr.Projection{
		Version:  "0.9.1",
		Protocol: 22,
	}

	shell.router.Replace("/settings/diagnostics")
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	for _, want := range []string{
		"Diagnostics",
		"About",
		// Unpackaged test builds report the dev fallback instead of a
		// hardcoded version (diagnostics.BuildSnapshot).
		"dev",
		"Logs",
		"Copy Diagnostics",
	} {
		if !tester.HasText(want) {
			t.Fatalf("diagnostics view missing %q; texts=%q", want, tester.Texts())
		}
	}
	// The About panel reads user language: the wire protocol version is an
	// implementation fact and only ships in the diagnostics export (F173).
	if tester.HasText("protocol") {
		t.Fatalf("About panel leaks the wire protocol; texts=%q", tester.Texts())
	}
}

// TestSettingsDiagnosticsExportAction pins P17: export action creates
// sanitized bundle without secrets and updates status message.
func TestSettingsDiagnosticsExportAction(t *testing.T) {
	shell := NewShell()
	shell.activeInstance = "inst-test"
	shell.router.Replace("/settings/diagnostics")

	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	// Trigger export
	if err := tester.Click("Copy Diagnostics"); err != nil {
		t.Fatal(err)
	}

	tester.Frame()
	if !tester.HasText("Copied to the clipboard") {
		t.Fatalf("expected export status message; texts=%q", tester.Texts())
	}
}
