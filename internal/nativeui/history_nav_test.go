package nativeui

/**
 * [INPUT]: 依赖真实夹具会话（trashFixture）、Sessions 页在 headless Tester 下的真键盘输入与焦点
 * [OUTPUT]: Sessions 列表的 listNav 回归：进页开场聚焦且不亮、↓/↑ 跨项目卡头移动、←/→ 折叠项目与展开/收起行内预览、Enter/Space 切换预览、选择模式里行是叶子（Enter/Space 勾选）、确认框收起后键盘回到光标、搜索框里的字母仍是文字
 * [POS]: 与 list_nav_test.go 并列的列表键盘契约；断言落在真焦点（Tester.Focused）与各页自己的状态上，只动临时目录里的夹具会话
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

// navSessions opens the Sessions page over the real-file fixture. Rows once
// settled: the open project alpha (flaky, release, spacing) with beta's
// header folded below it.
func navSessions(t *testing.T) (*trashFixture, *ui.Tester) {
	t.Helper()
	f := newTrashFixture(t)
	tester := f.page()
	settleFrames(tester, 4)
	return f, tester
}

// wakeOnFirstRow lights the cursor on the first session: ↑ climbs to its
// project header, ↓ comes back down.
func wakeOnFirstRow(tester *ui.Tester) {
	pressKey(tester, 0, ui.KeyUp)
	pressKey(tester, 0, ui.KeyDown)
}

// Arriving on Sessions puts the keyboard in the list: no click, no Tab, and
// nothing looks selected until a key has moved.
func TestSessionsListTakesTheKeyboardOnEntry(t *testing.T) {
	_, tester := navSessions(t)
	wantCursor(t, tester, "Fix the flaky terminal test")
	if rowFace(t, tester, "Fix the flaky terminal test") != rowFace(t, tester, "Review the release notes") {
		t.Fatal("a cursor the keyboard has not used must not look selected")
	}
	pressKey(tester, 0, ui.KeyDown)
	wantCursor(t, tester, "Review the release notes")
	if rowFace(t, tester, "Review the release notes") == rowFace(t, tester, "Tighten sidebar spacing") {
		t.Fatal("the first key must wake the cursor")
	}
}

// ↓/↑ walk the rows and the project headers between them, stopping at the
// ends; a folded project is one row, and → opens it.
func TestSessionsArrowsWalkRowsAndHeaders(t *testing.T) {
	_, tester := navSessions(t)
	for _, want := range []string{"Project alpha", "Project alpha"} { // the header is a row: ↑ climbs onto it, and it is not a ring
		pressKey(tester, 0, ui.KeyUp)
		wantCursor(t, tester, want)
	}
	if rowFace(t, tester, "Project alpha") == rowFace(t, tester, "Project beta") {
		t.Fatal("a project header under the cursor must wear the selection face")
	}
	for _, want := range []string{"Fix the flaky terminal test", "Review the release notes", "Tighten sidebar spacing", "Project beta", "Project beta"} {
		pressKey(tester, 0, ui.KeyDown)
		wantCursor(t, tester, want)
	}
	pressKey(tester, 0, ui.KeyRight)
	if !onScreen(tester, "Rename the settings groups") {
		t.Fatalf("→ on a folded project must open it; texts=%q", tester.Texts())
	}
	pressKey(tester, 0, ui.KeyDown)
	wantCursor(t, tester, "Rename the settings groups")
	pressKey(tester, 0, ui.KeyLeft)
	wantCursor(t, tester, "Project beta") // ← on a row climbs to its project
	pressKey(tester, 0, ui.KeyLeft)
	if onScreen(tester, "Rename the settings groups") {
		t.Fatalf("← on an open project must fold it; texts=%q", tester.Texts())
	}
	pressKey(tester, 0, ui.KeyHome)
	wantCursor(t, tester, "Project alpha")
	pressKey(tester, 0, ui.KeyEnd)
	wantCursor(t, tester, "Project beta")
}

// The inline preview is the row's fold: → opens it (and only opens), ←
// closes it, and a second ← climbs to the project; Enter toggles it like a
// click.
func TestSessionsRightAndLeftOpenAndCloseThePreview(t *testing.T) {
	f, tester := navSessions(t)
	wakeOnFirstRow(tester)
	pressKey(tester, 0, ui.KeyRight)
	if f.shell.hist.preview.key != trashKeyFlaky {
		t.Fatalf("→ must open the preview of the cursor row; preview %q", f.shell.hist.preview.key)
	}
	wantCursor(t, tester, "Fix the flaky terminal test")
	pressKey(tester, 0, ui.KeyRight)
	if f.shell.hist.preview.key != trashKeyFlaky {
		t.Fatalf("→ on an open row must not close it; preview %q", f.shell.hist.preview.key)
	}
	pressKey(tester, 0, ui.KeyLeft)
	if f.shell.hist.preview.key != "" {
		t.Fatalf("← on an open row must fold the preview; preview %q", f.shell.hist.preview.key)
	}
	wantCursor(t, tester, "Fix the flaky terminal test")
	pressKey(tester, 0, ui.KeyLeft)
	wantCursor(t, tester, "Project alpha")
	pressKey(tester, 0, ui.KeyDown)
	wantCursor(t, tester, "Fix the flaky terminal test")
	pressKey(tester, 0, ui.KeyEnter)
	if f.shell.hist.preview.key != trashKeyFlaky {
		t.Fatalf("Enter must open the preview; preview %q", f.shell.hist.preview.key)
	}
	pressKey(tester, 0, ui.KeyEnter)
	if f.shell.hist.preview.key != "" {
		t.Fatalf("Enter must toggle it closed; preview %q", f.shell.hist.preview.key)
	}
}

// While selecting a row is a plain leaf: Enter and Space tick it, ← climbs.
func TestSessionsKeysTickRowsWhileSelecting(t *testing.T) {
	f, tester := navSessions(t)
	f.shell.setHistorySelecting(true)
	settleFrames(tester, 2)
	wakeOnFirstRow(tester)
	pressKey(tester, 0, ui.KeyEnter)
	if !f.shell.hist.selected[trashKeyFlaky] || f.shell.hist.preview.key != "" {
		t.Fatalf("Enter must tick the row, not open it: selected=%v preview=%q", f.shell.hist.selected, f.shell.hist.preview.key)
	}
	pressKey(tester, 0, ui.KeyDown)
	pressKey(tester, 0, ui.KeySpace)
	if !f.shell.hist.selected[trashKeyRelease] {
		t.Fatalf("Space must tick the row: selected=%v", f.shell.hist.selected)
	}
	pressKey(tester, 0, ui.KeyRight)
	if f.shell.hist.preview.key != "" || len(f.shell.hist.selected) != 2 {
		t.Fatalf("→ on a leaf must do nothing: selected=%v preview=%q", f.shell.hist.selected, f.shell.hist.preview.key)
	}
	pressKey(tester, 0, ui.KeyLeft)
	wantCursor(t, tester, "Project alpha")
}

// A confirm raised from a row's menu takes the keyboard for its buttons;
// dismissing it hands the keyboard back to the row.
func TestSessionsKeyboardSurvivesAConfirm(t *testing.T) {
	f, tester := navSessions(t)
	f.shell.moveSessionsToTrash([]history.SessionMeta{f.meta(trashKeyFlaky)})
	f.shell.setHistoryTrashView(true)
	settleFrames(tester, 4)
	// A click parks the cursor on the row (and ticks it); a key wakes it.
	if err := tester.Click("Fix the flaky terminal test"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	wakeOnFirstRow(tester)
	wantCursor(t, tester, "Fix the flaky terminal test")

	f.shell.confirmDeleteForever([]string{trashKeyFlaky})
	settleFrames(tester, 3)
	if !f.shell.confirmOpen || f.shell.confirmKind != deleteForeverConfirm {
		t.Fatalf("the Delete Forever confirm did not open: open=%v kind=%q", f.shell.confirmOpen, f.shell.confirmKind)
	}
	if err := tester.Click("Cancel"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	wantCursor(t, tester, "Fix the flaky terminal test") // the dialog hands the keyboard back
	if f.inTrash() != 1 {
		t.Fatal("Cancel must keep the session in the Trash")
	}
}

// Letters typed into the search field above the list are text, not motions.
func TestSessionsSearchKeepsItsLetters(t *testing.T) {
	f, tester := navSessions(t)
	if err := tester.Click("Search sessions"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.TypeKey(0, ui.KeyJ, "j")
	tester.TypeKey(0, ui.KeyD, "d")
	tester.Frame()
	if f.shell.hist.query != "jd" {
		t.Fatalf("search text = %q, want the typed letters", f.shell.hist.query)
	}
	if f.inTrash() != 0 || !pathExists(f.paths[trashKeyFlaky]) {
		t.Fatal("a letter typed into the field acted on a row")
	}
}
