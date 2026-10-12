// [INPUT]: Git staged-file row selections.
// [OUTPUT]: Git file click handlers and the shared toggleMenu popover helper.
// [POS]: Changes-list click behavior; the repo menus themselves live in git_sidebar_tree (section "+"/context menus) and sidebar_switchers (project/branch pull-downs).
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
package nativeui

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// changesClickSide opens a tree row from one of the sidebar's split panes:
// files open the center review on that pane's side of the change.
func (s *Shell) changesClickSide(row gitworkbench.FlatRow, view gdViewSide) {
	if row.Node.Dir {
		path := row.Node.Path
		s.git.collapsedDirs[path] = !s.git.collapsedDirs[path]
		return
	}
	s.openChangesSide(row.Node.Path, view)
	s.revealDiffFile(row.Node.Path)
}

// openChangesSide opens the center review for path with the given view side.
func (s *Shell) openChangesSide(path string, view gdViewSide) {
	s.openChanges(path)
	if f, _ := s.gdFileForPath(path); f != nil {
		f.setView(view)
		s.git.gdRowsDirty = true
	}
}

// toggleMenu flips *open when e was clicked since the last frame. The
// click is consumed by the first pass that asks for it, so later passes
// of the same frame cannot flip it twice. Clicks() cannot be used here:
// the engine resets every element's click count at the end of each pass,
// so a count kept across frames would swallow every click after the
// first (the 2026-10-06 staged-files toggle could collapse but never
// re-expand).
func toggleMenu(open *bool, e ui.Element) {
	if e.Clicked() {
		*open = !*open
	}
}
