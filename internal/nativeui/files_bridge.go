package nativeui

/**
 * [INPUT]: rightPanel.selectedFilePath/currentRoot、filesview.ReadPreview（1 MiB 上限 + symlink 守卫）、mygo Bind 事件
 * [OUTPUT]: FilesPreviewBridge 绑定服务（Current/Open）与 files:preview 事件、Shell.publishFilesPreview
 * [POS]: Files 预览 WebView（产品决策 2026-10-13）唯一表现层桥：只读——页面没有任何写通道，文件操作仍走原生对话框 + filesview 域服务；不新增 Herdr 方法，无 localhost HTTP
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"errors"
	"path/filepath"

	"github.com/egoist/mygo"
	"github.com/wh-studio/herdr-client/internal/filesview"
)

// FilesPreviewTarget is the selection metadata the page hears through the
// files:preview event. An empty Path clears the preview. Content is
// deliberately absent: events stay small, the page pulls through Open.
type FilesPreviewTarget struct {
	Path  string `json:"path"`
	Name  string `json:"name,omitempty"`
	Theme string `json:"theme"`
}

// filesPreviewSelected fires whenever the tree selection changes. Events
// emitted before the page is ready are replayed by the runtime, so the
// page also starts from Current and stays current through this event alone.
var filesPreviewSelected = mygo.NewEvent[FilesPreviewTarget]("files:preview")

// FilesPreviewContent is one read-only file the page renders.
type FilesPreviewContent struct {
	Path      string `json:"path"`
	Name      string `json:"name"`
	SizeBytes int64  `json:"sizeBytes"`
	IsBinary  bool   `json:"isBinary"`
	Truncated bool   `json:"truncated"`
	Theme     string `json:"theme"`
	Content   string `json:"content"`
}

// FilesPreviewBridge is the bound service of the WebView Files preview.
// Bound methods run on their own goroutines; every Shell-state read
// marshals onto the window UI lane first. Strictly read-only: the page has
// no mutation channel — file operations stay native.
type FilesPreviewBridge struct {
	shell *Shell
}

// NewFilesPreviewBridge builds the files preview bound service.
func NewFilesPreviewBridge(shell *Shell) *FilesPreviewBridge {
	return &FilesPreviewBridge{shell: shell}
}

// Current returns the tree's current selection metadata (empty when no
// file is selected). The page calls it once on mount.
func (b *FilesPreviewBridge) Current(ctx context.Context) (FilesPreviewTarget, error) {
	var target FilesPreviewTarget
	b.shell.callOnWindow(func() {
		target = b.shell.filesPreviewTargetState()
	})
	return target, nil
}

// Open reads one file for preview. The request must still match the live
// selection — a stale page request after the user clicked elsewhere is
// refused instead of answering with content the tree no longer shows.
// The read (up to PreviewCap, symlink-guarded) runs on this bound
// goroutine; theme resolution marshals back onto the UI lane.
func (b *FilesPreviewBridge) Open(ctx context.Context, path string) (FilesPreviewContent, error) {
	var root string
	var selected bool
	b.shell.callOnWindow(func() {
		root = b.shell.rightPanel.currentRoot
		selected = root != "" && path == b.shell.rightPanel.selectedFilePath
	})
	if !selected {
		return FilesPreviewContent{}, errors.New("the file is no longer selected")
	}
	preview, err := filesview.ReadPreview(path, root)
	if err != nil {
		return FilesPreviewContent{}, err
	}
	content := FilesPreviewContent{
		Path:      preview.Path,
		Name:      filepath.Base(preview.Path),
		SizeBytes: preview.SizeBytes,
		IsBinary:  preview.IsBinary,
		Truncated: preview.Truncated,
		Content:   preview.Content,
	}
	b.shell.callOnWindow(func() {
		content.Theme = b.shell.chatBridgeTheme()
	})
	return content, nil
}

// filesPreviewTargetState projects the selection for the page (UI lane).
func (s *Shell) filesPreviewTargetState() FilesPreviewTarget {
	target := FilesPreviewTarget{Theme: s.chatBridgeTheme()}
	if path := s.rightPanel.selectedFilePath; path != "" {
		target.Path = path
		target.Name = filepath.Base(path)
	}
	return target
}

// publishFilesPreview pushes the current selection to the WebView page.
// Safe from render closures: the emit marshals onto the UI lane through a
// follow-up window update, never inside the frame being built. Publishes
// serialize through a trailing-pending chain, so rapid selections reach
// the page in order with the latest state winning.
func (s *Shell) publishFilesPreview() {
	if s.win == nil {
		return
	}
	s.filesPublishMu.Lock()
	if s.filesPublishing {
		s.filesPublishPending = true
		s.filesPublishMu.Unlock()
		return
	}
	s.filesPublishing = true
	s.filesPublishMu.Unlock()
	go func() {
		for {
			s.callOnWindow(func() {
				_ = filesPreviewSelected.Emit(s.win, s.filesPreviewTargetState())
			})
			s.filesPublishMu.Lock()
			if !s.filesPublishPending {
				s.filesPublishing = false
				s.filesPublishMu.Unlock()
				return
			}
			s.filesPublishPending = false
			s.filesPublishMu.Unlock()
		}
	}()
}
