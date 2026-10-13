package history

/**
 * [INPUT]: 依赖内存 Catalog 的会话元数据行与项目归一化键
 * [OUTPUT]: 提供 Catalog 的有界会话列表、Project/Provider 筛选、Provider 计数与 metadata 搜索
 * [POS]: History 查询层；筛选、排序和 LIMIT 在同一遍历内完成，只读元数据，永不触碰会话正文
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"sort"
	"strings"
)

// rows returns the non-archived rows accepted by match, most recently
// updated first (key ascending breaks ties). Rows are immutable once stored,
// so callers may read them after the lock is released.
func (c *Catalog) rows(match func(*catalogEntry) bool) []*catalogEntry {
	c.mu.RLock()
	defer c.mu.RUnlock()
	matched := make([]*catalogEntry, 0, len(c.entries))
	for _, entry := range c.entries {
		if !entry.meta.Archived && match(entry) {
			matched = append(matched, entry)
		}
	}
	sort.Slice(matched, func(i, j int) bool {
		if matched[i].meta.UpdatedAt != matched[j].meta.UpdatedAt {
			return matched[i].meta.UpdatedAt > matched[j].meta.UpdatedAt
		}
		return matched[i].meta.Key < matched[j].meta.Key
	})
	return matched
}

func summaries(rows []*catalogEntry, limit int) []SessionSummary {
	if limit < len(rows) {
		rows = rows[:max(limit, 0)]
	}
	out := make([]SessionSummary, 0, len(rows))
	for _, row := range rows {
		out = append(out, SessionSummary{Meta: row.meta, Description: row.description})
	}
	return out
}

func matchAll(*catalogEntry) bool { return true }

// ListSessions returns the most recently updated non-archived sessions with
// their description previews.
func (c *Catalog) ListSessions(limit int) []SessionSummary {
	return summaries(c.rows(matchAll), limit)
}

// ProviderCount is one Provider's non-archived session total.
type ProviderCount struct {
	Agent AgentID
	Count int
}

// ProviderCounts returns every Provider in the catalog with its session
// total, most recently active first — the metadata-only source of the
// Sessions tab counts, independent of the interactive list limit.
func (c *Catalog) ProviderCounts() []ProviderCount {
	latest := map[AgentID]int64{}
	counts := map[AgentID]int{}
	for _, row := range c.rows(func(e *catalogEntry) bool { return e.meta.Agent != "" }) {
		counts[row.meta.Agent]++
		latest[row.meta.Agent] = max(latest[row.meta.Agent], row.meta.UpdatedAt)
	}
	out := make([]ProviderCount, 0, len(counts))
	for agent, count := range counts {
		out = append(out, ProviderCount{Agent: agent, Count: count})
	}
	sort.Slice(out, func(i, j int) bool {
		if latest[out[i].Agent] != latest[out[j].Agent] {
			return latest[out[i].Agent] > latest[out[j].Agent]
		}
		return out[i].Agent < out[j].Agent
	})
	return out
}

// AvailableProviders returns all Provider identities in the catalog, not
// just the first page of recent conversations.
func (c *Catalog) AvailableProviders() []AgentID {
	counts := c.ProviderCounts()
	providers := make([]AgentID, 0, len(counts))
	for _, entry := range counts {
		providers = append(providers, entry.Agent)
	}
	return providers
}

// ListFilteredSessions applies Project and Provider scopes BEFORE the bounded
// limit. Filtering a globally limited page would hide older matching
// provider conversations (UI-006).
func (c *Catalog) ListFilteredSessions(projectPath string, provider AgentID, limit int) []SessionSummary {
	projectKey := ""
	if projectPath != "" {
		if projectKey = NormalizedProjectKey(projectPath); projectKey == "" {
			return []SessionSummary{}
		}
	}
	return summaries(c.rows(func(e *catalogEntry) bool {
		return (projectKey == "" || e.projectKey == projectKey) && (provider == "" || e.meta.Agent == provider)
	}), limit)
}

// Session fetches one session by key regardless of archived state.
func (c *Catalog) Session(key string) *SessionMeta {
	c.mu.RLock()
	defer c.mu.RUnlock()
	entry, ok := c.entries[key]
	if !ok {
		return nil
	}
	meta := entry.meta
	return &meta
}

// SearchMetadata filters sessions by title/path/agent metadata terms: every
// term must match one of the fields, case-insensitively.
func (c *Catalog) SearchMetadata(query string, projectPaths []string, agents []AgentID, limit int) []SessionSummary {
	terms := searchTerms(query)
	projectKeys := normalizedProjectKeys(projectPaths)
	if limit <= 0 || len(projectPaths) > 0 && len(projectKeys) == 0 ||
		len(terms) == 0 && len(projectKeys) == 0 && len(agents) == 0 {
		return []SessionSummary{}
	}
	return summaries(c.rows(func(e *catalogEntry) bool {
		if len(projectKeys) > 0 && !contains(projectKeys, e.projectKey) {
			return false
		}
		if len(agents) > 0 && !contains(agents, e.meta.Agent) {
			return false
		}
		fields := []string{e.meta.Title, e.meta.ProjectPath, e.meta.ProjectName, e.meta.ID, e.meta.FilePath, string(e.meta.Agent)}
		for _, term := range terms {
			matched := false
			for _, field := range fields {
				if strings.Contains(strings.ToLower(field), term) {
					matched = true
					break
				}
			}
			if !matched {
				return false
			}
		}
		return true
	}), limit)
}

// searchTerms splits the query into lower-cased terms.
func searchTerms(query string) []string {
	return strings.Fields(strings.ToLower(strings.TrimSpace(query)))
}

func normalizedProjectKeys(projectPaths []string) []string {
	keys := make([]string, 0, len(projectPaths))
	for _, path := range projectPaths {
		if key := NormalizedProjectKey(path); key != "" && !contains(keys, key) {
			keys = append(keys, key)
		}
	}
	return keys
}

func contains[T comparable](items []T, want T) bool {
	for _, item := range items {
		if item == want {
			return true
		}
	}
	return false
}
