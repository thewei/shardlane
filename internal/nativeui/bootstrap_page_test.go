package nativeui

/**
 * [INPUT]: Simulated startup/loading/failure flags and MyGo headless rendering
 * [OUTPUT]: Proof startup hides controls, centers the only loader and reveals the workspace
 * [POS]: Bootstrap visual and interaction regression for first-launch Herdr readiness
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/herdr"
	"testing"
)

func TestHerdrBootstrapHidesWorkspaceChromeAndCentersLoader(t *testing.T) {
	s := NewShell()
	s.bootstrapPending = true
	s.loading = true
	tt := ui.NewTester(s.View, 1200, 800)
	tt.Frame()
	label, ok := tt.Find("Loading Herdr workspaces…")
	if !ok {
		t.Fatalf("loading indicator missing: %q", tt.Texts())
	}
	if label.X+label.W/2 < 450 || label.X+label.W/2 > 750 ||
		label.Y+label.H/2 < 315 || label.Y+label.H/2 > 490 {
		t.Fatalf("loading label is not centered within the window: %+v", label)
	}
	for _, hidden := range []string{"Hide Sidebar", "Local Changes", "Terminal view", "History view", "Settings", "Workspace", "Agents"} {
		if tt.HasText(hidden) {
			t.Fatalf("%q must not mount during bootstrap: %q", hidden, tt.Texts())
		}
	}
	s.bootstrapPending = false
	s.loading = false
	s.status = "Native Terminal"
	s.activeInstance = "default"
	s.instances = []herdr.Instance{{Name: "default", DisplayName: "Default", Default: true}}
	s.projection = herdr.Projection{Protocol: 22}
	tt.Frame()
	if !tt.HasText("Changes view") || !tt.HasText("Agents") || tt.HasText("Loading Herdr workspaces…") {
		t.Fatalf("workbench failed to appear after ready: %q", tt.Texts())
	}
}

func TestHerdrBootstrapErrorOffersRetryWithoutLeakingMenus(t *testing.T) {
	s := NewShell()
	s.bootstrapPending = false
	s.bootstrapFailed = true
	s.loading = false
	s.errText = "herdr session list timed out"
	tt := ui.NewTester(s.View, 960, 640)
	tt.Frame()
	for _, label := range []string{"Unable to load workspaces", "Retry Connection", "herdr session list timed out"} {
		if !tt.HasText(label) {
			t.Fatalf("bootstrap error missing %q: %q", label, tt.Texts())
		}
	}
	if tt.HasText("Terminal view") || tt.HasText("Settings") {
		t.Fatalf("error page mounted active workspace: %q", tt.Texts())
	}
}
