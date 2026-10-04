package filesview

import (
	"os"
	"path/filepath"
	"testing"
)

// TestListDirectStableSortPinsDirectoriesFirst pins WIX-050/051: direct
// children only, stable sorting with directories first, then alphabetical.
func TestListDirectStableSortPinsDirectoriesFirst(t *testing.T) {
	dir := t.TempDir()

	// Create a mixed tree: files and directories with mixed casing.
	if err := os.Mkdir(filepath.Join(dir, "src"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(filepath.Join(dir, "Docs"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "README.md"), []byte("# Hello\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "main.go"), []byte("package main\n"), 0o600); err != nil {
		t.Fatal(err)
	}

	entries, err := ListDirect(dir, dir)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 4 {
		t.Fatalf("expected 4 entries, got %d", len(entries))
	}

	// Dirs first: Docs, src (case-insensitive "docs" < "src")
	if !entries[0].IsDir || entries[0].Name != "Docs" {
		t.Fatalf("expected entry 0 to be dir Docs, got %+v", entries[0])
	}
	if !entries[1].IsDir || entries[1].Name != "src" {
		t.Fatalf("expected entry 1 to be dir src, got %+v", entries[1])
	}
	// Files next: main.go, README.md ("main.go" < "readme.md")
	if entries[2].IsDir || entries[2].Name != "main.go" {
		t.Fatalf("expected entry 2 to be file main.go, got %+v", entries[2])
	}
	if entries[3].IsDir || entries[3].Name != "README.md" {
		t.Fatalf("expected entry 3 to be file README.md, got %+v", entries[3])
	}
}

// TestSymlinkToDirectoryPinsNoRecursiveFollow pins WIX-052: symlinks to
// directories are detected as TargetDir but never expanded recursively.
func TestSymlinkToDirectoryPinsNoRecursiveFollow(t *testing.T) {
	dir := t.TempDir()
	targetDir := filepath.Join(dir, "real-target")
	if err := os.Mkdir(targetDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(targetDir, "nested.txt"), []byte("nested"), 0o600); err != nil {
		t.Fatal(err)
	}

	linkPath := filepath.Join(dir, "sym-link")
	if err := os.Symlink(targetDir, linkPath); err != nil {
		t.Skip("symlink creation not supported on this platform/filesystem")
	}

	entries, err := ListDirect(dir, dir)
	if err != nil {
		t.Fatal(err)
	}

	found := false
	for _, e := range entries {
		if e.Name == "sym-link" {
			found = true
			if !e.IsSymlink {
				t.Fatalf("expected IsSymlink = true for sym-link")
			}
			if !e.TargetDir {
				t.Fatalf("expected TargetDir = true for symlink to dir")
			}
			if e.IsDir {
				t.Fatalf("IsDir must stay false for symlinks so recursion is prevented")
			}
		}
	}
	if !found {
		t.Fatal("sym-link not found in listing")
	}
}

// TestReadPreviewTextAndBinary pins WIX-054: text files return their content;
// files with NUL bytes are classified as binary.
func TestReadPreviewTextAndBinary(t *testing.T) {
	dir := t.TempDir()

	textPath := filepath.Join(dir, "sample.txt")
	textContent := "Hello, Shardlane!\nSecond line.\n"
	if err := os.WriteFile(textPath, []byte(textContent), 0o600); err != nil {
		t.Fatal(err)
	}

	preview, err := ReadPreview(textPath)
	if err != nil {
		t.Fatal(err)
	}
	if preview.IsBinary {
		t.Fatal("sample.txt must be classified as text")
	}
	if preview.Content != textContent {
		t.Fatalf("preview content mismatch: got %q, want %q", preview.Content, textContent)
	}
	if preview.Truncated {
		t.Fatal("small text file must not be truncated")
	}

	// Binary file with NUL byte
	binPath := filepath.Join(dir, "sample.bin")
	binContent := []byte{0x7f, 'E', 'L', 'F', 0x00, 0x01, 0x02}
	if err := os.WriteFile(binPath, binContent, 0o600); err != nil {
		t.Fatal(err)
	}

	binPreview, err := ReadPreview(binPath)
	if err != nil {
		t.Fatal(err)
	}
	if !binPreview.IsBinary {
		t.Fatal("sample.bin must be classified as binary")
	}
	if binPreview.Content != "" {
		t.Fatalf("binary preview must not include raw content; got %q", binPreview.Content)
	}
}

// TestReadPreviewCapPins1MiBLimit pins WIX-055: files larger than 1 MiB are
// truncated with Truncated = true.
func TestReadPreviewCapPins1MiBLimit(t *testing.T) {
	dir := t.TempDir()
	largePath := filepath.Join(dir, "large.txt")

	// Create 1.1 MiB of ASCII text
	size := PreviewCap + 100*1024
	buf := make([]byte, size)
	for i := range buf {
		buf[i] = 'A'
	}
	if err := os.WriteFile(largePath, buf, 0o600); err != nil {
		t.Fatal(err)
	}

	preview, err := ReadPreview(largePath)
	if err != nil {
		t.Fatal(err)
	}
	if !preview.Truncated {
		t.Fatal("large file must have Truncated = true")
	}
	if len(preview.Content) != PreviewCap {
		t.Fatalf("content length = %d, want PreviewCap %d", len(preview.Content), PreviewCap)
	}
}

// TestFormatFileSize pins human-readable size outputs.
func TestFormatFileSize(t *testing.T) {
	cases := []struct {
		bytes int64
		want  string
	}{
		{500, "500 B"},
		{1024, "1.0 KB"},
		{2048, "2.0 KB"},
		{1024 * 1024, "1.0 MB"},
		{5 * 1024 * 1024, "5.0 MB"},
		{2 * 1024 * 1024 * 1024, "2.0 GB"},
	}
	for _, tc := range cases {
		if got := FormatFileSize(tc.bytes); got != tc.want {
			t.Fatalf("FormatFileSize(%d) = %q, want %q", tc.bytes, got, tc.want)
		}
	}
}
