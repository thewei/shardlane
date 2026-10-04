// Package app coordinates Shardlane's UI-independent application services.
// Services own product semantics; presentation consumes them and Herdr stays
// the sole runtime authority.
package app

import (
	"context"
	"fmt"
	"log/slog"
	"strings"
	"sync"

	"github.com/wh-studio/herdr-client/next/internal/settings"
)

// SettingsService loads client preferences once, serializes updates, and
// persists them through the configured store. A corrupt or unreadable store
// never prevents startup: the service keeps defaults and records the load
// error for diagnostics.
type SettingsService struct {
	mu      sync.Mutex
	store   settings.Store
	value   settings.Settings
	loadErr error
}

func NewSettingsService(store settings.Store) *SettingsService {
	value, err := store.Load(context.Background())
	if err != nil {
		slog.Warn("load settings failed; using defaults", "error", err)
		if value.SchemaVersion == 0 {
			value = settings.Default()
		}
	}
	return &SettingsService{store: store, value: value, loadErr: err}
}

// LoadError reports the startup load failure, if any. It is diagnostic only:
// defaults are already active.
func (s *SettingsService) LoadError() error {
	return s.loadErr
}

func (s *SettingsService) Current() settings.Settings {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.value
}

// Update applies mutate to a copy of the current settings, validates the
// result, persists it, and publishes the new snapshot. A failed validation or
// save leaves the current snapshot unchanged.
func (s *SettingsService) Update(ctx context.Context, mutate func(*settings.Settings) error) (settings.Settings, error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	next := s.value
	if mutate == nil {
		return s.value, nil
	}
	if err := mutate(&next); err != nil {
		return s.value, fmt.Errorf("apply settings update: %w", err)
	}
	if err := ValidateSettings(&next); err != nil {
		return s.value, fmt.Errorf("validate settings update: %w", err)
	}
	if err := s.store.Save(ctx, next); err != nil {
		return s.value, fmt.Errorf("persist settings update: %w", err)
	}
	s.value = next
	return s.value, nil
}

// WindowStateKey returns the MyGo window-state key used for launch
// restoration, or the empty string when the user disabled restoration. The
// placement safety itself (display intersection, center fallback) is owned by
// MyGo's window-state persistence.
func WindowStateKey(restoreWindow bool) string {
	if !restoreWindow {
		return ""
	}
	return "main"
}

// ValidateSettings enforces the presentation-safe ranges for the 0.3.0
// settings surface. It never mutates the input.
func ValidateSettings(value *settings.Settings) error {
	switch value.General.Appearance {
	case "system", "light", "dark":
	default:
		return fmt.Errorf("unknown appearance %q", value.General.Appearance)
	}
	if value.Terminal.FontSize < 6 || value.Terminal.FontSize > 32 {
		return fmt.Errorf("terminal font size %g outside 6..32", value.Terminal.FontSize)
	}
	if value.Terminal.LineHeight < 1.0 || value.Terminal.LineHeight > 2.5 {
		return fmt.Errorf("terminal line height %g outside 1.0..2.5", value.Terminal.LineHeight)
	}
	if value.Terminal.Scrollback < 1*1024*1024 || value.Terminal.Scrollback > 1024*1024*1024 {
		return fmt.Errorf("terminal scrollback %d outside 1 MiB..1 GiB", value.Terminal.Scrollback)
	}
	if strings.TrimSpace(value.Terminal.FontFamily) == "" {
		return fmt.Errorf("terminal font family is empty")
	}
	return nil
}
