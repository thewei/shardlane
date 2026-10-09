package nativeui

/**
 * [INPUT]: bounded selected Git review and fake asynchronous one-shot Pi completion
 * [OUTPUT]: success/cancel/stale-workspace fences for native Commit suggestion
 * [POS]: UI coordinator tests, no provider API or real Git mutation
 * [PROTOCOL]: update header then check CLAUDE.md
 */

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/wh-studio/herdr-client/internal/agent"
)

func piTestShell(t *testing.T) *Shell {
	t.Helper()
	s := gitSurfaceTestShell(t, t.TempDir())
	s.showSurface(WorkspaceSurfaceDiff)
	s.openCommitSurface()
	s.surface.commit.Captured = true
	return s
}

func awaitPiApply(t *testing.T, queue chan func()) {
	t.Helper()
	select {
	case f := <-queue:
		f()
	case <-time.After(3 * time.Second):
		t.Fatal("Pi completion timeout")
	}
}

func TestPiCommitUsesSelectedSnapshotAndNeverCommits(t *testing.T) {
	s := piTestShell(t)
	queue := make(chan func(), 1)
	s.uiApplyOverride = func(f func()) { queue <- f }
	s.piCommitExecutor = func(ctx context.Context, req agent.PiCommitRequest) (agent.PiCommitResponse, error) {
		if req.Root != s.git.root || !strings.Contains(req.Prompt, "File: src/main.go") ||
			!strings.Contains(req.Prompt, "Conventional Commits") || strings.Contains(req.Prompt, "fmt.Println") {
			return agent.PiCommitResponse{}, context.Canceled
		}
		return agent.PiCommitResponse{Message: "fix: update Git interface"}, nil
	}
	s.startPiCommitSuggestion()
	if !s.piCommitRunning {
		t.Fatal("Pi did not start")
	}
	awaitPiApply(t, queue)
	if s.piCommitRunning || s.commitAIResponse != "fix: update Git interface" || s.surface.commit.Subject != "" || s.git.committing {
		t.Fatalf("Pi must provide a candidate, not commit or silently overwrite: %q", s.commitAIResponse)
	}
	s.applyManualAISuggestion()
	if s.surface.commit.Subject != "fix: update Git interface" || s.surface.commit.InFlight {
		t.Fatal("candidate acceptance must not execute Git")
	}
}

func TestPiCommitRejectsStaleRepoAndCancel(t *testing.T) {
	s := piTestShell(t)
	queue := make(chan func(), 2)
	s.uiApplyOverride = func(f func()) { queue <- f }
	s.piCommitExecutor = func(ctx context.Context, req agent.PiCommitRequest) (agent.PiCommitResponse, error) {
		return agent.PiCommitResponse{Message: "feat: stale data"}, nil
	}
	s.startPiCommitSuggestion()
	s.git.root = t.TempDir()
	awaitPiApply(t, queue)
	if s.commitAIResponse != "" || !strings.Contains(s.piCommitError, "changed") {
		t.Fatalf("old repository result applied: %q, %q", s.commitAIResponse, s.piCommitError)
	}
	s.git.root = s.projection.Projects[0].CWD
	s.startPiCommitSuggestion()
	s.resetCommitAIFlow()
	awaitPiApply(t, queue)
	if s.commitAIResponse != "" || s.piCommitRunning {
		t.Fatal("canceled Pi result applied after editor closed")
	}
}
