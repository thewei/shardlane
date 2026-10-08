package nativeui

import (
	"sync"
)

// WorkspaceSurfaceKind is one mutually exclusive visible center surface of
// the /workspace page (plan §5.1). Terminal/Diff/Commit are contextual
// surface state inside /workspace — never global Router routes.
type WorkspaceSurfaceKind string

const (
	WorkspaceSurfaceTerminal WorkspaceSurfaceKind = "terminal"
	WorkspaceSurfaceChat     WorkspaceSurfaceKind = "chat"
	WorkspaceSurfaceDiff     WorkspaceSurfaceKind = "diff"
	WorkspaceSurfaceCommit   WorkspaceSurfaceKind = "commit"
)

// WorkspaceContextKey identifies the runtime context a surface state belongs
// to (plan §5.1). It carries identity strings only — never Herdr runtime
// objects. RepoRoot is the resolved Git repository root of the selected Tab
// ("" when the Tab is not inside a repository).
type WorkspaceContextKey struct {
	InstanceID string
	ProjectID  string
	TabID      string
	RepoRoot   string
}

// DiffSurfaceState is the Diff surface's own presentation state.
type DiffSurfaceState struct {
	SelectedPath string
	SplitLayout  bool
}

// CommitSurfaceState is the Commit surface's own presentation state.
type CommitSurfaceState struct {
	Subject       string
	Body          string
	Selected      map[string]bool
	Captured      bool // stale-state fence captured at open
	InFlight      bool
	ErrText       string
	CommittedHash string
	// Amend replaces the last commit; AmendPrefilled guards loading HEAD's
	// message into the fields once per toggle.
	Amend          bool
	AmendPrefilled bool
}

// workspaceSurfaceState is the UI-independent primary-surface state machine
// for one /workspace center (plan §6 transition table). Pure state: no Herdr
// objects, no Git calls, no IO — transition tests run headless.
type workspaceSurfaceState struct {
	mu sync.Mutex

	kind    WorkspaceSurfaceKind
	context WorkspaceContextKey
	diff    DiffSurfaceState
	commit  CommitSurfaceState
	// previous is the surface restored when a Commit cancels.
	previous WorkspaceSurfaceKind
}

func (w *workspaceSurfaceState) current() WorkspaceSurfaceKind {
	w.mu.Lock()
	defer w.mu.Unlock()
	return w.kind
}

func (w *workspaceSurfaceState) activeContext() WorkspaceContextKey {
	w.mu.Lock()
	defer w.mu.Unlock()
	return w.context
}

// resetToTerminal applies the "startup / workspace selection / Project/Tab/
// Pane click → Terminal" family of transitions. A Tab/repo context change
// always lands on Terminal (plan §6 last row) and drops prior diff state.
func (w *workspaceSurfaceState) resetToTerminal(ctx WorkspaceContextKey) {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.context != ctx {
		w.diff = DiffSurfaceState{}
		w.commit = CommitSurfaceState{}
		w.previous = WorkspaceSurfaceTerminal
	}
	w.context = ctx
	w.kind = WorkspaceSurfaceTerminal
}

// openDiff implements "changed file click" / "Changes open": switch to the
// Diff surface for path ("" = current/first selection).
func (w *workspaceSurfaceState) openDiff(ctx WorkspaceContextKey, path string) {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.context != ctx {
		w.diff = DiffSurfaceState{}
	}
	w.context = ctx
	if path != "" || w.diff.SelectedPath == "" {
		w.diff.SelectedPath = path
	}
	w.previous = w.previousSurfaceFor(w.kind)
	w.kind = WorkspaceSurfaceDiff
}

// openChat switches to the Chat surface for the active context.
func (w *workspaceSurfaceState) openChat(ctx WorkspaceContextKey) {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.context != ctx {
		w.diff = DiffSurfaceState{}
	}
	w.context = ctx
	w.previous = w.previousSurfaceFor(w.kind)
	w.kind = WorkspaceSurfaceChat
}

// previousSurfaceFor remembers where a Commit cancel should land; leaving
// Terminal for Diff keeps Terminal as the eventual "back" target.
func (w *workspaceSurfaceState) previousSurfaceFor(from WorkspaceSurfaceKind) WorkspaceSurfaceKind {
	switch from {
	case WorkspaceSurfaceCommit:
		return WorkspaceSurfaceDiff
	default:
		return WorkspaceSurfaceTerminal
	}
}

// openCommit implements "Commit action → Commit". The preflight capture is
// flagged; the caller fills it right after (must not render a commit that
// silently trusts a stale snapshot).
func (w *workspaceSurfaceState) openCommit() {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.kind == WorkspaceSurfaceCommit {
		return
	}
	w.previous = w.kind
	w.kind = WorkspaceSurfaceCommit
	w.commit = CommitSurfaceState{Selected: map[string]bool{}}
}

// cancelCommit implements "Commit cancel → previous Diff".
func (w *workspaceSurfaceState) cancelCommit() WorkspaceSurfaceKind {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.kind != WorkspaceSurfaceCommit {
		return w.kind
	}
	w.kind = w.previous
	if w.kind == "" {
		w.kind = WorkspaceSurfaceDiff
	}
	w.commit = CommitSurfaceState{}
	return w.kind
}

// commitSucceeded implements "Commit success with changes → Diff refreshed"
// and "Commit success no changes → Diff empty state".
func (w *workspaceSurfaceState) commitSucceeded(remainingChanges bool) WorkspaceSurfaceKind {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.kind != WorkspaceSurfaceCommit {
		return w.kind
	}
	w.kind = WorkspaceSurfaceDiff
	w.commit.ErrText = ""
	return w.kind
}

// branchSwitched keeps the Diff surface and refreshes (plan §6: "switch
// branch → remain Diff + refresh").
func (w *workspaceSurfaceState) branchSwitched() {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.kind == WorkspaceSurfaceCommit {
		w.kind = WorkspaceSurfaceDiff
	}
}

// isValid reports whether a stored surface state can be restored for ctx
// (plan §6 "return /workspace → restore only valid prior context; else
// Terminal").
func (w *workspaceSurfaceState) isValid(ctx WorkspaceContextKey) bool {
	w.mu.Lock()
	defer w.mu.Unlock()
	return w.context == ctx && w.kind != ""
}

// surfaceForNavigation maps sidebar navigation intents (Project/Tab/Pane/
// Agent-to-Pane) to the center surface: always Terminal (plan §6).
func surfaceForNavigation() WorkspaceSurfaceKind { return WorkspaceSurfaceTerminal }
