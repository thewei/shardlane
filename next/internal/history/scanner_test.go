package history

import (
	"context"
	"os"
	"path/filepath"
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

	catalog, err := OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()

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

	summaries, err := catalog.ListSessions(10)
	if err != nil {
		t.Fatal(err)
	}
	if len(summaries) != 1 || summaries[0].Meta.Title != "Build history" {
		t.Fatalf("scanned sessions = %+v", summaries)
	}
	source := SessionFileRef{
		Agent: AgentCodex, NativeID: summaries[0].Meta.ID, FilePath: summaries[0].Meta.FilePath,
		MtimeMS: fileMTime(t, summaries[0].Meta.FilePath), SizeBytes: int64(len(codexFixtureRollout)),
	}
	window, err := catalog.CachedTranscriptWindow(summaries[0].Meta.Key, source, 0, 60)
	if err != nil || window == nil || len(window.Messages) == 0 {
		t.Fatalf("page cache was not prewarmed: (%+v, %v)", window, err)
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

func TestScannerMissingRootRemovesSessionAfterFullObservation(t *testing.T) {
	home := t.TempDir()
	sessions := filepath.Join(home, "sessions")
	path := writeCodexSessionFile(t, sessions, "rollout-2026-08-02T09-15-00-33333333-aaaa-bbbb-cccc-000000000003.jsonl", codexFixtureRollout)

	catalog, err := OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()
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
	summaries, _ := catalog.ListSessions(10)
	if len(summaries) != 0 {
		t.Fatalf("catalog = %+v", summaries)
	}
}

func TestScannerUnreadableRootAbortsRoundWithoutCleanup(t *testing.T) {
	home := t.TempDir()
	sessions := filepath.Join(home, "sessions")
	writeCodexSessionFile(t, sessions, "rollout-2026-08-02T09-15-00-44444444-aaaa-bbbb-cccc-000000000004.jsonl", codexFixtureRollout)

	catalog, err := OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()
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
	summaries, err := catalog.ListSessions(10)
	if err != nil {
		t.Fatal(err)
	}
	if len(summaries) != 1 {
		t.Fatalf("incomplete observation must not clean the catalog: %+v", summaries)
	}
}

func TestScannerCancellation(t *testing.T) {
	home := t.TempDir()
	sessions := filepath.Join(home, "sessions")
	writeCodexSessionFile(t, sessions, "rollout-2026-08-02T09-15-00-55555555-aaaa-bbbb-cccc-000000000005.jsonl", codexFixtureRollout)

	catalog, err := OpenCatalog(filepath.Join(t.TempDir(), "catalog.db"))
	if err != nil {
		t.Fatal(err)
	}
	defer catalog.Close()
	scanner := NewScanner(catalog, []SourceRoot{{Agent: AgentCodex, Directory: sessions, NativeID: rolloutNativeID}})

	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := scanner.Scan(ctx); err == nil {
		t.Fatal("expected cancellation error")
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
