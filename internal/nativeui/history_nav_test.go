package nativeui

/**
 * [INPUT]: 依赖真实夹具会话（trashFixture）、Sessions 页在 headless Tester 下的真键盘输入与焦点
 * [OUTPUT]: Sessions 列表的 listNav 回归：进页开场聚焦且不亮、j/k 跨项目卡头移动、h/l 折叠项目与展开/收起行内预览、选择模式与回收站里行是叶子、d 移入回收站（光标让位邻行、不亮时不动）、回收站里 u 放回/d 永久删除（先确认、确认框收起后键盘回到光标）、搜索框里的字母仍是文字
 * [POS]: 与 list_nav_test.go 并列的列表键盘契约；断言落在真焦点（Tester.Focused）与各页自己的状态上，只动临时目录里的夹具会话
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"os"
	"testing"
	"time"

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

// wakeOnFirstRow lights the cursor on the first session: k climbs to its
// project header, j comes back down.
func wakeOnFirstRow(tester *ui.Tester) { pressText(tester, "kj") }

// Arriving on Sessions puts the keyboard in the list: no click, no Tab, and
// nothing looks selected until a key has moved.
func TestSessionsListTakesTheKeyboardOnEntry(t *testing.T) {
	_, tester := navSessions(t)
	wantCursor(t, tester, "Fix the flaky terminal test")
	if rowFace(t, tester, "Fix the flaky terminal test") != rowFace(t, tester, "Review the release notes") {
		t.Fatal("a cursor the keyboard has not used must not look selected")
	}
	pressText(tester, "j")
	wantCursor(t, tester, "Review the release notes")
	if rowFace(t, tester, "Review the release notes") == rowFace(t, tester, "Tighten sidebar spacing") {
		t.Fatal("the first key must wake the cursor")
	}
}

// j/k walk the rows and the project headers between them, stopping at the
// ends; a folded project is one row, and l opens it.
func TestSessionsJKWalkRowsAndHeaders(t *testing.T) {
	_, tester := navSessions(t)
	for _, want := range []string{"Project alpha", "Project alpha"} { // the header is a row: k climbs onto it, and it is not a ring
		pressText(tester, "k")
		wantCursor(t, tester, want)
	}
	if rowFace(t, tester, "Project alpha") == rowFace(t, tester, "Project beta") {
		t.Fatal("a project header under the cursor must wear the selection face")
	}
	for _, want := range []string{"Fix the flaky terminal test", "Review the release notes", "Tighten sidebar spacing", "Project beta", "Project beta"} {
		pressText(tester, "j")
		wantCursor(t, tester, want)
	}
	pressText(tester, "l")
	if !onScreen(tester, "Rename the settings groups") {
		t.Fatalf("l on a folded project must open it; texts=%q", tester.Texts())
	}
	pressText(tester, "j")
	wantCursor(t, tester, "Rename the settings groups")
	pressText(tester, "h")
	wantCursor(t, tester, "Project beta") // h on a row climbs to its project
	pressText(tester, "h")
	if onScreen(tester, "Rename the settings groups") {
		t.Fatalf("h on an open project must fold it; texts=%q", tester.Texts())
	}
	pressText(tester, "gg")
	wantCursor(t, tester, "Project alpha")
	pressKey(tester, ui.Shift, ui.KeyG)
	wantCursor(t, tester, "Project beta")
}

// The inline preview is the row's fold: l opens it (and only opens), h
// closes it, and a second h climbs to the project; o toggles.
func TestSessionsLAndHOpenAndCloseThePreview(t *testing.T) {
	f, tester := navSessions(t)
	wakeOnFirstRow(tester)
	pressText(tester, "l")
	if f.shell.hist.preview.key != trashKeyFlaky {
		t.Fatalf("l must open the preview of the cursor row; preview %q", f.shell.hist.preview.key)
	}
	wantCursor(t, tester, "Fix the flaky terminal test")
	pressText(tester, "l")
	if f.shell.hist.preview.key != trashKeyFlaky {
		t.Fatalf("l on an open row must not close it; preview %q", f.shell.hist.preview.key)
	}
	pressText(tester, "h")
	if f.shell.hist.preview.key != "" {
		t.Fatalf("h on an open row must fold the preview; preview %q", f.shell.hist.preview.key)
	}
	wantCursor(t, tester, "Fix the flaky terminal test")
	pressText(tester, "h")
	wantCursor(t, tester, "Project alpha")
	pressText(tester, "j")
	wantCursor(t, tester, "Fix the flaky terminal test")
	pressText(tester, "o")
	if f.shell.hist.preview.key != trashKeyFlaky {
		t.Fatalf("o must open the preview; preview %q", f.shell.hist.preview.key)
	}
	pressText(tester, "o")
	if f.shell.hist.preview.key != "" {
		t.Fatalf("o must toggle it closed; preview %q", f.shell.hist.preview.key)
	}
}

// While selecting a row is a plain leaf: o and l tick it, h climbs.
func TestSessionsKeysTickRowsWhileSelecting(t *testing.T) {
	f, tester := navSessions(t)
	f.shell.setHistorySelecting(true)
	settleFrames(tester, 2)
	wakeOnFirstRow(tester)
	pressText(tester, "o")
	if !f.shell.hist.selected[trashKeyFlaky] || f.shell.hist.preview.key != "" {
		t.Fatalf("o must tick the row, not open it: selected=%v preview=%q", f.shell.hist.selected, f.shell.hist.preview.key)
	}
	pressText(tester, "j")
	pressText(tester, "l")
	if !f.shell.hist.selected[trashKeyRelease] {
		t.Fatalf("l must tick the row: selected=%v", f.shell.hist.selected)
	}
	pressText(tester, "h")
	wantCursor(t, tester, "Project alpha")
}

// d is the menu's Move to Trash on the row under a visible cursor — and the
// cursor steps onto the neighbouring row, so it keeps its place.
func TestSessionsDMovesToTrashAndTheCursorStays(t *testing.T) {
	f, tester := navSessions(t)
	pressText(tester, "d") // the parked, unlit cursor acts on nothing
	if !pathExists(f.paths[trashKeyFlaky]) || f.inTrash() != 0 {
		t.Fatal("d acted on a cursor nobody can see")
	}
	wakeOnFirstRow(tester)
	pressText(tester, "d")
	settleFrames(tester, 4)
	if pathExists(f.paths[trashKeyFlaky]) || f.inTrash() != 1 {
		t.Fatalf("d must move the session to the Trash (trash=%d)", f.inTrash())
	}
	if onScreen(tester, "Fix the flaky terminal test") {
		t.Fatalf("the trashed row is still listed; texts=%q", tester.Texts())
	}
	wantCursor(t, tester, "Review the release notes")
	pressText(tester, "d")
	settleFrames(tester, 4)
	wantCursor(t, tester, "Tighten sidebar spacing")
	pressText(tester, "d") // the project's last row
	settleFrames(tester, 4)
	if f.inTrash() != 3 {
		t.Fatalf("three d presses must trash three sessions, trash=%d", f.inTrash())
	}
}

// When the row that leaves was the project's last, the cursor steps up to
// the row above it, never onto the next project's header.
func TestSessionsDOnTheLastRowStepsUp(t *testing.T) {
	_, tester := navSessions(t)
	wakeOnFirstRow(tester)
	pressText(tester, "jj")
	wantCursor(t, tester, "Tighten sidebar spacing")
	pressText(tester, "d")
	settleFrames(tester, 4)
	wantCursor(t, tester, "Review the release notes")
}

// A session an Agent is working in is not moved by d either, and the toast
// says why.
func TestSessionsDRefusesASessionInUse(t *testing.T) {
	f, tester := navSessions(t)
	if err := os.Chtimes(f.paths[trashKeyFlaky], time.Now(), time.Now()); err != nil {
		t.Fatal(err)
	}
	wakeOnFirstRow(tester)
	pressText(tester, "d")
	settleFrames(tester, 3)
	if !pathExists(f.paths[trashKeyFlaky]) || f.inTrash() != 0 {
		t.Fatal("d moved a session that was just written")
	}
	if !tester.HasText("Still in use by an Agent — try again when it's idle.") {
		t.Fatalf("no refusal; texts=%q", tester.Texts())
	}
}

// In the Trash u puts the cursor row back and d asks before deleting it for
// good; the confirm hands the keyboard back, and Ctrl-D stays a half-page.
func TestSessionsTrashKeysPutBackAndDeleteForever(t *testing.T) {
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

	pressKey(tester, ui.Ctrl, ui.KeyD)
	if f.shell.confirmOpen {
		t.Fatal("Ctrl-D is a half-page jump, not a delete")
	}
	wantCursor(t, tester, "Fix the flaky terminal test") // the last row: nowhere further down
	pressKey(tester, ui.Ctrl, ui.KeyU)
	wantCursor(t, tester, "Project alpha")
	pressText(tester, "j")

	pressText(tester, "d")
	if !f.shell.confirmOpen || f.shell.confirmKind != deleteForeverConfirm || f.shell.confirmTarget != f.trashID(trashKeyFlaky) {
		t.Fatalf("d must ask before deleting for good: open=%v kind=%q target=%q", f.shell.confirmOpen, f.shell.confirmKind, f.shell.confirmTarget)
	}
	if f.inTrash() != 1 {
		t.Fatal("d must only ask")
	}
	if err := tester.Click("Cancel"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	wantCursor(t, tester, "Fix the flaky terminal test") // the dialog hands the keyboard back

	pressText(tester, "u")
	settleFrames(tester, 4)
	if !pathExists(f.paths[trashKeyFlaky]) || f.inTrash() != 0 {
		t.Fatalf("u must put the session back (trash=%d)", f.inTrash())
	}
	if onScreen(tester, "Fix the flaky terminal test") {
		t.Fatalf("the restored session is still listed in the Trash; texts=%q", tester.Texts())
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
