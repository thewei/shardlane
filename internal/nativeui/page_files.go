package nativeui

/**
 * [INPUT]: selected Herdr Tab working directory、rightPanel.selectedFilePath、General.WebViewFilesPreview 与 filesWebview
 * [OUTPUT]: /files 独立页面：中央预览由 filesPreview WebView 接管（2026-10-13 产品决策），偏好关闭或创建失败时给出真实空态
 * [POS]: 左树与头部（Copy Path / Reveal in Finder）保持原生；原生预览渲染器已按 2026-10-13 产品决策退役，页面内容只经只读桥，偏好关闭或创建失败时给出真实空态
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

// filesPage owns the preview center's chrome. The shared left sidebar
// renders directory navigation and sends selected files to the WebView
// page through the filesPreview bridge; this page never reads file content.
func (s *Shell) filesPage(c *ui.Context) {
	s.syncRightPanelRoot()
	t := c.Theme()
	sp := Spacing()
	tokens := designTokens(t.Dark)
	root := s.rightPanel.currentRoot
	selected := s.rightPanel.selectedFilePath
	ui.Column(c).FillWidth().Grow(1).MinWidth(0).MinHeight(0).Children(func() {
		ui.Row(c).FillWidth().Padding(sp.M, sp.L).Gap(sp.S).AlignItems(ui.Center).
			BorderWidth(0, 0, 1, 0).BorderColor(tokens.BorderSubtle).Children(func() {
			ui.Icon(c, iconFolder).Size(16, 16).TextColor(t.TextMuted)
			ui.Column(c).Grow(1).MinWidth(0).Gap(sp.XXS).Children(func() {
				ui.Text(c, "Files").FontSize(Typography().Section).FontWeight(WeightSemibold)
				if root != "" {
					ui.Text(c, root).FontSize(Typography().Caption).
						TextColor(t.TextMuted).SingleLine().Tooltip(root)
				}
			})
			if selected != "" {
				if ui.Button(c, "Copy Path").Clicked() {
					s.copyToClipboard(selected)
				}
				if ui.Button(c, "Reveal in Finder").Clicked() {
					s.openExternalPathParent(selected)
				}
			}
		})
		if root == "" {
			ui.Column(c).Grow(1).Center().Children(func() {
				emptyState(c, "No folder", "Open a tab in a project to browse its files.")
			})
			return
		}
		if s.filesWebviewBody(c) {
			return
		}
		// Truthful fallbacks: the WebView page is the preview (the native
		// renderer retired with the 2026-10-13 decision), so the off and
		// broken states say exactly that instead of a lesser preview.
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
