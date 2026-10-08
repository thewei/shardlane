package history

import (
	"database/sql"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	// The pure-Go SQLite driver backs the disposable Shardlane catalog.
	_ "modernc.org/sqlite"
)

// TranscriptCachePageSize is the fixed page size of the Shardlane-owned
// page-addressable transcript cache.
const TranscriptCachePageSize = 64

// DescriptionMaxChars bounds the session description preview.
const DescriptionMaxChars = 240

// Catalog is the Shardlane-owned disposable SQLite/FTS index over read-only
// provider history. It never mutates external agent stores.
type Catalog struct {
	db *sql.DB
}

// OpenCatalog opens (creating if needed) the catalog database at path.
func OpenCatalog(path string) (*Catalog, error) {
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return nil, fmt.Errorf("create catalog directory: %w", err)
	}
	dsn := "file:" + path + "?_pragma=foreign_keys(1)&_pragma=busy_timeout(2000)&_pragma=journal_mode(WAL)"
	db, err := sql.Open("sqlite", dsn)
	if err != nil {
		return nil, fmt.Errorf("open catalog: %w", err)
	}
	db.SetMaxOpenConns(1)
	catalog := &Catalog{db: db}
	if err := catalog.createSchema(); err != nil {
		db.Close()
		return nil, err
	}
	return catalog, nil
}

func (c *Catalog) createSchema() error {
	_, err := c.db.Exec(`
CREATE TABLE IF NOT EXISTS sessions (
	key TEXT PRIMARY KEY,
	native_id TEXT NOT NULL,
	agent TEXT NOT NULL,
	title TEXT NOT NULL,
	project_path TEXT NOT NULL,
	project_key TEXT NOT NULL DEFAULT '',
	project_name TEXT NOT NULL,
	file_path TEXT NOT NULL UNIQUE,
	created_at INTEGER NOT NULL,
	updated_at INTEGER NOT NULL,
	message_count INTEGER NOT NULL,
	size_bytes INTEGER NOT NULL,
	git_branch TEXT,
	model TEXT,
	tokens_used INTEGER,
	archived INTEGER NOT NULL DEFAULT 0,
	source TEXT,
	mtime_ms INTEGER NOT NULL,
	description TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS sessions_updated_at ON sessions(updated_at DESC);
CREATE INDEX IF NOT EXISTS sessions_agent ON sessions(agent);
CREATE INDEX IF NOT EXISTS sessions_agent_native ON sessions(agent, native_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS sessions_project_key ON sessions(project_key, updated_at DESC);
CREATE TABLE IF NOT EXISTS transcript_page_meta (
	session_key TEXT PRIMARY KEY REFERENCES sessions(key) ON DELETE CASCADE,
	agent TEXT NOT NULL,
	native_id TEXT NOT NULL,
	file_path TEXT NOT NULL,
	mtime_ms INTEGER NOT NULL,
	size_bytes INTEGER NOT NULL,
	message_count INTEGER NOT NULL,
	payload BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS transcript_page_cache (
	session_key TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
	page_index INTEGER NOT NULL,
	payload BLOB NOT NULL,
	PRIMARY KEY(session_key, page_index)
);
CREATE TABLE IF NOT EXISTS transcript_message_index (
	session_key TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
	seq INTEGER NOT NULL,
	message_index INTEGER NOT NULL,
	PRIMARY KEY(session_key, seq)
);
CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
	session_key UNINDEXED,
	seq UNINDEXED,
	role UNINDEXED,
	timestamp UNINDEXED,
	text,
	tokenize='trigram'
);`)
	if err != nil {
		return fmt.Errorf("create catalog schema: %w", err)
	}
	return nil
}

func (c *Catalog) Close() error {
	return c.db.Close()
}

// KnownFiles maps catalog file paths to the stored source mtime, the change
// signal for the scanner.
func (c *Catalog) KnownFiles() (map[string]int64, error) {
	rows, err := c.db.Query("SELECT file_path, mtime_ms FROM sessions")
	if err != nil {
		return nil, fmt.Errorf("list known files: %w", err)
	}
	defer rows.Close()
	known := make(map[string]int64)
	for rows.Next() {
		var path string
		var mtime int64
		if err := rows.Scan(&path, &mtime); err != nil {
			return nil, err
		}
		known[path] = mtime
	}
	return known, rows.Err()
}

// NormalizedProjectKey preserves spelling while normalizing components; it
// never touches the filesystem.
func NormalizedProjectKey(path string) string {
	trimmed := strings.TrimSpace(path)
	if trimmed == "" {
		return ""
	}
	return NormalizePathKey(trimmed)
}

// sessionDescriptionFromUnits builds the list-page preview: first user
// message, whitespace-collapsed, Unicode-safe truncation.
func sessionDescriptionFromUnits(units []IndexUnit) string {
	var candidate *IndexUnit
	for index := range units {
		if units[index].Role == RoleUser && strings.TrimSpace(units[index].Text) != "" {
			candidate = &units[index]
			break
		}
	}
	if candidate == nil {
		for index := range units {
			if strings.TrimSpace(units[index].Text) != "" {
				candidate = &units[index]
				break
			}
		}
	}
	if candidate == nil {
		return ""
	}
	var collapsed strings.Builder
	pendingSpace := false
	count := 0
	for _, ch := range candidate.Text {
		if ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r' || ch == '\v' || ch == '\f' || ch == 0x85 || ch == 0xA0 {
			pendingSpace = collapsed.Len() > 0
			continue
		}
		if count >= DescriptionMaxChars {
			collapsed.WriteByte(0xE2)
			collapsed.WriteByte(0x80)
			collapsed.WriteByte(0xA6)
			break
		}
		if pendingSpace {
			collapsed.WriteByte(' ')
			pendingSpace = false
		}
		collapsed.WriteRune(ch)
		count++
	}
	return collapsed.String()
}

// WriteSession upserts session metadata and reindexes FTS units. A changed
// source identity invalidates the page meta/index together so stale pages can
// never be presented for new content.
func (c *Catalog) WriteSession(meta SessionMeta, mtimeMS int64, units []IndexUnit) error {
	projectKey := NormalizedProjectKey(meta.ProjectPath)
	description := sessionDescriptionFromUnits(units)

	tx, err := c.db.Begin()
	if err != nil {
		return fmt.Errorf("begin session write: %w", err)
	}
	defer tx.Rollback()

	if _, err := tx.Exec(`INSERT INTO sessions (
		key, native_id, agent, title, project_path, project_key, project_name, file_path,
		created_at, updated_at, message_count, size_bytes, git_branch, model,
		tokens_used, archived, source, mtime_ms, description
	) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
	ON CONFLICT(key) DO UPDATE SET
		native_id=excluded.native_id, agent=excluded.agent, title=excluded.title,
		project_path=excluded.project_path, project_key=excluded.project_key,
		project_name=excluded.project_name, file_path=excluded.file_path,
		created_at=excluded.created_at, updated_at=excluded.updated_at,
		message_count=excluded.message_count, size_bytes=excluded.size_bytes,
		git_branch=excluded.git_branch, model=excluded.model,
		tokens_used=excluded.tokens_used, archived=excluded.archived,
		source=excluded.source, mtime_ms=excluded.mtime_ms, description=excluded.description`,
		meta.Key, meta.ID, string(meta.Agent), meta.Title, meta.ProjectPath, projectKey,
		meta.ProjectName, meta.FilePath, meta.CreatedAt, meta.UpdatedAt, meta.MessageCount,
		meta.SizeBytes, meta.GitBranch, meta.Model, meta.TokensUsed, meta.Archived,
		meta.Source, mtimeMS, description,
	); err != nil {
		return fmt.Errorf("upsert session: %w", err)
	}

	const identityStale = `session_key = ?1 AND EXISTS (
		SELECT 1 FROM transcript_page_meta
		WHERE session_key = ?1
		  AND (agent != ?2 OR native_id != ?3 OR file_path != ?4 OR mtime_ms != ?5 OR size_bytes != ?6))`
	if _, err := tx.Exec("DELETE FROM transcript_page_cache WHERE "+identityStale,
		meta.Key, string(meta.Agent), meta.ID, meta.FilePath, mtimeMS, meta.SizeBytes); err != nil {
		return fmt.Errorf("stale page cleanup: %w", err)
	}
	if _, err := tx.Exec("DELETE FROM transcript_message_index WHERE "+identityStale,
		meta.Key, string(meta.Agent), meta.ID, meta.FilePath, mtimeMS, meta.SizeBytes); err != nil {
		return fmt.Errorf("stale index cleanup: %w", err)
	}
	if _, err := tx.Exec(`DELETE FROM transcript_page_meta
		WHERE session_key = ?1
		  AND (agent != ?2 OR native_id != ?3 OR file_path != ?4 OR mtime_ms != ?5 OR size_bytes != ?6)`,
		meta.Key, string(meta.Agent), meta.ID, meta.FilePath, mtimeMS, meta.SizeBytes); err != nil {
		return fmt.Errorf("stale meta cleanup: %w", err)
	}
	if _, err := tx.Exec("DELETE FROM message_fts WHERE session_key = ?", meta.Key); err != nil {
		return fmt.Errorf("fts cleanup: %w", err)
	}
	for _, unit := range units {
		if _, err := tx.Exec(
			"INSERT INTO message_fts (session_key, seq, role, timestamp, text) VALUES (?,?,?,?,?)",
			meta.Key, unit.Seq, string(unit.Role), unit.Timestamp, unit.Text,
		); err != nil {
			return fmt.Errorf("fts insert: %w", err)
		}
	}
	return tx.Commit()
}

// RemoveMissing drops catalog rows whose file paths were not observed by the
// last scan. Unobserved roots never reach here (cleanup only follows a
// complete observation).
func (c *Catalog) RemoveMissing(seenPaths map[string]bool) (int, error) {
	rows, err := c.db.Query("SELECT DISTINCT file_path FROM sessions")
	if err != nil {
		return 0, fmt.Errorf("list files for cleanup: %w", err)
	}
	var missing []string
	for rows.Next() {
		var path string
		if err := rows.Scan(&path); err != nil {
			rows.Close()
			return 0, err
		}
		if !seenPaths[path] {
			missing = append(missing, path)
		}
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return 0, err
	}
	if len(missing) == 0 {
		return 0, nil
	}

	tx, err := c.db.Begin()
	if err != nil {
		return 0, err
	}
	defer tx.Rollback()
	for _, path := range missing {
		var key sql.NullString
		if err := tx.QueryRow("SELECT key FROM sessions WHERE file_path = ?", path).Scan(&key); err != nil && err != sql.ErrNoRows {
			return 0, err
		}
		if key.Valid {
			if _, err := tx.Exec("DELETE FROM message_fts WHERE session_key = ?", key.String); err != nil {
				return 0, err
			}
		}
		if _, err := tx.Exec("DELETE FROM sessions WHERE file_path = ?", path); err != nil {
			return 0, err
		}
	}
	if err := tx.Commit(); err != nil {
		return 0, err
	}
	return len(missing), nil
}
