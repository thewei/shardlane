package nativeui

/**
 * [INPUT]: selected Herdr Tab working directory, cached directory snapshots and bounded filesview.Preview
 * [OUTPUT]: standalone native Files page with sidebar tree and central read-only file preview
 * [POS]: header Files destination, never creates a right rail, PTY, or filesystem authority
 * [PROTOCOL]: update this header and check CLAUDE.md
 */

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/filesview"
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

// filesPage owns only read-only preview presentation. The shared left
// sidebar renders directory navigation and sends selected files here.
func (s *Shell) filesPage(c *ui.Context) {
	s.syncRightPanelRoot()
	t := c.Theme()
	sp := Spacing()
	tokens := designTokens(t.Dark)
	root := s.rightPanel.currentRoot
	selected := s.rightPanel.selectedFilePath
	preview := s.rightPanel.preview
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
			if selected != "" && preview != nil {
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
		if selected == "" || preview == nil || preview.Path != selected {
			ui.Column(c).Grow(1).Center().Gap(sp.S).Children(func() {
				ui.Icon(c, iconFile).Size(28, 28).TextColor(t.TextMuted)
				ui.Text(c, "Select a file to preview").FontSize(Typography().Section).FontWeight(WeightSemibold)
			})
			return
		}
		ui.Row(c).FillWidth().Padding(sp.S, sp.L).Gap(sp.M).AlignItems(ui.Center).Children(func() {
			ui.Text(c, filepath.Base(selected)).FontSize(Typography().Body).FontWeight(WeightSemibold).
				Grow(1).SingleLine().Tooltip(selected)
			ui.Text(c, filesview.FormatFileSize(preview.SizeBytes)).
				FontSize(Typography().Caption).TextColor(t.TextMuted)
			if preview.Truncated {
				statusPill(c, "Preview limited to 1 MiB", ToneAttention)
			}
		})
		if preview.IsBinary {
			ui.Column(c).Grow(1).Center().Children(func() {
				emptyState(c, "Can't preview this file", "It isn't a text file. Reveal it in Finder to open it.")
			})
			return
		}
		// Don't create thousands of MyGo accessibility text runs for a
		// huge one-line generated asset; the preview remains read-only.
		display := preview.Content
		if len(display) > 120*1024 {
			display = display[:120*1024]
			display = strings.ToValidUTF8(display, "")
			display += "\n\n[Large file: showing the first 120 KiB of the preview]"
		}
		ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
			ui.Column(c).FillWidth().Padding(sp.L).Gap(sp.S).Children(func() {
				ui.Text(c, display).Font(gdCodeFont()).FontSize(Typography().BodySmall).
					TextColor(t.Text)
				if preview.Truncated {
					ui.Text(c, fmt.Sprintf("Showing a limited preview of %s.", filesview.FormatFileSize(preview.SizeBytes))).
						FontSize(Typography().Caption).TextColor(t.TextMuted)
				}
			})
		})
	})
}
