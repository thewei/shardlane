package history

/**
 * [INPUT]: 依赖内存 Catalog 的元数据行与 Provider 原始会话文件（经 parseByAgent 现读）
 * [OUTPUT]: 对外提供 HistoryService 的列表/筛选/阅读查询与增量扫描入口
 * [POS]: 历史领域服务层；Project/Provider 条件在限额之前生效；Open 每次直接读取原始会话文件，不持久化任何会话正文
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

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
// AnchorSeq (message jump) or a StartIndex (paging) may be supplied; without
// either, the window starts at the first message — or, with Tail, ends at the
// newest one (the chat-style entry that pages upward with StartIndex).
type ConversationWindowRequest struct {
	ConversationID string
	AnchorSeq      *int64
	StartIndex     *int
	Limit          int
	Tail           bool
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
// catalog and the provider sources. It never mutates provider files and
// never stores conversation content: every transcript read goes to the
// original session file.
type HistoryService struct {
	catalog *Catalog
	scanner *Scanner
	parse   func(reference SessionFileRef) (ParsedTranscript, error)
}

func NewHistoryService(catalog *Catalog, roots []SourceRoot) *HistoryService {
	return &HistoryService{catalog: catalog, scanner: NewScanner(catalog, roots), parse: parseByAgent}
}

// Catalog exposes the underlying catalog for the scanner caller and tests.
func (s *HistoryService) Catalog() *Catalog { return s.catalog }

// Scan reconciles the catalog with the provider sources (background lane).
func (s *HistoryService) Scan(ctx context.Context) (ScanResult, error) {
	return s.scanner.Scan(ctx)
}

// AvailableProviders lists the catalog's actual Provider identities without
// imposing the interactive list page limit; this is a metadata-only read.
func (s *HistoryService) AvailableProviders(ctx context.Context) ([]AgentID, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return s.catalog.AvailableProviders(), nil
}

// ProviderCounts lists each catalog Provider with its session total; like
// AvailableProviders it is a metadata-only read without the list page limit.
func (s *HistoryService) ProviderCounts(ctx context.Context) ([]ProviderCount, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return s.catalog.ProviderCounts(), nil
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
		return s.catalog.SearchMetadata(q.Search, paths, agents, q.Limit), nil
	}
	if q.ProjectPath != "" || q.Provider != "" {
		return s.catalog.ListFilteredSessions(q.ProjectPath, q.Provider, q.Limit), nil
	}
	return s.catalog.ListSessions(q.Limit), nil
}

// Recent returns the most recent sessions for the Sidebar Recent section:
// metadata only, no transcript parse.
func (s *HistoryService) Recent(ctx context.Context, limit int) ([]SessionSummary, error) {
	return s.catalog.ListSessions(limit), nil
}

// Open reads the original session file and materializes one bounded
// transcript window from it. Nothing is cached on disk or reused between
// calls, so what the window shows is always what the provider file holds now.
func (s *HistoryService) Open(ctx context.Context, req ConversationWindowRequest) (TranscriptWindow, error) {
	if err := ctx.Err(); err != nil {
		return TranscriptWindow{}, err
	}
	limit := req.Limit
	if limit <= 0 || limit > WindowLimit {
		limit = WindowLimit
	}
	meta := s.catalog.Session(req.ConversationID)
	if meta == nil {
		return TranscriptWindow{}, fmt.Errorf("unknown conversation %q", req.ConversationID)
	}
	if meta.FilePath == "" || meta.ID == "" || meta.Agent == "" {
		return TranscriptWindow{}, fmt.Errorf("conversation %q has no readable source", req.ConversationID)
	}
	transcript, err := s.parse(SessionFileRef{
		Agent:     meta.Agent,
		NativeID:  meta.ID,
		FilePath:  meta.FilePath,
		MtimeMS:   meta.MtimeMS,
		SizeBytes: meta.SizeBytes,
	})
	if err != nil {
		return TranscriptWindow{}, fmt.Errorf("read conversation %q: %w", meta.Key, err)
	}
	if err := ctx.Err(); err != nil {
		return TranscriptWindow{}, err
	}

	messages := transcript.Mainline
	total := len(messages)
	start := 0
	if req.Tail {
		start = max(0, total-limit)
	}
	anchorIndex := -1
	if req.AnchorSeq != nil {
		for index, message := range messages {
			if message.Seq == *req.AnchorSeq {
				anchorIndex = index
				start = max(0, anchorIndex-(limit-13))
				break
			}
		}
	}
	if req.StartIndex != nil {
		start = max(0, *req.StartIndex)
		anchorIndex = -1
	}
	start = min(start, total)
	end := min(start+limit, total)

	return TranscriptWindow{
		Key:           meta.Key,
		Meta:          transcript.Meta,
		Start:         start,
		TotalMessages: total,
		Messages:      messages[start:end:end],
		AnchorIndex:   anchorIndex,
	}, nil
}
