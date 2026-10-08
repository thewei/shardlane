package history

import (
	"fmt"
	"path/filepath"
	"strings"
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

func catalogTestUnits(texts ...string) []IndexUnit {
	units := make([]IndexUnit, 0, len(texts))
	for index, text := range texts {
		units = append(units, IndexUnit{
			Seq:  int64(index),
			Role: RoleUser,
			Text: text,
		})
	}
	return units
}

func TestCatalogArchivedHiddenButReachableByKey(t *testing.T) {
	catalog, err := OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()

	live := catalogTestSession("live-1", "live session", "/work/demo/a.jsonl", "/work/demo")
	live.UpdatedAt, live.CreatedAt = 1000, 0
	archived := catalogTestSession("arch-1", "archived secret", "/work/demo/b.jsonl", "/work/demo")
	archived.Archived = true
	archived.UpdatedAt, archived.CreatedAt = 2000, 1000
	if err := catalog.WriteSession(live, 1000, catalogTestUnits("hello world")); err != nil {
		t.Fatal(err)
	}
	if err := catalog.WriteSession(archived, 2000, catalogTestUnits("hidden body")); err != nil {
		t.Fatal(err)
	}

	summaries, err := catalog.ListSessions(10)
	if err != nil {
		t.Fatal(err)
	}
	if len(summaries) != 1 || summaries[0].Meta.Key != "live-1" {
		t.Fatalf("list = %+v", summaries)
	}

	hits, err := catalog.SearchScoped("hidden", nil, nil, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(hits) != 0 {
		t.Fatalf("archived session leaked into search: %+v", hits)
	}

	reachable, err := catalog.Session("arch-1")
	if err != nil || reachable == nil || reachable.Title != "archived secret" {
		t.Fatalf("archived session by key = %+v, %v", reachable, err)
	}
}

func TestCatalogListOrderingAndDescription(t *testing.T) {
	catalog, err := OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()

	first := catalogTestSession("old", "Old", "/w/a.jsonl", "/w")
	first.UpdatedAt, first.CreatedAt = 1000, 0
	second := catalogTestSession("new", "New", "/w/b.jsonl", "/w")
	second.UpdatedAt, second.CreatedAt = 2000, 1000
	long := strings.Repeat("word ", 100)
	// The long text is the first user message so it becomes the description.
	if err := catalog.WriteSession(first, 1000, catalogTestUnits(long, "Second? no")); err != nil {
		t.Fatal(err)
	}
	if err := catalog.WriteSession(second, 2000, catalogTestUnits("First \n\t line\n  here")); err != nil {
		t.Fatal(err)
	}

	summaries, err := catalog.ListSessions(10)
	if err != nil {
		t.Fatal(err)
	}
	if len(summaries) != 2 || summaries[0].Meta.Key != "new" || summaries[1].Meta.Key != "old" {
		t.Fatalf("ordering = %+v", summaries)
	}
	if summaries[0].Description != "First line here" {
		t.Fatalf("description = %q", summaries[0].Description)
	}
	if got := summaries[1].Description; !strings.HasSuffix(got, "…") {
		t.Fatalf("long description must truncate: %q", got)
	}
}

func TestCatalogProjectScopingUsesNormalizedKey(t *testing.T) {
	catalog, err := OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()

	in := catalogTestSession("in", "In", "/w/in.jsonl", "/Users/me/work/project")
	out := catalogTestSession("out", "Out", "/w/out.jsonl", "/Users/me/other")
	if err := catalog.WriteSession(in, 1000, catalogTestUnits("a")); err != nil {
		t.Fatal(err)
	}
	if err := catalog.WriteSession(out, 2000, catalogTestUnits("b")); err != nil {
		t.Fatal(err)
	}

	total, sessions, err := catalog.SessionsForProject("/Users/me/work/./project", 10)
	if err != nil {
		t.Fatal(err)
	}
	if total != 1 || len(sessions) != 1 || sessions[0].Meta.Key != "in" {
		t.Fatalf("scoped = (%d, %+v)", total, sessions)
	}
}

func TestCatalogSearchTermsAndScopes(t *testing.T) {
	catalog, err := OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()

	alpha := catalogTestSession("alpha", "Alpha", "/w/a.jsonl", "/work/alpha")
	alpha.Agent = AgentClaudeCode
	beta := catalogTestSession("beta", "Beta", "/w/b.jsonl", "/work/beta")
	beta.Agent = AgentCodex
	if err := catalog.WriteSession(alpha, 1000, catalogTestUnits("the quick brown fox jumps")); err != nil {
		t.Fatal(err)
	}
	if err := catalog.WriteSession(beta, 2000, catalogTestUnits("储物柜 里 有 一只 猫", "lazy dog sleeps")); err != nil {
		t.Fatal(err)
	}

	hits, err := catalog.SearchScoped("quick brown", nil, nil, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(hits) != 1 || hits[0].Session.Key != "alpha" {
		t.Fatalf("fts hits = %+v", hits)
	}
	if hits[0].Seq != 0 || hits[0].Role != "user" {
		t.Fatalf("hit facts = %+v", hits[0])
	}

	// Two-term queries AND across rows: only beta has both terms.
	hits, err = catalog.SearchScoped("lazy dog", nil, nil, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(hits) != 1 || hits[0].Session.Key != "beta" {
		t.Fatalf("like hits = %+v", hits)
	}

	// A single non-CJK character never runs a body search.
	hits, err = catalog.SearchScoped("q", nil, nil, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(hits) != 0 {
		t.Fatalf("single-char hits = %+v", hits)
	}

	// Provider scope filters.
	hits, err = catalog.SearchScoped("dog", nil, []AgentID{AgentCodex}, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(hits) != 1 {
		t.Fatalf("scoped hits = %+v", hits)
	}
	hits, err = catalog.SearchScoped("dog", nil, []AgentID{AgentClaudeCode}, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(hits) != 0 {
		t.Fatalf("wrong-provider hits = %+v", hits)
	}

	metas, err := catalog.SearchMetadata("beta", nil, nil, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(metas) != 1 || metas[0].Key != "beta" {
		t.Fatalf("metadata search = %+v", metas)
	}
}

func TestCatalogPageCacheWindowsAndInvalidation(t *testing.T) {
	catalog, err := OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()

	source := SessionFileRef{
		Agent: AgentCodex, NativeID: "s1", FilePath: "/w/s1.jsonl",
		MtimeMS: 1000, SizeBytes: 4321,
	}
	transcript := ParsedTranscript{Meta: catalogTestSession("codex:s1", "Paged", source.FilePath, "/w")}
	for index := 0; index < 130; index++ {
		transcript.Mainline = append(transcript.Mainline, TranscriptMessage{
			Seq:  int64(index),
			Role: RoleUser,
			Kind: MessageText,
			Text: fmt.Sprintf("message %03d", index),
		})
	}
	if err := catalog.WriteSession(transcript.Meta, source.MtimeMS, unitsFromMessages(transcript.Mainline)); err != nil {
		t.Fatal(err)
	}
	if err := catalog.CacheTranscript(source, transcript); err != nil {
		t.Fatal(err)
	}

	window, err := catalog.CachedTranscriptWindow("codex:s1", source, 0, 60)
	if err != nil {
		t.Fatal(err)
	}
	if window == nil || len(window.Messages) != 60 || window.Start != 0 || window.TotalMessages != 130 {
		t.Fatalf("first window = %+v", window)
	}
	if window.Messages[0].Text != "message 000" || window.Messages[59].Text != "message 059" {
		t.Fatalf("first window bounds = %q..%q", window.Messages[0].Text, window.Messages[59].Text)
	}

	window, err = catalog.CachedTranscriptWindow("codex:s1", source, 100, 60)
	if err != nil {
		t.Fatal(err)
	}
	if window == nil || len(window.Messages) != 30 || window.Start != 100 {
		t.Fatalf("tail window = %+v", window)
	}
	if window.Messages[0].Text != "message 100" {
		t.Fatalf("tail bound = %q", window.Messages[0].Text)
	}

	index, ok, err := catalog.CachedTranscriptIndexForSeq("codex:s1", source, 77)
	if err != nil || !ok || index != 77 {
		t.Fatalf("seq lookup = (%d, %v, %v)", index, ok, err)
	}

	// A changed source identity is a miss and invalidates stored pages.
	changed := source
	changed.MtimeMS = 9999
	window, err = catalog.CachedTranscriptWindow("codex:s1", changed, 0, 60)
	if err != nil || window != nil {
		t.Fatalf("changed identity = (%+v, %v)", window, err)
	}
	window, err = catalog.CachedTranscriptWindow("codex:s1", source, 0, 60)
	if err != nil || window == nil {
		t.Fatalf("original identity must survive a foreign lookup = (%+v, %v)", window, err)
	}

	// Corrupt page payload is a disposable miss that clears the cache.
	if _, err := catalog.db.Exec(
		"UPDATE transcript_page_cache SET payload = X'00FF' WHERE session_key = 'codex:s1' AND page_index = 0",
	); err != nil {
		t.Fatal(err)
	}
	window, err = catalog.CachedTranscriptWindow("codex:s1", source, 0, 60)
	if err != nil || window != nil {
		t.Fatalf("corrupt cache = (%+v, %v)", window, err)
	}
	var remaining int
	if err := catalog.db.QueryRow(
		"SELECT COUNT(*) FROM transcript_page_cache WHERE session_key = 'codex:s1'",
	).Scan(&remaining); err != nil || remaining != 0 {
		t.Fatalf("corrupt cache was not cleared: %d, %v", remaining, err)
	}
}

func TestCatalogWriteSessionInvalidatesStalePages(t *testing.T) {
	catalog, err := OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()

	source := SessionFileRef{Agent: AgentCodex, NativeID: "s1", FilePath: "/w/s1.jsonl", MtimeMS: 1000, SizeBytes: 10}
	meta := catalogTestSession("codex:s1", "T", source.FilePath, "/w")
	if err := catalog.WriteSession(meta, source.MtimeMS, catalogTestUnits("body")); err != nil {
		t.Fatal(err)
	}
	if err := catalog.CacheTranscript(source, ParsedTranscript{Meta: meta, Mainline: []TranscriptMessage{{
		Seq: 0, Role: RoleUser, Kind: MessageText, Text: "old page",
	}}}); err != nil {
		t.Fatal(err)
	}

	// Same identity, changed mtime/size in the next scan round.
	if err := catalog.WriteSession(meta, 2000, catalogTestUnits("body")); err != nil {
		t.Fatal(err)
	}
	window, err := catalog.CachedTranscriptWindow("codex:s1", source, 0, 60)
	if err != nil || window != nil {
		t.Fatalf("stale pages must be invalidated: (%+v, %v)", window, err)
	}
}

func TestCatalogRemoveMissing(t *testing.T) {
	catalog, err := OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()

	keep := catalogTestSession("keep", "Keep", "/w/keep.jsonl", "/w")
	drop := catalogTestSession("drop", "Drop", "/w/drop.jsonl", "/w")
	if err := catalog.WriteSession(keep, 1000, catalogTestUnits("k")); err != nil {
		t.Fatal(err)
	}
	if err := catalog.WriteSession(drop, 2000, catalogTestUnits("d")); err != nil {
		t.Fatal(err)
	}

	removed, err := catalog.RemoveMissing(map[string]bool{"/w/keep.jsonl": true})
	if err != nil {
		t.Fatal(err)
	}
	if removed != 1 {
		t.Fatalf("removed = %d", removed)
	}
	summaries, err := catalog.ListSessions(10)
	if err != nil {
		t.Fatal(err)
	}
	if len(summaries) != 1 || summaries[0].Meta.Key != "keep" {
		t.Fatalf("after cleanup = %+v", summaries)
	}
}

func TestSessionDescriptionCollapseAndTruncation(t *testing.T) {
	units := catalogTestUnits("  a   b\t\nc  ")
	if got := sessionDescriptionFromUnits(units); got != "a b c" {
		t.Fatalf("description = %q", got)
	}
	long := strings.Repeat("字", DescriptionMaxChars+10)
	units = catalogTestUnits(long)
	got := sessionDescriptionFromUnits(units)
	if len([]rune(got)) != DescriptionMaxChars+1 || !strings.HasSuffix(got, "…") {
		t.Fatalf("unicode truncation = %d runes", len([]rune(got)))
	}
	if got := sessionDescriptionFromUnits(nil); got != "" {
		t.Fatalf("empty units description = %q", got)
	}
}
