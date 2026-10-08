package nativeui

import (
	"context"
	"log/slog"
	"sync/atomic"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

// RecentFeedLimit bounds the Sidebar Recent section.
const RecentFeedLimit = 8

// HistoryListView is the slice of the read-only HistoryService the shell
// consumes. The narrow seam keeps the presentation testable with
// deterministic fakes.
type HistoryListView interface {
	List(ctx context.Context, q history.HistoryQuery) ([]history.SessionSummary, error)
	Open(ctx context.Context, req history.ConversationWindowRequest) (history.TranscriptWindow, error)
	Recent(ctx context.Context, limit int) ([]history.SessionSummary, error)
	Scan(ctx context.Context) (history.ScanResult, error)
}

// historyState is the Shell's presentation state for the Native History
// surfaces. All runtime facts come from the read-only HistoryService; the
// state only tracks requests in flight and the bounded windows they returned.
type historyState struct {
	service HistoryListView
	cancel  context.CancelFunc

	query    string
	provider string
	// providers lists the agent kinds actually present in the last
	// unfiltered (All) load — the filter rail is derived from it instead of
	// a hardcoded trio that hid every other provider (2026-10-06 F37).
	providers []history.AgentID
	loaded    bool
	loading   bool
	errText   string
	sessions  []history.SessionSummary

	// listState owns the persistent native List behavior for the session
	// list (RENDER-01): stable keys, selection and keyboard navigation.
	listState    ui.ListState
	listSelected int

	detail *historyDetail

	listGen    atomic.Uint64
	listCancel context.CancelFunc
	openGen    atomic.Uint64
	openCancel context.CancelFunc
}

type historyDetail struct {
	key     string
	meta    history.SessionMeta
	window  history.TranscriptWindow
	loading bool
	errText string
	// expanded tracks per-block presentation disclosure state (thinking).
	expanded map[string]bool

	// scroll owns the detail's explicit scroll position (CLOSURE-03);
	// paging intents set the position the next frame should show.
	scroll        ui.ScrollState
	anchorPending bool
	scrollToTop   bool
	scrollToBot   bool
}

// dispatch runs fn on a background lane in the real app and synchronously in
// headless tests (no window yet), keeping one code path for callers.
func (s *Shell) dispatch(fn func()) {
	if s.win == nil {
		fn()
		return
	}
	go fn()
}

// requestHistoryList loads the bounded session list for the latest
// filter/search intent (CLOSURE-01 latest-request-wins): a new intent
// cancels the previous request, starts a new generation, and queries the
// latest filters immediately. Only the newest generation may apply, so a
// stale late result can never overwrite the newer answer. The apply also
// feeds the Sidebar Recent section (metadata only).
func (s *Shell) requestHistoryList() {
	service := s.hist.service
	if service == nil {
		return
	}
	if s.hist.listCancel != nil {
		s.hist.listCancel()
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.hist.listCancel = cancel
	s.hist.loading = true
	s.hist.errText = ""
	generation := s.hist.listGen.Add(1)
	query := history.HistoryQuery{
		ProjectPath: "",
		Provider:    history.AgentID(s.hist.provider),
		Search:      s.hist.query,
		Limit:       100,
	}
	s.dispatch(func() {
		summaries, err := service.List(ctx, query)
		s.applyOnWindow(func() {
			if generation != s.hist.listGen.Load() {
				// Stale generation: the request was superseded. Touch nothing.
				return
			}
			s.hist.loading = false
			if err != nil {
				s.hist.errText = err.Error()
				return
			}
			s.hist.loaded = true
			s.hist.sessions = summaries
			// Nothing is selected until the user selects it: the zero value
			// 0 pre-highlighted the first row as if chosen (2026-10-06 F15).
			s.hist.listSelected = -1
			if s.hist.provider == "" {
				s.hist.providers = distinctAgents(summaries)
			}
			s.feedRecentFromHistory(summaries)
		})
	})
}

// feedRecentFromHistory updates the Sidebar Recent rows from session
// metadata. It never parses transcript files.
func (s *Shell) feedRecentFromHistory(summaries []history.SessionSummary) {
	items := make([]RecentItem, 0, RecentFeedLimit)
	for _, summary := range summaries {
		if len(items) >= RecentFeedLimit {
			break
		}
		title := summary.Meta.Title
		if title == "" {
			title = history.Untitled
		}
		items = append(items, RecentItem{
			ID:        summary.Meta.Key,
			Title:     title,
			Subtitle:  summary.Meta.ProjectName,
			Kind:      string(summary.Meta.Agent),
			UpdatedAt: time.UnixMilli(summary.Meta.UpdatedAt),
		})
	}
	s.recentItems = items
}

// openHistoryConversation starts (or switches) the bounded detail window.
// A newer request cancels the obsolete one and its stale application.
func (s *Shell) openHistoryConversation(key string) {
	if s.hist.service == nil || key == "" {
		return
	}
	if s.hist.detail != nil && s.hist.detail.key == key && (s.hist.detail.loading || s.hist.detail.errText == "" && s.hist.detail.window.Key != "") {
		return
	}
	if s.hist.openCancel != nil {
		s.hist.openCancel()
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.hist.openCancel = cancel
	generation := s.hist.openGen.Add(1)
	s.hist.detail = &historyDetail{key: key, loading: true}
	service := s.hist.service

	s.dispatch(func() {
		window, err := service.Open(ctx, history.ConversationWindowRequest{ConversationID: key})
		s.applyOnWindow(func() {
			if generation != s.hist.openGen.Load() {
				return
			}
			detail := &historyDetail{key: key}
			if err != nil {
				detail.errText = err.Error()
			} else {
				detail.meta = window.Meta
				detail.window = window
				detail.anchorPending = window.AnchorIndex >= 0
			}
			s.hist.detail = detail
		})
	})
}

// pageHistoryDetail loads the adjacent bounded window with a 12-message
// overlap, replacing the current window in place.
func (s *Shell) pageHistoryDetail(earlier bool) {
	detail := s.hist.detail
	if detail == nil || detail.loading || s.hist.service == nil {
		return
	}
	current := detail.window
	const overlap = 12
	var start int
	if earlier {
		start = current.Start - (len(current.Messages) - overlap)
	} else {
		start = current.Start + (len(current.Messages) - overlap)
	}
	if start < 0 {
		start = 0
	}
	generation := s.hist.openGen.Add(1)
	detail.loading = true
	service := s.hist.service
	key := detail.key

	s.dispatch(func() {
		window, err := service.Open(context.Background(), history.ConversationWindowRequest{
			ConversationID: key,
			StartIndex:     &start,
		})
		s.applyOnWindow(func() {
			if generation != s.hist.openGen.Load() || s.hist.detail == nil || s.hist.detail.key != key {
				return
			}
			s.hist.detail.loading = false
			if err != nil {
				s.hist.detail.errText = err.Error()
				return
			}
			s.hist.detail.window = window
			// Intentional paging position (CLOSURE-03): earlier reveals new
			// content above (show the window top), later reveals new content
			// below (follow the end).
			s.hist.detail.scrollToTop = earlier
			s.hist.detail.scrollToBot = !earlier
		})
	})
}

// cancelHistoryWork drops obsolete in-flight history requests (conversation
// switch, route leave). Generation guards keep stale results from applying.
func (s *Shell) cancelHistoryDetailRequest() {
	if s.hist.openCancel != nil {
		s.hist.openCancel()
		s.hist.openCancel = nil
	}
	s.hist.openGen.Add(1)
	if s.hist.detail != nil {
		s.hist.detail = nil
	}
}

// refreshHistoryScan cancels any running scan, reconciles the catalog once,
// and reloads the visible list.
func (s *Shell) refreshHistoryScan() {
	if s.hist.service == nil {
		return
	}
	s.stopHistoryScanKeepState()
	s.startHistoryScan()
	s.hist.loaded = false
	s.hist.loading = false
	s.requestHistoryList()
}

// stopHistory cancels background scanner work and any in-flight list request
// at window close.
func (s *Shell) stopHistory() {
	s.stopHistoryScanKeepState()
	if s.hist.listCancel != nil {
		s.hist.listCancel()
		s.hist.listCancel = nil
	}
	if s.hist.openCancel != nil {
		s.hist.openCancel()
		s.hist.openCancel = nil
	}
}

func (s *Shell) stopHistoryScanKeepState() {
	if s.hist.cancel != nil {
		s.hist.cancel()
		s.hist.cancel = nil
	}
}

// startHistoryScan runs one background catalog reconciliation after the
// window attaches. There is no idle polling; the History page refresh action
// reruns it on demand.
func (s *Shell) startHistoryScan() {
	if s.hist.service == nil || s.win == nil {
		return
	}
	if s.hist.cancel != nil {
		return
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.hist.cancel = cancel
	service := s.hist.service
	go func() {
		result, err := service.Scan(ctx)
		if err != nil && ctx.Err() == nil {
			slog.Warn("history scan failed", "error", err)
			return
		}
		if err == nil {
			slog.Info("history scan finished", "scanned", result.Scanned, "changed", result.Changed, "removed", result.Removed)
		}
		s.applyOnWindow(func() {
			if ctx.Err() == nil {
				s.hist.loaded = false
			}
		})
	}()
}

// applyOnWindow marshals a state mutation onto the window update lane, or
// runs it inline when no window exists (headless tests).
func (s *Shell) applyOnWindow(apply func()) {
	if s.win == nil {
		apply()
		return
	}
	s.win.Update(apply)
}
