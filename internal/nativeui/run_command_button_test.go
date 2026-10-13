package nativeui

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/scripts"
)

// runControlTestShell seeds the split-header shell with both panes' cwd at
// a temp project root, so the pane card header's run control builds against
// real detected commands.
func runControlTestShell(t *testing.T, files map[string]string) (*Shell, *ui.Tester, string) {
	t.Helper()
	root := t.TempDir()
	for name, content := range files {
		if err := os.WriteFile(filepath.Join(root, name), []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	shell, tt := splitHeaderTestShell(t)
	shell.terminals["t1"].cwd = root
	shell.terminals["t1"].focused = true
	shell.terminals["t2"].cwd = root
	tt.Frame()
	return shell, tt, root
}

func TestRunControlOffersProjectCommands(t *testing.T) {
	_, tt, _ := runControlTestShell(t, map[string]string{
		"package.json": `{"scripts":{"dev":"vite","build":"vite build"}}`,
		"Makefile":     "check:\n\tgo test ./...\n",
	})

	if _, ok := tt.Find("Run"); !ok {
		t.Fatalf("idle pane with commands must render the run button")
	}
	if _, ok := tt.Find("Command menu"); !ok {
		t.Fatalf("idle pane with commands must render the command menu")
	}
	if err := tt.Click("Command menu"); err != nil {
		t.Fatalf("command menu missing: %v", err)
	}
	tt.Frame()
	joined := strings.Join(tt.Menu(), ",")
	for _, want := range []string{"dev", "build", "check"} {
		if !strings.Contains(joined, want) {
			t.Fatalf("command menu missing %q: %v", want, tt.Menu())
		}
	}
	tt.CloseMenu()
}

func TestRunControlMenuPutsLastUsedFirst(t *testing.T) {
	shell, tt, root := runControlTestShell(t, map[string]string{
		"package.json": `{"scripts":{"dev":"vite","build":"vite build"}}`,
	})
	if err := shell.lastUsedStore.Set(root, "build"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()

	if _, ok := tt.Find("Run build"); !ok {
		t.Fatalf("the run button must offer the last-used command, got labels without %q", "Run build")
	}
	if err := tt.Click("Command menu"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	menu := tt.Menu()
	if len(menu) == 0 || !strings.HasPrefix(menu[0], "build") || !strings.Contains(menu[0], "last used") {
		t.Fatalf("last-used command must lead the menu: %v", menu)
	}
	tt.CloseMenu()
}

func TestRunControlBusyShowsStopInstead(t *testing.T) {
	shell, tt, _ := runControlTestShell(t, map[string]string{
		"package.json": `{"scripts":{"dev":"vite"}}`,
	})
	shell.paneBusyIndex["p1"] = paneBusyInfo{Procs: []string{"vite"}}
	tt.Frame()

	if _, ok := tt.Find("Run"); ok {
		t.Fatalf("a busy pane must not offer Run")
	}
	if _, ok := tt.Find("Stop vite"); !ok {
		t.Fatalf("a busy non-agent pane must offer Stop vite")
	}
}

func TestRunControlAgentBusyRendersNothing(t *testing.T) {
	shell, tt, _ := runControlTestShell(t, map[string]string{
		"package.json": `{"scripts":{"dev":"vite"}}`,
	})
	shell.paneBusyIndex["p1"] = paneBusyInfo{Procs: []string{"vite"}}
	shell.projection.Agents = []herdr.Agent{{PaneID: "p1", Name: "Claude", Status: "working"}}
	tt.Frame()

	if _, ok := tt.Find("Stop vite"); ok {
		t.Fatalf("an agent pane's lifecycle is Chat-owned: no Stop")
	}
	if _, ok := tt.Find("Run"); ok {
		t.Fatalf("an agent pane must not offer Run")
	}
}

func TestRunControlWithoutCommandsHidden(t *testing.T) {
	_, tt, _ := runControlTestShell(t, nil)
	if _, ok := tt.Find("Run"); ok {
		t.Fatalf("no commands: no run button")
	}
	if _, ok := tt.Find("Command menu"); ok {
		t.Fatalf("no commands: no command menu")
	}
}

func TestRunControlHeadlessClickDropsCleanly(t *testing.T) {
	shell, tt, root := runControlTestShell(t, map[string]string{
		"package.json": `{"scripts":{"dev":"vite"}}`,
	})
	if err := shell.lastUsedStore.Set(root, "dev"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	// The mutate rule: without a window no live transaction starts. The
	// click must neither panic nor leave a stuck busy flag nor touch the
	// last-used store.
	if err := tt.Click("Run dev"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if shell.runBusy["p1"] {
		t.Fatalf("headless click must not start a transaction")
	}
	if got := shell.lastUsedStore.Get(root); got != "dev" {
		t.Fatalf("headless click must not rewrite last-used: %q", got)
	}
}

func TestComputePaneBusy(t *testing.T) {
	panes := []herdr.Pane{{ID: "p1"}, {ID: "p2"}, {ID: "p3"}}
	infos := map[string]*herdr.PaneProcessInfo{
		"p1": {PaneID: "p1", ShellPID: 100, ForegroundProcesses: []herdr.PaneProcessInfoProcess{
			{PID: 100, Name: "zsh"},
			{PID: 200, Name: "vite"},
		}},
		"p2": {PaneID: "p2", ShellPID: 300, ForegroundProcesses: []herdr.PaneProcessInfoProcess{
			{PID: 300, Name: "zsh"},
		}},
		"p3": {PaneID: "p3"},
	}
	busy := computePaneBusy(panes, infos)
	if got, ok := busy["p1"]; !ok || len(got.Procs) != 1 || got.Procs[0] != "vite" {
		t.Fatalf("p1 must be busy with vite, got %+v (ok=%v)", got, ok)
	}
	if _, ok := busy["p2"]; ok {
		t.Fatalf("an idle shell is not busy: %+v", busy)
	}
	if _, ok := busy["p3"]; ok {
		t.Fatalf("unmeasured panes are not busy: %+v", busy)
	}
}

func TestMergeCommandsFeedsCacheShape(t *testing.T) {
	defs := []scripts.ScriptDefinition{{ID: "1", Name: "dev", Command: "npm run dev"}}
	entries := scripts.MergeCommands(defs, []scripts.DetectedCommand{
		{Name: "dev", Command: "npm run dev", Source: "package.json"},
		{Name: "check", Command: "make check", Source: "Makefile"},
	})
	if len(entries) != 2 || entries[0].Name != "dev" || !entries[0].User || entries[1].Name != "check" {
		t.Fatalf("unexpected merged menu shape: %+v", entries)
	}
}
