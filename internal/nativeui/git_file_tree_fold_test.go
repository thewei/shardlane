package nativeui

/**
 * [INPUT]: MyGo clicks on changed-file directory rows in local and commit review
 * [OUTPUT]: Repeated collapse/reopen independent of selected-list index or mode
 * [POS]: Regression for previously non-responsive file-tree disclosures
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

func TestGitFileTreesRepeatedDirectoryClicks(t *testing.T) {
	for _, historical := range []bool{false, true} {
		name := "Local Changes"
		if historical {
			name = "All Commits"
		}
		t.Run(name, func(t *testing.T) {
			s := gitSurfaceTestShell(t, t.TempDir())
			s.git.rootOf[s.git.root] = s.git.root
			s.git.refreshing = true
			s.git.snapshot.Files = []gitworkbench.ChangeFile{
				{Path: "nested/one.go", Status: gitworkbench.StatusModified, Unstaged: true, Additions: 1},
				{Path: "nested/two.go", Status: gitworkbench.StatusModified, Unstaged: true, Additions: 2},
				{Path: "other/main.go", Status: gitworkbench.StatusModified, Unstaged: true, Additions: 3},
			}
			s.showSurface(WorkspaceSurfaceDiff)
			if historical {
				cm := gitworkbench.CommitInfo{Hash: "0123456789abcdef0123456789abcdef01234567", Short: "0123456", Subject: "test", Author: "Tester", Time: time.Now()}
				s.git.commits = []gitworkbench.CommitInfo{cm}
				s.commitsMode = true
				s.git.source = gdSourceCommit
				s.git.commitHash = cm.Hash
				s.git.commitMeta = &cm
				snap := *s.git.snapshot
				s.git.commitSnap = &snap
				s.git.gdFilesFor = nil
			}
			s.rebuildDiffRows()
			tt := ui.NewTester(s.View, 1280, 900)
			tt.SetPreferences(ui.Preferences{ReduceMotion: true})
			for i := 0; i < 3; i++ {
				if err := tt.Click("nested"); err != nil {
					t.Fatalf("collapse iteration %d: %v", i, err)
				}
				tt.Frame()
				if !s.git.collapsedDirs["nested"] {
					t.Fatalf("directory did not close at iteration %d", i)
				}
				if err := tt.Click("nested"); err != nil {
					t.Fatalf("reopen iteration %d: %v", i, err)
				}
				tt.Frame()
				if s.git.collapsedDirs["nested"] {
					t.Fatalf("directory did not reopen at iteration %d", i)
				}
			}
		})
	}
}
