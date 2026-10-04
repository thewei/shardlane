package scripts

import (
	"path/filepath"
	"testing"
)

// TestScriptStoreCRUDAndAtomicPersistence pins WIX-070..075: atomic JSON
// persistence, CRUD operations, project scoping, and validation.
func TestScriptStoreCRUDAndAtomicPersistence(t *testing.T) {
	dir := t.TempDir()
	storePath := filepath.Join(dir, "scripts.json")

	store, err := NewStore(storePath)
	if err != nil {
		t.Fatal(err)
	}

	// Validation
	if err := store.Save(ScriptDefinition{}); err == nil {
		t.Fatal("empty script ID must fail validation")
	}

	devScript := ScriptDefinition{
		ID:          "script-1",
		ProjectPath: "/work/web",
		Name:        "Dev Server",
		Command:     "npm run dev",
		OneShot:     false,
	}
	buildScript := ScriptDefinition{
		ID:          "script-2",
		ProjectPath: "/work/web",
		Name:        "Build",
		Command:     "npm run build",
		OneShot:     true,
	}
	otherScript := ScriptDefinition{
		ID:          "script-3",
		ProjectPath: "/work/other",
		Name:        "Test",
		Command:     "cargo test",
		OneShot:     true,
	}

	if err := store.Save(devScript); err != nil {
		t.Fatal(err)
	}
	if err := store.Save(buildScript); err != nil {
		t.Fatal(err)
	}
	if err := store.Save(otherScript); err != nil {
		t.Fatal(err)
	}

	// Project-scoped list
	webScripts := store.List("/work/web")
	if len(webScripts) != 2 {
		t.Fatalf("expected 2 scripts for /work/web, got %d", len(webScripts))
	}

	// Reload from disk to verify atomic persistence
	reloaded, err := NewStore(storePath)
	if err != nil {
		t.Fatal(err)
	}
	if len(reloaded.List("")) != 3 {
		t.Fatalf("reloaded store expected 3 scripts, got %d", len(reloaded.List("")))
	}

	// Update existing script
	devScript.Name = "Vite Dev"
	if err := store.Save(devScript); err != nil {
		t.Fatal(err)
	}
	updated, ok := store.Get("script-1")
	if !ok || updated.Name != "Vite Dev" {
		t.Fatalf("expected updated name 'Vite Dev', got %+v", updated)
	}

	// Delete
	if err := store.Delete("script-2"); err != nil {
		t.Fatal(err)
	}
	if len(store.List("/work/web")) != 1 {
		t.Fatalf("expected 1 script remaining for /work/web")
	}
	if err := store.Delete("non-existent"); err == nil {
		t.Fatal("deleting non-existent script should error")
	}
}
