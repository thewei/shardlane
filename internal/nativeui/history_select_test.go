package nativeui

/**
 * [INPUT]: 依赖多 Provider/多项目 Catalog 夹具、官方 MyGo headless Tester 与 Shell 本地隐藏设置
 * [OUTPUT]: 验证 Sessions 多选模式（逐行勾选、分组全选、批量 Hide/Restore）、隐藏列表写入上限与选择随筛选清除
 * [POS]: 批量整理回归；全程只读写本地 HiddenSessions，不依赖真实 Agent 或 Provider 文件
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

func hiddenKeys(shell *Shell) map[string]bool { return shell.hiddenSessionSet() }

// TestHistorySelectModeBatchHide pins the select flow: the toolbar toggle
// enters the mode, a row click ticks instead of unfolding, the bar counts,
// and Hide writes every ticked key to the local list in one go.
func TestHistorySelectModeBatchHide(t *testing.T) {
	shell := newMultiAgentHistoryShell(t)
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

	if err := tester.Click("Hide"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	hidden := hiddenKeys(shell)
	if len(hidden) != 2 || !hidden["pi:p1"] || !hidden["codex:c1"] {
		t.Fatalf("hidden after batch = %v", shell.settings.Session.HiddenSessions)
	}
	if shell.hist.selecting || len(shell.hist.selected) != 0 {
		t.Fatalf("a batch action must leave select mode: selecting=%v selected=%v", shell.hist.selecting, shell.hist.selected)
	}
	if tester.HasText("Tighten sidebar spacing") || tester.HasText("Fix the flaky terminal test") {
		t.Fatalf("hidden rows still listed; texts=%q", tester.Texts())
	}
	if shell.status != "Hid 2 sessions" {
		t.Fatalf("status = %q", shell.status)
	}
}

// TestHistorySelectGroupBoxAndRestore pins the project checkbox (ticks the
// whole group, a second click clears it) and Restore in the Hidden view.
func TestHistorySelectGroupBoxAndRestore(t *testing.T) {
	shell := newMultiAgentHistoryShell(t)
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

	// Hide three through the bar, then restore them from the Hidden view.
	shell.setHistorySelected([]string{"pi:p1", "pi:p2", "codex:c1"}, true)
	tester.Frame()
	if err := tester.Click("Hide"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if len(hiddenKeys(shell)) != 3 {
		t.Fatalf("hidden = %v", shell.settings.Session.HiddenSessions)
	}

	shell.hist.showHidden = true
	shell.setHistorySelecting(true)
	tester.Frame()
	if err := tester.Click("Select all"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if err := tester.Click("Restore"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if len(hiddenKeys(shell)) != 0 {
		t.Fatalf("Restore left %v hidden", shell.settings.Session.HiddenSessions)
	}
	if shell.status != "Restored 3 sessions" {
		t.Fatalf("status = %q", shell.status)
	}
}

// TestHistorySelectionDropsWithFilters pins that a selection never outlives
// the view it was made in: a search, tab or Hidden switch clears it.
func TestHistorySelectionDropsWithFilters(t *testing.T) {
	shell := newMultiAgentHistoryShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	shell.setHistorySelecting(true)
	shell.setHistorySelected([]string{"pi:p1"}, true)

	if err := tester.Click("Codex"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if len(shell.hist.selected) != 0 {
		t.Fatalf("provider switch kept the selection: %v", shell.hist.selected)
	}
	shell.setSessionsHidden([]string{"claude-code:k1"}, true) // the Hidden toggle shows once something is hidden
	tester.Frame()
	shell.setHistorySelected([]string{"codex:c1"}, true)
	if err := tester.Click("Hidden sessions"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if len(shell.hist.selected) != 0 {
		t.Fatalf("Hidden toggle kept the selection: %v", shell.hist.selected)
	}
}

// TestSetSessionsHiddenBoundsAndOrder pins the one write path: duplicates
// collapse, order is kept, restore removes only what was named, and the list
// never grows past its cap.
func TestSetSessionsHiddenBoundsAndOrder(t *testing.T) {
	shell := NewShell()
	shell.setSessionsHidden([]string{"a", "b", "a", "c"}, true)
	if got := fmt.Sprint(shell.settings.Session.HiddenSessions); got != "[a b c]" {
		t.Fatalf("hidden = %s", got)
	}
	shell.setSessionsHidden([]string{"b", "zzz"}, false)
	if got := fmt.Sprint(shell.settings.Session.HiddenSessions); got != "[a c]" {
		t.Fatalf("after restore = %s", got)
	}

	many := make([]string, 0, hiddenSessionsLimit+40)
	for i := 0; i < hiddenSessionsLimit+40; i++ {
		many = append(many, fmt.Sprintf("k%03d", i))
	}
	shell.setSessionsHidden(many, true)
	if got := len(shell.settings.Session.HiddenSessions); got != hiddenSessionsLimit {
		t.Fatalf("hidden length = %d, want the cap %d", got, hiddenSessionsLimit)
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
