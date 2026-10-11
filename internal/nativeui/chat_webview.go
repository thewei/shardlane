package nativeui

import (
	"log/slog"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 General.WebViewChat 偏好、主窗口 mygo.WebView（ui.WebView 元素）、原生 chat 表面回退
 * [OUTPUT]: 提供 Shell.chatWebviewBody（启用时渲染 WebView 聊天体并接管）、惰性创建与失败锁存
 * [POS]: N19 Phase 2 的路由替换点：仅替换 /workspace 与 /chat 的会话体，头部/导航/状态带保持原生；失败或关闭偏好即回退原生 Chat
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// useWebviewChat reports the General.WebViewChat preference (the escape
// hatch back to the Native chat surface).
func (s *Shell) useWebviewChat() bool {
	return s.settings.General.WebViewChat
}

// chatWebviewFor returns the lazily created chat page WebView, or nil when
// the preference is off, no window exists, or creation failed once (latched
// so the native chat stays the surface). The web view shows only while a
// frame builds its ui.WebView element, so keeping it created while another
// surface shows costs nothing. Call on the UI lane.
func (s *Shell) chatWebviewFor() *mygo.WebView {
	if !s.useWebviewChat() || s.win == nil || s.chatWebviewBroken {
		return nil
	}
	if s.chatWebview == nil {
		view, err := s.win.NewWebView(mygo.WebViewOptions{URL: "/chat"})
		if err != nil {
			s.chatWebviewBroken = true
			slog.Warn("create chat webview failed; native chat stays", "error", err)
			return nil
		}
		s.chatWebview = view
	}
	return s.chatWebview
}

// chatWebviewBody renders the WebView chat body when the preference is on
// and the page exists, reporting whether it took the surface over.
func (s *Shell) chatWebviewBody(c *ui.Context) bool {
	view := s.chatWebviewFor()
	if view == nil {
		return false
	}
	ui.WebView(c, view).Grow(1)
	return true
}
