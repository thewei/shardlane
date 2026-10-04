package app

import (
	"context"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/wh-studio/herdr-client/next/internal/settings"
)

func TestSettingsServiceLoadFailureKeepsDefaults(t *testing.T) {
	dir := t.TempDir()
	// Write a corrupt settings file; startup must survive with defaults.
	store := settings.NewFileStore(dir)
	if err := os.WriteFile(filepath.Join(dir, settings.FileName), []byte("{not json"), 0o600); err != nil {
		t.Fatal(err)
	}

	service := NewSettingsService(store)
	if service.LoadError() == nil {
		t.Fatal("expected a recorded load error for the corrupt file")
	}
	current := service.Current()
	if !reflect.DeepEqual(current, settings.Default()) {
		t.Fatalf("corrupt load = %+v, want defaults", current)
	}

	// A successful update over the corrupt start must still persist.
	updated, err := service.Update(context.Background(), func(v *settings.Settings) error {
		v.Terminal.FontSize = 14
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if updated.Terminal.FontSize != 14 {
		t.Fatalf("updated font size = %v", updated.Terminal.FontSize)
	}
}

func TestSettingsServiceUpdatePersistsAndRejectsInvalid(t *testing.T) {
	store := settings.NewMemoryStore(settings.Default())
	service := NewSettingsService(store)

	updated, err := service.Update(context.Background(), func(v *settings.Settings) error {
		v.General.Appearance = "dark"
		v.Terminal.Scrollback = 32 * 1024 * 1024
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if updated.General.Appearance != "dark" || updated.Terminal.Scrollback != 32*1024*1024 {
		t.Fatalf("update = %+v", updated)
	}
	if !reflect.DeepEqual(service.Current(), updated) {
		t.Fatal("Current did not publish the updated snapshot")
	}
	if persisted, err := store.Load(context.Background()); err != nil || persisted.General.Appearance != "dark" {
		t.Fatalf("store did not persist the update: %+v %v", persisted, err)
	}

	if _, err := service.Update(context.Background(), func(v *settings.Settings) error {
		v.Terminal.FontSize = 99
		return nil
	}); err == nil {
		t.Fatal("expected validation failure for font size 99")
	}
	if service.Current().Terminal.FontSize == 99 {
		t.Fatal("failed update must not change the current snapshot")
	}
}

func TestSettingsServiceSurvivesRestartOverFileStore(t *testing.T) {
	dir := t.TempDir()

	first := NewSettingsService(settings.NewFileStore(dir))
	if _, err := first.Update(context.Background(), func(v *settings.Settings) error {
		v.General.Appearance = "light"
		v.General.RestoreWindow = false
		v.Terminal.FontFamily = "Menlo"
		v.Terminal.OptionAsAlt = false
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	// A brand-new service over the same user-data directory, as after relaunch.
	second := NewSettingsService(settings.NewFileStore(dir))
	if second.LoadError() != nil {
		t.Fatalf("relaunch load error: %v", second.LoadError())
	}
	got := second.Current()
	want := first.Current()
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("relaunch settings = %+v, want %+v", got, want)
	}
	if got.General.Appearance != "light" || got.General.RestoreWindow || got.Terminal.FontFamily != "Menlo" || got.Terminal.OptionAsAlt {
		t.Fatalf("relaunched settings lost persisted values: %+v", got)
	}
}

func TestWindowStateKey(t *testing.T) {
	if got := WindowStateKey(true); got != "main" {
		t.Fatalf("WindowStateKey(true) = %q", got)
	}
	if got := WindowStateKey(false); got != "" {
		t.Fatalf("WindowStateKey(false) = %q", got)
	}
}

func TestValidateSettings(t *testing.T) {
	valid := settings.Default()
	if err := ValidateSettings(&valid); err != nil {
		t.Fatalf("defaults must validate: %v", err)
	}

	cases := []struct {
		name   string
		mutate func(*settings.Settings)
	}{
		{"unknown appearance", func(v *settings.Settings) { v.General.Appearance = "solarized" }},
		{"tiny font", func(v *settings.Settings) { v.Terminal.FontSize = 2 }},
		{"huge font", func(v *settings.Settings) { v.Terminal.FontSize = 64 }},
		{"line height below one", func(v *settings.Settings) { v.Terminal.LineHeight = 0.5 }},
		{"scrollback too small", func(v *settings.Settings) { v.Terminal.Scrollback = 1024 }},
		{"empty font family", func(v *settings.Settings) { v.Terminal.FontFamily = "  " }},
	}
	for _, tc := range cases {
		value := settings.Default()
		tc.mutate(&value)
		if err := ValidateSettings(&value); err == nil {
			t.Fatalf("%s: expected validation failure", tc.name)
		}
	}
}
