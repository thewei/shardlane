package nativeui

// [INPUT]: 依赖 shell.go 的 Shell（win/selectedTabID/settings）、dialogs.go 的 openConfirm 通道、router.go 的 currentRouteTitle
// [OUTPUT]: 对外提供 syncWindowTitle（路由驱动的原生窗口标题）、CloseTabFromMenu（菜单栏 ⌘W 关闭当前 Tab）
// [POS]: window_actions 的原生窗口行为补齐层：F103/F104 的落地文件，避免触碰并行会话持有的 shell.go/titlebar.go
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

import "fmt"

// syncWindowTitle mirrors the current route into the native window title.
// The in-titlebar route text (F17) fixed the in-app chrome only; Mission
// Control, ⌘Tab and window pickers kept reading the static launch title.
// The workspace route keeps the plain app name so the window does not
// advertise a project that changed underneath it.
func (s *Shell) syncWindowTitle() {
	if s.win == nil {
		return
	}
	title := "Shardlane"
	if route := s.currentRouteTitle(); route != "" && route != "Workspace" && route != "Shardlane" {
		title = fmt.Sprintf("%s — %s", "Shardlane", route)
	}
	s.win.SetTitle(title)
}

// CloseTabFromMenu is the menu-bar ⌘W target (F103): the Window role menu
// used to bind ⌘W to closing the whole window while the app model is
// tab-centric. Reuse the same guarded confirm channel as the sidebar's
// Close Tab so the action family stays single-sourced.
func (s *Shell) CloseTabFromMenu() {
	if s.selectedTabID == "" || s.loading {
		return
	}
	s.openConfirm("close-tab", s.selectedTabID, "Close Tab?", "This closes the selected Herdr Tab and all Panes in it.")
}
