package history

/**
 * [INPUT]: 依赖 SQLite 会话元数据表、项目归一化键与只读 FTS 索引
 * [OUTPUT]: 提供 Catalog 的有界会话列表、Project/Provider 筛选与 metadata 搜索
 * [POS]: History 查询存储层；筛选、排序和 LIMIT 在数据库内同一查询完成
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"fmt"
	"strings"
)

// CachedTranscriptWindow is one bounded, source-identity-verified transcript
// window read from the page cache.
type CachedTranscriptWindow struct {
	Meta             SessionMeta
	TotalMessages    int
	Start            int
	Messages         []TranscriptMessage
	Sidechains       []SidechainInfo
	UnknownLineCount uint32
}

const sessionColumns = "key, native_id, agent, title, project_path, project_key, project_name, " +
	"file_path, created_at, updated_at, message_count, size_bytes, git_branch, model, " +
	"tokens_used, archived, source, mtime_ms, description"

// ListSessions returns the most recently updated non-archived sessions with
// their description previews.
func (c *Catalog) ListSessions(limit int) ([]SessionSummary, error) {
	if limit <= 0 {
		return []SessionSummary{}, nil
	}
	rows, err := c.db.Query(
		"SELECT "+sessionColumns+" FROM sessions WHERE archived = 0 ORDER BY updated_at DESC LIMIT ?", limit)
	if err != nil {
		return nil, fmt.Errorf("list sessions: %w", err)
	}
	defer rows.Close()
	return collectSessionSummaries(rows)
}

// AvailableProviders returns all Provider identities in the read-only catalog,
// not just the first page of recent conversations.
func (c *Catalog) AvailableProviders() ([]AgentID, error) {
	rows, err := c.db.Query(
		"SELECT agent FROM sessions WHERE archived = 0 AND agent != '' " +
			"GROUP BY agent ORDER BY MAX(updated_at) DESC, agent ASC")
	if err != nil {
		return nil, fmt.Errorf("list available providers: %w", err)
	}
	defer rows.Close()
	providers := make([]AgentID, 0, 8)
	for rows.Next() {
		var agent AgentID
		if err := rows.Scan(&agent); err != nil {
			return nil, fmt.Errorf("scan available providers: %w", err)
		}
		providers = append(providers, agent)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("iterate available providers: %w", err)
	}
	return providers, nil
}

// ListFilteredSessions applies Project and Provider scopes in SQL BEFORE the
// bounded limit. Filtering a globally limited page would hide older matching
// provider conversations (UI-006).
func (c *Catalog) ListFilteredSessions(projectPath string, provider AgentID, limit int) ([]SessionSummary, error) {
	if limit <= 0 {
		return []SessionSummary{}, nil
	}
	conditions := []string{"archived = 0"}
	values := make([]any, 0, 3)
	if projectPath != "" {
		key := NormalizedProjectKey(projectPath)
		if key == "" {
			return []SessionSummary{}, nil
		}
		conditions = append(conditions, "project_key = ?")
		values = append(values, key)
	}
	if provider != "" {
		conditions = append(conditions, "agent = ?")
		values = append(values, string(provider))
	}
	values = append(values, limit)
	rows, err := c.db.Query(
		"SELECT "+sessionColumns+" FROM sessions WHERE "+
			strings.Join(conditions, " AND ")+" ORDER BY updated_at DESC, key ASC LIMIT ?", values...)
	if err != nil {
		return nil, fmt.Errorf("list filtered sessions: %w", err)
	}
	defer rows.Close()
	return collectSessionSummaries(rows)
}

// SessionsForProject returns (total, page) for one normalized project scope.
func (c *Catalog) SessionsForProject(projectPath string, limit int) (int, []SessionSummary, error) {
	projectKey := NormalizedProjectKey(projectPath)
	if projectKey == "" {
		return 0, []SessionSummary{}, nil
	}
	var total int
	if err := c.db.QueryRow(
		"SELECT COUNT(*) FROM sessions WHERE archived = 0 AND project_key = ?", projectKey,
	).Scan(&total); err != nil {
		return 0, nil, fmt.Errorf("count project sessions: %w", err)
	}
	if limit <= 0 {
		return total, []SessionSummary{}, nil
	}
	rows, err := c.db.Query(
		"SELECT "+sessionColumns+" FROM sessions WHERE archived = 0 AND project_key = ? "+
			"ORDER BY updated_at DESC, key ASC LIMIT ?", projectKey, limit)
	if err != nil {
		return 0, nil, fmt.Errorf("list project sessions: %w", err)
	}
	defer rows.Close()
	sessions, err := collectSessionSummaries(rows)
	return total, sessions, err
}

// Session fetches one session by key regardless of archived state.
func (c *Catalog) Session(key string) (*SessionMeta, error) {
	if key == "" {
		return nil, nil
	}
	return queryOneSession(c.db, "SELECT "+sessionColumns+" FROM sessions WHERE key = ?", key)
}

// SearchMetadata filters sessions by title/path/agent metadata terms.
func (c *Catalog) SearchMetadata(query string, projectPaths []string, agents []AgentID, limit int) ([]SessionMeta, error) {
	if limit <= 0 {
		return []SessionMeta{}, nil
	}
	terms := searchTerms(query)
	projectKeys := normalizedProjectKeys(projectPaths)
	if len(projectPaths) > 0 && len(projectKeys) == 0 {
		return []SessionMeta{}, nil
	}
	if len(terms) == 0 && len(projectKeys) == 0 && len(agents) == 0 {
		return []SessionMeta{}, nil
	}

	conditions := []string{"archived = 0"}
	values := []any{}
	for _, t := range terms {
		conditions = append(conditions,
			"(title LIKE ? ESCAPE '\\' OR project_path LIKE ? ESCAPE '\\' OR project_name LIKE ? ESCAPE '\\' "+
				"OR native_id LIKE ? ESCAPE '\\' OR file_path LIKE ? ESCAPE '\\' OR agent LIKE ? ESCAPE '\\')")
		for range 6 {
			values = append(values, "%"+escapeLike(t)+"%")
		}
	}
	if len(projectKeys) > 0 {
		conditions = append(conditions, "project_key IN ("+placeholders(len(projectKeys))+")")
		for _, key := range projectKeys {
			values = append(values, key)
		}
	}
	if len(agents) > 0 {
		conditions = append(conditions, "agent IN ("+placeholders(len(agents))+")")
		for _, agent := range agents {
			values = append(values, string(agent))
		}
	}
	values = append(values, limit)

	rows, err := c.db.Query(
		"SELECT "+sessionColumns+" FROM sessions WHERE "+
			strings.Join(conditions, " AND ")+" ORDER BY updated_at DESC LIMIT ?", values...)
	if err != nil {
		return nil, fmt.Errorf("search session metadata: %w", err)
	}
	defer rows.Close()
	summaries, err := collectSessionSummaries(rows)
	if err != nil {
		return nil, err
	}
	metas := make([]SessionMeta, 0, len(summaries))
	for _, summary := range summaries {
		metas = append(metas, summary.Meta)
	}
	return metas, nil
}

// SearchScoped searches message bodies through FTS (with a LIKE fallback for
// short terms), matching the Rust ordering and scope semantics.
func (c *Catalog) SearchScoped(query string, projectPaths []string, agents []AgentID, limit int) ([]SearchHit, error) {
	terms := searchTerms(query)
	if len(terms) == 0 || limit <= 0 {
		return []SearchHit{}, nil
	}
	for _, t := range terms {
		runes := []rune(t)
		if len(runes) == 1 && !isCJKSearchChar(runes[0]) {
			return []SearchHit{}, nil
		}
	}
	projectKeys := normalizedProjectKeys(projectPaths)
	if len(projectPaths) > 0 && len(projectKeys) == 0 {
		return []SearchHit{}, nil
	}
	for _, t := range terms {
		if len([]rune(t)) < 3 {
			return c.searchLike(terms, projectKeys, agents, limit)
		}
	}
	return c.searchFTS(terms, projectKeys, agents, limit)
}

func (c *Catalog) searchFTS(terms []string, projectKeys []string, agents []AgentID, limit int) ([]SearchHit, error) {
	quoted := make([]string, 0, len(terms))
	for _, t := range terms {
		quoted = append(quoted, `"`+strings.ReplaceAll(t, `"`, `""`)+`"`)
	}
	conditions := []string{"s.archived = 0", "message_fts MATCH ?"}
	values := []any{strings.Join(quoted, " AND ")}
	if len(projectKeys) > 0 {
		conditions = append(conditions, "s.project_key IN ("+placeholders(len(projectKeys))+")")
		for _, key := range projectKeys {
			values = append(values, key)
		}
	}
	if len(agents) > 0 {
		conditions = append(conditions, "s.agent IN ("+placeholders(len(agents))+")")
		for _, agent := range agents {
			values = append(values, string(agent))
		}
	}
	values = append(values, limit)

	rows, err := c.db.Query(
		"SELECT "+aliasedSessionColumns("s")+", f.seq, f.role, "+
			"snippet(message_fts, 4, '', '', ' … ', 28), f.timestamp "+
			"FROM message_fts f JOIN sessions s ON s.key = f.session_key "+
			"WHERE "+strings.Join(conditions, " AND ")+" "+
			"ORDER BY bm25(message_fts), s.updated_at DESC LIMIT ?", values...)
	if err != nil {
		return nil, fmt.Errorf("fts search: %w", err)
	}
	defer rows.Close()
	return collectSearchHits(rows)
}

func (c *Catalog) searchLike(terms []string, projectKeys []string, agents []AgentID, limit int) ([]SearchHit, error) {
	conditions := []string{"s.archived = 0"}
	values := []any{}
	for _, t := range terms {
		conditions = append(conditions, "f.text LIKE ? ESCAPE '\\'")
		values = append(values, "%"+escapeLike(t)+"%")
	}
	if len(projectKeys) > 0 {
		conditions = append(conditions, "s.project_key IN ("+placeholders(len(projectKeys))+")")
		for _, key := range projectKeys {
			values = append(values, key)
		}
	}
	if len(agents) > 0 {
		conditions = append(conditions, "s.agent IN ("+placeholders(len(agents))+")")
		for _, agent := range agents {
			values = append(values, string(agent))
		}
	}
	values = append(values, limit)

	rows, err := c.db.Query(
		"SELECT "+aliasedSessionColumns("s")+", f.seq, f.role, f.text, f.timestamp "+
			"FROM message_fts f JOIN sessions s ON s.key = f.session_key "+
			"WHERE "+strings.Join(conditions, " AND ")+" ORDER BY s.updated_at DESC LIMIT ?", values...)
	if err != nil {
		return nil, fmt.Errorf("like search: %w", err)
	}
	defer rows.Close()
	return collectSearchHits(rows)
}

func searchTerms(query string) []string {
	return strings.Fields(strings.TrimSpace(query))
}

func isCJKSearchChar(ch rune) bool {
	switch {
	case ch >= 0x3400 && ch <= 0x4DBF,
		ch >= 0x4E00 && ch <= 0x9FFF,
		ch >= 0xF900 && ch <= 0xFAFF,
		ch >= 0x3040 && ch <= 0x309F,
		ch >= 0x30A0 && ch <= 0x30FF,
		ch >= 0x31F0 && ch <= 0x31FF,
		ch >= 0xAC00 && ch <= 0xD7AF:
		return true
	}
	return false
}

func escapeLike(term string) string {
	return strings.NewReplacer(`\`, `\\`, `%`, `\%`, `_`, `\_`).Replace(term)
}

func placeholders(n int) string {
	return strings.TrimSuffix(strings.Repeat("?,", n), ",")
}
