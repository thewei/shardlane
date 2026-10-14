package nativeui

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

// Copy goes through MyGo's clipboard with the text untouched; a failed write
// says so in the status line instead of looking like it worked.
func TestCopyPutsTheTextOnTheClipboardIntact(t *testing.T) {
	s := NewShell()
	s.status = ""
	got := captureClipboard(t)
	const text = "/Users/me/项目/测试.txt"
	s.copyToClipboard(text)
	if len(*got) != 1 || (*got)[0] != text {
		t.Fatalf("clipboard writes = %q, want exactly %q", *got, text)
	}
	if s.status != "" {
		t.Fatalf("a good copy left the status %q", s.status)
	}

	writeClipboard = func(string) error { return errors.New("pasteboard busy") }
	s.copyToClipboard("x")
	if s.status != "Clipboard unavailable" {
		t.Fatalf("a failed copy left the status %q", s.status)
	}
}

// Reveal hands an existing path to Finder as it is, and a path that is gone
// is reported rather than silently doing nothing (F175).
func TestRevealShowsAnExistingPathAndReportsAGoneOne(t *testing.T) {
	s := NewShell()
	s.status = ""
	got := captureReveal(t)
	file := filepath.Join(t.TempDir(), "notes.txt")
	if err := os.WriteFile(file, []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	s.openExternalPathParent(file)
	if len(*got) != 1 || (*got)[0] != file || s.status != "" {
		t.Fatalf("reveals = %q status %q, want exactly %q", *got, s.status, file)
	}

	s.openExternalPathParent(filepath.Join(filepath.Dir(file), "gone.txt"))
	if len(*got) != 1 {
		t.Fatalf("a missing path reached Finder: %q", *got)
	}
	if s.status != "Could not reveal the file in Finder" {
		t.Fatalf("a missing path left the status %q", s.status)
	}
}

// desktopTool matches a program name handed to os/exec for something MyGo's
// Clipboard or Shell already does.
var desktopTool = regexp.MustCompile(`exec\.Command(Context)?\([^)]*"(pbcopy|pbpaste|wl-copy|xclip|xsel|clip)"`)

// The owner ladder says MyGo before a hand-rolled child process: a pbcopy
// child of a Finder-launched app has no UTF-8 locale and mangled every CJK
// copy. No production code spawns a clipboard tool again.
func TestNoClipboardToolIsSpawned(t *testing.T) {
	err := filepath.WalkDir("..", func(path string, d fs.DirEntry, err error) error {
		if err != nil || d.IsDir() || !strings.HasSuffix(path, ".go") || strings.HasSuffix(path, "_test.go") {
			return err
		}
		src, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		if loc := desktopTool.Find(src); loc != nil {
			t.Errorf("%s spawns %s: use mygo.Clipboard", path, loc)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
