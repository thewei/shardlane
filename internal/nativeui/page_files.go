package nativeui

/**
 * [INPUT]: selected Herdr Tab working directory、rightPanel.selectedFilePath、General.WebViewFilesPreview 与 filesWebview
 * [OUTPUT]: /files 独立页面：中央预览由 filesPreview WebView 接管（2026-10-13 产品决策），偏好关闭或创建失败时给出真实空态
 * [POS]: 左树保持原生；页面标题、根路径与文件动作（Copy Path / Reveal in Finder）归标题栏 Actions（route_header.go），页面只画内容；原生预览渲染器已按 2026-10-13 产品决策退役，页面内容只经只读桥，偏好关闭或创建失败时给出真实空态
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"github.com/egoist/mygo/ui"
)

// showViewFiles toggles the Files workbench; the active Tab CWD is the
// only root and a Commit draft must be explicitly dismissed first.
func (s *Shell) showViewFiles() {
	if s.holdCommitNavigation() {
		return
	}
	if s.router.Path() == routeFiles {
		s.showViewTerminal()
		return
	}
	s.syncRightPanelRoot()
	s.sidebarCollapsed = false // Files always exposes its tree on entry.
	s.router.Push(routeFiles)
}

// filesPage is the preview center: content only. The title bar names the page
// and the folder it browses and carries the open file's Actions (Copy Path,
// Reveal in Finder — route_header.go); the shared left sidebar renders
// directory navigation and sends selected files to the WebView page through
// the filesPreview bridge; this page never reads file content.
func (s *Shell) filesPage(c *ui.Context) {
	s.syncRightPanelRoot()
	root := s.rightPanel.currentRoot
	selected := s.rightPanel.selectedFilePath
	ui.Column(c).FillWidth().Grow(1).MinWidth(0).MinHeight(0).Children(func() {
		if root == "" {
			ui.Column(c).Grow(1).Center().Children(func() {
				emptyState(c, "No folder", "Open a tab in a project to browse its files.")
			})
			return
		}
		// The native tier renders what the WebView tier cannot take
		// (oversized text heads, oversized images) before the page is
		// considered at all (FILES-09).
		if selected != "" && s.rightPanel.nativePreviewPath == selected {
			s.nativePreviewBody(c, selected)
			return
		}
		if s.filesWebviewBody(c) {
			return
		}
		// Truthful fallbacks: the WebView page is the preview, so the off
		// and broken states say exactly that instead of offering a lesser
		// native preview.
		if !s.useWebviewPreview() {
			ui.Column(c).Grow(1).Center().Children(func() {
				emptyState(c, "Rich preview is off",
					"Turn on “Rich file preview” in Settings → General to preview files here.")
			})
			return
		}
		ui.Column(c).Grow(1).Center().Children(func() {
			emptyState(c, "Preview unavailable",
				"The embedded preview could not start. Copy Path and Reveal in Finder still work.")
		})
	})
}
