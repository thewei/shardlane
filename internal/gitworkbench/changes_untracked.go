package gitworkbench

import (
	"fmt"
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
// enter diff review.
func untrackedEntry(root, path string) (ChangeFile, bool) {
	full := filepath.Join(root, path)
	info, err := os.Lstat(full)
	if err != nil {
		return ChangeFile{}, false
	}
	if info.IsDir() {
		return ChangeFile{}, false
	}

	if info.Mode()&os.ModeSymlink != 0 {
		target, err := os.Readlink(full)
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

	if info.Size() > MaxPatchBytesPerFile {
		return ChangeFile{
			Path: path, Status: StatusUntracked, Untracked: true,
			TooLarge: true, Fingerprint: fingerprint(StatusUntracked, path, "", 0, 0, nil),
		}, true
	}
	data, err := os.ReadFile(full)
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
		if parsed := parsePatch(cf.Patch); parsed != nil {
			cf.Hunks = parsed.hunks
		}
	}
	cf.Fingerprint = fingerprint(StatusUntracked, path, "", cf.Additions, 0, cf.Patch)
	return cf, true
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
