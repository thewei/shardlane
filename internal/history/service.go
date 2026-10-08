package history

import (
	"context"
	"fmt"
)

// HistoryQuery is one read-only catalog list request.
type HistoryQuery struct {
	ProjectPath string
	Provider    AgentID
	Search      string
	Limit       int
}

// ConversationWindowRequest asks for one bounded transcript window. Either an
// AnchorSeq (search-target jump) or a StartIndex (paging) may be supplied.
type ConversationWindowRequest struct {
	ConversationID string
	AnchorSeq      *int64
	StartIndex     *int
	Limit          int
}

// TranscriptWindow is the UI-facing bounded transcript projection. The amount
// of materialized messages is bounded regardless of the source size.
type TranscriptWindow struct {
	Key           string
	Meta          SessionMeta
	Start         int
	TotalMessages int
	Messages      []TranscriptMessage
	AnchorIndex   int
}

// HasEarlier reports whether messages exist before this window.
func (w TranscriptWindow) HasEarlier() bool { return w.Start > 0 }

// HasLater reports whether messages exist after this window.
func (w TranscriptWindow) HasLater() bool { return w.Start+len(w.Messages) < w.TotalMessages }

// WindowLimit is the maximum number of messages the UI may materialize.
const WindowLimit = 60

// HistoryService is the UI-independent read-only history facade over the
// catalog and the provider sources. It never mutates provider files.
type HistoryService struct {
	catalog *Catalog
	scanner *Scanner
}

func NewHistoryService(catalog *Catalog, roots []SourceRoot) *HistoryService {
	return &HistoryService{catalog: catalog, scanner: NewScanner(catalog, roots)}
}

// Catalog exposes the underlying catalog for the scanner caller and tests.
func (s *HistoryService) Catalog() *Catalog { return s.catalog }

// Scan reconciles the catalog with the provider sources (background lane).
func (s *HistoryService) Scan(ctx context.Context) (ScanResult, error) {
	return s.scanner.Scan(ctx)
}

// List returns bounded session metadata. It never parses transcripts.
func (s *HistoryService) List(ctx context.Context, q HistoryQuery) ([]SessionSummary, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if q.Search != "" {
		agents := []AgentID(nil)
		if q.Provider != "" {
			agents = []AgentID{q.Provider}
		}
		paths := []string(nil)
		if q.ProjectPath != "" {
			paths = []string{q.ProjectPath}
		}
		metas, err := s.catalog.SearchMetadata(q.Search, paths, agents, q.Limit)
		if err != nil {
			return nil, err
		}
		summaries := make([]SessionSummary, 0, len(metas))
		for _, meta := range metas {
			summaries = append(summaries, SessionSummary{Meta: meta})
		}
		return summaries, nil
	}
	if q.ProjectPath != "" {
		_, summaries, err := s.catalog.SessionsForProject(q.ProjectPath, q.Limit)
		return summaries, err
	}
	summaries, err := s.catalog.ListSessions(q.Limit)
	if err != nil {
		return nil, err
	}
	if q.Provider == "" {
		return summaries, nil
	}
	filtered := make([]SessionSummary, 0, len(summaries))
	for _, summary := range summaries {
		if summary.Meta.Agent == q.Provider {
			filtered = append(filtered, summary)
		}
	}
	return filtered, nil
}

// Recent returns the most recent sessions for the Sidebar Recent section:
// metadata only, no transcript parse.
func (s *HistoryService) Recent(ctx context.Context, limit int) ([]SessionSummary, error) {
	return s.catalog.ListSessions(limit)
}

// Search returns bounded message-body hits.
func (s *HistoryService) Search(ctx context.Context, query string, projectPath string, limit int) ([]SearchHit, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	paths := []string(nil)
	if projectPath != "" {
		paths = []string{projectPath}
	}
	return s.catalog.SearchScoped(query, paths, nil, limit)
}

// Open materializes one bounded transcript window. Page-cache hits stay
// metadata-only; a miss parses the source once, prewarms the cache, then
// re-reads the window so UI state never holds a full transcript.
func (s *HistoryService) Open(ctx context.Context, req ConversationWindowRequest) (TranscriptWindow, error) {
	if err := ctx.Err(); err != nil {
		return TranscriptWindow{}, err
	}
	limit := req.Limit
	if limit <= 0 || limit > WindowLimit {
		limit = WindowLimit
	}
	meta, err := s.catalog.Session(req.ConversationID)
	if err != nil {
		return TranscriptWindow{}, err
	}
	if meta == nil {
		return TranscriptWindow{}, fmt.Errorf("unknown conversation %q", req.ConversationID)
	}
	source, ok := s.sourceRef(*meta)
	if !ok {
		return TranscriptWindow{}, fmt.Errorf("conversation %q has no readable source", req.ConversationID)
	}

	start := 0
	anchorIndex := -1
	if req.AnchorSeq != nil {
		index, found, err := s.catalog.CachedTranscriptIndexForSeq(meta.Key, source, *req.AnchorSeq)
		if err != nil {
			return TranscriptWindow{}, err
		}
		if found {
			anchorIndex = index
			start = anchorIndex - (limit - 13)
			if start < 0 {
				start = 0
			}
		}
	}
	if req.StartIndex != nil {
		start = max(0, *req.StartIndex)
		anchorIndex = -1
	}

	window, err := s.catalog.CachedTranscriptWindow(meta.Key, source, start, limit)
	if err != nil {
		return TranscriptWindow{}, err
	}
	if window == nil {
		if err := s.parseIntoCache(ctx, source, *meta); err != nil {
			return TranscriptWindow{}, err
		}
		if window, err = s.catalog.CachedTranscriptWindow(meta.Key, source, start, limit); err != nil {
			return TranscriptWindow{}, err
		}
		if window == nil {
			return TranscriptWindow{}, fmt.Errorf("transcript cache rebuild failed for %q", meta.Key)
		}
		if req.AnchorSeq != nil && anchorIndex < 0 {
			if index, found, err := s.catalog.CachedTranscriptIndexForSeq(meta.Key, source, *req.AnchorSeq); err != nil {
				return TranscriptWindow{}, err
			} else if found {
				anchorIndex = index
				start = max(0, anchorIndex-(limit-13))
				if window, err = s.catalog.CachedTranscriptWindow(meta.Key, source, start, limit); err != nil {
					return TranscriptWindow{}, err
				}
			}
		}
	}
	if window == nil {
		return TranscriptWindow{}, fmt.Errorf("transcript window unavailable for %q", meta.Key)
	}
	return TranscriptWindow{
		Key:           meta.Key,
		Meta:          window.Meta,
		Start:         window.Start,
		TotalMessages: window.TotalMessages,
		Messages:      window.Messages,
		AnchorIndex:   anchorIndex,
	}, nil
}

// sourceRef rebuilds the source identity facts stored with the session row.
func (s *HistoryService) sourceRef(meta SessionMeta) (SessionFileRef, bool) {
	if meta.FilePath == "" || meta.ID == "" || meta.Agent == "" {
		return SessionFileRef{}, false
	}
	var mtimeMS, sizeBytes int64
	if err := s.catalog.db.QueryRow(
		"SELECT mtime_ms, size_bytes FROM sessions WHERE key = ?", meta.Key,
	).Scan(&mtimeMS, &sizeBytes); err != nil {
		return SessionFileRef{}, false
	}
	return SessionFileRef{
		Agent:     meta.Agent,
		NativeID:  meta.ID,
		FilePath:  meta.FilePath,
		MtimeMS:   mtimeMS,
		SizeBytes: sizeBytes,
	}, true
}

// parseIntoCache is the page-cache-miss compatibility path: one complete
// adapter parse, cached, then dropped.
func (s *HistoryService) parseIntoCache(ctx context.Context, source SessionFileRef, meta SessionMeta) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	transcript, err := parseByAgent(source)
	if err != nil {
		return fmt.Errorf("parse conversation %q: %w", meta.Key, err)
	}
	return s.catalog.CacheTranscript(source, transcript)
}
