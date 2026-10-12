package nativeui

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/filesview"
)

// filesToolView renders the Files tool from background directory snapshots
// only — ListDirect never runs during render (GWB-003). The sidebar head
// above it (project, branch, refresh) is shared with the Git sidebar.
func (s *Shell) filesToolView(c *ui.Context) {
	root := s.rightPanel.currentRoot

	if root == "" {
		emptyState(c, "No Active Directory", "Select a project tab or pane with a valid working directory.")
		return
	}

	ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
		ui.Column(c).FillWidth().Padding(4, sidebarGutter, 12).Children(func() {
			s.renderDirectoryLevel(c, root, 0)
		})
	})
}

// refreshFiles drops the cached directory snapshots and rereads the root.
func (s *Shell) refreshFiles() {
	s.rightPanel.cachedDirEntries = make(map[string][]filesview.Entry)
	if root := s.rightPanel.currentRoot; root != "" {
		s.requestDirectory(root)
	}
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
	// One generation per root, not per directory: expanding siblings
	// concurrently must not invalidate one another's completed reads.
	s.beginDirectoryLoad(dir, root, s.rightPanel.filesGen.Load())
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

// renderDirectoryLevel renders cached entries on the shared tree grid; a
// cache miss schedules the background read and shows a loading marker
// instead of blocking (GWB-003).
func (s *Shell) renderDirectoryLevel(c *ui.Context, dir string, level int) {
	entries, ok := s.rightPanel.cachedDirEntries[dir]
	if !ok {
		s.requestDirectory(dir)
		if s.rightPanel.loadingDirs[dir] {
			ui.Row(c).FillWidth().Padding(2, 0, 2, treeGlyphX(level)).Children(func() {
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

		glyph, kind := iconFile, "File"
		if isDir {
			glyph, kind = iconFolder, "Folder"
		}
		row := s.sidebarTreeRow(c, sidebarTreeSpec{
			Key: "file:" + e.Path, Title: e.Name, Label: kind + " " + e.Name, Tooltip: e.Path, Level: level,
			Folder: isDir, Expanded: isExpanded, Glyph: glyph,
			Current: !isDir && s.rightPanel.selectedFilePath == e.Path, Dim: e.IsSymlink,
			Trailing: func() {
				// The size right-aligned, as the changes list puts its counts.
				if !isDir && e.SizeBytes > 0 {
					ui.Text(c, filesview.FormatFileSize(e.SizeBytes)).
						Font(gdCodeFont()).FontSize(Typography().Micro).TextColor(t.TextMuted).Shrink(0)
				} else if e.IsSymlink {
					ui.Text(c, "→").Font(gdCodeFont()).FontSize(Typography().BodySmall).TextColor(t.TextMuted).Shrink(0)
				}
			},
		})

		if isDir {
			if row.Clicked() {
				s.rightPanel.expandedDirs[e.Path] = !isExpanded
				if !isExpanded {
					s.requestDirectory(e.Path)
				}
			}
			if isExpanded {
				s.renderDirectoryLevel(c, e.Path, level+1)
			}
		} else {
			if row.Clicked() {
				s.openFilePreview(e.Path)
			}
		}
	}
}

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

// copyToClipboard is shared by the dedicated Files page and other native views.
func (s *Shell) copyToClipboard(text string) { s.copyTextToClipboard(text) }
