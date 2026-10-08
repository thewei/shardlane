package nativeui

import (
	"context"
	"log/slog"
	"path/filepath"
	"sync/atomic"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// gitService is the shell-side orchestration over internal/gitworkbench: it
// owns the single Git cache, background lanes and presentation state. All
// mutable presentation state below is touched only on the UI update lane;
// background lanes deliver through guarded applies with operation-specific
// generations (P0-04): unrelated operations never cancel each other.
type gitService struct {
	runner      *gitworkbench.Runner
	cache       *gitworkbench.Cache
	highlighter *gitworkbench.ChromaHighlighter

	// Operation-specific async generations (P0-04).
	rootGen      atomic.Uint64
	snapshotGen  atomic.Uint64
	branchesGen  atomic.Uint64
	preflightGen atomic.Uint64
	gdLoadGen    atomic.Uint64

	// root binding: resolved repo root of the selected Tab ("", not a repo).
	root             string
	rootBusy         bool
	rootResolvingCWD string
	rootOf           map[string]string // cwd -> resolved root; "" = confirmed not a repo
	rootCancel       context.CancelFunc

	// changes presentation
	snapshot   *gitworkbench.ChangesSnapshot
	refreshing bool
	// staleBanner marks "changes detected externally" (plan §21).
	staleBanner bool
	// expectDrift suppresses the banner for the app's own mutations: a
	// stage/commit legitimately changes the signature, and flagging the
	// user's own action as external drift read as a bug (2026-10-06 F25).
	expectDrift bool

	// godiff-style review state (gd_*.go): per-file view state, the lazy
	// row model, find and j/k selection. All touched only on the UI lane.
	gdFiles     []*gdFile
	gdFilesFor  *gitworkbench.ChangesSnapshot
	gdRows      []gdRow
	gdRowsDirty bool
	gdList      ui.ListState
	gdListEl    *ui.Element
	gdCurrent   int
	gdSelFile   int
	gdSelHunk   int
	gdViewed    map[string]string // path -> fingerprint marked viewed
	gdHScroll   map[string]float32
	gdWordWrap  bool
	gdCharW     float32
	// find in diffs
	gdFinding    bool
	gdQuery      string
	gdMatches    []gdMatch
	gdMatch      int
	gdMatchesFor string
	// gdFileMatches marks the files holding a match of the find bar; those
	// show their lines even while collapsed.
	gdFileMatches map[int]bool
	// gdTyping is set while a field of the surface has the focus, whose
	// keys are its own.
	gdTyping bool

	// diff surface presentation
	splitLayout bool
	// collapsedDirs is the Right Panel tree's collapse state (tree only).
	collapsedDirs map[string]bool

	// branches
	branches        []gitworkbench.Branch
	branchesLoading bool
	branchBusy      bool
	// upstream is the tracking state of the current branch (ahead/behind),
	// refreshed alongside the branches.
	upstream gitworkbench.UpstreamStatus
	// stashes lists the repository's stash entries for the branch menu.
	stashes        []gitworkbench.Stash
	stashesLoading bool
	// tags and worktrees feed the diff-mode repo bar, refreshed alongside
	// the branches.
	tags      []gitworkbench.Tag
	worktrees []gitworkbench.Worktree
	// commit history for the All Commits pane.
	commits        []gitworkbench.CommitInfo
	commitsLoading bool

	// gdSourceKind is what the review shows: the worktree's changes or one
	// commit's diff (the All Commits selection).
	source     gdSourceKind
	commitHash string
	commitSnap *gitworkbench.ChangesSnapshot
	commitMeta *gitworkbench.CommitInfo

	// explicit repository operations (stage/discard/stash/sync): the UI-level
	// busy flag keeps buttons single-shot; the Runner owns the real lock.
	opBusy bool

	// commit fence
	preflight     *gitworkbench.CommitPreflight
	preflightSnap *gitworkbench.ChangesSnapshot
	committing    bool

	// meta owns the sidebar's per-repository facts cache (branch/±/files).
	meta *gitMetaService

	// selected file for Changes↔Diff sync with oscillation guard
	revealPath string
	revealAt   time.Time
}

// gapExpansion is one expanded unchanged region: numbered context lines.
type gapExpansion struct {
	lines []gitworkbench.DiffLine
}

// newGitService builds the shell-side orchestration over
// internal/gitworkbench.
func newGitService() *gitService {
	runner := &gitworkbench.Runner{}
	return &gitService{
		runner:        runner,
		cache:         gitworkbench.NewCache(runner),
		highlighter:   gitworkbench.NewChromaHighlighter("github"),
		rootOf:        map[string]string{},
		collapsedDirs: map[string]bool{},
		gdViewed:      map[string]string{},
		gdHScroll:     map[string]float32{},
		gdFileMatches: map[int]bool{},
		gdSelFile:     -1,
		gdSelHunk:     -1,
		meta:          newGitMetaService(),
	}
}

// applyGuarded runs fn on the UI lane when the operation's generation is
// still current. Without a window (headless tests) background lanes drop
// their results — tests seed state directly and no render can race an inline
// application — unless a test installs uiApplyOverride to observe the real
// production apply path.
func (s *Shell) applyGuarded(current func() uint64, gen uint64, fn func()) {
	guarded := func() {
		if gen != current() {
			return
		}
		fn()
	}
	if s.uiApplyOverride != nil {
		s.uiApplyOverride(guarded)
		return
	}
	if s.win == nil {
		return
	}
	s.win.Update(guarded)
}

// applyOnUI runs fn on the UI lane without a generation guard (mutation
// completion paths own their busy flags instead). Same headless rule.
func (s *Shell) applyOnUI(fn func()) {
	if s.uiApplyOverride != nil {
		s.uiApplyOverride(fn)
		return
	}
	if s.win == nil {
		return
	}
	s.win.Update(fn)
}

// onUI marshals fn onto the window update lane; runs inline when no window
// exists (headless). Callers that must respect the drop-in-headless rule
// check s.win themselves before scheduling.
func (s *Shell) onUI(fn func()) {
	if s.win == nil {
		fn()
		return
	}
	s.win.Update(fn)
}

// syncGitContext rebinds the Git context to the selected Tab's repository
// root. A context change resets the diff/changes presentation (plan §6).
// The root cache distinguishes "not yet resolved" from a confirmed
// non-repository, so the first real repository always resolves (P0-03).
func (s *Shell) syncGitContext() {
	if s.git == nil {
		return
	}
	cwd := s.selectedTabCWD()
	if cwd == "" {
		if s.git.root != "" {
			s.git.root = ""
			s.resetGitContextState()
		}
		return
	}

	if root, known := s.git.rootOf[cwd]; known {
		if root == s.git.root {
			return
		}
		if s.git.rootCancel != nil {
			s.git.rootCancel()
			s.git.rootCancel = nil
		}
		s.git.rootBusy = false
		s.git.rootResolvingCWD = ""
		s.git.root = root
		s.resetGitContextState()
		if root != "" {
			s.ensureGitSnapshot(false)
			s.loadBranches()
		}
		return
	}

	// Not yet resolved. One in-flight resolution per cwd; switching cwd
	// cancels the obsolete resolution and starts a fresh one.
	if s.git.rootBusy && s.git.rootResolvingCWD == cwd {
		return
	}
	if s.git.rootCancel != nil {
		s.git.rootCancel()
		s.git.rootCancel = nil
	}
	s.git.rootBusy = true
	s.git.rootResolvingCWD = cwd
	if s.git.root != "" {
		s.git.root = ""
		s.resetGitContextState()
	}

	ctx, cancel := context.WithCancel(context.Background())
	s.git.rootCancel = cancel
	gen := s.git.rootGen.Add(1)

	go func() {
		root, err := s.git.runner.ResolveRoot(ctx, cwd)
		if ctx.Err() != nil {
			return
		}
		s.applyGuarded(s.git.rootGen.Load, gen, func() {
			s.git.rootBusy = false
			s.git.rootResolvingCWD = ""
			if err != nil {
				// Not a repository (or Git missing): record the negative
				// result so the empty state is stable and quiet.
				if ge, ok := gitworkbench.GitError(err); !ok || ge.Class != gitworkbench.ErrNotARepo {
					slog.Debug("git root resolve failed", "class", err)
				}
				s.git.rootOf[cwd] = ""
				if s.git.root == "" {
					return
				}
				s.git.root = ""
				s.resetGitContextState()
				return
			}
			s.git.rootOf[cwd] = root
			if s.selectedTabCWD() != cwd {
				return
			}
			s.git.root = root
			s.resetGitContextState()
			s.ensureGitSnapshot(false)
			s.loadBranches()
		})
	}()
}

// resetGitContextState clears transient presentation state on context change.
func (s *Shell) resetGitContextState() {
	s.git.snapshot = nil
	s.git.staleBanner = false
	s.git.gdFiles = nil
	s.git.gdFilesFor = nil
	s.git.gdRows = nil
	s.git.gdRowsDirty = true
	s.git.gdList = ui.ListState{}
	s.git.gdListEl = nil
	s.git.gdCurrent = 0
	s.git.gdSelFile, s.git.gdSelHunk = -1, -1
	s.git.gdHScroll = map[string]float32{}
	s.git.gdFileMatches = map[int]bool{}
	s.git.gdFinding, s.git.gdQuery = false, ""
	s.git.gdMatches, s.git.gdMatchesFor = nil, ""
	s.git.gdMatch = 0
	s.git.branches = nil
	s.git.preflight = nil
	s.git.preflightSnap = nil
	s.changesTree = nil
}

// setHighlightStyle follows the app theme (light/dark).
func (s *Shell) setHighlightStyle(dark bool) {
	if s.git == nil {
		return
	}
	if dark {
		s.git.highlighter.SetStyle("github-dark")
	} else {
		s.git.highlighter.SetStyle("github")
	}
}

// workspaceContext derives the current WorkspaceContextKey.
func (s *Shell) workspaceContext() WorkspaceContextKey {
	root := ""
	if s.git != nil {
		root = s.git.root
	}
	return WorkspaceContextKey{
		InstanceID: s.activeInstance,
		ProjectID:  s.selectedProjectID,
		TabID:      s.selectedTabID,
		RepoRoot:   root,
	}
}

// gitRepoName returns the repository directory name for the header.
func (s *Shell) gitRepoName() string {
	if s.git == nil || s.git.root == "" {
		return ""
	}
	return filepath.Base(s.git.root)
}

// gitSnapshot returns the current change snapshot, if any.
func (s *Shell) gitSnapshot() *gitworkbench.ChangesSnapshot {
	if s.git == nil {
		return nil
	}
	return s.git.snapshot
}

// dirtyWorktree reports whether the current snapshot has any changes.
func (s *Shell) dirtyWorktree() bool {
	snap := s.gitSnapshot()
	return snap != nil && len(snap.Files) > 0
}

// selectedChangeFile returns the ChangeFile record for the diff selection.
func (s *Shell) selectedChangeFile() (gitworkbench.ChangeFile, bool) {
	if s.git == nil || s.git.snapshot == nil || s.surface.diff.SelectedPath == "" {
		return gitworkbench.ChangeFile{}, false
	}
	return s.git.snapshot.ByPath(s.surface.diff.SelectedPath)
}
