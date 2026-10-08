package nativeui

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
)

// rightPanelTestShell builds a shell with an active project, tab, and pane with a concrete CWD.
func rightPanelTestShell(t *testing.T, cwd string) *Shell {
	t.Helper()
	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: cwd}},
		Tabs:     []herdr.Tab{{ID: "t1", Label: "main", ProjectID: "w1"}},
		Panes:    []herdr.Pane{{ID: "p1", TerminalID: "term-1", Label: "editor", TabID: "t1", CWD: cwd}},
	}
	shell.reconcileLocalSelection(shell.projection)
	return shell
}

// TestRightPanelToggleAndSurfaceSwitch pins the 0.10 Right Panel v2 contract:
// opening, closing, and switching tools (Changes/Files/Services). Lazygit
// no longer exists as a surface (GWB-020/070/071).
func TestRightPanelToggleAndSurfaceSwitch(t *testing.T) {
	dir := t.TempDir()
	shell := rightPanelTestShell(t, dir)

	if shell.rightPanel.open {
		t.Fatal("right panel must start closed by default")
	}

	// Toggle open
	shell.toggleRightPanel()
	if !shell.rightPanel.open {
		t.Fatal("expected right panel to open after toggle")
	}
	// Plan §10.2: no prior choice and no Git changes → Files.
	if shell.rightPanel.surface != SurfaceFiles {
		t.Fatalf("expected initial surface to be Files, got %s", shell.rightPanel.surface)
	}

	// Switch surface to Services and Files
	shell.openRightPanelSurface(SurfaceServices)
	if shell.rightPanel.surface != SurfaceServices {
		t.Fatalf("expected surface Services, got %s", shell.rightPanel.surface)
	}

	shell.openRightPanelSurface(SurfaceFiles)
	if shell.rightPanel.surface != SurfaceFiles {
		t.Fatalf("expected surface Files, got %s", shell.rightPanel.surface)
	}

	// Toggle close
	shell.toggleRightPanel()
	if shell.rightPanel.open {
		t.Fatal("expected right panel to close after toggle")
	}
}

// TestRightPanelLazygitRemoved pins the removal audit (GWB-021): no
// SurfaceLazygit value, no lazygit tool view remains on the target.
func TestRightPanelLazygitRemoved(t *testing.T) {
	// Compile-time evidence: the identifier below does not exist; this test
	// asserts the surfaces list to keep the target honest.
	if SurfaceChanges != "changes" || SurfaceFiles != "files" || SurfaceServices != "services" {
		t.Fatal("right panel surfaces drifted from Changes/Files/Services")
	}
	dir := t.TempDir()
	shell := rightPanelTestShell(t, dir)
	shell.toggleRightPanel()
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	if tester.HasText("Lazygit Tool") {
		t.Fatal("lazygit placeholder still rendered")
	}
}

// TestRightPanelSelectedTabCWDPinsToolRoot pins WIX-041: the active tool root
// follows the selected Tab CWD, not a global project root.
func TestRightPanelSelectedTabCWDPinsToolRoot(t *testing.T) {
	dir1 := t.TempDir()
	dir2 := t.TempDir()

	shell := NewShell()
	shell.activeInstance = "inst-1"
	shell.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: dir1}},
		Tabs: []herdr.Tab{
			{ID: "t1", Label: "tab-1", ProjectID: "w1"},
			{ID: "t2", Label: "tab-2", ProjectID: "w1"},
		},
		Panes: []herdr.Pane{
			{ID: "p1", TerminalID: "term-1", Label: "pane-1", TabID: "t1", CWD: dir1},
			{ID: "p2", TerminalID: "term-2", Label: "pane-2", TabID: "t2", CWD: dir2},
		},
	}

	// Select Tab 1
	shell.selectTab("t1")
	if got := shell.selectedTabCWD(); got != dir1 {
		t.Fatalf("expected tab 1 cwd %q, got %q", dir1, got)
	}

	// Select Tab 2
	shell.selectTab("t2")
	if got := shell.selectedTabCWD(); got != dir2 {
		t.Fatalf("expected tab 2 cwd %q, got %q", dir2, got)
	}
}

// TestRightPanelFilesTreeAndPreview pins WIX-050..055: background directory
// snapshots (no render-time IO), expand/collapse, and the preview modal.
func TestRightPanelFilesTreeAndPreview(t *testing.T) {
	dir := t.TempDir()

	subDir := filepath.Join(dir, "docs")
	if err := os.Mkdir(subDir, 0o755); err != nil {
		t.Fatal(err)
	}
	testFilePath := filepath.Join(dir, "README.md")
	testFileContent := "# My Project\nDocumentation here."
	if err := os.WriteFile(testFilePath, []byte(testFileContent), 0o600); err != nil {
		t.Fatal(err)
	}

	shell := rightPanelTestShell(t, dir)
	shell.openRightPanelSurface(SurfaceFiles)

	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	// First frame schedules the background read; run more frames until the
	// snapshot lands (bounded wait, no synchronous render IO).
	seen := false
	for i := 0; i < 50 && !seen; i++ {
		if tester.HasText("README.md") && tester.HasText("docs") {
			seen = true
			break
		}
		tester.Frame()
	}
	if !seen {
		t.Fatalf("file tree missing entries after background load; texts=%q", tester.Texts())
	}

	// Open file preview modal
	shell.openFilePreview(testFilePath)
	tester.Frame()

	if !shell.rightPanel.previewOpen || shell.rightPanel.preview == nil {
		t.Fatal("expected file preview modal to be open with preview data")
	}
	if shell.rightPanel.preview.Content != testFileContent {
		t.Fatalf("preview content mismatch: got %q, want %q", shell.rightPanel.preview.Content, testFileContent)
	}
	if !tester.HasText("Copy Path") {
		t.Fatalf("preview modal missing 'Copy Path' button; texts=%q", tester.Texts())
	}

	shell.rightPanel.previewOpen = false
	tester.Frame()
	if shell.rightPanel.previewOpen {
		t.Fatal("expected preview modal to close")
	}
}

// TestRightPanelServicesSurfaces pins the Services surface contract: real
// observed ports only (no 3000/5173/8080 samples) and script listing.
func TestRightPanelServicesSurfaces(t *testing.T) {
	dir := t.TempDir()
	shell := rightPanelTestShell(t, dir)

	shell.openRightPanelSurface(SurfaceServices)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	if !tester.HasText("Listening Ports & Local Preview") {
		t.Fatalf("Services surface missing Ports section; texts=%q", tester.Texts())
	}
	if tester.HasText(":3000") || tester.HasText(":5173") || tester.HasText(":8080") {
		t.Fatalf("hard-coded sample ports rendered: %q", tester.Texts())
	}
	// With no observed listeners the honest empty state shows.
	if !tester.HasText("No listening local ports observed.") {
		t.Fatalf("missing honest empty ports state; texts=%q", tester.Texts())
	}
}

// TestRightPanelChangesWithoutRepository shows the honest empty state when
// the selected tab is outside any Git repository (GWB-070).
func TestRightPanelChangesWithoutRepository(t *testing.T) {
	dir := t.TempDir()
	shell := rightPanelTestShell(t, dir)
	shell.toggleRightPanel()
	shell.openRightPanelSurface(SurfaceChanges)

	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	if !tester.HasText("No Git Repository") {
		t.Fatalf("expected no-repository empty state; texts=%q", tester.Texts())
	}
}
