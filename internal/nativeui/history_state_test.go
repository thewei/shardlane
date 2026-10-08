package nativeui

import (
	"context"
	"math"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/history"
)

// fakeHistoryView is a deterministic HistoryListView: every List call is
// announced on calls and blocks until the test releases it with a result.
type fakeHistoryView struct {
	calls     chan *fakeListCall
	openCalls chan *fakeOpenCall
}

type fakeListCall struct {
	ctx     context.Context
	query   history.HistoryQuery
	release chan listResult
}

type fakeOpenCall struct {
	ctx            context.Context
	conversationID string
	release        chan openResult
}

type openResult struct {
	window *history.TranscriptWindow
	err    error
}

type listResult struct {
	summaries []history.SessionSummary
	err       error
}

func newFakeHistoryView() *fakeHistoryView {
	return &fakeHistoryView{
		calls:     make(chan *fakeListCall, 8),
		openCalls: make(chan *fakeOpenCall, 8),
	}
}

func (f *fakeHistoryView) List(ctx context.Context, q history.HistoryQuery) ([]history.SessionSummary, error) {
	call := &fakeListCall{ctx: ctx, query: q, release: make(chan listResult, 1)}
	f.calls <- call
	select {
	case result := <-call.release:
		// Mirror the real service: an already-cancelled request stays cancelled.
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		return result.summaries, result.err
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

func (f *fakeHistoryView) Open(ctx context.Context, req history.ConversationWindowRequest) (history.TranscriptWindow, error) {
	call := &fakeOpenCall{ctx: ctx, conversationID: req.ConversationID, release: make(chan openResult, 1)}
	f.openCalls <- call
	select {
	case result := <-call.release:
		if err := ctx.Err(); err != nil {
			return history.TranscriptWindow{}, err
		}
		if result.window != nil {
			return *result.window, result.err
		}
		return history.TranscriptWindow{}, result.err
	case <-ctx.Done():
		return history.TranscriptWindow{}, ctx.Err()
	}
}

func (f *fakeHistoryView) Recent(ctx context.Context, limit int) ([]history.SessionSummary, error) {
	// Same releasable-call seam as List: the usage-projection refresh drives
	// Recent and its tests need deterministic results.
	call := &fakeListCall{ctx: ctx, query: history.HistoryQuery{Limit: limit}, release: make(chan listResult, 1)}
	f.calls <- call
	select {
	case result := <-call.release:
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		return result.summaries, result.err
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

func (f *fakeHistoryView) Scan(ctx context.Context) (history.ScanResult, error) {
	return history.ScanResult{}, nil
}

func summaryTitled(key, title string) history.SessionSummary {
	return history.SessionSummary{Meta: history.SessionMeta{Key: key, Title: title, Agent: history.AgentCodex}}
}

// TestHistoryListLatestRequestWins pins CLOSURE-01: a changed filter while a
// list request is in flight cancels it, starts a new generation querying the
// latest filters immediately, and a late stale result can neither overwrite
// the newer answer nor resurrect the loading state.
func TestHistoryListLatestRequestWins(t *testing.T) {
	shell := NewShell()
	shell.hist.service = newFakeHistoryView()
	fake := shell.hist.service.(*fakeHistoryView)
	shell.router.Replace("/history")

	// First intent: issued and held in flight by the fake.
	firstDone := make(chan struct{})
	go func() {
		defer close(firstDone)
		shell.requestHistoryList()
	}()
	first := <-fake.calls
	if first.query.Search != "" || first.query.Provider != "" {
		t.Fatalf("first query = %+v", first.query)
	}

	// The user changes the provider while the first request is still loading.
	// The new intent must not be dropped.
	shell.hist.provider = string(history.AgentCodex)
	secondDone := make(chan struct{})
	go func() {
		defer close(secondDone)
		shell.requestHistoryList()
	}()
	second := <-fake.calls
	if second.query.Provider != history.AgentCodex {
		t.Fatalf("second query = %+v, want the latest provider filter", second.query)
	}

	// The latest request applies immediately.
	second.release <- listResult{summaries: []history.SessionSummary{summaryTitled("codex:2", "Second Session")}}
	<-secondDone
	if shell.hist.loading {
		t.Fatal("latest request finished but the shell is still loading")
	}
	if !shell.hist.loaded || len(shell.hist.sessions) != 1 || shell.hist.sessions[0].Meta.Title != "Second Session" {
		t.Fatalf("applied sessions = %+v", shell.hist.sessions)
	}

	// The superseded request resolves late; its stale result must be ignored.
	first.release <- listResult{summaries: []history.SessionSummary{summaryTitled("codex:1", "First Session")}}
	<-firstDone
	if first.ctx.Err() != context.Canceled {
		t.Fatalf("superseded request context = %v, want canceled", first.ctx.Err())
	}
	if len(shell.hist.sessions) != 1 || shell.hist.sessions[0].Meta.Title != "Second Session" {
		t.Fatalf("stale result overwrote the latest answer: %+v", shell.hist.sessions)
	}
	if shell.hist.loading {
		t.Fatal("stale result resurrected the loading state")
	}

	tester := ui.NewTester(shell.View, 1200, 800)
	if tester.HasText("First Session") {
		t.Fatalf("stale rows rendered; texts=%q", tester.Texts())
	}
	if !tester.HasText("Second Session") {
		t.Fatalf("latest rows missing; texts=%q", tester.Texts())
	}
}

// TestHistoryDetailScrollIntentsFollowPaging pins CLOSURE-03: the detail has
// one explicit scroll owner; paging later follows the end, paging earlier
// returns to the top, and neither intent lingers into later frames.
func TestHistoryDetailScrollIntentsFollowPaging(t *testing.T) {
	shell := NewShell()
	catalog, err := history.OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()
	shell.hist.service = history.NewHistoryService(catalog, nil)

	source := history.SessionFileRef{Agent: history.AgentCodex, NativeID: "big", FilePath: "/nowhere/big.jsonl", MtimeMS: 1, SizeBytes: 2}
	meta := history.SessionMeta{
		Key: "codex:big", ID: "big", Agent: history.AgentCodex, Title: "Big",
		FilePath: source.FilePath, UpdatedAt: 1, SizeBytes: source.SizeBytes,
	}
	if err := catalog.WriteSession(meta, source.MtimeMS, nil); err != nil {
		t.Fatal(err)
	}
	transcript := history.ParsedTranscript{Meta: meta}
	for index := 0; index < 200; index++ {
		transcript.Mainline = append(transcript.Mainline, history.TranscriptMessage{
			Seq: int64(index), Role: history.RoleUser, Kind: history.MessageText, Text: "line",
		})
	}
	if err := catalog.CacheTranscript(source, transcript); err != nil {
		t.Fatal(err)
	}

	shell.router.Replace("/history/codex:big")
	tester := ui.NewTester(shell.View, 1200, 800)
	if shell.hist.detail == nil || shell.hist.detail.scroll.Y != 0 {
		t.Fatalf("fresh detail scroll = %+v", shell.hist.detail)
	}

	// Load later: the button lives at the window end, so scroll to it like a
	// reader would, then activate it.
	shell.hist.detail.scroll.Y = math.MaxFloat32
	tester.Frame()
	if err := tester.Click("Load 140 later messages"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.hist.detail.window.Start != 48 {
		t.Fatalf("later window start = %d, want 48", shell.hist.detail.window.Start)
	}
	if shell.hist.detail.scrollToBot || shell.hist.detail.scrollToTop {
		t.Fatal("paging intents were not consumed")
	}
	if shell.hist.detail.scroll.Y <= 0 {
		t.Fatalf("later paging did not scroll (Y=%f)", shell.hist.detail.scroll.Y)
	}

	// Load earlier: the button lives at the window top, so scroll back up
	// first, then activate it.
	shell.hist.detail.scroll.Y = 0
	tester.Frame()
	if err := tester.Click("Load 48 earlier messages"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if shell.hist.detail.window.Start != 0 {
		t.Fatalf("earlier window start = %d, want 0", shell.hist.detail.window.Start)
	}
	if shell.hist.detail.scrollToBot || shell.hist.detail.scrollToTop {
		t.Fatal("paging intents were not consumed")
	}
	if shell.hist.detail.scroll.Y != 0 {
		t.Fatalf("earlier paging did not return to the top (Y=%f)", shell.hist.detail.scroll.Y)
	}
}
