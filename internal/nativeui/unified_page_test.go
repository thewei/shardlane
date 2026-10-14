package nativeui

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

/**
 * [INPUT]: MyGo Tester, Settings routes, Git workbench snapshot and Workspace Chat gate
 * [OUTPUT]: Regression coverage for uniform Settings structure, compact Git clean surface (readable when narrow) and Terminal return
 * [POS]: UI-007/UI-008 implementation acceptance without touching live Herdr sessions
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

func TestSettingsSectionsShareHeaderAndBoundedScrollableBody(t *testing.T) {
	for _, tc := range []struct {
		route string
		title string
		body  string
	}{
		{"/settings/general", "General", "Appearance"},
		{"/settings/terminal", "Terminal", "Font"},
		{"/settings/diagnostics", "Diagnostics", "About"},
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
}

func TestGitFilterEmptyStateCanRecover(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.changes.filter = "not-a-matching-file"
	s.showSurface(WorkspaceSurfaceDiff)
	s.rebuildDiffRows()
	tester := ui.NewTester(s.View, 1200, 800)
	tester.Frame()
	if !tester.HasText("No matching files") || !tester.HasText("Clear File Filter") {
		t.Fatalf("file filter has no recovery: %q", tester.Texts())
	}
	if err := tester.Click("Clear File Filter"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if s.changes.filter != "" || !tester.HasText("src/main.go") {
		t.Fatalf("clearing file filter did not restore diff: %q", tester.Texts())
	}
}

// With both rails open at ~900 px (a window this narrow closes the navigator
// until the user opens it) the clean surface's column is ~350 px: the
// message must keep reading as words and the actions drop below it, instead of
// the text being squeezed into a column of single letters beside the buttons.
func TestCleanGitSurfaceKeepsItsMessageReadableWhenNarrow(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.gitFilePaneOpenNarrow = true
	s.git.rootOf[s.git.root] = s.git.root
	s.git.snapshot = &gitworkbench.ChangesSnapshot{
		Root: s.git.root, Branch: "main", Signature: "clean", Files: nil,
	}
	s.showSurface(WorkspaceSurfaceDiff)
	s.rebuildDiffRows()

	narrow := ui.NewTester(s.View, 900, 800)
	narrow.Frame()
	title, ok := narrow.Find("No local changes")
	if !ok {
		t.Fatalf("narrow clean surface lost its message: %q", narrow.Texts())
	}
	if title.W < 90 || title.H > 30 {
		t.Fatalf("message squeezed to %.0f x %.0f px at 900 px wide, want one readable line", title.W, title.H)
	}
	for _, want := range []string{"View Commits", "Open Terminal"} {
		if !narrow.HasText(want) {
			t.Fatalf("narrow clean surface lost %q: %q", want, narrow.Texts())
		}
	}

	// Room to spare keeps the actions beside the message.
	wide := ui.NewTester(s.View, 1700, 800)
	wide.Frame()
	title, _ = wide.Find("No local changes")
	if open, ok := wide.Find("Open Terminal"); !ok || open.Y > title.Y+title.H {
		t.Errorf("a wide clean surface keeps the actions on the message's row: title=%v open=%v (found %v)", title, open, ok)
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
