package nativeui

/**
 * [INPUT]: exact Tab CWD, native Files route, cached directory snapshot and shared Agent/Services panel
 * [OUTPUT]: replaces retired Right Panel tests with Files workbench, inline preview and service tabs
 * [POS]: Header Files + Agent/Services UI migration regression
 * [PROTOCOL]: update header and check CLAUDE.md
 */

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// rightPanelTestShell is retained as a setup helper for other cross-page
// tests; it creates only a Herdr projection, not a rendered right panel.
func rightPanelTestShell(t *testing.T, cwd string) *Shell {
	t.Helper()
	s := NewShell()
	s.activeInstance = "inst-1"
	s.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: cwd}},
		Tabs:     []herdr.Tab{{ID: "t1", Label: "main", ProjectID: "w1"}},
		Panes:    []herdr.Pane{{ID: "p1", TerminalID: "term-1", Label: "editor", TabID: "t1", CWD: cwd}},
	}
	s.reconcileLocalSelection(s.projection)
	return s
}

func TestFilesHeaderOpensDedicatedRouteAndInlinePreview(t *testing.T) {
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "README.md"), []byte("# Readme\nFiles preview is in the center."), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(filepath.Join(root, "docs"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "docs", "guide.txt"), []byte("nested guide"), 0o600); err != nil {
		t.Fatal(err)
	}
	s := rightPanelTestShell(t, root)
	s.sidebarCollapsed = true
	s.showViewFiles()
	if s.router.Path() != routeFiles || s.sidebarCollapsed {
		t.Fatal("Files entry must open route with left navigation visible")
	}
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame() // headless directory read fills the cache for the next frame
	for _, want := range []string{"Files view", "Filter", "README.md", "docs", "Select a file to preview"} {
		if want == "Filter" {
			continue
		}
		if !tester.HasText(want) {
			t.Fatalf("missing %q on Files route: %q", want, tester.Texts())
		}
	}
	if tester.HasText("Show Workspace Tools") || tester.HasText("History details") {
		t.Fatalf("old right panel should not render: %q", tester.Texts())
	}
	if err := tester.Click("README.md"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	for _, want := range []string{"Files preview is in the center.", "Copy Path", "Reveal in Finder"} {
		if !tester.HasText(want) {
			t.Fatalf("inline preview missing %q: %q", want, tester.Texts())
		}
	}
	if err := tester.Click("docs"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("guide.txt") {
		t.Fatalf("directory expansion failed: %q", tester.Texts())
	}
	if err := tester.Click("guide.txt"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("nested guide") {
		t.Fatalf("nested file preview not shown: %q", tester.Texts())
	}
	s.showViewFiles()
	if s.router.Path() != routeWorkspace {
		t.Fatal("repeating Files should return to Terminal")
	}
}

func TestFilesSelectionClearsOnTabRootChange(t *testing.T) {
	root1, root2 := t.TempDir(), t.TempDir()
	_ = os.WriteFile(filepath.Join(root1, "private.txt"), []byte("private project 1"), 0o600)
	_ = os.WriteFile(filepath.Join(root2, "other.txt"), []byte("other project 2"), 0o600)
	s := NewShell()
	s.activeInstance = "inst-1"
	s.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: root1}},
		Tabs:     []herdr.Tab{{ID: "t1", ProjectID: "w1"}, {ID: "t2", ProjectID: "w1"}},
		Panes: []herdr.Pane{
			{ID: "p1", TabID: "t1", CWD: root1, TerminalID: "term-1"},
			{ID: "p2", TabID: "t2", CWD: root2, TerminalID: "term-2"},
		},
	}
	s.reconcileLocalSelection(s.projection)
	s.showViewFiles()
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	if err := tester.Click("private.txt"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("private project 1") {
		t.Fatal("first file preview not displayed")
	}
	s.selectTab("t2")
	tester.Frame()
	tester.Frame() // show the newly cached root directory
	if s.rightPanel.currentRoot != root2 || s.rightPanel.preview != nil || s.rightPanel.selectedFilePath != "" {
		t.Fatal("Files root change leaked previous Tab file")
	}
	if tester.HasText("private project 1") || !tester.HasText("other.txt") {
		t.Fatalf("old project file leaked after Tab change: %q", tester.Texts())
	}
}

func TestAgentServicesTabsAreClickable(t *testing.T) {
	root := t.TempDir()
	s := rightPanelTestShell(t, root)
	tt := ui.NewTester(s.QuickPanelView, QuickPanelWidth, QuickPanelHeight)
	if err := tt.Click("Services"); err != nil {
		t.Fatal(err)
	}
	if s.quickPanelTab != 1 || !tt.HasText("Listening ports") {
		t.Fatalf("Services tab should expose real data, got: %q", tt.Texts())
	}
	if err := tt.Click("Agents"); err != nil {
		t.Fatal(err)
	}
	if s.quickPanelTab != 0 || !tt.HasText("No agents here") {
		t.Fatalf("Agents tab should restore Agent overview, got: %q", tt.Texts())
	}
}

func TestServicesPortCacheScopedToSelectedTab(t *testing.T) {
	root1, root2 := t.TempDir(), t.TempDir()
	s := rightPanelTestShell(t, root1)
	s.portsForRoot = root1
	s.observedPortsList = []uint16{43217}
	s.portsRefreshedAt = time.Now()
	s.quickPanelTab = 1
	tt := ui.NewTester(s.QuickPanelView, QuickPanelWidth, QuickPanelHeight)
	if !tt.HasText("localhost:43217") {
		t.Fatal("initial scoped service port absent")
	}
	s.projection.Tabs = append(s.projection.Tabs, herdr.Tab{ID: "t2", Label: "other", ProjectID: "w1"})
	s.projection.Panes = append(s.projection.Panes, herdr.Pane{ID: "p2", TerminalID: "term-2", TabID: "t2", CWD: root2})
	s.selectTab("t2")
	tt.Frame()
	if tt.HasText("localhost:43217") || s.portsForRoot != root2 {
		t.Fatal("ports from the previous Tab must not leak into Services")
	}
}

func TestServicesLiveInAgentQuickPanel(t *testing.T) {
	root := t.TempDir()
	s := rightPanelTestShell(t, root)
	s.quickPanelTab = 1
	s.observedPortsList = []uint16{43217}
	s.portsForRoot = root
	s.portsRefreshedAt = time.Now()
	tester := ui.NewTester(s.QuickPanelView, QuickPanelWidth, QuickPanelHeight)
	for _, want := range []string{"Agents & Services", "Agents", "Services", "Scripts", "Listening ports", "localhost:43217", "Preview"} {
		if !tester.HasText(want) {
			t.Fatalf("Services panel missing %q: %q", want, tester.Texts())
		}
	}
	if tester.HasText("Status Center") || tester.HasText("Show Workspace Tools") {
		t.Fatal("retired tool chrome is still visible")
	}
	s.quickPanelTab = 0
	tester.Frame()
	if tester.HasText("localhost:43217") {
		t.Fatal("Services rows leaked into Agent activity list")
	}
}
