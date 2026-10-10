package nativeui

/**
 * [INPUT]: Headless MyGo renderer with realistic local changes and commit ancestry
 * [OUTPUT]: Optional Git Local Changes and All Commits screenshots for layout inspection
 * [POS]: Visual QA only, enabled by SHARDLANE_GIT_LAYOUT_EVIDENCE
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

func TestGitLayoutRenderEvidence(t *testing.T) {
	dir := os.Getenv("SHARDLANE_GIT_LAYOUT_EVIDENCE")
	if dir == "" {
		t.Skip("set SHARDLANE_GIT_LAYOUT_EVIDENCE to render Git layout PNGs")
	}
	if err := os.MkdirAll(dir, 0755); err != nil {
		t.Fatal(err)
	}
	s := gitSurfaceTestShell(t, t.TempDir())
	s.git.rootOf[s.git.root] = s.git.root
	s.git.refreshing = true
	s.git.snapshot.Files[0].Unstaged = true
	s.git.snapshot.Files[1].Staged = true
	s.git.branches = []gitworkbench.Branch{{Name: "main", Current: true, HeadShort: "aaaaaaa"}, {Name: "feat/editor"}}
	s.git.remoteBranches = []gitworkbench.RemoteBranch{{Remote: "origin", Name: "main", HeadShort: "aaaaaaa"}}
	s.git.tags = []gitworkbench.Tag{{Name: "v0.10.0"}}
	s.git.stashes = []gitworkbench.Stash{{Ref: "stash@{0}", Subject: "WIP release"}}
	s.git.worktrees = []gitworkbench.Worktree{{Path: s.git.root, Branch: "main"}}
	for i := 0; i < 16; i++ {
		hash := fmt.Sprintf("%040x", 12345+i)
		short := hash[:7]
		if i == 0 {
			hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
			short = "aaaaaaa"
		}
		cm := gitworkbench.CommitInfo{Hash: hash, Short: short, Subject: "feat(ui): improve project navigation and Git review", Author: "Wilson", Time: time.Now().Add(-time.Duration(i) * time.Hour)}
		if i == 0 {
			cm.Parents = []string{"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
		}
		s.git.commits = append(s.git.commits, cm)
	}
	s.showSurface(WorkspaceSurfaceDiff)
	s.rebuildDiffRows()
	tt := ui.NewTester(s.View, 1280, 900)
	tt.SetDark(true)
	render := func(name string) {
		tt.Frame()
		file, err := os.Create(filepath.Join(dir, name))
		if err != nil {
			t.Fatal(err)
		}
		defer file.Close()
		if err := png.Encode(file, tt.Image()); err != nil {
			t.Fatal(err)
		}
	}
	render("local-changes.png")
	s.commitsMode = true
	s.git.source = gdSourceCommit
	s.git.commitHash = s.git.commits[0].Hash
	s.git.commitMeta = &s.git.commits[0]
	commitSnap := *s.git.snapshot
	s.git.commitSnap = &commitSnap
	s.git.gdFilesFor = nil
	s.rebuildDiffRows()
	render("all-commits.png")
}
