package nativeui

/**
 * [INPUT]: 依赖多 Provider/多项目 Catalog 夹具、内存回收站替身（fakeTrash）与官方 MyGo headless Tester
 * [OUTPUT]: 验证 Sessions 多选模式的机制（逐行勾选、分组全选、选择随筛选与回收站切换清除）与「项目折叠默认」
 * [POS]: 批量整理回归；删除/放回的真实文件语义在 history_trash_test.go，这里只验证选择本身
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
	"github.com/wh-studio/herdr-client/internal/sessiontrash"
)

// TestHistorySelectModeTicksRows pins the select flow: the toolbar toggle
// enters the mode, a row click ticks instead of unfolding, the bar counts and
// Done leaves the mode with the selection dropped.
func TestHistorySelectModeTicksRows(t *testing.T) {
	shell := newMultiAgentHistoryShell(t)
	shell.hist.trash = &fakeTrash{}
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	if tester.HasText("0 selected") {
		t.Fatalf("the selection bar must not show outside select mode; texts=%q", tester.Texts())
	}
	if err := tester.Click("Select sessions"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !shell.hist.selecting || !tester.HasText("0 selected") {
		t.Fatalf("Select did not enter the mode; texts=%q", tester.Texts())
	}

	for _, title := range []string{"Tighten sidebar spacing", "Fix the flaky terminal test"} {
		if err := tester.Click(title); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
	}
	if shell.router.Path() != routeHistory || shell.hist.preview.key != "" {
		t.Fatalf("a row click while selecting must tick, not open: path=%q preview=%q", shell.router.Path(), shell.hist.preview.key)
	}
	if !tester.HasText("2 selected") {
		t.Fatalf("selection count missing; texts=%q", tester.Texts())
	}

	if err := tester.Click("Done"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.hist.selecting || len(shell.hist.selected) != 0 {
		t.Fatalf("Done must leave select mode: selecting=%v selected=%v", shell.hist.selecting, shell.hist.selected)
	}
	if tester.HasText("selected") {
		t.Fatalf("the bar outlived select mode; texts=%q", tester.Texts())
	}
}

// TestHistorySelectGroupBox pins the project checkbox: it ticks the whole
// group, a second click clears it, and it never folds the group.
func TestHistorySelectGroupBox(t *testing.T) {
	shell := newMultiAgentHistoryShell(t)
	shell.hist.trash = &fakeTrash{}
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	shell.setHistorySelecting(true)
	tester.Frame()

	if err := tester.Click("Select every session in alpha"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if len(shell.hist.selected) != 4 || !tester.HasText("4 selected") {
		t.Fatalf("group checkbox selected %d; texts=%q", len(shell.hist.selected), tester.Texts())
	}
	if shell.hist.groupOpen["/work/alpha"] {
		t.Fatalf("the group checkbox must not toggle the fold: %v", shell.hist.groupOpen)
	}
	if err := tester.Click("Select every session in alpha"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if len(shell.hist.selected) != 0 {
		t.Fatalf("second click did not clear the group: %v", shell.hist.selected)
	}
}

// TestHistorySelectionDropsWithFilters pins that a selection never outlives
// the view it was made in: a tab or Trash switch clears it.
func TestHistorySelectionDropsWithFilters(t *testing.T) {
	shell := newMultiAgentHistoryShell(t)
	shell.hist.trash = &fakeTrash{items: []sessiontrash.Item{trashItemFixture("1700000000000-aaaaaaaa", "Old one")}}
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	tester.Frame() // the first frame requests the Trash list; this one shows its toggle
	shell.setHistorySelecting(true)
	shell.setHistorySelected([]string{"pi:p1"}, true)

	if err := tester.Click("Codex"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if len(shell.hist.selected) != 0 {
		t.Fatalf("provider switch kept the selection: %v", shell.hist.selected)
	}
	shell.setHistorySelected([]string{"codex:c1"}, true)
	if err := tester.Click("Trash"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if len(shell.hist.selected) != 0 || shell.hist.selecting {
		t.Fatalf("Trash toggle kept the selection: selecting=%v selected=%v", shell.hist.selecting, shell.hist.selected)
	}
}

// TestHistoryLatestProjectOpensAlone pins the fold default over a real
// two-project list: the newest project is open, the other one folded, and a
// search opens every project it matched in.
func TestHistoryLatestProjectOpensAlone(t *testing.T) {
	catalog := history.NewCatalog()
	seed := func(id, title, project string, updated int64) {
		meta := history.SessionMeta{
			Key: "codex:" + id, ID: id, Agent: history.AgentCodex, Title: title,
			ProjectPath: "/work/" + project, ProjectName: project, FilePath: "/nowhere/" + id,
			UpdatedAt: updated, MessageCount: 2,
		}
		catalog.Put(meta, updated, "")
	}
	seed("n1", "Newest in alpha", "alpha", 3000)
	seed("o1", "Oldest in beta", "beta", 1000)
	seed("m1", "Middle in gamma", "gamma", 2000)
	shell := NewShell(WithHistory(history.NewHistoryService(catalog, nil)))
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()

	if !tester.HasText("Newest in alpha") || tester.HasText("Middle in gamma") || tester.HasText("Oldest in beta") {
		t.Fatalf("only the latest project may start open; texts=%q", tester.Texts())
	}
	shell.hist.query = "in"
	shell.applyHistoryFilters()
	tester.Frame()
	tester.Frame()
	for _, want := range []string{"Newest in alpha", "Middle in gamma", "Oldest in beta"} {
		if !tester.HasText(want) {
			t.Fatalf("a search must open every matched project (%q missing); texts=%q", want, tester.Texts())
		}
	}
}
