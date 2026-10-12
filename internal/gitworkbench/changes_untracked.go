package gitworkbench

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"unicode/utf8"
)

// untrackedEntry models one untracked file as a pure addition with a
// clean-room synthesized patch (no subprocess per file). Symlinks are never
// followed (P1-04): an untracked symlink is modeled as git does — mode
// 120000 with the link target as content — so external file bytes can never
// enter diff review. A file past MaxPatchBytesPerFile bytes or LargeDiffLines
// lines is deferred rather than parsed.
func untrackedEntry(root, path string) (ChangeFile, bool) {
	return untrackedEntryWith(root, path, false)
}

// untrackedEntryWith is untrackedEntry; with full set it is the on-demand
// read behind Show: files up to MaxOnDemandPatchBytes are read and keep their
// hunks whatever their length, and a file past that is TooLarge.
func untrackedEntryWith(root, path string, full bool) (ChangeFile, bool) {
	fullPath := filepath.Join(root, path)
	info, err := os.Lstat(fullPath)
	if err != nil {
		return ChangeFile{}, false
	}
	if info.IsDir() {
		return ChangeFile{}, false
	}

	if info.Mode()&os.ModeSymlink != 0 {
		target, err := os.Readlink(fullPath)
		if err != nil {
			return ChangeFile{}, false
		}
		patchText := "diff --git a/" + path + " b/" + path + "\n" +
			"new file mode 120000\n" +
			"--- /dev/null\n" +
			"+++ b/" + path + "\n" +
			"@@ -0,0 +1 @@\n" +
			"+" + target + "\n" +
			"\\ No newline at end of file\n"
		cf := ChangeFile{
			Path:      path,
			Status:    StatusUntracked,
			Untracked: true,
			Additions: 1,
			Patch:     []byte(patchText),
		}
		if parsed := parsePatch(cf.Patch); parsed != nil {
			cf.Hunks = parsed.hunks
		}
		cf.Fingerprint = fingerprint(StatusUntracked, path, "", cf.Additions, 0, cf.Patch)
		return cf, true
	}

	limit := int64(MaxPatchBytesPerFile)
	if full {
		limit = MaxOnDemandPatchBytes
	}
	stamp := fileStamp(info)
	if info.Size() > limit {
		// Not read: a binary shows as one (its first bytes say so), anything
		// else waits behind Show, or is beyond it.
		if sniffBinary(fullPath) {
			return ChangeFile{
				Path: path, Status: StatusUntracked, Untracked: true, Binary: true,
				Patch:       []byte("diff --git a/" + path + " b/" + path + "\nBinary files a/" + path + " and /dev/null differ\n"),
				Fingerprint: fingerprint(StatusUntracked, path, "", 0, 0, stamp),
			}, true
		}
		tooLarge := info.Size() > MaxOnDemandPatchBytes
		return ChangeFile{
			Path: path, Status: StatusUntracked, Untracked: true,
			Deferred: !tooLarge, TooLarge: tooLarge,
			Fingerprint: fingerprint(StatusUntracked, path, "", 0, 0, stamp),
		}, true
	}
	data, err := os.ReadFile(fullPath)
	if err != nil {
		return ChangeFile{}, false
	}
	binary := hasNUL(data)
	text := ""
	if !binary {
		text = decodeText(data)
	}
	cf := ChangeFile{
		Path:      path,
		Status:    StatusUntracked,
		Untracked: true,
		Binary:    binary,
	}
	if binary {
		cf.Patch = []byte("diff --git a/" + path + " b/" + path + "\nBinary files a/" + path + " and /dev/null differ\n")
	} else {
		lines := splitLines(text)
		if !full && len(lines) > LargeDiffLines {
			cf.Deferred, cf.Additions = true, len(lines)
			cf.Fingerprint = fingerprint(StatusUntracked, path, "", cf.Additions, 0, stamp)
			return cf, true
		}
		var buf strings.Builder
		buf.WriteString("diff --git a/" + path + " b/" + path + "\n")
		buf.WriteString("new file mode 100644\n")
		buf.WriteString("--- /dev/null\n")
		buf.WriteString("+++ b/" + path + "\n")
		fmt.Fprintf(&buf, "@@ -0,0 +1,%d @@\n", len(lines))
		for _, line := range lines {
			buf.WriteByte('+')
			buf.WriteString(line)
			buf.WriteByte('\n')
		}
		cf.Patch = []byte(buf.String())
		cf.Additions = len(lines)
		// The synthesized patch is a little longer than the file it adds.
		if parsed := parsePatchLimit(cf.Patch, 2*MaxOnDemandPatchBytes); parsed != nil {
			cf.Hunks = parsed.hunks
			if parsed.tooLarge && full {
				cf.TooLarge, cf.Hunks, cf.Patch = true, nil, nil
			}
		}
	}
	cf.Fingerprint = fingerprint(StatusUntracked, path, "", cf.Additions, 0, cf.Patch)
	return cf, true
}

// sniffBinary reports whether the first bytes of a file hold a NUL, without
// reading the rest.
func sniffBinary(path string) bool {
	f, err := os.Open(path)
	if err != nil {
		return false
	}
	defer f.Close()
	head := make([]byte, 8192)
	n, _ := io.ReadFull(f, head)
	return hasNUL(head[:n])
}

func hasNUL(data []byte) bool {
	n := len(data)
	if n > 8192 {
		n = 8192
	}
	for i := 0; i < n; i++ {
		if data[i] == 0 {
			return true
		}
	}
	return false
}

func splitLines(s string) []string {
	s = strings.ReplaceAll(s, "\r\n", "\n")
	lines := strings.Split(s, "\n")
	if len(lines) > 0 && lines[len(lines)-1] == "" {
		lines = lines[:len(lines)-1]
	}
	return lines
}

// decodeText lossily converts file bytes to UTF-8 text for diff display.
// Valid UTF-8 passes through; anything else is lossily replaced so the UI
// never renders raw invalid sequences.
func decodeText(data []byte) string {
	if utf8.Valid(data) {
		return string(data)
	}
	runes := []rune(string(data))
	for i, r := range runes {
		if r == 0xFFFD {
			runes[i] = '·'
		}
	}
	return string(runes)
}

func sortStrings(values []string) {
	sort.Strings(values)
}

func sortFilesByPath(files []ChangeFile) {
	sort.SliceStable(files, func(i, j int) bool {
		return files[i].Path < files[j].Path
	})
}

func fileExists(path string) bool {
	info, err := os.Stat(path)
	return err == nil && !info.IsDir()
}
