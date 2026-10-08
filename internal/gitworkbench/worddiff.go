package gitworkbench

import (
	"strings"
	"unicode"
	"unicode/utf8"
)

// Word diff bounds (plan §18): presentation-only ranges, never Git authority.
const (
	// MaxWordDiffRunes caps one input line's participation in word diffing.
	MaxWordDiffRunes = 2000
	// MaxWordDiffProduct caps tokensOld*tokensNew against pathological cost.
	MaxWordDiffProduct = 250000
	// MaxWordDiffRewriteRatio suppresses marks when lines are mostly
	// rewritten (noisy word marks on unrelated lines hurt review).
	MaxWordDiffRewriteRatio = 0.72
)

// WordRange is a rune-index [Start,End) span inside one line's text.
type WordRange struct {
	Start int
	End   int
}

// WordDiff pairs one delete line with its adjacent add line and returns the
// changed rune ranges on each side. ok is false when the pairing is unhelpful
// (length caps, pathological size, or a mostly-rewritten pair).
func WordDiff(oldLine, newLine string) (oldRanges, newRanges []WordRange, ok bool) {
	if utf8.RuneCountInString(oldLine) > MaxWordDiffRunes ||
		utf8.RuneCountInString(newLine) > MaxWordDiffRunes {
		return nil, nil, false
	}
	if oldLine == newLine {
		return nil, nil, false
	}

	oldTokens := tokenizeWords(oldLine)
	newTokens := tokenizeWords(newLine)
	if len(oldTokens)*len(newTokens) > MaxWordDiffProduct {
		return nil, nil, false
	}

	// LCS over token indices.
	match := lcsTable(oldTokens, newTokens)

	// Similarity gate: token similarity below the floor means the lines are
	// effectively rewritten; word marks would be noise. Whitespace-only
	// tokens never count toward similarity.
	shared := 0
	contentTokens := 0
	for i, tok := range oldTokens {
		if !isContentToken(tok) {
			continue
		}
		contentTokens++
		for j := range match[i] {
			if match[i][j] {
				shared++
				break
			}
		}
	}
	for _, tok := range newTokens {
		if isContentToken(tok) {
			contentTokens++
		}
	}
	if contentTokens == 0 || shared == 0 {
		return nil, nil, false
	}
	similarity := 2 * float64(shared) / float64(contentTokens)
	if similarity < 1-MaxWordDiffRewriteRatio {
		return nil, nil, false
	}

	// Matched flags per side, collected once.
	oldMatched := make([]bool, len(oldTokens))
	for i, row := range match {
		oldMatched[i] = anyTrue(row)
	}
	newMatched := make([]bool, len(newTokens))
	for j := range newTokens {
		for _, row := range match {
			if j < len(row) && row[j] {
				newMatched[j] = true
				break
			}
		}
	}

	oldRanges = changedRanges(oldTokens, oldMatched)
	newRanges = changedRanges(newTokens, newMatched)
	if len(oldRanges) == 0 && len(newRanges) == 0 {
		return nil, nil, false
	}
	return oldRanges, newRanges, true
}

// wordToken is one atomic word segment whose joined reconstruction equals
// the original line.
type wordToken struct {
	text string
}

// tokenizeWords splits a line into word-granular tokens:
//   - ASCII word runs [A-Za-z0-9_]+;
//   - each non-ASCII letter/digit rune individually (CJK granularity);
//   - single punctuation/symbol runes;
//   - whitespace runs.
//
// "return foo(bar)" → ["return ", "foo", "(", "bar", ")"].
func tokenizeWords(line string) []wordToken {
	var tokens []wordToken
	runes := []rune(line)
	i := 0
	segment := func(end int) {
		if end > i {
			tokens = append(tokens, wordToken{text: string(runes[i:end])})
			i = end
		}
	}
	for i < len(runes) {
		r := runes[i]
		switch {
		case r == ' ' || r == '\t':
			j := i
			for j < len(runes) && (runes[j] == ' ' || runes[j] == '\t') {
				j++
			}
			segment(j)
		case isASCIIWord(r):
			j := i
			for j < len(runes) && isASCIIWord(runes[j]) {
				j++
			}
			segment(j)
		case unicode.IsLetter(r) || unicode.IsDigit(r):
			// Non-ASCII letter/digit: per-rune granularity.
			segment(i + 1)
		default:
			// Punctuation/symbol: single rune, swallowing no whitespace.
			segment(i + 1)
		}
	}
	return tokens
}

func isASCIIWord(r rune) bool {
	return r == '_' || (r >= '0' && r <= '9') || (r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z')
}

// isContentToken reports whether the token carries non-whitespace content.
func isContentToken(tok wordToken) bool {
	return strings.TrimSpace(tok.text) != ""
}

// lcsTable builds the token match matrix with the classic LCS walk, keeping
// byte offsets so changed ranges map back to rune indices.
func lcsTable(oldTokens, newTokens []wordToken) [][]bool {
	match := make([][]bool, len(oldTokens))
	for i := range match {
		match[i] = make([]bool, len(newTokens))
	}
	// lengths[i][j] = LCS length of oldTokens[i:], newTokens[j:]
	lengths := make([][]int32, len(oldTokens)+1)
	for i := range lengths {
		lengths[i] = make([]int32, len(newTokens)+1)
	}
	for i := len(oldTokens) - 1; i >= 0; i-- {
		for j := len(newTokens) - 1; j >= 0; j-- {
			if oldTokens[i].text == newTokens[j].text {
				lengths[i][j] = lengths[i+1][j+1] + 1
			} else if lengths[i+1][j] >= lengths[i][j+1] {
				lengths[i][j] = lengths[i+1][j]
			} else {
				lengths[i][j] = lengths[i][j+1]
			}
		}
	}
	// Walk the LCS to mark matched pairs.
	i, j := 0, 0
	for i < len(oldTokens) && j < len(newTokens) {
		switch {
		case oldTokens[i].text == newTokens[j].text:
			match[i][j] = true
			i++
			j++
		case lengths[i+1][j] >= lengths[i][j+1]:
			i++
		default:
			j++
		}
	}
	return match
}

// changedRanges collects the rune spans of unmatched tokens on one side,
// merging contiguous spans.
func changedRanges(tokens []wordToken, matched []bool) []WordRange {
	var ranges []WordRange
	offset := 0
	for i, tok := range tokens {
		runes := utf8.RuneCountInString(tok.text)
		if !matched[i] && strings.TrimSpace(tok.text) != "" {
			ranges = appendMerge(ranges, WordRange{Start: offset, End: offset + runes})
		}
		offset += runes
	}
	return ranges
}

func anyTrue(row []bool) bool {
	for _, v := range row {
		if v {
			return true
		}
	}
	return false
}

func appendMerge(ranges []WordRange, r WordRange) []WordRange {
	if n := len(ranges); n > 0 && ranges[n-1].End >= r.Start {
		if r.End > ranges[n-1].End {
			ranges[n-1].End = r.End
		}
		return ranges
	}
	return append(ranges, r)
}
