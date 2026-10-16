package nativeui

import (
	"math"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

/**
 * [INPUT]: 依赖 Shell/routeHeader/routeHeaderBlock、各内页入口（Hosts/Files/Sessions/Session Detail/Settings）与 ui.Tester 无头渲染
 * [OUTPUT]: 对外提供标题栏单一归属回归：内页标题只在标题栏且落在同一位置、内页与侧栏不再自画页头/标题回声、每个内页至多一个 Refresh 且为图标、Settings/Hosts 侧栏列表同一起点、Git/Files/Sessions 侧栏同一项目头、Session 详情标题栏可返回列表
 * [POS]: 2026-10-14 统一 Header 决策的守卫测试；与 route_header.go 同生共死
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// headerRoute builds a shell standing on one non-workspace route.
type headerRoute struct {
	name  string
	title string // the crumb the title bar must show
	setup func(t *testing.T) *Shell
}

func headerRoutes() []headerRoute {
	return []headerRoute{
		{"hosts", "Hosts", func(t *testing.T) *Shell {
			s := NewShell()
			s.hosts.loaded = true // never read the developer's real ~/.ssh/config
			s.router.Replace(routeHosts)
			return s
		}},
		{"files", "Files", func(t *testing.T) *Shell {
			root := t.TempDir()
			if err := os.WriteFile(filepath.Join(root, "README.md"), []byte("x"), 0o600); err != nil {
				t.Fatal(err)
			}
			s := rightPanelTestShell(t, root)
			s.showViewFiles()
			return s
		}},
		{"sessions", "Sessions", func(t *testing.T) *Shell {
			s := newMultiAgentHistoryShell(t)
			s.router.Replace(routeHistory)
			return s
		}},
		{"settings", "Settings", func(t *testing.T) *Shell {
			s := NewShell()
			s.router.Replace("/settings/general")
			return s
		}},
		{"changes", "Changes", func(t *testing.T) *Shell {
			s := rightPanelTestShell(t, t.TempDir())
			s.showSurface(WorkspaceSurfaceDiff)
			return s
		}},
		{"chat", "Chat", func(t *testing.T) *Shell { // the legacy full page
			s := NewShell()
			s.router.Replace("/chat")
			return s
		}},
	}
}

func settleFrames(tester *ui.Tester, frames int) {
	for i := 0; i < frames; i++ {
		tester.Frame()
		time.Sleep(20 * time.Millisecond)
	}
}

// TestTitleBarNamesTheRouteInOneSharedPlace pins the 2026-10-14 header
// decision: every inner page is named in the title bar, at the same spot on
// every route, never in the sidebar and never in the page.
func TestTitleBarNamesTheRouteInOneSharedPlace(t *testing.T) {
	var baseline float32
	for i, route := range headerRoutes() {
		tester := ui.NewTester(route.setup(t).View, 1200, 800)
		settleFrames(tester, 3)
		rect, ok := tester.Find(route.title)
		if !ok {
			t.Fatalf("%s: the title bar does not name the page %q; texts=%q", route.name, route.title, tester.Texts())
		}
		if rect.X < sidebarWidth {
			t.Errorf("%s: %q first shows at x=%.0f, inside the sidebar", route.name, route.title, rect.X)
		}
		if rect.Y+rect.H > titlebarHeight(0) {
			t.Errorf("%s: %q sits at y=%.1f–%.1f, below the title bar", route.name, route.title, rect.Y, rect.Y+rect.H)
		}
		if i == 0 {
			baseline = rect.Y
		} else if rect.Y < baseline-0.5 || rect.Y > baseline+0.5 {
			t.Errorf("%s: title at y=%.1f, other pages at y=%.1f — entering a page must not move it", route.name, rect.Y, baseline)
		}
	}
}

// TestInnerTitlesAreOneLargeRow pins the 2026-10-14 look of the bar: every
// inner page is a single row of Title-size type — no crumb chip, no second
// context row — so the Files folder, the Hosts config path and the like do
// not return as a muted line under it.
func TestInnerTitlesAreOneLargeRow(t *testing.T) {
	for _, route := range headerRoutes() {
		s := route.setup(t)
		tester := ui.NewTester(s.View, 1200, 800)
		settleFrames(tester, 3)
		rect, ok := tester.Find(route.title)
		if !ok {
			t.Fatalf("%s: title %q missing; texts=%q", route.name, route.title, tester.Texts())
		}
		if rect.H < Typography().Title {
			t.Errorf("%s: title is %.1f high, want Title-size type (>= %.0f)", route.name, rect.H, Typography().Title)
		}
		for _, echo := range []string{shortDir(s.rightPanel.currentRoot), compactSearchPath(s.hosts.mainPath)} {
			if echo != "" && echo != "." && tester.HasText(echo) {
				t.Errorf("%s: the bar repeats %q as a second row; texts=%q", route.name, echo, tester.Texts())
			}
		}
	}
}

// TestPagesDrawNoHeaderOfTheirOwn renders each inner page without the shell
// around it: whatever the title bar owns must not be drawn again inside.
func TestPagesDrawNoHeaderOfTheirOwn(t *testing.T) {
	root := t.TempDir()
	hosts := NewShell()
	hosts.hosts.loaded = true
	sessions := newMultiAgentHistoryShell(t)
	files := rightPanelTestShell(t, root)
	settings := NewShell()
	chat := NewShell()
	for _, tc := range []struct {
		name  string
		title string
		page  func(c *ui.Context)
		// sidebarRefresh marks the page whose sidebar owns the one Refresh
		// (the Hosts list, the Sessions project head).
		sidebarRefresh bool
	}{
		{"hosts", "Hosts", hosts.hostsPage, true},
		{"files", "Files", files.filesPage, false},
		{"sessions", "Sessions", sessions.historyPage, true},
		{"settings", "General", func(c *ui.Context) { settingsPage(c, func() { settings.settingsGeneral(c) }) }, false},
		{"chat", "Chat", chat.chatPage, false},
	} {
		tester := ui.NewTester(tc.page, 900, 700)
		settleFrames(tester, 2)
		texts := tester.Texts()
		if got := countTextExact(texts, tc.title); got != 0 {
			t.Errorf("%s: the page repeats the title %q the title bar owns; texts=%q", tc.name, tc.title, texts)
		}
		if got := countTextExact(texts, "Refresh"); tc.sidebarRefresh && got != 0 {
			t.Errorf("%s: the page draws a Refresh the sidebar already has; texts=%q", tc.name, texts)
		}
	}
}

// TestSidebarListsStartAtTheirFirstEntry pins the other half of the same
// rule: the inner-page sidebars no longer echo the page title above the list.
func TestSidebarListsStartAtTheirFirstEntry(t *testing.T) {
	s := newMultiAgentHistoryShell(t)
	for _, tc := range []struct {
		name  string
		title string
		nav   func(c *ui.Context)
	}{
		{"settings", "Settings", s.settingsNav},
		{"sessions", "Sessions", s.historyNav},
	} {
		tester := ui.NewTester(tc.nav, 260, 600)
		tester.Frame()
		if got := countTextExact(tester.Texts(), tc.title); got != 0 {
			t.Errorf("%s: the sidebar echoes the page title %q; texts=%q", tc.name, tc.title, tester.Texts())
		}
	}
}

// TestEachRouteHasOneRefreshAndItIsAnIcon pins the duplicate audit: Hosts had
// a Refresh in the sidebar and another in the page; Agents wore a text
// button while every other refresh is an icon. One per context, one face.
func TestEachRouteHasOneRefreshAndItIsAnIcon(t *testing.T) {
	for _, tc := range []struct {
		name    string
		refresh string // the control's label: the sidebar head names what it refreshes
		setup   func(t *testing.T) *Shell
	}{
		{"hosts", "Refresh", headerRoutes()[0].setup},
		{"sessions", "Refresh sessions", headerRoutes()[2].setup},
		{"agents", "Refresh", func(t *testing.T) *Shell {
			s := NewShell()
			s.providers.loaded = true // never start a real audit
			s.router.Replace("/settings/providers")
			return s
		}},
	} {
		tester := ui.NewTester(tc.setup(t).View, 1200, 800)
		settleFrames(tester, 3)
		if got := countTextExact(tester.Texts(), tc.refresh); got != 1 {
			t.Errorf("%s: %d %q controls, want exactly one; texts=%q", tc.name, got, tc.refresh, tester.Texts())
			continue
		}
		rect, _ := tester.Find(tc.refresh)
		if rect.W > 32 || rect.H > 32 {
			t.Errorf("%s: Refresh is %.0f×%.0f — an icon button, not a text button or a navigation row", tc.name, rect.W, rect.H)
		}
	}
}

// TestInnerSidebarsShareOneInset pins the sidebar body's inset: entering
// Hosts, the first line of the list starts where Settings' used to; Settings
// dropped the view-tab strip (2026-10-15), so its list starts one strip
// higher, at the same x.
func TestInnerSidebarsShareOneInset(t *testing.T) {
	routes := headerRoutes()
	var wantX, hostsY float32
	for i, tc := range []struct {
		name  string
		setup func(t *testing.T) *Shell
		first string // the first line of that page's sidebar list
	}{
		{"hosts", routes[0].setup, "SSH Config"},
		{"settings", routes[3].setup, "General"},
	} {
		tester := ui.NewTester(tc.setup(t).View, 1200, 800)
		settleFrames(tester, 3)
		rect, ok := tester.Find(tc.first)
		if !ok {
			t.Fatalf("%s: %q missing; texts=%q", tc.name, tc.first, tester.Texts())
		}
		if i == 0 {
			wantX, hostsY = rect.X, rect.Y
			continue
		}
		if math.Abs(float64(rect.X-wantX)) > 0.25 {
			t.Errorf("%s: sidebar list starts at x %.1f, the other inner pages at %.1f", tc.name, rect.X, wantX)
		}
		// The strip the Settings sidebar no longer renders: one icon tab row
		// (padding puts all six points of spacing below it).
		strip := gorexIconBtn + Spacing().S
		if math.Abs(float64(rect.Y-(hostsY-strip))) > 0.25 {
			t.Errorf("%s: sidebar list starts at y %.1f, want exactly one view-tab strip (%.0f) above the other inner pages' %.1f", tc.name, rect.Y, strip, hostsY)
		}
	}
}

// TestSessionBarLeadsBackToTheList pins the detail route's title bar: the
// Sessions crumb is the way back, as Settings › Git's parent names its page.
func TestSessionBarLeadsBackToTheList(t *testing.T) {
	s := newMultiAgentHistoryShell(t)
	s.router.Replace(routeHistory)
	tester := ui.NewTester(s.View, 1200, 800)
	settleFrames(tester, 2)
	s.router.Replace(routeHistory + "/pi:p2")
	settleFrames(tester, 12)
	if !tester.HasText("Rename the settings groups") || !tester.HasText("alpha · Pi") {
		t.Fatalf("the title bar must carry the session title and its facts; texts=%q", tester.Texts())
	}
	if err := tester.Click("Sessions"); err != nil {
		t.Fatalf("no Sessions crumb to go back with: %v; texts=%q", err, tester.Texts())
	}
	settleFrames(tester, 3)
	if got := s.router.Path(); got != routeHistory {
		t.Fatalf("Sessions crumb led to %q, want the list", got)
	}
}

// TestLongSessionTitleCannotPushTheBarsToolsOut pins the title crumb's
// truncation: a session title is user text of any length, and in a narrow
// window it must give way (ellipsis) instead of shoving the Actions pull-down
// and the trailing tools off the bar.
func TestLongSessionTitleCannotPushTheBarsToolsOut(t *testing.T) {
	long := "Investigate why the terminal resize handshake sometimes drops the last row after a split and then rewrite the whole attach path"
	catalog := history.NewCatalog()
	meta := history.SessionMeta{
		Key: "pi:long", ID: "long", Agent: history.AgentPi, Title: long, ProjectPath: "/work/alpha",
		ProjectName: "alpha", FilePath: "/nowhere/long", UpdatedAt: time.Now().UnixMilli(), MessageCount: 2,
	}
	catalog.Put(meta, meta.UpdatedAt, "")
	s := NewShell(WithHistory(history.NewHistoryService(catalog, nil)))
	s.router.Replace(routeHistory)
	const width = 760
	tester := ui.NewTester(s.View, width, 600)
	settleFrames(tester, 2)
	s.router.Replace(routeHistory + "/pi:long")
	settleFrames(tester, 12)
	title, ok := tester.Find(long)
	if !ok {
		t.Fatalf("session title missing from the bar; texts=%q", tester.Texts())
	}
	actions, ok := tester.Find("Actions")
	if !ok {
		t.Fatalf("Actions pull-down missing; texts=%q", tester.Texts())
	}
	tools, ok := tester.Find(windowPinLabel)
	if !ok {
		t.Fatalf("trailing tools missing; texts=%q", tester.Texts())
	}
	if title.X+title.W > actions.X+0.5 {
		t.Errorf("title (ends %.1f) runs into Actions (starts %.1f)", title.X+title.W, actions.X)
	}
	if actions.X+actions.W > tools.X+0.5 {
		t.Errorf("Actions (ends %.1f) runs into the tools (start %.1f)", actions.X+actions.W, tools.X)
	}
}

// TestRouteHeaderIdentity pins what the bar says per route, without
// rendering: parents, the way back, the facts and the Actions.
func TestRouteHeaderIdentity(t *testing.T) {
	s := newMultiAgentHistoryShell(t)
	s.router.Replace(routeHistory)
	// Rendering the list loads the catalog the detail header reads its meta from.
	settleFrames(ui.NewTester(s.View, 1200, 800), 3)

	s.router.Replace(routeWorkspace)
	if _, ok := s.routeHeader(); ok {
		t.Fatal("the Terminal keeps its own breadcrumb, not a route header")
	}
	s.showSurface(WorkspaceSurfaceDiff)
	spec, ok := s.routeHeader()
	if !ok || spec.title != "Changes" || spec.parent != "" || spec.actions == nil {
		t.Fatalf("Changes is a page of its own, titled in the bar with its Actions: %+v", spec)
	}
	s.showSurface(WorkspaceSurfaceTerminal)
	if _, ok := s.routeHeader(); ok {
		t.Fatal("back on the Terminal the breadcrumb returns")
	}

	s.router.Replace(routeHistory)
	spec, ok = s.routeHeader()
	if !ok || spec.title != "Sessions" || spec.parent != "" {
		t.Fatalf("sessions list header = %+v", spec)
	}

	s.router.Replace("/settings/git")
	spec, _ = s.routeHeader()
	if spec.parent != "Settings" || spec.title != "Git" || spec.onParent != nil {
		t.Fatalf("settings section header = %+v", spec)
	}
	s.router.Replace("/settings/nope")
	if spec, _ = s.routeHeader(); spec.title != "General" {
		t.Fatalf("an unknown section falls back to General, got %q", spec.title)
	}

	s.router.Replace(routeHosts)
	if spec, _ = s.routeHeader(); spec.title != "Hosts" || spec.titleTip == "" || spec.facts != "" {
		t.Fatalf("hosts header must name the config it indexes in its tooltip only: %+v", spec)
	}

	s.router.Replace("/inspector/p9")
	if spec, _ = s.routeHeader(); spec.title != "Agent Inspector" {
		t.Fatalf("inspector without a card keeps the generic title, got %q", spec.title)
	}

	// Session detail: back parent, session title, facts and its action menu.
	s.router.Replace(routeHistory + "/pi:p2")
	spec, _ = s.routeHeader()
	if spec.parent != "Sessions" || spec.onParent == nil ||
		spec.title != "Rename the settings groups" || spec.facts != "alpha · Pi" || spec.actions == nil {
		t.Fatalf("session detail header = %+v", spec)
	}
}

// TestFilesRouteHeaderCarriesPathActionsOnlyWithASelection pins the bar's
// Actions on /files: Copy Path and Reveal in Finder make sense once a file is
// open, so they were the page header's buttons exactly then.
func TestFilesRouteHeaderCarriesPathActionsOnlyWithASelection(t *testing.T) {
	root := t.TempDir()
	s := rightPanelTestShell(t, root)
	s.showViewFiles()
	spec, ok := s.routeHeader()
	if !ok || spec.title != "Files" || spec.titleTip != root {
		t.Fatalf("files header = %+v, want Files with %q as its tooltip", spec, root)
	}
	if spec.actions != nil {
		t.Fatal("no file open: nothing to act on")
	}
	s.rightPanel.selectedFilePath = filepath.Join(root, "a.txt")
	if spec, _ = s.routeHeader(); spec.actions == nil {
		t.Fatal("a file is open: the bar must offer Copy Path / Reveal in Finder")
	}
}

// TestProjectSidebarsShareOneHead pins the project head Git, Files and
// Sessions open their sidebars with: the project chip and the one Refresh
// icon sit at the same place on all three, so moving between the pages moves
// nothing above the list.
func TestProjectSidebarsShareOneHead(t *testing.T) {
	routes := headerRoutes()
	type spot struct{ chipX, chipY, refreshX, refreshY float32 }
	var want spot
	for i, tc := range []struct {
		name    string
		setup   func(t *testing.T) *Shell
		refresh string
	}{
		{"changes", routes[4].setup, "Refresh repository"},
		{"files", routes[1].setup, "Refresh files"},
		{"sessions", func(t *testing.T) *Shell { // a workspace with a project, like the other two
			s := rightPanelTestShell(t, t.TempDir())
			s.hist.service = newMultiAgentHistoryShell(t).hist.service
			s.router.Replace(routeHistory)
			return s
		}, "Refresh sessions"},
	} {
		s := tc.setup(t)
		tester := ui.NewTester(s.View, 1200, 800)
		settleFrames(tester, 3)
		chip, ok := tester.Find(s.sidebarProjectTitle())
		if !ok {
			t.Fatalf("%s: the sidebar has no project head %q; texts=%q", tc.name, s.sidebarProjectTitle(), tester.Texts())
		}
		refresh, ok := tester.Find(tc.refresh)
		if !ok {
			t.Fatalf("%s: the project head has no %q; texts=%q", tc.name, tc.refresh, tester.Texts())
		}
		got := spot{chip.X, chip.Y, refresh.X, refresh.Y}
		if i == 0 {
			want = got
			continue
		}
		for _, d := range []float32{got.chipX - want.chipX, got.chipY - want.chipY, got.refreshX - want.refreshX, got.refreshY - want.refreshY} {
			if math.Abs(float64(d)) > 0.25 {
				t.Errorf("%s: the project head sits at %+v, Git's at %+v", tc.name, got, want)
				break
			}
		}
	}
}
