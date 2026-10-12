package filesview

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestValidateNamePinsSingleSegmentNames: separators, dot segments, NUL and
// emptiness are refused; ordinary names — including dotfiles the user may
// legitimately create, like .gitignore — pass.
func TestValidateNamePinsSingleSegmentNames(t *testing.T) {
	bad := []string{"", "  ", "a/b", `a\b`, ".", "..", "x\x00y"}
	for _, name := range bad {
		if err := ValidateName(name); err == nil {
			t.Errorf("ValidateName(%q) = nil, want error", name)
		}
	}
	good := []string{"main.go", ".gitignore", "My File.txt", "数据"}
	for _, name := range good {
		if err := ValidateName(name); err != nil {
			t.Errorf("ValidateName(%q) = %v, want nil", name, err)
		}
	}
}

// TestCreateRenameDeleteRoundTrip covers the happy paths over a real temp
// root, including directory delete removing children.
func TestCreateRenameDeleteRoundTrip(t *testing.T) {
	root := t.TempDir()

	if _, err := CreateDir(root, ".", "src"); err != nil {
		t.Fatal(err)
	}
	created, err := CreateFile(root, "src", "main.go")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(created); err != nil {
		t.Fatalf("created file missing: %v", err)
	}
	if err := os.WriteFile(created, []byte("package main\n"), 0o600); err != nil {
		t.Fatal(err)
	}

	renamed, err := Rename(root, filepath.Join("src", "main.go"), "app.go")
	if err != nil {
		t.Fatal(err)
	}
	if filepath.Base(renamed) != "app.go" {
		t.Fatalf("renamed path = %q", renamed)
	}
	if _, err := os.Stat(filepath.Join(root, "src", "main.go")); !os.IsNotExist(err) {
		t.Fatal("old name still exists after rename")
	}

	if _, err := Delete(root, "src"); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(root, "src")); !os.IsNotExist(err) {
		t.Fatal("directory still exists after delete")
	}
}

// TestMutationsRefuseEscapes pins the guards: dot-segment names, absolute
// paths, parent traversal, and creation inside a symlinked directory that
// resolves outside the root.
func TestMutationsRefuseEscapes(t *testing.T) {
	root := t.TempDir()
	outside := t.TempDir()

	if _, err := CreateFile(root, "..", "escape.txt"); err == nil {
		t.Error("creating above the root must fail")
	}
	if _, err := CreateFile(root, filepath.Join("..", "deeper"), "x.txt"); err == nil {
		t.Error("traversal parent must fail")
	}
	if _, err := CreateFile(root, ".", "../x.txt"); err == nil {
		t.Error("dot-segment name must fail")
	}
	if _, err := Rename(root, filepath.Join("..", "elsewhere"), "y.txt"); err == nil {
		t.Error("renaming outside the root must fail")
	}
	if _, err := Delete(root, "."); err == nil {
		t.Error("deleting the root must fail")
	}

	// A symlinked directory inside the tree pointing outside must not
	// become a creation tunnel.
	link := filepath.Join(root, "jump")
	if err := os.Symlink(outside, link); err != nil {
		t.Fatal(err)
	}
	if _, err := CreateFile(root, "jump", "tunneled.txt"); err == nil {
		t.Error("creation through an outbound symlink must fail")
	}
	if _, err := os.Stat(filepath.Join(outside, "tunneled.txt")); !os.IsNotExist(err) {
		t.Error("file must not appear outside the root")
	}
}

// TestCreateRefusesExistingAndRenameRefusesOverwrite: the destructive side
// of each operation is explicit, never implicit.
func TestCreateRefusesExistingAndRenameRefusesOverwrite(t *testing.T) {
	root := t.TempDir()
	if _, err := CreateFile(root, ".", "a.txt"); err != nil {
		t.Fatal(err)
	}
	if _, err := CreateFile(root, ".", "a.txt"); err == nil {
		t.Error("creating over an existing file must fail")
	}
	if _, err := CreateDir(root, ".", "a.txt"); err == nil {
		t.Error("creating a directory over an existing file must fail")
	}
	if _, err := CreateFile(root, ".", "b.txt"); err != nil {
		t.Fatal(err)
	}
	if _, err := Rename(root, "b.txt", "a.txt"); err == nil {
		t.Error("renaming onto an existing entry must fail")
	}
	// The refusal must be an error, not a silent replace.
	if _, err := os.Stat(filepath.Join(root, "b.txt")); err != nil {
		t.Fatalf("source vanished despite refused rename: %v", err)
	}
}

// TestRenameAllowsCaseOnlyVariant pins the case-only rename: on default
// APFS the variant collides in Lstat, but fixing a name's casing is an
// ordinary rename and must go through (overwriting a genuinely different
// entry stays refused).
func TestRenameAllowsCaseOnlyVariant(t *testing.T) {
	root := t.TempDir()
	if _, err := CreateFile(root, ".", "readme.md"); err != nil {
		t.Fatal(err)
	}
	if _, err := Rename(root, "readme.md", "README.md"); err != nil {
		t.Fatalf("case-only rename refused: %v", err)
	}
	if _, err := os.Stat(filepath.Join(root, "README.md")); err != nil {
		t.Fatalf("renamed file missing: %v", err)
	}
	// A genuinely different existing target is still refused.
	if _, err := CreateFile(root, ".", "other.txt"); err != nil {
		t.Fatal(err)
	}
	if _, err := Rename(root, "other.txt", "README.md"); err == nil {
		t.Error("rename onto a distinct existing entry must fail")
	}
}

// TestDeleteUnlinksSymlinksWithoutFollowing pins that a symlink is removed
// as a link: the target directory survives untouched.
func TestDeleteUnlinksSymlinksWithoutFollowing(t *testing.T) {
	root := t.TempDir()
	target := t.TempDir()
	if err := os.WriteFile(filepath.Join(target, "keep.txt"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(root, "link")
	if err := os.Symlink(target, link); err != nil {
		t.Fatal(err)
	}
	if _, err := Delete(root, "link"); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Lstat(link); !os.IsNotExist(err) {
		t.Fatal("link still exists after delete")
	}
	if _, err := os.Stat(filepath.Join(target, "keep.txt")); err != nil {
		t.Fatalf("symlink target must survive: %v", err)
	}
}

// TestResolveWithinCanonicalizesSymlinkedRoots: a root reached through a
// symlink (e.g. /tmp on macOS) must not flag its own children as escapes.
func TestResolveWithinCanonicalizesSymlinkedRoots(t *testing.T) {
	realRoot := t.TempDir()
	if err := os.WriteFile(filepath.Join(realRoot, "f.txt"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	linkRoot := filepath.Join(t.TempDir(), "root-link")
	if err := os.Symlink(realRoot, linkRoot); err != nil {
		t.Fatal(err)
	}
	abs, err := resolveWithin(linkRoot, "f.txt")
	if err != nil {
		t.Fatalf("child of symlinked root refused: %v", err)
	}
	if !strings.HasSuffix(abs, "f.txt") {
		t.Errorf("resolved path = %q", abs)
	}
}
