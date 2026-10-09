package nativeui

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

/**
 * [INPUT]: 依赖 Router、WorkspacePrimarySurface、Settings/History 导航与 MyGo headless Tester
 * [OUTPUT]: 锁定内页返回、历史详情归属、重复点击、顶栏互斥、快捷键 Back/Forward 的统一语义
 * [POS]: UI-009 顶栏/侧栏导航回归，不触碰真实 Herdr 会话
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestHistoryRouteBoundaryDoesNotIncludeSimilarNames(t *testing.T) {
	for _, tc := range []struct {
		path    string
		history bool
	}{
		{routeHistory, true},
		{"/history-projects", true},
		{"/history/abc", true},
		{"/history-old", false},
		{"/historyless", false},
		{"/settings/general", false},
	} {
		if got := isHistoryRoute(tc.path); got != tc.history {
			t.Errorf("%s history=%v, want %v", tc.path, got, tc.history)
		}
		s := NewShell()
		s.router.Replace(tc.path)
		_, _, _, active := s.viewSwitchActive()
		if active != tc.history {
			t.Errorf("%s titlebar history=%v, want %v", tc.path, active, tc.history)
		}
	}
}

func TestHistoryViewNestedFirstReturnsIndexThenTerminal(t *testing.T) {
	for _, nested := range []string{"/history-projects", "/history/session-direct"} {
		t.Run(nested, func(t *testing.T) {
			s := NewShell()
			s.loading = false
			s.router.Replace(nested)
			s.showViewHistory()
			if got := s.router.Path(); got != routeHistory {
				t.Fatalf("nested History click went to %q, want History index", got)
			}
			_, _, _, active := s.viewSwitchActive()
			if !active {
				t.Fatal("History index is no longer marked active")
			}
			s.showViewHistory()
			if got := s.router.Path(); got != routeWorkspace {
				t.Fatalf("repeat History index click went to %q, want Workspace", got)
			}
			if s.surface.current() != WorkspaceSurfaceTerminal {
				t.Fatalf("leaving History index should reach Terminal, got %q", s.surface.current())
			}
		})
	}
}

func TestSidebarBackToTerminalDoesNotRestoreDiff(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.surface.openDiff(s.workspaceContext(), "example.go")
	s.router.Replace("/settings/general")
	tester := ui.NewTester(s.View, 1200, 800)
	if err := tester.Click("Back to Terminal"); err != nil {
		t.Fatal(err)
	}
	if got := s.router.Path(); got != routeWorkspace {
		t.Fatalf("sidebar back went to %q", got)
	}
	if got := s.surface.current(); got != WorkspaceSurfaceTerminal {
		t.Fatalf("Back to Terminal restored %q instead", got)
	}
}

func TestHistoryDeepLinkBackTargetsParentNotPreviousPage(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.router.Replace("/settings/general")
	s.router.Push("/history/session-direct")
	tester := ui.NewTester(s.View, 1200, 800)
	if err := tester.Click("Back to History"); err != nil {
		t.Fatal(err)
	}
	if got := s.router.Path(); got != routeHistory {
		t.Fatalf("History detail Back went to %q, want History index", got)
	}
}

func TestActiveSettingsAndHistorySidebarSelectionDoesNotGrowHistory(t *testing.T) {
	for _, tc := range []struct{ route, label string }{
		{routeHistory, "All conversations"},
		{"/history-projects", "By Project"},
		{"/settings/general", "General"},
	} {
		t.Run(tc.route, func(t *testing.T) {
			s := NewShell()
			s.loading = false
			s.router.Push(tc.route)
			tester := ui.NewTester(s.View, 1200, 800)
			if err := tester.Click(tc.label); err != nil {
				t.Fatal(err)
			}
			if got := s.router.Path(); got != tc.route {
				t.Fatalf("selected nav changed route to %q", got)
			}
			s.router.Back()
			if got := s.router.Path(); got != routeWorkspace {
				t.Fatalf("selected nav added a duplicate route to history: Back=%q", got)
			}
		})
	}
}
