// Package filesview owns the read-only Files tool of the Native Right Panel
// (0.9 P4 / WIX-050..055): lazy direct-directory listing, directories-first
// stable sort, symlink no-recursive-follow, 1 MiB preview cap, text/binary
// classification, and copy-path/reveal helpers. Opening the panel performs
// zero repository-wide recursive scans.
package filesview

import (
	"bytes"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"unicode/utf8"
)

// PreviewCap is the 1 MiB maximum size for a file preview (WIX-055).
const PreviewCap = 1024 * 1024

// EntryKind distinguishes directory nodes from files and symlinks.
type EntryKind string

const (
	KindDir     EntryKind = "dir"
	KindFile    EntryKind = "file"
	KindSymlink EntryKind = "symlink"
)

// Entry is one direct child of a listed directory.
type Entry struct {
	Name      string    `json:"name"`
	Path      string    `json:"path"` // absolute path
	RelPath   string    `json:"rel_path"`
	Kind      EntryKind `json:"kind"`
	SizeBytes int64     `json:"size_bytes"`
	IsDir     bool      `json:"is_dir"`
	IsSymlink bool      `json:"is_symlink"`
	// TargetDir marks a symlink pointing to a directory; the no-recursive-
	// follow rule prevents expanding it in the tree (WIX-052).
	TargetDir bool `json:"target_dir"`
}

// Preview carries the read-only content of one inspected file.
type Preview struct {
	Path      string `json:"path"`
	SizeBytes int64  `json:"size_bytes"`
	IsBinary  bool   `json:"is_binary"`
	Truncated bool   `json:"truncated"`
	Content   string `json:"content"`
}

// ErrNotADirectory reports attempting to list a non-directory.
var ErrNotADirectory = errors.New("path is not a directory")

// ListDirect lists only the immediate children of dir (no recursion;
// WIX-050). Results sort directories first, then alphabetical case-
// insensitive (WIX-051). Hidden files/directories (.git, .DS_Store, ...)
// stay visible by default so developers see project realities.
func ListDirect(dir string, root string) ([]Entry, error) {
	info, err := os.Stat(dir)
	if err != nil {
		return nil, err
	}
	if !info.IsDir() {
		return nil, fmt.Errorf("%w: %s", ErrNotADirectory, dir)
	}

	rawEntries, err := os.ReadDir(dir)
	if err != nil {
		return nil, err
	}

	entries := make([]Entry, 0, len(rawEntries))
	for _, raw := range rawEntries {
		name := raw.Name()
		fullPath := filepath.Join(dir, name)
		relPath, _ := filepath.Rel(root, fullPath)
		if relPath == "" || relPath == "." {
			relPath = name
		}

		info, err := raw.Info()
		var size int64
		if err == nil {
			size = info.Size()
		}

		isSymlink := raw.Type()&os.ModeSymlink != 0
		isDir := raw.IsDir()
		targetDir := false

		if isSymlink {
			// Resolve symlink target without following deeply.
			target, err := filepath.EvalSymlinks(fullPath)
			if err == nil {
				if targetInfo, err := os.Stat(target); err == nil {
					targetDir = targetInfo.IsDir()
				}
			}
		}

		kind := KindFile
		switch {
		case isDir:
			kind = KindDir
		case isSymlink:
			kind = KindSymlink
		}

		entries = append(entries, Entry{
			Name:      name,
			Path:      fullPath,
			RelPath:   relPath,
			Kind:      kind,
			SizeBytes: size,
			IsDir:     isDir,
			IsSymlink: isSymlink,
			TargetDir: targetDir,
		})
	}

	SortEntries(entries)
	return entries, nil
}

// SortEntries applies the stable WIX-051 order: directories first, then
// alphabetical case-insensitive.
func SortEntries(entries []Entry) {
	sort.SliceStable(entries, func(i, j int) bool {
		// Treat symlinks-to-directory as directories for sorting.
		dirI := entries[i].IsDir || entries[i].TargetDir
		dirJ := entries[j].IsDir || entries[j].TargetDir
		if dirI != dirJ {
			return dirI
		}
		// Alphabetical case-insensitive; on tie, preserve exact byte order.
		lowerI := strings.ToLower(entries[i].Name)
		lowerJ := strings.ToLower(entries[j].Name)
		if lowerI != lowerJ {
			return lowerI < lowerJ
		}
		return entries[i].Name < entries[j].Name
	})
}

// ReadPreview reads a file up to PreviewCap (1 MiB; WIX-055), classifying
// text vs binary (WIX-054). Truncated is true when the file exceeds 1 MiB.
// If root is provided, path is verified to stay within root (symlink escape guard).
func ReadPreview(path string, root ...string) (Preview, error) {
	if len(root) > 0 && root[0] != "" {
		canonicalRoot, err1 := filepath.EvalSymlinks(root[0])
		canonicalPath, err2 := filepath.EvalSymlinks(path)
		if err1 == nil && err2 == nil {
			rel, err := filepath.Rel(canonicalRoot, canonicalPath)
			if err != nil || strings.HasPrefix(rel, "..") {
				return Preview{}, fmt.Errorf("preview path %q escapes root %q", path, root[0])
			}
		}
	}

	info, err := os.Stat(path)
	if err != nil {
		return Preview{}, err
	}
	if info.IsDir() {
		return Preview{}, errors.New("cannot preview a directory")
	}

	size := info.Size()
	file, err := os.Open(path)
	if err != nil {
		return Preview{}, err
	}
	defer file.Close()

	// Read up to PreviewCap + 1 byte so truncation is provable.
	limited := io.LimitReader(file, PreviewCap+1)
	buf, err := io.ReadAll(limited)
	if err != nil {
		return Preview{}, err
	}

	truncated := false
	if len(buf) > PreviewCap {
		buf = buf[:PreviewCap]
		truncated = true
	}

	isBinary := IsBinary(buf)
	content := ""
	if !isBinary {
		content = string(buf)
	}

	return Preview{
		Path:      path,
		SizeBytes: size,
		IsBinary:  isBinary,
		Truncated: truncated,
		Content:   content,
	}, nil
}

// IsBinary detects binary content (WIX-054): contains NUL bytes in the first
// 8 KiB or is not valid UTF-8.
func IsBinary(buf []byte) bool {
	probeLen := len(buf)
	if probeLen > 8192 {
		probeLen = 8192
	}
	probe := buf[:probeLen]
	if bytes.IndexByte(probe, 0) >= 0 {
		return true
	}
	return !utf8.Valid(probe)
}

// FormatFileSize returns a compact, human-readable size string.
func FormatFileSize(bytes int64) string {
	switch {
	case bytes >= 1024*1024*1024:
		return fmt.Sprintf("%.1f GB", float64(bytes)/(1024*1024*1024))
	case bytes >= 1024*1024:
		return fmt.Sprintf("%.1f MB", float64(bytes)/(1024*1024))
	case bytes >= 1024:
		return fmt.Sprintf("%.1f KB", float64(bytes)/1024)
	default:
		return fmt.Sprintf("%d B", bytes)
	}
}
