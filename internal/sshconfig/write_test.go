package sshconfig

import (
	"os"
	"path/filepath"
	"testing"
)

func TestSaveFileReplacesBacksUpAndKeepsMode(t *testing.T) {
	path := filepath.Join(t.TempDir(), "config")
	write(t, path, "Host old\n")
	if err := os.Chmod(path, 0o640); err != nil {
		t.Fatal(err)
	}

	if err := SaveFile(path, "Host new\n  Port 2222\n"); err != nil {
		t.Fatal(err)
	}

	data, err := os.ReadFile(path)
	if err != nil || string(data) != "Host new\n  Port 2222\n" {
		t.Fatalf("content = %q, err = %v", data, err)
	}
	backup, err := os.ReadFile(path + ".bak")
	if err != nil || string(backup) != "Host old\n" {
		t.Fatalf("backup = %q, err = %v", backup, err)
	}
	fi, err := os.Stat(path)
	if err != nil || fi.Mode().Perm() != 0o640 {
		t.Fatalf("mode = %v, err = %v", fi.Mode(), err)
	}
}

func TestSaveFileFollowsSymlink(t *testing.T) {
	dir := t.TempDir()
	real := filepath.Join(dir, "real.conf")
	link := filepath.Join(dir, "link.conf")
	write(t, real, "Host old\n")
	if err := os.Symlink(real, link); err != nil {
		t.Fatal(err)
	}

	if err := SaveFile(link, "Host new\n"); err != nil {
		t.Fatal(err)
	}

	// The link must survive as a link and the edit must land on the target.
	fi, err := os.Lstat(link)
	if err != nil || fi.Mode()&os.ModeSymlink == 0 {
		t.Fatalf("link replaced by regular file: %v %v", fi.Mode(), err)
	}
	data, err := os.ReadFile(real)
	if err != nil || string(data) != "Host new\n" {
		t.Fatalf("target content = %q, err = %v", data, err)
	}
	backup, err := os.ReadFile(real + ".bak")
	if err != nil || string(backup) != "Host old\n" {
		t.Fatalf("backup = %q, err = %v", backup, err)
	}
}

func TestSaveFileCreatesNewConfig(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "new", "config")

	if err := SaveFile(path, "# fresh\n"); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(path)
	if err != nil || string(data) != "# fresh\n" {
		t.Fatalf("content = %q, err = %v", data, err)
	}
	fi, err := os.Stat(path)
	if err != nil || fi.Mode().Perm() != 0o600 {
		t.Fatalf("mode = %v, err = %v", fi.Mode(), err)
	}
}
