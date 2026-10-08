package nativeui

import (
	"testing"

	"github.com/wh-studio/herdr-client/internal/herdr"
)

// TestReconcileSurfaceScrollAdoptsAuthoritativeViewport pins the projection
// reconciliation: Herdr's viewport metrics are adopted while no scroll
// mutation is in flight, and an in-flight pane.scroll keeps the local mirror
// so a mid-gesture projection never snaps the viewport backwards.
func TestReconcileSurfaceScrollAdoptsAuthoritativeViewport(t *testing.T) {
	s := NewShell()
	surface := &terminalSurface{paneID: "p1", scrollOffset: 30}
	pane := herdr.Pane{ID: "p1", Scroll: &herdr.PaneScroll{OffsetFromBottom: 55, MaxOffsetFromBottom: 900}}

	s.reconcileSurfaceScroll(surface, pane)
	if surface.scrollMax != 900 || surface.scrollOffset != 55 {
		t.Fatalf("idle reconcile: max=%d offset=%d, want 900/55", surface.scrollMax, surface.scrollOffset)
	}

	s.dispatchPaneScroll("p1", 80)
	if d := s.paneScrolls["p1"]; d == nil || d.pending != 80 {
		t.Fatalf("dispatch state = %#v", s.paneScrolls["p1"])
	}
	s.paneScrolls["p1"].inflight = true
	surface.scrollOffset = 30
	s.reconcileSurfaceScroll(surface, pane)
	if surface.scrollOffset != 30 {
		t.Fatalf("in-flight reconcile reverted the mirror: offset=%d, want 30", surface.scrollOffset)
	}
	if surface.scrollMax != 900 {
		t.Fatalf("in-flight reconcile dropped the clamp: max=%d, want 900", surface.scrollMax)
	}
}
