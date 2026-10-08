package history

import (
	"database/sql"
	"fmt"
	"strings"
)

// sessionRow holds the nullable holders for one sessions-table projection and
// assembles the Go model after the scan. It is shared by list/search collectors.
type sessionRow struct {
	agent       string
	projectKey  string
	gitBranch   sql.NullString
	model       sql.NullString
	source      sql.NullString
	tokensUsed  sql.NullInt64
	mtimeMS     sql.NullInt64
	meta        SessionMeta
	description string
}

func (r *sessionRow) dest() []any {
	return []any{&r.meta.Key, &r.meta.ID, &r.agent, &r.meta.Title, &r.meta.ProjectPath,
		&r.projectKey, &r.meta.ProjectName, &r.meta.FilePath, &r.meta.CreatedAt,
		&r.meta.UpdatedAt, &r.meta.MessageCount, &r.meta.SizeBytes, &r.gitBranch,
		&r.model, &r.tokensUsed, &r.meta.Archived, &r.source, &r.mtimeMS, &r.description}
}

func (r *sessionRow) assemble() (SessionMeta, string) {
	if r.gitBranch.Valid {
		value := r.gitBranch.String
		r.meta.GitBranch = &value
	}
	if r.model.Valid {
		value := r.model.String
		r.meta.Model = &value
	}
	if r.tokensUsed.Valid {
		value := r.tokensUsed.Int64
		r.meta.TokensUsed = &value
	}
	if r.source.Valid {
		value := r.source.String
		r.meta.Source = &value
	}
	if r.mtimeMS.Valid {
		r.meta.MtimeMS = r.mtimeMS.Int64
	}
	r.meta.Agent = AgentID(r.agent)
	return r.meta, r.description
}

// aliasedSessionColumns is the session projection for JOIN queries.
func aliasedSessionColumns(alias string) string {
	columns := strings.Split(sessionColumns, ", ")
	for index, column := range columns {
		columns[index] = alias + "." + column
	}
	return strings.Join(columns, ", ")
}

func normalizedProjectKeys(projectPaths []string) []string {
	seen := make(map[string]bool, len(projectPaths))
	keys := make([]string, 0, len(projectPaths))
	for _, path := range projectPaths {
		key := NormalizedProjectKey(path)
		if key == "" || seen[key] {
			continue
		}
		seen[key] = true
		keys = append(keys, key)
	}
	return keys
}

func collectSessionSummaries(rows *sql.Rows) ([]SessionSummary, error) {
	summaries := []SessionSummary{}
	for rows.Next() {
		row := sessionRow{}
		if err := rows.Scan(row.dest()...); err != nil {
			return nil, err
		}
		meta, description := row.assemble()
		summaries = append(summaries, SessionSummary{Meta: meta, Description: description})
	}
	return summaries, rows.Err()
}

func queryOneSession(db *sql.DB, query string, args ...any) (*SessionMeta, error) {
	row := sessionRow{}
	err := db.QueryRow(query, args...).Scan(row.dest()...)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("load session: %w", err)
	}
	meta, _ := row.assemble()
	return &meta, nil
}

func collectSearchHits(rows *sql.Rows) ([]SearchHit, error) {
	hits := []SearchHit{}
	for rows.Next() {
		row := sessionRow{}
		hit := SearchHit{}
		var role string
		dest := append(row.dest(), &hit.Seq, &role, &hit.Snippet, &hit.Timestamp)
		if err := rows.Scan(dest...); err != nil {
			return nil, err
		}
		meta, _ := row.assemble()
		hit.Session = meta
		hit.Role = role
		hits = append(hits, hit)
	}
	return hits, rows.Err()
}
