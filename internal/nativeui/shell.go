// Package nativeui owns Shardlane's MyGo-native desktop presentation.
//
// [INPUT]: Herdr projections/actions and MyGo's native UI/Terminal toolkit.
// [OUTPUT]: one native Shardlane window with Sidebar, Header, Pane layout and terminals.
// [POS]: presentation adapter only; Herdr remains the sole runtime authority.
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
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

const (
	sidebarWidth float32 = 252
)

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
	// Bootstrap screen hides the entire workspace chrome until the first
	// Herdr projection is reconciled (ordinary mutations never hide it).
	bootstrapPending bool
	bootstrapFailed  bool
	// offline marks the last failure as Herdr-unreachable rather than a
	// failed presentation surface; it feeds shellState classification.
	offline bool
	status  string
	// statusShown deduplicates status toasts: every NEW status value the
	// app announces toasts once (F60 — the status strip only rendered on
	// the empty-terminal branch, dropping 38 write sites' feedback).
	statusShown string
	errText     string

	dialogOpen      bool
	dialogKind      string
	dialogTarget    string
	dialogTitle     string
	dialogLabel     string
	dialogValue     string
	confirmOpen     bool
	confirmKind     string
	confirmTarget   string
	confirmTitle    string
	confirmMessage  string
	confirmRepoRoot string // binds any pending Git confirmation to its original repository
	// Commit shares the existing verified Workspace Commit state; the
	// dialog is presentation-only and never creates a second Git draft.
	commitDialogOpen           bool
	commitDiscardPrompt        bool
	commitPromptPreviewOpen    bool
	commitPromptIncludeExcerpt bool
	commitPromptTextVisible    bool
	commitAIManualEntryOpen    bool
	commitAIResponse           string
	commitAIError              string
	commitAIReplaceConfirm     bool
	piQuickRunning             bool
	piQuickError               string
	piQuickGen                 uint64
	piQuickCancel              context.CancelFunc
	piCommitRunning            bool
	piCommitError              string
	piCommitCancel             context.CancelFunc
	piCommitGeneration         uint64
	piCommitExecutor           func(context.Context, agent.PiCommitRequest) (agent.PiCommitResponse, error)
	mergeDialogOpen            bool
	mergeTarget                string
	mergeRepoRoot              string
	mergeStrategy              int // 0: FF-only, 1: create merge commit

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
	settingsService        *app.SettingsService
	settings               settings.Settings
	settingsFontDraft      string
	settingsGitPromptDraft string
	settingsPiBinaryDraft  string

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
	// trayHandle is the bound mygo.Tray itself: the quick panel flyout
	// anchors to it (Tray option), not to a screen rectangle.
	trayHandle *mygo.Tray
	// dock controls the 0.9 P12 macOS Dock badge (NeedsAttention + ReviewPending).
	dock *dockController
	// quickPanel is the open quick-panel flyout (0.7 §17.1); nil means no
	// panel is open — the flyout is created on demand, and headless
	// sessions (tests, platform fallback) keep the anchor clicks no-ops.
	quickPanel *mygo.Window
	// quickPanelAgent selects one exact Agent for the shared quick panel;
	// nil means the tray/titlebar's full activity overview.
	quickPanelAgent *agent.AgentKey
	// 0: Agent activity, 1: workspace Services; one anchored flyout.
	quickPanelTab int
	// quickPanelDismissedAt records when the panel last closed: on macOS
	// the outside press that dismissed it goes on to the element under it,
	// so the anchor click that lands moments later must count as "close",
	// not as a fresh open (§17.1 toggle).
	quickPanelDismissedAt time.Time
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
	portsForRoot      string
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
	// The Changes center is split between commits/files and the existing Diff viewer.
	gitUpperHeight       float32
	gitFilePaneWidth     float32
	gitLowerTab          int
	gitQuickSubject      string
	gitQuickBody         string
	gitQuickAmend        bool
	gitCommitFilesFilter string
	// The Git reference tree is a presentation projection, scoped to the bound repo.
	gitTreeClosed   map[string]bool
	gitRefsFilter   string
	gitCommitFilter string
	unstagedOpen    bool
	stagedOpen      bool
	// commitsMode shows the All Commits pane instead of the split panes.
	gitConflictFilesExpanded bool
	commitsMode              bool
	commitsList              ui.ListState
	commitsSelected          int
	// repo bar menus (diff-mode top bar).
	tagsMenuOpen      bool
	stashesMenuOpen   bool
	worktreesMenuOpen bool
	// branch menu state.
	branchMenuOpen bool
	branchDraft    string
	branchFilter   string
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

	// chat owns the 0.6 Native Conversation surface state (bounded). The
	// composer disposition is derived live from the projection (0.6 §10):
	// chatPhase/chatLaunchPending/chatDisposition were stored fields that
	// nothing ever assigned, which left the native Send permanently
	// disabled until 2026-10-11.
	chatConversationID string
	chatPaneID         string
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
	// chatWebview is the chat page's embedded WebView (N19 Phase 2); nil
	// until the first chat build with General.WebViewChat on, and
	// chatWebviewBroken latches a failed creation so the native chat stays.
	chatWebview       *mygo.WebView
	chatWebviewBroken bool
	// chatLiveCommitted is the committed live projection length at the last	// applied sync — the pending-echo baseline coordinate (0.6 §12).
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
			// The saved appearance applies at AttachWindow (SetSource waits
			// on the main loop, which does not exist before the window).
			s.settingsGitPromptDraft = commitPromptPreference(s.settings.Workbench.CommitPrompt)
			s.settingsPiBinaryDraft = s.settings.Workbench.PiExecutable
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
	s.settingsGitPromptDraft = commitPromptPreference(s.settings.Workbench.CommitPrompt)
	s.settingsPiBinaryDraft = s.settings.Workbench.PiExecutable
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
	s.gitUpperHeight = gitSplitDefaultTop
	s.gitFilePaneWidth = gitFilePaneDefaultWidth
	s.gitLowerTab = 1
	s.gitTreeClosed = make(map[string]bool)
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
	if s.win != nil {
		// SetSource marshals to the main thread and waits; headless tests
		// have no main loop, so only apply with a window attached.
		s.syncThemeSource()
	}
}

// syncThemeSource applies Settings -> General -> Appearance to the
// framework's theme (the setting's own promise: "applies to the native
// shell chrome"), which the whole app — native chrome and the chat
// WebView — follows through mygo.Theme.
func (s *Shell) syncThemeSource() {
	var source mygo.ThemeSource
	switch s.settings.General.Appearance {
	case "light":
		source = mygo.ThemeLight
	case "dark":
		source = mygo.ThemeDark
	default:
		source = mygo.ThemeSystem
	}
	mygo.Theme.SetSource(source)
}

// Launch exposes the launch service for the desktop entrypoint's durable
// ledger attachment and tests.
func (s *Shell) Launch() *app.LaunchService { return s.launch }

func (s *Shell) AttachWindow(win *mygo.Window) {
	s.win = win
	s.bootstrapPending = true
	s.bootstrapFailed = false
	slog.Debug("native shell attached", "window_id", win.ID())
	// Appearance changes (system or Settings) re-publish the chat
	// projection, whose theme field carries the resolved appearance to the
	// WebView chat page.
	s.syncThemeSource()
	mygo.Theme.OnUpdated(func() {
		s.applyOnWindow(func() { s.publishChatBridge() })
	})
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
	s.applyOnWindow(func() {
		if s.holdCommitNavigation() {
			return
		}
		s.router.Push(path)
	})
}

// ToggleRightPanelFromMenu is the menu-bar seam for the panel toggle: the
// ⌥⌘B binding stays in-app only, so the menu item carries no accelerator
// that could double-fire with it.
func (s *Shell) ToggleRightPanelFromMenu() {
	// Legacy menu action opens the standalone Files workbench instead.
	s.applyOnWindow(func() { s.showViewFiles() })
}

// OpenCommandCenterFromMenu opens the palette from the menu bar; opening an
// already-open palette is a no-op re-render, never a toggle.
func (s *Shell) OpenCommandCenterFromMenu() {
	s.applyOnWindow(func() { s.openCommandCenter(commandcenter.ScopeAll) })
}

func (s *Shell) Close() {
	slog.Info("native shell closing", "instance", s.activeInstance)
	// Persist the session while the state is still intact: this is the menu
	// Quit path (the close button hides the window without closing it).
	s.SaveSession()
	s.cancelQuickAICommit()
	s.cancelPiCommitSuggestion()
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
	if !s.bootstrapPending && !s.bootstrapFailed {
		s.handleMRUShortcuts(c)
		s.handleRouteShortcuts(c)
		s.handleCommandCenterShortcuts(c)
		s.handleSurfaceShortcuts(c)
	}
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
	// During startup the workspace and all navigation remain unmounted.
	// This also prevents invisible keyboard shortcuts from mutating state.
	if !s.bootstrapPending && !s.bootstrapFailed {
		if c.Shortcut(ui.Super, ui.KeyB) {
			s.toggleSidebar()
		}
		s.syncRouteVisibility()
	}

	// The gorex background gradient runs under the whole window: the
	// title bar, the sidebar and the workspace's terminal cards all sit
	// on it directly — one continuous surface, no fills of their own
	// under the chrome (2026-10-06 revised plan).
	ui.Column(c).Fill().Draw(func(p *ui.Painter, r ui.Rect) {
		paintGorexBackground(p, r, gorexColorsOf(theme.Dark))
	}).Children(func() {
		// Prime the persistent container's entrance animation at zero
		// without fading the loader itself. The first ready frame eases in
		// only the real workspace chrome, respecting Reduce Motion.
		content := ui.Column(c).Fill()
		if s.bootstrapPending || s.bootstrapFailed {
			_ = content.Animate("workspace-enter", 0, 260*time.Millisecond)
			content.Children(func() { s.bootstrapPage(c) })
		} else {
			content.Opacity(content.Animate("workspace-enter", 1, 260*time.Millisecond))
			content.Children(func() {
				s.titlebar(c)
				ui.Row(c).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(func() {
					s.sidebarSlide(c)
					s.routeView(c)
				})
			})
		}
	})
	if !s.bootstrapPending && !s.bootstrapFailed {
		s.dialogs(c)
		s.agentSwitcherOverlay(c)
		s.commandCenterOverlay(c)
	}
}
