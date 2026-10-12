package main

import (
	"testing"

	"github.com/egoist/mygo"
)

// An unpackaged run is named after its executable. The temporary binary
// that `mygo build` and `mygo dev` start to write the TypeScript client
// carries a process id there, so every build left a data and a log
// directory behind. Unpackaged runs take one fixed name instead; a packaged
// app keeps its bundle's name, so the installed app's data is never touched.
func TestUnpackagedRunTakesTheFixedDevName(t *testing.T) {
	if mygo.App.IsPackaged() {
		t.Skip("the test binary runs from an app bundle")
	}
	t.Cleanup(func() { mygo.App.SetName("") })

	if got := mygo.App.Name(); got == "Shardlane-dev" {
		t.Fatalf("Name() = %q before naming the run; the check below would prove nothing", got)
	}
	nameUnpackagedRun()
	if got := mygo.App.Name(); got != "Shardlane-dev" {
		t.Fatalf("Name() = %q, want Shardlane-dev", got)
	}
}
