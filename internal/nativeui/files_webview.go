package nativeui

/**
 * [INPUT]: 依赖 General.WebViewFilesPreview 偏好、主窗口 mygo.WebView（ui.WebView 元素）
 * [OUTPUT]: 提供 Shell.filesWebviewFor/filesWebviewBody：/files 中央预览的 WebView 接管、惰性创建与失败锁存
 * [POS]: 产品决策 2026-10-13 的路由替换点：仅替换 /files 的预览中心，左树/头部/操作保持原生；失败或关闭偏好即显示真实空态（原生预览渲染器已退役）
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"log/slog"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

// useWebviewPreview reports the General.WebViewFilesPreview preference
// (the escape hatch back to the plain "preview unavailable" states).
func (s *Shell) useWebviewPreview() bool {
	return s.settings.General.WebViewFilesPreview
}

// filesWebviewFor returns the lazily created files preview WebView, or nil
// when the preference is off, no window exists, or creation failed once
// (latched so the page keeps working without the preview). Like the chat
// page's view, it shows only while a frame builds its ui.WebView element.
// Call on the UI lane.
func (s *Shell) filesWebviewFor() *mygo.WebView {
	if !s.useWebviewPreview() || s.win == nil || s.filesWebviewBroken {
		return nil
	}
	if s.filesWebview == nil {
		view, err := s.win.NewWebView(mygo.WebViewOptions{URL: "/files-viewer"})
		if err != nil {
			s.filesWebviewBroken = true
			slog.Warn("create files preview webview failed; preview states stay truthful", "error", err)
			return nil
		}
		s.filesWebview = view
	}
	return s.filesWebview
}

// filesWebviewBody renders the WebView preview body when the preference is
// on and the page exists, reporting whether it took the center over.
func (s *Shell) filesWebviewBody(c *ui.Context) bool {
	view := s.filesWebviewFor()
	if view == nil {
		return false
	}
	ui.WebView(c, view).Grow(1)
	return true
}
