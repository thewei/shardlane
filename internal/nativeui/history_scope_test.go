package nativeui

/**
 * [INPUT]: 依赖 trashFixture（真实临时 Provider 会话文件 + 真实 HistoryService / sessiontrash.Store）、站在项目文件夹上的 Shell 与官方 MyGo headless Tester
 * [OUTPUT]: 验证 Sessions 范围：默认只列当前项目（含子文件夹）、侧栏范围行与计数、切到 All、方向键与 Enter 在范围行间移动、随项目切换重载、无项目即全部、空项目的出路、Trash 随范围、Recent 不被范围污染
 * [POS]: Sessions 项目范围的回归；全程只动临时目录里的夹具会话
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
)

// standOn puts the Shell on a project folder the way a selected Tab does: its
// pane's working directory is what the sidebar head — and the Sessions
// scope — follow.
func standOn(s *Shell, cwd string) {
	s.activeInstance = "inst-1"
	s.projection = herdr.Projection{
		Projects: []herdr.Project{{ID: "w1", Label: "demo", CWD: cwd}},
		Tabs:     []herdr.Tab{{ID: "t1", Label: "main", ProjectID: "w1"}},
		Panes:    []herdr.Pane{{ID: "p1", TerminalID: "term-1", Label: "editor", TabID: "t1", CWD: cwd}},
	}
	s.reconcileLocalSelection(s.projection)
}

// The fixture's sessions: alpha holds k1, k2 and the Codex one, beta holds k3.
// The list groups by project and opens only the newest group (alpha), so a
// project's presence in a list is read off its group card.
const (
	scopeTitleRelease  = "Review the release notes"
	scopeTitleSpacing  = "Tighten sidebar spacing"
	scopeTitleFlaky    = "Fix the flaky terminal test"
	scopeTitleSettings = "Rename the settings groups"
	scopeCardAlpha     = "Project alpha"
	scopeCardBeta      = "Project beta"
)

func scopedTrashFixture(t *testing.T, cwd string) *trashFixture {
	t.Helper()
	f := newTrashFixture(t)
	standOn(f.shell, cwd)
	return f
}

func hasAll(tester *ui.Tester, titles ...string) bool {
	for _, title := range titles {
		if !tester.HasText(title) {
			return false
		}
	}
	return true
}

func hasAny(tester *ui.Tester, titles ...string) bool {
	for _, title := range titles {
		if tester.HasText(title) {
			return true
		}
	}
	return false
}

// TestSessionsOpenOnTheProjectsOwnAndSwitchToAll pins the scope: the list
// opens on the sessions of the project the sidebar head names, the scope rows
// carry the two counts, and "All sessions" widens the list (and back).
func TestSessionsOpenOnTheProjectsOwnAndSwitchToAll(t *testing.T) {
	f := scopedTrashFixture(t, "/work/alpha")
	tester := f.page()
	settleFrames(tester, 3)
	if !hasAll(tester, scopeCardAlpha, scopeTitleRelease, scopeTitleSpacing, scopeTitleFlaky) || hasAny(tester, scopeCardBeta) {
		t.Fatalf("the list must open on /work/alpha's sessions only; texts=%q", tester.Texts())
	}
	if f.shell.hist.projectTotal != 3 || f.shell.hist.allTotal != 4 {
		t.Fatalf("scope rows count %d / %d, want 3 / 4", f.shell.hist.projectTotal, f.shell.hist.allTotal)
	}
	for _, want := range []string{"This project", "All sessions"} {
		if _, ok := tester.Find(want); !ok {
			t.Fatalf("the sidebar has no %q scope row; texts=%q", want, tester.Texts())
		}
	}

	if err := tester.Click("All sessions"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	if !f.shell.hist.scopeAll || !hasAll(tester, scopeCardAlpha, scopeCardBeta) {
		t.Fatalf("All sessions must list every project; scopeAll=%v texts=%q", f.shell.hist.scopeAll, tester.Texts())
	}

	if err := tester.Click("This project"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	if f.shell.hist.scopeAll || hasAny(tester, scopeCardBeta) || !hasAll(tester, scopeTitleRelease) {
		t.Fatalf("This project must narrow the list again; texts=%q", tester.Texts())
	}
}

// TestSessionsScopeCoversSubfoldersOfTheProject pins that a Tab standing in a
// subfolder still scopes to what lives below it, and that the project's tabs
// count only its own sessions.
func TestSessionsScopeCoversSubfoldersOfTheProject(t *testing.T) {
	f := scopedTrashFixture(t, "/work")
	tester := f.page()
	settleFrames(tester, 3)
	if !hasAll(tester, scopeCardAlpha, scopeCardBeta) {
		t.Fatalf("/work holds every fixture session below it; texts=%q", tester.Texts())
	}
	standOn(f.shell, "/work/alpha")
	settleFrames(tester, 3)
	if hasAny(tester, scopeCardBeta) || f.shell.historyTabCount(history.AgentClaudeCode) != 2 || f.shell.historyTabCount(history.AgentCodex) != 1 {
		t.Fatalf("alpha's tabs count claude=%d codex=%d; texts=%q",
			f.shell.historyTabCount(history.AgentClaudeCode), f.shell.historyTabCount(history.AgentCodex), tester.Texts())
	}
}

// TestSessionsListFollowsTheProject pins the reload: switching the project
// (another Tab's folder) re-scopes the open list, and the Agent tab starts
// over because the new project may not have that Agent.
func TestSessionsListFollowsTheProject(t *testing.T) {
	f := scopedTrashFixture(t, "/work/alpha")
	tester := f.page()
	settleFrames(tester, 3)
	f.shell.hist.provider = string(history.AgentCodex)
	f.shell.applyHistoryFilters()
	settleFrames(tester, 3)
	if !hasAll(tester, scopeTitleFlaky) || hasAny(tester, scopeTitleRelease) {
		t.Fatalf("setup: the Codex tab lists the one Codex session; texts=%q", tester.Texts())
	}

	standOn(f.shell, "/work/beta")
	settleFrames(tester, 4)
	if !hasAll(tester, scopeTitleSettings) || hasAny(tester, scopeTitleRelease, scopeTitleFlaky) {
		t.Fatalf("the list must follow the project to /work/beta; texts=%q", tester.Texts())
	}
	if f.shell.hist.provider != "" || f.shell.hist.projectTotal != 1 {
		t.Fatalf("the Agent tab must start over and the count follow: provider=%q projectTotal=%d", f.shell.hist.provider, f.shell.hist.projectTotal)
	}
}

// TestSessionsWithoutAProjectListEverything pins the no-project case: with no
// folder to scope to the list is the whole catalog and only the All row shows.
func TestSessionsWithoutAProjectListEverything(t *testing.T) {
	f := newTrashFixture(t)
	tester := f.page()
	settleFrames(tester, 3)
	if !hasAll(tester, scopeCardAlpha, scopeCardBeta) {
		t.Fatalf("no project must list every session; texts=%q", tester.Texts())
	}
	if _, ok := tester.Find("This project"); ok {
		t.Fatalf("there is no project to scope to; texts=%q", tester.Texts())
	}
}

// TestRemoteInstanceHasNoProjectToScopeTo pins that Sessions — read from this
// Mac — never scope to a path of another machine.
func TestRemoteInstanceHasNoProjectToScopeTo(t *testing.T) {
	f := scopedTrashFixture(t, "/work/alpha")
	if f.shell.historyProjectRoot() != "/work/alpha" {
		t.Fatalf("setup: project root = %q", f.shell.historyProjectRoot())
	}
	f.shell.instances = []herdr.Instance{{Name: "inst-1", Machine: &herdr.Machine{ID: "m1"}}}
	if root := f.shell.historyProjectRoot(); root != "" {
		t.Fatalf("a remote instance scoped Sessions to %q", root)
	}
}

// TestEmptyProjectScopeOffersAllSessions pins the way out of a project with
// no sessions while others have some.
func TestEmptyProjectScopeOffersAllSessions(t *testing.T) {
	f := scopedTrashFixture(t, "/work/none")
	tester := f.page()
	settleFrames(tester, 3)
	if !tester.HasText("No sessions in this project") {
		t.Fatalf("an empty project must say so; texts=%q", tester.Texts())
	}
	if err := tester.Click("Show All Sessions"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	if !f.shell.hist.scopeAll || !hasAll(tester, scopeCardAlpha, scopeCardBeta) {
		t.Fatalf("Show All Sessions must list everything; texts=%q", tester.Texts())
	}
}

// TestTrashFollowsTheScope pins that the Trash view is scoped like the list
// and Empty Trash removes exactly what that view shows, saying so first.
func TestTrashFollowsTheScope(t *testing.T) {
	f := scopedTrashFixture(t, "/work/alpha")
	tester := f.page()
	f.shell.moveSessionsToTrash([]history.SessionMeta{f.meta(trashKeyRelease), f.meta(trashKeySettings)})
	f.shell.setHistoryTrashView(true)
	settleFrames(tester, 4)
	if !hasAll(tester, scopeTitleRelease) || hasAny(tester, scopeTitleSettings) {
		t.Fatalf("the project's Trash holds its own entry only; texts=%q", tester.Texts())
	}

	if err := tester.Click("Empty Trash"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("All 1 session of this project in the Trash will be removed from this Mac. This can't be undone.") {
		t.Fatalf("Empty Trash must say it is the project's; texts=%q", tester.Texts())
	}
	if err := tester.Click("Confirm"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	if f.inTrash() != 1 {
		t.Fatalf("Empty Trash under a project removed %d entries too many (trash=%d)", 2-f.inTrash(), f.inTrash())
	}

	if err := tester.Click("All sessions"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	if !f.shell.hist.showTrash || !hasAll(tester, scopeTitleSettings) {
		t.Fatalf("All sessions must show the other project's Trash entry; texts=%q", tester.Texts())
	}
}

// TestTrashOffersNothingForAProjectWithNoEntries pins that the toggle and the
// view follow the scope: another project's Trash entries are not this one's.
func TestTrashOffersNothingForAProjectWithNoEntries(t *testing.T) {
	f := scopedTrashFixture(t, "/work/alpha")
	tester := f.page()
	f.shell.moveSessionsToTrash([]history.SessionMeta{f.meta(trashKeySettings)})
	settleFrames(tester, 3)
	if f.shell.trashAvailable() {
		t.Fatalf("/work/alpha has nothing in the Trash, yet the page offers it; texts=%q", tester.Texts())
	}
	f.shell.setHistoryTrashView(true)
	settleFrames(tester, 3)
	if !tester.HasText("Nothing from this project in the Trash") {
		t.Fatalf("an empty project Trash must say so; texts=%q", tester.Texts())
	}
}

// TestRecentFeedIgnoresTheProjectScope pins the feed's contract: the
// unfiltered latest sessions of the whole catalog, never a project's slice.
func TestRecentFeedIgnoresTheProjectScope(t *testing.T) {
	f := scopedTrashFixture(t, "/work/beta")
	tester := f.page()
	settleFrames(tester, 3)
	if len(f.shell.recentItems) != 0 {
		t.Fatalf("a project-scoped list fed the Recent rows: %+v", f.shell.recentItems)
	}
	if err := tester.Click("All sessions"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 3)
	if len(f.shell.recentItems) != 4 {
		t.Fatalf("the unscoped list must feed every session, got %d", len(f.shell.recentItems))
	}
}

// TestSessionScopeRowsTakeArrowsAndEnter pins the keyboard: the two scope
// rows are one Tab stop, Up and Down move between them and Enter picks — no
// letter keys involved.
func TestSessionScopeRowsTakeArrowsAndEnter(t *testing.T) {
	f := scopedTrashFixture(t, "/work/alpha")
	tester := f.page()
	settleFrames(tester, 3)
	if err := tester.Click("This project"); err != nil {
		t.Fatal(err)
	}
	settleFrames(tester, 2)
	pressKey(tester, 0, ui.KeyDown)
	wantCursor(t, tester, "All sessions")
	pressKey(tester, 0, ui.KeyEnter)
	settleFrames(tester, 3)
	if !f.shell.hist.scopeAll {
		t.Fatalf("Enter on All sessions did not widen the scope; texts=%q", tester.Texts())
	}
	pressKey(tester, 0, ui.KeyUp)
	wantCursor(t, tester, "This project")
}
