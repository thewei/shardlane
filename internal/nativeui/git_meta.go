package nativeui

import (
	"context"
	"sync"
	"time"

	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// gitMeta keeps the per-repository facts the sidebar Agent rows show
// (branch, diff additions/deletions/file count). It never queries Git on
// the render path: facts refresh in background lanes triggered by
// projection updates or row binds, results land under a mutex, and the
// next frame reads the snapshot. Freshness shares the right panel's
// snapshot cache, so the sidebar can never disagree with Changes.
type gitMetaEntry struct {
	branch   string
	adds     int
	dels     int
	files    int
	resolved bool // a repository was resolved and facts were read
	inflight bool
	nextTry  time.Time
}

const (
	gitMetaTTL      = 15 * time.Second
	gitMetaRetryTTL = 2 * time.Minute
)

type gitMetaService struct {
	mu           sync.Mutex
	entries      map[string]gitMetaEntry
	pendingRoots map[string]bool
}

func newGitMetaService() *gitMetaService {
	return &gitMetaService{
		entries:      map[string]gitMetaEntry{},
		pendingRoots: map[string]bool{},
	}
}

// get returns the cached facts for a root, if any were resolved.
func (g *gitMetaService) get(root string) (gitMetaEntry, bool) {
	g.mu.Lock()
	defer g.mu.Unlock()
	e, ok := g.entries[root]
	return e, ok && e.resolved
}

// refreshGitMeta schedules fact refreshes for every repository the
// sidebar currently shows (update-path caller).
func (s *Shell) refreshGitMeta() {
	if s.git == nil || s.git.meta == nil || s.win == nil {
		return
	}
	for _, agent := range s.projection.Agents {
		if agent.CWD != "" {
			s.ensureGitMetaForCWD(agent.CWD)
		}
	}
	if cwd := s.selectedTabCWD(); cwd != "" {
		s.ensureGitMetaForCWD(cwd)
	}
}

// ensureGitMetaForCWD resolves cwd → root (once, in the background) and
// then refreshes that root's facts when stale.
func (s *Shell) ensureGitMetaForCWD(cwd string) {
	if cwd == "" {
		return
	}
	root := s.git.rootOf[cwd]
	if root == "" {
		s.resolveGitRootAsync(cwd)
		return
	}
	s.ensureGitMeta(root)
}

// ensureGitMeta schedules a background fact refresh for one root when the
// cached copy is stale; callers stay on the UI lane.
func (s *Shell) ensureGitMeta(root string) {
	if s.git == nil || s.git.meta == nil || root == "" || s.win == nil {
		return
	}
	s.git.meta.mu.Lock()
	entry, ok := s.git.meta.entries[root]
	if ok && (entry.inflight || time.Now().Before(entry.nextTry)) {
		s.git.meta.mu.Unlock()
		return
	}
	entry.inflight = true
	entry.nextTry = time.Now().Add(gitMetaTTL)
	s.git.meta.entries[root] = entry
	s.git.meta.mu.Unlock()

	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 6*gitworkbench.ReadTimeout)
		defer cancel()
		snap, err := s.git.cache.Refresh(ctx, root)
		s.applyOnUI(func() {
			s.git.meta.mu.Lock()
			defer s.git.meta.mu.Unlock()
			entry := s.git.meta.entries[root]
			entry.inflight = false
			if err != nil || snap == nil {
				// Not a repository (or Git failed): keep the row quiet and
				// retry only after the backoff.
				entry.resolved = false
				entry.nextTry = time.Now().Add(gitMetaRetryTTL)
				s.git.meta.entries[root] = entry
				return
			}
			entry.branch = snap.Branch
			entry.adds = snap.TotalAdditions
			entry.dels = snap.TotalDeletions
			entry.files = len(snap.Files)
			entry.resolved = true
			entry.nextTry = time.Now().Add(gitMetaTTL)
			s.git.meta.entries[root] = entry
		})
	}()
}

// gitMetaForCWD resolves the display facts for a Pane cwd through its
// repository root; ok is false until the background lane produced them.
func (s *Shell) gitMetaForCWD(cwd string) (branch string, adds, dels, files int, ok bool) {
	if s.git == nil || cwd == "" {
		return "", 0, 0, 0, false
	}
	root := s.git.rootOf[cwd]
	if root == "" {
		return "", 0, 0, 0, false
	}
	entry, ok := s.git.meta.get(root)
	if !ok {
		return "", 0, 0, 0, false
	}
	return entry.branch, entry.adds, entry.dels, entry.files, true
}

// resolveGitRootAsync resolves cwd → repository root once, in the
// background, recording the answer in git.rootOf (shared with the right
// panel's context resolution).
func (s *Shell) resolveGitRootAsync(cwd string) {
	if s.git == nil || s.win == nil || cwd == "" {
		return
	}
	s.git.meta.mu.Lock()
	pending := s.git.meta.pendingRoots[cwd]
	if !pending {
		s.git.meta.pendingRoots[cwd] = true
	}
	s.git.meta.mu.Unlock()
	if pending {
		return
	}
	go func() {
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		root, err := s.git.runner.ResolveRoot(ctx, cwd)
		s.applyOnUI(func() {
			s.git.meta.mu.Lock()
			delete(s.git.meta.pendingRoots, cwd)
			s.git.meta.mu.Unlock()
			if err != nil {
				return // not a repository; stay quiet
			}
			s.git.rootOf[cwd] = root
			s.ensureGitMetaForCWD(cwd)
		})
	}()
}
