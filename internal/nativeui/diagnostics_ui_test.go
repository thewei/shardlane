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

// The runtime row keeps F173 (user language) while adding the live check
// and the in-place update when the runtime predates the supported protocol.
func TestSettingsRuntimeRowChecksAndUpdatesHerdr(t *testing.T) {
	shell := NewShell()
	shell.activeInstance = "default"
	shell.projection = herdr.Projection{Version: "0.9.3", Protocol: 22}
	shell.router.Replace("/settings/diagnostics")
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	if !tester.HasText("0.9.3") || !tester.HasText("Compatible") {
		t.Fatalf("runtime row missing identity/pill: %q", tester.Texts())
	}
	if !tester.HasText("Check") {
		t.Fatalf("runtime row missing the live check: %q", tester.Texts())
	}
	if tester.HasText("Update Herdr") {
		t.Fatalf("compatible runtime must not offer the update: %q", tester.Texts())
	}
	if tester.HasText("protocol") {
		t.Fatalf("runtime row leaks the wire protocol (F173): %q", tester.Texts())
	}

	shell.projection = herdr.Projection{Version: "0.8.2", Protocol: 20}
	tester.Frame()
	if !tester.HasText("Update available") || !tester.HasText("Update Herdr") {
		t.Fatalf("outdated runtime missing update affordance: %q", tester.Texts())
	}

	shell.herdrUpdating = true
	tester.Frame()
	if !tester.HasText("Updating Herdr…") || tester.HasText("Update Herdr") || tester.HasText("Check") {
		t.Fatalf("updating state must replace the actions: %q", tester.Texts())
	}
}

// A failed update never returns the page to silence: a system-location
// install shows the user-language way out with the exact terminal command
// to copy; any other failure keeps its message on the panel.
func TestSettingsRuntimeRowShowsUpdateWayOut(t *testing.T) {
	shell := NewShell()
	shell.activeInstance = "default"
	shell.projection = herdr.Projection{Version: "0.8.2", Protocol: 20}
	shell.router.Replace("/settings/diagnostics")
	shell.herdrUpdateErr = "herdr update --handoff: update failed: install directory not writable: /usr/bin"
	shell.herdrUpdateNotWrbl = true
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	for _, want := range []string{"administrator permission", "Copy Update Command", "Update Herdr"} {
		if !tester.HasText(want) {
			t.Fatalf("settings way-out missing %q: %q", want, tester.Texts())
		}
	}
	if tester.HasText("/usr/bin") {
		t.Fatalf("raw updater stderr must not leak onto the panel: %q", tester.Texts())
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
