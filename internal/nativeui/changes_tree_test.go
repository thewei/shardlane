package nativeui

/**
 * [INPUT]: 官方 MyGo headless Tester 与 Git 侧栏夹具（src/main.go 同时有暂存与未暂存两侧）
 * [OUTPUT]: 验证 Changes 文件树与 Git/Files 树共用对齐网格（文件夹与文件标题逐层缩进一格）、行内 Stage/Unstage 为图标按钮而不是文字 "+" / "−"
 * [POS]: Diff 侧栏文件树呈现回归；网格常量只在 sidebar_tree.go 一处生效
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func changesTreeShell(t *testing.T) (*Shell, *ui.Tester) {
	t.Helper()
	root := t.TempDir()
	shell := gitSurfaceTestShell(t, root)
	shell.git.rootOf[root] = root
	cf := &shell.git.snapshot.Files[0]
	cf.Staged, cf.Unstaged = true, true
	if shell.router.Path() != routeWorkspace {
		shell.router.Push(routeWorkspace)
	}
	shell.showSurface(WorkspaceSurfaceDiff)
	shell.rebuildDiffRows()
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	tester.Frame()
	return shell, tester
}

// A file under a folder sits exactly one indent right of the folder's own
// title: both read from the same icon and title columns as the Git and Files
// trees.
func TestChangesRowsShareTheSidebarGrid(t *testing.T) {
	_, tester := changesTreeShell(t)
	dir, ok := tester.Find("src")
	if !ok {
		t.Fatalf("folder row missing: %q", tester.Texts())
	}
	file, ok := tester.Find("main.go")
	if !ok {
		t.Fatalf("file row missing: %q", tester.Texts())
	}
	if got := file.X - dir.X; !near(got, TreeIndentWidth) {
		t.Fatalf("file title sits %.1f right of its folder's, want one indent (%v)", got, float32(TreeIndentWidth))
	}
}

// Row actions are icon buttons named for what they do; no row draws a bare
// "+" or "−" as text.
func TestChangesRowActionsAreNamedIconButtons(t *testing.T) {
	shell, tester := changesTreeShell(t)
	if texts := tester.Texts(); containsText(texts, "+") || containsText(texts, "−") {
		t.Fatalf("a row action is still a text glyph: %q", texts)
	}
	for _, label := range []string{"Stage folder src", "Stage src/main.go", "Unstage src/main.go"} {
		if _, ok := tester.Find(label); !ok {
			t.Fatalf("row action %q missing: %q", label, tester.Texts())
		}
	}
	if err := tester.Click("Stage src/main.go"); err != nil {
		t.Fatal(err)
	}
	if !shell.git.opBusy {
		t.Fatal("the row's Stage icon did not start the operation")
	}
}
