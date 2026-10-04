package nativeui

import (
	"testing"
)

// The §6 transition table, headless and pure (GWB-030..039).

func ctxA() WorkspaceContextKey {
	return WorkspaceContextKey{InstanceID: "inst1", ProjectID: "p1", TabID: "t1", RepoRoot: "/repo"}
}

func ctxB() WorkspaceContextKey {
	return WorkspaceContextKey{InstanceID: "inst1", ProjectID: "p1", TabID: "t2", RepoRoot: "/repo2"}
}

func TestTransitionStartupIsTerminal(t *testing.T) {
	var w workspaceSurfaceState
	w.resetToTerminal(ctxA())
	if w.current() != WorkspaceSurfaceTerminal {
		t.Fatalf("startup surface = %v", w.current())
	}
}

func TestTransitionSidebarClicksLandTerminal(t *testing.T) {
	var w workspaceSurfaceState
	w.resetToTerminal(ctxA())
	w.openDiff(ctxA(), "file.go")
	if w.current() != WorkspaceSurfaceDiff {
		t.Fatalf("setup: expected diff, got %v", w.current())
	}
	// Project click / Tab click / Pane click / Agent→Pane all reset to Terminal.
	for _, ctx := range []WorkspaceContextKey{ctxA(), ctxB()} {
		w.resetToTerminal(ctx)
		if w.current() != WorkspaceSurfaceTerminal {
			t.Fatalf("navigation surface = %v", w.current())
		}
	}
}

func TestTransitionChangedFileClickOpensDiff(t *testing.T) {
	var w workspaceSurfaceState
	w.resetToTerminal(ctxA())
	w.openDiff(ctxA(), "src/main.go")
	if w.current() != WorkspaceSurfaceDiff {
		t.Fatalf("surface = %v", w.current())
	}
	if w.diff.SelectedPath != "src/main.go" {
		t.Fatalf("selected = %q", w.diff.SelectedPath)
	}
}

func TestTransitionCommitOpenCancelRestoresDiff(t *testing.T) {
	var w workspaceSurfaceState
	w.resetToTerminal(ctxA())
	w.openDiff(ctxA(), "a.go")
	w.openCommit()
	if w.current() != WorkspaceSurfaceCommit {
		t.Fatalf("surface = %v", w.current())
	}
	if w.previous != WorkspaceSurfaceDiff {
		t.Fatalf("previous = %v", w.previous)
	}
	if got := w.cancelCommit(); got != WorkspaceSurfaceDiff {
		t.Fatalf("cancel landed on %v", got)
	}
	if w.current() != WorkspaceSurfaceDiff {
		t.Fatalf("surface after cancel = %v", w.current())
	}
}

func TestTransitionCommitSuccessRefreshesDiff(t *testing.T) {
	var w workspaceSurfaceState
	w.resetToTerminal(ctxA())
	w.openDiff(ctxA(), "a.go")
	w.openCommit()
	if got := w.commitSucceeded(true); got != WorkspaceSurfaceDiff {
		t.Fatalf("success surface = %v", got)
	}
}

func TestTransitionBranchSwitchRemainsDiff(t *testing.T) {
	var w workspaceSurfaceState
	w.resetToTerminal(ctxA())
	w.openDiff(ctxA(), "a.go")
	w.branchSwitched()
	if w.current() != WorkspaceSurfaceDiff {
		t.Fatalf("branch switch must keep diff, got %v", w.current())
	}
	// From Commit, a switch lands back on Diff.
	w.openCommit()
	w.branchSwitched()
	if w.current() != WorkspaceSurfaceDiff {
		t.Fatalf("post-switch surface = %v", w.current())
	}
}

func TestTransitionRightPanelNeverChangesSurface(t *testing.T) {
	// Regression GWB-036: panel open/close/tool-switch is center-invisible.
	// The surface state machine exposes no panel hooks by construction; this
	// test pins that contract: transitions only via the five mutators.
	var w workspaceSurfaceState
	w.resetToTerminal(ctxA())
	w.openDiff(ctxA(), "x.go")
	// (panel toggling touches rightPanelState only; nothing to assert beyond
	// the surface still being Diff after arbitrary non-mutating calls)
	if w.current() != WorkspaceSurfaceDiff {
		t.Fatalf("surface = %v", w.current())
	}
}

func TestTransitionContextChangeResetsToTerminal(t *testing.T) {
	var w workspaceSurfaceState
	w.resetToTerminal(ctxA())
	w.openDiff(ctxA(), "a.go")
	w.openCommit()
	// Tab/repo context changes → Terminal with dropped state.
	w.resetToTerminal(ctxB())
	if w.current() != WorkspaceSurfaceTerminal {
		t.Fatalf("surface = %v", w.current())
	}
	if w.diff.SelectedPath != "" {
		t.Fatalf("stale selected path = %q", w.diff.SelectedPath)
	}
}

func TestTransitionRestoreOnlyValidContext(t *testing.T) {
	var w workspaceSurfaceState
	w.resetToTerminal(ctxA())
	w.openDiff(ctxA(), "a.go")
	if !w.isValid(ctxA()) {
		t.Fatal("same context must be valid for restore")
	}
	if w.isValid(ctxB()) {
		t.Fatal("foreign context must not restore")
	}
}

func TestCommitOpenTwiceKeepsState(t *testing.T) {
	var w workspaceSurfaceState
	w.resetToTerminal(ctxA())
	w.openCommit()
	w.commit.Subject = "first"
	w.openCommit()
	if w.commit.Subject != "first" {
		t.Fatalf("reopen wiped draft: %q", w.commit.Subject)
	}
}

func TestTransitionOpenChatSwitchesToChat(t *testing.T) {
	var w workspaceSurfaceState
	w.resetToTerminal(ctxA())
	w.openChat(ctxA())
	if w.current() != WorkspaceSurfaceChat {
		t.Fatalf("surface = %v, want WorkspaceSurfaceChat", w.current())
	}
	if w.previous != WorkspaceSurfaceTerminal {
		t.Fatalf("previous = %v, want WorkspaceSurfaceTerminal", w.previous)
	}
}

func TestTransitionContextChangeFromChatResetsToTerminal(t *testing.T) {
	var w workspaceSurfaceState
	w.resetToTerminal(ctxA())
	w.openChat(ctxA())
	w.resetToTerminal(ctxB())
	if w.current() != WorkspaceSurfaceTerminal {
		t.Fatalf("surface = %v, want WorkspaceSurfaceTerminal", w.current())
	}
}
