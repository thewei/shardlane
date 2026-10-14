package nativeui

/**
 * [INPUT]: 依赖真实临时 Provider 会话文件、真实 HistoryService / sessiontrash.Store 与官方 MyGo headless Tester
 * [OUTPUT]: 验证 Sessions 删除 = 移入回收站（行菜单/批量/详情页/预览）与撤销、回收站视图（列表/筛选/放回/永久删除与清空的确认）、运行中 Agent 会话与刚写入会话的保护
 * [POS]: 回收站回归；全程只动临时目录里的夹具会话，绝不触碰用户真实 Agent 会话
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
	"github.com/wh-studio/herdr-client/internal/sessiontrash"
)

const (
	trashKeyRelease  = "claude-code:k1"
	trashKeySpacing  = "claude-code:k2"
	trashKeySettings = "claude-code:k3"
	trashKeyFlaky    = "codex:33333333-aaaa-bbbb-cccc-000000000003"
)

// trashFixture is a Shell over real provider files: a fake home holding
// Claude Code and Codex sessions (an hour old, outside the busy window),
// scanned into a real catalog, and a Trash in a temp directory. Nothing here
// can reach a user's own sessions.
type trashFixture struct {
	t       *testing.T
	home    string
	service *history.HistoryService
	store   *sessiontrash.Store
	shell   *Shell
	paths   map[string]string // session key → its provider file
}

func newTrashFixture(t *testing.T) *trashFixture {
	t.Helper()
	t.Setenv("CODEX_HOME", "")
	home := t.TempDir()
	roots := history.DefaultRoots(home)
	f := &trashFixture{t: t, home: home, paths: map[string]string{}}
	// alpha holds the newest sessions, so it is the project that starts open.
	// Listed newest first, that is: flaky, k1, k2 in alpha, then beta's k3.
	f.claude("-work-alpha", "k1", "/work/alpha", "Review the release notes", "2026-08-01T03:00:00Z", time.Hour)
	f.claude("-work-alpha", "k2", "/work/alpha", "Tighten sidebar spacing", "2026-08-01T02:00:00Z", 2*time.Hour)
	f.claude("-work-beta", "k3", "/work/beta", "Rename the settings groups", "2026-07-01T01:00:00Z", 5*time.Hour)
	f.codex("33333333-aaaa-bbbb-cccc-000000000003", "/work/alpha", "Fix the flaky terminal test", "2026-08-02T09:15:00Z", 90*time.Minute)
	f.service = history.NewHistoryService(history.NewCatalog(), roots)
	if _, err := f.service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	f.store = sessiontrash.New(filepath.Join(t.TempDir(), "trash"), roots)
	f.shell = NewShell(WithHistory(f.service), WithSessionTrash(f.store))
	return f
}

// write puts one provider file in place, last written `age` ago — always
// well outside the Trash's busy window, unless a test touches it again.
func (f *trashFixture) write(key, path, body string, age time.Duration) {
	f.t.Helper()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		f.t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
		f.t.Fatal(err)
	}
	old := time.Now().Add(-age)
	if err := os.Chtimes(path, old, old); err != nil {
		f.t.Fatal(err)
	}
	f.paths[key] = path
}

func (f *trashFixture) claude(project, id, cwd, title, stamp string, age time.Duration) {
	body := fmt.Sprintf(`{"type":"user","cwd":%q,"timestamp":%q,"message":{"content":%q}}
{"type":"assistant","cwd":%q,"timestamp":%q,"message":{"id":"m1","content":[{"type":"text","text":"Done."}]}}
`, cwd, stamp, title, cwd, stamp)
	f.write("claude-code:"+id, filepath.Join(f.home, ".claude", "projects", project, id+".jsonl"), body, age)
}

func (f *trashFixture) codex(id, cwd, title, stamp string, age time.Duration) {
	body := fmt.Sprintf(`{"timestamp":%q,"type":"session_meta","payload":{"cwd":%q,"originator":"codex_cli_rs"}}
{"timestamp":%q,"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":%q}]}}
{"timestamp":%q,"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done."}]}}
`, stamp, cwd, stamp, title, stamp)
	f.write("codex:"+id, filepath.Join(f.home, ".codex", "sessions", "2026", "08", "02", "rollout-2026-08-02T09-15-00-"+id+".jsonl"), body, age)
}

// page opens the Sessions page and lets the list and the Trash load.
func (f *trashFixture) page() *ui.Tester {
	f.shell.router.Replace(routeHistory)
	tester := ui.NewTester(f.shell.View, 1200, 800)
	tester.Frame()
	tester.Frame()
	return tester
}

// listed is the catalog as the page reads it, by session key.
func (f *trashFixture) listed() map[string]history.SessionMeta {
	f.t.Helper()
	rows, err := f.service.Recent(context.Background(), HistoryListLimit)
	if err != nil {
		f.t.Fatal(err)
	}
	out := make(map[string]history.SessionMeta, len(rows))
	for _, row := range rows {
		out[row.Meta.Key] = row.Meta
	}
	return out
}

func (f *trashFixture) meta(key string) history.SessionMeta {
	f.t.Helper()
	meta, ok := f.listed()[key]
	if !ok {
		f.t.Fatalf("session %q is not in the catalog", key)
	}
	return meta
}

func pathExists(path string) bool {
	_, err := os.Lstat(path)
	return err == nil
}

func (f *trashFixture) inTrash() int { return f.store.Count() }

// trashID is the Trash entry that holds the file the session came from.
func (f *trashFixture) trashID(key string) string {
	f.t.Helper()
	items, err := f.store.List()
	if err != nil {
		f.t.Fatal(err)
	}
	for _, item := range items {
		if item.Original == f.paths[key] {
			return item.ID
		}
	}
	f.t.Fatalf("no Trash entry for %q", key)
	return ""
}

// TestRowMenuMovesASessionToTrashAndOffersUndo pins the delete itself: the
// row's menu says Move to Trash (never Hide), the file leaves the provider
// folder for the Trash, the row and its catalog entry are gone at once, and
// the toast carries Undo.
func TestRowMenuMovesASessionToTrashAndOffersUndo(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()

	if err := tester.RightClick("Review the release notes"); err != nil {
		t.Fatal(err)
	}
	menu := tester.Menu()
	if !containsText(menu, "Move to Trash") || containsText(menu, "Hide") || containsText(menu, "Restore") {
		t.Fatalf("row menu = %q", menu)
	}
	if err := tester.ChooseMenuItem("Move to Trash"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()

	if pathExists(f.paths[trashKeyRelease]) {
		t.Fatal("the session file is still in the provider folder")
	}
	if f.inTrash() != 1 || len(f.shell.hist.trashItems) != 1 {
		t.Fatalf("Trash holds %d (list shows %d), want 1", f.inTrash(), len(f.shell.hist.trashItems))
	}
	if _, ok := f.listed()[trashKeyRelease]; ok {
		t.Fatal("the catalog still lists the trashed session")
	}
	if tester.HasText("Review the release notes") {
		t.Fatalf("the trashed row is still listed; texts=%q", tester.Texts())
	}
	for _, kept := range []string{"Tighten sidebar spacing", "Fix the flaky terminal test"} {
		if !tester.HasText(kept) {
			t.Fatalf("%q left the list with its sibling; texts=%q", kept, tester.Texts())
		}
	}
	if !tester.HasText("Moved to Trash") || !tester.HasText("Undo") {
		t.Fatalf("no Undo toast; texts=%q", tester.Texts())
	}
	if _, ok := tester.Find("Trash"); !ok || !tester.HasText("1") {
		t.Fatalf("the Trash toggle with its count is missing; texts=%q", tester.Texts())
	}
}

// TestUndoPutsTheSessionBack pins the way back: Undo returns the file to its
// exact place and the row to the list, and the Trash toggle goes away again
// with nothing left in it.
func TestUndoPutsTheSessionBack(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()
	if err := tester.RightClick("Review the release notes"); err != nil {
		t.Fatal(err)
	}
	if err := tester.ChooseMenuItem("Move to Trash"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()

	if err := tester.Click("Undo"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()

	if !pathExists(f.paths[trashKeyRelease]) || f.inTrash() != 0 {
		t.Fatalf("Undo left the file in the Trash (back=%v, trash=%d)", pathExists(f.paths[trashKeyRelease]), f.inTrash())
	}
	if !tester.HasText("Review the release notes") {
		t.Fatalf("the restored session is not back in the list; texts=%q", tester.Texts())
	}
	if _, ok := tester.Find("Trash"); ok {
		t.Fatalf("the Trash toggle stays with nothing in it; texts=%q", tester.Texts())
	}
}

// TestTrashViewListsReadBackRowsAndPutsThemBack pins the Trash view: rows
// are read back from the trashed files (titles, projects, Agents), a click
// ticks instead of opening, the bar's Put Back returns the ticked entries.
func TestTrashViewListsReadBackRowsAndPutsThemBack(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()
	f.shell.moveSessionsToTrash([]history.SessionMeta{f.meta(trashKeyRelease), f.meta(trashKeyFlaky)})
	tester.Frame()

	if err := tester.Click("Trash"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()
	if !f.shell.hist.showTrash {
		t.Fatal("the toggle did not open the Trash")
	}
	for _, want := range []string{"Review the release notes", "Fix the flaky terminal test", "2 in Trash", "All", "Claude Code", "Codex"} {
		if !tester.HasText(want) {
			t.Fatalf("Trash view missing %q; texts=%q", want, tester.Texts())
		}
	}
	if tester.HasText("Tighten sidebar spacing") {
		t.Fatalf("a live session leaked into the Trash view; texts=%q", tester.Texts())
	}

	if tester.HasText("0 selected") || tester.HasText("Put Back") {
		t.Fatalf("the batch bar must wait for a tick in the Trash; texts=%q", tester.Texts())
	}
	if err := tester.Click("Review the release notes"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if f.shell.router.Path() != routeHistory || f.shell.hist.preview.key != "" {
		t.Fatalf("a Trash row click must tick, not open: path=%q preview=%q", f.shell.router.Path(), f.shell.hist.preview.key)
	}
	if !tester.HasText("1 selected") {
		t.Fatalf("selection bar missing; texts=%q", tester.Texts())
	}
	if err := tester.Click("Put Back"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()

	if !pathExists(f.paths[trashKeyRelease]) || pathExists(f.paths[trashKeyFlaky]) {
		t.Fatal("Put Back must restore exactly the ticked entry")
	}
	if f.inTrash() != 1 || !tester.HasText("1 in Trash") {
		t.Fatalf("Trash after Put Back: %d; texts=%q", f.inTrash(), tester.Texts())
	}
	if tester.HasText("Review the release notes") {
		t.Fatalf("the restored entry is still listed in the Trash; texts=%q", tester.Texts())
	}
}

// TestTrashViewOpensEveryProject pins that the Trash shows all it holds:
// unlike the session list, where only the latest project starts open, no
// project hides its entries behind a fold (here: two projects in the Trash).
func TestTrashViewOpensEveryProject(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()
	f.shell.moveSessionsToTrash([]history.SessionMeta{f.meta(trashKeyFlaky), f.meta(trashKeySettings)})
	f.shell.setHistoryTrashView(true)
	settleFrames(tester, 4)
	for _, want := range []string{"Fix the flaky terminal test", "Rename the settings groups"} {
		if !tester.HasText(want) {
			t.Fatalf("the Trash hid %q behind a fold; texts=%q", want, tester.Texts())
		}
	}
}

// TestTrashViewFiltersByAgentAndSearch pins that the tabs and the search
// field work over the Trash too, with counts of what the Trash holds.
func TestTrashViewFiltersByAgentAndSearch(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()
	f.shell.moveSessionsToTrash([]history.SessionMeta{f.meta(trashKeyRelease), f.meta(trashKeySettings), f.meta(trashKeyFlaky)})
	f.shell.setHistoryTrashView(true)
	tester.Frame()
	tester.Frame()

	if got, want := f.shell.historyTabCount(history.AgentClaudeCode), 2; got != want {
		t.Fatalf("Claude Code Trash count = %d, want %d", got, want)
	}
	if got := f.shell.historyTabCount(history.AgentCodex); got != 1 {
		t.Fatalf("Codex Trash count = %d, want 1", got)
	}
	if err := tester.Click("Codex"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("Fix the flaky terminal test") || tester.HasText("Review the release notes") {
		t.Fatalf("the Codex tab is not scoped; texts=%q", tester.Texts())
	}
	if err := tester.Click("All"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()

	f.shell.hist.query = "settings"
	f.shell.applyHistoryFilters()
	tester.Frame()
	if !tester.HasText("Rename the settings groups") || tester.HasText("Review the release notes") {
		t.Fatalf("search did not narrow the Trash; texts=%q", tester.Texts())
	}
	f.shell.hist.query = "zzz-nothing"
	f.shell.applyHistoryFilters()
	tester.Frame()
	if !tester.HasText("No matching sessions") {
		t.Fatalf("an empty filtered Trash must say so; texts=%q", tester.Texts())
	}

	// Leaving the Trash resets the tab, so the catalog never opens scoped to
	// a Provider only the Trash had.
	f.shell.hist.query = ""
	f.shell.hist.provider = string(history.AgentCodex)
	f.shell.setHistoryTrashView(false)
	if f.shell.hist.provider != "" || f.shell.hist.showTrash {
		t.Fatalf("leaving the Trash kept provider=%q showTrash=%v", f.shell.hist.provider, f.shell.hist.showTrash)
	}
}

// TestDeleteForeverAsksFirstAndRemovesTheFile pins permanent deletion: the
// row menu's Delete Forever only opens a confirm; Cancel changes nothing,
// Confirm removes the file from the Mac for good.
func TestDeleteForeverAsksFirstAndRemovesTheFile(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()
	f.shell.moveSessionsToTrash([]history.SessionMeta{f.meta(trashKeySpacing)})
	f.shell.setHistoryTrashView(true)
	tester.Frame()
	tester.Frame()

	if err := tester.RightClick("Tighten sidebar spacing"); err != nil {
		t.Fatal(err)
	}
	if menu := tester.Menu(); !containsText(menu, "Put Back") || !containsText(menu, "Delete Forever…") {
		t.Fatalf("Trash row menu = %q", menu)
	}
	if err := tester.ChooseMenuItem("Delete Forever…"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !f.shell.confirmOpen || f.inTrash() != 1 {
		t.Fatalf("Delete Forever must wait for the confirm (open=%v, trash=%d)", f.shell.confirmOpen, f.inTrash())
	}
	if err := tester.Click("Cancel"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if f.inTrash() != 1 || f.shell.confirmOpen {
		t.Fatalf("Cancel must leave the Trash alone (open=%v, trash=%d)", f.shell.confirmOpen, f.inTrash())
	}

	if err := tester.RightClick("Tighten sidebar spacing"); err != nil {
		t.Fatal(err)
	}
	if err := tester.ChooseMenuItem("Delete Forever…"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if err := tester.Click("Confirm"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()
	if f.inTrash() != 0 || pathExists(f.paths[trashKeySpacing]) {
		t.Fatalf("Confirm left the session behind (trash=%d)", f.inTrash())
	}
	if !tester.HasText("Trash is empty") {
		t.Fatalf("an emptied Trash must say so; texts=%q", tester.Texts())
	}
}

// TestEmptyTrashConfirmsThenClears pins the toolbar's Empty Trash.
func TestEmptyTrashConfirmsThenClears(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()
	f.shell.moveSessionsToTrash([]history.SessionMeta{f.meta(trashKeyRelease), f.meta(trashKeySpacing)})
	f.shell.setHistoryTrashView(true)
	tester.Frame()
	tester.Frame()

	if err := tester.Click("Empty Trash"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !f.shell.confirmOpen || f.inTrash() != 2 {
		t.Fatalf("Empty Trash must ask first (open=%v, trash=%d)", f.shell.confirmOpen, f.inTrash())
	}
	if err := tester.Click("Confirm"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()
	if f.inTrash() != 0 || len(f.shell.hist.trashItems) != 0 {
		t.Fatalf("Empty Trash left %d entries", f.inTrash())
	}
	if _, ok := tester.Find("Empty Trash"); ok {
		t.Fatalf("Empty Trash stays with nothing to empty; texts=%q", tester.Texts())
	}
}

// TestBatchMoveToTrashFromTheSelectionBar pins the select flow's batch
// delete: ticked rows move together and select mode ends.
func TestBatchMoveToTrashFromTheSelectionBar(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()
	if err := tester.Click("Select sessions"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	for _, title := range []string{"Tighten sidebar spacing", "Fix the flaky terminal test"} {
		if err := tester.Click(title); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
	}
	if !tester.HasText("2 selected") {
		t.Fatalf("selection bar missing; texts=%q", tester.Texts())
	}
	if err := tester.Click("Move to Trash"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()

	if pathExists(f.paths[trashKeySpacing]) || pathExists(f.paths[trashKeyFlaky]) || !pathExists(f.paths[trashKeyRelease]) {
		t.Fatal("the batch moved the wrong sessions")
	}
	if f.shell.hist.selecting || len(f.shell.hist.selected) != 0 {
		t.Fatalf("a batch delete must end select mode: selecting=%v selected=%v", f.shell.hist.selecting, f.shell.hist.selected)
	}
	if !tester.HasText("Moved 2 sessions to Trash") {
		t.Fatalf("batch toast missing; texts=%q", tester.Texts())
	}
}

// TestSessionJustWrittenStaysWhereItIs pins the live-session guard at the
// UI: a file touched a moment ago is refused, the row stays, and the toast
// says why instead of offering an Undo for nothing.
func TestSessionJustWrittenStaysWhereItIs(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()
	now := time.Now()
	if err := os.Chtimes(f.paths[trashKeyRelease], now, now); err != nil {
		t.Fatal(err)
	}

	f.shell.moveSessionsToTrash([]history.SessionMeta{f.meta(trashKeyRelease)})
	tester.Frame()

	if !pathExists(f.paths[trashKeyRelease]) || f.inTrash() != 0 {
		t.Fatal("a just-written session was moved")
	}
	if !tester.HasText("Still in use by an Agent — try again when it's idle.") || tester.HasText("Undo") {
		t.Fatalf("refusal toast wrong; texts=%q", tester.Texts())
	}
	if !tester.HasText("Review the release notes") {
		t.Fatalf("a refused session left the list; texts=%q", tester.Texts())
	}
}

// TestSessionAnAgentIsWorkingInIsNeverMoved pins the other guard: a running
// Agent whose measured session identity names the session — by file or by
// native id — keeps it in place however old the file is.
func TestSessionAnAgentIsWorkingInIsNeverMoved(t *testing.T) {
	f := newTrashFixture(t)
	f.page()
	byID := &herdr.AgentSessionIdentity{Agent: "claude-code", Kind: "id", Source: "hook", Value: "k1"}
	byPath := &herdr.AgentSessionIdentity{Agent: "claude-code", Kind: "path", Source: "hook", Value: f.paths[trashKeySpacing]}
	other := &herdr.AgentSessionIdentity{Agent: "claude-code", Kind: "id", Source: "hook", Value: "someone-else"}
	wrongProvider := &herdr.AgentSessionIdentity{Agent: "codex", Kind: "id", Source: "hook", Value: "k3"}
	f.shell.projection.Agents = []herdr.Agent{
		{AgentSession: byID}, {AgentSession: byPath}, {AgentSession: other}, {AgentSession: wrongProvider}, {},
	}

	if !f.shell.sessionInUse(f.meta(trashKeyRelease)) || !f.shell.sessionInUse(f.meta(trashKeySpacing)) {
		t.Fatal("a session a live Agent names must count as in use")
	}
	if f.shell.sessionInUse(f.meta(trashKeySettings)) || f.shell.sessionInUse(f.meta(trashKeyFlaky)) {
		t.Fatal("an unrelated session (or another provider's id) must not count as in use")
	}

	f.shell.moveSessionsToTrash([]history.SessionMeta{f.meta(trashKeyRelease), f.meta(trashKeySettings)})
	if !pathExists(f.paths[trashKeyRelease]) || pathExists(f.paths[trashKeySettings]) {
		t.Fatal("only the session no Agent is working in may move")
	}
	if f.shell.pendingToastAction.message != "Moved to Trash · 1 still in use" {
		t.Fatalf("toast = %q", f.shell.pendingToastAction.message)
	}
}

// TestPutBackNeverOverwritesAndSaysSo pins the conflict path at the UI.
func TestPutBackNeverOverwritesAndSaysSo(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()
	f.shell.moveSessionsToTrash([]history.SessionMeta{f.meta(trashKeyRelease)})
	tester.Frame()
	if err := os.WriteFile(f.paths[trashKeyRelease], []byte("a newer file"), 0o600); err != nil {
		t.Fatal(err)
	}

	if err := tester.Click("Undo"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()
	if data, _ := os.ReadFile(f.paths[trashKeyRelease]); string(data) != "a newer file" {
		t.Fatal("Put Back overwrote an existing file")
	}
	if f.inTrash() != 1 || !tester.HasText("Can't put back — a session file already exists there.") {
		t.Fatalf("trash=%d; texts=%q", f.inTrash(), tester.Texts())
	}
}

// TestDetailPageMoveToTrashLeavesThePage pins the detail header's menu: the
// open session can be moved to the Trash from the page, which then returns
// to the Sessions list instead of staring at a file that is gone.
func TestDetailPageMoveToTrashLeavesThePage(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()
	f.shell.router.Replace(routeHistory + "/" + trashKeySpacing)
	settleFrames(tester, 12)
	if err := tester.Click("Actions"); err != nil {
		t.Fatalf("detail header has no actions pull-down: %v; texts=%q", err, tester.Texts())
	}
	tester.Frame()
	if !containsText(tester.Menu(), "Move to Trash") {
		t.Fatalf("detail actions = %q", tester.Menu())
	}
	if err := tester.ChooseMenuItem("Move to Trash"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 12)

	if pathExists(f.paths[trashKeySpacing]) || f.inTrash() != 1 {
		t.Fatal("the open session was not moved")
	}
	if got := f.shell.router.Path(); got != routeHistory {
		t.Fatalf("still on %q after the session left", got)
	}
}

// TestPreviewMoveToTrashFoldsTheRow pins the inline preview's own action.
func TestPreviewMoveToTrashFoldsTheRow(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()
	if err := tester.Click("Tighten sidebar spacing"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()
	if f.shell.hist.preview.key != trashKeySpacing {
		t.Fatalf("preview = %q", f.shell.hist.preview.key)
	}
	if err := tester.Click("Move to Trash"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if f.shell.hist.preview.key != "" || pathExists(f.paths[trashKeySpacing]) || f.inTrash() != 1 {
		t.Fatalf("preview=%q, file kept=%v, trash=%d", f.shell.hist.preview.key, pathExists(f.paths[trashKeySpacing]), f.inTrash())
	}
}

// TestTrashingThePreviewedRowFromItsMenuClosesThePreview pins that whichever
// path moves a session, a preview showing it does not stay up over a file
// that is gone.
func TestTrashingThePreviewedRowFromItsMenuClosesThePreview(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()
	if err := tester.Click("Tighten sidebar spacing"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()
	if f.shell.hist.preview.key != trashKeySpacing {
		t.Fatalf("preview = %q", f.shell.hist.preview.key)
	}
	if err := tester.RightClick("Tighten sidebar spacing"); err != nil {
		t.Fatal(err)
	}
	if err := tester.ChooseMenuItem("Move to Trash"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if f.shell.hist.preview.key != "" || f.inTrash() != 1 {
		t.Fatalf("preview=%q trash=%d", f.shell.hist.preview.key, f.inTrash())
	}
}

// TestSessionsOfferNoDeleteWithoutATrash pins the degraded start: with no
// Trash store the page has no Select, no Trash toggle and no delete item —
// nothing that would pretend to delete.
func TestSessionsOfferNoDeleteWithoutATrash(t *testing.T) {
	shell := newMultiAgentHistoryShell(t)
	shell.router.Replace(routeHistory)
	tester := ui.NewTester(shell.View, 1200, 800)
	tester.Frame()
	if _, ok := tester.Find("Select sessions"); ok {
		t.Fatalf("Select without a Trash; texts=%q", tester.Texts())
	}
	if err := tester.RightClick("Tighten sidebar spacing"); err != nil {
		t.Fatal(err)
	}
	if containsText(tester.Menu(), "Move to Trash") {
		t.Fatalf("a delete item without a Trash: %q", tester.Menu())
	}
	tester.CloseMenu()
	if err := tester.Click("Tighten sidebar spacing"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()
	if shell.hist.preview.key == "" {
		t.Fatal("the row did not open its preview")
	}
	if tester.HasText("Move to Trash") {
		t.Fatalf("the preview offers a delete without a Trash; texts=%q", tester.Texts())
	}
}

// fakeTrash is an in-memory SessionTrashView for the tests that are about
// the page and not about files: Trash/Restore/Delete/Empty only move entries
// between the live list (not modelled) and its own item list.
type fakeTrash struct {
	mu    sync.Mutex
	items []sessiontrash.Item
	moved int
}

func (f *fakeTrash) Trash(meta history.SessionMeta) (sessiontrash.Item, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.moved++
	item := sessiontrash.Item{
		ID:        fmt.Sprintf("1700000000000-%08x", f.moved),
		Agent:     meta.Agent,
		Original:  meta.FilePath,
		File:      "/trash/" + meta.ID + ".jsonl",
		DeletedAt: time.Unix(1_700_000_000, 0),
		Summary:   history.SessionSummary{Meta: meta},
	}
	f.items = append(f.items, item)
	return item, nil
}

func (f *fakeTrash) Restore(id string) (sessiontrash.Item, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	for i, item := range f.items {
		if item.ID == id {
			f.items = append(f.items[:i:i], f.items[i+1:]...)
			return item, nil
		}
	}
	return sessiontrash.Item{}, sessiontrash.ErrNotFound
}

func (f *fakeTrash) Delete(id string) error {
	_, err := f.Restore(id)
	return err
}

func (f *fakeTrash) Empty() (int, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	n := len(f.items)
	f.items = nil
	return n, nil
}

func (f *fakeTrash) List() ([]sessiontrash.Item, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]sessiontrash.Item(nil), f.items...), nil
}

// trashItemFixture is one Trash entry that never touched a file.
func trashItemFixture(id, title string) sessiontrash.Item {
	return sessiontrash.Item{
		ID:        id,
		Agent:     history.AgentClaudeCode,
		Original:  "/nowhere/" + id + ".jsonl",
		File:      "/trash/" + id + ".jsonl",
		DeletedAt: time.Unix(1_700_000_000, 0),
		Summary: history.SessionSummary{Meta: history.SessionMeta{
			Key: "claude-code:" + id, ID: id, Agent: history.AgentClaudeCode, Title: title,
			ProjectPath: "/work/alpha", ProjectName: "alpha", MessageCount: 2,
		}},
	}
}
