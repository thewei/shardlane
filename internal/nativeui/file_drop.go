package nativeui

import (
	"errors"
	"fmt"
	"runtime"
	"sort"
	"strings"
	"unicode"
)

// P13 File Drop bounds (0.9 P13 / WIX-180..185).
const (
	MaxFileDropPaths = 256
	MaxFileDropBytes = 64 * 1024 // 64 KiB
)

var (
	ErrTooManyPaths            = errors.New("file drop exceeds maximum 256 paths limit")
	ErrGeneratedTextTooBig     = errors.New("generated drop text exceeds 64 KiB limit")
	ErrControlCharacters       = errors.New("paths contain forbidden control characters (newlines/tabs)")
	ErrWindowsShellUnsupported = errors.New("file drop paste is unavailable on Windows until shell-specific quoting is supported")
)

// SafeShellQuote encodes dropped file paths into safe shell arguments (P13).
// Invariants:
// - paths only, never file contents
// - max 256 paths
// - max 64 KiB total output
// - rejects any control characters (< 0x20 or newlines) to prevent execution injection
// - never appends Enter or newline
// - POSIX shell quoting: wraps paths containing spaces/metacharacters in single quotes safely
// - Windows is gated off until a shell-specific quoting adapter is proven (P1-11)
func SafeShellQuote(paths []string) (string, error) {
	if runtime.GOOS == "windows" {
		return "", ErrWindowsShellUnsupported
	}
	if len(paths) > MaxFileDropPaths {
		return "", ErrTooManyPaths
	}
	if len(paths) == 0 {
		return "", nil
	}

	quoted := make([]string, 0, len(paths))
	totalLen := 0

	for _, p := range paths {
		// Verify no control characters exist anywhere in the path
		for _, r := range p {
			if unicode.IsControl(r) || r < 32 {
				return "", fmt.Errorf("%w: path %q has byte %d", ErrControlCharacters, p, r)
			}
		}

		q := quoteSingle(p)
		totalLen += len(q) + 1
		if totalLen > MaxFileDropBytes {
			return "", ErrGeneratedTextTooBig
		}
		quoted = append(quoted, q)
	}

	return strings.Join(quoted, " "), nil
}

// quoteSingle wraps a path in single quotes if it contains spaces or shell metacharacters.
// In single quotes, the only character needing special handling is the single quote itself,
// which is safely escaped as `'\”`.
func quoteSingle(s string) string {
	if s == "" {
		return "''"
	}

	needsQuotes := false
	for _, r := range s {
		switch r {
		case ' ', '\t', '\n', '$', '`', '"', '\\', '&', ';', '|', '*', '?', '~', '<', '>', '(', ')', '[', ']', '{', '}', '^', '#', '!':
			needsQuotes = true
		}
	}

	if !needsQuotes && !strings.ContainsRune(s, '\'') {
		return s
	}

	// Replace ' with '\''
	return "'" + strings.ReplaceAll(s, "'", `'\''`) + "'"
}

// HandleFileDrop safely converts dropped paths into quoted text and pastes
// them into the target pane terminal without trailing newline execution
// (GWB-008/009). Drops only reach terminals while the Terminal surface is
// the visible primary surface — never a hidden one.
func (s *Shell) HandleFileDrop(paths []string, targetPaneID string) error {
	quotedText, err := SafeShellQuote(paths)
	if err != nil {
		s.status = "File drop rejected: " + err.Error()
		return err
	}
	if quotedText == "" {
		return nil
	}
	if s.surface.current() != WorkspaceSurfaceTerminal {
		s.status = "File drop ignored: Terminal is not the visible surface"
		return nil
	}

	pane := s.selectedPane()
	if targetPaneID != "" {
		pane = paneByID(s.projection, targetPaneID)
	}
	if pane == nil || pane.TerminalID == "" {
		s.status = "File drop: no target terminal pane available"
		return errors.New("no target terminal available")
	}
	surface := s.terminals[pane.TerminalID]
	if surface == nil {
		s.status = "File drop: terminal not attached"
		return errors.New("terminal not attached")
	}

	surface.term.Paste(quotedText)
	s.status = fmt.Sprintf("Dropped %d path%s to terminal", len(paths), pluralS(len(paths)))
	return nil
}

// HandleFileDropAt resolves the visible terminal under the drop point
// (window CSS pixels) and pastes there (GWB-009 exact-hit-target rule).
// Fails closed if no visible terminal is under the pointer (P1-10).
func (s *Shell) HandleFileDropAt(x, y int, paths []string) error {
	if s.surface.current() != WorkspaceSurfaceTerminal {
		s.status = "File drop ignored: Terminal is not the visible surface"
		return errors.New("terminal is not the visible surface")
	}
	if terminalID, ok := s.terminalAtPoint(float32(x), float32(y)); ok {
		if surface := s.terminals[terminalID]; surface != nil {
			quoted, err := SafeShellQuote(paths)
			if err != nil {
				s.status = "File drop rejected: " + err.Error()
				return err
			}
			if quoted == "" {
				return nil
			}
			surface.term.Paste(quoted)
			s.status = fmt.Sprintf("Dropped %d path%s to terminal", len(paths), pluralS(len(paths)))
			return nil
		}
	}
	// Exact-hit rule: fail closed when no terminal is under pointer (do not guess/fallback).
	s.status = "File drop ignored: no terminal under pointer"
	return errors.New("no terminal under pointer")
}

// terminalAtPoint maps window CSS pixels onto the visible pane geometry
// using the canvas bounds captured at render time. Uses half-open rects
// and deterministic ordering (P1-10).
func (s *Shell) terminalAtPoint(x, y float32) (string, bool) {
	b := s.terminalCanvasBounds
	if b.W <= 0 || b.H <= 0 {
		return "", false
	}
	if x < b.X || y < b.Y || x >= b.X+b.W || y >= b.Y+b.H {
		return "", false
	}

	// Deterministic sorting of keys so shared boundaries tie-break consistently.
	keys := make([]string, 0, len(s.terminals))
	for k := range s.terminals {
		keys = append(keys, k)
	}
	sort.Strings(keys)

	for _, key := range keys {
		surface := s.terminals[key]
		left, top, width, height := surfacePercent(surface.area, surface.rect)
		px := b.X + left/100*b.W
		py := b.Y + top/100*b.H
		pw := width / 100 * b.W
		ph := height / 100 * b.H
		if x >= px && y >= py && x < px+pw && y < py+ph {
			return key, true
		}
	}
	return "", false
}
