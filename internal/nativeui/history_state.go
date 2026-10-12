package nativeui

/**
 * [INPUT]: 依赖 HistoryService 的只读列表/详情查询与 MyGo 窗口调度
 * [OUTPUT]: 提供 History 查询代际隔离、筛选重置、列表/分组/多选/行内预览的瞬态状态、各 Provider 会话总数与扫描的分批上报（每批约 100 个来源刷新一次可见列表）
 * [POS]: History 页面状态编排层；查询/筛选不改变 Herdr Runtime 或来源历史文件
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

import (
	"context"
	"log/slog"
	"strings"
	"sync/atomic"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

// RecentFeedLimit bounds the Sidebar Recent section.
const RecentFeedLimit = 8

// HistoryListLimit bounds each interactive History result set. A full page
// means "latest N", not the exact total size of the catalog.
const HistoryListLimit = 100

// HistoryListView is the slice of the read-only HistoryService the shell
// consumes. The narrow seam keeps the presentation testable with
// deterministic fakes.
type HistoryListView interface {
	List(ctx context.Context, q history.HistoryQuery) ([]history.SessionSummary, error)
	Open(ctx context.Context, req history.ConversationWindowRequest) (history.TranscriptWindow, error)
	Recent(ctx context.Context, limit int) ([]history.SessionSummary, error)
	Scan(ctx context.Context) (history.ScanResult, error)
}

// HistoryProviderView is optional for test fakes; the production service
// obtains the full catalog's Provider choices without truncating to 100 rows.
type HistoryProviderView interface {
	AvailableProviders(ctx context.Context) ([]history.AgentID, error)
}

// HistoryProviderCountView is optional for test fakes; the production service
// reports each Provider's catalog-wide session total for the Sessions tabs.
type HistoryProviderCountView interface {
	ProviderCounts(ctx context.Context) ([]history.ProviderCount, error)
}

// HistoryScanProgressView is optional for test fakes; the production service
// calls onBatch after every indexed batch, and the shell reloads the visible
// list on each one so thousands of session sources surface batch by batch
// instead of holding the page on the loading state until the round ends.
type HistoryScanProgressView interface {
	ScanWithProgress(ctx context.Context, onBatch func()) (history.ScanResult, error)
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
	// counts is each Provider's catalog-wide session total, refreshed with
	// every list request (it never depends on the search or tab scope).
	counts   map[history.AgentID]int
	loaded   bool
	loading  bool
	errText  string
	sessions []history.SessionSummary
	// scanning is true while a background catalog scan runs: the catalog is
	// rebuilt in memory on every launch, so an empty list during that time
	// means "still reading", not "no sessions".
	scanning bool

	// listState owns the persistent native List behavior for the session
	// list (RENDER-01): stable keys, selection and keyboard navigation.
	listState    ui.ListState
	listSelected int

	// N19 #9: the grouped Sessions view state — the user's explicit
	// open/closed choice per project (absent: the default, the latest
	// project open) and the recycle toggle.
	groupOpen  map[string]bool
	showHidden bool

	// Multi-select (history_select.go): selecting is the mode, selected the
	// session keys ticked in it. Transient — filters and Done clear both.
	selecting bool
	selected  map[string]bool

	// preview is the one inline session preview open in the list
	// (history_preview.go); previewGen/previewCancel make a newer open win.
	preview       historyPreview
	previewGen    atomic.Uint64
	previewCancel context.CancelFunc

	// Continue (history_continue.go): the plan awaiting the user's confirm,
	// whether a confirmed one is still running, and the runtime seam tests
	// replace.
	pendingContinue *continueRequest
	continuing      bool
	continuation    continuationRuntime

	// viewFrame counts Shell.View calls and detailFrame is the frame the
	// session detail page last built in; the read-only chat binding is
	// released once the page stops building (history_detail.go).
	viewFrame   uint64
	detailFrame uint64

	listGen    atomic.Uint64
	listCancel context.CancelFunc
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
		Search:      strings.TrimSpace(s.hist.query),
		Limit:       HistoryListLimit,
	}
	s.dispatch(func() {
		summaries, err := service.List(ctx, query)
		var counts map[history.AgentID]int
		if source, ok := service.(HistoryProviderCountView); ok && err == nil {
			if totals, lookupErr := source.ProviderCounts(ctx); lookupErr == nil {
				counts = make(map[history.AgentID]int, len(totals))
				for _, total := range totals {
					counts[total.Agent] = total.Count
				}
			} else if ctx.Err() == nil {
				slog.Warn("history provider counts unavailable", "error", lookupErr)
			}
		}
		var providers []history.AgentID
		if err == nil && query.Provider == "" && query.Search == "" {
			providers = distinctAgents(summaries)
			if source, ok := service.(HistoryProviderView); ok {
				if available, lookupErr := source.AvailableProviders(ctx); lookupErr == nil {
					providers = available
				} else if ctx.Err() == nil {
					slog.Warn("history provider catalog unavailable", "error", lookupErr)
				}
			}
		}
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
			if counts != nil {
				s.hist.counts = counts
			}
			// Nothing is selected until the user selects it: the zero value
			// 0 pre-highlighted the first row as if chosen (2026-10-06 F15).
			s.hist.listSelected = -1
			if query.Provider == "" && query.Search == "" {
				s.hist.providers = providers
				s.feedRecentFromHistory(summaries)
			}
		})
	})
}

// applyHistoryFilters is the one transition for interactive Search/Provider
// changes. The previous result set must not remain clickable under new
// criteria while its replacement is loading.
func (s *Shell) applyHistoryFilters() {
	s.hist.sessions = nil
	s.hist.listSelected = -1
	s.hist.loaded = false
	s.closeHistoryPreview()
	s.clearHistorySelection()
	s.requestHistoryList()
}

func (s *Shell) clearHistoryFilters() {
	if s.hist.query == "" && s.hist.provider == "" {
		return
	}
	s.hist.query = ""
	s.hist.provider = ""
	s.applyHistoryFilters()
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
	s.closeHistoryPreview()
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
	s.hist.scanning = true
	go s.runHistoryScan(ctx)
}

// runHistoryScan is the scan goroutine's body. A progressive service reports
// every indexed batch, and each report reloads the visible list on the window
// lane, so the page is usable after the first batch while the rest of a huge
// catalog is still being read. The completion applies the usual
// loaded=false re-request once the round ends.
func (s *Shell) runHistoryScan(ctx context.Context) {
	service := s.hist.service
	if service == nil {
		return
	}
	var (
		result history.ScanResult
		err    error
	)
	if progressive, ok := service.(HistoryScanProgressView); ok {
		result, err = progressive.ScanWithProgress(ctx, func() {
			s.applyOnWindow(func() {
				if ctx.Err() == nil {
					s.requestHistoryList()
				}
			})
		})
	} else {
		result, err = service.Scan(ctx)
	}
	if err != nil && ctx.Err() == nil {
		slog.Warn("history scan failed", "error", err)
		s.applyOnWindow(func() { s.hist.scanning = false })
		return
	}
	if err == nil {
		slog.Info("history scan finished", "scanned", result.Scanned, "changed", result.Changed, "removed", result.Removed)
	}
	s.applyOnWindow(func() {
		if ctx.Err() == nil {
			s.hist.scanning = false
			s.hist.loaded = false
		}
	})
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
