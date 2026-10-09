// Package nativeui owns Shardlane's MyGo-native desktop presentation.
//
// [INPUT]: Herdr projections/actions and MyGo's native UI/Terminal toolkit.
// [OUTPUT]: one native Shardlane window with Sidebar, Header, Pane layout and terminals.
// [POS]: presentation adapter only; Herdr remains the sole runtime authority.
package nativeui

import (
	"context"
	"log/slog"
	"path/filepath"
	"runtime"
	"sync/atomic"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/agent"
	"github.com/wh-studio/herdr-client/internal/app"
	"github.com/wh-studio/herdr-client/internal/commandcenter"
	"github.com/wh-studio/herdr-client/internal/conversation"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"github.com/wh-studio/herdr-client/internal/history"
	"github.com/wh-studio/herdr-client/internal/scripts"
	"github.com/wh-studio/herdr-client/internal/services"
	"github.com/wh-studio/herdr-client/internal/settings"
)

const sidebarWidth float32 = 252

// Shell is the native presentation state for one Shardlane window. Long-lived
// runtime facts are never authored here: projection is a disposable Herdr
// snapshot and terminal surfaces are visibility-scoped attachments.
type Shell struct {
	win     *mygo.Window
	runtime *herdr.Manager
	router  *ui.Router

	searchQuery  string
	visibleRoute string

	instances      []herdr.Instance
	activeInstance string
	projection     herdr.Projection
	// machineStates caches the latest `herdr machine status` verdict per
	// saved profile ID; presentation only, refreshed in the background.
	machineStates        map[string]herdr.MachineState
	machineStatusAt      time.Time
	machineStatusRunning bool
	selectedProjectID    string
	selectedTabID        string
	selectedPaneID       string
	selected             string
	projectsOpen         bool
	agentsOpen           bool
	pinsOpen             bool
	// crumbOpen toggles the three breadcrumb switcher popovers
	// (project / tab / pane); at most one is open at a time.
	crumbOpen        [3]bool
	recentItems      []RecentItem
	expandedProjects map[string]bool
	expandedTabs     map[string]bool
	loading          bool
	// offline marks the last failure as Herdr-unreachable rather than a
	// failed presentation surface; it feeds shellState classification.
	offline bool
	status  string
	// statusShown deduplicates status toasts: every NEW status value the
	// app announces toasts once (F60 — the status strip only rendered on
	// the empty-terminal branch, dropping 38 write sites' feedback).
	statusShown string
	errText     string

	dialogOpen     bool
	dialogKind     string
	dialogTarget   string
	dialogTitle    string
	dialogLabel    string
	dialogValue    string
	confirmOpen    bool
	confirmKind    string
	confirmTarget  string
	confirmTitle   string
	confirmMessage string

	terminals map[string]*terminalSurface
	// terminalCanvasBounds is the canvas rect captured at render for
	// file-drop hit testing.
	terminalCanvasBounds ui.Rect
	// paneScrolls serializes pane.scroll mutations per visible pane (at most
	// one RPC in flight, newest absolute offset wins); paneScrollSink
	// replaces the Herdr mutation in tests.
	paneScrolls    map[string]*paneScrollDispatch
	paneScrollSink func(paneID string, offset int)
	// agentMouse serializes pane.send_text mouse writes for Agent TUI
	// panes (wheel, press and drag alike); agentMouseSink replaces the
	// send in tests.
	agentMouse     map[string]*agentMouseDispatch
	agentMouseSink func(paneID, text string)

	// settingsService owns persistence; settings is the presentation snapshot
	// refreshed after every successful update.
	settingsService   *app.SettingsService
	settings          settings.Settings
	settingsFontDraft string

	// hist owns the Native History presentation state over the read-only
	// HistoryService.
	hist historyState

	// integrations owns the Providers/Integrations health presentation state.
	integrations integrationUIState

	// launch drives the Agent launch transaction over the verified Herdr
	// transport (history continuation, live handoff, follow-up queue).
	launch *app.LaunchService

	// workbench owns the 0.5 Agent workbench presentation state.
	workbench workbenchState

	// interactionBroker owns pending structured interactions (0.6 §17).
	interactionBroker *conversation.InteractionBroker

	// tray owns the 0.7 §17-18 macOS menu-bar projection; nil when no tray
	// is bound (headless sessions, tests, platform fallback).
	tray *statusTray
	// dock controls the 0.9 P12 macOS Dock badge (NeedsAttention + ReviewPending).
	dock *dockController
	// quickPanel is the floating Agent activity panel window (0.7 §17.1);
	// nil means headless (tests, platform fallback) and the anchor clicks
	// stay no-ops.
	quickPanel *mygo.Window
	// quickPanelPlacement remembers the anchor + work area the panel was
	// last shown at, so a status-tab change re-fits the height in place.
	quickPanelPlacement quickPanelPlacement
	// quickPanelBlurHideAt records when the panel last hid because it lost
	// key status: the anchor click that steals that focus lands moments
	// later and must count as "close", not as a fresh open (§17.1 toggle).
	quickPanelBlurHideAt time.Time
	// quitting is set from the application quit path (OnBeforeQuit) so
	// close-preventing windows (the quick panel) yield to a real quit
	// instead of aborting it. Main-thread only, like the events that set
	// and read it.
	quitting bool
	// commandCenter owns the 0.9 P1 palette state; the index cache is
	// rebuilt from the snapshot on every open (zero IO). commandCenterReveal
	// is the row the keyboard moved to and the list must reveal (-1 none);
	// commandCenterQueryFocused mirrors the query field's focus for tests.
	commandCenterOpen         bool
	commandCenterScope        commandcenter.Scope
	commandCenterQuery        string
	commandCenterSelected     int
	commandCenterReveal       int
	commandCenterQueryFocused bool
	commandCenterIndexCache   *commandcenter.Index
	// rightPanel owns the Native Right Panel (Changes/Files/Services).
	rightPanel rightPanelState

	// sidebarCollapsed hides the sidebar (2026-10-06 A6): session-only
	// presentation state, toggled from the header's leading cluster.
	sidebarCollapsed bool
	// windowPinned mirrors the main window's always-on-top level for the
	// titlebar pin's selected face: session-only presentation state, never
	// persisted and only ever changed through toggleWindowPinned.
	windowPinned bool

	// scriptStore owns the user script definitions (persisted under
	// PathUserData/scripts.json in production).
	scriptStore *scripts.Store
	// scriptBusy marks running script launches by name.
	scriptBusy map[string]bool
	// portProbe owns the listening port observer.
	portProbe *services.PortProbe
	// portsState caches the observed ports snapshot for the Services view.
	observedPortsList []uint16
	portsRefreshedAt  time.Time
	// serviceIndex caches per-Pane service activity for the sidebar tree
	// (foreground processes crossed with the listener snapshot), refreshed
	// on the scan lane; render reads the map only.
	serviceIndex      sidebarServiceIndex
	servicesScannedAt time.Time
	serviceScanCancel context.CancelFunc
	// previews owns persistent preview window controllers (GWB-007).
	previews *previewManager
	// git owns the 0.10 Git Workbench orchestration (single cache).
	git *gitService
	// surface owns the WorkspacePrimarySurface state machine (plan §5/§6).
	surface workspaceSurfaceState
	// changes panel state.
	changes         changesPanelState
	changesList     ui.ListState
	changesSelected int
	changesTree     *gitworkbench.TreeNode
	// sidebarFiles is the left sidebar's Godiff Files state while the
	// Diff/Commit surface is up: Fork-style unstaged/staged split panes,
	// each with its own list state over the shared tree data. The choices
	// hold the rows picked with Cmd/Shift-click for the pane's Stage or
	// Unstage action, keyed by path.
	unstagedList     ui.ListState
	unstagedSelected int
	unstagedChoice   ui.Selection[string]
	stagedList       ui.ListState
	stagedSelected   int
	stagedChoice     ui.Selection[string]
	stagedPaneHeight float32
	unstagedOpen     bool
	stagedOpen       bool
	// commitsMode shows the All Commits pane instead of the split panes.
	commitsMode     bool
	commitsList     ui.ListState
	commitsSelected int
	// repo bar menus (diff-mode top bar).
	tagsMenuOpen      bool
	stashesMenuOpen   bool
	worktreesMenuOpen bool
	// branch menu state.
	branchMenuOpen bool
	branchDraft    string
	tagDraft       string
	// pendingToast is flushed as a toast on the next frame (background
	// lanes cannot toast).
	pendingToast string
	// terminalFind owns the Terminal-surface find bar (CopySearch adapter).
	terminalFind terminalFindState
	// userDataDir is the production persistence root ("" in headless tests).
	userDataDir string

	// usageByAgent caches the §17.6 History-meta usage projection per agent
	// (§17.2 secondary line); refreshed on the dispatch lane, read-only at
	// render.
	usageByAgent     map[agent.AgentKey]agent.AgentUsageSnapshot
	usageRefreshedAt time.Time

	// inspector owns the 0.8 read-only Agent inspector state.
	inspector inspectorState
	// historyProjects caches the 0.8 grouped management view's bounded
	// metadata; refreshed on the dispatch lane, read-only at render.
	historyProjects         []history.SessionSummary
	historyProjectsLoading  bool
	historyProjectsLoaded   bool
	historyProjectsInflight bool

	// diagnostics state (0.9 P16/P17)
	diagLogLevelFilter string
	diagSearchQuery    string
	diagExportStatus   string

	// chat owns the 0.6 Native Conversation surface state (bounded).
	chatConversationID string
	chatPaneID         string
	chatPhase          agent.AgentRuntimePhase
	chatLaunchPending  bool
	chatDisposition    conversation.PromptDisposition
	chatTurns          []conversation.TimelineTurn
	chatDraft          string
	chatQueuedText     string
	chatOutcome        string
	chatLastRequestID  string
	// Per-turn disclosure state keyed "index:part": one shared boolean made
	// every Thinking/Tools collapsible open and close together (F94).
	chatTurnExpanded map[string]bool
	chatLoading      bool
	chatGen          atomic.Uint64
	chatOpenCancel   context.CancelFunc
	// chatLive* own the bound live transcript tail (0.6 §7): the decoder
	// pump is owned by the binding and stops when the conversation switches.
	chatLiveCancel      context.CancelFunc
	chatLiveWake        chan struct{}
	chatLivePump        *history.LivePump
	chatLiveFingerprint string
	chatLivePath        string
	// chatLiveCommitted is the committed live projection length at the last
	// applied sync — the pending-echo baseline coordinate (0.6 §12).
	chatLiveCommitted int
	// chatPendingEcho is the unconsumed pending submission echo; nil once
	// the provider source confirmed it.
	chatPendingEcho *conversation.PendingEcho
	// chatLiveBackstopOverride shrinks the pump's timer backstop in tests;
	// zero keeps the production backstop.
	chatLiveBackstopOverride time.Duration

	watchCancel context.CancelFunc
	generation  atomic.Uint64
	// uiApplyOverride, when set (tests only), replaces win.Update for guarded
	// background applies so headless tests can observe the production apply
	// path. Production never sets it; nil keeps the drop-in-headless rule.
	uiApplyOverride func(fn func())
}

// ShellOption customizes one native shell window.
type ShellOption func(*Shell)

// WithSettings injects the application SettingsService. Without it the shell
// edits defaults through an in-memory store, which keeps headless UI tests
// self-contained.
func WithSettings(service *app.SettingsService) ShellOption {
	return func(s *Shell) {
		if service != nil {
			s.settingsService = service
			s.settings = service.Current()
			s.settingsFontDraft = s.settings.Terminal.FontFamily
		}
	}
}

// WithHistory injects the read-only HistoryService backing the Native
// History pages and the Sidebar Recent rows. Without it the pages render
// over an empty in-memory catalog (headless tests, degraded startup).
func WithHistory(service *history.HistoryService) ShellOption {
	return func(s *Shell) {
		if service != nil {
			s.hist.service = service
		}
	}
}

func NewShell(opts ...ShellOption) *Shell {
	s := &Shell{
		runtime:          herdr.NewManager(),
		router:           ui.NewRouter("/workspace"),
		visibleRoute:     routeWorkspace,
		projectsOpen:     true,
		agentsOpen:       true,
		pinsOpen:         true,
		expandedProjects: make(map[string]bool),
		expandedTabs:     make(map[string]bool),
		loading:          true,
		status:           "Loading Herdr workspaces…",
		statusShown:      "Loading Herdr workspaces…",
		terminals:        make(map[string]*terminalSurface),

		historyProjectsLoading: true,
		rightPanel:             defaultRightPanelState(),
	}
	// The agent activity panel opens on All: the bucket enum's zero value
	// is NeedsAttention, so the zero filter would land users on an empty
	// "No agents here" first screen (2026-10-06 F57).
	s.workbench.filterAll = true
	s.settingsService = app.NewSettingsService(settings.NewMemoryStore(settings.Default()))
	s.settings = s.settingsService.Current()
	s.settingsFontDraft = s.settings.Terminal.FontFamily
	s.integrations.service = agent.NewIntegrationHealthService()
	s.launch = app.NewLaunchService(s.runtime)
	s.interactionBroker = conversation.NewInteractionBroker(nil)
	s.portProbe = services.NewPortProbe()
	s.git = newGitService()
	s.previews = newPreviewManager()
	// Preview external-open failures surface as the shell's status toast.
	s.previews.report = func(msg string) { s.status = msg }
	s.scriptBusy = make(map[string]bool)
	s.changesSelected = -1
	s.changesList.Selected = &s.changesSelected
	s.unstagedSelected = -1
	s.unstagedList.Selected = &s.unstagedSelected
	s.stagedSelected = -1
	s.stagedList.Selected = &s.stagedSelected
	s.stagedPaneHeight = 220
	s.unstagedOpen, s.stagedOpen = true, true
	s.git.splitLayout = s.settings.Workbench.SplitDiff
	// Stores fall back to memory when no user-data dir is injected
	// (headless tests); production wires scripts.json through WithUserDataDir.
	s.scriptStore, _ = scripts.NewStore("")
	s.diagLogLevelFilter = "ALL"
	for _, opt := range opts {
		opt(s)
	}
	s.setHighlightStyle(s.settings.General.Appearance == "dark")
	s.restoreSession()
	return s
}

// WithUserDataDir injects the MyGo user-data directory for production
// persistence (scripts.json). Empty keeps in-memory stores.
func WithUserDataDir(dir string) ShellOption {
	return func(s *Shell) {
		if dir == "" {
			return
		}
		s.userDataDir = dir
		if store, err := scripts.NewStore(filepath.Join(dir, "scripts.json")); err == nil {
			s.scriptStore = store
		}
	}
}

// applySettings routes one user settings mutation through the application
// service and refreshes the presentation snapshot.
func (s *Shell) applySettings(mutate func(*settings.Settings) error) {
	updated, err := s.settingsService.Update(context.Background(), mutate)
	if err != nil {
		slog.Warn("apply settings update failed", "error", err)
		return
	}
	s.settings = updated
	s.settingsFontDraft = updated.Terminal.FontFamily
}

// Launch exposes the launch service for the desktop entrypoint's durable
// ledger attachment and tests.
func (s *Shell) Launch() *app.LaunchService { return s.launch }

func (s *Shell) AttachWindow(win *mygo.Window) {
	s.win = win
	slog.Debug("native shell attached", "window_id", win.ID())
	if s.launch != nil {
		// The follow-up delivery worker owns the queue's state machine for
		// the window's lifetime (0.6 §11.3).
		s.launch.StartFollowUpWorker()
	}
	// File drops paste into the exact visible terminal under the pointer
	// (GWB-008/009); drops while Diff/Commit is visible never fall through.
	win.OnFileDrop(func(e *mygo.FileDropEvent) {
		if err := s.HandleFileDropAt(e.X, e.Y, e.Paths); err != nil {
			slog.Debug("file drop rejected", "error", err)
		}
	})
	s.reloadInstances(true)
	s.startHistoryScan()
	s.startServiceScan()
}

// toggleSidebar flips the sidebar's collapsed state (2026-10-06 A6).
func (s *Shell) toggleSidebar() {
	s.sidebarCollapsed = !s.sidebarCollapsed
}

// Navigate routes one menu-bar command to the router on the window's UI
// lane (the App menu lives outside the render path). Push dedupes a repeat
// of the current path, so a menu accelerator and an in-app shortcut firing
// together stay harmless.
func (s *Shell) Navigate(path string) {
	s.applyOnWindow(func() { s.router.Push(path) })
}

// ToggleRightPanelFromMenu is the menu-bar seam for the panel toggle: the
// ⌥⌘B binding stays in-app only, so the menu item carries no accelerator
// that could double-fire with it.
func (s *Shell) ToggleRightPanelFromMenu() {
	s.applyOnWindow(func() { s.toggleRightPanel() })
}

// OpenCommandCenterFromMenu opens the palette from the menu bar; opening an
// already-open palette is a no-op re-render, never a toggle.
func (s *Shell) OpenCommandCenterFromMenu() {
	s.applyOnWindow(func() { s.openCommandCenter(commandcenter.ScopeAll) })
}

// MarkQuitting records that the application quit has started (fired from
// OnBeforeQuit before any window closes): close-preventing windows check
// it to yield to the quit.
func (s *Shell) MarkQuitting() {
	s.quitting = true
}

func (s *Shell) Close() {
	slog.Info("native shell closing", "instance", s.activeInstance)
	// Persist the session while the state is still intact: this is the menu
	// Quit path (the close button hides the window without closing it).
	s.SaveSession()
	if s.watchCancel != nil {
		s.watchCancel()
		s.watchCancel = nil
	}
	if s.serviceScanCancel != nil {
		s.serviceScanCancel()
		s.serviceScanCancel = nil
	}
	if s.launch != nil {
		s.launch.StopFollowUpWorker()
	}
	s.stopLiveTail()
	s.stopHistory()
	s.closeAllTerminals()
	// Kill the SSH socket bridges of saved-machine instances: they are
	// process-global children that would otherwise outlive the app.
	herdr.CloseBridges()
}
func (s *Shell) View(c *ui.Context) {
	theme := shardlaneTheme(s.resolvedDark(c.Theme().Dark))
	c.SetTheme(theme)
	if runtime.GOOS == "darwin" {
		c.Root().Background(ui.Transparent)
	} else {
		c.Root().Background(theme.Background)
	}
	s.handleMRUShortcuts(c)
	s.handleRouteShortcuts(c)
	s.handleCommandCenterShortcuts(c)
	s.handleSurfaceShortcuts(c)
	if s.pendingToast != "" {
		c.Toast(s.pendingToast)
		s.pendingToast = ""
	}
	// Loading text never toasts (statusShown seeds it), and repeats of the
	// same value stay silent; a changed status announces exactly once.
	if !s.loading && s.status != "" && s.status != s.statusShown {
		c.Toast(s.status)
		s.statusShown = s.status
	}
	if c.Shortcut(ui.Super|ui.Alt, ui.KeyB) {
		s.toggleRightPanel()
	}
	if c.Shortcut(ui.Super, ui.KeyB) {
		s.toggleSidebar()
	}
	s.syncRouteVisibility()

	// The gorex background gradient runs under the whole window: the
	// title bar, the sidebar and the workspace's terminal cards all sit
	// on it directly — one continuous surface, no fills of their own
	// under the chrome (2026-10-06 revised plan).
	ui.Column(c).Fill().Draw(func(p *ui.Painter, r ui.Rect) {
		paintGorexBackground(p, r, gorexColorsOf(theme.Dark))
	}).Children(func() {
		s.titlebar(c)
		ui.Row(c).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(func() {
			s.sidebarSlide(c)
			s.routeView(c)
			s.rightPanelSlide(c)
		})
	})
	s.dialogs(c)
	s.agentSwitcherOverlay(c)
	s.commandCenterOverlay(c)
}
