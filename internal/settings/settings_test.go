package settings

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestMemoryStoreRoundTrip(t *testing.T) {
	ctx := context.Background()
	store := NewMemoryStore(Default())
	got, err := store.Load(ctx)
	if err != nil {
		t.Fatal(err)
	}
	got.General.Appearance = "dark"
	if err := store.Save(ctx, got); err != nil {
		t.Fatal(err)
	}
	reloaded, err := store.Load(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if reloaded.General.Appearance != "dark" {
		t.Fatalf("appearance = %q", reloaded.General.Appearance)
	}
}

func TestFileStoreMissingUsesDefaults(t *testing.T) {
	store := NewFileStore(t.TempDir())
	got, err := store.Load(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if got.SchemaVersion != CurrentSchemaVersion || got.Terminal.FontSize != 13 {
		t.Fatalf("unexpected defaults: %#v", got)
	}
}

func TestFileStoreRoundTripAtomically(t *testing.T) {
	dir := t.TempDir()
	store := NewFileStore(dir)
	value := Default()
	value.Terminal.FontSize = 15
	value.General.RestoreWindow = false

	if err := store.Save(context.Background(), value); err != nil {
		t.Fatal(err)
	}
	got, err := store.Load(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if got.Terminal.FontSize != 15 || got.General.RestoreWindow {
		t.Fatalf("roundtrip mismatch: %#v", got)
	}
	info, err := os.Stat(filepath.Join(dir, FileName))
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm() != 0o600 {
		t.Fatalf("settings mode = %o", info.Mode().Perm())
	}
	matches, err := filepath.Glob(filepath.Join(dir, ".settings-*.tmp"))
	if err != nil {
		t.Fatal(err)
	}
	if len(matches) != 0 {
		t.Fatalf("temp files left behind: %v", matches)
	}
}

func TestFileStoreCorruptFallsBackWithError(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, FileName)
	if err := os.WriteFile(path, []byte("{not-json"), 0o600); err != nil {
		t.Fatal(err)
	}
	store := NewFileStoreAt(path)
	got, err := store.Load(context.Background())
	if err == nil {
		t.Fatal("expected decode error")
	}
	if got.SchemaVersion != CurrentSchemaVersion || got.General.Appearance != "system" {
		t.Fatalf("corrupt fallback = %#v", got)
	}
}

func TestFileStoreRejectsFutureSchema(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, FileName)
	if err := os.WriteFile(path, []byte("{\"schema_version\":999}"), 0o600); err != nil {
		t.Fatal(err)
	}
	got, err := NewFileStoreAt(path).Load(context.Background())
	if err == nil {
		t.Fatal("expected schema error")
	}
	if got.SchemaVersion != CurrentSchemaVersion {
		t.Fatalf("future-schema fallback = %#v", got)
	}
}

func TestFileStoreSessionRoundTrip(t *testing.T) {
	store := NewFileStore(t.TempDir())
	value, err := store.Load(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	value.Session = SessionSettings{
		Route:             "/settings/terminal",
		Instance:          "main",
		ProjectID:         "proj-1",
		TabID:             "tab-1",
		PaneID:            "pane-1",
		ProjectsOpen:      true,
		AgentsOpen:        true,
		PinsOpen:          true,
		ExpandedProjects:  []string{"proj-a", "proj-b"},
		ExpandedTabs:      []string{"tab-9"},
		RightPanelOpen:    true,
		RightPanelSurface: "files",
		RightPanelWidth:   420,
	}
	if err := store.Save(context.Background(), value); err != nil {
		t.Fatal(err)
	}
	got, err := store.Load(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	want := value.Session
	if got.Session.Route != want.Route || got.Session.Instance != want.Instance ||
		got.Session.ProjectID != want.ProjectID || got.Session.TabID != want.TabID ||
		got.Session.PaneID != want.PaneID {
		t.Fatalf("session navigation fields = %#v", got.Session)
	}
	if !got.Session.ProjectsOpen || !got.Session.AgentsOpen || !got.Session.PinsOpen {
		t.Fatalf("session visibility flags = %#v", got.Session)
	}
	if len(got.Session.ExpandedProjects) != 2 || len(got.Session.ExpandedTabs) != 1 {
		t.Fatalf("session expansion = %#v / %#v", got.Session.ExpandedProjects, got.Session.ExpandedTabs)
	}
	if !got.Session.RightPanelOpen || got.Session.RightPanelSurface != "files" || got.Session.RightPanelWidth != 420 {
		t.Fatalf("session right panel = %#v", got.Session)
	}
}
