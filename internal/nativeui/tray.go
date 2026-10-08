package nativeui

import (
	"bytes"
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"log/slog"

	"github.com/egoist/mygo"
)

// trayPlatform is the MyGo tray seam (0.7 §17): production binds the real
// mygo.Tray; deterministic tests record applied projections.
type trayPlatform interface {
	SetTitle(title string)
	SetToolTip(tip string)
}

// statusTray owns the macOS menu-bar projection (0.7 §17-18): the shared
// StatusCenterSnapshot projects onto title/tooltip only when the tray
// fingerprint changes, so idle frames and navigation selection never rebuild
// the platform tray.
type statusTray struct {
	platform trayPlatform
	// applied reports whether any projection reached the platform; the
	// first apply always writes.
	applied     bool
	fingerprint string
}

func newStatusTray(platform trayPlatform) *statusTray {
	return &statusTray{platform: platform}
}

// apply projects one snapshot onto the platform (0.7 §18): writes happen
// only when the tray fingerprint changed since the last apply.
func (t *statusTray) apply(snapshot StatusCenterSnapshot, connected bool) {
	fingerprint := TrayFingerprint(snapshot, connected)
	if t.applied && fingerprint == t.fingerprint {
		return
	}
	t.fingerprint = fingerprint
	t.applied = true
	counts := TrayCountsFromSnapshot(snapshot)
	t.platform.SetTitle(TrayTitle(counts))
	t.platform.SetToolTip(TrayToolTip(counts, connected))
}

// AttachTray binds the macOS menu-bar tray (0.7 §17): the normal click
// toggles the §17.1 floating Status Quick Panel anchored beneath the
// menu-bar item; without an attached panel the in-window Status Center
// stays the surface. The click routes as an action — never a direct
// runtime mutation.
func (s *Shell) AttachTray(tray *mygo.Tray) {
	s.tray = newStatusTray(tray)
	// The status-first title is empty when nothing is actionable; the
	// template glyph is the always-visible menu-bar mark, so bind it once
	// at attach (0.7 §17).
	if png, err := TrayIconPNG(); err != nil {
		slog.Warn("render tray icon failed", "error", err)
	} else if err := tray.SetIcon(png, true); err != nil {
		slog.Warn("set tray icon failed", "error", err)
	}
	tray.OnClick(func() {
		s.applyOnWindow(func() {
			s.ToggleQuickPanel(tray.Bounds())
		})
	})
	s.updateTray()
}

// updateTray projects the current status-center snapshot onto the menu bar
// (0.7 §17-18). No-op when no tray is bound: headless sessions, tests, and
// platforms where tray creation failed keep the in-window Status Center as
// the only surface.
func (s *Shell) updateTray() {
	if s.tray == nil {
		return
	}
	s.tray.apply(s.BuildStatusCenterSnapshot(), !s.offline)
}

// TrayIconPNG renders the menu-bar template glyph (0.7 §17): a small black
// terminal-prompt mark on transparent background so macOS tints it against
// the menu bar.
func TrayIconPNG() ([]byte, error) {
	const size = 32
	bounds := image.Rect(0, 0, size, size)
	canvas := image.NewRGBA(bounds)
	draw.Draw(canvas, bounds, image.Transparent, image.Point{}, draw.Src)
	black := color.RGBA{R: 0, G: 0, B: 0, A: 255}
	// Terminal window outline: rounded feel via a 2px inset border.
	border := image.Rect(3, 7, size-3, size-7)
	draw.Draw(canvas, border, &image.Uniform{black}, image.Point{}, draw.Over)
	inner := image.Rect(5, 9, size-5, size-9)
	draw.Draw(canvas, inner, image.Transparent, image.Point{}, draw.Src)
	draw.Draw(canvas, image.Rect(3, 7, size-3, 12), &image.Uniform{black}, image.Point{}, draw.Over)
	// Prompt chevron plus cursor line.
	for y := 16; y < 22; y++ {
		canvas.Set(8+(y-16), y, black)
		canvas.Set(12-(y-16), y, black)
	}
	draw.Draw(canvas, image.Rect(16, 16, 24, 18), &image.Uniform{black}, image.Point{}, draw.Over)
	var buf bytes.Buffer
	if err := png.Encode(&buf, canvas); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}
