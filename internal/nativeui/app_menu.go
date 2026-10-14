package nativeui

// [INPUT]: 依赖 shell.go 的菜单接缝（Navigate/ShowWorkspaceFromMenu/ShowChangesFromMenu/ShowFilesFromMenu/OpenCommandCenterFromMenu）、window_actions.go 的 CloseTabFromMenu 与 MyGo 菜单 API
// [OUTPUT]: 对外提供 MenuBar（macOS 菜单栏：应用菜单 + Go + Edit + Window）
// [POS]: 菜单栏的单一来源（原内联在 main.go，现可测试）：页面入口与侧栏视图标签、⌘P 命令面板同名同序（TestLaunchersAgreeOnViewDestinations）
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

import "github.com/egoist/mygo"

// MenuBar builds the macOS menu bar. checkForUpdates is the updater plugin's
// item — a plugin main.go owns — and sits after About, where macOS apps put it.
//
// The application menu is the stock one (About, Settings…, Hide, Quit); Go
// carries the app's own destinations in the order of the sidebar's view
// tabs, and accelerators only on idempotent destinations (Router.Push
// dedupes; the Terminal view keeps the Terminal), never on a toggle, so a
// menu accelerator and an in-app shortcut that fire together stay harmless.
func (s *Shell) MenuBar(checkForUpdates *mygo.MenuItem) *mygo.Menu {
	nav := func(label, accelerator, path string) *mygo.MenuItem {
		return &mygo.MenuItem{Label: label, Accelerator: accelerator, Click: func(*mygo.MenuItem, *mygo.Window) {
			s.Navigate(path)
		}}
	}
	do := func(label, accelerator string, fn func()) *mygo.MenuItem {
		return &mygo.MenuItem{Label: label, Accelerator: accelerator, Click: func(*mygo.MenuItem, *mygo.Window) { fn() }}
	}

	appMenu := []*mygo.MenuItem{{Role: mygo.RoleAbout}}
	if checkForUpdates != nil {
		appMenu = append(appMenu, checkForUpdates)
	}
	appMenu = append(appMenu,
		mygo.Separator(),
		nav("Settings…", "CmdOrCtrl+,", routeSettings),
		mygo.Separator(),
		&mygo.MenuItem{Role: mygo.RoleHide},
		&mygo.MenuItem{Role: mygo.RoleHideOthers},
		&mygo.MenuItem{Role: mygo.RoleUnhide},
		mygo.Separator(),
		&mygo.MenuItem{Role: mygo.RoleQuit},
	)

	return mygo.NewMenu([]*mygo.MenuItem{
		{Label: "Shardlane", Submenu: appMenu},
		{Label: "Go", Submenu: []*mygo.MenuItem{
			do("Workspace", "CmdOrCtrl+1", s.ShowWorkspaceFromMenu),
			do("Changes", "", s.ShowChangesFromMenu),
			nav("Sessions", "", routeHistory),
			do("Files", "", s.ShowFilesFromMenu),
			nav("Hosts", "", routeHosts),
			mygo.Separator(),
			// ⌘W belongs to the tab model, not the window (F103): the stock
			// Window role bound it to closing the whole window in a
			// tab-centric app. The guarded confirm keeps the action family
			// single-sourced with the sidebar.
			do("Close Tab", "CmdOrCtrl+W", s.CloseTabFromMenu),
			nav("Set Up Agents", "", "/settings/providers"),
			do("Command Center", "CmdOrCtrl+Shift+P", s.OpenCommandCenterFromMenu),
		}},
		{Role: mygo.RoleEditMenu},
		// Custom Window menu (F103): Minimize/Zoom without the stock
		// "Close Window ⌘W" item so the accelerator stays tab-scoped.
		{Label: "Window", Submenu: []*mygo.MenuItem{
			{Role: mygo.RoleMinimize},
			{Role: mygo.RoleZoom},
		}},
	})
}
