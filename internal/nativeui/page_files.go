package nativeui

/**
 * [INPUT]: selected Herdr Tab working directory、rightPanel 预览分级状态（selectedFilePath/previewPlanned/nativePreviewPath）、General.WebViewFilesPreview 与 filesWebview
 * [OUTPUT]: /files 独立页面：中央 = 查看条 + 分级预览——文本/代码走原生 editor，Markdown 渲染与网页图片走 filesPreview WebView（2026-10-14 产品决策），真实空态与失败态
 * [POS]: 左树保持原生；页面标题、根路径与文件动作（Copy Path / Reveal in Finder）归标题栏 Actions（route_header.go），页面只画内容；WebView 页内容只经只读桥，创建失败时给出真实空态
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

// goFiles is the destination form of showViewFiles (see goChanges): naming
// Files while it is open keeps it instead of leaving for the Terminal.
func (s *Shell) goFiles() {
	if s.router.Path() == routeFiles {
		return
	}
	s.showViewFiles()
}

// filesPage is the preview center: the open file's bar over its tier. The
// title bar names the page and the folder it browses and carries the open
// file's Actions (Copy Path, Reveal in Finder — route_header.go); the
// shared left sidebar renders directory navigation and picks the file; the
// plan (files_native_preview.go) decides whether the native code view, the
// native bitmap or the WebView page shows it. This page never edits.
func (s *Shell) filesPage(c *ui.Context) {
	s.syncRightPanelRoot()
	st := &s.rightPanel
	root := st.currentRoot
	selected := st.selectedFilePath
	ui.Column(c).FillWidth().Grow(1).MinWidth(0).MinHeight(0).Children(func() {
		switch {
		case root == "":
			emptyState(c, "No folder", "Open a tab in a project to browse its files.")
		case selected == "":
			emptyState(c, "Select a file to preview", "")
		default:
			s.filesPreviewBar(c, selected)
			s.filesPreviewBody(c, selected)
		}
	})
}

// filesPreviewBody is the tier the plan chose for the open file.
func (s *Shell) filesPreviewBody(c *ui.Context, selected string) {
	st := &s.rightPanel
	switch {
	case st.previewPlanned != selected:
		// The size probe is still landing.
		ui.Column(c).Grow(1).Center().Children(func() {
			ui.Spinner(c).Size(16, 16)
		})
	case st.nativePreviewPath == selected:
		s.nativePreviewBody(c)
	default:
		if !s.filesWebviewBody(c) {
			// The page could not start: say so instead of a blank center.
			emptyState(c, "Preview unavailable",
				"The embedded preview could not start. Copy Path and Reveal in Finder still work.")
		}
	}
}
