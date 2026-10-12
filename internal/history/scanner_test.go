package history

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"
)

func writeCodexSessionFile(t *testing.T, dir, name string, body string) string {
	t.Helper()
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, name)
	if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestScannerIndexesChangedSourcesOnce(t *testing.T) {
	home := t.TempDir()
	root := filepath.Join(home, "codex")
	sessions := filepath.Join(root, "sessions")
	writeCodexSessionFile(t, sessions, "rollout-2026-08-02T09-15-00-22222222-aaaa-bbbb-cccc-000000000002.jsonl", codexFixtureRollout)

	catalog := NewCatalog()

	scanner := NewScanner(catalog, []SourceRoot{{
		Agent: AgentCodex, Directory: sessions, NativeID: rolloutNativeID,
	}})

	result, err := scanner.Scan(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if result.Scanned != 1 || result.Changed != 1 || result.Removed != 0 {
		t.Fatalf("first scan = %+v", result)
	}

	summaries := catalog.ListSessions(10)
	if len(summaries) != 1 || summaries[0].Meta.Title != "Build history" {
		t.Fatalf("scanned sessions = %+v", summaries)
	}
	if summaries[0].Description != "Build history" {
		t.Fatalf("description line = %q", summaries[0].Description)
	}

	// Unchanged sources are not re-parsed.
	result, err = scanner.Scan(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if result.Changed != 0 {
		t.Fatalf("second scan changed = %+v", result)
	}

	// A touched source is re-indexed.
	future := time.Now().Add(2 * time.Second)
	if err := os.Chtimes(summaries[0].Meta.FilePath, future, future); err != nil {
		t.Fatal(err)
	}
	result, err = scanner.Scan(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if result.Changed != 1 {
		t.Fatalf("touched scan = %+v", result)
	}
}

// TestScannerReindexesWhenSizeChangesAtSameMtime pins the change signal: a
// provider file rewritten within one mtime tick is still re-read because its
// size differs.
func TestScannerReindexesWhenSizeChangesAtSameMtime(t *testing.T) {
	sessions := filepath.Join(t.TempDir(), "sessions")
	path := writeCodexSessionFile(t, sessions, "rollout-2026-08-02T09-15-00-22222222-aaaa-bbbb-cccc-000000000002.jsonl", codexFixtureRollout)
	catalog := NewCatalog()
	scanner := NewScanner(catalog, []SourceRoot{{Agent: AgentCodex, Directory: sessions, NativeID: rolloutNativeID}})
	if _, err := scanner.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}

	stamp := fileMTime(t, path)
	grown := codexFixtureRollout + "\n"
	if err := os.WriteFile(path, []byte(grown), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Chtimes(path, time.UnixMilli(stamp), time.UnixMilli(stamp)); err != nil {
		t.Fatal(err)
	}
	result, err := scanner.Scan(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if result.Changed != 1 {
		t.Fatalf("size change at the same mtime was skipped: %+v", result)
	}
}

func TestScannerMissingRootRemovesSessionAfterFullObservation(t *testing.T) {
	home := t.TempDir()
	sessions := filepath.Join(home, "sessions")
	path := writeCodexSessionFile(t, sessions, "rollout-2026-08-02T09-15-00-33333333-aaaa-bbbb-cccc-000000000003.jsonl", codexFixtureRollout)

	catalog := NewCatalog()
	scanner := NewScanner(catalog, []SourceRoot{{Agent: AgentCodex, Directory: sessions, NativeID: rolloutNativeID}})
	if _, err := scanner.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}

	// The provider root disappearing is a legitimate empty source: cleanup
	// follows the complete observation.
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	result, err := scanner.Scan(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if result.Removed != 1 {
		t.Fatalf("removed = %+v", result)
	}
	if summaries := catalog.ListSessions(10); len(summaries) != 0 {
		t.Fatalf("catalog = %+v", summaries)
	}
}

func TestScannerUnreadableRootAbortsRoundWithoutCleanup(t *testing.T) {
	home := t.TempDir()
	sessions := filepath.Join(home, "sessions")
	writeCodexSessionFile(t, sessions, "rollout-2026-08-02T09-15-00-44444444-aaaa-bbbb-cccc-000000000004.jsonl", codexFixtureRollout)

	catalog := NewCatalog()
	scanner := NewScanner(catalog, []SourceRoot{{Agent: AgentCodex, Directory: sessions, NativeID: rolloutNativeID}})
	if _, err := scanner.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}

	// Replace the root with a regular file: the round must error and the
	// catalog must keep the previously indexed session.
	if err := os.RemoveAll(sessions); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(sessions, []byte("not a directory"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := scanner.Scan(context.Background()); err == nil {
		t.Fatal("expected an observation error for the unreadable root")
	}
	if summaries := catalog.ListSessions(10); len(summaries) != 1 {
		t.Fatalf("incomplete observation must not clean the catalog: %+v", summaries)
	}
}

func TestScannerCancellation(t *testing.T) {
	home := t.TempDir()
	sessions := filepath.Join(home, "sessions")
	writeCodexSessionFile(t, sessions, "rollout-2026-08-02T09-15-00-55555555-aaaa-bbbb-cccc-000000000005.jsonl", codexFixtureRollout)

	catalog := NewCatalog()
	scanner := NewScanner(catalog, []SourceRoot{{Agent: AgentCodex, Directory: sessions, NativeID: rolloutNativeID}})

	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := scanner.Scan(ctx); err == nil {
		t.Fatal("expected cancellation error")
	}
}

// TestScannerParallelRoundMatchesSequential pins that the worker pool leaves
// the catalog exactly as a one-worker round does.
func TestScannerParallelRoundMatchesSequential(t *testing.T) {
	sessions := filepath.Join(t.TempDir(), "sessions")
	for i := 0; i < 24; i++ {
		name := fmt.Sprintf("rollout-2026-08-02T09-15-00-22222222-aaaa-bbbb-cccc-%012d.jsonl", i)
		body := strings.Replace(codexFixtureRollout, "Build history", fmt.Sprintf("Build history %d", i), 1)
		writeCodexSessionFile(t, sessions, name, body)
	}
	scan := func(workers int) ([]SessionSummary, ScanResult) {
		scanner := NewScanner(NewCatalog(), []SourceRoot{{Agent: AgentCodex, Directory: sessions, NativeID: rolloutNativeID}})
		scanner.workers = workers
		result, err := scanner.Scan(context.Background())
		if err != nil {
			t.Fatal(err)
		}
		return scanner.catalog.ListSessions(100), result
	}
	sequential, sequentialResult := scan(1)
	parallel, parallelResult := scan(4)
	if sequentialResult.Changed != 24 || sequentialResult != parallelResult {
		t.Fatalf("results differ: sequential %+v, parallel %+v", sequentialResult, parallelResult)
	}
	if !reflect.DeepEqual(sequential, parallel) {
		t.Fatalf("catalogs differ:\nsequential %+v\nparallel   %+v", sequential, parallel)
	}
}

// TestScannerCommitsInListingOrderWhateverTheParseOrder pins the collision
// rule: of two files that map to one session key the later listing wins,
// even when the earlier file is the one whose parse finishes last.
func TestScannerCommitsInListingOrderWhateverTheParseOrder(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "sessions")
	first := writeCodexSessionFile(t, dir, "a.jsonl", "x")
	second := writeCodexSessionFile(t, dir, "b.jsonl", "x")
	scanner := NewScanner(NewCatalog(), []SourceRoot{{Agent: AgentCodex, Directory: dir, NativeID: func(string) string { return "same" }}})
	scanner.workers = 2
	scanner.parse = func(reference SessionFileRef) (ParsedTranscript, error) {
		if reference.FilePath == first {
			time.Sleep(60 * time.Millisecond)
		}
		return ParsedTranscript{Meta: SessionMeta{Key: "codex:same", ID: "same", Agent: AgentCodex, FilePath: reference.FilePath}}, nil
	}
	if _, err := scanner.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	rows := scanner.catalog.ListSessions(10)
	if len(rows) != 1 || rows[0].Meta.FilePath != second {
		t.Fatalf("the later listing must win; rows = %+v", rows)
	}
}

// TestScannerParseFailureAbortsRoundWithoutCleanup pins that one unreadable
// source fails the round and leaves the catalog uncleaned: a session whose
// file is gone stays listed until a complete round can prove it is missing.
func TestScannerParseFailureAbortsRoundWithoutCleanup(t *testing.T) {
	sessions := filepath.Join(t.TempDir(), "sessions")
	var paths []string
	for i := 0; i < 3; i++ {
		name := fmt.Sprintf("rollout-2026-08-02T09-15-00-22222222-aaaa-bbbb-cccc-%012d.jsonl", i)
		paths = append(paths, writeCodexSessionFile(t, sessions, name, codexFixtureRollout))
	}
	scanner := NewScanner(NewCatalog(), []SourceRoot{{Agent: AgentCodex, Directory: sessions, NativeID: rolloutNativeID}})
	if _, err := scanner.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}

	if err := os.Remove(paths[2]); err != nil {
		t.Fatal(err)
	}
	future := time.Now().Add(2 * time.Second)
	for _, path := range paths[:2] {
		if err := os.Chtimes(path, future, future); err != nil {
			t.Fatal(err)
		}
	}
	scanner.parse = func(reference SessionFileRef) (ParsedTranscript, error) {
		if reference.FilePath == paths[1] {
			return ParsedTranscript{}, errors.New("unreadable source")
		}
		return parseByAgent(reference)
	}
	if _, err := scanner.Scan(context.Background()); err == nil || !strings.Contains(err.Error(), "unreadable source") {
		t.Fatalf("round error = %v, want the parse failure", err)
	}
	if rows := scanner.catalog.ListSessions(10); len(rows) != 3 {
		t.Fatalf("a failed round must not clean the catalog: %+v", rows)
	}
}

// TestScannerSurfacesRowsInBatchesOfOneHundred pins the progressive-scan
// contract: the observer fires after every 100 queryable sources plus the
// tail, each time with its batch already committed, so a tree of thousands
// of single-session files surfaces in ~100-row batches instead of landing
// all at once when the round completes.
func TestScannerSurfacesRowsInBatchesOfOneHundred(t *testing.T) {
	sessions := filepath.Join(t.TempDir(), "sessions")
	const total = 250
	for i := 0; i < total; i++ {
		name := fmt.Sprintf("rollout-2026-08-02T09-15-00-22222222-aaaa-bbbb-cccc-%012d.jsonl", i)
		body := strings.Replace(codexFixtureRollout, "Build history", fmt.Sprintf("Build history %d", i), 1)
		writeCodexSessionFile(t, sessions, name, body)
	}
	catalog := NewCatalog()
	scanner := NewScanner(catalog, []SourceRoot{{Agent: AgentCodex, Directory: sessions, NativeID: rolloutNativeID}})
	scanner.workers = 1
	var batches []int
	result, err := scanner.scan(context.Background(), func() {
		batches = append(batches, len(catalog.ListSessions(total)))
	})
	if err != nil {
		t.Fatal(err)
	}
	if result.Changed != total {
		t.Fatalf("changed = %+v", result)
	}
	if !reflect.DeepEqual(batches, []int{100, 200, total}) {
		t.Fatalf("batch sizes = %v, want one queryable batch per 100 sources plus the tail", batches)
	}
	if rows := len(catalog.ListSessions(total)); rows != total {
		t.Fatalf("catalog rows = %d, want %d", rows, total)
	}
}

func fileMTime(t *testing.T, path string) int64 {
	t.Helper()
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	return info.ModTime().UnixMilli()
}
