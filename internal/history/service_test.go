package history

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func newTestService(t *testing.T) (*HistoryService, string) {
	t.Helper()
	home := t.TempDir()
	roots := []SourceRoot{{
		Agent:     AgentCodex,
		Directory: filepath.Join(home, "sessions"),
		NativeID:  rolloutNativeID,
	}}
	return NewHistoryService(NewCatalog(), roots), home
}

const testRolloutName = "rollout-2026-08-02T09-15-00-22222222-aaaa-bbbb-cccc-000000000002.jsonl"
const testRolloutKey = "codex:22222222-aaaa-bbbb-cccc-000000000002"

func TestServiceListAndRecentNeverParseTranscripts(t *testing.T) {
	service, _ := newTestService(t)
	// The catalog row references a file that does not exist: metadata reads
	// must succeed without any transcript parse.
	meta := catalogTestSession("codex:ghost", "Ghost", "/nowhere/rollout.jsonl", "/work")
	meta.Agent = AgentCodex
	meta.ID = "ghost"
	service.Catalog().Put(meta, 1000, "body")

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
	searched, err := service.List(context.Background(), HistoryQuery{Limit: 10, Search: "ghost"})
	if err != nil {
		t.Fatal(err)
	}
	if len(searched) != 1 || searched[0].Description != "body" {
		t.Fatalf("metadata search = %+v", searched)
	}
}

// TestServiceOpenReadsOriginalFileWindows pins the open path: a window is cut
// from the parsed original file, bounded, with honest paging flags.
func TestServiceOpenReadsOriginalFileWindows(t *testing.T) {
	service, home := newTestService(t)
	writeCodexSessionFile(t, filepath.Join(home, "sessions"), testRolloutName, codexFixtureRollout)
	if _, err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}

	window, err := service.Open(context.Background(), ConversationWindowRequest{ConversationID: testRolloutKey})
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

	// Explicit paging: flags report bounds.
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

	// A start beyond the end clamps to an empty window at the end.
	beyond, err := service.Open(context.Background(), ConversationWindowRequest{
		ConversationID: window.Key,
		StartIndex:     ptr(500),
	})
	if err != nil {
		t.Fatal(err)
	}
	if beyond.Start != beyond.TotalMessages || len(beyond.Messages) != 0 || beyond.HasLater() {
		t.Fatalf("clamped window = %+v", beyond)
	}
}

// TestServiceOpenAlwaysReadsOriginalFile is the no-copy contract: nothing of
// a transcript is kept between opens, so a change to the provider file shows
// in the very next Open — without a rescan.
func TestServiceOpenAlwaysReadsOriginalFile(t *testing.T) {
	service, home := newTestService(t)
	path := writeCodexSessionFile(t, filepath.Join(home, "sessions"), testRolloutName, codexFixtureRollout)
	if _, err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	before, err := service.Open(context.Background(), ConversationWindowRequest{ConversationID: testRolloutKey})
	if err != nil {
		t.Fatal(err)
	}

	appended := `{"timestamp":"2026-08-02T09:20:00Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Appended after the scan."}]}}` + "\n"
	file, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.WriteString(appended); err != nil {
		t.Fatal(err)
	}
	file.Close()

	after, err := service.Open(context.Background(), ConversationWindowRequest{ConversationID: testRolloutKey})
	if err != nil {
		t.Fatal(err)
	}
	if after.TotalMessages != before.TotalMessages+1 {
		t.Fatalf("total messages %d → %d, want one more from the original file", before.TotalMessages, after.TotalMessages)
	}
	if last := after.Messages[len(after.Messages)-1]; last.Text != "Appended after the scan." {
		t.Fatalf("last message = %q", last.Text)
	}

	// The file is the only source: once it is gone the conversation cannot
	// be opened, whatever was seen before.
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	if _, err := service.Open(context.Background(), ConversationWindowRequest{ConversationID: testRolloutKey}); err == nil {
		t.Fatal("open must fail once the original file is gone")
	}
}

func TestServiceOpenAnchorSeqIncludesTarget(t *testing.T) {
	service, home := newTestService(t)
	writeCodexSessionFile(t, filepath.Join(home, "sessions"), testRolloutName, codexFixtureRollout)
	if _, err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	all, err := service.Open(context.Background(), ConversationWindowRequest{ConversationID: testRolloutKey})
	if err != nil {
		t.Fatal(err)
	}
	var anchor int64 = -1
	for _, message := range all.Messages {
		if message.Text == "Implemented." {
			anchor = message.Seq
		}
	}
	if anchor < 0 {
		t.Fatalf("fixture lost its assistant answer: %+v", all.Messages)
	}

	window, err := service.Open(context.Background(), ConversationWindowRequest{
		ConversationID: testRolloutKey,
		AnchorSeq:      &anchor,
		Limit:          60,
	})
	if err != nil {
		t.Fatal(err)
	}
	if window.AnchorIndex < window.Start || window.AnchorIndex >= window.Start+len(window.Messages) {
		t.Fatalf("anchor outside window: anchor=%d window=%d..%d",
			window.AnchorIndex, window.Start, window.Start+len(window.Messages))
	}
	if window.Messages[window.AnchorIndex-window.Start].Seq != anchor {
		t.Fatalf("anchor index %d does not point at seq %d", window.AnchorIndex, anchor)
	}

	missing := int64(9999)
	window, err = service.Open(context.Background(), ConversationWindowRequest{ConversationID: testRolloutKey, AnchorSeq: &missing})
	if err != nil || window.AnchorIndex != -1 {
		t.Fatalf("unknown anchor = (%d, %v), want index -1", window.AnchorIndex, err)
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
	meta := catalogTestSession("codex:big", "Big", "/nowhere/big.jsonl", "/work")
	meta.Agent = AgentCodex
	meta.ID = "big"
	service.Catalog().Put(meta, 1, "body")
	service.parse = func(SessionFileRef) (ParsedTranscript, error) {
		transcript := ParsedTranscript{Meta: meta}
		for index := 0; index < 200; index++ {
			transcript.Mainline = append(transcript.Mainline, TranscriptMessage{
				Seq: int64(index), Role: RoleUser, Kind: MessageText, Text: "line",
			})
		}
		return transcript, nil
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

// TestServiceOpenTailReturnsNewestWindow pins the chat-style entry: Tail asks
// for the newest Limit messages, an explicit StartIndex or AnchorSeq still
// wins, and a transcript shorter than the limit just starts at zero.
func TestServiceOpenTailReturnsNewestWindow(t *testing.T) {
	service, _ := newTestService(t)
	meta := catalogTestSession("codex:tail", "Tail", "/nowhere/tail.jsonl", "/work")
	meta.Agent = AgentCodex
	meta.ID = "tail"
	service.Catalog().Put(meta, 1, "body")
	total := 0
	service.parse = func(SessionFileRef) (ParsedTranscript, error) {
		transcript := ParsedTranscript{Meta: meta}
		for index := 0; index < total; index++ {
			transcript.Mainline = append(transcript.Mainline, TranscriptMessage{
				Seq: int64(index), Role: RoleUser, Kind: MessageText, Text: "line",
			})
		}
		return transcript, nil
	}

	total = 200
	tail, err := service.Open(context.Background(), ConversationWindowRequest{ConversationID: "codex:tail", Tail: true})
	if err != nil {
		t.Fatal(err)
	}
	if tail.Start != 140 || len(tail.Messages) != WindowLimit || tail.Messages[0].Seq != 140 || tail.Messages[WindowLimit-1].Seq != 199 {
		t.Fatalf("tail window = start %d, %d messages", tail.Start, len(tail.Messages))
	}
	if !tail.HasEarlier() || tail.HasLater() {
		t.Fatalf("tail flags = earlier:%v later:%v", tail.HasEarlier(), tail.HasLater())
	}

	small, err := service.Open(context.Background(), ConversationWindowRequest{ConversationID: "codex:tail", Tail: true, Limit: 8})
	if err != nil {
		t.Fatal(err)
	}
	if small.Start != 192 || len(small.Messages) != 8 {
		t.Fatalf("limited tail = start %d, %d messages", small.Start, len(small.Messages))
	}

	paged, err := service.Open(context.Background(), ConversationWindowRequest{ConversationID: "codex:tail", Tail: true, StartIndex: ptr(10), Limit: 5})
	if err != nil {
		t.Fatal(err)
	}
	if paged.Start != 10 || len(paged.Messages) != 5 {
		t.Fatalf("explicit start must beat Tail: start %d, %d messages", paged.Start, len(paged.Messages))
	}

	total = 12
	short, err := service.Open(context.Background(), ConversationWindowRequest{ConversationID: "codex:tail", Tail: true})
	if err != nil {
		t.Fatal(err)
	}
	if short.Start != 0 || len(short.Messages) != 12 || short.HasEarlier() {
		t.Fatalf("short tail = start %d, %d messages", short.Start, len(short.Messages))
	}
}

func ptr(value int) *int { return &value }

// TestServiceForgetRemovesRowsFromEveryRead pins that a forgotten session
// is gone from the list and from the by-key lookup at once, without
// waiting for the next scan's cleanup.
func TestServiceForgetRemovesRowsFromEveryRead(t *testing.T) {
	service, _ := newTestService(t)
	for _, key := range []string{"one", "two"} {
		meta := catalogTestSession("codex:"+key, "Session "+key, "/nowhere/"+key+".jsonl", "/work")
		meta.Agent = AgentCodex
		service.Catalog().Put(meta, 1000, "")
	}

	if removed := service.Forget("codex:one"); removed != 1 {
		t.Fatalf("Forget removed %d, want 1", removed)
	}
	summaries, err := service.List(context.Background(), HistoryQuery{Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	if len(summaries) != 1 || summaries[0].Meta.Key != "codex:two" {
		t.Fatalf("list after Forget = %+v", summaries)
	}
	if counts, _ := service.ProviderCounts(context.Background()); len(counts) != 1 || counts[0].Count != 1 {
		t.Fatalf("provider counts after Forget = %+v", counts)
	}
}
