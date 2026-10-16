package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

func TestProbeChipSwitch(t *testing.T) {
	shell, tester, _ := treeTestShell(t)
	other := t.TempDir()
	shell.projection.Projects = append(shell.projection.Projects, herdr.Project{ID: "w2", Label: "elsewhere", CWD: other})
	shell.projection.Tabs = append(shell.projection.Tabs, herdr.Tab{ID: "t2", Label: "main", ProjectID: "w2"})
	shell.projection.Panes = append(shell.projection.Panes, herdr.Pane{ID: "p2", TerminalID: "term-2", Label: "editor", TabID: "t2", CWD: other})
	shell.git.rootOf[other] = other
	tester.Frame()
	dump := func(tag string) {
		sp := shell.selectedPane()
		spID, spCWD := "<nil>", ""
		if sp != nil {
			spID, spCWD = sp.ID, sp.CWD
		}
		t.Logf("%s: proj=%q tab=%q pane=%q selPane=%q selPaneCWD=%q cwd=%q root=%q nPanes=%d", tag,
			shell.selectedProjectID, shell.selectedTabID, shell.selectedPaneID,
			spID, spCWD, shell.selectedTabCWD(), shell.git.root, len(shell.projection.Panes))
		for _, pn := range shell.projection.Panes {
			t.Logf("  pane %q tab=%q cwd=%q", pn.ID, pn.TabID, pn.CWD)
		}
	}
	if err := tester.Click("Switch project"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()
	dump("menu-open")
	if err := tester.Click("elsewhere"); err != nil {
		t.Fatal(err)
	}
	dump("post-click")
	tester.Frame()
	dump("post-frame")
}
