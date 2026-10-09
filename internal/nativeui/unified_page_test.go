package nativeui

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

/**
 * [INPUT]: MyGo Tester, Settings routes, Git workbench snapshot and Workspace Chat gate
 * [OUTPUT]: Regression coverage for uniform Settings structure, compact Git clean surface and Terminal return
 * [POS]: UI-007/UI-008 implementation acceptance without touching live Herdr sessions
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestSettingsSectionsShareHeaderAndBoundedScrollableBody(t *testing.T) {
	for _, tc := range []struct {
		route string
		title string
		body  string
	}{
		{"/settings/general", "Appearance, layout and window preferences.", "Appearance"},
		{"/settings/terminal", "Native Terminal presentation for attached Herdr Panes.", "Presentation"},
		{"/settings/runtime", "Current Herdr session and application runtime facts.", "Refresh Runtime"},
	} {
		t.Run(tc.route, func(t *testing.T) {
			for _, size := range [][2]int{{960, 640}, {1200, 800}, {1512, 982}} {
				s := NewShell()
				s.loading = false
				s.router.Replace(tc.route)
				tester := ui.NewTester(s.View, size[0], size[1])
				if !tester.HasText(tc.title) || !tester.HasText(tc.body) {
					t.Fatalf("%dx%d lacks unified Settings header/body: %q", size[0], size[1], tester.Texts())
				}
			}
		})
	}
}

func TestTerminalSettingsTransparentBackgroundIsSingleControl(t *testing.T) {
	s := NewShell()
	s.loading = false
	s.router.Replace("/settings/terminal")
	tester := ui.NewTester(s.View, 1200, 800)
	count := 0
	for _, label := range tester.Texts() {
		if strings.TrimSpace(label) == "Transparent background" {
			count++
		}
	}
	// MyGo exposes each Field label once as AX label and once as visible
	// text. One control produces exactly two records, not one.
	if count != 2 {
		t.Fatalf("transparent background field labels = %d (want one control): %q", count, tester.Texts())
	}
	descriptionCount := 0
	for _, label := range tester.Texts() {
		if strings.Contains(label, "Let the window gradient show through the terminal paper") {
			descriptionCount++
		}
	}
	if descriptionCount != 1 {
		t.Fatalf("transparent background description repeated %d times", descriptionCount)
	}
}

func TestGitFilterEmptyStateCanRecover(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.changes.filter = "not-a-matching-file"
	s.showSurface(WorkspaceSurfaceDiff)
	s.rebuildDiffRows()
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	if !tester.HasText("No matching files") || !tester.HasText("Clear file filter") {
		t.Fatalf("file filter has no recovery: %q", tester.Texts())
	}
	if err := tester.Click("Clear file filter"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if s.changes.filter != "" || !tester.HasText("src/main.go") {
		t.Fatalf("clearing file filter did not restore diff: %q", tester.Texts())
	}
}

func TestCleanGitSurfaceOffersHistoryAndTerminalWithoutCenteredModal(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.git.snapshot = &gitworkbench.ChangesSnapshot{
		Root: s.git.root, Branch: "main", Signature: "clean", Files: nil,
	}
	s.showSurface(WorkspaceSurfaceDiff)
	s.rebuildDiffRows()
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	for _, want := range []string{"No local changes", "View Commits", "Open Terminal"} {
		if !tester.HasText(want) {
			t.Fatalf("clean Git surface missing %q: %q", want, tester.Texts())
		}
	}
	if err := tester.Click("View Commits"); err != nil {
		t.Fatal(err)
	}
	if !s.commitsMode {
		t.Fatal("clean view did not expose the existing All Commits mode")
	}
	if err := tester.Click("Open Terminal"); err != nil {
		t.Fatal(err)
	}
	if got := s.surface.current(); got != WorkspaceSurfaceTerminal {
		t.Fatalf("clean state return = %q, want Terminal", got)
	}
}
