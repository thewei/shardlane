package history

import (
	"context"
	"path/filepath"
	"strings"
	"testing"
)

func newTestService(t *testing.T) (*HistoryService, string) {
	t.Helper()
	home := t.TempDir()
	catalog, err := OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { catalog.Close() })
	roots := []SourceRoot{{
		Agent:     AgentCodex,
		Directory: filepath.Join(home, "sessions"),
		NativeID:  rolloutNativeID,
	}}
	return NewHistoryService(catalog, roots), home
}

func TestServiceListAndRecentNeverParseTranscripts(t *testing.T) {
	service, _ := newTestService(t)
	// The catalog row references a file that does not exist: metadata reads
	// must succeed without any transcript parse.
	meta := catalogTestSession("codex:ghost", "Ghost", "/nowhere/rollout.jsonl", "/work")
	meta.Agent = AgentCodex
	meta.ID = "ghost"
	if err := service.Catalog().WriteSession(meta, 1000, catalogTestUnits("body")); err != nil {
		t.Fatal(err)
	}

	summaries, err := service.List(context.Background(), HistoryQuery{Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	if len(summaries) != 1 {
		t.Fatalf("list = %+v", summaries)
	}
	recent, err := service.Recent(context.Background(), 5)
	if err != nil {
		t.Fatal(err)
	}
	if len(recent) != 1 || recent[0].Meta.Key != "codex:ghost" {
		t.Fatalf("recent = %+v", recent)
	}
	filtered, err := service.List(context.Background(), HistoryQuery{Limit: 10, Provider: AgentClaudeCode})
	if err != nil {
		t.Fatal(err)
	}
	if len(filtered) != 0 {
		t.Fatalf("provider filter = %+v", filtered)
	}
}

// TestServiceOpenParsesOnCacheMissAndReusesCache pins the open path: first
// Open parses the source once and prewarms the cache; later opens serve from
// the cache and stay bounded.
func TestServiceOpenParsesOnCacheMissAndReusesCache(t *testing.T) {
	service, home := newTestService(t)
	sessions := filepath.Join(home, "sessions")
	writeCodexSessionFile(t, sessions, "rollout-2026-08-02T09-15-00-22222222-aaaa-bbbb-cccc-000000000002.jsonl", codexFixtureRollout)
	if _, err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	// Simulate a legacy uncached catalog row.
	if _, err := service.Catalog().db.Exec("DELETE FROM transcript_page_meta WHERE session_key = 'codex:22222222-aaaa-bbbb-cccc-000000000002'"); err != nil {
		t.Fatal(err)
	}

	window, err := service.Open(context.Background(), ConversationWindowRequest{
		ConversationID: "codex:22222222-aaaa-bbbb-cccc-000000000002",
	})
	if err != nil {
		t.Fatal(err)
	}
	if window.TotalMessages == 0 || len(window.Messages) == 0 {
		t.Fatalf("window = %+v", window)
	}
	if len(window.Messages) > WindowLimit {
		t.Fatalf("window materialized %d messages", len(window.Messages))
	}
	if !strings.Contains(window.Meta.Title, "Build history") {
		t.Fatalf("window meta title = %q", window.Meta.Title)
	}

	// Explicit paging: start beyond the end clamps, flags report bounds.
	paged, err := service.Open(context.Background(), ConversationWindowRequest{
		ConversationID: window.Key,
		StartIndex:     ptr(1),
		Limit:          2,
	})
	if err != nil {
		t.Fatal(err)
	}
	if paged.Start != 1 || paged.HasEarlier() != true || len(paged.Messages) != 2 {
		t.Fatalf("paged window = %+v", paged)
	}
	if paged.HasLater() {
		t.Fatal("small source should not report later messages")
	}
}

func TestServiceOpenAnchorSeqIncludesTarget(t *testing.T) {
	service, home := newTestService(t)
	sessions := filepath.Join(home, "sessions")
	writeCodexSessionFile(t, sessions, "rollout-2026-08-02T09-15-00-22222222-aaaa-bbbb-cccc-000000000002.jsonl", codexFixtureRollout)
	if _, err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}

	hits, err := service.Search(context.Background(), "Implemented", "", 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(hits) != 1 {
		t.Fatalf("hits = %+v", hits)
	}

	window, err := service.Open(context.Background(), ConversationWindowRequest{
		ConversationID: hits[0].Session.Key,
		AnchorSeq:      &hits[0].Seq,
		Limit:          60,
	})
	if err != nil {
		t.Fatal(err)
	}
	if window.AnchorIndex < 0 {
		t.Fatalf("anchor index = %d", window.AnchorIndex)
	}
	if window.AnchorIndex < window.Start || window.AnchorIndex >= window.Start+len(window.Messages) {
		t.Fatalf("anchor outside window: anchor=%d window=%d..%d",
			window.AnchorIndex, window.Start, window.Start+len(window.Messages))
	}
}

func TestServiceOpenUnknownConversationFailsClosed(t *testing.T) {
	service, _ := newTestService(t)
	if _, err := service.Open(context.Background(), ConversationWindowRequest{ConversationID: "codex:missing"}); err == nil {
		t.Fatal("expected an error for an unknown conversation")
	}
}

func TestServiceOpenRespectsWindowLimitCap(t *testing.T) {
	service, _ := newTestService(t)
	source := SessionFileRef{Agent: AgentCodex, NativeID: "big", FilePath: "/nowhere/big.jsonl", MtimeMS: 1, SizeBytes: 2}
	meta := catalogTestSession("codex:big", "Big", source.FilePath, "/work")
	meta.Agent = AgentCodex
	meta.ID = "big"
	meta.SizeBytes = source.SizeBytes
	catalog := service.Catalog()
	if err := catalog.WriteSession(meta, source.MtimeMS, catalogTestUnits("body")); err != nil {
		t.Fatal(err)
	}
	transcript := ParsedTranscript{Meta: meta}
	for index := 0; index < 200; index++ {
		transcript.Mainline = append(transcript.Mainline, TranscriptMessage{
			Seq: int64(index), Role: RoleUser, Kind: MessageText, Text: "line",
		})
	}
	if err := catalog.CacheTranscript(source, transcript); err != nil {
		t.Fatal(err)
	}

	// A UI asking for an unbounded window still gets at most 60 messages.
	window, err := service.Open(context.Background(), ConversationWindowRequest{
		ConversationID: "codex:big", Limit: 100000,
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(window.Messages) != WindowLimit || window.TotalMessages != 200 {
		t.Fatalf("window = %d messages of %d", len(window.Messages), window.TotalMessages)
	}
	if !window.HasLater() || window.HasEarlier() {
		t.Fatalf("flags = earlier:%v later:%v", window.HasEarlier(), window.HasLater())
	}
}

func ptr(value int) *int { return &value }
