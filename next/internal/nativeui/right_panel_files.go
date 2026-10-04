package nativeui

import (
	"fmt"
	"path/filepath"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/next/internal/filesview"
)

// filesToolView renders the Files tool from background directory snapshots
// only — ListDirect never runs during render (GWB-003).
func (s *Shell) filesToolView(c *ui.Context) {
	sp := Spacing()
	root := s.rightPanel.currentRoot

	if root == "" {
		emptyState(c, "No Active Directory", "Select a project tab or pane with a valid working directory.")
		return
	}

	ui.Column(c).Grow(1).MinHeight(0).Children(func() {
		ui.Row(c).FillWidth().Padding(sp.XS, sp.M).Gap(sp.S).AlignItems(ui.Center).Children(func() {
			rel := filepath.Base(root)
			ui.Text(c, rel).FontSize(Typography().Caption).FontWeight(600).Grow(1).SingleLine()

			if ui.Button(c, "Refresh").FontSize(Typography().Micro).Clicked() {
				s.rightPanel.cachedDirEntries = make(map[string][]filesview.Entry)
				s.requestDirectory(root)
			}
		})

		ui.Scroll(c).Grow(1).Children(func() {
			ui.Column(c).FillWidth().Padding(sp.XS, sp.S).Gap(4).Children(func() {
				s.renderDirectoryLevel(c, root, 0)
			})
		})
	})
}

// requestDirectory schedules a background directory read (GWB-003): the
// snapshot lands through guarded apply; render only ever reads cachedDirEntries.
func (s *Shell) requestDirectory(dir string) {
	if s.rightPanel.loadingDirs == nil {
		s.rightPanel.loadingDirs = make(map[string]bool)
	}
	if s.rightPanel.loadingDirs[dir] {
		return
	}
	s.rightPanel.loadingDirs[dir] = true
	root := s.rightPanel.currentRoot
	if s.win == nil {
		// Headless (tests): resolve synchronously so no real async lane
		// races the single-frame renderer. Production (win != nil) always
		// takes beginDirectoryLoad below — render never blocks on IO there.
		entries, err := filesview.ListDirect(dir, root)
		delete(s.rightPanel.loadingDirs, dir)
		if err == nil && s.rightPanel.currentRoot == root {
			s.rightPanel.cachedDirEntries[dir] = entries
		}
		return
	}
	s.beginDirectoryLoad(dir, root, s.rightPanel.filesGen.Add(1))
}

// beginDirectoryLoad is the production lane: mark in-flight, read in the
// background, apply through the guarded UI update (GWB-003).
func (s *Shell) beginDirectoryLoad(dir, root string, gen uint64) {
	if s.rightPanel.loadingDirs == nil {
		s.rightPanel.loadingDirs = map[string]bool{}
	}
	s.rightPanel.loadingDirs[dir] = true
	go func() {
		entries, err := filesview.ListDirect(dir, root)
		s.applyGuarded(s.rightPanel.filesGen.Load, gen, func() {
			delete(s.rightPanel.loadingDirs, dir)
			// Apply only when root and directory context still match.
			if s.rightPanel.currentRoot != root {
				return
			}
			if err != nil {
				return
			}
			s.rightPanel.cachedDirEntries[dir] = entries
		})
	}()
}

// renderDirectoryLevel renders cached entries; a cache miss schedules the
// background read and shows a loading marker instead of blocking (GWB-003).
func (s *Shell) renderDirectoryLevel(c *ui.Context, dir string, depth int) {
	entries, ok := s.rightPanel.cachedDirEntries[dir]
	if !ok {
		s.requestDirectory(dir)
		if s.rightPanel.loadingDirs[dir] {
			ui.Row(c).FillWidth().Padding(2, sp_x(depth)).Children(func() {
				ui.Spinner(c).Size(10, 10)
			})
		}
		return
	}

	t := c.Theme()

	for _, entry := range entries {
		e := entry
		// .git is plumbing, not user content (F115): every other dotdir
		// stays visible; the object database does not belong in a file tree.
		if e.Name == ".git" && e.IsDir {
			continue
		}
		// Symlinks to directories must not be recursively expanded (P1-05).
		isDir := e.IsDir && !e.IsSymlink
		isExpanded := s.rightPanel.expandedDirs[e.Path]

		// The rows speak the Godiff files-list language (2026-10-06 A3):
		// 28 DIP rows, rotating chevrons, muted folder/file glyphs, the
		// size right-aligned where the changes list puts its counts.
		paddingLeft := float32(6 + depth*TreeIndentWidth)

		var row *ui.Element
		ui.Box(c).Children(func() {
			r := ui.Row(c).Height(28).FillWidth().Padding(0, 8, 0, paddingLeft).Gap(5).
				Radius(6).MinWidth(0).AlignItems(ui.Center)
			if r.Hovered() {
				r.Background(ui.RGBA(127, 127, 127, 0.08))
			}
			r.Children(func() {
				arrow := ui.Box(c).Size(14, 14).Center().Shrink(0)
				if isDir {
					arrow.Children(func() {
						ic := ui.Icon(c, iconChevronDown).FontSize(12).TextColor(t.TextMuted)
						target := float32(0)
						if !isExpanded {
							target = -90
						}
						ic.Rotate(ic.Animate("rot", target, 150*time.Millisecond))
					})
					ui.Icon(c, iconFolder).FontSize(14).TextColor(t.TextMuted)
				} else {
					arrow.Children(func() {
						ui.Icon(c, iconFile).FontSize(14).TextColor(t.TextMuted)
					})
				}

				name := ui.Text(c, e.Name).FontSize(13).Grow(1).SingleLine().Shrink(1).MinWidth(0)
				if e.IsSymlink {
					name.TextColor(t.TextMuted)
				}

				if !isDir && e.SizeBytes > 0 {
					ui.Text(c, filesview.FormatFileSize(e.SizeBytes)).
						Font(gdCodeFont()).FontSize(10).TextColor(t.TextMuted).Shrink(0).
						Tooltip(e.Path)
				} else if e.IsSymlink {
					ui.Text(c, "→").Font(gdCodeFont()).FontSize(11).TextColor(t.TextMuted).Shrink(0)
				}
			})
			row = r
		})

		if isDir {
			if row.Clicked() {
				s.rightPanel.expandedDirs[e.Path] = !isExpanded
				if !isExpanded {
					s.requestDirectory(e.Path)
				}
			}
			if isExpanded {
				s.renderDirectoryLevel(c, e.Path, depth+1)
			}
		} else {
			if row.Clicked() {
				s.openFilePreview(e.Path)
			}
		}
	}
}

func sp_x(depth int) float32 { return float32(depth*TreeIndentWidth) + 4 }

// openFilePreview loads file content up to 1 MiB and presents the preview
// modal. Click-driven IO stays in the interaction handler (not render).
func (s *Shell) openFilePreview(path string) {
	preview, err := filesview.ReadPreview(path, s.rightPanel.currentRoot)
	if err != nil {
		return
	}
	s.rightPanel.preview = &preview
	s.rightPanel.selectedFilePath = path
	s.rightPanel.previewOpen = true
}

// filePreviewModal renders a modal for inspecting the file, copying path,
// and opening in Finder.
func (s *Shell) filePreviewModal(c *ui.Context) {
	preview := s.rightPanel.preview
	if preview == nil {
		return
	}
	t := c.Theme()
	sp := Spacing()

	ui.Modal(c, &s.rightPanel.previewOpen, func() {
		ui.Column(c).Width(640).Height(500).Padding(sp.L).Gap(sp.M).
			Radius(Radius().Card).Background(designTokens(t.Dark).Panel).
			Border(1, designTokens(t.Dark).BorderSubtle).Children(func() {

			ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
				ui.Column(c).Grow(1).Children(func() {
					ui.Text(c, filepath.Base(preview.Path)).FontSize(Typography().Section).FontWeight(650).SingleLine()
					ui.Text(c, preview.Path).FontSize(Typography().Caption).TextColor(t.TextMuted).SingleLine()
				})

				ui.Row(c).Gap(sp.S).Children(func() {
					if ui.Button(c, "Copy Path").Clicked() {
						s.copyToClipboard(preview.Path)
					}
					if ui.Button(c, "Reveal in Finder").Clicked() {
						s.openExternalPathParent(preview.Path)
					}
					if ui.Button(c, "Close").Clicked() {
						s.rightPanel.previewOpen = false
					}
				})
			})

			ui.Scroll(c).Grow(1).Children(func() {
				if preview.IsBinary {
					emptyState(c, "Binary File", fmt.Sprintf("Size: %s. Binary files cannot be previewed in text mode.", filesview.FormatFileSize(preview.SizeBytes)))
				} else {
					ui.Column(c).FillWidth().Padding(sp.S).Background(designTokens(t.Dark).Content).
						Radius(Radius().Control).Children(func() {
						if preview.Truncated {
							ui.Text(c, "⚠ File truncated to 1 MiB").FontSize(Typography().Caption).
								TextColor(designTokens(t.Dark).StatusColor(ToneAttention, t.Dark))
						}
						ui.Text(c, preview.Content).FontSize(Typography().BodySmall)
					})
				}
			})
		})
	})
}

// copyToClipboard is a small helper using platform tools.
func (s *Shell) copyToClipboard(text string) {
	s.copyTextToClipboard(text)
}
