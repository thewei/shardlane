package nativeui

/**
 * [INPUT]: 依赖选中 Tab 的仓库根/工作目录（git root，否则 cwd）、HistoryListView 的按项目计数与 sessiontrash 条目
 * [OUTPUT]: Sessions 的范围（This project 默认 / All sessions）：historyScopeRoot/historyProjectRoot、范围切换与随项目变化的重载、按范围的计数与 Trash 条目、侧栏的范围行（historyNav，纵向撑满让标准页脚钉底）
 * [POS]: Sessions 侧栏与列表之间的唯一范围真相；范围只收窄只读目录的视图，不改 Herdr Runtime 或会话文件
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"log/slog"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
	"github.com/wh-studio/herdr-client/internal/sessiontrash"
)

// historyProjectRoot is the folder of the project the sidebar header names:
// the selected Tab's repository, else its working directory. Sessions are
// read from this Mac, so a remote Herdr instance has no project to match.
func (s *Shell) historyProjectRoot() string {
	if s.activeIsRemote() {
		return ""
	}
	if s.git != nil && s.git.root != "" {
		return s.git.root
	}
	return s.selectedTabCWD()
}

// historyScopeRoot is the folder the Sessions list is scoped to: the
// project's, until the scope is switched to All. "" is unscoped.
func (s *Shell) historyScopeRoot() string {
	if s.hist.scopeAll {
		return ""
	}
	return s.historyProjectRoot()
}

// showHistoryScope opens the Sessions list at one scope. The Agent tab
// resets: the two scopes list different Agents.
func (s *Shell) showHistoryScope(all bool) {
	if s.hist.scopeAll != all {
		s.hist.scopeAll = all
		s.hist.provider = ""
		s.applyHistoryFilters()
	}
	s.showHistoryIndex()
}

// syncHistoryScope reloads the list when the project the header names has
// moved under it — a project switch, another Tab's folder, the repository
// resolving — so the list always shows what the sidebar says.
func (s *Shell) syncHistoryScope() {
	if !s.hist.loaded || s.hist.scopeRoot == s.historyScopeRoot() {
		return
	}
	s.hist.provider = ""
	s.applyHistoryFilters()
}

// historyTotals reads the Provider totals the page needs: the whole
// catalog's (the All scope's tabs and count) and the project's (the project
// scope's tabs and count). A source that cannot total returns nil for it.
func historyTotals(ctx context.Context, service HistoryListView, projectRoot string) (all, project []history.ProviderCount) {
	if source, ok := service.(HistoryProviderCountView); ok {
		if totals, err := source.ProviderCounts(ctx); err == nil {
			all = totals
		} else if ctx.Err() == nil {
			slog.Warn("history provider counts unavailable", "error", err)
		}
	}
	if source, ok := service.(HistoryProjectCountView); ok && projectRoot != "" {
		if totals, err := source.ProviderCountsIn(ctx, projectRoot); err == nil {
			project = totals
		} else if ctx.Err() == nil {
			slog.Warn("history project counts unavailable", "error", err)
		}
	}
	return all, project
}

// providerTotal sums every Provider's count.
func providerTotal(totals []history.ProviderCount) int {
	sum := 0
	for _, total := range totals {
		sum += total.Count
	}
	return sum
}

// trashScoped is what the Trash view shows: the entries of the scope's
// project, or all of them under All. An entry whose file could not be read has
// no project folder and shows under All only.
func (s *Shell) trashScoped() []sessiontrash.Item {
	root := s.historyScopeRoot()
	if root == "" {
		return s.hist.trashItems
	}
	scoped := make([]sessiontrash.Item, 0, len(s.hist.trashItems))
	for _, item := range s.hist.trashItems {
		if history.ProjectContains(root, item.Summary.Meta.ProjectPath) {
			scoped = append(scoped, item)
		}
	}
	return scoped
}

// noProjectSessions is the empty state of a project scope that holds nothing
// while the other projects do, with the way out.
func (s *Shell) noProjectSessions(c *ui.Context, title string) {
	ui.Column(c).Grow(1).Center().Gap(Spacing().M).Children(func() {
		ui.Text(c, title).FontWeight(WeightSemibold)
		if ui.Button(c, "Show All Sessions").Clicked() {
			s.showHistoryScope(true)
		}
	})
}

// historyNav is the Sessions sidebar body: the scope of the list — this
// project's sessions (the default) or every project's — under the shared
// project/branch header. A branch switch changes nothing here: sessions
// belong to the folder, and the switch lands on the Git and Terminal views.
// Either row opens the list at its scope, also from a session's detail.
func (s *Shell) historyNav(c *ui.Context) {
	t := c.Theme()
	onIndex := s.router.Path() == routeHistory
	root := s.historyProjectRoot()
	count := func(n int) func() {
		if n == 0 || s.hist.showTrash {
			return nil
		}
		return func() {
			ui.Text(c, itoa(n)).FontSize(Typography().Micro).TextColor(t.TextMuted).SingleLine().Shrink(0)
		}
	}
	// Grow keeps the scope rows at the top while the standard sidebar foot
	// pins to the column's bottom edge (2026-10-15: Sessions wears the same
	// foot as the Workspace sidebar).
	ui.Column(c).FillWidth().Grow(1).MinHeight(0).Padding(2, sidebarGutter).Gap(1).FocusGroup(ui.Vertical).Children(func() {
		if root != "" {
			if _, clicked := s.sidebarTreeRow(c, sidebarTreeSpec{
				Key: "sessions-scope-project", Title: "This project", Tooltip: root, Glyph: iconFolder,
				Selected: onIndex && !s.hist.scopeAll, Trailing: count(s.hist.projectTotal),
			}); clicked {
				s.showHistoryScope(false)
			}
		}
		if _, clicked := s.sidebarTreeRow(c, sidebarTreeSpec{
			Key: "sessions-scope-all", Title: "All sessions", Glyph: iconHistory,
			Selected: onIndex && (s.hist.scopeAll || root == ""), Trailing: count(s.hist.allTotal),
		}); clicked {
			s.showHistoryScope(true)
		}
	})
}
