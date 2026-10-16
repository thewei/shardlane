//go:build !js

package mygo

import (
	"bytes"
	"image/png"
	"path/filepath"
	"slices"
	"testing"

	"github.com/egoist/mygo/internal/fake"
	"github.com/egoist/mygo/internal/platform"
)

func TestFileIcon(t *testing.T) {
	dir := t.TempDir()
	data, err := Shell.FileIcon(dir, 24)
	if err != nil {
		t.Fatal(err)
	}
	if img, err := png.Decode(bytes.NewReader(data)); err != nil || img.Bounds().Dx() != 24 {
		t.Errorf("FileIcon: %v, %v", img, err)
	}
	if _, err := Shell.FileIcon(dir, 0); err == nil {
		t.Error("FileIcon of size 0 did not fail")
	}
	if _, err := Shell.FileIcon(filepath.Join(dir, "missing"), 16); err == nil {
		t.Error("FileIcon of a missing file did not fail")
	}
}

func TestInstalledApps(t *testing.T) {
	fake.Apps = []platform.Application{
		{Name: "zed", ID: "dev.zed.Zed", Path: "/Applications/Zed.app"},
		{Name: "Calculator", ID: "com.apple.calculator", Path: "/System/Applications/Calculator.app"},
		{Name: "Arc", ID: "company.thebrowser.Browser", Path: "/Applications/Arc.app"},
	}
	defer func() { fake.Apps, fake.Opened = nil, nil }()
	apps, err := Shell.InstalledApps()
	if err != nil {
		t.Fatal(err)
	}
	var names []string
	for _, a := range apps {
		names = append(names, a.Name)
	}
	if want := []string{"Arc", "Calculator", "zed"}; !slices.Equal(names, want) {
		t.Errorf("InstalledApps: %v, want %v", names, want)
	}
	if err := Shell.LaunchApp(apps[1]); err != nil || !slices.Equal(fake.Opened, []string{"/System/Applications/Calculator.app"}) {
		t.Errorf("LaunchApp: %v, opened %v", err, fake.Opened)
	}
	if err := Shell.LaunchApp(InstalledApp{Name: "nothing"}); err == nil {
		t.Error("LaunchApp of an app without a path did not fail")
	}
}
