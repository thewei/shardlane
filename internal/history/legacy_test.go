package history

import (
	"os"
	"path/filepath"
	"testing"
)

// TestRemoveLegacyCatalogDeletesSqliteFamilyOnly pins the migration cleanup:
// the old catalog and its sidecars go, every other user-data file stays, and
// repeating the call on a clean directory is a no-op.
func TestRemoveLegacyCatalogDeletesSqliteFamilyOnly(t *testing.T) {
	dir := t.TempDir()
	write := func(name string, size int) string {
		path := filepath.Join(dir, name)
		if err := os.WriteFile(path, make([]byte, size), 0o600); err != nil {
			t.Fatal(err)
		}
		return path
	}
	write(LegacyCatalogFileName, 100)
	write(LegacyCatalogFileName+"-wal", 20)
	write(LegacyCatalogFileName+"-shm", 3)
	settings := write("settings.json", 7)
	ledger := write("follow-up-ledger.json", 9)

	freed, err := RemoveLegacyCatalog(dir)
	if err != nil {
		t.Fatal(err)
	}
	if freed != 123 {
		t.Fatalf("freed = %d, want 123", freed)
	}
	for _, name := range []string{LegacyCatalogFileName, LegacyCatalogFileName + "-wal", LegacyCatalogFileName + "-shm"} {
		if _, err := os.Stat(filepath.Join(dir, name)); !os.IsNotExist(err) {
			t.Fatalf("%s survived the cleanup (err=%v)", name, err)
		}
	}
	for _, kept := range []string{settings, ledger} {
		if _, err := os.Stat(kept); err != nil {
			t.Fatalf("cleanup touched %s: %v", kept, err)
		}
	}

	if freed, err := RemoveLegacyCatalog(dir); err != nil || freed != 0 {
		t.Fatalf("second call = (%d, %v), want (0, nil)", freed, err)
	}
	if freed, err := RemoveLegacyCatalog(filepath.Join(dir, "missing")); err != nil || freed != 0 {
		t.Fatalf("missing directory = (%d, %v), want (0, nil)", freed, err)
	}
}
