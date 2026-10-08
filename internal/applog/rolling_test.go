package applog

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRollingFileBoundsDiskUsage(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "app.log")
	writer, err := NewRollingFile(path, 64, 2)
	if err != nil {
		t.Fatal(err)
	}
	defer writer.Close()
	for index := 0; index < 12; index++ {
		if _, err := writer.Write([]byte(strings.Repeat("x", 20) + "\n")); err != nil {
			t.Fatal(err)
		}
	}
	var total int64
	for _, suffix := range []string{"", ".1", ".2"} {
		info, err := os.Stat(path + suffix)
		if err != nil {
			if os.IsNotExist(err) {
				continue
			}
			t.Fatal(err)
		}
		if info.Size() > 64 {
			t.Fatalf("%s size=%d > 64", suffix, info.Size())
		}
		total += info.Size()
	}
	if total > 64*3 {
		t.Fatalf("total log bytes=%d > %d", total, 64*3)
	}
}

func TestRollingFileTruncatesPathologicalWrite(t *testing.T) {
	path := filepath.Join(t.TempDir(), "app.log")
	writer, err := NewRollingFile(path, 32, 1)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := writer.Write([]byte(strings.Repeat("a", 100))); err != nil {
		t.Fatal(err)
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if info.Size() > 32 {
		t.Fatalf("size=%d > 32", info.Size())
	}
}
