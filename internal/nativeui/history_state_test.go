package nativeui

import (
	"context"
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
