//go:build !js

package e2e

import (
	"bytes"
	"image/png"
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/egoist/mygo"
)

// TestFileIcon draws the icons of an application, a folder and a file at
// the size asked for, and fails for a path that does not exist.
func TestFileIcon(t *testing.T) {
	dir := t.TempDir()
	file := filepath.Join(dir, "notes.txt")
	os.WriteFile(file, []byte("hello"), 0o644)
	paths := []string{dir, file}
	switch runtime.GOOS {
	case "darwin":
		paths = append(paths, "/System/Applications/Calculator.app")
	case "windows":
		paths = append(paths, filepath.Join(os.Getenv("SystemRoot"), "System32", "notepad.exe"))
	case "linux":
		// A desktop entry's icon is its application's, from the theme.
		entry := filepath.Join(dir, "editor.desktop")
		os.WriteFile(entry, []byte("[Desktop Entry]\nType=Application\nName=Editor\nExec=true\nIcon=folder\n"), 0o644)
		// One whose icon the theme lacks, as Byobu's on Ubuntu, gets the
		// icon of desktop entries.
		missing := filepath.Join(dir, "unknown.desktop")
		os.WriteFile(missing, []byte("[Desktop Entry]\nType=Application\nName=Unknown\nExec=true\nIcon=mygo-no-such-icon\n"), 0o644)
		paths = append(paths, entry, missing)
	}
	for _, p := range paths {
		for _, size := range []int{16, 64} {
			data, err := mygo.Shell.FileIcon(p, size)
			if err != nil {
				t.Errorf("FileIcon(%q, %d): %v", p, size, err)
				continue
			}
			img, err := png.Decode(bytes.NewReader(data))
			if err != nil {
				t.Errorf("FileIcon(%q, %d): not a PNG: %v", p, size, err)
				continue
			}
			if b := img.Bounds(); b.Dx() != size || b.Dy() != size {
				t.Errorf("FileIcon(%q, %d) is %dx%d", p, size, b.Dx(), b.Dy())
			}
			opaque := 0
			for y := 0; y < size; y++ {
				for x := 0; x < size; x++ {
					if _, _, _, a := img.At(x, y).RGBA(); a > 0 {
						opaque++
					}
				}
			}
			if opaque < size*size/8 {
				t.Errorf("FileIcon(%q, %d) draws %d of %d pixels", p, size, opaque, size*size)
			}
		}
	}
	if runtime.GOOS == "linux" {
		// The entry above names the icon of folders.
		entry, _ := mygo.Shell.FileIcon(filepath.Join(dir, "editor.desktop"), 32)
		folder, _ := mygo.Shell.FileIcon(dir, 32)
		if !bytes.Equal(entry, folder) {
			t.Error("a desktop entry's icon is not the one it names")
		}
	}
	if _, err := mygo.Shell.FileIcon(filepath.Join(dir, "missing"), 32); err == nil {
		t.Error("FileIcon of a missing file did not fail")
	}
}
