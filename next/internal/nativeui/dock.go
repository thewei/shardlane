package nativeui

import (
	"fmt"
	"strconv"
)

// DockBadgeText computes the P12 desktop attention badge text from the shared
// StatusCenterSnapshot: Dock badge = NeedsAttention + ReviewPending.
// Working agents do NOT increase the badge count (0.9 P12 invariant).
// Returns "" when no attention/review is required, which clears the badge.
func DockBadgeText(snapshot StatusCenterSnapshot) string {
	count := snapshot.NeedsAttention + snapshot.ReviewPending
	if count <= 0 {
		return ""
	}
	return strconv.Itoa(count)
}

// dockPlatform defines the platform abstraction for macOS Dock manipulations.
type dockPlatform interface {
	SetBadge(text string)
}

// dockController manages updating the Dock badge with deduplication.
type dockController struct {
	platform  dockPlatform
	lastBadge string
}

func newDockController(platform dockPlatform) *dockController {
	return &dockController{
		platform:  platform,
		lastBadge: "",
	}
}

// update projects the status center snapshot to the platform Dock badge.
func (d *dockController) update(snapshot StatusCenterSnapshot) {
	if d.platform == nil {
		return
	}
	newBadge := DockBadgeText(snapshot)
	if newBadge != d.lastBadge {
		d.lastBadge = newBadge
		d.platform.SetBadge(newBadge)
	}
}

// AttachDock binds the macOS Dock platform to the shell.
func (s *Shell) AttachDock(dock dockPlatform) {
	s.dock = newDockController(dock)
	s.updateDockBadge()
}

// updateDockBadge triggers the Dock badge update from the current snapshot.
func (s *Shell) updateDockBadge() {
	if s.dock == nil {
		return
	}
	s.dock.update(s.BuildStatusCenterSnapshot())
}

// FormatDockAttentionMessage returns an optional formatted console/log label.
func FormatDockAttentionMessage(snapshot StatusCenterSnapshot) string {
	badge := DockBadgeText(snapshot)
	if badge == "" {
		return "Dock: Clear"
	}
	return fmt.Sprintf("Dock Badge: %s (Attention: %d, Review: %d)", badge, snapshot.NeedsAttention, snapshot.ReviewPending)
}
