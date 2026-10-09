package nativeui

/**
 * [INPUT]: 960x640 and 1200x800 MyGo headless windows, large Git file selection and AI disclosure
 * [OUTPUT]: fixed Commit/Cancel footer remains within the window when body grows
 * [POS]: native modal layout regression, no Git mutation
 * [PROTOCOL]: update header and check CLAUDE.md
 */

import (
	"fmt"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

func TestCommitFileSelectionActionsAndDisabledSubmit(t *testing.T) {
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.openCommitSurface()
	s.surface.commit.Subject = "fix: selected changes"
	tester := ui.NewTester(s.View, 960, 640)
	tester.Frame()
	if !tester.HasText("2 / 2") {
		t.Fatal("Commit modal should show selected/total file count")
	}
	if err := tester.Click("Clear selection"); err != nil {
		t.Fatal(err)
	}
	if len(s.selectedCommitPaths()) != 0 || !tester.HasText("Commit 0 files") {
		t.Fatal("Clear should remove files and make Commit unavailable")
	}
	if err := tester.Click("Select all"); err != nil {
		t.Fatal(err)
	}
	if len(s.selectedCommitPaths()) != 2 || !tester.HasText("Commit 2 files") {
		t.Fatal("Select all should restore explicit commit selection")
	}
}

func TestCommitFooterRemainsVisibleWithManyFilesAndAI(t *testing.T) {
	for _, size := range []struct{ w, h int }{{960, 640}, {1200, 800}} {
		t.Run(fmt.Sprintf("%dx%d", size.w, size.h), func(t *testing.T) {
			s := gitSurfaceTestShell(t, t.TempDir())
			for i := 0; i < 30; i++ {
				s.git.snapshot.Files = append(s.git.snapshot.Files,
					gitworkbench.ChangeFile{Path: fmt.Sprintf("src/extra/file-%02d.go", i), Status: gitworkbench.StatusModified})
			}
			s.showSurface(WorkspaceSurfaceDiff)
			s.openCommitSurface()
			tester := ui.NewTester(s.View, size.w, size.h)
			tester.Frame()
			before, ok := tester.Find("Cancel Commit")
			if !ok || before.Y < 0 || before.Y+before.H > float32(size.h) {
				t.Fatalf("Commit footer must remain on screen: found=%v rect=%+v", ok, before)
			}
			s.commitPromptPreviewOpen = true
			s.commitPromptTextVisible = true
			s.commitAIManualEntryOpen = true
			tester.Frame()
			after, ok := tester.Find("Cancel Commit")
			if !ok || after.Y < 0 || after.Y+after.H > float32(size.h) {
				t.Fatalf("expanded AI view hid Commit footer: found=%v rect=%+v", ok, after)
			}
			if delta := after.Y - before.Y; delta > 3 || delta < -3 {
				t.Fatalf("Commit footer jumped during AI disclosure: before=%+v after=%+v", before, after)
			}
			if !tester.HasText("Commit 32 files") {
				t.Fatalf("selected file count lost: %q", tester.Texts())
			}
		})
	}
}
