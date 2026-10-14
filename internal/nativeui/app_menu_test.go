package nativeui

// [INPUT]: 依赖 app_menu.go 的 MenuBar、command_center_ui.go 的命令注册表、sidebar_test.go 的 sidebarViewTabLabels
// [OUTPUT]: 菜单栏结构（macOS 惯例）、三个启动器（侧栏标签 / ⌘P / Go 菜单）同名同目的地、"点名当前页保持不动"的回归
// [POS]: 菜单栏与命令面板的守卫；菜单结构不在 main.go，所以这里能测
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

import (
	"slices"
	"strings"
	"testing"

	"github.com/egoist/mygo"

	"github.com/wh-studio/herdr-client/internal/commandcenter"
)

// menuLabels lists a submenu's labels, separators as "-".
func menuLabels(items []*mygo.MenuItem) []string {
	out := make([]string, 0, len(items))
	for _, it := range items {
		if it.Type == mygo.MenuItemSeparator {
			out = append(out, "-")
			continue
		}
		out = append(out, it.Label)
	}
	return out
}

func menuNamed(t *testing.T, menu *mygo.Menu, label string) *mygo.MenuItem {
	t.Helper()
	for _, it := range menu.Items() {
		if it.Label == label {
			return it
		}
	}
	t.Fatalf("no %q menu in the bar", label)
	return nil
}

func menuItemNamed(items []*mygo.MenuItem, label string) *mygo.MenuItem {
	for _, it := range items {
		if it.Label == label {
			return it
		}
	}
	return nil
}

// TestMenuBarFollowsTheMacConventions pins the bar's shape: the application
// menu is the stock one — Settings… (⌘,) between About and Hide, never in a
// custom menu — and every command lives in exactly one menu.
func TestMenuBarFollowsTheMacConventions(t *testing.T) {
	s := NewShell()
	menu := s.MenuBar(&mygo.MenuItem{Label: "Check for Updates…"})

	var bar []string
	for _, it := range menu.Items() {
		bar = append(bar, it.Label)
	}
	if want := []string{"Shardlane", "Go", "Edit", "Window"}; !slices.Equal(bar, want) {
		t.Fatalf("menu bar = %q, want %q", bar, want)
	}

	appMenu := menuNamed(t, menu, "Shardlane").Submenu
	labels := menuLabels(appMenu)
	// About, Hide and Quit carry the app's name: match by prefix.
	wantApp := []string{"About ", "Check for Updates…", "-", "Settings…", "-", "Hide ", "Hide Others", "Show All", "-", "Quit "}
	if len(labels) != len(wantApp) || labels[5] == "Hide Others" {
		t.Fatalf("application menu = %q, want the shape %q", labels, wantApp)
	}
	for i, want := range wantApp {
		if !strings.HasPrefix(labels[i], want) {
			t.Fatalf("application menu item %d = %q, want %q… (menu %q)", i, labels[i], want, labels)
		}
	}
	if settings := menuItemNamed(appMenu, "Settings…"); settings.Accelerator != "CmdOrCtrl+," {
		t.Fatalf("Settings… accelerator = %q, want CmdOrCtrl+,", settings.Accelerator)
	}
	if hide := appMenu[5]; hide.Accelerator != "Cmd+H" {
		t.Fatalf("%q accelerator = %q — ⌘H must hide the app", hide.Label, hide.Accelerator)
	}

	seen := map[string]string{}
	for _, name := range []string{"Shardlane", "Go"} {
		for _, it := range menuNamed(t, menu, name).Submenu {
			if it.Type == mygo.MenuItemSeparator {
				continue
			}
			label := strings.TrimSuffix(it.Label, "…")
			if other, dup := seen[label]; dup {
				t.Errorf("%q is in the %s menu and the %s menu — a command lives in one", it.Label, other, name)
			}
			seen[label] = name
		}
	}
}

// viewDestinations pairs each sidebar view tab with the name the other two
// launchers — the ⌘P palette and the Go menu — give the same page.
var viewDestinations = []struct{ tab, name string }{
	{"Changes view", "Changes"},
	{"History view", "Sessions"},
	{"Files view", "Files"},
	{"Hosts view", "Hosts"},
}

// TestLaunchersAgreeOnViewDestinations pins the rule that a page reachable
// from the sidebar is reachable from the palette and the menu bar, under one
// name and in the tabs' order: the Changes view was once a sidebar-only page.
func TestLaunchersAgreeOnViewDestinations(t *testing.T) {
	var tabs []string
	for _, d := range viewDestinations {
		tabs = append(tabs, d.tab)
	}
	if !slices.Equal(tabs, sidebarViewTabLabels) {
		t.Fatalf("destinations %q must cover the sidebar tabs %q, in order", tabs, sidebarViewTabLabels)
	}

	s := NewShell()
	palette := map[string]bool{}
	for _, a := range s.buildCommandCenterActions() {
		if a.Section == "App" && a.Available {
			palette[a.Title] = true
		}
	}
	goMenu := menuLabels(menuNamed(t, s.MenuBar(nil), "Go").Submenu)
	var inMenu []string
	for _, d := range viewDestinations {
		if !palette[d.name] {
			t.Errorf("%s has no %q action in the Command Center", d.tab, d.name)
		}
		if !slices.Contains(goMenu, d.name) {
			t.Errorf("%s has no %q item in the Go menu %q", d.tab, d.name, goMenu)
		}
		if i := slices.Index(goMenu, d.name); i >= 0 {
			inMenu = append(inMenu, goMenu[i])
		}
	}
	var order []string
	for _, d := range viewDestinations {
		order = append(order, d.name)
	}
	if !slices.IsSorted(indexesIn(goMenu, order)) {
		t.Errorf("Go menu lists the views as %q, the tabs run %q", inMenu, order)
	}
}

func indexesIn(list, names []string) []int {
	out := make([]int, 0, len(names))
	for _, n := range names {
		out = append(out, slices.Index(list, n))
	}
	return out
}

// TestNamingTheCurrentViewKeepsIt pins the destination form of the view
// launchers: the palette and the Go menu name a page, so choosing the one
// already open stays on it — only the sidebar tab toggles back to the Terminal.
func TestNamingTheCurrentViewKeepsIt(t *testing.T) {
	landing := func(s *Shell) string {
		return s.router.Path() + "|" + string(s.surface.current())
	}
	// Only Changes is a Workspace surface; the other pages leave the surface
	// as the shell had it.
	want := func(s *Shell, name string) string {
		switch name {
		case "Changes":
			return routeWorkspace + "|" + string(WorkspaceSurfaceDiff)
		case "Sessions":
			return routeHistory + "|" + string(s.surface.current())
		case "Files":
			return routeFiles + "|" + string(s.surface.current())
		}
		return routeHosts + "|" + string(s.surface.current())
	}
	for _, via := range []string{"palette", "menu"} {
		for _, d := range viewDestinations {
			s := gitSurfaceTestShell(t, t.TempDir())
			s.hosts.loaded = true // never read the developer's real ~/.ssh/config
			choose := func() {
				if via == "menu" {
					item := menuItemNamed(menuNamed(t, s.MenuBar(nil), "Go").Submenu, d.name)
					if item == nil {
						t.Fatalf("no %q in the Go menu", d.name)
					}
					item.Click(item, nil)
					return
				}
				for _, a := range s.buildCommandCenterActions() {
					if a.Title == d.name && a.Section == "App" {
						s.executeCommandCenterResult(commandcenter.Result{Action: a})
						return
					}
				}
				t.Fatalf("no %q in the Command Center", d.name)
			}
			expected := want(s, d.name)
			choose()
			first := landing(s)
			if first != expected {
				t.Errorf("%s %q landed on %q, want %q", via, d.name, first, expected)
			}
			choose()
			if again := landing(s); again != first {
				t.Errorf("%s %q a second time went to %q — naming the open page keeps it (was %q)", via, d.name, again, first)
			}
		}
	}
}

// TestWorkspaceMenuLandsTheTerminalFromAnySurface pins Go ▸ Workspace (⌘1):
// from the Workspace's own Changes or Chat surface a bare route push was a
// no-op (same route), so the item looked dead; it lands the Terminal like
// Back does, and choosing it again keeps the Terminal.
func TestWorkspaceMenuLandsTheTerminalFromAnySurface(t *testing.T) {
	goTo := func(s *Shell) {
		item := menuItemNamed(menuNamed(t, s.MenuBar(nil), "Go").Submenu, "Workspace")
		if item == nil {
			t.Fatal("no Workspace in the Go menu")
		}
		item.Click(item, nil)
	}
	starts := map[string]func(s *Shell){
		"Changes": func(s *Shell) { s.showViewChanges() },
		"Chat":    func(s *Shell) { s.surface.openChat(s.workspaceContext()) },
		"Sessions with a Changes view left behind": func(s *Shell) {
			s.showViewChanges()
			s.router.Push(routeHistory)
		},
		"Files": func(s *Shell) { s.router.Push(routeFiles) },
	}
	for name, start := range starts {
		s := gitSurfaceTestShell(t, t.TempDir())
		start(s)
		goTo(s)
		if got := s.router.Path(); got != routeWorkspace || s.surface.current() != WorkspaceSurfaceTerminal {
			t.Errorf("Workspace from %s landed on %s showing %q, want the Terminal at %s", name, got, s.surface.current(), routeWorkspace)
		}
		goTo(s)
		if got := s.router.Path(); got != routeWorkspace || s.surface.current() != WorkspaceSurfaceTerminal {
			t.Errorf("Workspace a second time (from %s) left the Terminal: %s showing %q", name, got, s.surface.current())
		}
	}
}
