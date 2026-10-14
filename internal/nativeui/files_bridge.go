package nativeui

/**
 * [INPUT]: rightPanel.selectedFilePath/currentRoot、filesview.ReadPreview/ReadImageDataURL（flavor 上限 + symlink 守卫）、platform.OpenURL、mygo Bind 事件
 * [OUTPUT]: FilesPreviewBridge 绑定服务（Current/Open/OpenExternal）与 files:preview 事件、Shell.publishFilesPreview（有序发布）
 * [POS]: Files 预览 WebView（产品决策 2026-10-13，2026-10-14 收窄为 Markdown 渲染与网页图片）唯一表现层桥：只读——页面没有任何写通道（OpenExternal 仅放行 http/https 出站），文件操作仍走原生对话框 + filesview 域服务；文本/代码由原生 editor 显示、WebView 兜不住的文件由 Native 兜底层接管，均不经此桥；不新增 Herdr 方法，无 localhost HTTP
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"errors"
	"net/url"
	"os"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo"
	"github.com/wh-studio/herdr-client/internal/filesview"
	"github.com/wh-studio/herdr-client/internal/platform"
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

// FilesPreviewContent is one read-only file the page renders. Flavor tells
// the page which view to take: "markdown" (rendered) or "image" (DataURL);
// text and code never reach the page.
type FilesPreviewContent struct {
	Path      string `json:"path"`
	Name      string `json:"name"`
	SizeBytes int64  `json:"sizeBytes"`
	Flavor    string `json:"flavor"`
	IsBinary  bool   `json:"isBinary"`
	Truncated bool   `json:"truncated"`
	Theme     string `json:"theme"`
	Content   string `json:"content"`
	DataURL   string `json:"dataUrl,omitempty"`
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

// Open reads one file for preview. The request must be a file the page
// owns — the live selection, planned for the WebView tier — so a stale
// page request after the user clicked elsewhere, or for a file the native
// code view shows, is refused instead of answering with content the tree
// no longer shows. The flavor routes the read: Markdown pulls through
// ReadPreview (1 MiB cap, symlink-guarded); images become a data URL
// (24 MiB cap). Text never crosses the bridge — the native code view owns
// it — so an error here is a stale race, not a user-facing state.
func (b *FilesPreviewBridge) Open(ctx context.Context, path string) (FilesPreviewContent, error) {
	var root string
	var owned bool
	b.shell.callOnWindow(func() {
		root = b.shell.rightPanel.currentRoot
		owned = root != "" && b.shell.webPreviewOwns(path)
	})
	if !owned {
		return FilesPreviewContent{}, errors.New("the file is no longer selected")
	}
	content := FilesPreviewContent{Path: path, Name: filepath.Base(path)}
	switch flavor := filesview.PreviewFlavor(path); flavor {
	case filesview.FlavorImage:
		url, err := filesview.ReadImageDataURL(path)
		if err != nil {
			return FilesPreviewContent{}, err
		}
		content.Flavor = string(flavor)
		content.DataURL = url
		if info, err := os.Stat(path); err == nil {
			content.SizeBytes = info.Size()
		}
	case filesview.FlavorMarkdown:
		preview, err := filesview.ReadPreview(path, root)
		if err != nil {
			return FilesPreviewContent{}, err
		}
		content.Flavor = string(flavor)
		content.SizeBytes = preview.SizeBytes
		content.IsBinary = preview.IsBinary
		content.Truncated = preview.Truncated
		content.Content = preview.Content
	default:
		return FilesPreviewContent{}, errors.New("this file previews natively")
	}
	b.shell.callOnWindow(func() {
		content.Theme = b.shell.chatBridgeTheme()
	})
	return content, nil
}

// OpenExternal hands an http(s) link from the page to the system browser.
// The scheme is the whole gate: file:, javascript:, data: and everything
// else are refused — the read-only page gets no filesystem or protocol
// surface beyond this one door.
func (b *FilesPreviewBridge) OpenExternal(ctx context.Context, rawURL string) error {
	parsed, err := url.Parse(strings.TrimSpace(rawURL))
	if err != nil || (parsed.Scheme != "http" && parsed.Scheme != "https") || parsed.Host == "" {
		return errors.New("only http(s) links open in the browser")
	}
	target := parsed.String()
	var openErr error
	b.shell.callOnWindow(func() {
		openErr = platform.OpenURL(target)
	})
	return openErr
}

// filesPreviewTargetState projects the selection for the page (UI lane).
func (s *Shell) filesPreviewTargetState() FilesPreviewTarget {
	target := FilesPreviewTarget{Theme: s.chatBridgeTheme()}
	// The page hears only a file it owns: one the native tier shows, or one
	// still being planned, is no business of it, so it never lingers on
	// another file's content under this name.
	if path := s.rightPanel.selectedFilePath; s.webPreviewOwns(path) {
		target.Path = path
		target.Name = filepath.Base(path)
	}
	return target
}

// webPreviewOwns reports whether the WebView page is the tier showing path:
// it is the selection, its plan has landed, and the native tier did not take
// it (UI lane).
func (s *Shell) webPreviewOwns(path string) bool {
	st := &s.rightPanel
	return path != "" && st.selectedFilePath == path && st.previewPlanned == path && st.nativePreviewPath != path
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
