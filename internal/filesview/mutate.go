// mutate.go owns the Files tree's bounded local file operations (FILES-02):
// create file/folder, rename in place, delete. Every operation is validated
// against the tree root — clean relative paths, single-segment names, and a
// symlink-canonicalized parent check so a symlinked directory inside the
// tree never becomes a mutation tunnel outside it. The final segment of a
// delete/rename target may itself be a symlink: it is unlinked or renamed
// as a link, never followed. These are plain filesystem capabilities of the
// client; git index state stays with the Git Workbench, and no Herdr method
// is involved.

package filesview

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// ErrBadName reports a name that cannot be one child entry.
var ErrBadName = errors.New("invalid name")

// ValidateName refuses names that are blank, carry path separators (on any
// platform, so a cross-platform tree cannot create names Windows breaks on)
// or NUL, or are relative path segments themselves.
func ValidateName(name string) error {
	switch {
	case strings.TrimSpace(name) == "":
		return fmt.Errorf("%w: blank", ErrBadName)
	case strings.ContainsRune(name, 0):
		return fmt.Errorf("%w: contains NUL", ErrBadName)
	case strings.ContainsAny(name, `/\`), strings.ContainsRune(name, os.PathSeparator):
		return fmt.Errorf("%w: path separators are not allowed", ErrBadName)
	case name == ".", name == "..":
		return fmt.Errorf("%w: %q is a relative path segment", ErrBadName, name)
	}
	return nil
}

// resolveWithin joins root and rel and verifies the target stays inside
// root. The parent chain is canonicalized: an intermediate symlinked
// directory (root/link/sub with link → elsewhere) must not tunnel a
// mutation outside the tree. The final segment is left as written — a
// symlink entry is unlinked/renamed as a link, never followed. A missing
// parent passes here and fails at the OS call with its own error.
func resolveWithin(root, rel string) (string, error) {
	if root == "" {
		return "", errors.New("no folder open")
	}
	cleanRel := filepath.Clean(rel)
	if filepath.IsAbs(cleanRel) ||
		cleanRel == ".." || strings.HasPrefix(cleanRel, ".."+string(filepath.Separator)) {
		return "", fmt.Errorf("path %q is outside the folder", rel)
	}
	full := filepath.Join(root, cleanRel)
	if cleanRel == "." {
		return full, nil // the root itself: valid parent, refused for mutation
	}
	canonicalRoot, err := filepath.EvalSymlinks(root)
	if err != nil {
		return full, nil // missing root fails at the OS call, not here
	}
	resolved, err := filepath.EvalSymlinks(filepath.Dir(full))
	if err != nil {
		return full, nil
	}
	inside, err := filepath.Rel(canonicalRoot, resolved)
	if err != nil || inside == ".." || strings.HasPrefix(inside, ".."+string(filepath.Separator)) {
		return "", fmt.Errorf("path %q is outside the folder", rel)
	}
	return full, nil
}

// refuseRootEntry keeps the root itself out of the mutating operations: it
// is the tree's anchor, not an entry.
func refuseRootEntry(rel string) error {
	if rel == "" || filepath.Clean(rel) == "." {
		return errors.New("the folder itself cannot be renamed or deleted")
	}
	return nil
}

// CreateFile creates one empty file inside dirRel. The name must not exist.
// Returns the created absolute path.
func CreateFile(root, dirRel, name string) (string, error) {
	if err := ValidateName(name); err != nil {
		return "", err
	}
	target, err := resolveWithin(root, filepath.Join(dirRel, name))
	if err != nil {
		return "", err
	}
	file, err := os.OpenFile(target, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o644)
	if err != nil {
		return "", err
	}
	return target, file.Close()
}

// CreateDir creates one directory inside dirRel (single level, so a typo
// cannot build an unexpected hierarchy). The name must not exist.
func CreateDir(root, dirRel, name string) (string, error) {
	if err := ValidateName(name); err != nil {
		return "", err
	}
	target, err := resolveWithin(root, filepath.Join(dirRel, name))
	if err != nil {
		return "", err
	}
	return target, os.Mkdir(target, 0o755)
}

// Rename renames the entry at rel within its own directory. A distinct
// existing target is refused rather than silently replaced (os.Rename would
// overwrite it); a case-only variant of the same name is allowed through —
// on case-insensitive filesystems (default APFS) it collides in Lstat but
// is the ordinary "fix the casing" rename.
func Rename(root, rel, newName string) (string, error) {
	if err := refuseRootEntry(rel); err != nil {
		return "", err
	}
	if err := ValidateName(newName); err != nil {
		return "", err
	}
	oldAbs, err := resolveWithin(root, rel)
	if err != nil {
		return "", err
	}
	newAbs := filepath.Join(filepath.Dir(oldAbs), newName)
	if _, err := os.Lstat(newAbs); err == nil && !strings.EqualFold(newAbs, oldAbs) {
		return "", fmt.Errorf("%q already exists", newName)
	}
	return newAbs, os.Rename(oldAbs, newAbs)
}

// Delete removes the entry at rel. Directories are removed recursively (the
// UI confirms first); a symlink is unlinked, never followed.
func Delete(root, rel string) (string, error) {
	if err := refuseRootEntry(rel); err != nil {
		return "", err
	}
	abs, err := resolveWithin(root, rel)
	if err != nil {
		return "", err
	}
	info, err := os.Lstat(abs)
	if err != nil {
		return "", err
	}
	if info.IsDir() {
		return abs, os.RemoveAll(abs)
	}
	return abs, os.Remove(abs)
}
