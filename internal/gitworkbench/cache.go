package gitworkbench

import (
	"context"
	"log/slog"
	"sync"
	"time"
)

// cacheEntry is one root's stale-while-refresh slot. A stale snapshot stays
// visible while a refresh runs; a refresh whose context (root) changed can
// still land in the cache but is flagged so the UI never swaps it into a
// foreign context (plan §36).
type cacheEntry struct {
	snapshot    *ChangesSnapshot
	generatedAt time.Time
	refreshing  bool
}

// Cache is the single per-process Git change cache, keyed by repo root.
// It supersedes internal/gitintel; no second Git cache may exist alongside.
type Cache struct {
	runner  *Runner
	mu      sync.Mutex
	entries map[string]*cacheEntry
	// Inflight dedupes concurrent refreshes of one root.
	inflight map[string]*refreshCall
}

type refreshCall struct {
	done chan struct{}
	snap *ChangesSnapshot
	err  error
}

// NewCache builds the cache over one runner.
func NewCache(runner *Runner) *Cache {
	return &Cache{
		runner:   runner,
		entries:  map[string]*cacheEntry{},
		inflight: map[string]*refreshCall{},
	}
}

// Get returns the cached snapshot and whether it is fresh enough to show
// without a refresh banner (StaleAfter since generation).
func (c *Cache) Get(root string) (*ChangesSnapshot, bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	entry := c.entries[root]
	if entry == nil {
		return nil, false
	}
	fresh := c.runner.now().Sub(entry.generatedAt) < StaleAfter
	return entry.snapshot, fresh
}

// NeedsRefresh reports whether a background refresh is worth starting.
func (c *Cache) NeedsRefresh(root string) bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	entry := c.entries[root]
	if entry == nil {
		return true
	}
	return c.runner.now().Sub(entry.generatedAt) >= StaleAfter && !entry.refreshing
}

// StaleAfter is the stale-while-refresh threshold.
const StaleAfter = 30 * time.Second

// Refresh acquires a new snapshot for root. Concurrent refreshes of one root
// share one call; the loser waits and receives the same result. The snapshot
// always lands in the cache; callers must still verify Root/Signature match
// their current context before swapping visible state.
func (c *Cache) Refresh(ctx context.Context, root string) (*ChangesSnapshot, error) {
	c.mu.Lock()
	if call, ok := c.inflight[root]; ok {
		c.mu.Unlock()
		select {
		case <-call.done:
			return call.snap, call.err
		case <-ctx.Done():
			return nil, ctx.Err()
		}
	}
	call := &refreshCall{done: make(chan struct{})}
	c.inflight[root] = call
	if entry := c.entries[root]; entry != nil {
		entry.refreshing = true
	}
	c.mu.Unlock()

	defer func() {
		c.mu.Lock()
		delete(c.inflight, root)
		if entry := c.entries[root]; entry != nil {
			entry.refreshing = false
		}
		c.mu.Unlock()
		close(call.done)
	}()

	started := time.Now()
	snap, err := c.runner.Snapshot(ctx, root)
	if err != nil && snap == nil {
		call.snap, call.err = nil, err
		return nil, err
	}
	c.mu.Lock()
	c.entries[root] = &cacheEntry{snapshot: snap, generatedAt: c.runner.now()}
	c.mu.Unlock()
	slog.Debug("gitworkbench refresh", "op_class", "read", "op", "snapshot", "root", root, "files", len(snap.Files), "ms", time.Since(started).Milliseconds())
	call.snap, call.err = snap, err
	return snap, err
}

// Invalidate drops one root (after mutations the caller refreshes directly).
func (c *Cache) Invalidate(root string) {
	c.mu.Lock()
	delete(c.entries, root)
	c.mu.Unlock()
}
