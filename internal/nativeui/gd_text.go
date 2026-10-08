package nativeui

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/wh-studio/herdr-client/internal/codehl"
	"github.com/wh-studio/herdr-client/internal/gitworkbench"
)

// Text styling helpers of the Godiff-style review, ported from egoist/godiff
// spans.go and sidebar.go helpers (commit 88b89e0).

// gdTabWidth is the columns a tab takes.
const gdTabWidth = 4

// gdMark is a range of a line drawn with a background: a changed word or a
// match of the find bar.
type gdMark struct {
	start, end int // byte offsets into the line
	color      ui.Color
}

// gdCodeSpans styles a line of code: the colors of its tokens, and the
// backgrounds of its marks, later marks over earlier ones. Tabs become
// spaces up to the next tab stop.
func gdCodeSpans(text string, segs []codehl.Seg, marks []gdMark, pal *gdPalette) []ui.Span {
	if text == "" {
		return []ui.Span{{Text: " "}}
	}
	// The places where the style may change.
	cuts := []int{0, len(text)}
	for _, s := range segs {
		cuts = append(cuts, int(s.Start), int(s.End))
	}
	for _, m := range marks {
		cuts = append(cuts, m.start, m.end)
	}
	slices.Sort(cuts)
	cuts = slices.Compact(cuts)

	var spans []ui.Span
	col := 0
	si := 0
	for i := 0; i+1 < len(cuts); i++ {
		start, end := cuts[i], cuts[i+1]
		if start >= len(text) || start < 0 {
			break
		}
		end = min(end, len(text))
		if end <= start {
			continue
		}
		for si < len(segs) && int(segs[si].End) <= start {
			si++
		}
		color := pal.code
		if si < len(segs) && int(segs[si].Start) <= start {
			color = pal.syntax[segs[si].Class]
		}
		var bg ui.Color
		for _, m := range marks {
			if m.start <= start && end <= m.end {
				bg = m.color
			}
		}
		part := text[start:end]
		if strings.IndexByte(part, '\t') >= 0 {
			part, col = gdExpandTabs(part, col)
		} else {
			col += len(part)
		}
		if n := len(spans); n > 0 && spans[n-1].Color == color && spans[n-1].Background == bg {
			spans[n-1].Text += part
			continue
		}
		spans = append(spans, ui.Span{Text: part, Color: color, Background: bg})
	}
	return spans
}

// gdExpandTabs replaces the tabs of s, which starts at column col, and
// returns the column after it.
func gdExpandTabs(s string, col int) (string, int) {
	var b strings.Builder
	for _, r := range s {
		if r == '\t' {
			n := gdTabWidth - col%gdTabWidth
			b.WriteString(strings.Repeat(" ", n))
			col += n
			continue
		}
		b.WriteRune(r)
		col++
	}
	return b.String(), col
}

// gdColumns returns how many columns a line takes, tabs expanded.
func gdColumns(s string) int {
	col := 0
	for _, r := range s {
		if r == '\t' {
			col += gdTabWidth - col%gdTabWidth
		} else {
			col++
		}
	}
	return col
}

// gdCompact writes a count briefly: 999, 1.2k, 15k, 1.2m.
func gdCompact(n int) string {
	switch {
	case n < 1000:
		return fmt.Sprint(n)
	case n < 10000:
		return strings.TrimSuffix(fmt.Sprintf("%.1f", float64(n)/1000), ".0") + "k"
	case n < 1000000:
		return fmt.Sprintf("%dk", (n+500)/1000)
	}
	return strings.TrimSuffix(fmt.Sprintf("%.1f", float64(n)/1000000), ".0") + "m"
}

// gdThousands writes a count with separators: 1,234.
func gdThousands(n int) string {
	s := fmt.Sprint(n)
	for i := len(s) - 3; i > 0; i -= 3 {
		s = s[:i] + "," + s[i:]
	}
	return s
}

func gdLines(n int, what string) string {
	if n == 1 {
		return "1 " + what + " line"
	}
	return gdThousands(n) + " " + what + " lines"
}

// gdFuzzyMatch reports whether the runes of query appear in s in order.
func gdFuzzyMatch(s, query string) bool {
	if query == "" {
		return true
	}
	s, query = strings.ToLower(s), strings.ToLower(query)
	i := 0
	for _, r := range s {
		if i < len(query) && r == rune(query[i]) {
			i++
		}
	}
	return i >= len(query)
}

// gdAbbreviateHome writes the home directory as ~.
func gdAbbreviateHome(p string) string {
	home, err := os.UserHomeDir()
	if err == nil && (p == home || strings.HasPrefix(p, home+string(filepath.Separator))) {
		return "~" + p[len(home):]
	}
	return p
}

// gdCountable reports whether a file's lines were counted.
func gdCountable(cf *gitworkbench.ChangeFile) bool {
	return !cf.Binary && !cf.TooLarge
}

// gdWordMarks converts rune-index word-diff ranges into byte-offset marks.
func gdWordMarks(text string, ranges []gitworkbench.WordRange, color ui.Color) []gdMark {
	if len(ranges) == 0 {
		return nil
	}
	var marks []gdMark
	for _, r := range ranges {
		start, end := gdByteOffset(text, r.Start), gdByteOffset(text, r.End)
		if end > start {
			marks = append(marks, gdMark{start: start, end: end, color: color})
		}
	}
	return marks
}

// itoa writes a small integer (shared by the review and other surfaces).
func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	digits := ""
	neg := n < 0
	if neg {
		n = -n
	}
	for n > 0 {
		digits = string(rune('0'+n%10)) + digits
		n /= 10
	}
	if neg {
		return "-" + digits
	}
	return digits
}

// statusTone maps a change status to the shared status tone (Commit list).
func statusTone(status gitworkbench.ChangeStatus) StatusTone {
	switch status {
	case gitworkbench.StatusAdded:
		return ToneSuccess
	case gitworkbench.StatusDeleted:
		return ToneError
	case gitworkbench.StatusRenamed, gitworkbench.StatusCopied:
		return ToneInfo
	case gitworkbench.StatusConflicted:
		return ToneAttention
	case gitworkbench.StatusUntracked:
		return ToneWorking
	default:
		return ToneWarning
	}
}

// gdPair is a deleted line and the added line that replaces it, by their
// indices in a hunk's lines.
type gdPair struct{ del, add int }

// gdPairs pairs the deleted lines of each run of changes with the added
// lines that follow them, in order, as a split view shows them side by side.
func gdPairs(lines []gitworkbench.DiffLine) []gdPair {
	var pairs []gdPair
	for i := 0; i < len(lines); {
		if lines[i].Kind != gitworkbench.KindDelete {
			i++
			continue
		}
		start := i
		for i < len(lines) && lines[i].Kind == gitworkbench.KindDelete {
			i++
		}
		dels := i - start
		addStart := i
		for i < len(lines) && lines[i].Kind == gitworkbench.KindAdd {
			i++
		}
		adds := i - addStart
		for k := range min(dels, adds) {
			pairs = append(pairs, gdPair{del: start + k, add: addStart + k})
		}
	}
	return pairs
}

func gdByteOffset(s string, runeIndex int) int {
	n := 0
	i := 0
	for i < len(s) && n < runeIndex {
		_, size := utf8.DecodeRuneInString(s[i:])
		i += size
		n++
	}
	return i
}
