package gitworkbench

import (
	"strings"
	"testing"
)

/**
 * [INPUT]: isolated real Git repositories with divergent branches, conflicts and sequencer state
 * [OUTPUT]: proofs of operation identity, no hidden mutations, continue/abort and cherry-pick
 * [POS]: local Git sequencer integration regression; never operates on current user repo
 * [PROTOCOL]: update header and check CLAUDE.md
 */

func setupMergeConflict(t *testing.T) (string, *Runner) {
	t.Helper()
	root, r := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()
	main := r.CurrentBranch(ctx, root)
	mustRunGit(t, root, "switch", "-q", "-c", "feature")
	writeRepoFile(t, root, "base.txt", "feature line\nbase line 2\n")
	mustRunGit(t, root, "add", "base.txt")
	mustRunGit(t, root, "commit", "-q", "-m", "feature change")
	mustRunGit(t, root, "switch", "-q", main)
	writeRepoFile(t, root, "base.txt", "main line\nbase line 2\n")
	mustRunGit(t, root, "add", "base.txt")
	mustRunGit(t, root, "commit", "-q", "-m", "main change")
	return root, r
}

func TestMergeNoFFCreatesMergeCommitWithoutConflicts(t *testing.T) {
	root, r := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()
	main := r.CurrentBranch(ctx, root)
	mustRunGit(t, root, "switch", "-q", "-c", "topic")
	writeRepoFile(t, root, "topic.txt", "topic line\n")
	mustRunGit(t, root, "add", "topic.txt")
	mustRunGit(t, root, "commit", "-q", "-m", "topic")
	mustRunGit(t, root, "switch", "-q", main)
	if err := r.MergeNoFastForward(ctx, root, "topic"); err != nil {
		t.Fatalf("clean no-ff merge: %v", err)
	}
	parents := strings.Fields(runGit(t, root, "rev-list", "--parents", "-n", "1", "HEAD"))
	if len(parents) != 3 {
		t.Fatalf("explicit merge commit should have two parents: %v", parents)
	}
	if state, err := r.SequencerState(ctx, root); err != nil || state.Active() {
		t.Fatalf("finished merge must have no active sequencer: %+v %v", state, err)
	}
}

func TestMergeNoFFConflictContinue(t *testing.T) {
	root, r := setupMergeConflict(t)
	ctx, cancel := opCtx(t)
	defer cancel()
	head := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
	if err := r.MergeNoFastForward(ctx, root, "feature"); err == nil {
		t.Fatal("the intended conflicting merge should not succeed")
	}
	state, err := r.SequencerState(ctx, root)
	if err != nil || state.Kind != SequencerMerge || state.Unmerged != 1 || len(state.UnmergedPaths) != 1 || state.UnmergedPaths[0] != "base.txt" {
		t.Fatalf("merge conflict = %+v, %v", state, err)
	}
	if err := r.FinishSequencer(ctx, root, SequencerMerge, false); err == nil {
		t.Fatal("Continue must reject unresolved index entries")
	}
	if err := r.FinishSequencer(ctx, root, SequencerRevert, true); err == nil {
		t.Fatal("Abort for wrong operation must not touch the merge")
	}
	if got := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD")); got != head {
		t.Fatal("failure/incorrect operation moved HEAD")
	}
	writeRepoFile(t, root, "base.txt", "merged line\nbase line 2\n")
	mustRunGit(t, root, "add", "base.txt")
	state, err = r.SequencerState(ctx, root)
	if err != nil || state.Kind != SequencerMerge || state.Unmerged != 0 {
		t.Fatalf("resolved merge = %+v, %v", state, err)
	}
	if err := r.FinishSequencer(ctx, root, SequencerMerge, false); err != nil {
		t.Fatalf("continue merge: %v", err)
	}
	state, err = r.SequencerState(ctx, root)
	if err != nil || state.Active() || state.Unmerged != 0 {
		t.Fatalf("merge still active: %+v, %v", state, err)
	}
	parents := strings.Fields(runGit(t, root, "rev-list", "--parents", "-n", "1", "HEAD"))
	if len(parents) != 3 {
		t.Fatalf("expected two-parent merge commit, got %v", parents)
	}
}

func TestMergeNoFFConflictAbortRestoresOriginalHead(t *testing.T) {
	root, r := setupMergeConflict(t)
	ctx, cancel := opCtx(t)
	defer cancel()
	head := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
	before := readRepoFile(t, root, "base.txt")
	if err := r.MergeNoFastForward(ctx, root, "feature"); err == nil {
		t.Fatal("expected merge conflict")
	}
	if err := r.FinishSequencer(ctx, root, SequencerMerge, true); err != nil {
		t.Fatalf("abort merge: %v", err)
	}
	if got := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD")); got != head {
		t.Fatal("abort unexpectedly moved HEAD")
	}
	if got := readRepoFile(t, root, "base.txt"); got != before {
		t.Fatalf("abort unexpectedly changed clean baseline: %q", got)
	}
	state, err := r.SequencerState(ctx, root)
	if err != nil || state.Active() {
		t.Fatalf("sequencer remains active: %+v, %v", state, err)
	}
}

func TestRevertConflictAbortAndKeepPublishedHistory(t *testing.T) {
	root, r := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()
	writeRepoFile(t, root, "base.txt", "changed by A\nbase line 2\n")
	mustRunGit(t, root, "commit", "-qam", "A")
	hashA := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
	writeRepoFile(t, root, "base.txt", "changed by B\nbase line 2\n")
	mustRunGit(t, root, "commit", "-qam", "B")
	head := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
	if err := r.RevertCommit(ctx, root, hashA); err == nil {
		t.Fatal("revert A must conflict after B changed same lines")
	}
	state, err := r.SequencerState(ctx, root)
	if err != nil || state.Kind != SequencerRevert || state.Unmerged != 1 {
		t.Fatalf("revert conflict = %+v, %v", state, err)
	}
	if err := r.FinishSequencer(ctx, root, SequencerRevert, true); err != nil {
		t.Fatalf("abort revert: %v", err)
	}
	if got := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD")); got != head {
		t.Fatal("abort revert moved HEAD")
	}
	if got := readRepoFile(t, root, "base.txt"); got != "changed by B\nbase line 2\n" {
		t.Fatalf("abort revert didn't restore original file: %q", got)
	}
}

func TestRevertConflictContinueWithResolvedFile(t *testing.T) {
	root, r := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()
	writeRepoFile(t, root, "base.txt", "A line\nbase line 2\n")
	mustRunGit(t, root, "commit", "-qam", "A")
	hash := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
	writeRepoFile(t, root, "base.txt", "B line\nbase line 2\n")
	mustRunGit(t, root, "commit", "-qam", "B")
	if err := r.RevertCommit(ctx, root, hash); err == nil {
		t.Fatal("revert must conflict with changed B line")
	}
	writeRepoFile(t, root, "base.txt", "base line 1\nbase line 2\n")
	mustRunGit(t, root, "add", "base.txt")
	if err := r.FinishSequencer(ctx, root, SequencerRevert, false); err != nil {
		t.Fatalf("continue revert: %v", err)
	}
	if got := readRepoFile(t, root, "base.txt"); got != "base line 1\nbase line 2\n" {
		t.Fatalf("resolved revert content: %q", got)
	}
	state, err := r.SequencerState(ctx, root)
	if err != nil || state.Active() {
		t.Fatalf("revert marker remained after Continue: %+v %v", state, err)
	}
}

func TestCherryPickConflictContinueAndAbort(t *testing.T) {
	for _, abort := range []bool{false, true} {
		t.Run(map[bool]string{false: "continue", true: "abort"}[abort], func(t *testing.T) {
			root, r := setupMergeConflict(t)
			ctx, cancel := opCtx(t)
			defer cancel()
			hash := strings.TrimSpace(runGit(t, root, "rev-parse", "feature"))
			before := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
			if err := r.CherryPickCommit(ctx, root, hash); err == nil {
				t.Fatal("cherry-pick must conflict")
			}
			state, err := r.SequencerState(ctx, root)
			if err != nil || state.Kind != SequencerCherryPick || state.Unmerged != 1 {
				t.Fatalf("cherry-pick status: %+v %v", state, err)
			}
			if !abort {
				writeRepoFile(t, root, "base.txt", "resolved line\nbase line 2\n")
				mustRunGit(t, root, "add", "base.txt")
			}
			if err := r.FinishSequencer(ctx, root, SequencerCherryPick, abort); err != nil {
				t.Fatalf("finish cherry-pick: %v", err)
			}
			state, err = r.SequencerState(ctx, root)
			if err != nil || state.Active() {
				t.Fatalf("sequencer not closed: %+v %v", state, err)
			}
			after := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
			if abort && after != before || !abort && after == before {
				t.Fatalf("unexpected HEAD after abort=%v: %q vs %q", abort, before, after)
			}
		})
	}
}

func TestCherryPickCleanConflictAndSafety(t *testing.T) {
	root, r := tempRepo(t)
	ctx, cancel := opCtx(t)
	defer cancel()
	main := r.CurrentBranch(ctx, root)
	mustRunGit(t, root, "switch", "-q", "-c", "topic")
	writeRepoFile(t, root, "new.txt", "new feature\n")
	mustRunGit(t, root, "add", "new.txt")
	mustRunGit(t, root, "commit", "-q", "-m", "new feature")
	hash := strings.TrimSpace(runGit(t, root, "rev-parse", "HEAD"))
	mustRunGit(t, root, "switch", "-q", main)
	if err := r.CherryPickCommit(ctx, root, "bad"); err == nil {
		t.Fatal("abbreviated hash must be refused")
	}
	if err := r.CherryPickCommit(ctx, root, hash); err != nil {
		t.Fatalf("cherry-pick: %v", err)
	}
	if got := readRepoFile(t, root, "new.txt"); got != "new feature\n" {
		t.Fatalf("missing cherry-pick content: %q", got)
	}
	if err := r.CherryPickCommit(ctx, root, hash); err == nil {
		t.Fatal("repeating applied commit must not silently mutate")
	}
}
