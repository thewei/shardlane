// [INPUT]: Scratch Git repository with local remote-tracking refs and symbolic HEAD.
// [OUTPUT]: Contract test proving remote refs are discovered without symbolic HEAD entries.
// [POS]: Git reference discovery regression, no network/fetch dependence.
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
package gitworkbench

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

func TestListRemoteBranchesGroupsAndSkipsSymbolicHEAD(t *testing.T) {
	root := t.TempDir()
	git := func(args ...string) {
		t.Helper()
		cmd := exec.Command("git", args...)
		cmd.Dir = root
		cmd.Env = append(os.Environ(), "GIT_CONFIG_NOSYSTEM=1")
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v: %s", args, err, out)
		}
	}
	git("init", "-q")
	git("-c", "user.name=Example", "-c", "user.email=example@test", "commit", "--allow-empty", "-qm", "initial")
	git("update-ref", "refs/remotes/origin/main", "HEAD")
	git("update-ref", "refs/remotes/upstream/feat/child", "HEAD")
	git("symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main")
	branches, err := (&Runner{}).ListRemoteBranches(context.Background(), filepath.Clean(root))
	if err != nil {
		t.Fatal(err)
	}
	if len(branches) != 2 {
		t.Fatalf("branches = %+v, want exactly two real refs", branches)
	}
	if branches[0].Remote != "origin" || branches[0].Name != "main" || branches[1].Remote != "upstream" || branches[1].Name != "feat/child" {
		t.Fatalf("unexpected remote refs: %+v", branches)
	}
}
