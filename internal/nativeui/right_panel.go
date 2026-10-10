package nativeui

/**
 * [INPUT]: selected Herdr Tab CWD and bounded background directory snapshots
 * [OUTPUT]: scoped Files explorer cache and persistent URL preview controllers
 * [POS]: Files workbench storage only; no docked right panel remains
 * [PROTOCOL]: update this header and check CLAUDE.md
 */

import (
	"github.com/wh-studio/herdr-client/internal/filesview"
	"github.com/wh-studio/herdr-client/internal/platform"
	"github.com/wh-studio/herdr-client/internal/preview"
	"sync"
	"sync/atomic"
)

// RightPanelSurface/legacy fields remain decode-compatible with persisted
// 0.10 sessions; they do not build or control an auxiliary rail.
type RightPanelSurface string

const (
	SurfaceChanges  RightPanelSurface = "changes"
	SurfaceFiles    RightPanelSurface = "files"
	SurfaceServices RightPanelSurface = "services"
)
const (
	DefaultRightPanelWidth = 320
	MinRightPanelWidth     = 240
	MaxRightPanelWidth     = 500
)

type rightPanelState struct {
	// Legacy session hints, intentionally ignored by the native layout.
	open          bool
	historyOpen   bool
	surface       RightPanelSurface
	width         int
	lastToolByTab map[string]RightPanelSurface

	// Active Files explorer data, fenced by the exact selected Tab CWD.
	expandedDirs     map[string]bool
	cachedDirEntries map[string][]filesview.Entry
	loadingDirs      map[string]bool
	currentRoot      string
	selectedFilePath string
	preview          *filesview.Preview
	previewOpen      bool // legacy; inline Files preview does not create a modal
	filesGen         atomic.Uint64
}

func defaultRightPanelState() rightPanelState {
	return rightPanelState{
		surface: SurfaceFiles, width: DefaultRightPanelWidth,
		lastToolByTab:    make(map[string]RightPanelSurface),
		expandedDirs:     make(map[string]bool),
		cachedDirEntries: make(map[string][]filesview.Entry),
		loadingDirs:      make(map[string]bool),
	}
}

// syncRightPanelRoot updates the Files explorer's snapshot fence only.
// Despite the compatibility name this never opens a right panel.
func (s *Shell) syncRightPanelRoot() {
	root := s.selectedTabCWD()
	if root == s.rightPanel.currentRoot {
		return
	}
	s.rightPanel.currentRoot = root
	s.rightPanel.filesGen.Add(1)
	s.rightPanel.selectedFilePath = ""
	s.rightPanel.preview = nil
	s.rightPanel.previewOpen = false
	s.rightPanel.expandedDirs = make(map[string]bool)
	s.rightPanel.cachedDirEntries = make(map[string][]filesview.Entry)
	s.rightPanel.loadingDirs = make(map[string]bool)
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
