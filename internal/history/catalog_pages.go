package history

import (
	"database/sql"
	"encoding/json"
	"fmt"
)

// transcriptPageMetaPayload is the JSON blob stored in
// transcript_page_meta.payload: normalized metadata plus the sidechain and
// unknown-line facts needed to present the window without re-parsing.
type transcriptPageMetaPayload struct {
	Meta             SessionMeta     `json:"meta"`
	Sidechains       []SidechainInfo `json:"sidechains"`
	UnknownLineCount uint32          `json:"unknown_line_count"`
}

// CacheTranscript is the single transcript cache write: page-addressable meta
// + fixed 64-message pages + the seq→message_index lookup for one source
// identity. The full transcript object is dropped by the caller afterwards.
func (c *Catalog) CacheTranscript(source SessionFileRef, transcript ParsedTranscript) error {
	metaPayload, err := json.Marshal(transcriptPageMetaPayload{
		Meta:             transcript.Meta,
		Sidechains:       transcript.Sidechains,
		UnknownLineCount: transcript.UnknownLineCount,
	})
	if err != nil {
		return fmt.Errorf("encode page meta: %w", err)
	}

	tx, err := c.db.Begin()
	if err != nil {
		return fmt.Errorf("begin cache write: %w", err)
	}
	defer tx.Rollback()

	if _, err := tx.Exec(`INSERT INTO transcript_page_meta (
		session_key, agent, native_id, file_path, mtime_ms, size_bytes, message_count, payload
	) VALUES (?,?,?,?,?,?,?,?)
	ON CONFLICT(session_key) DO UPDATE SET
		agent=excluded.agent, native_id=excluded.native_id, file_path=excluded.file_path,
		mtime_ms=excluded.mtime_ms, size_bytes=excluded.size_bytes,
		message_count=excluded.message_count, payload=excluded.payload`,
		transcript.Meta.Key, string(source.Agent), source.NativeID, source.FilePath,
		source.MtimeMS, source.SizeBytes, len(transcript.Mainline), metaPayload,
	); err != nil {
		return fmt.Errorf("upsert page meta: %w", err)
	}
	if _, err := tx.Exec("DELETE FROM transcript_page_cache WHERE session_key = ?", transcript.Meta.Key); err != nil {
		return fmt.Errorf("clear pages: %w", err)
	}
	if _, err := tx.Exec("DELETE FROM transcript_message_index WHERE session_key = ?", transcript.Meta.Key); err != nil {
		return fmt.Errorf("clear seq index: %w", err)
	}

	for pageIndex := 0; pageIndex*TranscriptCachePageSize < len(transcript.Mainline); pageIndex++ {
		end := (pageIndex + 1) * TranscriptCachePageSize
		if end > len(transcript.Mainline) {
			end = len(transcript.Mainline)
		}
		page, err := json.Marshal(transcript.Mainline[pageIndex*TranscriptCachePageSize : end])
		if err != nil {
			return fmt.Errorf("encode page: %w", err)
		}
		if _, err := tx.Exec(
			"INSERT INTO transcript_page_cache (session_key, page_index, payload) VALUES (?,?,?)",
			transcript.Meta.Key, pageIndex, page,
		); err != nil {
			return fmt.Errorf("insert page: %w", err)
		}
	}
	for index, message := range transcript.Mainline {
		if _, err := tx.Exec(
			"INSERT OR IGNORE INTO transcript_message_index (session_key, seq, message_index) VALUES (?,?,?)",
			transcript.Meta.Key, message.Seq, index,
		); err != nil {
			return fmt.Errorf("insert seq index: %w", err)
		}
	}
	return tx.Commit()
}

func (c *Catalog) clearTranscriptPageCache(sessionKey string) {
	_, _ = c.db.Exec("DELETE FROM transcript_page_cache WHERE session_key = ?", sessionKey)
	_, _ = c.db.Exec("DELETE FROM transcript_message_index WHERE session_key = ?", sessionKey)
	_, _ = c.db.Exec("DELETE FROM transcript_page_meta WHERE session_key = ?", sessionKey)
}

// CachedTranscriptWindow reads one bounded window from the page cache after
// verifying the full source identity. A nil result is a cache miss (unknown
// session, changed source, or corrupt pages, which are cleared as disposable).
func (c *Catalog) CachedTranscriptWindow(sessionKey string, source SessionFileRef, start, limit int) (*CachedTranscriptWindow, error) {
	var messageCount int64
	var metaPayload []byte
	err := c.db.QueryRow(
		`SELECT message_count, payload FROM transcript_page_meta
		 WHERE session_key = ?1 AND agent = ?2 AND native_id = ?3
		   AND file_path = ?4 AND mtime_ms = ?5 AND size_bytes = ?6`,
		sessionKey, string(source.Agent), source.NativeID, source.FilePath,
		source.MtimeMS, source.SizeBytes,
	).Scan(&messageCount, &metaPayload)
	if err == sql.ErrNoRows {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("read page meta: %w", err)
	}

	var payload transcriptPageMetaPayload
	if err := json.Unmarshal(metaPayload, &payload); err != nil {
		c.clearTranscriptPageCache(sessionKey)
		return nil, nil
	}
	total := int(messageCount)
	if total < 0 {
		total = 0
	}
	if start > total {
		start = total
	}
	if limit <= 0 || start == total {
		return &CachedTranscriptWindow{
			Meta: payload.Meta, TotalMessages: total, Start: start,
			Messages:         []TranscriptMessage{},
			Sidechains:       payload.Sidechains,
			UnknownLineCount: payload.UnknownLineCount,
		}, nil
	}
	end := start + limit
	if end > total {
		end = total
	}
	firstPage := start / TranscriptCachePageSize
	lastPage := (end - 1) / TranscriptCachePageSize

	// All page payloads are collected before any validation clear: the
	// single-connection pool cannot run the cleanup Execs while the page
	// query is still open.
	type rawPage struct {
		index   int
		payload []byte
	}
	rows, err := c.db.Query(
		"SELECT page_index, payload FROM transcript_page_cache "+
			"WHERE session_key = ? AND page_index BETWEEN ? AND ? ORDER BY page_index ASC",
		sessionKey, firstPage, lastPage,
	)
	if err != nil {
		return nil, fmt.Errorf("read pages: %w", err)
	}
	var pages []rawPage
	for rows.Next() {
		page := rawPage{}
		if err := rows.Scan(&page.index, &page.payload); err != nil {
			rows.Close()
			return nil, err
		}
		pages = append(pages, page)
	}
	if err := rows.Err(); err != nil {
		rows.Close()
		return nil, err
	}
	rows.Close()

	messages := make([]TranscriptMessage, 0, end-start)
	for _, page := range pages {
		var decoded []TranscriptMessage
		if err := json.Unmarshal(page.payload, &decoded); err != nil {
			c.clearTranscriptPageCache(sessionKey)
			return nil, nil
		}
		pageStart := page.index * TranscriptCachePageSize
		for offset, message := range decoded {
			index := pageStart + offset
			if index >= start && index < end {
				messages = append(messages, message)
			}
		}
	}
	if len(messages) != end-start {
		c.clearTranscriptPageCache(sessionKey)
		return nil, nil
	}
	return &CachedTranscriptWindow{
		Meta:             payload.Meta,
		TotalMessages:    total,
		Start:            start,
		Messages:         messages,
		Sidechains:       payload.Sidechains,
		UnknownLineCount: payload.UnknownLineCount,
	}, nil
}

// CachedTranscriptIndexForSeq resolves an authoritative FTS seq to the
// normalized message index inside the cached transcript.
func (c *Catalog) CachedTranscriptIndexForSeq(sessionKey string, source SessionFileRef, seq int64) (int, bool, error) {
	var index int64
	err := c.db.QueryRow(
		`SELECT i.message_index
		 FROM transcript_message_index i
		 JOIN transcript_page_meta m ON m.session_key = i.session_key
		 WHERE i.session_key = ?1 AND i.seq = ?2
		   AND m.agent = ?3 AND m.native_id = ?4 AND m.file_path = ?5
		   AND m.mtime_ms = ?6 AND m.size_bytes = ?7`,
		sessionKey, seq, string(source.Agent), source.NativeID, source.FilePath,
		source.MtimeMS, source.SizeBytes,
	).Scan(&index)
	if err == sql.ErrNoRows {
		return 0, false, nil
	}
	if err != nil {
		return 0, false, fmt.Errorf("resolve seq index: %w", err)
	}
	if index < 0 {
		return 0, false, nil
	}
	return int(index), true, nil
}
