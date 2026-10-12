package nativeui

/**
 * [INPUT]: gitSurfaceTestShell 的内存快照、headless Tester（900/1700 宽度）
 * [OUTPUT]: Diff 文件卡片头在窄窗口下让位于文件名、宽窗口下保留全部控件的回归
 * [POS]: gdFileHeader 的响应式契约；宽度估算（gdCardWidth）与 gitFileNavigationColumn 共用同一来源
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// gdHeaderShell opens the Diff surface on a file whose name needs room.
func gdHeaderShell(t *testing.T) *Shell {
	t.Helper()
	shell := gitSurfaceTestShell(t, t.TempDir())
	shell.git.snapshot.Files[0].Path = "internal/nativeui/diff_surface.go"
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	shell.buildGdRows()
	return shell
}

// At ~900 px with the sidebar and the file navigator open (a window this
// narrow closes the navigator until the user opens it), the card is about
// 330 px wide: the header must spend it on the name, not on tools.
func TestDiffHeaderKeepsItsFileNameWhenNarrow(t *testing.T) {
	shell := gdHeaderShell(t)
	shell.gitFilePaneOpenNarrow = true
	tester := ui.NewTester(shell.View, 900, 800)
	tester.Frame()

	name, ok := tester.Find("diff_surface.go")
	if !ok {
		t.Fatalf("the header does not show the file name: %q", tester.Texts())
	}
	// 15 monospace columns: anything much under 100 is an ellipsis stub.
	if name.W < 100 {
		t.Fatalf("file name squeezed to %.0f px at 900 px wide", name.W)
	}
	if dir, ok := tester.Find("internal/nativeui/"); !ok || dir.Y <= name.Y {
		t.Errorf("a narrow header shows the folder below the name: dir=%v (found %v) name=%v", dir, ok, name)
	}
	for _, tool := range []string{"Copy path", "Open file in editor"} {
		if _, ok := tester.Find(tool); ok {
			t.Errorf("%q still takes room in a narrow header", tool)
		}
	}
	viewed, ok := tester.Find("Viewed")
	if !ok {
		t.Fatalf("a narrow header keeps its Viewed toggle: %q", tester.Texts())
	}
	if viewed.W > 44 {
		t.Errorf("Viewed is %.0f px wide in a narrow header, want the box alone", viewed.W)
	}
}

// A wide card has room for everything, so nothing steps aside.
func TestDiffHeaderKeepsEveryControlWhenWide(t *testing.T) {
	shell := gdHeaderShell(t)
	tester := ui.NewTester(shell.View, 1700, 800)
	tester.Frame()

	if _, ok := tester.Find("Open file in editor"); !ok {
		t.Fatalf("a wide header lost its Open button: %q", tester.Texts())
	}
	viewed, ok := tester.Find("Viewed")
	if !ok || viewed.W < 80 {
		t.Fatalf("a wide header shows Viewed with its label, got %v (found %v)", viewed, ok)
	}
	name, ok := tester.Find("diff_surface.go")
	if !ok || name.W < 100 {
		t.Fatalf("file name = %v (found %v)", name, ok)
	}
	if dir, ok := tester.Find("internal/nativeui/"); !ok || dir.Y != name.Y {
		t.Errorf("a wide header keeps the folder on the name's line: dir=%v (found %v) name=%v", dir, ok, name)
	}
}

// The card's width follows the window, the sidebar and the navigator, and the
// navigator can be dragged wider. A narrow center has no navigator to
// subtract until the user opens it.
func TestGdCardWidth(t *testing.T) {
	shell := gdHeaderShell(t)
	var ctxWidth float32
	probe := func(c *ui.Context) {
		ctxWidth = shell.gdCardWidth(c)
		ui.Box(c)
	}
	ui.NewTester(probe, 1340, 800)

	want := 1340 - sidebarWidth - gitFilePaneDefaultWidth - gitFileSplitterWidth - 2*gdListMargin
	if ctxWidth != want {
		t.Fatalf("card width = %v, want %v", ctxWidth, want)
	}

	shell.sidebarCollapsed = true
	ui.NewTester(probe, 1340, 800).Frame()
	if ctxWidth != want+sidebarWidth {
		t.Fatalf("with the sidebar hidden the card width = %v, want %v", ctxWidth, want+sidebarWidth)
	}

	shell.sidebarCollapsed = false
	shell.gitFilePaneWidth = 400
	ui.NewTester(probe, 1340, 800).Frame()
	if ctxWidth != want-(400-gitFilePaneDefaultWidth) {
		t.Fatalf("with a wider navigator the card width = %v, want %v", ctxWidth, want-(400-gitFilePaneDefaultWidth))
	}

	shell.gitFilePaneWidth = gitFilePaneDefaultWidth
	ui.NewTester(probe, 900, 800).Frame()
	if want := float32(900 - sidebarWidth - 2*gdListMargin); ctxWidth != want {
		t.Fatalf("a narrow window closes the navigator: card width = %v, want %v", ctxWidth, want)
	}

	shell.gitFilePaneOpenNarrow = true
	ui.NewTester(probe, 900, 800).Frame()
	if want := float32(900 - sidebarWidth - gitFilePaneDefaultWidth - gitFileSplitterWidth - 2*gdListMargin); ctxWidth != want {
		t.Fatalf("a narrow window with the navigator opened: card width = %v, want %v", ctxWidth, want)
	}
}
