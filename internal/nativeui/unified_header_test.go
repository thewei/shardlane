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
 * [OUTPUT]: 对外提供标题栏单一归属回归：内页标题只在标题栏且落在同一位置、内页与侧栏不再自画页头/标题回声、每个内页至多一个 Refresh 且为图标、Sessions/Settings/Hosts 侧栏列表同一起点、Session 详情标题栏可返回列表
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

// TestPagesDrawNoHeaderOfTheirOwn renders each inner page without the shell
// around it: whatever the title bar owns must not be drawn again inside.
func TestPagesDrawNoHeaderOfTheirOwn(t *testing.T) {
	root := t.TempDir()
	hosts := NewShell()
	hosts.hosts.loaded = true
	sessions := newMultiAgentHistoryShell(t)
	files := rightPanelTestShell(t, root)
	settings := NewShell()
	for _, tc := range []struct {
		name  string
		title string
		page  func(c *ui.Context)
		// sidebarRefresh marks the page whose list lives in the sidebar,
		// which owns the one Refresh (Sessions keeps its toolbar icon: no
		// list in its sidebar to host one).
		sidebarRefresh bool
	}{
		{"hosts", "Hosts", hosts.hostsPage, true},
		{"files", "Files", files.filesPage, false},
		{"sessions", "Sessions", sessions.historyPage, false},
		{"settings", "General", func(c *ui.Context) { settingsPage(c, func() { settings.settingsGeneral(c) }) }, false},
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
		name  string
		setup func(t *testing.T) *Shell
	}{
		{"hosts", headerRoutes()[0].setup},
		{"sessions", headerRoutes()[2].setup},
		{"agents", func(t *testing.T) *Shell {
			s := NewShell()
			s.providers.loaded = true // never start a real audit
			s.router.Replace("/settings/providers")
			return s
		}},
	} {
		tester := ui.NewTester(tc.setup(t).View, 1200, 800)
		settleFrames(tester, 3)
		if got := countTextExact(tester.Texts(), "Refresh"); got != 1 {
			t.Errorf("%s: %d Refresh controls, want exactly one; texts=%q", tc.name, got, tester.Texts())
			continue
		}
		rect, _ := tester.Find("Refresh")
		if rect.W > 32 || rect.H > 32 {
			t.Errorf("%s: Refresh is %.0f×%.0f — an icon button, not a text button or a navigation row", tc.name, rect.W, rect.H)
		}
	}
}

// TestInnerSidebarsShareOneInset pins the sidebar body's inset: entering
// Sessions, Settings or Hosts, the first line of the list starts at the same
// x and y (the Hosts list once inset itself a second time: 14pt lower, 9pt in).
func TestInnerSidebarsShareOneInset(t *testing.T) {
	routes := headerRoutes()
	var wantX, wantY float32
	for i, tc := range []struct {
		name  string
		setup func(t *testing.T) *Shell
		first string // the first line of that page's sidebar list
	}{
		{"sessions", routes[2].setup, "All sessions"},
		{"settings", routes[3].setup, "General"},
		{"hosts", routes[0].setup, "SSH Config"},
	} {
		tester := ui.NewTester(tc.setup(t).View, 1200, 800)
		settleFrames(tester, 3)
		rect, ok := tester.Find(tc.first)
		if !ok {
			t.Fatalf("%s: %q missing; texts=%q", tc.name, tc.first, tester.Texts())
		}
		if i == 0 {
			wantX, wantY = rect.X, rect.Y
			continue
		}
		if math.Abs(float64(rect.X-wantX)) > 0.25 || math.Abs(float64(rect.Y-wantY)) > 0.25 {
			t.Errorf("%s: sidebar list starts at (%.1f, %.1f), the other inner pages at (%.1f, %.1f)", tc.name, rect.X, rect.Y, wantX, wantY)
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
	tools, ok := tester.Find("Changes view")
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
// rendering: parents, the way back, the context line and the Actions.
func TestRouteHeaderIdentity(t *testing.T) {
	s := newMultiAgentHistoryShell(t)
	s.router.Replace(routeHistory)
	// Rendering the list loads the catalog the detail header reads its meta from.
	settleFrames(ui.NewTester(s.View, 1200, 800), 3)

	s.router.Replace(routeWorkspace)
	if _, ok := s.routeHeader(false); ok {
		t.Fatal("the workspace keeps its own breadcrumb, not a route header")
	}

	s.router.Replace(routeHistory)
	spec, ok := s.routeHeader(false)
	if !ok || spec.current.label != "Sessions" || spec.parent.label != "" {
		t.Fatalf("sessions list header = %+v", spec)
	}

	s.router.Replace("/settings/git")
	spec, _ = s.routeHeader(false)
	if spec.parent.label != "Settings" || spec.current.label != "Git" || spec.onParent != nil {
		t.Fatalf("settings section header = %+v", spec)
	}
	s.router.Replace("/settings/nope")
	if spec, _ = s.routeHeader(false); spec.current.label != "General" {
		t.Fatalf("an unknown section falls back to General, got %q", spec.current.label)
	}

	s.router.Replace(routeHosts)
	if spec, _ = s.routeHeader(false); spec.current.label != "Hosts" || spec.detail == "" {
		t.Fatalf("hosts header must name the config it indexes: %+v", spec)
	}

	s.router.Replace("/inspector/p9")
	if spec, _ = s.routeHeader(false); spec.current.label != "Agent Inspector" {
		t.Fatalf("inspector without a card keeps the generic title, got %q", spec.current.label)
	}

	// Session detail: back crumb, session title, facts and its action menu.
	s.router.Replace(routeHistory + "/pi:p2")
	spec, _ = s.routeHeader(false)
	if spec.parent.label != "Sessions" || spec.onParent == nil ||
		spec.current.label != "Rename the settings groups" || spec.detail != "alpha · Pi" || spec.actions == nil {
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
	spec, ok := s.routeHeader(false)
	if !ok || spec.current.label != "Files" || spec.detail != shortDir(root) {
		t.Fatalf("files header = %+v, want Files over %q", spec, shortDir(root))
	}
	if spec.actions != nil {
		t.Fatal("no file open: nothing to act on")
	}
	s.rightPanel.selectedFilePath = filepath.Join(root, "a.txt")
	if spec, _ = s.routeHeader(false); spec.actions == nil {
		t.Fatal("a file is open: the bar must offer Copy Path / Reveal in Finder")
	}
}
