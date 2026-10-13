package nativeui

/**
 * [INPUT]: filesview.CreateFile/CreateDir/Rename/Delete、rightPanel 缓存围栏（filesGen）与右键菜单/工具条动作
 * [OUTPUT]: Files 树文件操作的执行通道：对话框分发（dialogs.go）→ 后台执行 → 缓存失效/局部刷新/Toast 与选中迁移
 * [POS]: 仅本地文件系域能力（internal/filesview 域服务）；不触碰 git index，不新增 Herdr 方法；删除为永久操作，确认框走共享 AlertDialog
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"
	"log/slog"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/filesview"
)

// filesNewFile opens the shared text dialog to create a file in dir (the
// tree root or a folder row's absolute path).
func (s *Shell) filesNewFile(dir string) {
	s.openTextDialog("files-new-file", dir, "New File", "Name", "")
	s.dialogAction = "Create"
}

// filesNewFolder opens the shared text dialog to create a folder in dir.
func (s *Shell) filesNewFolder(dir string) {
	s.openTextDialog("files-new-dir", dir, "New Folder", "Name", "")
	s.dialogAction = "Create"
}

// filesRename opens the shared text dialog renaming path in place.
func (s *Shell) filesRename(path, current string) {
	s.openTextDialog("files-rename", path, "Rename", "Name", current)
	s.dialogAction = "Rename"
}

// filesDelete routes through the shared destructive confirm; the confirm
// never default-focuses the destructive button (F69).
func (s *Shell) filesDelete(path string, isDir bool) {
	name := filepath.Base(path)
	message := "The file will be permanently deleted."
	if isDir {
		message = "The folder and everything inside it will be permanently deleted."
	}
	s.openConfirm("files-delete", path, fmt.Sprintf("Delete “%s”?", name), message)
}

// filesRowMenu is a tree row's right-click menu: creation inside folders,
// rename/delete, and the read-only helpers.
func (s *Shell) filesRowMenu(m *ui.Menu, e filesview.Entry) {
	isDir := e.IsDir && !e.IsSymlink
	if isDir {
		if m.Item("New File…").Chosen() {
			s.filesNewFile(e.Path)
		}
		if m.Item("New Folder…").Chosen() {
			s.filesNewFolder(e.Path)
		}
		m.Separator()
	}
	if m.Item("Rename…").Chosen() {
		s.filesRename(e.Path, e.Name)
	}
	if m.Item("Delete…").Chosen() {
		s.filesDelete(e.Path, isDir)
	}
	m.Separator()
	if m.Item("Copy Path").Chosen() {
		s.copyToClipboard(e.Path)
	}
	if m.Item("Reveal in Finder").Chosen() {
		s.openExternalPathParent(e.Path)
	}
}

// filesDialogLive guards dialog submits: the Files dialogs render every
// frame regardless of route, so a dialog carried off /files (⌘[, ⌘K, tray)
// must not act on the frozen root. The git dialogs guard the same way by
// repository root.
func (s *Shell) filesDialogLive() bool {
	if s.router.Path() != routeFiles {
		s.pendingToast = "The Files page closed. Reopen the action there."
		return false
	}
	return true
}

// runFilesMutation executes a dialog-backed create/rename. The dialog
// target (creation parent, renamed entry) is an absolute path captured when
// the dialog opened; it must still live under the tree root.
func (s *Shell) runFilesMutation(kind, target, value string) {
	root := s.rightPanel.currentRoot
	value = strings.TrimSpace(value)
	if value == "" {
		return
	}
	if !s.filesDialogLive() {
		return
	}
	if !s.filesTargetInRoot(target) {
		s.pendingToast = "The folder changed. Try again in the new folder."
		return
	}
	rel, err := filepath.Rel(root, target)
	if err != nil {
		return
	}
	switch kind {
	case "files-new-file":
		s.runFilesOp("create file", func() (string, error) {
			return filesview.CreateFile(root, rel, value)
		}, target, "")
	case "files-new-dir":
		s.runFilesOp("create folder", func() (string, error) {
			return filesview.CreateDir(root, rel, value)
		}, target, "")
	case "files-rename":
		if filepath.Base(target) == value {
			return // nothing to do; close quietly
		}
		parent := filepath.Dir(target)
		s.runFilesOp("rename", func() (string, error) {
			return filesview.Rename(root, rel, value)
		}, parent, target)
	}
}

// runFilesDelete executes a confirmed delete in the background — removing a
// large folder must not hold the UI lane.
func (s *Shell) runFilesDelete(target string) {
	if !s.filesDialogLive() {
		return
	}
	root := s.rightPanel.currentRoot
	if !s.filesTargetInRoot(target) {
		s.pendingToast = "The folder changed. Try again in the new folder."
		return
	}
	rel, err := filepath.Rel(root, target)
	if err != nil {
		return
	}
	name := filepath.Base(target)
	s.status = fmt.Sprintf("Deleting “%s”…", name)
	s.runFilesOp("delete", func() (string, error) {
		_, err := filesview.Delete(root, rel)
		return "", err // deleted: no path survives for the selection to follow
	}, filepath.Dir(target), target)
}

// filesTargetInRoot reports whether an absolute dialog target still sits
// under the tree root (literal containment; symlink guards live in the
// filesview domain).
func (s *Shell) filesTargetInRoot(target string) bool {
	root := s.rightPanel.currentRoot
	if root == "" || target == "" {
		return false
	}
	rel, err := filepath.Rel(root, target)
	return err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator))
}

// runFilesOp is the single execution channel: run the domain operation off
// the UI lane, then invalidate caches, refresh the affected directory,
// migrate/clear the selection, and surface failures as a bounded toast.
// oldPath is set for rename (selection follows the new name) and delete
// (selection inside the removed path clears).
func (s *Shell) runFilesOp(op string, run func() (string, error), refreshDir, oldPath string) {
	root := s.rightPanel.currentRoot
	gen := s.rightPanel.filesGen.Load()
	apply := func(newPath string, err error) {
		// The status clears even when the tree moved on: the operation
		// finished (or was abandoned) either way.
		if strings.HasPrefix(s.status, "Deleting “") {
			s.status = ""
		}
		if gen != s.rightPanel.filesGen.Load() {
			return
		}
		if s.rightPanel.currentRoot != root {
			return // the tree moved on; nothing to refresh
		}
		if err != nil {
			slog.Warn("files op failed", "op", op)
			s.pendingToast = err.Error()
			return
		}
		slog.Info("files op done", "op", op)
		s.migrateFilesSelection(oldPath, newPath)
		s.invalidateFilesTree(refreshDir, oldPath)
		// The operation may have changed ignore semantics (a new
		// .gitignore, a now-matching *.log, a deleted ignore file):
		// re-arm the git lookup for the next render.
		s.rightPanel.ignoredRoot = ""
		s.requestDirectory(refreshDir)
	}
	if s.win == nil {
		// Headless (tests): run inline like requestDirectory.
		newPath, err := run()
		apply(newPath, err)
		return
	}
	go func() {
		newPath, err := run()
		s.applyOnWindow(func() { apply(newPath, err) })
	}()
}

// migrateFilesSelection keeps the preview coherent across mutations: a
// renamed selection follows its file (the native tier's content moves with
// it — a rename changes neither flavor nor size); a selection inside a
// removed path clears. The preview bridge hears both.
func (s *Shell) migrateFilesSelection(oldPath, newPath string) {
	st := &s.rightPanel
	if st.selectedFilePath == "" || oldPath == "" {
		return
	}
	switch {
	case st.selectedFilePath == oldPath && newPath != "":
		st.selectedFilePath = newPath
		if st.nativePreviewPath == oldPath {
			st.nativePreviewPath = newPath
			return
		}
		s.publishFilesPreview()
	case st.selectedFilePath == oldPath || strings.HasPrefix(st.selectedFilePath, oldPath+string(filepath.Separator)):
		st.selectedFilePath = ""
		s.clearNativeFilesPreview()
		s.publishFilesPreview()
	}
}

// invalidateFilesTree drops cached snapshots under refreshDir and, for a
// removed/renamed-away path, every expanded or cached descendant.
func (s *Shell) invalidateFilesTree(dir, removed string) {
	st := &s.rightPanel
	delete(st.cachedDirEntries, dir)
	if removed == "" {
		return
	}
	prefix := removed + string(filepath.Separator)
	for path := range st.cachedDirEntries {
		if path == removed || strings.HasPrefix(path, prefix) {
			delete(st.cachedDirEntries, path)
		}
	}
	for path := range st.expandedDirs {
		if path == removed || strings.HasPrefix(path, prefix) {
			delete(st.expandedDirs, path)
		}
	}
}
