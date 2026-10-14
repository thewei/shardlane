package nativeui

/**
 * [INPUT]: 依赖 General.WebViewFilesPreview 偏好、主窗口 mygo.WebView（ui.WebView 元素）
 * [OUTPUT]: 提供 Shell.filesWebviewFor/filesWebviewBody：/files 中央里 Markdown 渲染与网页图片的 WebView 接管、惰性创建与失败锁存
 * [POS]: 产品决策 2026-10-13/14：WebView 只承担 HTML 擅长的预览（文本/代码已回原生 editor），左树/头部/查看条/操作保持原生；偏好关闭时计划不再把文件交给它，创建失败时显示真实空态
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"log/slog"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

// useWebviewPreview reports the General.WebViewFilesPreview preference
// (off: the plan keeps every file on the native tier).
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
