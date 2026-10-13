package history

import (
	"fmt"
	"strings"
	"sync"
	"testing"
)

func catalogTestSession(key, title, filePath, projectPath string) SessionMeta {
	return SessionMeta{
		Key:         key,
		ID:          "native-" + key,
		Agent:       AgentClaudeCode,
		Title:       title,
		ProjectPath: projectPath,
		ProjectName: projectName(projectPath),
		FilePath:    filePath,
	}
}

func catalogTestMessages(texts ...string) []TranscriptMessage {
	messages := make([]TranscriptMessage, 0, len(texts))
	for index, text := range texts {
		messages = append(messages, TranscriptMessage{Seq: int64(index), Role: RoleUser, Kind: MessageText, Text: text})
	}
	return messages
}

func TestCatalogArchivedHiddenButReachableByKey(t *testing.T) {
	catalog := NewCatalog()

	live := catalogTestSession("live-1", "live session", "/work/demo/a.jsonl", "/work/demo")
	live.UpdatedAt, live.CreatedAt = 1000, 0
	archived := catalogTestSession("arch-1", "archived secret", "/work/demo/b.jsonl", "/work/demo")
	archived.Archived = true
	archived.UpdatedAt, archived.CreatedAt = 2000, 1000
	catalog.Put(live, 1000, "")
	catalog.Put(archived, 2000, "")

	summaries := catalog.ListSessions(10)
	if len(summaries) != 1 || summaries[0].Meta.Key != "live-1" {
		t.Fatalf("list = %+v", summaries)
	}
	if found := catalog.SearchMetadata("secret", nil, nil, 10); len(found) != 0 {
		t.Fatalf("archived session leaked into search: %+v", found)
	}
	reachable := catalog.Session("arch-1")
	if reachable == nil || reachable.Title != "archived secret" {
		t.Fatalf("archived session by key = %+v", reachable)
	}
	if catalog.Session("missing") != nil {
		t.Fatal("unknown key must not resolve")
	}
}

func TestCatalogListOrderingAndDescription(t *testing.T) {
	catalog := NewCatalog()

	first := catalogTestSession("old", "Old", "/w/a.jsonl", "/w")
	first.UpdatedAt, first.CreatedAt = 1000, 0
	second := catalogTestSession("new", "New", "/w/b.jsonl", "/w")
	second.UpdatedAt, second.CreatedAt = 2000, 1000
	tied := catalogTestSession("tied", "Tied", "/w/c.jsonl", "/w")
	tied.UpdatedAt = 1000
	catalog.Put(first, 1000, descriptionFromMessages(catalogTestMessages(strings.Repeat("word ", 100))))
	catalog.Put(second, 2000, descriptionFromMessages(catalogTestMessages("First \n\t line\n  here")))
	catalog.Put(tied, 1000, "")

	summaries := catalog.ListSessions(10)
	if len(summaries) != 3 || summaries[0].Meta.Key != "new" || summaries[1].Meta.Key != "old" || summaries[2].Meta.Key != "tied" {
		t.Fatalf("ordering (newest first, key breaks ties) = %+v", summaries)
	}
	if summaries[0].Description != "First line here" {
		t.Fatalf("description = %q", summaries[0].Description)
	}
	if got := summaries[1].Description; !strings.HasSuffix(got, "…") {
		t.Fatalf("long description must truncate: %q", got)
	}
	if got := catalog.ListSessions(2); len(got) != 2 {
		t.Fatalf("limit not applied: %d rows", len(got))
	}
	if got := catalog.ListSessions(0); len(got) != 0 {
		t.Fatalf("zero limit = %+v", got)
	}
}

func TestCatalogSearchMetadataTermsAndScopes(t *testing.T) {
	catalog := NewCatalog()
	alpha := catalogTestSession("alpha", "Alpha Release Notes", "/w/a.jsonl", "/work/alpha")
	alpha.Agent = AgentClaudeCode
	alpha.UpdatedAt = 1000
	beta := catalogTestSession("beta", "储物柜 里 有 一只 猫", "/w/b.jsonl", "/work/beta")
	beta.Agent = AgentCodex
	beta.UpdatedAt = 2000
	catalog.Put(alpha, 1000, "")
	catalog.Put(beta, 2000, "")

	keys := func(found []SessionSummary) string {
		out := make([]string, 0, len(found))
		for _, summary := range found {
			out = append(out, summary.Meta.Key)
		}
		return strings.Join(out, ",")
	}
	for _, tc := range []struct {
		name     string
		query    string
		projects []string
		agents   []AgentID
		want     string
	}{
		{"title is case-insensitive", "release NOTES", nil, nil, "alpha"},
		{"every term must match", "release missing", nil, nil, ""},
		{"CJK title", "储物柜", nil, nil, "beta"},
		{"project name", "beta", nil, nil, "beta"},
		{"native id", "native-alpha", nil, nil, "alpha"},
		{"file path", "/w/b", nil, nil, "beta"},
		{"agent name", "codex", nil, nil, "beta"},
		{"provider scope", "a", nil, []AgentID{AgentCodex}, "beta"},
		{"project scope", "", []string{"/work/./alpha"}, nil, "alpha"},
		{"blank query without scope", "  ", nil, nil, ""},
		{"unknown project scope", "alpha", []string{"   "}, nil, ""},
	} {
		if got := keys(catalog.SearchMetadata(tc.query, tc.projects, tc.agents, 10)); got != tc.want {
			t.Errorf("%s: SearchMetadata(%q) = %q, want %q", tc.name, tc.query, got, tc.want)
		}
	}
	if got := catalog.SearchMetadata("a", nil, nil, 0); len(got) != 0 {
		t.Fatalf("zero limit = %+v", got)
	}
}

// TestCatalogPutReplacesRowAndKeepsFilePathUnique pins the row identity
// rules: a key is one row, a provider file backs at most one row, and the
// stamp the scanner compares follows the latest observation.
func TestCatalogPutReplacesRowAndKeepsFilePathUnique(t *testing.T) {
	catalog := NewCatalog()
	meta := catalogTestSession("codex:s1", "First title", "/w/s1.jsonl", "/w")
	meta.SizeBytes = 10
	catalog.Put(meta, 1000, "d1")

	meta.Title, meta.SizeBytes = "Second title", 20
	catalog.Put(meta, 2000, "d2")
	rows := catalog.ListSessions(10)
	if len(rows) != 1 || rows[0].Meta.Title != "Second title" || rows[0].Description != "d2" {
		t.Fatalf("replace = %+v", rows)
	}
	if stamp := catalog.KnownFiles()["/w/s1.jsonl"]; stamp.mtimeMS != 2000 || stamp.size != 20 {
		t.Fatalf("stamp = %+v", stamp)
	}
	if rows[0].Meta.MtimeMS != 2000 {
		t.Fatalf("row mtime = %d", rows[0].Meta.MtimeMS)
	}

	// A different key claiming the same provider file evicts the older row.
	other := catalogTestSession("codex:s2", "Renamed", "/w/s1.jsonl", "/w")
	catalog.Put(other, 3000, "")
	rows = catalog.ListSessions(10)
	if len(rows) != 1 || rows[0].Meta.Key != "codex:s2" {
		t.Fatalf("path uniqueness = %+v", rows)
	}
	if catalog.Session("codex:s1") != nil {
		t.Fatal("evicted row still reachable by key")
	}

	// Moving a key to a new file releases the old path.
	moved := other
	moved.FilePath = "/w/moved.jsonl"
	catalog.Put(moved, 4000, "")
	known := catalog.KnownFiles()
	if _, stale := known["/w/s1.jsonl"]; stale || len(known) != 1 {
		t.Fatalf("known files after move = %+v", known)
	}
}

func TestCatalogRemoveMissing(t *testing.T) {
	catalog := NewCatalog()
	catalog.Put(catalogTestSession("keep", "Keep", "/w/keep.jsonl", "/w"), 1000, "")
	catalog.Put(catalogTestSession("drop", "Drop", "/w/drop.jsonl", "/w"), 2000, "")

	if removed := catalog.RemoveMissing(map[string]bool{"/w/keep.jsonl": true}); removed != 1 {
		t.Fatalf("removed = %d", removed)
	}
	summaries := catalog.ListSessions(10)
	if len(summaries) != 1 || summaries[0].Meta.Key != "keep" {
		t.Fatalf("after cleanup = %+v", summaries)
	}
	if _, stale := catalog.KnownFiles()["/w/drop.jsonl"]; stale {
		t.Fatal("removed row's path is still known")
	}
}

// TestCatalogConcurrentReadWrite runs under -race: the scanner writes on a
// background lane while the UI lists.
func TestCatalogConcurrentReadWrite(t *testing.T) {
	catalog := NewCatalog()
	var wg sync.WaitGroup
	for writer := 0; writer < 4; writer++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for index := 0; index < 200; index++ {
				key := fmt.Sprintf("k%d-%d", writer, index%20)
				catalog.Put(catalogTestSession(key, key, "/w/"+key+".jsonl", "/w"), int64(index), "")
				catalog.RemoveMissing(map[string]bool{"/w/" + key + ".jsonl": true})
			}
		}()
	}
	for reader := 0; reader < 4; reader++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for index := 0; index < 200; index++ {
				catalog.ListSessions(10)
				catalog.ProviderCounts()
				catalog.SearchMetadata("k", nil, nil, 10)
				catalog.KnownFiles()
			}
		}()
	}
	wg.Wait()
}

func TestSessionDescriptionCollapseAndTruncation(t *testing.T) {
	if got := descriptionFromMessages(catalogTestMessages("  a   b\t\nc  ")); got != "a b c" {
		t.Fatalf("description = %q", got)
	}
	long := strings.Repeat("字", DescriptionMaxChars+10)
	got := descriptionFromMessages(catalogTestMessages(long))
	if len([]rune(got)) != DescriptionMaxChars+1 || !strings.HasSuffix(got, "…") {
		t.Fatalf("unicode truncation = %d runes", len([]rune(got)))
	}
	if got := descriptionFromMessages(nil); got != "" {
		t.Fatalf("empty transcript description = %q", got)
	}
}

// TestDescriptionPrefersFirstUserText pins the candidate order: the first
// non-blank user text wins; without one the first non-blank text message
// stands in (its tool-call summaries included); meta messages never count.
func TestDescriptionPrefersFirstUserText(t *testing.T) {
	messages := []TranscriptMessage{
		{Seq: 0, Role: RoleSystem, Kind: MessageMeta, Text: "scaffolding"},
		{Seq: 1, Role: RoleAssistant, Kind: MessageText, Text: "assistant first"},
		{Seq: 2, Role: RoleUser, Kind: MessageText, Text: "   "},
		{Seq: 3, Role: RoleUser, Kind: MessageText, Text: "the real prompt"},
	}
	if got := descriptionFromMessages(messages); got != "the real prompt" {
		t.Fatalf("user candidate = %q", got)
	}
	noUser := []TranscriptMessage{
		{Seq: 0, Role: RoleSystem, Kind: MessageMeta, Text: "scaffolding"},
		{Seq: 1, Role: RoleAssistant, Kind: MessageText, ToolCalls: []ToolCall{{Name: "shell", InputPreview: "cargo test"}}},
	}
	if got := descriptionFromMessages(noUser); got != "shell cargo test" {
		t.Fatalf("fallback candidate = %q", got)
	}
}
