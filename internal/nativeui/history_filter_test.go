package nativeui

/**
 * [INPUT]: 依赖 History 查询状态机、官方 MyGo headless Tester 与隔离 HistoryService 测试夹具
 * [OUTPUT]: 验证查询计数、无结果重置、Provider 导航稳定及搜索期间列表失效
 * [POS]: UI-006 History 筛选闭环回归，不依赖真实 Agent 或用户终端
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

func TestHistoryResultLabelDescribesBoundedView(t *testing.T) {
	tests := []struct {
		loading, loaded bool
		errText         string
		count           int
		want            string
	}{
		{loading: true, want: "Searching…"},
		{errText: "offline", want: "Unable to load results"},
		{want: "Results not loaded"},
		{loaded: true, count: 0, want: "0 results · Newest first"},
		{loaded: true, count: 1, want: "1 result · Newest first"},
		{loaded: true, count: HistoryListLimit, want: "Latest 100 results · Newest first"},
	}
	for _, tc := range tests {
		got := historyResultLabel(tc.loading, tc.loaded, tc.errText, tc.count)
		if got != tc.want {
			t.Errorf("label = %q, want %q", got, tc.want)
		}
	}
	if !historyFiltersActive(" fix ", "") || !historyFiltersActive("", string(history.AgentCodex)) {
		t.Fatal("search and provider scopes must both register as active")
	}
	if historyFiltersActive("  ", "") {
		t.Fatal("whitespace-only search must not count as an active filter")
	}
}

func TestHistoryNoResultsCanClearFilters(t *testing.T) {
	shell, _, _ := newHistoryTestShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame() // the rail renders before the initial History page load
	if !tester.HasText("Build history") || !tester.HasText("1 result · Newest first") {
		t.Fatalf("initial list missing: %q", tester.Texts())
	}
	shell.hist.query = "session-not-present"
	shell.applyHistoryFilters()
	tester.Frame()
	if !tester.HasText("No matching conversations") || !tester.HasText("0 results · Newest first") {
		t.Fatalf("search result feedback missing: %q", tester.Texts())
	}
	if err := tester.Click("Clear filters"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.hist.query != "" || shell.hist.provider != "" {
		t.Fatalf("clear filters left query=%q provider=%q", shell.hist.query, shell.hist.provider)
	}
	if !tester.HasText("Build history") {
		t.Fatalf("clear filters did not recover the list: %q", tester.Texts())
	}
}

func TestHistoryFiltersOnlyRenderWhereTheyApply(t *testing.T) {
	shell, _, _ := newHistoryTestShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	if !tester.HasText("Search History") {
		t.Fatalf("History list must offer search: %q", tester.Texts())
	}
	shell.router.Replace("/history-projects")
	tester.Frame()
	if tester.HasText("Search History") || tester.HasText("Filter") {
		t.Fatalf("Project grouping must not expose inoperative list filters: %q", tester.Texts())
	}
	shell.router.Replace("/history/codex:22222222-aaaa-bbbb-cccc-000000000002")
	tester.Frame()
	if tester.HasText("Search History") || tester.HasText("Filter") {
		t.Fatalf("Conversation detail must not expose inoperative list filters: %q", tester.Texts())
	}
}

func TestHistorySearchDoesNotEraseProviderNavigationOrRecent(t *testing.T) {
	shell, _, _ := newHistoryTestShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	if len(shell.hist.providers) != 1 || shell.hist.providers[0] != history.AgentCodex {
		t.Fatalf("initial provider options = %+v", shell.hist.providers)
	}
	if len(shell.recentItems) != 1 {
		t.Fatalf("initial recent = %+v", shell.recentItems)
	}
	shell.hist.query = "nothing-here"
	shell.applyHistoryFilters()
	tester.Frame()
	if len(shell.hist.providers) != 1 || shell.hist.providers[0] != history.AgentCodex {
		t.Fatalf("search replaced provider choices with results: %+v", shell.hist.providers)
	}
	if len(shell.recentItems) != 1 || shell.recentItems[0].Title != "Build history" {
		t.Fatalf("search polluted Workspace Recent with filtered rows: %+v", shell.recentItems)
	}
}

func TestHistoryPendingFilterCannotOpenStaleRow(t *testing.T) {
	shell := NewShell()
	fake := newFakeHistoryView()
	shell.hist.service = fake
	shell.hist.sessions = []history.SessionSummary{summaryTitled("codex:old", "Old Session")}
	shell.hist.loaded = true
	shell.hist.listSelected = 0
	shell.hist.query = "new"
	shell.router.Replace(routeHistory)

	done := make(chan struct{})
	go func() {
		defer close(done)
		shell.applyHistoryFilters()
	}()
	call := <-fake.calls
	if call.query.Search != "new" || len(shell.hist.sessions) != 0 || shell.hist.listSelected != -1 {
		t.Fatalf("stale result remains selectable: query=%q sessions=%d selected=%d",
			call.query.Search, len(shell.hist.sessions), shell.hist.listSelected)
	}
	call.release <- listResult{summaries: []history.SessionSummary{summaryTitled("codex:new", "New Session")}}
	<-done
	tester := ui.NewTester(shell.View, 1200, 800)
	if !tester.HasText("New Session") || strings.Contains(strings.Join(tester.Texts(), " "), "Old Session") {
		t.Fatalf("outdated session visible: %q", tester.Texts())
	}
}
