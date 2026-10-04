package gitworkbench

import (
	"path/filepath"
	"strings"
	"sync"

	"github.com/alecthomas/chroma/v2"
	"github.com/alecthomas/chroma/v2/lexers"
	"github.com/alecthomas/chroma/v2/styles"
)

// Highlight dependency/license audit (GWB-120):
//   - dependency: github.com/alecthomas/chroma/v2 v2.27.0 (+ dlclark/regexp2 v2)
//   - license: MIT (both), no transitive copyleft, no network, no cgo;
//   - access stays behind the Highlighter interface so plain/binary/generated
//     files bypass lexing entirely.

// HighlightSpan is one styled rune-range of a diff line (presentation only;
// word-diff and highlight ranges compose at the UI layer).
type HighlightSpan struct {
	Start, End int // rune indices
	Color      string
	Bold       bool
	Italic     bool
}

// Highlighter converts one source line into styled spans.
type Highlighter interface {
	// HighlightLine styles one line of file content. The relative path picks
	// the lexer.
	HighlightLine(relPath, line string) []HighlightSpan
	// Supported reports whether a lexer exists for the path (callers skip
	// work cheaply for plain text).
	Supported(relPath string) bool
}

// HighlightBounds keep lexing bounded (plan §17).
const (
	MaxHighlightLineRunes  = 2000
	maxHighlightRegionByte = 2 << 20
)

// ChromaHighlighter is the Chroma-backed Highlighter with a style that the
// UI flips with the app theme. Safe for concurrent use.
type ChromaHighlighter struct {
	mu        sync.RWMutex
	styleName string
}

var _ Highlighter = (*ChromaHighlighter)(nil)

// NewChromaHighlighter builds the adapter; empty style defaults to the light
// "github" style. "github-dark" (or any Chroma style) is valid.
func NewChromaHighlighter(style string) *ChromaHighlighter {
	if style == "" {
		style = "github"
	}
	return &ChromaHighlighter{styleName: style}
}

// SetStyle switches the Chroma style when the app theme changes.
func (h *ChromaHighlighter) SetStyle(style string) {
	if style == "" {
		style = "github"
	}
	h.mu.Lock()
	h.styleName = style
	h.mu.Unlock()
}

// Supported reports whether Chroma has a lexer for the file name.
func (h *ChromaHighlighter) Supported(relPath string) bool {
	return lexers.Match(filepath.Base(relPath)) != nil
}

func (h *ChromaHighlighter) style() *chroma.Style {
	h.mu.RLock()
	name := h.styleName
	h.mu.RUnlock()
	if style := styles.Get(name); style != nil {
		return style
	}
	return styles.Fallback
}

// HighlightLine tokenizes one line and returns colored spans. Failures
// degrade to no spans (plain text), never errors.
func (h *ChromaHighlighter) HighlightLine(relPath, line string) []HighlightSpan {
	if line == "" || !h.Supported(relPath) {
		return nil
	}
	if runeLen(line) > MaxHighlightLineRunes {
		return nil
	}
	lexer := lexers.Match(filepath.Base(relPath))
	if lexer == nil {
		return nil
	}
	iterator, err := lexer.Tokenise(nil, line)
	if err != nil {
		return nil
	}
	style := h.style()
	var spans []HighlightSpan
	offset := 0
	for _, token := range iterator.Tokens() {
		runes := runeLen(token.String())
		entry := style.Get(token.Type)
		if entry.IsZero() {
			offset += runes
			continue
		}
		spans = append(spans, HighlightSpan{
			Start:  offset,
			End:    offset + runes,
			Color:  strings.TrimPrefix(entry.Colour.String(), "#"),
			Bold:   entry.Bold == chroma.Yes,
			Italic: entry.Italic == chroma.Yes,
		})
		offset += runes
	}
	return spans
}

// HighlightRegion lexes a bounded window of lines at once (one lexer pass
// per line is wasteful for expansion); per-line failure degrades to nil.
func (h *ChromaHighlighter) HighlightRegion(relPath string, lines []string) [][]HighlightSpan {
	result := make([][]HighlightSpan, len(lines))
	if !h.Supported(relPath) {
		return result
	}
	total := 0
	for _, l := range lines {
		total += len(l)
	}
	if total > maxHighlightRegionByte {
		return result
	}
	for i, l := range lines {
		result[i] = h.HighlightLine(relPath, l)
	}
	return result
}

func runeLen(s string) int {
	n := 0
	for range s {
		n++
	}
	return n
}
