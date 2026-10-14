package nativeui

/**
 * [INPUT]: selected Herdr Tab CWD、cachedDirEntries/filesGen 缓存围栏、filesview 忽略规则（固定名单 + git 集合）
 * [OUTPUT]: /files 左侧目录树（惰性展开、忽略过滤、"显示忽略项"开关、新建/重命名/删除右键菜单与工具条）及选中文件 → filesPreview 桥事件
 * [POS]: 树只调用 internal/filesview 域服务，渲染零 IO（GWB-003）；预览内容仅经 WebView 桥（2026-10-13 产品决策），此处不再读取文件正文
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"path/filepath"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/filesview"
)

// filesToolView renders the Files tree from background directory snapshots
// only — ListDirect never runs during render (GWB-003). The shared project
// header above it (root path, refresh) lives in the sidebar.
func (s *Shell) filesToolView(c *ui.Context) {
	root := s.rightPanel.currentRoot
	if root == "" {
		emptyState(c, "No Active Directory", "Select a project tab or pane with a valid working directory.")
		return
	}
	s.requestIgnoreSet()
	ui.Column(c).FillWidth().Grow(1).MinHeight(0).Children(func() {
		s.filesTreeToolbar(c)
		ui.Scroll(c).Grow(1).MinHeight(0).Children(func() {
			ui.Column(c).FillWidth().Padding(4, sidebarGutter, 12).Children(func() {
				s.renderDirectoryLevel(c, root, 0)
			})
		})
	})
}

// filesTreeToolbar is the tree's compact action strip: creation at the
// tree root and the ignored-visibility toggle.
func (s *Shell) filesTreeToolbar(c *ui.Context) {
	k := gorexColorsOf(c.Theme().Dark)
	ui.Row(c).FillWidth().Padding(4, sidebarGutter).Gap(4).AlignItems(ui.Center).Children(func() {
		// The ellipsis follows the context-menu twins (HIG: buttons that
		// open a naming dialog advertise it) — F160. The buttons are the
		// shared dense icon button (2026-10-14): its on state is the same
		// neutral chip every toggle wears, not a private accent tint.
		if gorexIconButton(c, k, iconPlus, "New File…", "New File…", false, gorexPaneBtn, gorexPaneGlyph).Clicked() {
			s.filesNewFile(s.rightPanel.currentRoot)
		}
		if gorexIconButton(c, k, iconFolder, "New Folder…", "New Folder…", false, gorexPaneBtn, gorexPaneGlyph).Clicked() {
			s.filesNewFolder(s.rightPanel.currentRoot)
		}
		ui.Spacer(c)
		if gorexIconButton(c, k, iconEyeOff, "Show ignored files", "Show ignored files",
			s.rightPanel.showIgnored, gorexPaneBtn, gorexPaneGlyph).Clicked() {
			s.rightPanel.showIgnored = !s.rightPanel.showIgnored
		}
	})
}

// refreshFiles drops the cached snapshots, the ignore set, and every
// in-flight read (a generation bump invalidates them; clearing loadingDirs
// keeps their abandoned slots from blocking reloads), then rereads the
// root — a new .gitignore must re-arm the git lookup too.
func (s *Shell) refreshFiles() {
	s.rightPanel.filesGen.Add(1)
	s.rightPanel.cachedDirEntries = make(map[string][]filesview.Entry)
	s.rightPanel.loadingDirs = make(map[string]bool)
	s.rightPanel.ignoredRoot = ""
	s.rightPanel.ignoredSet = filesview.NewIgnoreSet(nil)
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

// requestIgnoreSet loads the git-owned ignored paths for the root once.
// The fixed-name set applies immediately; the git set merges in when the
// lookup lands. A non-repository settles with its error and no re-spam.
func (s *Shell) requestIgnoreSet() {
	st := &s.rightPanel
	root := st.currentRoot
	if root == "" || st.ignoredRoot == root {
		return
	}
	st.ignoredRoot = root
	gen := st.filesGen.Load()
	apply := func(paths []string) {
		if st.currentRoot != root || gen != st.filesGen.Load() {
			return
		}
		st.ignoredSet = filesview.NewIgnoreSet(paths)
		// The selection predates the set: an entry picked during the
		// lookup window may now be hidden — drop it and clear the page.
		if sel := st.selectedFilePath; sel != "" && !st.showIgnored {
			if rel, err := filepath.Rel(root, sel); err == nil {
				if st.ignoredSet.Ignored(filepath.Base(sel), rel) {
					st.selectedFilePath = ""
					s.publishFilesPreview()
				}
			}
		}
	}
	if s.win == nil {
		paths, err := filesview.GitIgnorePaths(context.Background(), root)
		if err == nil {
			apply(paths)
		}
		return
	}
	go func() {
		paths, err := filesview.GitIgnorePaths(context.Background(), root)
		s.applyGuarded(s.rightPanel.filesGen.Load, gen, func() {
			if err != nil {
				return // keep the fixed-name set; never toast a probe failure
			}
			apply(paths)
		})
	}()
}

// filesIgnored reports whether the tree hides this entry (render-time only:
// pure lookups, no IO).
func (s *Shell) filesIgnored(e filesview.Entry) bool {
	if s.rightPanel.showIgnored {
		return false
	}
	return s.rightPanel.ignoredSet.Ignored(e.Name, e.RelPath)
}

// renderDirectoryLevel renders cached, filtered entries on the shared tree
// grid; a cache miss schedules the background read and shows a loading
// marker instead of blocking (GWB-003).
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

	switch visible := countVisible(entries, func(e filesview.Entry) bool { return s.filesIgnored(e) }); {
	case len(entries) == 0:
		filesMutedRow(c, "Empty", level)
		return
	case visible == 0:
		filesMutedRow(c, "Hidden by ignore rules", level)
		return
	}

	for _, entry := range entries {
		e := entry
		if s.filesIgnored(e) {
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
			Menu: func(m *ui.Menu) { s.filesRowMenu(m, e) },
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

// countVisible is a tiny filter counter so the empty states can distinguish
// a truly empty directory from one whose entries all hide.
func countVisible(entries []filesview.Entry, hidden func(filesview.Entry) bool) int {
	n := 0
	for _, e := range entries {
		if !hidden(e) {
			n++
		}
	}
	return n
}

// filesMutedRow is the tree's quiet single-line state ("Empty").
func filesMutedRow(c *ui.Context, text string, level int) {
	t := c.Theme()
	ui.Row(c).FillWidth().Padding(2, 0, 2, treeGlyphX(level)).Children(func() {
		ui.Text(c, text).FontSize(Typography().Micro).TextColor(t.TextMuted)
	})
}

// copyToClipboard is shared by the dedicated Files page and other native views.
func (s *Shell) copyToClipboard(text string) { s.copyTextToClipboard(text) }
