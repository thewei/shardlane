// Shardlane (next) — MyGo Native UI application entrypoint.
//
// main is deliberately thin: it wires process-level services (bounded
// logs, persisted settings, the read-only history catalog, the durable
// follow-up ledger) into nativeui.NewShell, shows the single main window,
// and binds the macOS tray and Dock surfaces. All presentation lives in
// internal/nativeui; Herdr stays the runtime authority.
package main

import (
	"log/slog"
	"os"
	"path/filepath"
	"runtime"
	"sync"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/plugins/updater"
	updaterNative "github.com/egoist/mygo/plugins/updater/native"
	"github.com/egoist/mygo/ui"

	"github.com/wh-studio/herdr-client/next/internal/app"
	"github.com/wh-studio/herdr-client/next/internal/applog"
	"github.com/wh-studio/herdr-client/next/internal/history"
	"github.com/wh-studio/herdr-client/next/internal/nativeui"
	"github.com/wh-studio/herdr-client/next/internal/settings"
)

// historyCatalogFileName is the Shardlane-owned disposable FTS index over
// read-only provider history, inside the MyGo user-data directory.
const historyCatalogFileName = "history.sqlite3"

// followUpLedgerFileName mirrors app.LaunchService's durable ledger name.
const followUpLedgerFileName = "follow-up-ledger.json"

func main() {
	if !mygo.App.RequestSingleInstanceLock() {
		return
	}

	// The updater window is drawn in native UI (no WebView, matching the
	// frozen WebView presentation rule). Without an `updates` block in
	// mygo.json the window truthfully reports that this build cannot check;
	// it becomes live the moment signed releases are published (Gate E5).
	mygo.Use(updaterNative.Plugin)

	userDataDir, err := mygo.App.Path(mygo.PathUserData)
	if err != nil {
		slog.Error("resolve user data dir", "error", err)
		os.Exit(1)
	}
	logsDir := mustLogsDir()
	if _, err := applog.Setup(logsDir); err != nil {
		slog.Error("setup application logging", "error", err)
	} else if stop, err := applog.CaptureStderr(logsDir); err != nil {
		slog.Warn("capture stderr failed; runtime panics stay invisible", "error", err)
	} else {
		defer stop()
	}

	// Settings persist as settings.json in the user-data directory; without
	// the file the store falls back to defaults on first launch.
	settingsService := app.NewSettingsService(settings.NewFileStore(userDataDir))

	// The history catalog is disposable: a failed open degrades to the
	// in-memory empty catalog instead of blocking startup.
	historyOpts := []nativeui.ShellOption{}
	if catalog, err := history.OpenCatalog(filepath.Join(userDataDir, historyCatalogFileName)); err != nil {
		slog.Warn("open history catalog failed; degraded catalog", "error", err)
	} else {
		home, _ := os.UserHomeDir()
		historyService := history.NewHistoryService(catalog, history.DefaultRoots(home))
		historyOpts = append(historyOpts, nativeui.WithHistory(historyService))
	}

	shell := nativeui.NewShell(append([]nativeui.ShellOption{
		nativeui.WithSettings(settingsService),
		nativeui.WithUserDataDir(userDataDir),
	}, historyOpts...)...)
	shell.Launch().AttachLedgerPath(filepath.Join(userDataDir, followUpLedgerFileName))

	// Notification clicks route back to the Agent's owning pane, including
	// clicks on notifications left over from a previous run (MyGo hands
	// back the stable notification ID across launches).
	mygo.App.OnNotificationClick(shell.RouteAgentNotification)

	var closeOnce sync.Once
	closeShell := func() { closeOnce.Do(shell.Close) }

	// The close button only hides the window (session stays alive); a real
	// quit comes from the app menu (RoleQuit → App.Quit), which fires
	// OnBeforeQuit before closing windows. The flag lets OnClose tell the
	// two apart. MarkQuitting also reaches the close-preventing windows:
	// mygo aborts the whole quit when any window prevents its close, so a
	// prevented quick-panel close would strand a windowless process on the
	// single-instance lock (2026-10-07 user report).
	quitting := false
	mygo.App.OnBeforeQuit(func(e *mygo.QuitEvent) {
		quitting = true
		shell.MarkQuitting()
	})

	mygo.App.WhenReady(func() {
		slog.Info("application ready", "log_path", applog.Path())
		win := mygo.NewWindow(mygo.WindowOptions{
			Title:  "Shardlane",
			Width:  1280,
			Height: 820,
			// Sidebar (250) + a usable content column + a closed right
			// panel: below this the titlebar tools and diff gutters squeeze
			// into unreadability (2026-10-06 F41).
			MinWidth:  720,
			MinHeight: 420,
			// Restore the last bounds (position/size/maximized) unless the
			// user disabled restoration in Settings → General.
			StateKey: app.WindowStateKey(settingsService.Current().General.RestoreWindow),
			// The gorex-style header (2026-10-06) draws itself under the
			// traffic lights: hidden title bar, lights at the header chip's
			// height, and a base color matching the background gradient's mid
			// stop so the first frame does not flash white.
			TitleBarStyle:        mygo.TitleBarHidden,
			TrafficLightPosition: &mygo.Point{X: 16, Y: 15},
			BackgroundColor:      "light-dark(#efe1e6, #231e27)",
			// SingleInstanceLock focus-on-second-launch only has a window to
			// focus once this handle exists; keep it in the ready closure.
			Content: ui.View(shell.View),
		})
		shell.AttachWindow(win)
		win.OnClose(func(e *mygo.CloseEvent) {
			if !quitting {
				shell.SaveSession()
				e.PreventDefault()
				win.Hide()
				return
			}
			closeShell()
		})
		// Re-open surfaces: Dock-icon clicks and a second `open` of the
		// single-instance lock both unhide the main window.
		showMain := func() {
			if !win.IsVisible() {
				win.Show()
			}
			win.Focus()
		}
		mygo.App.OnActivate(func(hasVisibleWindows bool) { showMain() })
		mygo.App.OnSecondInstance(func(args []string, workingDir string) { showMain() })

		// macOS status tray (0.7 §17-18) and Dock badge (0.9 P12). Either
		// binding may fail outside packaged macOS runs; the tray click is
		// then simply absent, and the titlebar agent button stays the
		// in-window anchor for the floating panel.
		if tray, err := mygo.NewTray(mygo.TrayOptions{Title: "Shardlane"}); err != nil {
			slog.Warn("attach status tray failed", "error", err)
		} else {
			shell.AttachTray(tray)
		}
		shell.AttachDock(mygo.App.Dock)

		// Floating Agent activity panel (0.7 §17.1): the shared surface
		// behind the tray click and the titlebar's agent button. Created
		// hidden and frameless; toggles anchor beneath their trigger.
		if panel := mygo.NewWindow(mygo.WindowOptions{
			Width:         nativeui.QuickPanelWidth,
			Height:        nativeui.QuickPanelHeight,
			Hidden:        true,
			Frameless:     true,
			AlwaysOnTop:   true,
			DisableResize: true,
			Content:       ui.View(shell.QuickPanelView),
		}); panel != nil {
			shell.AttachQuickPanel(panel)
		}

		// macOS app menu with a Check for Updates item (truthful status via
		// the updater plugin), plus a Go menu so the core commands are
		// discoverable from the menu bar — accelerators only on idempotent
		// route pushes (Router.Push dedupes), never on the panel toggle.
		if runtime.GOOS == "darwin" {
			nav := func(label, accelerator, path string) *mygo.MenuItem {
				return &mygo.MenuItem{Label: label, Accelerator: accelerator, Click: func(*mygo.MenuItem, *mygo.Window) {
					shell.Navigate(path)
				}}
			}
			mygo.App.SetMenu(mygo.NewMenu([]*mygo.MenuItem{
				{Label: "Shardlane", Submenu: []*mygo.MenuItem{
					{Role: mygo.RoleAbout},
					updater.MenuItem(),
					mygo.Separator(),
					{Role: mygo.RoleQuit},
				}},
				{Label: "Go", Submenu: []*mygo.MenuItem{
					nav("Workspace", "CmdOrCtrl+1", "/workspace"),
					nav("History", "", "/history"),
					nav("History by Project", "", "/history-projects"),
					mygo.Separator(),
					// ⌘W belongs to the tab model, not the window (F103):
					// the stock Window role bound it to closing the whole
					// window in a tab-centric app. The guarded confirm keeps
					// the action family single-sourced with the sidebar.
					{Label: "Close Tab", Accelerator: "CmdOrCtrl+W", Click: func(*mygo.MenuItem, *mygo.Window) {
						shell.CloseTabFromMenu()
					}},
					{Label: "Toggle Right Panel", Click: func(*mygo.MenuItem, *mygo.Window) {
						shell.ToggleRightPanelFromMenu()
					}},
					{Label: "Command Center", Accelerator: "CmdOrCtrl+Shift+P", Click: func(*mygo.MenuItem, *mygo.Window) {
						shell.OpenCommandCenterFromMenu()
					}},
					nav("Settings", "CmdOrCtrl+,", "/settings/general"),
				}},
				{Role: mygo.RoleEditMenu},
				// Custom Window menu (F103): Minimize/Zoom without the stock
				// "Close Window ⌘W" item so the accelerator stays tab-scoped.
				{Label: "Window", Submenu: []*mygo.MenuItem{
					{Role: mygo.RoleMinimize},
					{Role: mygo.RoleZoom},
				}},
			}))
		}

		slog.Info("main window shown", "bounds", win.Bounds())
	})

	if err := mygo.App.Run(); err != nil {
		slog.Error("application run", "error", err)
	}
	closeShell()
	if err := applog.Close(); err != nil {
		slog.Warn("close application log", "error", err)
	}
}

// mustLogsDir resolves the platform log directory (~/Library/Logs/Shardlane
// on macOS). Unrecoverable failures fall back next to the user data dir.
func mustLogsDir() string {
	dir, err := mygo.App.Path(mygo.PathLogs)
	if err == nil && dir != "" {
		return dir
	}
	slog.Warn("resolve log dir failed", "error", err)
	userData, err2 := mygo.App.Path(mygo.PathUserData)
	if err2 != nil {
		return os.TempDir()
	}
	return filepath.Join(userData, "logs")
}
