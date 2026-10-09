package nativeui

import (
	"sync"
	"sync/atomic"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/filesview"
	"github.com/wh-studio/herdr-client/internal/platform"
	"github.com/wh-studio/herdr-client/internal/preview"
)

/**
 * [INPUT]: 依赖 git/files/services 工具只读快照、当前 Router 页面与 context_panel 的页面归属策略
 * [OUTPUT]: 提供右侧浮动卡片渲染、Workspace 工具切换与保留状态
 * [POS]: nativeui 的共享右侧卡片 chrome；按页面渲染 Workspace 工具或 History 上下文，避免交叉泄漏
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// RightPanelSurface defines the active tool in the Native Right Panel.
// 0.10 target surfaces: Changes / Files / Services (plan §10.1). Lazygit is
// removed from the product.
type RightPanelSurface string

const (
	SurfaceChanges  RightPanelSurface = "changes"
	SurfaceFiles    RightPanelSurface = "files"
	SurfaceServices RightPanelSurface = "services"
)

// RightPanelWidth bounds (WIX-046).
const (
	DefaultRightPanelWidth = 320
	MinRightPanelWidth     = 240
	MaxRightPanelWidth     = 500
)

// rightPanelState holds presentation state for the Project tool area.
type rightPanelState struct {
	// open is Workspace-tool visibility; History remembers its own inspector
	// visibility so toggling it cannot surprise a later Terminal visit.
	open        bool
	historyOpen bool
	surface     RightPanelSurface
	width       int
	// lastToolByTab remembers the per-Tab last tool choice (GWB-072).
	lastToolByTab map[string]RightPanelSurface

	// Files tool state: directories render from background snapshots only
	// (GWB-003); the render path never performs filesystem IO.
	expandedDirs     map[string]bool
	cachedDirEntries map[string][]filesview.Entry
	loadingDirs      map[string]bool
	currentRoot      string
	selectedFilePath string
	preview          *filesview.Preview
	previewOpen      bool
	filesGen         atomic.Uint64
}

func defaultRightPanelState() rightPanelState {
	return rightPanelState{
		open:             false,
		surface:          SurfaceChanges,
		width:            DefaultRightPanelWidth,
		lastToolByTab:    make(map[string]RightPanelSurface),
		expandedDirs:     make(map[string]bool),
		cachedDirEntries: make(map[string][]filesview.Entry),
		loadingDirs:      make(map[string]bool),
	}
}

// toggleRightPanel toggles panel visibility. The center surface is never
// changed by panel open/close (GWB-074 regression contract).
func (s *Shell) contextPanelOpen() bool {
	switch s.activeContextPanel() {
	case contextPanelWorkspace:
		return s.rightPanel.open
	case contextPanelHistory:
		return s.rightPanel.historyOpen
	default:
		return false
	}
}

// toggleRightPanelForViewport is the user-action entry point. At narrow
// widths an explicit request to show the Inspector makes room by collapsing
// the left navigation when that is sufficient. Otherwise it explains the
// constraint instead of silently toggling an invisible saved preference.
func (s *Shell) toggleRightPanelForViewport(windowWidth float32) {
	if s.activeContextPanel() == contextPanelNone {
		return
	}
	if !s.contextPanelVisible(windowWidth) {
		if !s.contextPanelFits(windowWidth) {
			if s.sidebarCollapsed || !contextPanelFits(windowWidth, false, s.rightPanel.width) {
				s.pendingToast = "More space is needed for this panel. Widen the window to open it."
				return
			}
			s.sidebarCollapsed = true
		}
		if !s.contextPanelOpen() {
			s.toggleRightPanel()
		}
		return
	}
	s.toggleRightPanel()
}

func (s *Shell) toggleRightPanel() {
	switch s.activeContextPanel() {
	case contextPanelWorkspace:
		s.rightPanel.open = !s.rightPanel.open
		if s.rightPanel.open {
			s.syncRightPanelRoot()
		}
	case contextPanelHistory:
		s.rightPanel.historyOpen = !s.rightPanel.historyOpen
	}
}

// openRightPanelSurfaceForViewport is the Command Center's explicit tool
// destination. It uses the same width policy as the titlebar/shortcut, and
// navigates to Workspace before opening its own contextual tool. Invalid
// widths keep the user's current page/tool/visibility intact.
func (s *Shell) openRightPanelSurfaceForViewport(surface RightPanelSurface, windowWidth float32) {
	if !contextPanelFits(windowWidth, false, s.rightPanel.width) {
		s.pendingToast = "More space is needed for this panel. Widen the window to open it."
		return
	}
	if s.router.Path() != routeWorkspace {
		s.showViewTerminal()
	}
	if !s.contextPanelFits(windowWidth) {
		s.sidebarCollapsed = true
	}
	s.openRightPanelSurface(surface)
}

// openRightPanelSurface switches to a surface, remembers the per-Tab choice
// and ensures visibility. Internal tool tabs call it only while visible.
func (s *Shell) openRightPanelSurface(surface RightPanelSurface) {
	s.rightPanel.surface = surface
	s.rightPanel.open = true
	if tab := s.selectedTabID; tab != "" {
		s.rightPanel.lastToolByTab[tab] = surface
	}
	s.syncRightPanelRoot()
}

// syncRightPanelRoot resets tool state when the active Tab root changes.
func (s *Shell) syncRightPanelRoot() {
	root := s.selectedTabCWD()
	if root != s.rightPanel.currentRoot {
		s.rightPanel.currentRoot = root
		s.rightPanel.expandedDirs = make(map[string]bool)
		s.rightPanel.cachedDirEntries = make(map[string][]filesview.Entry)
		s.rightPanel.loadingDirs = make(map[string]bool)
		s.rightPanel.preview = nil
		s.rightPanel.previewOpen = false
		// Restore this Tab's last tool; default Changes when a Git
		// repository with changes exists, else Files (plan §10.2).
		if tab := s.selectedTabID; tab != "" {
			if last, ok := s.rightPanel.lastToolByTab[tab]; ok {
				s.rightPanel.surface = last
			} else if s.git != nil && s.git.root != "" && s.dirtyWorktree() {
				s.rightPanel.surface = SurfaceChanges
			} else {
				s.rightPanel.surface = SurfaceFiles
			}
		}
	}
}

// rightPanelContent builds the complete Native Right Panel body
// (WIX-042/043). Visibility and the slide animation live in the host
// (rightPanelSlide): this builds at the panel's full open width and is
// clipped while the host slides.
func (s *Shell) rightPanelContent(c *ui.Context) {
	t := c.Theme()
	sp := Spacing()

	if s.activeContextPanel() == contextPanelWorkspace {
		s.syncRightPanelRoot()
	}

	// AlignItems(Stretch) is load-bearing: without it the panel column keeps
	// its intrinsic height and centers vertically in the shell row, floating
	// the surface switcher mid-panel and collapsing every tool view to zero
	// height. The panel rides in a gorex card on the gradient (2026-10-06):
	// no fill of its own, the window gutter stays a full gorexGap. The left
	// padding carries that same gorexGap (2026-10-06 A1 spacing report):
	// with 0 the panel card sat flush against the terminal cards' 8pt inset
	// and the two surfaces read as one merged column.
	ui.Row(c).Width(float32(s.rightPanel.width)).MinHeight(0).Grow(0).Shrink(0).
		AlignItems(ui.Stretch).Padding(gorexGap, gorexGap, gorexGap, gorexGap).Children(func() {
		k := gorexColorsOf(t.Dark)
		card := ui.Column(c).Grow(1).MinWidth(0).Radius(gorexCardR).Clip().
			Background(k.cardFocused).Border(1, k.cardBorderFocused).
			Shadow(0, 1, 2, 0, k.shadowFocused).
			Shadow(0, 6, 22, -2, k.shadowFocused)
		card.Transition(ui.ElementTransition{Colors: true, Duration: 160 * time.Millisecond})
		card.Children(func() {
			if s.activeContextPanel() == contextPanelHistory {
				s.historyContextPanel(c)
				return
			}
			ui.Column(c).FillWidth().Grow(1).MinHeight(0).Children(func() {
				// Header & Surface Switcher (2026-10-06 A4/A5): no close
				// button — the panel toggles from the header and ⌥⌘B — and
				// the switcher runs the full card width.
				ui.Row(c).FillWidth().Padding(sp.S, sp.M).AlignItems(ui.Center).
					BorderWidth(0, 0, 1, 0).BorderColor(designTokens(t.Dark).BorderSubtle).Children(func() {
					surfaces := []string{"Changes", "Files", "Services"}
					if s.sidebarInDiffMode() {
						surfaces[0] = "Review"
					}
					currentIdx := 0
					switch s.rightPanel.surface {
					case SurfaceFiles:
						currentIdx = 1
					case SurfaceServices:
						currentIdx = 2
					}

					segmented := ui.Segmented(c, &currentIdx, surfaces...)
					segmented.FillWidth()
					if segmented.Changed() {
						switch currentIdx {
						case 0:
							s.openRightPanelSurface(SurfaceChanges)
						case 1:
							s.openRightPanelSurface(SurfaceFiles)
						case 2:
							s.openRightPanelSurface(SurfaceServices)
						}
					}
				})

				// Content area based on surface
				ui.Column(c).Grow(1).MinHeight(0).Children(func() {
					switch s.rightPanel.surface {
					case SurfaceChanges:
						if s.sidebarInDiffMode() {
							s.gitReviewInspector(c)
						} else {
							s.changesToolView(c)
						}
					case SurfaceFiles:
						s.filesToolView(c)
					case SurfaceServices:
						s.servicesToolView(c)
					}
				})
			})
		})
	})

	if s.activeContextPanel() == contextPanelWorkspace && s.rightPanel.previewOpen && s.rightPanel.preview != nil {
		s.filePreviewModal(c)
	}
}

// previewManager owns persistent preview window controllers keyed by target
// (GWB-007): one controller per URL key, reused and focused, never leaked
// per click. The loopback-only boundary stays in the preview package.
type previewManager struct {
	mu          sync.Mutex
	controllers map[string]*preview.WindowController
	// report surfaces external-open failures in the shell's bounded
	// status toast; set by the Shell after construction.
	report func(string)
}

func newPreviewManager() *previewManager {
	return &previewManager{controllers: map[string]*preview.WindowController{}}
}

// openLocalPreviewURL reuses or creates the persistent controller for the
// target and focuses its window.
func (s *Shell) openLocalPreviewURL(targetURL string) {
	controller := s.previews.controllerFor(targetURL)
	if _, err := controller.OpenPreviewWindow(targetURL); err != nil {
		s.status = "Preview window unavailable"
		return
	}
	s.status = "Opened preview: " + targetURL
}

func (m *previewManager) controllerFor(targetURL string) *preview.WindowController {
	m.mu.Lock()
	defer m.mu.Unlock()
	if c := m.controllers[targetURL]; c != nil {
		return c
	}
	c := preview.NewWindowController(preview.WithOpenExternal(func(url string) error {
		if err := platform.OpenURL(url); err != nil {
			// The toast reaches the main window on its next frame; the
			// operation log line (preview package) carries the shape.
			if m.report != nil {
				m.report("Could not open " + url)
			}
			return err
		}
		return nil
	}))
	m.controllers[targetURL] = c
	return c
}
