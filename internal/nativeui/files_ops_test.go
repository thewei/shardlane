package nativeui

/**
 * [INPUT]: headless Files 树、filesview 忽略规则与文件操作域、共享文本/确认对话框通道
 * [OUTPUT]: FILES-01 忽略过滤与开关、FILES-02 新建/重命名/删除经共享对话框的回归
 * [POS]: 仅验证 nativeui 接线与缓存失效；域语义（守卫/逃逸）由 internal/filesview 测试覆盖
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
)

func filesOpsTestShell(t *testing.T) (*Shell, *ui.Tester, string) {
	t.Helper()
	root := t.TempDir()
	s := rightPanelTestShell(t, root)
	s.showViewFiles()
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	tester.Frame() // second frame lets the headless directory read land
	return s, tester, root
}

// submitDialogText closes the shared text dialog the way its frame path
// does, then dispatches the submit.
func submitDialogText(s *Shell, kind, target, value string) {
	s.dialogOpen = false
	s.submitTextDialog(kind, target, value)
}

// submitDialogConfirm closes the shared confirm the way its AlertDialog
// branch does, then dispatches the confirmation.
func submitDialogConfirm(s *Shell, kind, target string) {
	s.confirmOpen = false
	s.submitConfirm(kind, target)
}

// TestFilesTreeHidesIgnoredNamesUntilToggled pins the tree-side ignore
// filter: node_modules stays out of the render until "Show ignored files"
// turns on, and the empty-state rows tell the two cases apart.
func TestFilesTreeHidesIgnoredNamesUntilToggled(t *testing.T) {
	s, tester, root := filesOpsTestShell(t)
	if err := os.MkdirAll(filepath.Join(root, "node_modules", "pkg"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "node_modules", "pkg", "index.js"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "README.md"), []byte("hello"), 0o600); err != nil {
		t.Fatal(err)
	}
	empty := filepath.Join(root, "placeholder")
	if err := os.Mkdir(empty, 0o700); err != nil {
		t.Fatal(err)
	}
	s.refreshFiles()
	tester.Frame()
	if tester.HasText("node_modules") {
		t.Fatalf("node_modules must hide by default: %q", tester.Texts())
	}
	if !tester.HasText("README.md") {
		t.Fatalf("ordinary entries must stay visible: %q", tester.Texts())
	}

	// Expand the empty directory via direct state (headless click plumbing
	// is covered elsewhere): it renders the "Empty" quiet row.
	s.rightPanel.expandedDirs[empty] = true
	tester.Frame()
	if !tester.HasText("Empty") {
		t.Fatalf("empty directory state missing: %q", tester.Texts())
	}

	s.rightPanel.showIgnored = true
	tester.Frame()
	if !tester.HasText("node_modules") {
		t.Fatalf("toggle must reveal ignored entries: %q", tester.Texts())
	}
}

// TestFilesCreateRenameDeleteThroughDialogs drives the full dialog channel:
// the tree toolbar creates, the shared text dialog renames, and the shared
// confirm deletes; caches invalidate so the next frame shows the result.
func TestFilesCreateRenameDeleteThroughDialogs(t *testing.T) {
	s, tester, root := filesOpsTestShell(t)

	s.filesNewFile(root)
	submitDialogText(s, "files-new-file", root, "notes.txt")
	created := filepath.Join(root, "notes.txt")
	if _, err := os.Stat(created); err != nil {
		t.Fatalf("create failed: %v", err)
	}

	// The tree refreshes without a manual reload (cache invalidated).
	tester.Frame()
	if !tester.HasText("notes.txt") {
		t.Fatalf("created file missing from tree: %q", tester.Texts())
	}

	s.filesRename(created, "notes.txt")
	submitDialogText(s, "files-rename", created, "journal.txt")
	renamed := filepath.Join(root, "journal.txt")
	if _, err := os.Stat(created); !os.IsNotExist(err) {
		t.Fatal("old name survived rename")
	}
	if _, err := os.Stat(renamed); err != nil {
		t.Fatalf("rename failed: %v", err)
	}

	// Selecting the file then deleting it clears the selection too.
	s.openFilePreview(renamed)
	s.filesDelete(renamed, false)
	submitDialogConfirm(s, "files-delete", renamed)
	if _, err := os.Stat(renamed); !os.IsNotExist(err) {
		t.Fatal("delete failed")
	}
	if s.rightPanel.selectedFilePath != "" {
		t.Fatalf("selection survived delete: %q", s.rightPanel.selectedFilePath)
	}
	tester.Frame()
	if tester.HasText("journal.txt") {
		t.Fatalf("deleted file still rendered: %q", tester.Texts())
	}
}

// TestFilesDeleteFolderConfirmCarriesTheDirectoryCopy: the destructive
// confirm names a folder delete explicitly before anything is removed.
func TestFilesDeleteFolderConfirmCarriesTheDirectoryCopy(t *testing.T) {
	s, _, root := filesOpsTestShell(t)
	dir := filepath.Join(root, "bulky")
	if err := os.Mkdir(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	s.filesDelete(dir, true)
	if !s.confirmOpen || s.confirmKind != "files-delete" || s.confirmTarget != dir {
		t.Fatalf("confirm state = open:%v kind:%q target:%q", s.confirmOpen, s.confirmKind, s.confirmTarget)
	}
	if !strings.Contains(s.confirmMessage, "folder and everything inside it") {
		t.Fatalf("folder delete copy = %q", s.confirmMessage)
	}
	// Nothing is removed until the confirm submits.
	submitDialogConfirm(s, "files-delete", dir)
	if _, err := os.Stat(dir); !os.IsNotExist(err) {
		t.Fatal("folder survived confirmed delete")
	}
}
